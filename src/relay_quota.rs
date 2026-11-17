//! Read-only adapters for distinct relay protocols. Do not convert site quota to money.
//! Protocol references (reviewed 2026-11-08):
//! https://github.com/QuantumNous/new-api/blob/main/controller/token.go
//! https://github.com/songquanpeng/one-api/blob/main/controller/billing.go
//! https://github.com/Wei-Shaw/sub2api/blob/main/backend/internal/handler/gateway_handler.go
use super::{Snapshot, Window};
use serde_json::Value;
use std::io::Read;
use std::time::{Duration,Instant};
#[path="relay_script.rs"]
pub mod script;

pub const AUTO_TIMEOUT:Duration=Duration::from_secs(15);
const PROBE_TIMEOUT:Duration=Duration::from_secs(5);
const MAX_PROBE_REQUESTS:usize=5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { NewApi, OneApi, Sub2Api, General }
impl Kind {
    fn name(self) -> &'static str { match self { Self::NewApi => "New API", Self::OneApi => "兼容计费接口", Self::Sub2Api => "Sub2API",Self::General=>"通用余额" } }
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum Scope { #[default] Unknown, Account, Subscription, Key, Reported }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Amount {
    #[serde(default)]
    pub scope: Scope,
    pub label: String,
    pub unit: String,
    pub remaining: Option<f64>,
    pub limit: Option<f64>,
    pub used: Option<f64>,
    pub unlimited: bool,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Quota {
    pub source: String,
    pub amounts: Vec<Amount>,
    /// Expiry is not a reset. Keep it out of Window.reset.
    pub expires_at: Option<i64>,
}
impl Quota {
    pub fn primary(&self)->Option<&Amount>{
        self.amounts.iter().find(|a|a.displayable()&&matches!(a.scope,Scope::Account))
            .or_else(||self.amounts.iter().filter(|a|a.displayable()).min_by(|a,b|a.remaining.unwrap().total_cmp(&b.remaining.unwrap())))
    }
    pub fn unavailable_text(&self)->&'static str{
        if self.amounts.iter().any(|a|matches!(a.scope,Scope::Subscription)){"额度暂不可查询"}else{"余额暂不可查询"}
    }
    pub fn compact(&self) -> String {
        let Some(a) = self.primary() else { return self.unavailable_text().into() };
        if matches!(a.scope,Scope::Subscription){format!("{}剩余 {}",a.label,a.remaining_text())}else{a.remaining_text()}
    }
    pub fn full(&self) -> String {
        let mut rows = vec![self.source.clone()];
        if self.primary().is_none(){rows.push(self.unavailable_text().into());}
        for a in self.amounts.iter().filter(|a|a.displayable()) {
            let value = a.remaining_text();
            let used = a.used.map(|n|format!("，已用 {} {}",number(n),a.unit)).unwrap_or_default();
            rows.push(format!("{}：剩余 {value}{used}",a.label));
        }
        if self.amounts.iter().any(|a|a.displayable()&&matches!(a.scope,Scope::Subscription)){
            if let Some(at) = self.expires_at.and_then(|t|chrono::DateTime::from_timestamp(t,0)) { rows.push(format!("到期 {} UTC",at.format("%Y-%m-%d %H:%M"))); }
        }
        rows.join("\n")
    }
}
impl Amount {
    fn displayable(&self)->bool{
        !self.unlimited&&self.remaining.is_some_and(f64::is_finite)&&match self.scope{
            Scope::Account|Scope::Reported=>true,Scope::Subscription=>self.limit.is_some_and(|n|n.is_finite()&&n>0.0),_=>false,
        }
    }
    pub fn remaining_text(&self)->String{
        if !self.displayable(){return "额度暂不可查询".into();}
        let remaining=self.remaining.unwrap();
        if matches!(self.scope,Scope::Subscription){format!("{:.0}%",(remaining/self.limit.unwrap()*100.0).clamp(0.0,100.0))}else{format!("{} {}",number(remaining),self.unit)}
    }
}
fn number(n:f64)->String {
    let raw = if n.abs() >= 1e12 || (n != 0.0 && n.abs() < 0.0001) { format!("{n:.3e}") } else { format!("{n:.4}") };
    if raw.contains('e') { raw } else { raw.trim_end_matches('0').trim_end_matches('.').to_owned() }
}

/// Accept the site root, preserving its reverse-proxy path prefix.
/// HTTPS only: localhost HTTP is deliberately not an implicit credential exception.
pub fn validate_base_url(raw:&str)->Result<String,String> {
    let raw=raw.trim();
    if raw.is_empty() || raw.chars().any(char::is_whitespace) || raw.contains('\\') { return Err("请输入有效的 HTTPS 站点地址".into()); }
    let url=url::Url::parse(raw).map_err(|_|"请输入完整的 HTTPS 站点地址")?;
    if url.scheme()!="https" || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return Err("站点地址必须使用 HTTPS，且不能包含账号、查询参数或片段".into());
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}
/// Preserve the user's exact deployment path; never guess whether /v1 is a
/// tenant prefix or an API suffix. The UI asks for the site root, not API base.
pub fn normalize_base_url(raw:&str)->Result<String,String>{validate_base_url(raw)}

fn numeric(v:&Value)->Option<f64> { v.as_f64().filter(|n|n.is_finite()) }
fn required(v:&Value,key:&str)->Result<f64,String> { numeric(&v[key]).ok_or_else(||"站点返回的额度字段缺失或无效".into()) }
fn nonnegative(v:&Value,key:&str)->Result<f64,String> { required(v,key).and_then(|n|if n>=0.0{Ok(n)}else{Err("站点返回的额度字段无效".into())}) }
fn expiry(v:&Value)->Option<i64> {
    v.as_i64().filter(|n|*n>0).or_else(||v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d|d.timestamp()).filter(|n|*n>0))
}
fn snapshot(kind:Kind,amounts:Vec<Amount>,expires_at:Option<i64>,windows:Vec<Window>)->Snapshot {
    Snapshot { plan:kind.name().into(), relay:Some(Quota{source:kind.name().into(),amounts,expires_at}), windows, at:chrono::Utc::now().timestamp(), ..Default::default() }
}
fn window(label:&str,limit:f64,remaining:f64,seconds:Option<i64>,reset:Option<i64>)->Option<Window> {
    (limit>0.0).then(||Window{label:label.into(),seconds,remaining:((remaining/limit).clamp(0.0,1.0)*100.0) as f32,reset,amount:None})
}
fn amount(label:&str,unit:&str,remaining:Option<f64>,limit:Option<f64>,used:Option<f64>,unlimited:bool,scope:Scope)->Amount {
    Amount{scope,label:label.into(),unit:unit.into(),remaining,limit,used,unlimited}
}

