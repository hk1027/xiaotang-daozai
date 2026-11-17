//! WorkBuddy 官方客户端积分汇总，浏览器授权连接。
use super::{Account,Snapshot};
use serde_json::Value;
use std::{io::Read,sync::mpsc,time::Duration};
#[derive(Clone,Debug)]
#[derive(serde::Serialize,serde::Deserialize)]
pub struct Credits {pub remaining:f64,pub packages:Vec<(String,f64)>}
impl Credits {
    pub fn compact(&self)->String{super::deepseek::Balance{currency:"CNY".into(),total:self.remaining,granted:0.0,topped_up:0.0}.compact().trim_start_matches('¥').to_owned()}
    pub fn full(&self)->String{let text=super::deepseek::Balance{currency:"CNY".into(),total:self.remaining,granted:0.0,topped_up:0.0}.full(self.remaining);text.trim_start_matches('¥').trim_end_matches('0').trim_end_matches('.').to_owned()}
}
pub fn parse(value:&Value)->Result<Snapshot,String>{
    if value["code"].as_i64()!=Some(0){return Err("WorkBuddy 积分接口暂未返回结果，请稍后重试".into());}
    let data=&value["data"];let rows=data["Packages"].as_array().ok_or("积分接口缺少资源包数据")?;
    let mut packages=Vec::new();let mut remaining=0.0;
    for row in rows{
        let amount=row["CycleRemainCapacity"].as_f64().or_else(||row["CycleRemainCapacity"].as_str().and_then(|s|s.parse().ok())).filter(|n|n.is_finite()&&*n>=0.0).ok_or("积分余额字段无效")?;
        remaining+=amount;packages.push((row["PackageCode"].as_str().unwrap_or_default().into(),amount));
    }
    if !remaining.is_finite(){return Err("积分余额超出范围".into());}
    if let Some(groups)=data["CreditGroups"].as_array(){
        packages.clear();for group in groups{
            let label=match group["Kind"].as_str(){Some("plan_base")=>"套餐",Some("plan_bonus")=>"加赠",Some("addon")=>"加量包",Some("benefit")=>"赠送",_=>continue};
            if let Some(amount)=group["CycleRemainCapacity"].as_f64().or_else(||group["CycleRemainCapacity"].as_str().and_then(|s|s.parse::<f64>().ok())).filter(|n|n.is_finite()&&*n>0.0){packages.push((label.into(),amount));}
        }
    }else{packages.clear();}
    Ok(Snapshot{plan:data["SubscriptionPackageName"].as_str().unwrap_or_default().into(),credits:Some(Credits{remaining,packages}),at:chrono::Utc::now().timestamp(),..Default::default()})
}
fn valid_origin(origin:&str)->bool{matches!(origin,"https://www.codebuddy.cn"|"https://www.workbuddy.cn")}
fn slot(uid:&str)->String{ring::digest::digest(&ring::digest::SHA256,uid.as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect()}
#[derive(serde::Serialize,serde::Deserialize)]
struct Session { origin:String, ua:String, cookie:String, #[serde(default)] auth:Option<Auth> }
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(rename_all="camelCase")]
struct Auth{
    access_token:String,
    #[serde(default)] refresh_token:String,
    #[serde(default)] domain:String,
    #[serde(default)] uid:String,
}

fn session_path(id:&str)->Result<std::path::PathBuf,String>{
    if id.len()!=64 || !id.bytes().all(|c|c.is_ascii_hexdigit()){return Err("WorkBuddy 账号标识无效".into());}
    Ok(crate::model::config_dir().join("credentials").join(format!("workbuddy-{id}.bin")))
}
fn save_session(id:&str,session:&Session)->Result<(),String>{
    let path=session_path(id)?;
    let bytes=serde_json::to_vec(session).map_err(|_|"登录会话无效")?;
    let encrypted=super::gemini::protect(&bytes,true)?;
    std::fs::create_dir_all(path.parent().unwrap()).and_then(|_|crate::model::atomic_write(&path,&encrypted)).map_err(|_|"无法保存 WorkBuddy 登录会话".into())
}
pub fn forget(account:&Account){
    if let Some(path)=account.slot.as_deref().and_then(|id|session_path(id).ok()){let _=std::fs::remove_file(path);}
}
const SUMMARY:&str="/billing/meter/get-user-resource-summary";
enum Failure { Transport, Expired, Message(String) }
impl Failure {
    fn message(self)->String{match self{Self::Transport=>"暂时无法连接 WorkBuddy 官网".into(),Self::Expired=>"官网登录已过期，请重新连接 WorkBuddy".into(),Self::Message(s)=>s}}
}
fn api(agent:&ureq::Agent,session:&Session,path:&str)->Result<Value,Failure>{
    let url=format!("{}{path}",session.origin);
    let mut req=agent.request(if path==SUMMARY{"POST"}else{"GET"},&url)
        .set("User-Agent",&session.ua).set("Cookie",&session.cookie).set("Accept","application/json").set("X-Client-Platform","web");
    if let Some(auth)=&session.auth{
        req=req.set("Authorization",&format!("Bearer {}",auth.access_token)).set("X-User-Id",&auth.uid).set("X-Domain",&auth.domain);
    }
    let response=if path==SUMMARY{req.set("Content-Type","application/json").send_string("{}")}else{req.call()}.map_err(|e|match e{
        ureq::Error::Status(401,_)=>Failure::Expired,
        ureq::Error::Status(code,_)=>Failure::Message(format!("WorkBuddy 官网返回 {code}，请稍后重试")),
        _=>Failure::Transport
    })?;
    serde_json::from_reader(response.into_reader().take(2*1024*1024)).map_err(|_|Failure::Message("WorkBuddy 官网返回了无效数据".into()))
}
fn verified(agent:&ureq::Agent,s:&Session,id:&str)->Result<Snapshot,Failure>{
    if let Some(auth)=&s.auth{
        if slot(&auth.uid)!=id{return Err(Failure::Message("WorkBuddy 授权账号不匹配".into()));}
        return parse(&api(agent,s,SUMMARY)?).map_err(Failure::Message);
    }
    let profile=api(agent,s,"/console/account")?;
    let uid=profile.pointer("/data/uid").and_then(Value::as_str).ok_or_else(||Failure::Message("官网未返回 WorkBuddy 账号信息".into()))?;
    if slot(uid)!=id || profile.pointer("/data/isCurrentOneIdEnterprise")==Some(&Value::Bool(true)){
        return Err(Failure::Message("官网账号与已绑定的个人账号不一致".into()));
    }
    parse(&api(agent,s,SUMMARY)?).map_err(Failure::Message)
}
fn query(agent:&ureq::Agent,s:&Session,id:&str)->Result<Snapshot,Failure>{
    if !valid_origin(&s.origin){return Err(Failure::Message("WorkBuddy 登录来源无效".into()));}
    with_transport_retry(agent,&s.origin,|agent|verified(agent,s,id))
}
fn with_transport_retry<T>(agent:&ureq::Agent,origin:&str,request:impl Fn(&ureq::Agent)->Result<T,Failure>)->Result<T,Failure>{
    let result=request(agent);
    if !matches!(result,Err(Failure::Transport)){return result;}
    // 仅在本地代理映射到基准测试地址且连接失败时重新解析；TLS 仍校验原官网域名。
    use std::net::{Ipv4Addr,SocketAddr,ToSocketAddrs};
    let host=origin.trim_start_matches("https://");
    let mapped=(host,443).to_socket_addrs().ok().is_some_and(|mut addresses|addresses.any(|a|matches!(a,SocketAddr::V4(v) if v.ip().octets()[0]==198 && matches!(v.ip().octets()[1],18|19))));
    if !mapped{return result;}
    let lookup=format!("https://dns.alidns.com/resolve?name={host}&type=A");
    let dns=agent.get(&lookup).call().map_err(|_|Failure::Transport)?;
    let data:Value=serde_json::from_reader(dns.into_reader().take(65536)).map_err(|_|Failure::Transport)?;
    let ips:Vec<_>=data["Answer"].as_array().into_iter().flatten().filter(|r|r["type"]==1).filter_map(|r|r["data"].as_str()?.parse::<Ipv4Addr>().ok()).map(|ip|SocketAddr::from((ip,443))).collect();
    if ips.is_empty(){return result;}
    let endpoint=format!("{host}:443");
    let direct=ureq::AgentBuilder::new().try_proxy_from_env(false).timeout(Duration::from_secs(12)).redirects(0)
        .resolver(move |name:&str|if name==endpoint{Ok(ips.clone())}else{Err(std::io::Error::other("官网地址不匹配"))}).build();
    request(&direct)
}
// 只读取客户端当前会话；未知加密格式交给独立官网登录，不改写客户端文件。
fn local_snapshot(agent:&ureq::Agent,expected:Option<&str>)->Result<(String,Snapshot),String>{
    let base=std::env::var_os("LOCALAPPDATA").ok_or("未找到本机用户目录")?;
    let path=std::path::PathBuf::from(base).join("CodeBuddyExtension/Data/Public/auth/workbuddy-desktop-ai.info");
    if path.with_extension("info.logged-out").exists(){return Err("WorkBuddy 客户端已退出登录".into());}
    let raw=std::fs::read(&path).map_err(|_|"未找到 WorkBuddy 客户端登录")?;
    let root:Value=serde_json::from_slice(&raw).map_err(|_|"无法读取 WorkBuddy 客户端登录")?;
    let uid=root.pointer("/account/uid").and_then(Value::as_str).ok_or("客户端尚未登录")?;
    let id=slot(uid);
    if expected.is_some_and(|s|s!=id){return Err("客户端当前账号与已绑定账号不同".into());}
    if root.pointer("/account/isCurrentOneIdEnterprise")==Some(&Value::Bool(true)){return Err("请使用 WorkBuddy 个人账号".into());}
    let token=root.pointer("/auth/accessToken").and_then(Value::as_str).filter(|s|!s.is_empty()).ok_or("客户端登录已加密，请通过官网连接")?;
    let domain=root.pointer("/auth/domain").and_then(Value::as_str).unwrap_or_default();
    let response=agent.post("https://www.codebuddy.cn/billing/meter/get-user-resource-summary")
        .set("Authorization",&format!("Bearer {token}")).set("X-User-Id",uid).set("X-Domain",domain)
        .set("Content-Type","application/json").send_string("{}").map_err(|_|"暂时无法使用客户端登录查询积分")?;
    let value:Value=serde_json::from_reader(response.into_reader().take(2*1024*1024)).map_err(|_|"客户端积分响应无效")?;
    Ok((id,parse(&value)?))
}
fn refresh_auth(agent:&ureq::Agent,session:&Session)->Result<Auth,Failure>{
    let auth=session.auth.as_ref().ok_or(Failure::Expired)?;
    let response=with_transport_retry(agent,&session.origin,|agent|{
        agent.post(&format!("{}/v2/plugin/auth/token/refresh",session.origin))
            .set("X-Refresh-Token",&auth.refresh_token).set("X-Auth-Refresh-Source","plugin")
            .set("X-Domain",&auth.domain).set("Content-Type","application/json").send_string("{}")
            .map_err(|e|match e{
                ureq::Error::Transport(_)=>Failure::Transport,
                ureq::Error::Status(401|403,_)=>Failure::Expired,
                _=>Failure::Message("WorkBuddy 登录服务暂不可用，请稍后重试".into()),
            })
    })?;
    let value:Value=serde_json::from_reader(response.into_reader().take(2*1024*1024)).map_err(|_|Failure::Message("WorkBuddy 刷新响应无效".into()))?;
    let mut next:Auth=serde_json::from_value(value["data"].clone()).map_err(|_|Failure::Message("WorkBuddy 授权已失效，请重新连接".into()))?;
    next.uid=auth.uid.clone();
    if next.refresh_token.is_empty(){next.refresh_token=auth.refresh_token.clone();}
    if next.domain.is_empty(){next.domain=auth.domain.clone();}
    if next.access_token.is_empty(){return Err(Failure::Message("WorkBuddy 未返回有效刷新凭据".into()));}
    Ok(next)
}
pub fn fetch(agent:&ureq::Agent,account:&Account)->Result<Snapshot,(String,u64)>{
    let id=account.slot.as_deref().ok_or(("请连接 WorkBuddy 浏览器账号".into(),0))?;
    if let Ok((_,snapshot))=local_snapshot(agent,Some(id)){return Ok(snapshot);}
    let path=session_path(id).map_err(|e|(e,0))?;
    match std::fs::read(&path){
        Ok(bytes)=>{
            let raw=super::gemini::protect(&bytes,false).map_err(|e|(e,30))?;
            let mut session:Session=serde_json::from_slice(&raw).map_err(|_|("保存的 WorkBuddy 会话无效，请重新连接".into(),0))?;
            match query(agent,&session,id){
                Ok(snapshot)=>return Ok(snapshot),
                Err(Failure::Expired)=>{
                    if session.auth.as_ref().is_some_and(|a|!a.refresh_token.is_empty()){
                        match refresh_auth(agent,&session){
                            Ok(next)=>{
                                session.auth=Some(next);
                                // 刷新令牌可能轮换，先保存，再查询积分，避免网络中断丢失新令牌。
                                if std::fs::read(&path).ok().as_deref()==Some(bytes.as_slice()){
                                    save_session(id,&session).map_err(|e|(e,30))?;
                                }
                                return query(agent,&session,id).map_err(|e|(e.message(),30));
                            },
                            Err(Failure::Expired)=>{},
                            Err(e)=>return Err((e.message(),30)),
                        }
                    }
                },
                Err(e)=>return Err((e.message(),30))
            }
        },
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
        Err(_)=>return Err(("无法读取已保存的 WorkBuddy 会话".into(),30))
    }
    Err(("WorkBuddy 登录已过期或尚未连接，请点击重新连接".into(),60))
}
#[cfg(windows)]
#[path="workbuddy_login.rs"]
mod login;

pub struct Connected {pub slot:String,pub label:String,pub snapshot:Snapshot}
pub struct Connect {rx:mpsc::Receiver<Result<Connected,String>>,cancel:std::sync::Arc<std::sync::atomic::AtomicBool>}
impl Drop for Connect {fn drop(&mut self){self.cancel.store(true,std::sync::atomic::Ordering::Relaxed);}}
impl Connect {
    pub fn start(alias:String,expected:Option<String>)->Self{
        let(tx,rx)=mpsc::channel();
        let cancel=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_cancel=cancel.clone();
        std::thread::spawn(move||{let result=(||{
            let local=local_snapshot(&super::http_agent(),expected.as_deref());
            #[cfg(windows)]
            let (slot,snapshot)=match local{
                Ok(result)=>result,
                Err(_)=>login::connect(expected.as_deref(),worker_cancel)?,
            };
            #[cfg(not(windows))]
            let (slot,snapshot):(String,Snapshot)=return Err("WorkBuddy 登录暂仅支持 Windows".into());
            Ok(Connected{slot,label:if alias.trim().is_empty(){"WorkBuddy".into()}else{alias.trim().into()},snapshot})
        })();let _=tx.send(result);});Self{rx,cancel}
    }
    pub fn poll(&self)->Option<Result<Connected,String>>{self.rx.try_recv().ok()}
}
#[cfg(test)]mod tests{
    use super::*;use serde_json::json;
    #[test]
    fn workbuddy_old_cookie_sessions_remain_readable(){
        let session:Session=serde_json::from_value(json!({"origin":"https://www.codebuddy.cn","ua":"test","cookie":"session=test"})).unwrap();
        assert!(session.auth.is_none());
        assert_eq!(session.cookie,"session=test");
    }
    #[test]
    #[ignore="手动验证默认浏览器官网登录，需用户完成登录"]
    fn workbuddy_login_without_extension(){
        let config=super::super::Config::load();
        let expected=config.accounts.iter().find(|a|a.enabled&&a.provider==super::super::Provider::WorkBuddy).and_then(|a|a.slot.as_deref());
        let cancel=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result=if let Ok(state)=std::env::var("DAOZAI_TEST_WORKBUDDY_STATE"){
            login::wait_for_login(&super::super::http_agent(),&state,expected,cancel)
        }else{login::connect(expected,cancel)};
        let (id,snapshot)=result.expect("官网登录");
        assert!(snapshot.credits.is_some());
        let bytes=std::fs::read(session_path(&id).unwrap()).unwrap();
        let session:Session=serde_json::from_slice(&super::super::gemini::protect(&bytes,false).unwrap()).unwrap();
        assert!(query(&super::super::http_agent(),&session,&id).map_err(Failure::message).unwrap().credits.is_some());
    }
    #[test]
    #[ignore="手动验证官网登录续期并保存新会话"]
    fn workbuddy_refresh_saved_login(){
        let config=super::super::Config::load();
        let a=config.accounts.iter().find(|a|a.enabled&&a.provider==super::super::Provider::WorkBuddy).unwrap();
        let id=a.slot.as_deref().unwrap();
        let bytes=std::fs::read(session_path(id).unwrap()).unwrap();
        let mut session:Session=serde_json::from_slice(&super::super::gemini::protect(&bytes,false).unwrap()).unwrap();
        let agent=super::super::http_agent();
        session.auth=Some(refresh_auth(&agent,&session).map_err(Failure::message).unwrap());
        save_session(id,&session).unwrap();
        assert!(fetch(&agent,a).unwrap().credits.is_some());
    }
    #[test]
    #[ignore="只使用已保存的官网会话验证积分，不连接浏览器"]
    fn workbuddy_saved_session_without_browser(){
        let config=super::super::Config::load();
        let a=config.accounts.iter().find(|a|a.enabled&&a.provider==super::super::Provider::WorkBuddy).unwrap();
        let id=a.slot.as_deref().unwrap();
        let bytes=std::fs::read(session_path(id).unwrap()).unwrap();
        let s:Session=serde_json::from_slice(&super::super::gemini::protect(&bytes,false).unwrap()).unwrap();
        let snapshot=query(&super::super::http_agent(),&s,id).map_err(Failure::message).unwrap();
        assert!(snapshot.credits.is_some());
    }
    #[test]fn workbuddy_sums_valid_packages_and_rejects_missing_balance(){
        let s=parse(&json!({"code":0,"data":{"Packages":[{"PackageCode":"base","CycleRemainCapacity":"1200"},{"PackageCode":"gift","CycleRemainCapacity":80}]}})).unwrap();let c=s.credits.unwrap();assert_eq!(c.remaining,1280.0);assert_eq!(c.compact(),"1.28k");assert_eq!(c.full(),"1,280");
        for v in [json!({"code":0}),json!({"code":401}),json!({"code":0,"data":{"Packages":[{}]}}),json!({"code":0,"data":{"Packages":[{"CycleRemainCapacity":-1}]}})]{assert!(parse(&v).is_err());}
        assert_eq!(parse(&json!({"code":0,"data":{"Packages":[]}})).unwrap().credits.unwrap().remaining,0.0);
    }
}
