//! Gemini 网页订阅额度，接口与字段映射参考 CodexBar 的 gemini-web 插件。
//! 浏览器连接只在用户点击时读取会话；会话由 Windows DPAPI 加密保存。
use super::{Account,Snapshot,Window};
use serde_json::{json,Value};
use std::{fs,io::Read,path::PathBuf,sync::{mpsc,Arc,atomic::{AtomicBool,Ordering}}};

#[cfg(windows)]
#[path="gemini_browser.rs"]
mod browser;

const ENDPOINT:&str="https://gemini.google.com/_/BardChatUi/graphql/schemas/GEMINI_WEB_GRAPHQL/executeQuery";
const SIGNATURE:&str="2/dWbCvkVB0X16r9NPnxifeOAAqdCqMf0xeWvau9yPy7g=";
const LOGIN_ERROR:&str="Gemini 登录已失效，请重新连接浏览器账号";
const PARSE_ERROR:&str="Gemini 额度接口已变化，暂时无法读取";

pub fn parse(value:&Value)->Result<Snapshot,String>{
    let data=value.as_array().ok_or(PARSE_ERROR)?.iter().filter_map(|v|v.get("data")).find(|d|d.get("viewer").is_some()).ok_or(PARSE_ERROR)?;
    if data["viewer"].is_null(){return Err(LOGIN_ERROR.into());}
    let usage=data.pointer("/viewer/rpcs/getUsageInfo").ok_or(PARSE_ERROR)?;
    let pools=usage["pools"].as_array().ok_or(PARSE_ERROR)?;
    let windows=[("GXU_FIVE_HOURLY",18000,"5 小时"),("GXU_WEEKLY",604800,"本周")].into_iter().filter_map(|(kind,seconds,label)|{
        let pool=pools.iter().find(|p|p["poolType"]==kind)?;
        let used=pool["percentageUsed"].as_f64().filter(|n|n.is_finite() && *n>=0.0)?;
        Some(Window{label:label.into(),seconds:Some(seconds),remaining:(100.0-used*100.0).clamp(0.0,100.0) as f32,reset:super::reset(&pool["gxu"]["windowRefreshesAt"]),amount:None})
    }).collect::<Vec<_>>();
    if windows.is_empty(){return Err("此 Google 账号暂未提供 Gemini 订阅额度".into());}
    Ok(Snapshot{kiro:None,relay:None,credits:None,wallet:None,plan:if usage["subscriptionTier"]=="TIER1"{"AI Pro"}else{""}.into(),windows,extra:vec![],at:chrono::Utc::now().timestamp()})
}
fn decode(raw:&str)->Result<Snapshot,String>{
    let raw=raw.trim_start();let raw=raw.strip_prefix(")]}'").unwrap_or(raw).trim_start();
    if raw.starts_with('<'){return Err(LOGIN_ERROR.into());}
    parse(&serde_json::from_str::<Value>(raw).map_err(|_|PARSE_ERROR)?)
}
fn email(raw:&str)->Result<String,String>{
    let value=raw.trim().to_ascii_lowercase();
    if value.len()>254 || !value.is_ascii() || value.chars().any(char::is_whitespace) || value.split('@').count()!=2 || !value.split('@').all(|s|!s.is_empty()) {return Err("请填写这个 Google 账号的完整邮箱".into());}
    Ok(value)
}
fn slot(email:&str)->String{ring::digest::digest(&ring::digest::SHA256,email.as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect()}
fn cookie(raw:&str)->Result<String,String>{
    let raw=raw.trim();let raw=raw.strip_prefix("Cookie:").or_else(||raw.strip_prefix("cookie:")).unwrap_or(raw).trim();
    if raw.is_empty() || raw.len()>32768 || !raw.is_ascii() || raw.bytes().any(|b|b<32 || b==127){return Err("请粘贴有效的 Cookie 请求头".into());}
    if !raw.split(';').any(|part|part.trim().split_once('=').is_some_and(|(name,value)|matches!(name,"SID"|"__Secure-1PSID"|"__Secure-3PSID") && !value.is_empty())){return Err("Cookie 中没有 Google 登录会话，请重新复制".into());}
    Ok(raw.into())
}
struct Session {email:String,cookie:String}
fn request_url(email:&str)->String{
    let mut url=url::Url::parse(ENDPOINT).unwrap();
    let variables=json!({"g1Request":{"entrypoints":["GEMINI_USAGE_SETTINGS_UPGRADE_BANNER"]},"workspaceRequest":{"recommendationsForSurfaceRequests":[]}}).to_string();
    url.query_pairs_mut().append_pair("query_signature",SIGNATURE).append_pair("variables",&variables).append_pair("authuser",email);url.into()
}
fn redirect_target(current:&str,location:&str)->Option<String>{
    let target=url::Url::parse(current).ok()?.join(location).ok()?;
    // Google 会把邮箱形式的 authuser 规范化为账号编号；只跟随同站的用量查询。
    (target.scheme()=="https" && target.host_str()==Some("gemini.google.com") && target.port_or_known_default()==Some(443) && target.username().is_empty() && target.password().is_none() && target.path().ends_with("/_/BardChatUi/graphql/schemas/GEMINI_WEB_GRAPHQL/executeQuery")).then(||target.into())
}
fn request(agent:&ureq::Agent,url:&str,session:&Session)->Result<Snapshot,(String,u64)>{
    let mut current=url.to_owned();
    for _ in 0..3{
    let response=agent.get(&current).set("Cookie",&session.cookie).set("Accept","application/json").set("Referer","https://gemini.google.com/usage").set("User-Agent","Mozilla/5.0 Daozai").call().map_err(|e|match e{
        ureq::Error::Status(401|403,_)=>(LOGIN_ERROR.into(),0),
        ureq::Error::Status(429,r)=>("Gemini 请求过于频繁，稍后自动重试".into(),r.header("Retry-After").and_then(|s|s.parse().ok()).unwrap_or(180).clamp(30,3600)),
        ureq::Error::Status(n,_)=>(format!("Gemini 额度服务暂不可用（{n}）"),60),
        _=>("无法连接 Gemini，请检查网络或代理".into(),30)
    })?;
    if (300..400).contains(&response.status()){
        current=response.header("Location").and_then(|to|redirect_target(&current,to)).ok_or_else(||(LOGIN_ERROR.into(),0))?;continue;
    }
    let mut body=String::new();response.into_reader().take(1024*1024).read_to_string(&mut body).map_err(|_|("读取 Gemini 额度失败".into(),30))?;
    return decode(&body).map_err(|e|(e,60));
    }
    Err((LOGIN_ERROR.into(),0))
}
fn session_path(slot:&str)->Result<PathBuf,String>{
    if slot.len()!=64 || !slot.bytes().all(|b|b.is_ascii_hexdigit()){return Err(LOGIN_ERROR.into());}
    Ok(crate::model::config_dir().join("credentials").join(format!("gemini-{slot}.bin")))
}
#[cfg(windows)]
pub(super) fn protect(raw:&[u8],encrypt:bool)->Result<Vec<u8>,String>{
    use windows::Win32::{Foundation::{HLOCAL,LocalFree},Security::Cryptography::{CryptProtectData,CryptUnprotectData,CRYPT_INTEGER_BLOB,CRYPTPROTECT_UI_FORBIDDEN}};
    let input=CRYPT_INTEGER_BLOB{cbData:raw.len().try_into().map_err(|_|LOGIN_ERROR)?,pbData:raw.as_ptr().cast_mut()};let mut output=CRYPT_INTEGER_BLOB::default();
    unsafe{
        if encrypt{CryptProtectData(&input,None,None,None,None,CRYPTPROTECT_UI_FORBIDDEN,&mut output)}else{CryptUnprotectData(&input,None,None,None,None,CRYPTPROTECT_UI_FORBIDDEN,&mut output)}.map_err(|_|"无法访问 Windows 安全保存的登录会话")?;
        let bytes=std::slice::from_raw_parts(output.pbData,output.cbData as usize).to_vec();let _=LocalFree(HLOCAL(output.pbData.cast()));Ok(bytes)
    }
}
#[cfg(not(windows))]
pub(super) fn protect(_raw:&[u8],_encrypt:bool)->Result<Vec<u8>,String>{Err("当前系统暂不支持安全保存登录会话".into())}
fn save(session:&Session)->Result<String,String>{
    let slot=slot(&session.email);let path=session_path(&slot)?;
    let raw=json!({"email":session.email,"cookie":session.cookie}).to_string();let encrypted=protect(raw.as_bytes(),true)?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|_|"无法保存 Gemini 登录")?;
    crate::model::atomic_write(&path,&encrypted).map_err(|_|"无法保存 Gemini 登录")?;Ok(slot)
}
fn load(account:&Account)->Result<Session,String>{
    let expected=account.slot.as_deref().ok_or(LOGIN_ERROR)?;let path=session_path(expected)?;
    let raw=fs::read(path).map_err(|_|LOGIN_ERROR)?;let raw=protect(&raw,false)?;
    let value:Value=serde_json::from_slice(&raw).map_err(|_|LOGIN_ERROR)?;
    let email=email(value["email"].as_str().ok_or(LOGIN_ERROR)?)?;
    if slot(&email)!=expected{return Err("Gemini 账号不一致，请重新连接".into());}
    Ok(Session{email,cookie:cookie(value["cookie"].as_str().ok_or(LOGIN_ERROR)?)?})
}
pub fn forget(account:&Account){if let Some(path)=account.slot.as_deref().and_then(|s|session_path(s).ok()){let _=fs::remove_file(path);}}
pub fn address(account:&Account)->Option<String>{load(account).ok().map(|s|s.email)}
pub fn fetch(agent:&ureq::Agent,account:&Account)->Result<Snapshot,(String,u64)>{
    let session=load(account).map_err(|e|(e,0))?;request(agent,&request_url(&session.email),&session)
}
pub struct Connected {pub slot:String,pub label:String,pub snapshot:Snapshot,session:Session}
impl Connected {pub fn finish(self)->Result<Self,String>{save(&self.session)?;Ok(self)}}
pub struct Connect {rx:mpsc::Receiver<Result<Connected,String>>,cancel:Arc<AtomicBool>}
impl Connect {
    pub fn start(alias:String,address:String,header:Option<String>,expected:Option<String>)->Self{
        let(tx,rx)=mpsc::channel();let cancel=Arc::new(AtomicBool::new(false));let stopped=cancel.clone();
        std::thread::spawn(move||{let result=(||{
            let session=if let Some(header)=header{Session{email:email(&address)?,cookie:cookie(&header)?}}else{
                #[cfg(windows)]{browser::login(&address,&stopped)?}
                #[cfg(not(windows))]{return Err("Chrome 原生连接暂仅支持 Windows，可使用手动 Cookie 连接".into());}
            };
            if stopped.load(Ordering::Relaxed){return Err("已取消 Gemini 连接".into());}
            if expected.as_deref().is_some_and(|s|s!=slot(&session.email)){return Err("当前浏览器是另一个 Google 账号，请切回原账号后重新连接".into());}
            let snapshot=request(&super::http_agent(),&request_url(&session.email),&session).map_err(|e|e.0)?;
            if stopped.load(Ordering::Relaxed){return Err("已取消 Gemini 连接".into());}
            let slot=slot(&session.email);let label=if alias.trim().is_empty(){format!("Gemini · {}",session.email)}else{alias.trim().into()};
            Ok(Connected{slot,label,snapshot,session})
        })();let _=tx.send(result);});Self{rx,cancel}
    }
    pub fn poll(&self)->Option<Result<Connected,String>>{self.rx.try_recv().ok()}
    #[cfg(test)]
    pub(crate) fn pending()->(Self,mpsc::Sender<Result<Connected,String>>){let(tx,rx)=mpsc::channel();(Self{rx,cancel:Arc::new(AtomicBool::new(false))},tx)}
}
impl Drop for Connect {fn drop(&mut self){self.cancel.store(true,Ordering::Relaxed);}}