pub fn parse_new_api(v:&Value)->Result<Snapshot,String> {
    if (v["code"]!=true&&v["object"]!="token_usage")||v["code"]==false||v["success"]==false||!v["error"].is_null() { return Err("站点未返回有效的令牌额度".into()); }
    let d=if v["object"]=="token_usage"{v}else{&v["data"]};
    if d["object"]!="token_usage" { return Err("该站点版本不支持 New API 令牌额度接口".into()); }
    if !d["unit"].is_null()&&d["unit"]!="quota"{return Err("站点额度单位与 New API 令牌协议不一致".into());}
    let unlimited=d["unlimited_quota"].as_bool().ok_or("站点未返回令牌限额状态")?;
    let used=nonnegative(d,"total_used")?;
    let (remaining,limit)=if unlimited {(None,None)}else{(Some(required(d,"total_available")?),Some(nonnegative(d,"total_granted")?))};
    let windows=match (remaining,limit) { (Some(r),Some(l))=>window("令牌额度",l,r,None,None).into_iter().collect(),_=>vec![] };
    let mut amounts=vec![amount("令牌额度","站点配额",remaining,limit,Some(used),unlimited,Scope::Key)];
    if d["user_quota"].is_object() {
        let u=&d["user_quota"];
        if !u["unit"].is_null()&&u["unit"]!="quota"{return Err("站点账户额度单位与令牌额度不一致".into());}
        amounts.push(amount("账户额度","站点配额",Some(required(u,"total_available")?),None,Some(nonnegative(u,"total_used")?),false,Scope::Account));
    }
    Ok(snapshot(Kind::NewApi,amounts,expiry(&d["expires_at"]),windows))
}

pub fn parse_new_api_user(v:&Value,status:&Value)->Result<Snapshot,String>{
    if v["success"]!=true||!v["error"].is_null(){return Err("无法读取账户余额，请核对用户 Token、用户 ID 和查询权限".into());}
    let d=&v["data"];let remaining=required(d,"quota")?;let used=nonnegative(d,"used_quota")?;
    // 账户配额按站点公开配置换算；未提供有效配置时保留原始单位。
    let conversion=(||{
        if status["success"]!=true{return None;}
        let s=&status["data"];let per=numeric(&s["quota_per_unit"]).filter(|n|*n>0.0)?;
        let display=s["quota_display_type"].as_str().or_else(||s["display_in_currency"].as_bool().map(|b|if b{"USD"}else{"TOKENS"}))?;
        let (rate,unit)=match display{
            "USD"=>(1.0,"USD"),
            "CNY"=>(numeric(&s["usd_exchange_rate"])? ,"CNY"),
            "CUSTOM"=>(numeric(&s["custom_currency_exchange_rate"])? ,s["custom_currency_symbol"].as_str().filter(|s|!s.is_empty()&&s.chars().count()<=12&&!s.chars().any(char::is_control))?),
            _=>return None,
        };
        let factor=rate/per;
        (rate>0.0&&factor.is_finite()&&factor>0.0&&(remaining*factor).is_finite()&&(used*factor).is_finite()).then_some((factor,unit))
    })();
    let (factor,unit)=conversion.unwrap_or((1.0,"站点配额"));
    Ok(snapshot(Kind::NewApi,vec![amount(if conversion.is_some(){"账户余额"}else{"账户额度"},unit,Some(remaining*factor),None,Some(used*factor),false,Scope::Account)],None,vec![]))
}

pub fn parse_one_api(subscription:&Value,usage:&Value)->Result<Snapshot,String> {
    if !subscription["error"].is_null() || !usage["error"].is_null() || subscription["object"]!="billing_subscription" || usage["object"]!="list" {
        return Err("该站点未返回有效的 One API 计费额度".into());
    }
    let limit=nonnegative(subscription,"hard_limit_usd")?;
    // This endpoint multiplies usage by 100 even when DisplayInCurrencyEnabled is false.
    let used=nonnegative(usage,"total_usage")?/100.0;
    // Official One API uses this sentinel for unlimited, but cannot distinguish it
    // from a real numeric limit. Never present a fabricated balance or percentage.
    let ambiguous=limit==100_000_000.0;
    let remaining=if ambiguous {None}else{Some(limit-used)};
    let windows=remaining.and_then(|r|window("计费额度",limit,r,None,None)).into_iter().collect();
    Ok(snapshot(Kind::OneApi,vec![amount("计费额度","站点单位",remaining,if ambiguous{None}else{Some(limit)},Some(used),false,Scope::Unknown)],expiry(&subscription["access_until"]),windows))
}

pub fn parse_sub2api(v:&Value)->Result<Snapshot,String> {
    if v["isValid"]==false||(!v["isValid"].is_null()&&!v["isValid"].is_boolean())||v["success"]==false||!v["error"].is_null() { return Err("站点未返回有效的 Sub2API 额度".into()); }
    if v["mode"].is_null(){
        let mut result=serde_json::json!({"planName":"API Key 额度","unit":v["unit"].as_str().or(v["quota"]["unit"].as_str()).unwrap_or("USD")});
        if let Some(n)=numeric(&v["remaining"]).filter(|n|*n>=0.0){result["remaining"]=serde_json::json!(n);}
        if numeric(&v["quota"]["limit"]).is_some_and(|n|n>0.0){result["total"]=v["quota"]["limit"].clone();result["used"]=v["quota"]["used"].clone();}
        if v["remaining"].is_null()&&v["quota"].is_null(){return Err("站点未返回 Sub2API 额度字段".into());}
        return script::parse_result(&result,"Sub2API");
    }
    if !v["unit"].is_null()&&v["unit"]!="USD"{return Err("站点返回了未知的额度单位".into());}
    let mut amounts=vec![];let mut windows=vec![];
    match v["mode"].as_str() {
        Some("quota_limited")=>{
            if !v["quota"].is_null() {
                let q=&v["quota"];
                if q["unit"]!="USD" {return Err("站点返回了未知的额度单位".into());}
                let limit=nonnegative(q,"limit")?;let used=nonnegative(q,"used")?;let remaining=required(q,"remaining")?;
                amounts.push(amount("API Key 总额度","USD",Some(remaining),Some(limit),Some(used),false,Scope::Key));
                if let Some(w)=window("总额度",limit,remaining,None,None){windows.push(w);}
            }
            if let Some(rows)=v["rate_limits"].as_array() {
                if rows.len()>3 {return Err("站点返回了重复或过多的限额周期".into());}
                let mut seen=std::collections::HashSet::new();
                for row in rows {
                    if !row["unit"].is_null()&&row["unit"]!="USD"{return Err("站点返回了未知的额度单位".into());}
                    let (label,seconds)=match row["window"].as_str(){Some("5h")=>("5 小时",18000),Some("1d")=>("每日",86400),Some("7d")=>("每周",604800),_=>return Err("站点返回了未知的限额周期".into())};
                    if !seen.insert(seconds){return Err("站点返回了重复的限额周期".into());}
                    let limit=nonnegative(row,"limit")?;let used=nonnegative(row,"used")?;let remaining=required(row,"remaining")?;
                    // Official rate_limits fields are USD spending limits, not request counts.
                    amounts.push(amount(label,"USD",Some(remaining),Some(limit),Some(used),false,Scope::Key));
                    if let Some(w)=window(label,limit,remaining,Some(seconds),expiry(&row["reset_at"])){windows.push(w);}
                }
            }
        }
        Some("unrestricted")=>{
            if v["unit"]!="USD" {return Err("站点返回了未知的额度单位".into());}
            if v["subscription"].is_object() {
                let sub=&v["subscription"];
                for (prefix,label,seconds) in [("daily","每日",86400),("weekly","每周",604800),("monthly","每月",2592000)] {
                    let field=format!("{prefix}_limit_usd");
                    // Null / absent is not an exhausted window, and does not prove unlimited.
                    if sub[&field].is_null(){continue;}
                    let limit=nonnegative(sub,&field)?;
                    if limit==0.0 {continue;}
                    let used=nonnegative(sub,&format!("{prefix}_usage_usd"))?;let remaining=(limit-used).max(0.0);
                    amounts.push(amount(label,"USD",Some(remaining),Some(limit),Some(used),false,Scope::Subscription));
                    if let Some(w)=window(label,limit,remaining,Some(seconds),None){windows.push(w);}
                }
                if amounts.is_empty() {
                    // -1 is an explicit unlimited signal only for this subscription mode.
                    if numeric(&v["remaining"])==Some(-1.0) {
                        amounts.push(amount("订阅额度","USD",None,None,None,true,Scope::Subscription));
                    } else { return Err("站点未返回可用的订阅限额明细".into()); }
                }
            } else if !v["balance"].is_null() {
                amounts.push(amount("账户余额","USD",Some(required(v,"balance")?),None,None,false,Scope::Account));
            } else {
                return Err("站点未返回订阅限额或钱包余额，请检查站点版本与权限".into());
            }
        }
        _=>return Err("该站点版本不支持当前 Sub2API 额度协议".into()),
    }
    if amounts.is_empty(){return Err("站点未返回可用的额度明细".into());}
    if v["mode"]=="quota_limited"{
        if let Some(remaining)=numeric(&v["remaining"]).filter(|n|*n>=0.0){amounts.insert(0,amount("API Key 剩余额度","USD",Some(remaining),None,None,false,Scope::Reported));}
    }
    let expires=expiry(&v["expires_at"]).or_else(||expiry(&v["subscription"]["expires_at"]));
    Ok(snapshot(Kind::Sub2Api,amounts,expires,windows))
}

