//! 按 MIME 结构获取正文和日程，避免把图片、压缩包等附件下载到常驻进程。
use super::{Account,Item,Parsed,parse_message,body_of,code_of,links_of,meeting_of};
use imap_proto::{BodyStructure,MessageSection,SectionPath};
use std::io::{Read,Write};

fn parts(body:&BodyStructure<'_>,path:&mut Vec<u32>,out:&mut Vec<Vec<u32>>){
    match body {
        BodyStructure::Multipart{bodies,..}=>for (i,body) in bodies.iter().enumerate(){
            path.push(i as u32+1);parts(body,path,out);path.pop();
        },
        BodyStructure::Text{common,..}|BodyStructure::Basic{common,..}=>{
            let ty=&common.ty;
            if ty.ty.eq_ignore_ascii_case("text") && ["plain","html","calendar"].iter().any(|sub|ty.subtype.eq_ignore_ascii_case(sub))
                || ty.ty.eq_ignore_ascii_case("application") && ty.subtype.eq_ignore_ascii_case("ics") {
                out.push(if path.is_empty(){vec![1]}else{path.clone()});
            }
        },
        BodyStructure::Message{..}=>{},
    }
}

fn make_item(parsed:Parsed,fetch:&imap::types::Fetch,account:&Account)->Item{
    Item {received_unix: fetch.internal_date().map(|d|d.timestamp()), account:account.email.clone(),
        from:if parsed.from.is_empty(){account.email.clone()}else{parsed.from.clone()},
        subject:if parsed.subject.is_empty(){"（无主题）".into()}else{parsed.subject.clone()},
        time:fetch.internal_date().map(|d|d.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string()).unwrap_or_default(),
        code:code_of(&parsed),summary:body_of(&parsed).into(),html:parsed.html.clone().into(),links:links_of(&parsed),meeting:meeting_of(&parsed.calendar),uid:fetch.uid.unwrap_or(0)}
}

/// 服务器明确拒绝了这条命令（NO / BAD）：连接仍然同步，可以换一种方式读这封信。
/// 其他错误（超时、解析失败、连接中断）说明响应没读完，这条连接已经不能再用，
/// 必须交给上层断开重连；继续发命令会读到上一条命令残留的响应。
fn rejected(error:&imap::Error)->bool{matches!(error,imap::Error::No(_)|imap::Error::Bad(_))}

// 部分旧服务器没有 BODYSTRUCTURE 或 MIME 分段支持，继续使用原有完整读取路径。
fn legacy<S:Read+Write>(session:&mut imap::Session<S>,uid:u32,account:&Account)->Result<Option<Item>,()>{
    let fetched=match session.uid_fetch(uid.to_string(),"(UID INTERNALDATE BODY.PEEK[HEADER] BODY.PEEK[TEXT])"){
        Ok(fetched)=>fetched,Err(error) if rejected(&error)=>return Ok(None),Err(_)=>return Err(()),
    };
    let Some(fetch)=fetched.iter().find(|f|f.uid==Some(uid))else{return Ok(None)};
    let Some(header)=fetch.header() else{return Ok(None)};
    let mut raw=header.to_vec();raw.extend_from_slice(b"\r\n");
    if let Some(text)=fetch.text(){raw.extend_from_slice(text);}
    Ok(Some(make_item(parse_message(&raw),fetch,account)))
}

