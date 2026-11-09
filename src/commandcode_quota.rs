//! Command Code 订阅额度：API Key 存 Windows 凭据管理器，只读 5 小时与每周滚动窗口。
use super::{Account,Snapshot,Window};
use serde_json::Value;
use std::{io::Read,sync::mpsc};

const BASE:&str="https://api.commandcode.ai";
const LOGIN_ERROR:&str="Command Code 登录已失效，请重新连接";

fn plan_label(id:&str)->String{
    let raw=id.strip_prefix("individual-").or_else(||id.strip_prefix("team-")).unwrap_or(id);
    match raw{"goat"=>"GOAT".into(),"go"=>"Go".into(),"pro"=>"Pro".into(),other=>other.to_owned()}
}
/// 官方 CLI 内置的套餐额度表；表里没有的套餐不猜总额。
fn plan_pool(id:&str)->Option<f64>{
    Some(match id{
        "individual-go"|"individual-go-v1"=>10.0,
        "individual-goat"=>70.0,
        "individual-pro"=>30.0,
        "individual-pro-v1"=>80.0,
        "individual-provider"=>15.0,
        "individual-max"=>150.0,
        "individual-ultra"=>300.0,
        "teams-pro"=>40.0,
        _=>return None,
    })
}
pub(super) fn parse(value:&Value)->Result<Snapshot,String>{
    let limits=value.get("windowLimits").ok_or("Command Code 尚未返回额度窗口")?;
    let mut windows=Vec::new();
    for (key,label,seconds) in [("fiveHour","5 小时",18000i64),("weekly","本周",604800)]{
        let window=&limits[key];
        let Some(used)=super::number(&window["used"]) else{continue;};
        let Some(cap)=super::number(&window["cap"]).filter(|n|n.is_finite()&&*n>0.0) else{continue;};
        let reset=window["resetAt"].as_i64().map(|ms|ms/1000).filter(|t|*t>0);
        windows.push(Window{label:label.into(),seconds:Some(seconds),remaining:((1.0-used/cap)*100.0).clamp(0.0,100.0) as f32,reset});
    }
    if windows.is_empty(){return Err("Command Code 未返回有效的额度窗口".into());}
    Ok(Snapshot{at:chrono::Utc::now().timestamp(),windows,..Default::default()})
}
fn get(agent:&ureq::Agent,path:&str,key:&str)->Result<Value,(String,u64)>{
    let response=agent.get(&format!("{BASE}{path}")).set("Authorization",&format!("Bearer {key}")).set("Accept","application/json").call().map_err(|e|match e{
        ureq::Error::Status(401|403,_)=>(LOGIN_ERROR.to_string(),0),
        ureq::Error::Status(429,response)=>("请求过于频繁，稍后自动重试".into(),response.header("Retry-After").and_then(|s|s.parse().ok()).unwrap_or(180).clamp(30,3600)),
        ureq::Error::Status(code,_)=>(format!("Command Code 额度服务暂不可用（{code}）"),60),
        _=>("无法连接 Command Code，请检查网络或代理".into(),30)
    })?;
    let mut body=String::new();
    response.into_reader().take(1024*1024).read_to_string(&mut body).map_err(|_|("读取 Command Code 响应失败".into(),30))?;
    serde_json::from_str(&body).map_err(|_|("Command Code 返回了无效数据".into(),60))
}
fn fetch_with_key(agent:&ureq::Agent,key:&str)->Result<Snapshot,String>{
    let credits=get(agent,"/alpha/billing/credits",key).map_err(|e|e.0)?;
    let mut snapshot=parse(&credits)?;
    // 套餐名与月度额度只用于展示，订阅查询失败不影响两个滚动窗口。
    if let Ok(subscription)=get(agent,"/alpha/billing/subscriptions",key){
        if let Some(id)=subscription.pointer("/data/planId").and_then(Value::as_str){
            snapshot.plan=plan_label(id);
            if let (Some(pool),Some(left),Some(week))=(
                plan_pool(id),
                super::number(&credits["credits"]["monthlyCredits"]),
                super::number(&credits["windowLimits"]["weekly"]["used"]),
            ){
                // monthlyCredits 是剩余量；用「周已用 + 剩余 = 套餐总额」反查，表过期就不显示本月。
                if pool>0.0 && (pool-left-week).abs()<0.02 {
                    let reset=subscription.pointer("/data/currentPeriodEnd").and_then(Value::as_str)
                        .and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()).map(|t|t.timestamp());
                    snapshot.windows.push(Window{label:"本月".into(),seconds:None,remaining:(left/pool*100.0).clamp(0.0,100.0) as f32,reset});
                }
            }
        }
    }
    Ok(snapshot)
}
fn slot_of(key:&str)->String{ring::digest::digest(&ring::digest::SHA256,key.as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect()}
#[cfg(windows)]
fn credential(slot:&str)->Result<keyring::Entry,String>{keyring::Entry::new("Daozai.CommandCode",slot).map_err(|_|"无法打开 Windows 凭据管理器".into())}
#[cfg(windows)]
fn save(slot:&str,key:&str)->Result<(),String>{credential(slot)?.set_password(key).map_err(|_|"无法安全保存 API Key".into())}
#[cfg(not(windows))]
fn save(_slot:&str,_key:&str)->Result<(),String>{Err("当前系统不支持安全保存 API Key".into())}
#[cfg(windows)]
fn load(slot:&str)->Result<String,String>{credential(slot)?.get_password().map_err(|error|match error {keyring::Error::NoEntry=>"未找到已保存的 Command Code API Key，请重新连接".into(),_=>"暂时无法读取 Windows 凭据，请稍后重试".into()})}
#[cfg(not(windows))]
fn load(_slot:&str)->Result<String,String>{Err("当前系统不支持读取 API Key".into())}
pub fn forget(account:&Account){
    #[cfg(windows)]
    if let Some(slot)=&account.slot{if let Ok(entry)=credential(slot){let _=entry.delete_credential();}}
    #[cfg(not(windows))] let _=account;
}
pub fn fetch(agent:&ureq::Agent,account:&Account)->Result<Snapshot,(String,u64)>{
    let slot=account.slot.as_deref().ok_or(("请连接 Command Code API Key".into(),0))?;
    let key=load(slot).map_err(|e|(e,0))?;
    if slot_of(&key)!=slot{return Err(("保存的 API Key 与账号不匹配，请重新连接".into(),0));}
    fetch_with_key(agent,&key).map_err(|e|(e,60))
}
pub struct Connected {pub slot:String,pub label:String,pub snapshot:Snapshot}
pub struct Connect {rx:mpsc::Receiver<Result<Connected,String>>}
impl Connect {
    pub fn start(alias:String,key:String)->Self{
        let(tx,rx)=mpsc::channel();std::thread::spawn(move||{let result=(||{
            let key=key.trim();if key.is_empty()||key.chars().any(char::is_whitespace){return Err("请输入有效的 API Key".into());}
            let snapshot=fetch_with_key(&super::http_agent(),key)?;
            let slot=slot_of(key);save(&slot,key)?;
            Ok(Connected{slot,label:if alias.trim().is_empty(){"Command Code".into()}else{alias.trim().into()},snapshot})
        })();let _=tx.send(result);});Self{rx}
    }
    pub fn poll(&self)->Option<Result<Connected,String>>{self.rx.try_recv().ok()}
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test] fn commandcode_windows_convert_used_and_cap_to_remaining(){
        let s=parse(&json!({"windowLimits":{"fiveHour":{"used":0.72,"cap":14,"resetAt":1794244287531i64},"weekly":{"used":13.56,"cap":35,"resetAt":1794483171196i64}}})).unwrap();
        assert_eq!(s.windows.len(),2);
        assert_eq!(s.windows[0].label,"5 小时");
        assert_eq!(s.windows[0].reset,Some(1794244287));
        assert!((s.windows[0].remaining-94.86).abs()<0.01);
        assert!((s.windows[1].remaining-61.26).abs()<0.01);
    }
    #[test] fn commandcode_missing_or_broken_windows_are_not_full_quota(){
        for value in [json!({}),json!({"windowLimits":{}}),json!({"windowLimits":{"fiveHour":{"used":1,"cap":0}}})]{assert!(parse(&value).is_err());}
        let s=parse(&json!({"windowLimits":{"weekly":{"used":40,"cap":35}}})).unwrap();
        assert_eq!(s.windows[0].remaining,0.0);
        assert!(s.windows[0].reset.is_none());
    }
}