#[cfg(test)]
pub(crate) fn simulated_connected()->Connected{
    let session=Session{email:"synthetic@example.com".into(),cookie:"SID=synthetic-only".into()};
    Connected{slot:slot(&session.email),label:"Gemini 测试".into(),snapshot:Snapshot::default(),session}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(five:Value,week:Value)->Value{json!([{"data":{"viewer":{"rpcs":{"getUsageInfo":{"subscriptionTier":"TIER1","pools":[{"poolType":"GXU_FIVE_HOURLY","percentageUsed":five,"gxu":{"windowRefreshesAt":"2030-01-01T08:24:11Z"}},{"poolType":"GXU_WEEKLY","percentageUsed":week,"gxu":{"windowRefreshesAt":"2030-01-04T03:24:11Z"}}]}}}},"errors":[{"message":"unrelated upsell"}]}])}
    #[test] fn gemini_fractional_usage_and_real_resets(){let s=parse(&response(json!(0.24),json!(0.01786011))).unwrap();assert_eq!(s.plan,"AI Pro");assert_eq!(s.rings().len(),2);assert_eq!(s.windows[0].remaining,76.0);assert!((s.windows[1].remaining-98.21399).abs()<0.001);assert_eq!(s.windows[0].reset,Some(1893486251));assert_eq!(s.windows[0].seconds,Some(18000));assert_eq!(s.windows[1].seconds,Some(604800));}
    #[test] fn gemini_zero_exhaustion_and_missing_windows(){let s=parse(&response(json!(0),json!(1))).unwrap();assert_eq!(s.windows[0].remaining,100.0);assert_eq!(s.windows[1].remaining,0.0);for bad in [Value::Null,json!(-1),json!("0.25"),json!("NaN")]{let s=parse(&response(bad.clone(),json!(0.5))).unwrap();assert_eq!(s.rings().len(),1);assert_eq!(s.rings()[0].label,"本周");assert!(parse(&response(bad.clone(),bad)).is_err());}assert_eq!(parse(&response(json!(1.2),Value::Null)).unwrap().windows[0].remaining,0.0);}
    #[test] fn gemini_signed_out_or_changed_queries_are_not_full_quota(){for raw in [r#"[{"data":{"viewer":null}}]"#,r#"[{"data":{}}]"#,"{}","[]",r#"[{"data":{"viewer":{"rpcs":{"getUsageInfo":{"pools":[]}}}}}]"#]{assert!(decode(raw).is_err());}let raw=format!(")]}}'\n{}",response(json!(0.3),json!(0.2)));assert_eq!(decode(&raw).unwrap().windows[0].remaining,70.0);assert!(decode("<html>private</html>").unwrap_err().contains("登录"));}
    #[test] fn gemini_unknown_plan_and_reset_stay_unknown(){let mut v=response(json!(0.1),Value::Null);let usage=&mut v[0]["data"]["viewer"]["rpcs"]["getUsageInfo"];usage["subscriptionTier"]=json!("NEW_TIER");usage["pools"][0]["gxu"]["windowRefreshesAt"]=json!("unknown");let s=parse(&v).unwrap();assert!(s.plan.is_empty());assert!(s.windows[0].reset.is_none());}
    #[test] fn gemini_account_selection_and_cookie_validation(){let e=email(" Personal@Example.COM ").unwrap();assert_eq!(e,"personal@example.com");let u=url::Url::parse(&request_url(&e)).unwrap();let pairs=u.query_pairs().collect::<std::collections::HashMap<_,_>>();assert_eq!(pairs["authuser"],e);assert_eq!(pairs["query_signature"],SIGNATURE);assert_eq!(u.host_str(),Some("gemini.google.com"));assert_ne!(slot(&e),slot("work@example.com"));for bad in ["", "test", "a@b\r\nInjected: yes"]{assert!(email(bad).is_err());}assert_eq!(cookie("Cookie: SID=test; SAPISID=other").unwrap(),"SID=test; SAPISID=other");for bad in ["", "SID=test\r\nX: bad", "foo=bar"]{assert!(cookie(bad).is_err());}assert!(session_path("../../other").is_err());}
    #[test] fn gemini_account_redirects_never_forward_cookies_to_another_site(){let url=request_url("personal@example.com");let target=redirect_target(&url,"?authuser=0").unwrap();assert_eq!(url::Url::parse(&target).unwrap().query(),Some("authuser=0"));for to in ["https://accounts.google.com/signin","https://gemini.google.com.evil.invalid/","http://gemini.google.com/","https://gemini.google.com:444/","https://user@gemini.google.com/","/app"]{assert!(redirect_target(&url,to).is_none());}}
    #[cfg(windows)] #[test] fn gemini_windows_storage_is_encrypted_and_roundtrips(){let raw=b"{\"email\":\"synthetic@example.com\",\"cookie\":\"SID=synthetic-secret\"}";let sealed=protect(raw,true).unwrap();assert!(!sealed.windows(16).any(|w|w==b"synthetic-secret"));assert_eq!(protect(&sealed,false).unwrap(),raw);let mut corrupt=sealed;corrupt[24]^=1;assert!(protect(&corrupt,false).is_err());}
    #[test] fn gemini_read_only_request_does_not_echo_private_errors(){
        use std::net::TcpListener;let server=TcpListener::bind("127.0.0.1:0").unwrap();let url=format!("http://{}/quota",server.local_addr().unwrap());
        let task=std::thread::spawn(move||{let(mut stream,_)=server.accept().unwrap();let mut bytes=[0u8;4096];let n=stream.read(&mut bytes).unwrap();let raw=String::from_utf8_lossy(&bytes[..n]);assert!(raw.starts_with("GET /quota HTTP/1.1"));assert!(raw.to_ascii_lowercase().contains("cookie: sid=synthetic"));use std::io::Write;stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 14\r\nConnection: close\r\n\r\nprivate-secret").unwrap();});
        let session=Session{email:"synthetic@example.com".into(),cookie:"SID=synthetic".into()};let error=request(&ureq::AgentBuilder::new().redirects(0).build(),&url,&session).unwrap_err();assert!(error.0.contains("重新连接"));assert!(!error.0.contains("private-secret"));task.join().unwrap();
    }
    #[test]
    #[ignore="按需只读验证已有 Gemini 加密会话，不更新配置"]
    fn gemini_saved_session_live_read_only(){
        let _g=crate::ui_tests::ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
        let config=super::super::Config::load();let a=config.accounts.iter().find(|a|a.enabled&&a.provider==super::super::Provider::Gemini).expect("需要已有 Gemini 账号");
        let snapshot=fetch(&super::super::http_agent(),a).unwrap();assert!(!snapshot.windows.is_empty());
        println!("已有 Gemini 加密会话读取成功，返回 {} 个额度周期",snapshot.windows.len());
    }
    #[cfg(windows)] #[test]
    #[ignore="使用指定隔离目录验证捕获会话，写入隔离账号配置"]
    fn gemini_captured_session_independent_refresh(){
        let _g=crate::ui_tests::ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
        assert!(std::env::var_os("DAOZAI_DATA_DIR").is_some(),"真实验证必须指定隔离测试目录");
        let slot=fs::read_to_string(crate::model::config_dir().join("verified-slot.txt")).unwrap();
        let mut config=super::super::Config::default();let id=config.connect_gemini(slot,"Gemini 真实验证".into());
        let account=config.accounts.iter().find(|a|a.id==id).unwrap();let snapshot=fetch(&super::super::http_agent(),account).unwrap();
        assert_eq!(snapshot.windows.len(),2);config.save_checked().unwrap();
        println!("浏览器已断开，原生查询成功：{:?}",snapshot.windows.iter().map(|w|(&w.label,w.remaining)).collect::<Vec<_>>());
    }
    #[cfg(windows)] #[test]
    #[ignore="需要本人允许 Chrome 原生连接，仅更新指定隔离目录中的会话"]
    fn gemini_native_chrome_live_connection(){
        let _g=crate::ui_tests::ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
        assert!(std::env::var_os("DAOZAI_DATA_DIR").is_some(),"真实验证必须指定隔离测试目录");
        let config=super::super::Config::load();let account=config.accounts.iter().find(|a|a.provider==super::super::Provider::Gemini).unwrap();
        let connect=Connect::start(account.alias.clone(),address(account).unwrap(),None,account.slot.clone());
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(315);
        let result=loop{if let Some(result)=connect.poll(){break result;}assert!(std::time::Instant::now()<deadline,"Chrome 授权等待超时");std::thread::sleep(std::time::Duration::from_millis(50));};
        let connected=result.unwrap().finish().unwrap();assert_eq!(connected.snapshot.windows.len(),2);
        assert_eq!(Some(connected.slot),account.slot);assert!(fetch(&super::super::http_agent(),account).is_ok());
        println!("原生 Rust 授权连接、身份校验、加密保存及独立刷新全部通过");
    }
}
