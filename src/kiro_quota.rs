//! Kiro 额度：只读登录缓存与 GetUsageLimits，不回写、不续期；令牌是否失效以服务端 401/403 为准（本地 expiresAt 只是客户端的续期计划时间）。
use super::{Account, Amount, Config, Provider, Snapshot, Window};
use serde_json::{json, Value};
use std::{io::Read, path::{Path, PathBuf}, sync::mpsc};

pub(crate) fn path()->Option<PathBuf>{
    Some(PathBuf::from(std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME"))?).join(".aws/sso/cache/kiro-auth-token.json"))
}

struct Login { access_token:String, profile_arn:Option<String>, region:String }
fn endpoint(region:&str)->Result<&'static str,String>{
    match region{
        "us-east-1"=>Ok("https://codewhisperer.us-east-1.amazonaws.com/"),
        "eu-central-1"=>Ok("https://q.eu-central-1.amazonaws.com/"),
        _=>Err("暂不支持此 Kiro 区域，请使用美国东部或欧洲中部区域的登录".into()),
    }
}
fn login(root:&Value)->Result<Login,String>{
    for field in ["profileArn","region"]{if root.get(field).is_some_and(|v|!v.is_null()&&!v.is_string()){return Err("Kiro 登录区域或 profileArn 字段无效".into());}}
    let token=root["accessToken"].as_str().filter(|s|!s.is_empty()&&s.len()<=65536&&s.is_ascii()&&!s.bytes().any(|b|b<=32||b==127)).ok_or("未找到有效的 Kiro 登录凭据，请先在 Kiro 中登录")?.to_owned();
    let profile_arn=root["profileArn"].as_str().map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned);
    let arn_region=if let Some(arn)=profile_arn.as_deref(){
        let parts=arn.splitn(6,':').collect::<Vec<_>>();
        if parts.len()!=6||parts[0]!="arn"||parts[1]!="aws"||!matches!(parts[2],"codewhisperer"|"qdeveloper")||parts[3].is_empty(){return Err("Kiro 登录中的 profileArn 无效".into());}
        Some(parts[3])
    }else{None};
    let explicit=root["region"].as_str().map(str::trim).filter(|s|!s.is_empty());
    if explicit.zip(arn_region).is_some_and(|(a,b)|a!=b){return Err("Kiro 登录区域与 profileArn 不一致，请在 Kiro 中重新登录".into());}
    let region=arn_region.or(explicit).unwrap_or("us-east-1").to_owned();endpoint(&region)?;
    if root.get("expiresAt").is_some_and(|v|!v.is_null()&&v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()).is_none()){
        return Err("Kiro 登录过期时间无效，请在 Kiro 中重新登录".into());
    }
    Ok(Login{access_token:token,profile_arn,region})
}
fn body(profile_arn:Option<&str>)->Value{
    match profile_arn.filter(|arn|!arn.trim().is_empty()){Some(arn)=>json!({"profileArn":arn}),None=>json!({})}
}
fn owner(v:&Value)->Result<String,String>{
    let user=v["userInfo"]["userId"].as_str().map(str::trim).filter(|s|!s.is_empty()&&s.len()<=4096).ok_or("Kiro 未返回可核验的用户标识，暂不显示此账号额度")?;
    let hash=ring::digest::digest(&ring::digest::SHA256,user.as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>();
    Ok(format!("kiro-user:{hash}"))
}
pub(super) fn verified_slot(slot:&str)->bool{slot.strip_prefix("kiro-user:").is_some_and(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))}
pub(crate) fn check_owner(account:&Account,snapshot:&Snapshot)->Result<(),String>{
    let actual=snapshot.kiro.as_ref().map(|d|d.owner.as_str()).filter(|s|verified_slot(s)).ok_or("Kiro 用户身份尚未核验，请重新连接")?;
    if account.slot.as_deref().is_some_and(|s|s!=actual){return Err("Kiro 已切换账号或身份尚未核验，请重新连接对应账号".into());}
    Ok(())
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pool {pub label:String,pub amount:Amount,pub reset:Option<i64>,pub expires:Option<i64>}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Details {pub owner:String,pub pools:Vec<Pool>}

pub(crate) fn add(config:&mut Config,path:&Path,alias:&str)->Result<(),String>{
    let root=super::login_json(path)?;login(&root)?;
    let path=std::fs::canonicalize(path).map_err(|_|"无法读取 Kiro 登录文件")?;
    if config.accounts.iter().any(|a|a.provider==Provider::Kiro&&a.source==path){return Err("这个 Kiro 登录文件已经添加，请使用重新连接".into());}
    let next=config.accounts.iter().filter(|a|a.provider==Provider::Kiro).count()+1;
    let id=format!("Kiro-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    if config.selected.is_empty(){config.selected=id.clone();}
    // 首次查询后绑定服务端用户标识，客户端注册及令牌不作为用户身份。
    config.accounts.push(Account{id,provider:Provider::Kiro,alias:if alias.trim().is_empty(){format!("Kiro · 账号 {next}")}else{alias.trim().into()},source:path,slot:None,enabled:true,pinned:config.accounts.is_empty()});
    Ok(())
}
fn response_error(error:ureq::Error)->(String,u64){
    match error{
        ureq::Error::Status(401|403,_) =>("Kiro 登录已过期，请打开 Kiro 重新登录后刷新额度".into(),0),
        ureq::Error::Status(429,r) =>("Kiro 请求过于频繁，稍后自动重试".into(),r.header("Retry-After").and_then(|s|s.parse::<u64>().ok()).unwrap_or(180).clamp(30,3600)),
        ureq::Error::Status(code,_) =>(format!("Kiro 额度服务返回 {code}"),60),
        _ =>("无法连接 Kiro 额度服务，请检查网络或代理".into(),30),
    }
}
fn request_at(agent:&ureq::Agent,l:&Login,url:&str)->Result<Snapshot,(String,u64)>{
    let response=agent.post(url).set("Content-Type","application/x-amz-json-1.0")
        .set("X-Amz-Target","AmazonCodeWhispererService.GetUsageLimits")
        .set("Authorization",&format!("Bearer {}",l.access_token)).set("Accept","application/json")
        .send_string(&body(l.profile_arn.as_deref()).to_string()).map_err(response_error)?;
    if !(200..300).contains(&response.status()){return Err(response_error(ureq::Error::Status(response.status(),response)));}
    let mut bytes=Vec::new();response.into_reader().take(1024*1024+1).read_to_end(&mut bytes).map_err(|_|("读取 Kiro 额度响应失败".into(),30))?;
    if bytes.len()>1024*1024{return Err(("Kiro 额度响应过大".into(),60));}
    let v:Value=serde_json::from_slice(&bytes).map_err(|_|("Kiro 额度响应无效".into(),60))?;
    parse(&v).map_err(|e|(e,60))
}
fn query(agent:&ureq::Agent,path:&Path)->Result<Snapshot,(String,u64)>{
    query_with(path,|l|request_at(agent,l,endpoint(&l.region).map_err(|e|(e,0))?))
}
fn query_with(path:&Path,request:impl FnOnce(&Login)->Result<Snapshot,(String,u64)>)->Result<Snapshot,(String,u64)>{
    let root=super::login_json(path).map_err(|e|(e,0))?;let l=login(&root).map_err(|e|(e,0))?;
    // expiresAt 只是 Kiro 客户端的续期计划时间，客户端懒续期，过了这个点令牌在服务端通常仍然有效；
    // 直接带令牌请求，由服务端 401/403 判定失效。Kiro 续期后改写缓存文件，每次查询都重新读取，能自动恢复。
    let snapshot=request(&l)?;
    let current=super::login_json(path).and_then(|root|login(&root)).map_err(|e|(e,0))?;
    if current.access_token!=l.access_token||current.profile_arn!=l.profile_arn||current.region!=l.region{return Err(("Kiro 登录信息已变化，请重新刷新额度".into(),0));}
    Ok(snapshot)
}
pub(super) fn fetch(agent:&ureq::Agent,a:&Account)->Result<Snapshot,(String,u64)>{
    if a.slot.as_deref().is_some_and(|s|!verified_slot(s)){return Err(("Kiro 账号身份需要重新核验，请重新连接".into(),0));}
    let snapshot=query(agent,&a.source)?;check_owner(a,&snapshot).map_err(|e|(e,0))?;Ok(snapshot)
}

pub struct Connected {pub source:PathBuf,pub label:String,pub snapshot:Snapshot}
pub struct Connect {rx:mpsc::Receiver<Result<Connected,String>>,pub target:Option<Account>,cancelled:bool}
impl Connect {
    pub fn start(path:PathBuf,label:String,target:Option<Account>)->Self{
        let(tx,rx)=mpsc::channel();std::thread::spawn(move||{
            let result=(||{let source=std::fs::canonicalize(path).map_err(|_|"无法读取 Kiro 登录文件")?;
                let snapshot=query(&super::http_agent(),&source).map_err(|e|e.0)?;Ok(Connected{source,label,snapshot})})();let _=tx.send(result);
        });Self{rx,target,cancelled:false}
    }
    pub fn poll(&self)->Option<Result<Connected,String>>{self.rx.try_recv().ok()}
    pub fn cancel(&mut self){self.cancelled=true;}
    pub fn cancelled(&self)->bool{self.cancelled}
    #[cfg(test)]
    pub(crate) fn pending(target:Option<Account>)->(Self,mpsc::Sender<Result<Connected,String>>){let(tx,rx)=mpsc::channel();(Self{rx,target,cancelled:false},tx)}
}
pub(crate) fn apply_connection(config:&mut Config,target:Option<&Account>,connected:&Connected)->Result<String,String>{
    let slot=connected.snapshot.kiro.as_ref().map(|d|d.owner.clone()).filter(|s|verified_slot(s)).ok_or("Kiro 用户身份尚未核验")?;
    let index=if let Some(old)=target{
        let index=config.accounts.iter().position(|a|a.id==old.id&&a.provider==Provider::Kiro&&a.source==old.source&&(a.slot==old.slot||(old.slot.is_none()&&a.slot.as_deref()==Some(&slot)))).ok_or("原 Kiro 账号已变化，请重新连接")?;
        if old.slot.as_deref().filter(|s|verified_slot(s)).is_some_and(|s|s!=slot){return Err("本机 Kiro 当前不是这个账号，请切换回原账号或添加新账号".into());}Some(index)
    }else{config.accounts.iter().position(|a|a.provider==Provider::Kiro&&a.slot.is_none()&&a.source==connected.source)};
    if config.accounts.iter().enumerate().any(|(i,a)|Some(i)!=index&&a.provider==Provider::Kiro&&a.slot.as_deref()==Some(&slot)){return Err("这个 Kiro 账号已经添加，请使用重新连接".into());}
    let id=if let Some(index)=index{
        let a=&mut config.accounts[index];a.source=connected.source.clone();a.slot=Some(slot);a.enabled=true;if !connected.label.trim().is_empty(){a.alias=connected.label.trim().into();}a.id.clone()
    }else{
        let id=format!("Kiro-{slot}");let pinned=config.accounts.is_empty();
        config.accounts.push(Account{id:id.clone(),provider:Provider::Kiro,alias:if connected.label.trim().is_empty(){"Kiro".into()}else{connected.label.trim().into()},source:connected.source.clone(),slot:Some(slot),enabled:true,pinned});id
    };config.selected=id.clone();Ok(id)
}
fn timestamp(v:&Value)->Option<i64>{
    if let Some(t)=v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()){return Some(t.timestamp()).filter(|t|*t>0&&*t<=4_102_444_800);}
    let t=super::number(v)?;let t=if t>=1_000_000_000_000.0{t/1000.0}else{t};(t>0.0&&t<=4_102_444_800.0).then_some(t as i64)
}
fn amount_field(v:&Value,precise:&str,integer:&str)->Result<f64,String>{
    let raw=v.get(precise).filter(|v|!v.is_null()).unwrap_or(&v[integer]);
    super::number(raw).filter(|n|*n>=0.0).ok_or_else(||"Kiro 额度字段无效，暂时无法显示".into())
}
fn pool(v:&Value,label:String,reset:Option<i64>,expires:Option<i64>)->Result<Pool,String>{
    let limit=amount_field(v,"usageLimitWithPrecision","usageLimit")?;let used=amount_field(v,"currentUsageWithPrecision","currentUsage")?;
    Ok(Pool{label,amount:Amount::new((limit-used).max(0.0),limit),reset,expires})
}
fn active_pool(v:&Value,status:&str,expiry:&str,label:&str)->Result<Option<Pool>,String>{
    if v.is_null(){return Ok(None);}
    let status=v[status].as_str().ok_or("Kiro 积分池状态缺失，暂时无法显示完整点数")?;
    if status.eq_ignore_ascii_case("EXPIRED")||status.eq_ignore_ascii_case("INACTIVE"){return Ok(None);}
    if !status.eq_ignore_ascii_case("ACTIVE"){return Err("Kiro 积分池状态未知，暂时无法显示完整点数".into());}
    let expires=if v[expiry].is_null(){None}else{Some(timestamp(&v[expiry]).ok_or("Kiro 额度到期时间无效")?)};
    if expires.is_some_and(|t|t<=chrono::Utc::now().timestamp()){return Ok(None);}
    pool(v,label.into(),None,expires).map(Some)
}
pub(super) fn parse(v:&Value)->Result<Snapshot,String>{
    let owner=owner(v)?;
    let plan=v["subscriptionInfo"]["subscriptionTitle"].as_str().filter(|s|!s.trim().is_empty()).unwrap_or("Kiro").to_owned();
    let mut snapshot=Snapshot{plan,at:chrono::Utc::now().timestamp(),..Default::default()};
    let rows=v["usageBreakdownList"].as_array().ok_or("Kiro 未返回用量明细")?;
    let credit=rows.iter().find(|e|e["resourceType"]=="CREDIT").ok_or("Kiro 未返回套餐额度")?;
    let limit=amount_field(credit,"usageLimitWithPrecision","usageLimit")?;let used=amount_field(credit,"currentUsageWithPrecision","currentUsage")?;
    let overage=if credit["currentOveragesWithPrecision"].is_null()&&credit["currentOverages"].is_null(){0.0}else{amount_field(credit,"currentOveragesWithPrecision","currentOverages")?};
    let reset=timestamp(&credit["nextDateReset"]).or_else(||timestamp(&v["nextDateReset"]));
    let mut pools=vec![Pool{label:"月度额度".into(),amount:Amount::new((limit-(used-overage).max(0.0)).max(0.0),limit),reset,expires:None}];
    if let Some(p)=active_pool(&credit["freeTrialInfo"],"freeTrialStatus","freeTrialExpiry","试用额度")?{pools.push(p);}
    let bonuses=match &credit["bonuses"]{
        Value::Null=>&[][..],
        Value::Array(items)=>items.as_slice(),
        item @ Value::Object(_)=>std::slice::from_ref(item),
        _=>return Err("Kiro 赠送额度格式异常，暂时无法显示完整点数".into()),
    };
    for bonus in bonuses{
        if !bonus.is_object(){return Err("Kiro 赠送额度明细无效，暂时无法显示完整点数".into());}
        if let Some(p)=active_pool(bonus,"status","expiresAt",bonus["displayName"].as_str().filter(|s|!s.trim().is_empty()).unwrap_or("赠送 / 加购额度"))?{pools.push(p);}
    }
    let total=Amount::new(pools.iter().map(|p|p.amount.remaining).sum(),pools.iter().map(|p|p.amount.limit).sum());
    if !total.limit.is_finite()||!total.remaining.is_finite(){return Err("Kiro 额度总量无效".into());}
    snapshot.windows.push(Window{label:if pools.len()>1{"可用点数"}else{"月度额度"}.into(),seconds:None,remaining:if total.limit>0.0{(total.remaining/total.limit*100.0) as f32}else{0.0},reset:if pools.len()>1{None}else{reset},amount:Some(total)});
    if v["overageConfiguration"]["overageStatus"].as_str().is_some_and(|s|s.eq_ignore_ascii_case("ENABLED")){
        if !credit["overageCapWithPrecision"].is_null()||!credit["overageCap"].is_null(){
            let cap=amount_field(credit,"overageCapWithPrecision","overageCap")?;
            if cap>0.0{let left=(cap-overage).max(0.0);snapshot.windows.push(Window{label:"超额上限余量".into(),seconds:None,remaining:(left/cap*100.0).clamp(0.0,100.0) as f32,reset,amount:Some(Amount::new(left,cap))});}
        }
    }
    snapshot.kiro=Some(Details{owner,pools});Ok(snapshot)
}
#[cfg(test)]
mod regressions {
    use super::*;
    #[test]
    fn arn_region_is_used_when_region_field_is_missing(){
        let l=login(&json!({"accessToken":"synthetic","profileArn":"arn:aws:codewhisperer:eu-central-1:123:profile/test"})).unwrap();
        assert_eq!(l.region,"eu-central-1");
    }
    #[test]
    fn untrusted_region_cannot_change_the_aws_hostname(){
        assert!(login(&json!({"accessToken":"synthetic","region":"attacker.invalid/"})).is_err());
    }
    #[test]
    fn active_trial_credits_are_included_in_available_points(){
        let s=parse(&json!({"userInfo":{"userId":"test-user"},"usageBreakdownList":[{"resourceType":"CREDIT","usageLimitWithPrecision":50.0,"currentUsageWithPrecision":50.0,
            "freeTrialInfo":{"freeTrialStatus":"ACTIVE","usageLimitWithPrecision":500.0,"currentUsageWithPrecision":100.0,"freeTrialExpiry":4102444800.0}}]})).unwrap();
        assert_eq!(s.windows[0].amount.as_ref().unwrap().remaining,400.0);
    }
    #[test]
    fn expired_bonus_does_not_change_monthly_usage(){
        let s=parse(&json!({"userInfo":{"userId":"test-user"},"usageBreakdownList":[{"resourceType":"CREDIT","usageLimitWithPrecision":1000.0,"currentUsageWithPrecision":130.5,
            "currentOveragesWithPrecision":30.5,"bonuses":[{"status":"EXPIRED","usageLimitWithPrecision":0.0,"currentUsageWithPrecision":0.0}]}]})).unwrap();
        assert_eq!(s.windows[0].amount.as_ref().unwrap().remaining,900.0);
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn plan_and_overage_map_to_remaining_credits(){
        let s=parse(&json!({"userInfo":{"userId":"test-user"},"usageBreakdownList":[{"resourceType":"CREDIT","currentUsageWithPrecision":130.5,"usageLimitWithPrecision":1000.0,
            "currentOveragesWithPrecision":30.5,"overageCapWithPrecision":200.0,"nextDateReset":1900000000.0}],"overageConfiguration":{"overageStatus":"ENABLED"}})).unwrap();
        assert_eq!(s.windows.len(),2);
        assert_eq!(s.windows[0].label,"月度额度");assert_eq!(s.windows[0].remaining,90.0);assert_eq!(s.windows[0].reset,Some(1900000000));
        assert_eq!(s.windows[0].amount,Some(Amount::new(900.0,1000.0)));
        assert_eq!(s.windows[0].summary(),"月度额度剩余 900 / 1000");
        assert_eq!(s.windows[1].label,"超额上限余量");assert_eq!(s.windows[1].remaining,84.75);
        assert_eq!(s.windows[1].amount,Some(Amount::new(169.5,200.0)));assert_eq!(s.windows[1].summary(),"超额上限余量剩余 169.5 / 200");
    }
    #[test]fn amounts_keep_one_decimal_and_never_round_past_the_limit(){
        assert_eq!(Amount::new(904.09,1000.0).full(),"904.1 / 1000");
        assert_eq!(Amount::new(904.09,1000.0).short(),"904");
        assert_eq!(Amount::new(95.91,1000.0).short(),"95.9");
        assert_eq!(Amount::new(999.99,1000.0).full(),"1000 / 1000");
        assert_eq!(Amount::new(-4.0,10.0).full(),"0 / 10");
        assert_eq!(Amount::new(0.0,0.0).full(),"0 / 0");
        assert_eq!(Amount::new(0.01,1000.0).short(),"0.01");
        assert_eq!(Amount::new(20.0,10.0).full(),"10 / 10");
    }
    #[test]fn bonus_usage_cannot_exceed_the_plan_and_missing_data_is_an_error(){
        let s=parse(&json!({"userInfo":{"userId":"test-user"},"usageBreakdownList":[{"resourceType":"CREDIT","currentUsageWithPrecision":1200.0,"usageLimitWithPrecision":1000.0,
            "bonuses":[{"status":"EXPIRED","usageLimit":1,"currentUsage":0}],"nextDateReset":1900000000.0}]})).unwrap();
        assert_eq!(s.windows.len(),1);assert_eq!(s.windows[0].remaining,0.0);
        assert!(parse(&json!({"userInfo":{"userId":"test-user"},"usageBreakdownList":[]})).is_err());
        assert!(parse(&json!({})).is_err());
    }
    #[test]fn endpoint_follows_the_profile_region(){
        assert_eq!(endpoint("us-east-1").unwrap(),"https://codewhisperer.us-east-1.amazonaws.com/");
        assert_eq!(endpoint("eu-central-1").unwrap(),"https://q.eu-central-1.amazonaws.com/");
        assert!(endpoint("ap-northeast-1").is_err());
    }
    #[test]fn plan_title_and_idc_body(){
        assert_eq!(body(None),json!({}));assert_eq!(body(Some("  ")),json!({}));
        assert_eq!(body(Some("arn:aws:codewhisperer:us-east-1:1:profile/x")),json!({"profileArn":"arn:aws:codewhisperer:us-east-1:1:profile/x"}));
        let s=parse(&json!({"userInfo":{"userId":"test-user"},"subscriptionInfo":{"subscriptionTitle":"KIRO PRO"},"usageBreakdownList":[{"resourceType":"CREDIT",
            "currentUsageWithPrecision":95.91,"usageLimitWithPrecision":1000.0,"nextDateReset":1796083200.0}]})).unwrap();
        assert_eq!(s.plan,"KIRO PRO");assert!((s.windows[0].remaining-90.409).abs()<0.001);assert_eq!(s.windows[0].reset,Some(1796083200));
        assert_eq!(s.windows[0].summary(),"月度额度剩余 904.1 / 1000");
    }
    #[test]fn server_identity_survives_token_refresh_and_separates_idc_users(){
        let a=owner(&json!({"accessToken":"old","clientIdHash":"same-client","userInfo":{"userId":"user-a"}})).unwrap();
        let refreshed=owner(&json!({"accessToken":"new","clientIdHash":"same-client","userInfo":{"userId":"user-a"}})).unwrap();
        let b=owner(&json!({"accessToken":"other","clientIdHash":"same-client","userInfo":{"userId":"user-b"}})).unwrap();
        assert_eq!(a,refreshed);assert_ne!(a,b);assert!(verified_slot(&a));
        assert!(owner(&json!({"clientIdHash":"same-client","profileArn":"shared-profile"})).is_err());
    }
    fn payload()->Value{json!({"userInfo":{"userId":"test-user"},"usageBreakdownList":[{"resourceType":"CREDIT","usageLimitWithPrecision":1000.0,"currentUsageWithPrecision":100.0,"nextDateReset":4102444800_i64}]})}
    fn account(slot:Option<String>)->Account{Account{id:"kiro-test".into(),provider:Provider::Kiro,alias:"工作账号".into(),source:"old.json".into(),slot,enabled:true,pinned:true}}
    #[test]fn regions_and_headers_reject_ambiguous_or_untrusted_input(){
        for region in ["us-east-1.amazonaws.com.attacker.invalid/","us-east-1@attacker.invalid","eu-central-1?x=1","us-west-2"]{assert!(login(&json!({"accessToken":"fake","region":region})).is_err());}
        assert!(login(&json!({"accessToken":"fake","profileArn":"arn:aws:codewhisperer:eu-central-1:1:profile/a","region":"us-east-1"})).is_err());
        for token in ["", "fake\r\nInjected: value", "fake token"]{assert!(login(&json!({"accessToken":token})).is_err());}
        assert!(login(&json!({"accessToken":"fake","expiresAt":"invalid"})).is_err());
        assert!(login(&json!({"accessToken":"fake","region":123})).is_err());
    }
    #[test]fn trial_bonus_and_overage_have_separate_balances_and_dates(){
        let mut v=payload();let row=&mut v["usageBreakdownList"][0];
        row["freeTrialInfo"]=json!({"freeTrialStatus":"ACTIVE","usageLimit":500,"currentUsage":100,"freeTrialExpiry":4102444800000_i64});
        row["bonuses"]=json!([{"status":"ACTIVE","displayName":"加购点数","usageLimit":125,"currentUsage":25,"expiresAt":4102444800_i64}]);
        row["overageCapWithPrecision"]=json!(200);v["overageConfiguration"]=json!({"overageStatus":"ENABLED"});
        let s=parse(&v).unwrap();assert_eq!(s.windows[0].label,"可用点数");assert_eq!(s.windows[0].amount,Some(Amount::new(1400.0,1625.0)));assert_eq!(s.windows[0].reset,None);
        assert_eq!(s.windows[1].amount,Some(Amount::new(200.0,200.0)));let pools=s.kiro.unwrap().pools;
        assert_eq!(pools.len(),3);assert_eq!(pools[0].reset,Some(4102444800));assert_eq!(pools[1].expires,Some(4102444800));assert!(pools[1].reset.is_none());
    }
    #[test]
    fn single_bonus_matches_a_list_and_invalid_shapes_fail(){
        let mut v=payload();
        let bonus=json!({"status":"ACTIVE","displayName":"赠送","usageLimit":30,"currentUsage":0});
        for bonuses in [bonus.clone(),json!([bonus])]{
            v["usageBreakdownList"][0]["bonuses"]=bonuses;
            let s=parse(&v).unwrap();
            assert_eq!(s.windows[0].amount,Some(Amount::new(930.0,1030.0)));
            assert_eq!(s.kiro.unwrap().pools[1].label,"赠送");
        }
        for invalid in [json!("unknown"),json!({}),json!([null])]{
            v["usageBreakdownList"][0]["bonuses"]=invalid;
            assert!(parse(&v).is_err());
        }
    }
    #[test]fn inactive_or_expired_credits_are_excluded_and_invalid_amounts_fail(){
        let mut v=payload();v["usageBreakdownList"][0]["freeTrialInfo"]=json!({"freeTrialStatus":"ACTIVE","usageLimit":500,"currentUsage":0,"freeTrialExpiry":1000});
        v["usageBreakdownList"][0]["bonuses"]=json!([{"status":"EXPIRED","usageLimit":500,"currentUsage":0},{"status":"ACTIVE","usageLimit":500,"currentUsage":0,"expiresAt":1000}]);
        assert_eq!(parse(&v).unwrap().windows[0].amount,Some(Amount::new(900.0,1000.0)));
        for value in [json!(-1),json!("NaN"),json!("Infinity"),json!(true)]{v["usageBreakdownList"][0]["currentUsageWithPrecision"]=value;assert!(parse(&v).is_err());}
        let mut v=payload();v["usageBreakdownList"][0]["freeTrialInfo"]=json!({"freeTrialStatus":"ACTIVE","usageLimit":500,"currentUsage":"invalid"});assert!(parse(&v).is_err());
        for status in [Value::Null,json!("UNKNOWN"),json!(1)]{let mut v=payload();v["usageBreakdownList"][0]["bonuses"]=json!([{"status":status,"usageLimit":500,"currentUsage":0}]);assert!(parse(&v).is_err());}
    }
    #[test]fn zero_monthly_limit_can_still_have_valid_trial_points(){
        let mut v=payload();v["usageBreakdownList"][0]["usageLimitWithPrecision"]=json!(0);v["usageBreakdownList"][0]["currentUsageWithPrecision"]=json!(0);
        v["usageBreakdownList"][0]["freeTrialInfo"]=json!({"freeTrialStatus":"ACTIVE","usageLimit":500,"currentUsage":100});
        assert_eq!(parse(&v).unwrap().windows[0].amount,Some(Amount::new(400.0,500.0)));
    }
    #[test]fn reconnect_keeps_preferences_and_rejects_another_user_or_changed_target(){
        let snapshot=parse(&payload()).unwrap();let old=account(Some(snapshot.kiro.as_ref().unwrap().owner.clone()));
        let mut config=Config::default();config.accounts.push(old.clone());config.colors.insert(old.id.clone(),super::super::Accent::Gold);
        let connected=Connected{source:"new.json".into(),label:String::new(),snapshot};
        assert_eq!(apply_connection(&mut config,Some(&old),&connected).unwrap(),old.id);assert_eq!(config.accounts.len(),1);assert_eq!(config.accounts[0].source,connected.source);assert_eq!(config.accounts[0].alias,old.alias);assert!(config.accounts[0].pinned);assert_eq!(config.colors[&old.id],super::super::Accent::Gold);
        let saved=config.clone();assert!(apply_connection(&mut config,Some(&old),&connected).is_err());assert_eq!(config,saved);
        let old=config.accounts[0].clone();let mut other=payload();other["userInfo"]["userId"]=json!("other-user");let other=Connected{snapshot:parse(&other).unwrap(),..connected};
        assert!(apply_connection(&mut config,Some(&old),&other).is_err());assert_eq!(config,saved);
    }
    #[test]fn duplicate_and_unverified_accounts_do_not_bypass_identity_checks(){
        let snapshot=parse(&payload()).unwrap();let bound=account(Some(snapshot.kiro.as_ref().unwrap().owner.clone()));let mut config=Config::default();config.accounts.push(bound.clone());
        let connected=Connected{source:"copy.json".into(),label:"副本".into(),snapshot:snapshot.clone()};let saved=config.clone();assert!(apply_connection(&mut config,None,&connected).is_err());assert_eq!(config,saved);
        assert!(check_owner(&account(Some("legacy-client-hash".into())),&snapshot).is_err());assert!(check_owner(&account(None),&Snapshot::default()).is_err());
        let mut other=payload();other["userInfo"]["userId"]=json!("other-user");assert!(check_owner(&bound,&parse(&other).unwrap()).is_err());
        let cache=json!({&bound.id:{"provider":"Kiro","slot":bound.slot,"source":bound.source,"snapshot":snapshot}});assert!(bound.cached_snapshot(&cache).is_some());
        let mut moved=bound.clone();moved.source="different.json".into();assert!(moved.cached_snapshot(&cache).is_none());let mut old_cache=cache;old_cache[&bound.id]["snapshot"]["kiro"]=Value::Null;assert!(bound.cached_snapshot(&old_cache).is_none());
    }
    #[test]fn first_refresh_can_bind_the_same_user_while_a_connection_is_pending(){
        let pending=account(None);let mut config=Config::default();let snapshot=parse(&payload()).unwrap();let mut bound=pending.clone();bound.slot=Some(snapshot.kiro.as_ref().unwrap().owner.clone());config.accounts.push(bound);
        let connected=Connected{source:"new.json".into(),label:String::new(),snapshot};assert_eq!(apply_connection(&mut config,Some(&pending),&connected).unwrap(),pending.id);assert_eq!(config.accounts.len(),1);
    }
    #[test]fn login_changes_during_a_query_discard_the_result_without_rewriting_tokens(){
        let path=std::env::temp_dir().join(format!("daozai-kiro-read-only-{}.json",std::process::id()));let original=json!({"accessToken":"first"}).to_string();std::fs::write(&path,&original).unwrap();
        let s=query_with(&path,|_|Ok(parse(&payload()).unwrap())).unwrap();assert!(s.kiro.is_some());assert_eq!(std::fs::read_to_string(&path).unwrap(),original);
        let result=query_with(&path,|_|{std::fs::write(&path,json!({"accessToken":"second"}).to_string()).unwrap();Ok(parse(&payload()).unwrap())});assert!(result.unwrap_err().0.contains("已变化"));std::fs::remove_file(path).unwrap();
    }
    fn server(status:&str,headers:&str,body:Vec<u8>)->(String,std::thread::JoinHandle<String>){
        use std::io::{Read,Write};
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let url=format!("http://{}/",listener.local_addr().unwrap());let status=status.to_owned();let headers=headers.to_owned();
        let task=std::thread::spawn(move||{let(mut stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();let mut raw=Vec::new();let mut buf=[0;4096];
            loop{let n=stream.read(&mut buf).unwrap();if n==0{break;}raw.extend_from_slice(&buf[..n]);if let Some(end)=raw.windows(4).position(|b|b==b"\r\n\r\n"){
                let head=String::from_utf8_lossy(&raw[..end]);let size=head.lines().find_map(|s|s.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v|v.trim().parse::<usize>().ok())).unwrap_or(0);if raw.len()>=end+4+size{break;}
            }}
            let response=format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",body.len());stream.write_all(response.as_bytes()).unwrap();let _=stream.write_all(&body);String::from_utf8(raw).unwrap()
        });(url,task)
    }
    fn test_agent()->ureq::Agent{ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(3)).build()}
    #[test]fn usage_request_sends_the_expected_headers_and_empty_idc_body(){
        let(url,task)=server("200 OK","Content-Type: application/json\r\n",payload().to_string().into_bytes());let l=login(&json!({"accessToken":"synthetic"})).unwrap();
        assert!(request_at(&test_agent(),&l,&url).is_ok());let raw=task.join().unwrap().to_ascii_lowercase();assert!(raw.contains("authorization: bearer synthetic"));assert!(raw.contains("x-amz-target: amazoncodewhispererservice.getusagelimits"));assert!(raw.ends_with("{}"));
    }
    #[test]fn rate_limits_keep_retry_after_and_errors_never_echo_the_response(){
        let l=login(&json!({"accessToken":"synthetic"})).unwrap();let(url,task)=server("429 Too Many Requests","Retry-After: 900\r\n",b"private-response".to_vec());let error=request_at(&test_agent(),&l,&url).unwrap_err();task.join().unwrap();assert_eq!(error.1,900);assert!(!error.0.contains("private-response"));assert!(!error.0.contains("synthetic"));
        let(url,task)=server("401 Unauthorized","",b"private-response".to_vec());assert_eq!(request_at(&test_agent(),&l,&url).unwrap_err().1,0);task.join().unwrap();
    }
    #[test]fn redirects_and_responses_without_user_identity_are_rejected(){
        let l=login(&json!({"accessToken":"synthetic"})).unwrap();let(url,task)=server("302 Found","Location: http://127.0.0.1:1/\r\n",Vec::new());let e=request_at(&test_agent(),&l,&url).unwrap_err();assert!(e.0.contains("302"));task.join().unwrap();
        let mut v=payload();v.as_object_mut().unwrap().remove("userInfo");let(url,task)=server("200 OK","",v.to_string().into_bytes());assert!(request_at(&test_agent(),&l,&url).unwrap_err().0.contains("用户标识"));task.join().unwrap();
    }
    #[test]fn decompressed_responses_are_limited_to_one_mebibyte(){
        use std::io::Write;let l=login(&json!({"accessToken":"synthetic"})).unwrap();let mut body=payload().to_string().into_bytes();body.resize(1024*1024+1,b' ');
        for gzip in [false,true]{let headers=if gzip{"Content-Encoding: gzip\r\n"}else{""};let bytes=if gzip{let mut e=flate2::write::GzEncoder::new(Vec::new(),flate2::Compression::fast());e.write_all(&body).unwrap();e.finish().unwrap()}else{body.clone()};let(url,task)=server("200 OK",headers,bytes);assert!(request_at(&test_agent(),&l,&url).unwrap_err().0.contains("过大"));task.join().unwrap();}
    }
    #[test]
    #[ignore="按需只读核验本机 Kiro 额度，不保存配置"]
    fn kiro_local_probe(){
        let mut config=super::super::Config::default();
        let path=path().expect("需要 USERPROFILE");
        assert!(path.is_file(),"未找到 Kiro 登录文件，未完成真机核验");
        add(&mut config,&path,"").unwrap();
        let account=config.accounts.iter().find(|a|a.provider==super::super::Provider::Kiro).unwrap();
        let snapshot=fetch(&super::super::http_agent(),account).map_err(|e|e.0).expect("Kiro 真机额度查询失败");
        assert!(snapshot.kiro.is_some()&&!snapshot.windows.is_empty(),"未返回可核验的 Kiro 额度");
        println!("Kiro 账号与额度核验通过");
    }
}