pub(crate) fn item<S:Read+Write>(session:&mut imap::Session<S>,uid:u32,account:&Account)->Result<Option<Item>,()>{
    let fetched=match session.uid_fetch(uid.to_string(),"(UID INTERNALDATE BODYSTRUCTURE BODY.PEEK[HEADER])"){
        Ok(fetched)=>fetched,Err(error) if rejected(&error)=>return legacy(session,uid,account),Err(_)=>return Err(()),
    };
    let Some(fetch)=fetched.iter().find(|f|f.uid==Some(uid))else{return Ok(None)};
    let Some(body)=fetch.bodystructure()else{return legacy(session,uid,account)};
    let mut selected=Vec::new();parts(body,&mut Vec::new(),&mut selected);
    let Some(header)=fetch.header() else{return legacy(session,uid,account)};
    let mut parsed=parse_message(header);
    for path in selected {
        let section=path.iter().map(u32::to_string).collect::<Vec<_>>().join(".");
        let result=session.uid_fetch(uid.to_string(),format!("(UID BODY.PEEK[{section}.MIME] BODY.PEEK[{section}])"));
        let part=match result{Ok(part)=>part,Err(error) if rejected(&error)=>return legacy(session,uid,account),Err(_)=>return Err(())};
        let data=part.iter().find(|f|f.uid==Some(uid)).and_then(|f|Some((
            f.section(&SectionPath::Part(path.clone(),Some(MessageSection::Mime)))?,
            f.section(&SectionPath::Part(path,None))?,
        )));
        let Some((header,body))=data else{return legacy(session,uid,account)};
        let mut raw=Vec::with_capacity(header.len()+body.len()+2);raw.extend_from_slice(header);
        if !header.ends_with(b"\r\n\r\n"){raw.extend_from_slice(b"\r\n");}
        raw.extend_from_slice(body);
        let part=parse_message(&raw);
        parsed.text.push_str(&part.text);parsed.html.push_str(&part.html);parsed.calendar.push_str(&part.calendar);
        parsed.emphasized.extend(part.emphasized);
    }
    if !parsed.text.trim().is_empty(){parsed.emphasized.clear();}
    Ok(Some(make_item(parsed,fetch,account)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor,sync::{Arc,Mutex}};
    #[derive(Debug)]
    struct Wire{input:Cursor<Vec<u8>>,output:Arc<Mutex<Vec<u8>>>}
    impl Read for Wire{fn read(&mut self,buf:&mut [u8])->std::io::Result<usize>{self.input.read(buf)}}
    impl Write for Wire{
        fn write(&mut self,buf:&[u8])->std::io::Result<usize>{self.output.lock().unwrap().extend_from_slice(buf);Ok(buf.len())}
        fn flush(&mut self)->std::io::Result<()>{Ok(())}
    }
    fn run(reply:String)->(Item,String){
        let output=Arc::new(Mutex::new(Vec::new()));
        let wire=Wire{input:Cursor::new(format!("a1 OK login\r\n{reply}").into_bytes()),output:output.clone()};
        let mut session=imap::Client::new(wire).login("test","test").unwrap();
        let account=Account {custom: None, email:"test@example.com".into(),provider:super::super::Provider::Qq,enabled:true};
        let item=item(&mut session,42,&account).unwrap().unwrap();
        let commands=String::from_utf8(output.lock().unwrap().clone()).unwrap();(item,commands)
    }
    fn literal(text:&str)->String{format!("{{{}}}\r\n{text}",text.len())}
    fn wire(input:&str)->(imap::Session<Wire>,Arc<Mutex<Vec<u8>>>){
        let output=Arc::new(Mutex::new(Vec::new()));
        let wire=Wire{input:Cursor::new(format!("a1 OK login\r\n{input}").into_bytes()),output:output.clone()};
        (imap::Client::new(wire).login("test","test").unwrap(),output)
    }
    fn account()->Account{Account {custom: None, email:"test@example.com".into(),provider:super::super::Provider::Qq,enabled:true}}
    #[test]
    fn out_of_sync_response_reports_an_error_instead_of_crashing(){
        // 用户崩溃日志：vendor\imap\src\client.rs:1369 的标签断言。a2 的位置收到上一条命令迟到的 a1 结束行。
        let (mut session,output)=wire("a1 OK late\r\n* 1 FETCH (UID 42 BODY[HEADER] {9}\r\nSubject:\n)\r\na2 OK fetch\r\n");
        assert!(item(&mut session,42,&account()).is_err(),"错位的连接应交给上层重连");
        let commands=String::from_utf8(output.lock().unwrap().clone()).unwrap();
        assert!(!commands.contains("BODY.PEEK[TEXT]"),"连接失步后不能再发备用读取命令");
        assert!(matches!(session.noop(),Err(imap::Error::ConnectionLost)));
    }
    #[test]
    fn unparseable_fetch_does_not_reuse_the_connection(){
        let (mut session,output)=wire("* 1 FETCH (UID 42 BODYSTRUCTURE ((((\r\na2 OK fetch\r\n* 1 FETCH (UID 42 BODY[HEADER] {9}\r\nSubject:\n BODY[TEXT] {1}\r\nx)\r\na3 OK fetch\r\n");
        assert!(item(&mut session,42,&account()).is_err());
        let commands=String::from_utf8(output.lock().unwrap().clone()).unwrap();
        assert!(!commands.contains("BODY.PEEK[TEXT]"));
    }
    #[test]
    fn server_rejection_still_falls_back_on_the_same_connection(){
        let header="Subject: Old server\r\nContent-Type: text/plain\r\n\r\n";
        let (mut session,_)=wire(&format!("a2 NO unsupported\r\n* 1 FETCH (UID 42 BODY[HEADER] {} BODY[TEXT] {})\r\na3 OK fetch\r\n",literal(header),literal("Full text")));
        let item=item(&mut session,42,&account()).unwrap().unwrap();
        assert_eq!(item.summary.as_ref(),"Full text");
    }

    #[test]
    fn qq_empty_body_structure_accepts_nil_transfer_encoding(){
        let response=b"* 1 FETCH (UID 42 BODYSTRUCTURE (\"TEXT\" \"PLAIN\" (\"CHARSET\" \"UTF-8\") NIL NIL NIL 0 0 NIL NIL NIL))\r\n";
        assert!(imap_proto::parse_response(response).is_ok(),"空邮件的 NIL 编码不能让后续收信响应错位");
    }
    #[test]
    fn large_binary_attachment_is_never_requested_and_full_text_is_kept(){
        let header="From: sender@example.com\r\nSubject: Login code\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n";
        let body=format!("Your verification code is 834921.\r\n{}\r\nEnd of message", "long body ".repeat(10000));
        let mime="Content-Type: text/plain; charset=utf-8\r\n\r\n";
        let structure=r#"(("TEXT" "PLAIN" ("CHARSET" "UTF-8") NIL NIL "7BIT" 100000 1)("APPLICATION" "ZIP" NIL NIL NIL "BASE64" 33554432) "MIXED")"#;
        let reply=format!("* 1 FETCH (UID 42 BODYSTRUCTURE {structure} BODY[HEADER] {})\r\na2 OK fetch\r\n* 1 FETCH (UID 42 BODY[1.MIME] {} BODY[1] {})\r\na3 OK fetch\r\n",literal(header),literal(mime),literal(&body));
        let (item,commands)=run(reply);
        assert_eq!(item.code.as_deref(),Some("834921"));assert_eq!(item.summary.as_ref(),body.replace("\r\n","\n"));
        assert!(!commands.contains("BODY.PEEK[TEXT]") && !commands.contains("BODY.PEEK[2]"),"不能下载 32 MiB 二进制附件");
        let copy=item.clone();assert_eq!(item.summary.as_ptr(),copy.summary.as_ptr(),"列表、详情和独立阅读窗口应共享正文");
    }
    #[test]
    fn unsupported_structure_falls_back_without_marking_mail_read(){
        let header="Subject: Old server\r\nContent-Type: text/plain\r\n\r\n";
        let reply=format!("a2 BAD unsupported\r\n* 1 FETCH (UID 42 BODY[HEADER] {} BODY[TEXT] {})\r\na3 OK fetch\r\n",literal(header),literal("Full text"));
        let (item,commands)=run(reply);assert_eq!(item.summary.as_ref(),"Full text");
        assert!(commands.contains("BODY.PEEK[TEXT]"));assert!(!commands.contains("BODY[TEXT]"));
    }
    #[test]
    fn nested_alternatives_preserve_html_links_encoding_and_calendar(){
        use base64::Engine;
        let header="Subject: Meeting\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n";
        let structure=r#"((("TEXT" "PLAIN" ("CHARSET" "UTF-8") NIL NIL "QUOTED-PRINTABLE" 100 1)("TEXT" "HTML" ("CHARSET" "UTF-8") NIL NIL "BASE64" 100 1) "ALTERNATIVE")("IMAGE" "PNG" NIL NIL NIL "BASE64" 10000000)("TEXT" "CALENDAR" ("CHARSET" "UTF-8") NIL NIL "BASE64" 100 1) "MIXED")"#;
        let html=base64::engine::general_purpose::STANDARD.encode("<p>会议资料</p><a href=\"https://example.com/doc?a=1&amp;b=2\">打开文档</a>");
        let calendar=base64::engine::general_purpose::STANDARD.encode("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:test-meeting\r\nSUMMARY:项目评审\r\nDTSTART:20261104T060000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
        let mut reply=format!("* 1 FETCH (UID 42 BODYSTRUCTURE {structure} BODY[HEADER] {})\r\na2 OK fetch\r\n",literal(header));
        for (i,(path,kind,encoding,body)) in [
            ("1.1","plain","quoted-printable","Your verification code is =38=33=34=39=32=31."),
            ("1.2","html","base64",html.as_str()),("3","calendar","base64",calendar.as_str()),
        ].into_iter().enumerate(){
            let mime=format!("Content-Type: text/{kind}; charset=utf-8\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n");
            reply.push_str(&format!("* 1 FETCH (UID 42 BODY[{path}.MIME] {} BODY[{path}] {})\r\na{} OK fetch\r\n",literal(&mime),literal(body),i+3));
        }
        let (item,commands)=run(reply);
        assert_eq!(item.code.as_deref(),Some("834921"));
        assert_eq!(item.links[0].url,"https://example.com/doc?a=1&b=2");assert_eq!(item.links[0].label,"打开文档");
        assert_eq!(item.meeting.unwrap().title,"项目评审");assert!(!commands.contains("BODY.PEEK[2]"));
    }

    #[test]
    #[ignore="使用已配置邮箱进行只读对比；不打印账号、凭据或邮件内容"]
    fn live_segment_fetch_matches_legacy_for_recent_mail(){
        let account=super::super::load_accounts().into_iter().find(|a|a.enabled).expect("需要一个已配置邮箱");
        let password=super::super::secrets::load(&account.credential_key()).expect("读取凭据失败");
        let mut session=super::super::net::connect(&account,&password).expect("连接失败");
        session.examine("INBOX").unwrap();
        let unread=session.uid_search("UNSEEN").unwrap();
        let mut uids=session.uid_search("ALL").unwrap().into_iter().collect::<Vec<_>>();uids.sort_unstable();
        let mut count=0;
        for uid in uids.into_iter().rev().take(20){
            let old=legacy(&mut session,uid,&account).expect("旧路径读取失败").unwrap();
            let new=item(&mut session,uid,&account).expect("分段读取失败").unwrap();
            assert!(old.subject==new.subject && old.from==new.from && old.time==new.time,"元信息不一致");
            assert!(old.summary==new.summary,"正文不一致，旧长度={}，新长度={}",old.summary.len(),new.summary.len());
            assert!(old.code==new.code,"验证码不一致");assert!(old.links==new.links,"链接不一致");assert!(old.meeting==new.meeting,"日程不一致");
            count+=1;
        }
        assert!(count>0,"收件箱为空，不能据此确认读取兼容性");
        assert_eq!(unread,session.uid_search("UNSEEN").unwrap(),"只读检查不能改变未读状态");session.logout().unwrap();
        println!("compared_messages={count} content_equal=true unread_unchanged=true");
    }
}
