//! 邮件整理：操作服务器已读标记、回收站和恢复，逐账号串行执行。
use crate::mail::{Account,Item};
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Hash)]
pub enum Folder{#[default] Inbox,Trash}
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum Action{Load,Read{uid:u32,seen:bool},ReadAll,Move{uid:u32}}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Request{pub account:Account,pub folder:Folder,pub validity:Option<u32>,pub known:Vec<u32>,pub extra:Vec<(u32,u32)>,pub action:Action}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Snapshot{pub validity:u32,pub entries:Vec<(u32,bool)>,pub items:Vec<Item>}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Completed{pub request:Request,pub result:Result<Snapshot,String>}

#[cfg(windows)]
mod protocol{
    use super::*;
    use std::io::{Read,Write};
    fn fail(context:&str,error:imap::Error)->String{
        let detail=match error{imap::Error::No(_)=>"服务器拒绝操作",imap::Error::Bad(_)=>"服务器不支持该操作",_=>"连接中断，请刷新后确认结果"};
        format!("{context}：{detail}")
    }
    fn set(uids:&[u32])->String{uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")}
    fn trash<S:Read+Write>(session:&mut imap::Session<S>)->Result<String,String>{
        use imap::types::NameAttribute;
        let names=session.list(Some(""),Some("*")).map_err(|e|fail("读取回收站",e))?;
        let selectable=|n:&&imap::types::Name|!n.attributes().contains(&NameAttribute::NoSelect);
        let special=names.iter().filter(selectable).find(|n|n.attributes().iter().any(|a|matches!(a,NameAttribute::Custom(s) if s.eq_ignore_ascii_case("\\Trash"))));
        if let Some(n)=special{return Ok(n.name().into());}
        // 仅采用服务器实际列出的常见回收站名称，不猜测或新建文件夹。
        let mut aliases=vec!["Trash".to_string(),"Deleted Messages".into(),"Deleted Items".into(),"INBOX.Trash".into(),"INBOX/Trash".into(),"[Gmail]/Trash".into()];
        use base64::Engine;
        for name in ["已删除","已删除邮件","垃圾箱","废纸篓","废件箱"]{
            let bytes=name.encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<_>>();
            aliases.push(format!("&{}-",base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes).replace('/',",")));aliases.push(name.into());
        }
        names.iter().filter(selectable).find(|n|aliases.iter().any(|alias|n.name().eq_ignore_ascii_case(alias)))
            .map(|n|n.name().into()).ok_or_else(||"未识别到邮箱回收站，请先在邮箱客户端确认回收站文件夹".into())
    }
    fn select<S:Read+Write>(session:&mut imap::Session<S>,name:&str,expected:Option<u32>,write:bool)->Result<(u32,u32),String>{
        let mailbox=if write{session.select(name)}else{session.examine(name)}.map_err(|e|fail("打开邮件文件夹",e))?;
        let validity=mailbox.uid_validity.filter(|v|*v>0).ok_or("邮箱未提供稳定的邮件标识，暂不能整理")?;
        if write && expected!=Some(validity){return Err("邮件列表已变化，请刷新后重试".into());}
        Ok((validity,mailbox.exists))
    }
    fn change<S:Read+Write>(session:&mut imap::Session<S>,request:&Request,destination:Option<&str>)->Result<(),String>{
        let uid=match request.action{Action::Read{uid,..}|Action::Move{uid}=>Some(uid),_=>None};
        if let Some(uid)=uid{
            if uid==0{return Err("邮件标识无效，请刷新".into());}
            if !session.uid_search(format!("UID {uid}")).map_err(|e|fail("确认邮件",e))?.contains(&uid){return Err("这封邮件已被其他客户端移走，请刷新".into());}
        }
        match request.action{
            Action::Load=>{},
            Action::Read{uid,seen}=>{session.uid_store(uid.to_string(),if seen{"+FLAGS.SILENT (\\Seen)"}else{"-FLAGS.SILENT (\\Seen)"}).map_err(|e|fail("更新已读状态",e))?;},
            Action::ReadAll=>{
                let mut uids=session.uid_search("UNSEEN UNDELETED").map_err(|e|fail("读取未读邮件",e))?.into_iter().collect::<Vec<_>>();uids.sort_unstable();
                for batch in uids.chunks(200){session.uid_store(set(batch),"+FLAGS.SILENT (\\Seen)").map_err(|e|fail("全部标为已读",e))?;}
            },
            Action::Move{uid}=>{
                let destination=destination.ok_or("未找到目标邮件文件夹")?;
                let caps=session.capabilities().map_err(|e|fail("检查邮箱能力",e))?;
                if caps.has_str("MOVE"){
                    session.uid_mv(uid.to_string(),destination).map_err(|e|fail("移动邮件",e))?;
                }else{return Err("邮箱未提供 MOVE，无法安全移动邮件。可关闭同步在本机整理，或在邮箱客户端操作".into());}
            }
        }
        Ok(())
    }
    pub(super) fn execute<S:Read+Write>(session:&mut imap::Session<S>,request:&Request)->Result<Snapshot,String>{
        let trash=if request.folder==Folder::Trash || matches!(request.action,Action::Move{..}){Some(trash(session)?)}else{None};
        let name=if request.folder==Folder::Inbox{"INBOX"}else{trash.as_deref().unwrap()};
        let write=request.action!=Action::Load;
        let(validity,mut exists)=select(session,name,request.validity,write)?;
        let destination=if request.folder==Folder::Inbox{trash.as_deref()}else{Some("INBOX")};
        change(session,request,destination)?;
        if write{let(current,count)=select(session,name,None,false)?;if current!=validity{return Err("邮件文件夹已变化，请刷新后确认操作结果".into());}exists=count;}
        let mut entries=Vec::new();let mut items=Vec::new();
        if exists>0{
            let limit=request.known.len().clamp(20,60) as u32;
            let first=exists.saturating_sub(limit-1).max(1);
            let fetched=session.fetch(format!("{first}:*"),"(UID FLAGS)").map_err(|e|fail("读取最近邮件状态",e))?;
            for f in fetched.iter().filter(|f|!f.flags().contains(&imap::types::Flag::Deleted)){
                if let Some(uid)=f.uid{entries.push((uid,f.flags().contains(&imap::types::Flag::Seen)));}
            }
            entries.sort_unstable_by(|a,b|b.0.cmp(&a.0));
        }
        let extra:Vec<_>=request.extra.iter().filter(|(v,uid)|*v==validity&&*uid>0&&!entries.iter().any(|(id,_)|id==uid)).map(|(_,uid)|*uid).take(60).collect();
        if !extra.is_empty(){
            let fetched=session.uid_fetch(set(&extra),"(UID FLAGS)").map_err(|e|fail("读取本机回收站邮件状态",e))?;
            for f in fetched.iter().filter(|f|!f.flags().contains(&imap::types::Flag::Deleted)){
                if let Some(uid)=f.uid{if !entries.iter().any(|(id,_)|*id==uid){entries.push((uid,f.flags().contains(&imap::types::Flag::Seen)));}}
            }
        }
        for (uid,_) in &entries{
            if request.validity==Some(validity)&&request.known.contains(uid){continue;}
            if let Some(item)=crate::mail::fetch::item(session,*uid,&request.account).map_err(|_|"读取邮件失败，请刷新后重试")?{items.push(item);}
        }
        Ok(Snapshot{validity,entries,items})
    }
    pub(super) fn run(request:&Request)->Result<Snapshot,String>{
        let password=crate::mail::secrets::load(&request.account.credential_key())?;
        let mut session=crate::mail::net::connect(&request.account,&password)?;
        let result=execute(&mut session,request);let _=session.logout();result
    }
    #[cfg(test)]mod tests{
        use super::*;use std::{io::Cursor,sync::{Arc,Mutex}};
        #[derive(Debug)]
        struct Wire{input:Cursor<Vec<u8>>,out:Arc<Mutex<Vec<u8>>>}
        impl Read for Wire{fn read(&mut self,b:&mut[u8])->std::io::Result<usize>{self.input.read(b)}}
        impl Write for Wire{fn write(&mut self,b:&[u8])->std::io::Result<usize>{self.out.lock().unwrap().extend_from_slice(b);Ok(b.len())}fn flush(&mut self)->std::io::Result<()>{Ok(())}}
        fn session(replies:&str)->(imap::Session<Wire>,Arc<Mutex<Vec<u8>>>){let out=Arc::new(Mutex::new(Vec::new()));let wire=Wire{input:Cursor::new(format!("a1 OK login\r\n{replies}").into_bytes()),out:out.clone()};(imap::Client::new(wire).login("test","test").unwrap(),out)}
        fn request(action:Action)->Request{Request{account:Account{email:"test@example.com".into(),provider:crate::mail::Provider::Qq,custom:None,enabled:true},folder:Folder::Inbox,validity:Some(7),known:vec![42],extra:vec![],action}}
        #[test]fn changed_uid_validity_blocks_all_writes(){
            let(mut s,out)=session("* OK [UIDVALIDITY 8] changed\r\na2 OK select\r\n");
            assert!(execute(&mut s,&request(Action::Read{uid:42,seen:true})).is_err());
            let wire=String::from_utf8(out.lock().unwrap().clone()).unwrap();assert!(!wire.contains("STORE")&&!wire.contains("MOVE"));
        }
        #[test]fn seen_only_updates_the_requested_uid(){
            let(mut s,out)=session("* SEARCH 42\r\na2 OK search\r\na3 OK store\r\n");
            change(&mut s,&request(Action::Read{uid:42,seen:true}),None).unwrap();
            let wire=String::from_utf8(out.lock().unwrap().clone()).unwrap();assert!(wire.contains("UID STORE 42 +FLAGS.SILENT (\\Seen)"));assert!(!wire.contains("EXPUNGE"));
        }
        #[test]fn move_never_expunges_other_deleted_messages(){
            for supports_move in [true,false]{
                let replies=if supports_move{"* SEARCH 42\r\na2 OK search\r\n* CAPABILITY IMAP4rev1 MOVE\r\na3 OK caps\r\na4 OK move\r\n"}else{"* SEARCH 42\r\na2 OK search\r\n* CAPABILITY IMAP4rev1 UIDPLUS\r\na3 OK caps\r\na4 OK copy\r\na5 OK store\r\na6 OK expunge\r\n"};
                let(mut s,out)=session(replies);let result=change(&mut s,&request(Action::Move{uid:42}),Some("Trash"));
                assert_eq!(result.is_ok(),supports_move);
                let wire=String::from_utf8(out.lock().unwrap().clone()).unwrap();assert!(!wire.lines().any(|l|l.ends_with(" EXPUNGE")));
                if supports_move{assert!(wire.contains("UID MOVE 42 \"Trash\""));}else{assert!(!wire.contains("COPY")&&!wire.contains("STORE")&&!wire.contains("EXPUNGE"));}
            }
        }
        #[test]fn unsupported_move_does_not_copy_or_delete(){
            let(mut s,out)=session("* SEARCH 42\r\na2 OK search\r\n* CAPABILITY IMAP4rev1\r\na3 OK caps\r\n");assert!(change(&mut s,&request(Action::Move{uid:42}),Some("Trash")).is_err());
            let wire=String::from_utf8(out.lock().unwrap().clone()).unwrap();assert!(!wire.contains("COPY")&&!wire.contains("STORE"));
        }
        #[test]
        #[ignore="只读检查本机邮箱整理能力；不修改任何邮件"]
        fn live_mail_management_capabilities(){
            let account=crate::mail::load_accounts().into_iter().find(|a|a.enabled).expect("需要已连接的邮箱");
            let password=crate::mail::secrets::load(&account.credential_key()).unwrap();
            let mut session=crate::mail::net::connect(&account,&password).unwrap();
            let caps=session.capabilities().unwrap();let supports_move=caps.has_str("MOVE");let uidplus=caps.has_str("UIDPLUS");
            let name=trash(&mut session).expect("应能识别服务器回收站");
            let inbox=select(&mut session,"INBOX",None,false).unwrap();let trash_validity=select(&mut session,&name,None,false).unwrap();
            session.logout().unwrap();assert!(inbox.0>0&&trash_validity.0>0);
            println!("trash_found=true uid_validity=true move={supports_move} uidplus={uidplus}");
        }
        #[test]fn refresh_reuses_cached_bodies_and_only_reads_flags(){
            let(mut s,out)=session("* OK [UIDVALIDITY 7] valid\r\n* 1 EXISTS\r\na2 OK examine\r\n* 1 FETCH (UID 42 FLAGS (\\Seen))\r\na3 OK fetch\r\n");
            let snapshot=execute(&mut s,&request(Action::Load)).unwrap();assert!(snapshot.items.is_empty());assert_eq!(snapshot.entries,vec![(42,true)]);
            let wire=String::from_utf8(out.lock().unwrap().clone()).unwrap();assert!(wire.contains("EXAMINE")&&wire.contains("(UID FLAGS)"));assert!(!wire.contains("BODY")&&!wire.contains("STORE")&&!wire.contains("SEARCH"));
        }
        #[test]fn local_trash_reload_respects_uid_validity_and_reuses_bodies(){
            let(mut s,out)=session("* OK [UIDVALIDITY 7] valid\r\na2 OK examine\r\n* 1 FETCH (UID 42 FLAGS (\\Seen))\r\na3 OK fetch\r\n");
            let mut req=request(Action::Load);req.extra=vec![(7,42),(8,99)];
            let snapshot=execute(&mut s,&req).unwrap();assert_eq!(snapshot.entries,vec![(42,true)]);assert!(snapshot.items.is_empty());
            let wire=String::from_utf8(out.lock().unwrap().clone()).unwrap();assert!(wire.contains("UID FETCH 42"));assert!(!wire.contains("99")&&!wire.contains("BODY"));
        }
        #[test]fn read_all_uses_only_unread_uids(){
            let(mut s,out)=session("* SEARCH 42 43\r\na2 OK search\r\na3 OK store\r\n");change(&mut s,&request(Action::ReadAll),None).unwrap();
            let wire=String::from_utf8(out.lock().unwrap().clone()).unwrap();assert!(wire.contains("UID STORE 42,43 +FLAGS.SILENT (\\Seen)"));
        }
        #[test]fn trash_uses_server_special_use_name(){
            let(mut s,_)=session("* LIST (\\Trash) \"/\" \"SpecialTrash\"\r\na2 OK list\r\n");assert_eq!(trash(&mut s).unwrap(),"SpecialTrash");
        }
    }
}
#[cfg(windows)]pub fn start()->(std::sync::mpsc::Sender<Request>,std::sync::mpsc::Receiver<Completed>){
    let(tx,rx)=std::sync::mpsc::channel::<Request>();let(done,out)=std::sync::mpsc::channel();
    std::thread::spawn(move||while let Ok(request)=rx.recv(){let result=protocol::run(&request);let _=done.send(Completed{request,result});});(tx,out)
}
