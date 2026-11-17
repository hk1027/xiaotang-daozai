//! DeepSeek API 余额与系统凭据存储。
use super::{Account,Snapshot};
use serde_json::Value;
use std::{io::Read,sync::mpsc};

#[derive(Clone,Debug)]
#[derive(serde::Serialize,serde::Deserialize)]
pub struct Balance {pub currency:String,pub total:f64,pub granted:f64,pub topped_up:f64}
impl Balance {
    pub fn symbol(&self)->&str{if self.currency=="CNY"{"¥"}else{"$"}}
    pub fn full(&self,value:f64)->String{
        let raw=format!("{:.2}",value.abs());let (whole,decimal)=raw.split_once('.').unwrap();let mut grouped=String::new();
        for (i,ch) in whole.chars().enumerate(){if i>0&&(whole.len()-i)%3==0{grouped.push(',');}grouped.push(ch);}
        format!("{}{}{grouped}.{decimal}",self.symbol(),if value<0.0{"-"}else{""})
    }
    pub fn compact(&self)->String{
        let n=self.total;let magnitude=n.abs();
        let (value,suffix)=if magnitude>=100_000_000.0{(n/100_000_000.0,"亿")}else if magnitude>=10_000.0{(n/10_000.0,"万")}else if magnitude>=1000.0{(n/1000.0,"k")}else{(n,"")};
        let digits=if suffix.is_empty(){if magnitude<100.0{1}else{0}}else if value.abs()<10.0{2}else{1};
        let factor=10f64.powi(digits);let text=format!("{:.*}",(digits as usize),(value*factor).trunc()/factor);
        let text=if text.contains('.') {text.trim_end_matches('0').trim_end_matches('.')}else{&text};
        format!("{}{text}{suffix}",self.symbol())
    }
}
#[derive(Clone,Debug)]
#[derive(serde::Serialize,serde::Deserialize)]
pub struct Wallet {pub available:bool,pub balances:Vec<Balance>}
impl Wallet {pub fn primary(&self)->&Balance{self.balances.iter().find(|b|b.currency=="CNY").unwrap_or(&self.balances[0])}}
pub fn parse(v:&Value)->Result<Snapshot,String>{
    let available=v["is_available"].as_bool().ok_or("余额响应缺少账户状态")?;
    let rows=v["balance_infos"].as_array().filter(|rows|!rows.is_empty()).ok_or("服务未返回余额")?;
    let mut balances=Vec::new();
    for row in rows{
        let currency=row["currency"].as_str().filter(|c|matches!(*c,"CNY"|"USD")).ok_or("暂不支持该余额币种")?;
        let amount=|key:&str|row[key].as_str().and_then(|s|s.parse::<f64>().ok()).filter(|n|n.is_finite()).ok_or_else(||"余额字段无效，请稍后刷新".to_string());
        if balances.iter().any(|b:&Balance|b.currency==currency){return Err("余额币种重复".into());}
        balances.push(Balance{currency:currency.into(),total:amount("total_balance")?,granted:amount("granted_balance")?,topped_up:amount("topped_up_balance")?});
    }
    Ok(Snapshot{wallet:Some(Wallet{available,balances}),at:chrono::Utc::now().timestamp(),..Default::default()})
}
#[cfg(windows)]
fn credential(slot:&str)->Result<keyring::Entry,String>{keyring::Entry::new("Daozai.DeepSeek",slot).map_err(|_|"无法打开 Windows 凭据管理器".into())}
#[cfg(windows)]
fn save(slot:&str,key:&str)->Result<(),String>{credential(slot)?.set_password(key).map_err(|_|"无法安全保存 API Key".into())}
#[cfg(not(windows))]
fn save(_slot:&str,_key:&str)->Result<(),String>{Err("当前系统不支持安全保存 API Key".into())}
#[cfg(windows)]
fn load(slot:&str)->Result<String,String>{credential(slot)?.get_password().map_err(|error|match error {keyring::Error::NoEntry=>"未找到已保存的 DeepSeek API Key，请重新连接".into(),_=>"暂时无法读取 Windows 凭据，请稍后重试".into()})}
#[cfg(not(windows))]
fn load(_slot:&str)->Result<String,String>{Err("当前系统不支持读取 API Key".into())}
pub fn forget(account:&Account){
    #[cfg(windows)]
    if let Some(slot)=&account.slot{if let Ok(entry)=credential(slot){let _=entry.delete_credential();}}
    #[cfg(not(windows))] let _=account;
}
fn response_error(status:u16)->(String,u64){
    match status{
        401=>("本次认证失败（401），请先重试".into(),0),
        403=>("查询被拒绝（403），请检查网络或权限".into(),60),
        429=>("请求过于频繁，稍后自动重试".into(),180),
        _=>(format!("余额服务暂不可用（{status}），请稍后重试"),30)
    }
}
fn request(agent:&ureq::Agent,key:&str)->Result<Snapshot,(String,u64)>{
    let response=agent.get("https://api.deepseek.com/user/balance").set("Authorization",&format!("Bearer {key}")).set("Accept","application/json").call().map_err(|e|match e{
        ureq::Error::Status(status,_)=>response_error(status),
        _=>("无法更新 DeepSeek 余额，请检查网络后重试".into(),30)
    })?;
    let mut body=String::new();response.into_reader().take(1024*1024).read_to_string(&mut body).map_err(|_|("读取余额失败".into(),30))?;
    let v=serde_json::from_str(&body).map_err(|_|("余额响应无效".into(),30))?;parse(&v).map_err(|e|(e,30))
}
pub fn fetch(agent:&ureq::Agent,account:&Account)->Result<Snapshot,(String,u64)>{
    let slot=account.slot.as_deref().ok_or(("请连接 DeepSeek API Key".into(),0))?;
    request(agent,&load(slot).map_err(|e|(e,0))?)
}
pub struct Connected {pub slot:String,pub label:String,pub snapshot:Snapshot}
pub struct Connect {rx:mpsc::Receiver<Result<Connected,String>>}
impl Connect {
    pub fn start(alias:String,key:String)->Self{
        let(tx,rx)=mpsc::channel();std::thread::spawn(move||{let result=(||{
            let key=key.trim();if key.is_empty()||key.chars().any(char::is_whitespace){return Err("请输入有效的 API Key".into());}
            let snapshot=request(&super::http_agent(),key).map_err(|e|e.0)?;
            let digest=ring::digest::digest(&ring::digest::SHA256,key.as_bytes());let slot=digest.as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>();
            save(&slot,key)?;Ok(Connected{slot,label:if alias.trim().is_empty(){"DeepSeek".into()}else{alias.trim().into()},snapshot})
        })();let _=tx.send(result);});Self{rx}
    }
    pub fn poll(&self)->Option<Result<Connected,String>>{self.rx.try_recv().ok()}
}
#[cfg(test)]
mod tests {
    use super::*;use serde_json::json;
    #[test] fn deepseek_auth_and_access_errors_are_distinct(){
        assert!(response_error(401).0.contains("401"));assert!(response_error(401).0.contains("重试"));assert!(!response_error(401).0.contains("重新连接"));
        assert!(!response_error(403).0.contains("Key"));assert!(response_error(403).0.contains("403"));
        assert_eq!(response_error(429).1,180);assert!(response_error(503).0.contains("503"));
        assert!(!response_error(401).0.contains("DeepSeek"));
    }
    #[test]
    #[ignore = "按需只读诊断 DeepSeek，输出状态码与响应类别，不输出密钥或响应正文"]
    fn deepseek_live_read_only_diagnostic(){
        let config=super::super::Config::load();
        for account in config.accounts.iter().filter(|a|a.provider==super::super::Provider::DeepSeek&&a.enabled){
            let slot=account.slot.as_deref().expect("缺少安全存储引用");let key=load(slot).expect("无法读取安全存储");
            let digest=ring::digest::digest(&ring::digest::SHA256,key.as_bytes());let fingerprint=digest.as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>();println!("credential_integrity={}",fingerprint==slot);
            for (route,agent) in [("configured",super::super::http_agent()),("direct",ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).redirects(0).build())]{
                let response=match agent.get("https://api.deepseek.com/user/balance").set("Authorization",&format!("Bearer {key}")).set("Accept","application/json").call(){Ok(r)=>r,Err(ureq::Error::Status(_,r))=>r,Err(_)=>{println!("route={route} transport_error=true");continue;}};
                let status=response.status();let json=response.header("content-type").is_some_and(|t|t.contains("application/json"));let mut raw=String::new();let _=response.into_reader().take(65536).read_to_string(&mut raw);
                let value:Value=serde_json::from_str(&raw).unwrap_or(Value::Null);let message=value.pointer("/error/message").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
                println!("route={route} status={status} json={json} valid_balance={} auth_message={} blocked_message={}",parse(&value).is_ok(),message.contains("api key")||message.contains("authentication"),message.contains("region")||message.contains("country")||message.contains("blocked"));
            }
        }
    }
    #[test] fn deepseek_amounts_have_bounded_compact_labels(){for (total,want) in [(28.60,"¥28.6"),(999.0,"¥999"),(1280.5,"¥1.28k"),(12800.0,"¥1.28万"),(128000.0,"¥12.8万"),(9999.99,"¥9.99k"),(100000000.0,"¥1亿")] {let b=Balance{currency:"CNY".into(),total,granted:0.0,topped_up:total};assert_eq!(b.compact(),want);if total==12800.0{assert_eq!(b.full(total),"¥12,800.00");}}}
    #[test] fn deepseek_requires_valid_amounts_and_preserves_currency(){let mut v=json!({"is_available":false,"balance_infos":[{"currency":"USD","total_balance":"0.0001","granted_balance":"0","topped_up_balance":"0.0001"}]});let s=parse(&v).unwrap();let w=s.wallet.unwrap();assert!(!w.available);assert_eq!(w.primary().currency,"USD");assert_eq!(w.primary().total,0.0001);v["balance_infos"][0]["total_balance"]=json!("NaN");assert!(parse(&v).is_err());assert!(parse(&json!({"is_available":true,"balance_infos":[]})).is_err());}
}
