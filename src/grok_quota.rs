//! Grok Bot 周额度。凭据与接口映射参考 ai-usagebar，浏览器授权参考 gbu。
//! 原客户端登录文件只读；独立授权保存在 Windows 凭据管理器。
use super::{Account,Snapshot,Window};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD,Engine};
use ring::{digest,rand::{SecureRandom,SystemRandom}};
#[cfg(test)] use base64::engine::general_purpose::STANDARD;
#[cfg(test)] use ring::aead;
use serde_json::{json,Value};
use std::{fs,io::Read,path::{Path,PathBuf},sync::{mpsc,Arc,atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};

const BACKEND:&str="https://api2.cursor.sh";
const CLIENT_ID:&str="KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB";
const LOGIN_ERROR:&str="Grok Bot 登录不可用，请使用浏览器重新连接";

pub fn parse(v:&Value)->Result<Snapshot,String>{
    if v["usesPooledEnterpriseAllowance"].as_bool()==Some(true){return Err("企业共享额度未提供个人百分比".into());}
    if v["hasNonZeroIncludedLimit"].as_bool()==Some(false){return Err("此账号没有可显示的套餐额度".into());}
    let used=super::number(&v["usagePercent"]).filter(|n|(0.0..=100.0).contains(n)).ok_or("服务未返回有效的额度百分比")?;
    let reset=super::reset(&v["nextResetTimestampUtc"]);
    let seconds=super::reset(&v["currentPeriodStart"]).zip(reset).and_then(|(start,end)|end.checked_sub(start)).filter(|n|*n>0);
    let plan=["grokPlanLabel","cursorPlanName"].iter().find_map(|key|v[key].as_str().filter(|s|!s.trim().is_empty())).unwrap_or_default().to_owned();
    Ok(Snapshot{kiro:None,relay:None,credits:None,wallet:None,plan,at:chrono::Utc::now().timestamp(),windows:vec![Window{label:if v["isUnified"].as_bool()==Some(true){"共享周额度"}else{"本周"}.into(),seconds,remaining:(100.0-used) as f32,reset,amount:None}],extra:vec![]})
}

pub fn native_path()->Option<PathBuf>{std::env::var_os("APPDATA").map(|p|PathBuf::from(p).join("Grok Bot/sand-secrets.json")).filter(|p|p.is_file())}

pub struct Profile {pub slot:String,pub label:String}
fn accounts(raw:&[u8])->Result<(Option<String>,serde_json::Map<String,Value>),String>{
    let root:Value=serde_json::from_slice(raw).map_err(|_|LOGIN_ERROR)?;
    let value=match &root["cursor-accounts"]{Value::String(s)=>serde_json::from_str(s).map_err(|_|LOGIN_ERROR)?,other=>other.clone()};
    let entries=value["accounts"].as_object().filter(|m|!m.is_empty()).ok_or(LOGIN_ERROR)?.clone();
    Ok((value["active"].as_str().map(str::to_owned),entries))
}
pub fn profiles(path:&Path)->Result<Vec<Profile>,String>{
    let raw=fs::read(path).map_err(|_|"未找到 Grok Bot 登录文件")?;let(active,entries)=accounts(&raw)?;
    let mut slots:Vec<_>=entries.keys().cloned().collect();slots.sort_by_key(|slot|(Some(slot)!=active.as_ref(),slot.clone()));
    let key=native_key(path).ok();
    Ok(slots.into_iter().enumerate().map(|(i,slot)|{
        let label=key.as_ref().and_then(|key|entries[&slot]["cursor-account-profile"].as_str().and_then(|s|decrypt(key,s,&slot).ok()))
            .and_then(|s|serde_json::from_str::<Value>(&s).ok()).and_then(|p|p["email"].as_str().map(str::to_owned))
            .map(|email|format!("Grok Bot · {email}")).unwrap_or_else(||format!("Grok Bot · 账号 {}",i+1));
        Profile{slot,label}
    }).collect())
}

fn decrypt(key:&[u8],encoded:&str,slot:&str)->Result<String,String>{
    let encoded=if let Some(scoped)=encoded.strip_prefix("scoped:v1:"){
        let(scope,value)=scoped.split_once(':').ok_or(LOGIN_ERROR)?;if scope!=slot{return Err(LOGIN_ERROR.into());}value
    }else{encoded};
    super::native_auth::decrypt(key,encoded).map_err(|_|LOGIN_ERROR.into())
}

#[cfg(windows)]
fn native_key(path:&Path)->Result<Vec<u8>,String>{
    super::native_auth::key(path).map_err(|_|LOGIN_ERROR.into())
}
#[cfg(not(windows))]
fn native_key(_path:&Path)->Result<Vec<u8>,String>{Err("当前系统请使用浏览器连接 Grok Bot".into())}

struct Session {token:String,refresh:Option<String>}
fn claims(token:&str)->Result<Value,String>{
    let payload=token.split('.').nth(1).ok_or(LOGIN_ERROR)?;
    let raw=URL_SAFE_NO_PAD.decode(payload).map_err(|_|LOGIN_ERROR)?;serde_json::from_slice(&raw).map_err(|_|LOGIN_ERROR.into())
}
fn slot_of(token:&str)->Result<String,String>{
    let value=claims(token)?;let sub=value["sub"].as_str().filter(|s|!s.is_empty()).ok_or(LOGIN_ERROR)?;
    Ok(digest::digest(&digest::SHA256,sub.as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect())
}
fn session(value:&Value)->Result<Session,String>{
    let token=value["accessToken"].as_str().or_else(||value["access_token"].as_str()).filter(|s|!s.is_empty()).ok_or(LOGIN_ERROR)?.to_owned();
    let refresh=value["refreshToken"].as_str().or_else(||value["refresh_token"].as_str()).filter(|s|!s.is_empty()).map(str::to_owned);
    let _=slot_of(&token)?;Ok(Session{token,refresh})
}
fn native_session(account:&Account)->Result<Session,String>{
    let raw=fs::read(&account.source).map_err(|_|LOGIN_ERROR)?;let(active,entries)=accounts(&raw)?;
    let slot=account.slot.as_deref().or(active.as_deref()).ok_or(LOGIN_ERROR)?;
    let entry=entries.get(slot).ok_or(LOGIN_ERROR)?;let key=native_key(&account.source).map_err(|_|"无法读取 Grok Bot 本机解密密钥，请使用浏览器连接")?;
    let token=decrypt(&key,entry["cursor-access-token"].as_str().ok_or(LOGIN_ERROR)?,slot).map_err(|_|"无法解密 Grok Bot 本机登录，请使用浏览器连接")?;
    if slot_of(&token)?!=slot{return Err("Grok Bot 账号已变化，请重新识别账号".into());}
    Ok(Session{token,refresh:None})
}
#[cfg(windows)]
fn credential(slot:&str)->Result<keyring::Entry,String>{
    if slot.len()!=64 || !slot.bytes().all(|b|b.is_ascii_hexdigit()){return Err(LOGIN_ERROR.into());}
    keyring::Entry::new("Daozai.GrokBot",slot).map_err(|_|"无法打开 Windows 凭据管理器".into())
}
#[cfg(windows)]
fn save_session(slot:&str,value:&Session)->Result<(),String>{
    if slot_of(&value.token)?!=slot{return Err("登录账号不一致，请重新连接".into());}
    credential(slot)?.set_password(&json!({"accessToken":value.token,"refreshToken":value.refresh}).to_string()).map_err(|_|"无法安全保存登录，请检查 Windows 凭据管理器".into())
}
#[cfg(not(windows))]
fn save_session(_slot:&str,_value:&Session)->Result<(),String>{Err("当前系统暂不支持安全保存 Grok Bot 登录".into())}
#[cfg(windows)]
fn stored_session(slot:&str)->Result<Session,String>{
    let raw=credential(slot)?.get_password().map_err(|_|LOGIN_ERROR)?;
    let value:Value=serde_json::from_str(&raw).map_err(|_|LOGIN_ERROR)?;let session=session(&value)?;
    if slot_of(&session.token)?!=slot{return Err(LOGIN_ERROR.into());}Ok(session)
}
#[cfg(not(windows))]
fn stored_session(_slot:&str)->Result<Session,String>{Err(LOGIN_ERROR.into())}
pub fn forget(account:&Account){
    #[cfg(windows)]
    if account.source.as_os_str().is_empty(){if let Some(slot)=&account.slot{if let Ok(entry)=credential(slot){let _=entry.delete_credential();}}}
    #[cfg(not(windows))] let _=account;
}

fn read_json(response:ureq::Response)->Result<Value,String>{
    let mut body=Vec::new();response.into_reader().take(1024*1024+1).read_to_end(&mut body).map_err(|_|"读取 Grok Bot 响应失败")?;
    if body.len()>1024*1024{return Err("Grok Bot 响应过大".into());}
    serde_json::from_slice(&body).map_err(|_|"Grok Bot 未返回有效数据".into())
}
fn http_error(error:ureq::Error)->(String,u64){match error{
    ureq::Error::Status(401,_)=>("Grok Bot 登录已失效（401），请重新连接".into(),0),
    ureq::Error::Status(403,_)=>("Grok Bot 额度请求被拒绝（403），请检查账号权限或网络代理".into(),60),
    ureq::Error::Status(429,response)=>("请求过于频繁，稍后自动重试".into(),response.header("Retry-After").and_then(|s|s.parse().ok()).unwrap_or(180).clamp(30,3600)),
    ureq::Error::Status(code,_)=>(format!("Grok Bot 额度服务暂不可用（{code}）"),60),
    _=>("无法连接 Grok Bot，请检查网络或代理".into(),30),
}}
fn checksum(machine:&str,millis:i64)->String{
    let stamp=(millis/1_000_000) as u32;
    // 与客户端 JavaScript 的移位规则一致：移位数按 32 取模。
    let mut bytes=[(stamp>>8) as u8,stamp as u8,(stamp>>24) as u8,(stamp>>16) as u8,(stamp>>8) as u8,stamp as u8];
    let mut previous=165;
    for (i,byte) in bytes.iter_mut().enumerate(){*byte=(*byte^previous).wrapping_add(i as u8);previous=*byte;}
    format!("{}{machine}",URL_SAFE_NO_PAD.encode(bytes))
}
fn usage_request(agent:&ureq::Agent,url:&str,token:&str)->Result<ureq::Response,ureq::Error>{
    let request_id=uuid();
    agent.post(url).set("Authorization",&format!("Bearer {token}")).set("Content-Type","application/json").set("Accept","application/json")
        .set("Connect-Protocol-Version","1").set("x-cursor-client-type","sand").set("x-cursor-client-version","0.1.0")
        .set("x-cursor-checksum",&checksum(&request_id,chrono::Utc::now().timestamp_millis())).set("x-request-id",&request_id)
        .set("x-sand-box-namespace","prod").set("x-ghost-mode","true").send_string("{}")
}
fn renew(agent:&ureq::Agent,old:&Session)->Result<Session,String>{
    let refresh=old.refresh.as_deref().ok_or(LOGIN_ERROR)?;
    let response=agent.post(&format!("{BACKEND}/oauth/token")).set("Content-Type","application/json")
        .send_string(&json!({"client_id":CLIENT_ID,"grant_type":"refresh_token","refresh_token":refresh}).to_string()).map_err(|_|LOGIN_ERROR)?;
    renewed_session(old,&read_json(response)?)
}
fn renewed_session(old:&Session,value:&Value)->Result<Session,String>{
    let mut renewed=session(value)?;if renewed.refresh.is_none(){renewed.refresh=old.refresh.clone();}
    if slot_of(&renewed.token)?!=slot_of(&old.token)?{return Err("登录账号不一致，请重新连接".into());}Ok(renewed)
}
pub fn fetch(agent:&ureq::Agent,account:&Account)->Result<Snapshot,(String,u64)>{
    let own=account.source.as_os_str().is_empty();let slot=account.slot.as_deref().ok_or_else(||(LOGIN_ERROR.to_owned(),0))?;
    let mut auth=if own{stored_session(slot)}else{native_session(account)}.map_err(|e|(e,0))?;
    if own && auth.refresh.is_some() && claims(&auth.token).ok().and_then(|v|v["exp"].as_i64()).is_some_and(|exp|exp<=chrono::Utc::now().timestamp()+120){
        auth=renew(agent,&auth).map_err(|e|(e,0))?;save_session(slot,&auth).map_err(|e|(e,0))?;
    }
    let stamp=fs::metadata(&account.source).and_then(|m|m.modified()).ok();
    let url=format!("{BACKEND}/aiserver.v1.DashboardService/GetSandUsageStatus");let mut result=usage_request(agent,&url,&auth.token);
    if own && auth.refresh.is_some() && matches!(&result,Err(ureq::Error::Status(401,_))){
        auth=renew(agent,&auth).map_err(|e|(e,0))?;save_session(slot,&auth).map_err(|e|(e,0))?;result=usage_request(agent,&url,&auth.token);
    }
    let value=read_json(result.map_err(http_error)?).map_err(|e|(e,60))?;
    if !own && stamp!=fs::metadata(&account.source).and_then(|m|m.modified()).ok(){return Err(("Grok Bot 登录已更新，请刷新额度".into(),0));}
    parse(&value).map_err(|e|(e,60))
}

pub struct Connected {pub slot:String,pub label:String}
fn uuid()->String{
    // 请求标识无需保密，以纳秒时间和进程号生成，避免依赖系统随机源。
    let input=format!("{}:{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default());
    let hash=digest::digest(&digest::SHA256,input.as_bytes());let mut bytes=[0u8;16];bytes.copy_from_slice(&hash.as_ref()[..16]);
    bytes[6]=(bytes[6]&15)|64;bytes[8]=(bytes[8]&63)|128;
    let hex:String=bytes.iter().map(|b|format!("{b:02x}")).collect();format!("{}-{}-{}-{}-{}",&hex[..8],&hex[8..12],&hex[12..16],&hex[16..20],&hex[20..])
}
pub struct Login {rx:mpsc::Receiver<Result<Connected,String>>,cancel:Arc<AtomicBool>}
fn challenge(verifier:&str)->String{URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256,verifier.as_bytes()).as_ref())}
impl Login {
    pub fn start(alias:String)->Result<(Self,String),String>{
        let mut seed=[0u8;48];SystemRandom::new().fill(&mut seed).map_err(|_|"无法创建安全登录，请重试")?;
        let verifier=URL_SAFE_NO_PAD.encode(&seed[..32]);seed[38]=(seed[38]&15)|64;seed[40]=(seed[40]&63)|128;
        let hex:String=seed[32..].iter().map(|b|format!("{b:02x}")).collect();let id=format!("{}-{}-{}-{}-{}",&hex[..8],&hex[8..12],&hex[12..16],&hex[16..20],&hex[20..]);
        let url=format!("https://cursor.com/loginDeepControl?challenge={}&uuid={id}&mode=login&redirectTarget=sand&supportsSelectedTeamLogin=true",challenge(&verifier));
        let(tx,rx)=mpsc::channel();let cancel=Arc::new(AtomicBool::new(false));let stopped=cancel.clone();
        std::thread::spawn(move||{
            let agent=super::http_agent();let deadline=Instant::now()+Duration::from_secs(300);let mut delay=10;
            while Instant::now()<deadline && !stopped.load(Ordering::Relaxed){
                let result=agent.get(&format!("{BACKEND}/auth/poll?uuid={id}&verifier={verifier}")).set("Accept","application/json").set("Content-Type","application/json").call();
                match result{
                    Ok(response)=>{let result=read_json(response).and_then(|v|session(&v)).and_then(|auth|{
                        let slot=slot_of(&auth.token)?;if stopped.load(Ordering::Relaxed){return Err("登录已取消".into());}
                        save_session(&slot,&auth)?;let label=if alias.trim().is_empty(){claims(&auth.token).ok().and_then(|v|v["email"].as_str().map(|email|format!("Grok Bot · {email}"))).unwrap_or_default()}else{alias.clone()};
                        Ok(Connected{slot,label})
                    });let _=tx.send(result);return;},
                    Err(ureq::Error::Status(404,_))=>{},
                    Err(ureq::Error::Status(401|403,_))=>{let _=tx.send(Err("登录未获授权，请重试".into()));return;},
                    Err(_)=>{},
                }
                for _ in 0..delay{if stopped.load(Ordering::Relaxed){return;}std::thread::sleep(Duration::from_millis(100));}delay=(delay+5).min(50);
            }
            if !stopped.load(Ordering::Relaxed){let _=tx.send(Err("登录等待超时，请重试".into()));}
        });
        Ok((Self{rx,cancel},url))
    }
    pub fn poll(&self)->Option<Result<Connected,String>>{match self.rx.try_recv(){Ok(result)=>Some(result),Err(mpsc::TryRecvError::Disconnected)=>Some(Err("登录连接已结束，请重试".into())),Err(mpsc::TryRecvError::Empty)=>None}}
}
impl Drop for Login {fn drop(&mut self){self.cancel.store(true,Ordering::Relaxed);}}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quota_preserves_zero_fractional_usage_and_real_reset(){
        let mut value=json!({"usagePercent":19.150778,"hasNonZeroIncludedLimit":true,"currentPeriodStart":"2030-01-01T10:00:00Z","nextResetTimestampUtc":"2030-01-08T10:00:00Z","grokPlanLabel":"Super"});
        let s=parse(&value).unwrap();assert!((s.windows[0].remaining-80.84922).abs()<0.001);assert_eq!(s.windows[0].seconds,Some(604800));assert_eq!(s.windows.len(),1);assert_eq!(s.plan,"Super");
        value["usagePercent"]=json!(0);assert_eq!(parse(&value).unwrap().windows[0].remaining,100.0);
        value["usagePercent"]=json!("100");assert_eq!(parse(&value).unwrap().windows[0].remaining,0.0);
        value["isUnified"]=json!(true);assert_eq!(parse(&value).unwrap().windows[0].label,"共享周额度");
    }
    #[test]
    fn missing_or_invalid_readings_are_never_an_unused_allowance(){
        for value in [Value::Null,json!({}),json!({"usagePercent":null}),json!({"usagePercent":"NaN"}),json!({"usagePercent":-1}),json!({"usagePercent":101}),json!({"usagePercent":0,"hasNonZeroIncludedLimit":false}),json!({"usagePercent":0,"usesPooledEnterpriseAllowance":true})]{assert!(parse(&value).is_err());}
        let s=parse(&json!({"usagePercent":20})).unwrap();assert_eq!(s.windows[0].reset,None);assert_eq!(s.windows[0].seconds,None);
    }
    #[test]
    fn account_slots_survive_string_wrapping_and_do_not_follow_the_active_slot(){
        let value=json!({"active":"work","accounts":{"personal":{"cursor-access-token":"one"},"work":{"cursor-access-token":"two"}}});
        for wrapped in [value.clone(),Value::String(value.to_string())]{let(active,rows)=accounts(&serde_json::to_vec(&json!({"cursor-accounts":wrapped})).unwrap()).unwrap();assert_eq!(active.as_deref(),Some("work"));assert_eq!(rows["personal"]["cursor-access-token"],"one");assert_eq!(rows.len(),2);}
    }
    #[test]
    fn windows_envelope_requires_the_correct_key_scope_and_authentication_tag(){
        let key=[17u8;32];let nonce=[7u8;12];let secret=b"synthetic-token";
        let aead=aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM,&key).unwrap());
        let mut sealed=secret.to_vec();aead.seal_in_place_append_tag(aead::Nonce::assume_unique_for_key(nonce),aead::Aad::empty(),&mut sealed).unwrap();
        let mut blob=b"v10".to_vec();blob.extend_from_slice(&nonce);blob.extend_from_slice(&sealed);let encoded=STANDARD.encode(&blob);
        assert_eq!(decrypt(&key,&encoded,"account").unwrap(),"synthetic-token");
        assert_eq!(decrypt(&key,&format!("scoped:v1:account:{encoded}"),"account").unwrap(),"synthetic-token");
        assert!(decrypt(&key,&format!("scoped:v1:other:{encoded}"),"account").is_err());assert!(decrypt(&[18u8;32],&encoded,"account").is_err());
        *blob.last_mut().unwrap()^=1;let error=decrypt(&key,&STANDARD.encode(blob),"account").unwrap_err();assert_eq!(error,LOGIN_ERROR);assert!(!error.contains("synthetic-token"));
    }
    #[cfg(windows)]
    #[test]
    fn windows_native_accounts_use_their_own_slots_and_leave_the_client_files_unchanged(){
        use windows::{core::PCWSTR,Win32::{Foundation::{HLOCAL,LocalFree},Security::Cryptography::{CryptProtectData,CRYPT_INTEGER_BLOB,CRYPTPROTECT_UI_FORBIDDEN}}};
        let key=[23u8;32];let input=CRYPT_INTEGER_BLOB{cbData:32,pbData:key.as_ptr().cast_mut()};let mut output=CRYPT_INTEGER_BLOB::default();
        unsafe{CryptProtectData(&input,PCWSTR::null(),None,None,None,CRYPTPROTECT_UI_FORBIDDEN,&mut output).unwrap();}
        let mut protected=b"DPAPI".to_vec();protected.extend_from_slice(unsafe{std::slice::from_raw_parts(output.pbData,output.cbData as usize)});
        unsafe{let _=LocalFree(HLOCAL(output.pbData.cast()));}
        let dir=std::env::temp_dir().join(format!("daozai-grok-dpapi-{}-{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap()));fs::create_dir(&dir).unwrap();
        fs::write(dir.join("Local State"),json!({"os_crypt":{"encrypted_key":STANDARD.encode(protected)}}).to_string()).unwrap();
        assert_eq!(native_key(&dir.join("sand-secrets.json")).unwrap(),key);
        let token=|subject:&str|format!("e30.{}.test",URL_SAFE_NO_PAD.encode(json!({"sub":subject}).to_string()));
        let personal=token("synthetic-personal");let work=token("synthetic-work");let personal_slot=slot_of(&personal).unwrap();let work_slot=slot_of(&work).unwrap();
        let seal=|text:&str|{let aead=aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM,&key).unwrap());let nonce=[3u8;12];let mut sealed=text.as_bytes().to_vec();aead.seal_in_place_append_tag(aead::Nonce::assume_unique_for_key(nonce),aead::Aad::empty(),&mut sealed).unwrap();let mut blob=b"v10".to_vec();blob.extend_from_slice(&nonce);blob.extend_from_slice(&sealed);STANDARD.encode(blob)};
        let path=dir.join("sand-secrets.json");let raw=json!({"cursor-accounts":json!({"active":work_slot,"accounts":{&personal_slot:{"cursor-access-token":seal(&personal)},&work_slot:{"cursor-access-token":seal(&work)}}}).to_string()}).to_string();fs::write(&path,&raw).unwrap();
        let account=Account{id:"synthetic".into(),provider:super::super::Provider::GrokBot,alias:"个人".into(),source:path.clone(),slot:Some(personal_slot),enabled:true,pinned:true};
        assert_eq!(native_session(&account).unwrap().token,personal);assert_eq!(fs::read_to_string(&path).unwrap(),raw);
        fs::remove_file(path).unwrap();fs::remove_file(dir.join("Local State")).unwrap();fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn browser_login_uses_the_rfc7636_challenge(){
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),"E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }
    #[test]
    fn request_checksum_matches_the_client_timestamp_encoding(){
        assert_eq!(checksum("synthetic-machine",1_790_000_000_000),"9cbI1oq_synthetic-machine");
    }
    #[test]
    fn forbidden_requests_are_not_reported_as_expired_credentials(){
        let response=ureq::Response::new(403,"Forbidden","private-response").unwrap();
        let(message,wait)=http_error(ureq::Error::Status(403,response));
        assert!(message.contains("403"));assert!(!message.contains("重新连接"));assert!(!message.contains("private-response"));assert!(wait>0);
    }
    #[test]
    fn renewal_keeps_the_same_account_and_preserves_or_rotates_its_refresh_token(){
        let token=|sub:&str,exp:i64|format!("e30.{}.test",URL_SAFE_NO_PAD.encode(json!({"sub":sub,"exp":exp}).to_string()));
        let old=Session{token:token("synthetic",1),refresh:Some("old-refresh".into())};
        let renewed=renewed_session(&old,&json!({"access_token":token("synthetic",2)})).unwrap();assert_eq!(renewed.refresh.as_deref(),Some("old-refresh"));
        let renewed=renewed_session(&old,&json!({"access_token":token("synthetic",2),"refresh_token":"rotated"})).unwrap();assert_eq!(renewed.refresh.as_deref(),Some("rotated"));
        assert!(renewed_session(&old,&json!({"access_token":token("other",2),"refresh_token":"wrong-account"})).is_err());assert!(renewed_session(&old,&json!({})).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn independent_login_round_trips_through_windows_credentials_and_can_be_removed(){
        struct Cleanup(keyring::Entry);
        impl Drop for Cleanup{fn drop(&mut self){let _=self.0.delete_credential();}}
        let subject=format!("synthetic-daozai-{}-{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap());
        let token=format!("e30.{}.test",URL_SAFE_NO_PAD.encode(json!({"sub":subject}).to_string()));let slot=slot_of(&token).unwrap();let _cleanup=Cleanup(credential(&slot).unwrap());
        let auth=Session{token,refresh:Some("synthetic-refresh".into())};save_session(&slot,&auth).unwrap();let restored=stored_session(&slot).unwrap();assert_eq!(restored.token,auth.token);assert_eq!(restored.refresh,auth.refresh);
        let account=Account{id:"synthetic".into(),provider:super::super::Provider::GrokBot,alias:"测试".into(),source:PathBuf::new(),slot:Some(slot.clone()),enabled:true,pinned:true};forget(&account);assert!(stored_session(&slot).is_err());
    }
    #[test]
    fn request_is_a_read_only_rpc_and_errors_never_echo_the_body(){
        use std::io::{BufRead,BufReader,Read,Write};use std::net::TcpListener;
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();let url=format!("http://{}/aiserver.v1.DashboardService/GetSandUsageStatus",listener.local_addr().unwrap());
        let server=std::thread::spawn(move||{
            let(mut stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
            // 读完请求再关闭连接，避免分包时未读完的正文触发 TCP 重置。
            let mut reader=BufReader::new(&mut stream);let mut request=String::new();
            loop{let mut line=String::new();assert!(reader.read_line(&mut line).unwrap()>0);if line=="\r\n"{break;}request.push_str(&line);}
            let request=request.to_ascii_lowercase();
            let length=request.lines().find_map(|line|line.strip_prefix("content-length:").map(|v|v.trim().parse::<usize>().unwrap())).unwrap();
            let mut body=vec![0;length];reader.read_exact(&mut body).unwrap();assert_eq!(body,b"{}");
            assert!(request.starts_with("post /aiserver.v1.dashboardservice/getsandusagestatus "));assert!(request.contains("authorization: bearer synthetic-token"));assert!(request.contains("connect-protocol-version: 1"));
            assert!(request.contains("x-cursor-checksum: "));assert!(request.contains("x-request-id: "));
            stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 24\r\nConnection: close\r\n\r\nsecret-that-must-not-echo").unwrap();
        });
        let error=usage_request(&ureq::AgentBuilder::new().build(),&url,"synthetic-token").unwrap_err();let(message,wait)=http_error(error);assert!(message.contains("401"));assert!(!message.contains("secret-that-must-not-echo"));assert_eq!(wait,0);server.join().unwrap();
    }
}