pub fn parse_general(v:&Value)->Result<Snapshot,String>{
    if v["is_active"]==false||v["success"]==false||v["isValid"]==false||!v["error"].is_null(){return Err("站点未返回可用余额，请核对账号与权限".into());}
    let remaining=required(v,"balance")?;
    let unit=v["unit"].as_str().unwrap_or("USD");script::validate_unit(unit)?;
    Ok(snapshot(Kind::General,vec![amount("账户余额",unit,Some(remaining),None,None,false,Scope::Account)],None,vec![]))
}
fn endpoint(base:&str,path:&str)->String{
    let base=if path=="/v1/usage"{base.strip_suffix("/v1").unwrap_or(base)}else{base};format!("{base}{path}")
}

fn response_error(status:u16)->(String,u64) {
    match status {
        401=>("站点认证失败（401），请核对 API Key".into(),0),
        403=>("站点拒绝额度查询（403），请检查权限".into(),60),
        404=>("站点没有该额度接口（404），请核对类型、地址和版本".into(),60),
        429=>("站点请求过于频繁，请稍后重试".into(),180),
        300..=399=>("站点要求重定向；为保护 API Key 已停止，请填写最终 HTTPS 地址".into(),0),
        _=>(format!("站点额度服务暂不可用（{status}）"),30),
    }
}
#[derive(Debug)]
enum ProbeError { Unsupported((String,u64)),Stop((String,u64)) }
impl ProbeError {fn detail(self)->(String,u64){match self{Self::Unsupported(e)|Self::Stop(e)=>e}}}
fn query(agent:&ureq::Agent,url:&str,key:&str,timeout:Option<Duration>)->Result<Value,ProbeError> {
    // Callers must supply the application's timeout-bounded, redirects(0) agent.
    let request=agent.get(url).set("Authorization",&format!("Bearer {key}")).set("Accept","application/json");
    let request=if let Some(timeout)=timeout{request.timeout(timeout)}else{request};
    read_response(request)
}
fn read_response(request:ureq::Request)->Result<Value,ProbeError>{
    read_response_result(request.call())
}
fn read_response_result(response:Result<ureq::Response,ureq::Error>)->Result<Value,ProbeError>{
    let response=response.map_err(|e|match e{
        ureq::Error::Status(s,_)=>if matches!(s,404|405){ProbeError::Unsupported(response_error(s))}else{ProbeError::Stop(response_error(s))},
        _=>ProbeError::Stop(("无法连接额度站点，请检查网络与地址".into(),30)),
    })?;
    if !(200..300).contains(&response.status()){return Err(ProbeError::Stop(response_error(response.status())));}
    const MAX:u64=1024*1024;
    let mut raw=Vec::new();
    response.into_reader().take(MAX+1).read_to_end(&mut raw).map_err(|_|ProbeError::Stop(("读取站点额度失败".into(),30)))?;
    if raw.len() as u64>MAX{return Err(ProbeError::Stop(("站点额度响应过大".into(),60)));}
    serde_json::from_slice(&raw).map_err(|_|ProbeError::Unsupported(("站点未返回有效的 JSON 额度响应".into(),30)))
}
fn get(agent:&ureq::Agent,url:&str,key:&str)->Result<Value,(String,u64)>{query(agent,url,key,None).map_err(ProbeError::detail)}

pub fn request_new_api_user(agent:&ureq::Agent,base_url:&str,token:&str,user_id:&str,cancelled:impl Fn()->bool)->Result<Snapshot,(String,u64)>{
    if cancelled(){return Err(("已取消连接".into(),0));}
    let base=validate_base_url(base_url).map_err(|e|(e,0))?;let token=token.trim();let user_id=user_id.trim();
    if token.is_empty()||token.len()>8192||token.chars().any(|c|c.is_whitespace()||c.is_control()){return Err(("请输入有效的用户 Access Token".into(),0));}
    let id=user_id.parse::<u64>().ok().filter(|id|*id>0).ok_or(("请输入有效的数字用户 ID".into(),0))?;
    let value=read_response(agent.get(&format!("{base}/api/user/self")).set("Authorization",&format!("Bearer {token}")).set("New-Api-User",&id.to_string()).set("Accept","application/json")).map_err(|e|{
        let (message,wait)=e.detail();if message.contains("401"){("用户认证失败（401），请核对 Access Token 和用户 ID；API Key 无法读取账户余额".into(),wait)}else{(message.replace("API Key","用户 Token"),wait)}
    })?;
    let snapshot=parse_new_api_user(&value,&Value::Null).map_err(|e|(e,60))?;
    if value["data"]["id"].as_u64().is_some_and(|actual|actual!=id){return Err(("站点返回的用户 ID 与填写值不一致，请核对账号".into(),0));}
    if cancelled(){return Err(("已取消连接".into(),0));}
    let status=read_response(agent.get(&format!("{base}/api/status")).set("Accept","application/json").timeout(PROBE_TIMEOUT)).ok();
    if cancelled(){return Err(("已取消连接".into(),0));}
    match status{Some(status)=>parse_new_api_user(&value,&status).map_err(|e|(e,60)),None=>Ok(snapshot)}
}

fn detect_with(mut query:impl FnMut(&str,Duration)->Result<Value,ProbeError>,cancelled:impl Fn()->bool,budget:Duration)->Result<(Kind,Snapshot),(String,u64)>{
    let deadline=Instant::now()+budget;let mut requests=0;
    let mut next=|path:&str|->Result<Option<Value>,(String,u64)>{
        if cancelled(){return Err(("已取消连接".into(),0));}
        let remaining=deadline.checked_duration_since(Instant::now()).filter(|d|!d.is_zero()).ok_or_else(||("自动识别超时，请稍后重试或在高级设置中手动选择".into(),30))?;
        if requests>=MAX_PROBE_REQUESTS{return Err(("已达到自动识别请求上限".into(),60));}requests+=1;
        let response=query(path,remaining.min(PROBE_TIMEOUT));
        if cancelled(){return Err(("已取消连接".into(),0));}
        if Instant::now()>=deadline{return Err(("自动识别超时，请稍后重试或在高级设置中手动选择".into(),30));}
        let value=match response{Ok(v)=>v,Err(ProbeError::Unsupported(_))=>return Ok(None),Err(ProbeError::Stop(e))=>return Err(e)};
        if value["code"]==false||value["success"]==false||value["isValid"]==false||!value["error"].is_null(){return Err(("站点拒绝额度查询，请核对 API Key 与查询权限".into(),60));}
        Ok(Some(value))
    };
    // 优先专用额度协议；兼容计费接口不证明站点品牌，也不证明账户或 Key 范围。
    for (kind,path) in [(Kind::General,"/user/balance"),(Kind::NewApi,"/api/usage/token/"),(Kind::Sub2Api,"/v1/usage"),(Kind::OneApi,"/dashboard/billing/subscription")] {
        let Some(value)=next(path)?else{continue;};
        let result=match kind{
            Kind::General if value.get("balance").is_some()=>parse_general(&value),
            Kind::NewApi if value["data"]["object"]=="token_usage"||value["object"]=="token_usage"=>parse_new_api(&value),
            Kind::Sub2Api if value.get("mode").is_some()||value.get("remaining").is_some()||value.get("quota").is_some()=>parse_sub2api(&value),
            Kind::OneApi if value["object"]=="billing_subscription"=>{
                let usage=next("/dashboard/billing/usage")?.ok_or_else(||("计费额度明细不完整，请核对站点版本或在高级设置中选择类型".into(),60))?;
                parse_one_api(&value,&usage)
            },
            _=>continue,
        };
        return result.map(|s|(kind,s)).map_err(|e|(e,60));
    }
    Err(("暂时无法自动识别额度接口，请核对站点地址或在高级设置中手动选择".into(),60))
}
pub fn detect_cancellable(agent:impl Fn(Duration)->ureq::Agent,base_url:&str,key:&str,cancelled:impl Fn()->bool)->Result<(Kind,Snapshot),(String,u64)>{
    let base=validate_base_url(base_url).map_err(|e|(e,0))?;let key=key.trim();
    if key.is_empty()||key.len()>8192||key.chars().any(|c|c.is_whitespace()||c.is_control()){return Err(("请输入有效的 API Key".into(),0));}
    detect_with(|path,timeout|query(&agent(timeout),&endpoint(&base,path),key,Some(timeout)),cancelled,AUTO_TIMEOUT)
}
/// 已保存的协议直接查询；刷新和手动指定类型均不自动切换协议。
pub fn request_cancellable(agent:&ureq::Agent,kind:Kind,base_url:&str,key:&str,cancelled:impl Fn()->bool)->Result<Snapshot,(String,u64)> {
    if cancelled(){return Err(("已取消连接".into(),0));}
    let base=validate_base_url(base_url).map_err(|e|(e,0))?;
    let key=key.trim();
    if key.is_empty() || key.len()>8192 || key.chars().any(|c|c.is_whitespace()||c.is_control()) {return Err(("请输入有效的 API Key".into(),0));}
    let result=match kind {
        Kind::General=>parse_general(&get(agent,&format!("{base}/user/balance"),key)?),
        Kind::NewApi=>parse_new_api(&get(agent,&format!("{base}/api/usage/token/"),key)?),
        Kind::OneApi=>{
            let subscription=get(agent,&format!("{base}/dashboard/billing/subscription"),key)?;
            if cancelled(){return Err(("已取消连接".into(),0));}
            let usage=get(agent,&format!("{base}/dashboard/billing/usage"),key)?;
            parse_one_api(&subscription,&usage)
        }
        Kind::Sub2Api=>parse_sub2api(&get(agent,&endpoint(&base,"/v1/usage"),key)?),
    };
    result.map_err(|e|(e,60))
}

#[cfg(test)]
mod tests {
    use super::*;use serde_json::json;
    #[test]
    fn cc_switch_general_balance_is_detected_with_an_api_key(){
        let (_,snapshot)=detect_with(|path,_|if path=="/user/balance"{Ok(json!({"is_active":true,"balance":1.1}))}else{Err(unsupported())},||false,AUTO_TIMEOUT).unwrap();
        assert_eq!(snapshot.relay.unwrap().compact(),"1.1 USD");
    }
    #[test]
    fn cc_switch_sub2api_uses_reported_remaining_without_requiring_mode(){
        let snapshot=parse_sub2api(&json!({"isValid":true,"remaining":8,"unit":"USD","quota":{"limit":10,"used":2}})).unwrap();
        assert_eq!(snapshot.relay.unwrap().compact(),"8 USD");
    }
    #[test]
    fn new_api_user_balance_preserves_zero_negative_and_missing_values(){
        for remaining in [0,-87919,550000]{
            let value=json!({"success":true,"data":{"quota":remaining,"used_quota":86089298}});
            let snapshot=parse_new_api_user(&value,&Value::Null).unwrap();assert!(snapshot.windows.is_empty());
            let q=snapshot.relay.unwrap();let amount=q.primary().unwrap();
            assert!(matches!(amount.scope,Scope::Account));assert_eq!(amount.remaining,Some(remaining as f64));
            assert_eq!(amount.used,Some(86089298.0));assert_eq!(amount.unit,"站点配额");
        }
        for value in [json!({"success":false,"message":"sensitive-token"}),json!({"success":true,"data":{"used_quota":0}}),json!({"success":true,"data":{"quota":0}}),json!({"success":true,"data":{"quota":"1.1","used_quota":0}}),json!({"success":true,"data":{"quota":1,"used_quota":-1}})]{
            let error=parse_new_api_user(&value,&Value::Null).unwrap_err();assert!(!error.contains("sensitive-token"));
        }
    }
    #[test]
    fn new_api_user_money_uses_site_units_and_exchange_rates(){
        let value=json!({"success":true,"data":{"quota":550000,"used_quota":1000000}});
        for (settings,want) in [
            (json!({"quota_per_unit":500000,"quota_display_type":"USD"}),"1.1 USD"),
            (json!({"quota_per_unit":1000000,"quota_display_type":"USD"}),"0.55 USD"),
            (json!({"quota_per_unit":500000,"quota_display_type":"CNY","usd_exchange_rate":7}),"7.7 CNY"),
            (json!({"quota_per_unit":500000,"quota_display_type":"CUSTOM","custom_currency_exchange_rate":2,"custom_currency_symbol":"积分"}),"2.2 积分"),
            (json!({"quota_per_unit":500000,"display_in_currency":true}),"1.1 USD"),
            (json!({"quota_per_unit":500000,"display_in_currency":false}),"550000 站点配额"),
            (json!({"quota_per_unit":0,"quota_display_type":"USD"}),"550000 站点配额"),
            (json!({"quota_per_unit":500000,"quota_display_type":"CNY"}),"550000 站点配额"),
            (json!({"quota_per_unit":500000,"quota_display_type":"UNKNOWN"}),"550000 站点配额"),
        ]{
            let snapshot=parse_new_api_user(&value,&json!({"success":true,"data":settings})).unwrap();assert_eq!(snapshot.relay.unwrap().compact(),want);
        }
        let status=json!({"success":true,"data":{"quota_per_unit":500000,"quota_display_type":"USD"}});
        let tiny=parse_new_api_user(&json!({"success":true,"data":{"quota":1,"used_quota":0}}),&status).unwrap();
        assert_eq!(tiny.relay.unwrap().primary().unwrap().remaining,Some(0.000002));
        assert_eq!(parse_new_api_user(&value,&json!({"success":false,"data":status["data"]})).unwrap().relay.unwrap().compact(),"550000 站点配额");
    }
    #[test]
    fn new_api_user_errors_never_probe_token_routes_or_expose_credentials(){
        for status in [401,403,302,429,500,200]{
            let calls=std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));let count=calls.clone();
            let agent=ureq::AgentBuilder::new().redirects(0).middleware(move|request:ureq::Request,_:ureq::MiddlewareNext|{
                assert!(request.url().ends_with("/api/user/self"));count.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
                ureq::Response::new(status,"Error",r#"{"success":false,"message":"sensitive-body"}"#)
            }).build();
            let error=request_new_api_user(&agent,"https://relay.example/prefix","synthetic-user-token","42",||false).unwrap_err().0;
            assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed),1);assert!(!error.contains("sensitive-body")&&!error.contains("synthetic-user-token"));
            if status==401{assert!(error.contains("用户 ID"));}
        }
    }
    #[test]
    fn new_api_user_status_failure_keeps_raw_account_balance(){
        let agent=ureq::AgentBuilder::new().middleware(|request:ureq::Request,_:ureq::MiddlewareNext|{
            if request.url().ends_with("/api/user/self"){ureq::Response::new(200,"OK",r#"{"success":true,"data":{"quota":550000,"used_quota":1000000}}"#)}else{ureq::Response::new(404,"Not Found","")}
        }).build();
        assert_eq!(request_new_api_user(&agent,"https://relay.example","synthetic","42",||false).unwrap().relay.unwrap().compact(),"550000 站点配额");
    }
    #[test]
    fn new_api_user_rejects_bad_inputs_mismatched_identity_and_cancellation(){
        let agent=ureq::AgentBuilder::new().middleware(|_:ureq::Request,_:ureq::MiddlewareNext|panic!("无效输入不应发送请求")).build();
        for (base,token,id) in [("http://relay.example","synthetic","42"),("https://relay.example","bad\nkey","42"),("https://relay.example","","42"),("https://relay.example","synthetic","0"),("https://relay.example","synthetic","invalid")]{assert!(request_new_api_user(&agent,base,token,id,||false).is_err());}
        assert!(request_new_api_user(&agent,"https://relay.example","synthetic","42",||true).is_err());
        let agent=ureq::AgentBuilder::new().middleware(|request:ureq::Request,_:ureq::MiddlewareNext|{
            assert!(request.url().ends_with("/api/user/self"));ureq::Response::new(200,"OK",r#"{"success":true,"data":{"id":43,"quota":1,"used_quota":0}}"#)
        }).build();
        assert!(request_new_api_user(&agent,"https://relay.example","synthetic","42",||false).unwrap_err().0.contains("不一致"));
    }
    fn serve_once(response:Vec<u8>,delay:std::time::Duration)->(String,std::thread::JoinHandle<String>){
        use std::{io::{Read,Write},net::TcpListener};
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();let url=format!("http://{}/api/usage/token/",listener.local_addr().unwrap());
        let worker=std::thread::spawn(move||{
            let (mut stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
            let mut request=Vec::new();let mut byte=[0u8;1];
            while request.len()<8192 && !request.ends_with(b"\r\n\r\n"){stream.read_exact(&mut byte).unwrap();request.push(byte[0]);}
            std::thread::sleep(delay);let _=stream.write_all(&response);String::from_utf8(request).unwrap()
        });(url,worker)
    }
    #[test]
    fn http_errors_and_redirects_keep_credentials_out_of_messages(){
        let agent=ureq::AgentBuilder::new().try_proxy_from_env(false).redirects(0).timeout(std::time::Duration::from_secs(2)).build();
        for status in [401,403,429,302,500]{
            let body=r#"{"message":"sk-sensitive-response"}"#;
            let response=format!("HTTP/1.1 {status} Error\r\nContent-Length: {}\r\nLocation: https://must-not-connect.invalid\r\nConnection: close\r\n\r\n{body}",body.len()).into_bytes();
            let (url,worker)=serve_once(response,std::time::Duration::ZERO);
            let (error,wait)=get(&agent,&url,"synthetic-wire-key").unwrap_err();let request=worker.join().unwrap();
            assert!(request.starts_with("GET /api/usage/token/ HTTP/1.1\r\n"));
            assert!(request.to_ascii_lowercase().contains("authorization: bearer synthetic-wire-key\r\n"));
            assert!(!error.contains("sk-sensitive")&&!error.contains("synthetic-wire"));
            if status==429{assert_eq!(wait,180);}if status==302{assert!(error.contains("重定向"));}
        }
    }
    #[test]
    fn oversized_and_slow_responses_are_bounded(){
        let agent=ureq::AgentBuilder::new().try_proxy_from_env(false).redirects(0).timeout(std::time::Duration::from_millis(150)).build();
        let mut response=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",1024*1024+1).into_bytes();response.resize(response.len()+1024*1024+1,b' ');
        let (url,worker)=serve_once(response,std::time::Duration::ZERO);let error=get(&agent,&url,"synthetic").unwrap_err().0;worker.join().unwrap();assert!(error.contains("过大"));
        let (url,worker)=serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}".to_vec(),std::time::Duration::from_millis(350));
        assert!(get(&agent,&url,"synthetic").is_err());worker.join().unwrap();
    }
    #[test] fn cancelled_validation_makes_no_request(){
        let agent=ureq::AgentBuilder::new().redirects(0).build();
        assert_eq!(request_cancellable(&agent,Kind::OneApi,"https://must-not-resolve.invalid","synthetic",||true).unwrap_err().0,"已取消连接");
    }
    #[test] fn persisted_prefix_is_not_normalized_again(){
        let base=normalize_base_url("https://relay.example/v1/v1").unwrap();assert_eq!(base,"https://relay.example/v1/v1");
        assert_eq!(validate_base_url(&base).unwrap(),base);
    }
    #[test] fn compact_names_the_exhausted_limiting_window(){
        let q=Quota{source:"Sub2API".into(),expires_at:None,amounts:vec![amount("每日","USD",Some(10.0),Some(10.0),Some(0.0),false,Scope::Subscription),amount("每周","USD",Some(0.0),Some(20.0),Some(20.0),false,Scope::Subscription)]};
        assert_eq!(q.compact(),"每周剩余 0%");
    }
    #[test] fn bases_are_https_and_never_contain_credentials_or_parameters(){
        for bad in ["http://example.com","http://localhost:8080","https://u:p@example.com","https://example.com?key=secret","https://example.com/#secret","file:///tmp/a","https://example.com\\evil","https://exa mple.com"] {assert!(normalize_base_url(bad).is_err(),"{bad}");}
        assert_eq!(normalize_base_url(" https://relay.example/prefix/v1/ ").unwrap(),"https://relay.example/prefix/v1");
        assert_eq!(normalize_base_url("https://relay.example/").unwrap(),"https://relay.example");
    }
    fn new_fixture()->Value {json!({"code":true,"data":{"object":"token_usage","total_granted":500000,"total_available":250000,"total_used":250000,"unlimited_quota":false,"expires_at":1900000000}})}
    fn unsupported()->ProbeError{ProbeError::Unsupported(response_error(404))}
    #[test]
    fn auto_prefers_key_quota_and_keeps_account_quota_separate(){
        let mut paths=vec![];let mut value=new_fixture();value["data"]["user_quota"]=json!({"total_available":12,"total_used":8});
        let (kind,snapshot)=detect_with(|path,timeout|{assert!(timeout<=PROBE_TIMEOUT);paths.push(path.to_owned());Ok(value.clone())},||false,AUTO_TIMEOUT).unwrap();
        assert_eq!(kind,Kind::NewApi);assert_eq!(paths,["/user/balance","/api/usage/token/"]);
        let q=snapshot.relay.unwrap();assert_eq!(q.amounts[0].label,"令牌额度");assert_eq!(q.amounts[0].remaining,Some(250000.0));
        assert_eq!(q.amounts[1].label,"账户额度");assert_eq!(q.amounts[1].remaining,Some(12.0));assert!(q.amounts.iter().all(|a|a.unit=="站点配额"));
    }
    #[test]
    fn auto_falls_back_to_sub2api_before_compatible_billing(){
        let mut paths=vec![];
        let (kind,snapshot)=detect_with(|path,_|{paths.push(path.to_owned());if path=="/v1/usage"{Ok(json!({"isValid":true,"mode":"unrestricted","unit":"USD","balance":7}))}else{Err(unsupported())}},||false,AUTO_TIMEOUT).unwrap();
        assert_eq!(kind,Kind::Sub2Api);assert_eq!(paths,["/user/balance","/api/usage/token/","/v1/usage"]);
        let q=snapshot.relay.unwrap();assert_eq!(q.amounts[0].label,"账户余额");assert_eq!(q.compact(),"7 USD");assert!(snapshot.windows.is_empty());
    }
    #[test]
    fn auto_billing_uses_at_most_five_requests_and_does_not_invent_scope_or_currency(){
        let mut paths=vec![];
        let (kind,snapshot)=detect_with(|path,_|{paths.push(path.to_owned());match path{
            "/dashboard/billing/subscription"=>Ok(json!({"object":"billing_subscription","hard_limit_usd":100})),
            "/dashboard/billing/usage"=>Ok(json!({"object":"list","total_usage":2500})),_=>Err(unsupported()),
        }},||false,AUTO_TIMEOUT).unwrap();
        assert_eq!(kind,Kind::OneApi);assert_eq!(paths.len(),MAX_PROBE_REQUESTS);
        let q=snapshot.relay.unwrap();assert_eq!(q.source,"兼容计费接口");assert_eq!(q.amounts[0].remaining,Some(75.0));assert_eq!(q.amounts[0].unit,"站点单位");
        assert!(q.full().contains("余额暂不可查询"));assert!(!q.full().contains("USD")&&!q.full().contains('$'));
    }
    #[test]
    fn auto_stops_on_broken_recognized_quota_instead_of_accepting_a_different_balance(){
        for value in [json!({"code":true,"data":{"object":"token_usage"}}),json!({"code":false,"message":"sk-sensitive"}),json!({"success":false}),json!({"error":{"message":"sk-sensitive"}})] {
            let mut calls=0;let error=detect_with(|_,_|{calls+=1;Ok(value.clone())},||false,AUTO_TIMEOUT).unwrap_err().0;
            assert_eq!(calls,if value["data"]["object"]=="token_usage"{2}else{1});assert!(!error.contains("sk-sensitive"));
        }
    }
    #[test]
    fn auto_does_not_accept_conflicting_currency_or_missing_billing_usage(){
        let mut value=new_fixture();value["data"]["unit"]=json!("USD");
        assert!(detect_with(|_,_|Ok(value.clone()),||false,AUTO_TIMEOUT).unwrap_err().0.contains("单位"));
        for value in [json!({"isValid":true,"mode":"quota_limited","quota":{"unit":"CNY","limit":10,"used":1,"remaining":9}}),json!({"isValid":true,"mode":"quota_limited","rate_limits":[{"window":"1d","unit":"requests","limit":10,"used":1,"remaining":9}]})] {
            let mut calls=0;let error=detect_with(|path,_|{calls+=1;if path=="/v1/usage"{Ok(value.clone())}else{Err(unsupported())}},||false,AUTO_TIMEOUT).unwrap_err().0;
            assert!(error.contains("单位"));assert_eq!(calls,3);
        }
        let error=detect_with(|path,_|if path.ends_with("subscription"){Ok(json!({"object":"billing_subscription","hard_limit_usd":10}))}else{Err(unsupported())},||false,AUTO_TIMEOUT).unwrap_err().0;
        assert!(error.contains("不完整"));
    }
    #[test]
    fn auto_unknown_shapes_never_become_zero_balances(){
        let mut calls=0;let error=detect_with(|_,_|{calls+=1;Ok(json!({"unrecognized":0}))},||false,AUTO_TIMEOUT).unwrap_err().0;
        assert_eq!(calls,4);assert!(error.contains("高级设置"));
        let mut value=new_fixture();value["data"]["unlimited_quota"]=json!(true);
        let (_,snapshot)=detect_with(|_,_|Ok(value.clone()),||false,AUTO_TIMEOUT).unwrap();assert_eq!(snapshot.relay.unwrap().amounts[0].remaining,None);
    }
    #[test]
    fn auto_budget_and_cancellation_stop_further_requests_and_reject_late_success(){
        let mut calls=0;assert!(detect_with(|_,_|{calls+=1;Ok(new_fixture())},||true,AUTO_TIMEOUT).is_err());assert_eq!(calls,0);
        let error=detect_with(|_,_|{calls+=1;Ok(new_fixture())},||false,Duration::ZERO).unwrap_err().0;assert!(error.contains("超时"));assert_eq!(calls,0);
        let cancelled=std::cell::Cell::new(false);
        assert!(detect_with(|_,_|{calls+=1;cancelled.set(true);Err(unsupported())},||cancelled.get(),AUTO_TIMEOUT).is_err());assert_eq!(calls,1);
        let error=detect_with(|_,timeout|{assert!(timeout<=Duration::from_millis(20));std::thread::sleep(Duration::from_millis(40));Ok(new_fixture())},||false,Duration::from_millis(20)).unwrap_err().0;assert!(error.contains("超时"));
    }
    #[test]
    fn auto_http_auth_rate_limit_redirect_and_server_errors_do_not_probe_other_routes(){
        let agent=ureq::AgentBuilder::new().try_proxy_from_env(false).redirects(0).timeout(Duration::from_secs(2)).build();
        for status in [401,403,429,302,500]{
            let body=r#"{"message":"sk-sensitive-response"}"#;
            let response=format!("HTTP/1.1 {status} Error\r\nContent-Length: {}\r\nLocation: https://must-not-connect.invalid\r\nConnection: close\r\n\r\n{body}",body.len()).into_bytes();
            let (url,worker)=serve_once(response,Duration::ZERO);let mut calls=0;
            let (error,_)=detect_with(|_,timeout|{calls+=1;query(&agent,&url,"synthetic-auto-key",Some(timeout))},||false,AUTO_TIMEOUT).unwrap_err();
            let request=worker.join().unwrap();assert!(request.contains("Bearer synthetic-auto-key"));assert_eq!(calls,1);assert!(!error.contains("sensitive")&&!error.contains("synthetic-auto"));
        }
    }
    #[test]
    fn auto_rejects_bad_addresses_and_keys_before_any_network_request(){
        let agent=ureq::AgentBuilder::new().redirects(0).build();
        for base in ["http://must-not-resolve.invalid","https://u:p@must-not-resolve.invalid","https://must-not-resolve.invalid?key=secret"]{assert!(detect_cancellable(|_|agent.clone(),base,"synthetic",||false).is_err());}
        for key in ["","bad\nkey"]{assert!(detect_cancellable(|_|agent.clone(),"https://must-not-resolve.invalid/prefix",key,||false).is_err());}
    }
    #[test]
    fn auto_http_flow_preserves_origin_prefix_and_bearer_with_bounded_clients(){
        let calls=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));let history=calls.clone();
        let (kind,snapshot)=detect_cancellable(move|timeout|{
            assert!(!timeout.is_zero()&&timeout<=PROBE_TIMEOUT);let history=history.clone();
            ureq::AgentBuilder::new().redirects(0).timeout_connect(timeout).middleware(move|request:ureq::Request,_:ureq::MiddlewareNext|{
                assert_eq!(request.header("Authorization"),Some("Bearer synthetic-prefix-key"));
                history.lock().unwrap().push(request.url().to_owned());
                let body=if request.url().ends_with("/v1/usage"){r#"{"isValid":true,"mode":"unrestricted","unit":"USD","balance":7}"#}else{"<!doctype html>"};
                ureq::Response::new(200,"OK",body)
            }).build()
        },"https://relay.example/tenant/v1","synthetic-prefix-key",||false).unwrap();
        assert_eq!(kind,Kind::Sub2Api);assert_eq!(snapshot.relay.unwrap().compact(),"7 USD");
        assert_eq!(*calls.lock().unwrap(),["https://relay.example/tenant/v1/user/balance","https://relay.example/tenant/v1/api/usage/token/","https://relay.example/tenant/v1/usage"]);
    }
    #[test] fn new_api_preserves_units_and_expiry_is_not_reset(){
        let s=parse_new_api(&new_fixture()).unwrap();assert_eq!(s.windows[0].remaining,50.0);assert_eq!(s.windows[0].reset,None);
        let q=s.relay.unwrap();assert_eq!(q.amounts[0].unit,"站点配额");assert!(!q.full().contains('$'));assert_eq!(q.expires_at,Some(1900000000));
    }
    #[test] fn missing_zero_and_unlimited_are_distinct(){
        let mut v=new_fixture();v["data"]["total_available"]=Value::Null;assert!(parse_new_api(&v).is_err());
        v["data"]["total_available"]=json!(0);assert_eq!(parse_new_api(&v).unwrap().windows[0].remaining,0.0);
        v["data"]["unlimited_quota"]=json!(true);let s=parse_new_api(&v).unwrap();assert!(s.windows.is_empty());let q=s.relay.unwrap();assert!(q.amounts[0].unlimited);assert_eq!(q.amounts[0].remaining,None);
        v["code"]=json!(false);assert!(parse_new_api(&v).is_err());
    }
    #[test] fn one_api_has_configurable_units_and_cent_scaled_usage(){
        let sub=json!({"object":"billing_subscription","hard_limit_usd":100});let usage=json!({"object":"list","total_usage":2500});
        let s=parse_one_api(&sub,&usage).unwrap();assert_eq!(s.windows[0].remaining,75.0);let q=s.relay.unwrap();assert_eq!(q.amounts[0].remaining,Some(75.0));assert_eq!(q.amounts[0].unit,"站点单位");
        let mut sentinel=sub.clone();sentinel["hard_limit_usd"]=json!(100000000);let s=parse_one_api(&sentinel,&usage).unwrap();assert!(s.windows.is_empty());assert_eq!(s.relay.unwrap().amounts[0].remaining,None);
        assert!(parse_one_api(&sub,&json!({"object":"list"})).is_err());assert!(parse_one_api(&json!({"error":{"message":"secret"}}),&usage).is_err());
    }
    #[test] fn sub2api_wallet_does_not_become_a_percentage(){
        let s=parse_sub2api(&json!({"mode":"unrestricted","isValid":true,"unit":"USD","balance":0,"remaining":0})).unwrap();assert!(s.windows.is_empty());assert_eq!(s.relay.unwrap().compact(),"0 USD");
        assert!(parse_sub2api(&json!({"mode":"unrestricted","isValid":true,"unit":"USD"})).is_err());
    }
    #[test] fn sub2api_retains_all_key_windows_and_explicit_reset(){
        let s=parse_sub2api(&json!({"mode":"quota_limited","isValid":true,"quota":{"limit":10,"used":2,"remaining":8,"unit":"USD"},"rate_limits":[{"window":"5h","limit":5,"used":1,"remaining":4,"reset_at":"2030-01-01T00:00:00Z"},{"window":"1d","limit":20,"used":0,"remaining":20}]})).unwrap();
        assert_eq!(s.windows.len(),3);assert_eq!(s.windows[1].remaining,80.0);assert!(s.windows[1].reset.is_some());assert_eq!(s.windows[2].reset,None);
        assert!(parse_sub2api(&json!({"mode":"quota_limited","isValid":true,"quota":{"limit":10,"used":2,"unit":"USD"}})).is_err());
    }
    #[test] fn duplicate_sub2api_periods_are_rejected(){
        let row=json!({"window":"5h","limit":5,"used":1,"remaining":4});
        assert!(parse_sub2api(&json!({"mode":"quota_limited","isValid":true,"rate_limits":[row.clone(),row]})).is_err());
    }
    #[test] fn sub2api_subscription_missing_and_unlimited_do_not_become_zero(){
        let s=parse_sub2api(&json!({"mode":"unrestricted","isValid":true,"unit":"USD","remaining":3,"subscription":{"daily_limit_usd":10,"daily_usage_usd":7,"weekly_limit_usd":null}})).unwrap();assert_eq!(s.windows.len(),1);assert_eq!(s.windows[0].remaining,30.0);
        let s=parse_sub2api(&json!({"mode":"unrestricted","isValid":true,"unit":"USD","remaining":-1,"subscription":{"daily_limit_usd":null}})).unwrap();assert!(s.relay.unwrap().amounts[0].unlimited);
        assert!(parse_sub2api(&json!({"mode":"unrestricted","isValid":true,"unit":"USD","remaining":0,"subscription":{}})).is_err());
    }
    #[test] fn finite_zero_limit_is_not_unlimited(){
        let s=parse_one_api(&json!({"object":"billing_subscription","hard_limit_usd":0}),&json!({"object":"list","total_usage":0})).unwrap();
        assert!(s.windows.is_empty());let q=s.relay.unwrap();assert_eq!(q.amounts[0].remaining,Some(0.0));assert!(!q.amounts[0].unlimited);
        let mut v=new_fixture();v["data"]["total_granted"]=json!(0);v["data"]["total_available"]=json!(0);v["data"]["total_used"]=json!(0);
        let s=parse_new_api(&v).unwrap();assert!(s.windows.is_empty());assert!(!s.relay.unwrap().amounts[0].unlimited);
    }
    #[test] fn tiny_positive_amounts_are_not_displayed_as_zero(){assert_ne!(number(0.000001),"0");assert_eq!(number(0.0),"0");}
    #[test] fn quota_cache_roundtrip_preserves_unknown_and_unlimited(){
        let mut v=new_fixture();v["data"]["unlimited_quota"]=json!(true);
        let q=parse_new_api(&v).unwrap().relay.unwrap();let raw=serde_json::to_string(&q).unwrap();
        let restored:Quota=serde_json::from_str(&raw).unwrap();assert_eq!(restored.amounts[0].remaining,None);assert!(restored.amounts[0].unlimited);assert_eq!(restored.compact(),"余额暂不可查询");
    }
    #[test] fn balance_display_does_not_substitute_key_budgets_for_wallets(){
        for value in [false,true]{
            let mut v=new_fixture();v["data"]["unlimited_quota"]=json!(value);
            let q=parse_new_api(&v).unwrap().relay.unwrap();
            assert_eq!(q.compact(),"余额暂不可查询");
            assert!(!q.full().contains("令牌额度")&&!q.full().contains("不限额"));
        }
        let q=parse_sub2api(&json!({"mode":"quota_limited","isValid":true,"quota":{"limit":10,"used":2,"remaining":8,"unit":"USD"},"rate_limits":[{"window":"7d","limit":20,"used":20,"remaining":0}]})).unwrap().relay.unwrap();
        assert_eq!(q.compact(),"余额暂不可查询");assert!(!q.full().contains("API Key 总额度"));
        let q=parse_one_api(&json!({"object":"billing_subscription","hard_limit_usd":100}),&json!({"object":"list","total_usage":2500})).unwrap().relay.unwrap();
        assert_eq!(q.compact(),"余额暂不可查询");
    }
    #[test] fn balance_display_prefers_account_data_and_keeps_zero_balances(){
        for remaining in [0,500000]{
            let mut v=new_fixture();v["data"]["total_available"]=json!(1);v["data"]["user_quota"]=json!({"total_available":remaining,"total_used":8});
            let q=parse_new_api(&v).unwrap().relay.unwrap();
            assert_eq!(q.compact(),format!("{remaining} 站点配额"));
            assert!(q.full().contains("账户额度"));assert!(!q.full().contains("令牌额度"));
        }
    }
    #[test] fn balance_display_subscription_uses_remaining_percentages(){
        let q=parse_sub2api(&json!({"mode":"unrestricted","isValid":true,"unit":"USD","remaining":3,"subscription":{"daily_limit_usd":10,"daily_usage_usd":7,"weekly_limit_usd":100,"weekly_usage_usd":60}})).unwrap().relay.unwrap();
        assert_eq!(q.compact(),"每日剩余 30%");assert!(q.full().contains("剩余 30%"));
        let q=parse_sub2api(&json!({"mode":"unrestricted","isValid":true,"unit":"USD","remaining":-1,"subscription":{"daily_limit_usd":null}})).unwrap().relay.unwrap();
        assert_eq!(q.compact(),"额度暂不可查询");assert!(!q.full().contains("不限额"));
    }
    #[test] fn balance_display_legacy_cache_does_not_restore_key_limits(){
        let q:Quota=serde_json::from_value(json!({"source":"Sub2API","amounts":[{"label":"每日","unit":"USD","remaining":20,"limit":100,"used":80,"unlimited":false}],"expires_at":null})).unwrap();
        assert_eq!(q.compact(),"余额暂不可查询");assert!(!q.full().contains("20"));
    }
    #[test] fn untrusted_errors_and_nonfinite_numbers_are_not_exposed(){
        let v=json!({"code":false,"message":"sk-secret https://private.example"});let error=parse_new_api(&v).unwrap_err();assert!(!error.contains("secret"));assert!(!error.contains("private"));
        let mut v=new_fixture();v["data"]["total_used"]=json!("NaN");assert!(parse_new_api(&v).is_err());
        assert_eq!(response_error(429).1,180);assert!(response_error(302).0.contains("重定向"));
    }
}
