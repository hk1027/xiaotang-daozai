//! 兼容 CC Switch 的 request / extractor 查询脚本。
use super::{amount,window,read_response,Scope,Quota,Snapshot};
use serde_json::Value;
use std::time::{Duration,Instant};

pub const DEFAULT_SCRIPT:&str=r#"({
  request: {
    url: "{{baseUrl}}/user/balance",
    method: "GET",
    headers: { "Authorization": "Bearer {{apiKey}}" }
  },
  extractor: function(response) {
    return { remaining: response.balance, unit: "USD" };
  }
})"#;

#[derive(Clone,serde::Serialize,serde::Deserialize)]
pub struct Credential {pub script:String,pub api_key:String,pub access_token:String,pub user_id:String}

fn evaluate(code:&str,response:Option<&Value>,budget:Duration)->Result<Value,String>{
    let runtime=rquickjs::Runtime::new().map_err(|_|"无法启动查询脚本")?;
    runtime.set_memory_limit(16*1024*1024);runtime.set_max_stack_size(256*1024);
    let deadline=Instant::now()+budget;runtime.set_interrupt_handler(Some(Box::new(move||Instant::now()>=deadline)));
    let context=rquickjs::Context::full(&runtime).map_err(|_|"无法启动查询脚本")?;
    context.with(|ctx|{
        let result=(||->rquickjs::Result<String>{
            let config:rquickjs::Object=ctx.eval(code)?;
            let value:rquickjs::Value=if let Some(response)=response{
                let extractor:rquickjs::Function=config.get("extractor")?;
                extractor.call((ctx.json_parse(response.to_string())?,))?
            }else{config.get("request")?};
            ctx.json_stringify(value)?.ok_or(rquickjs::Error::Unknown)?.get()
        })().map_err(|_|"查询脚本执行失败、超时或超出内存限制".to_owned())?;
        serde_json::from_str(&result).map_err(|_|"查询脚本未返回有效 JSON".into())
    })
}

pub fn validate_unit(unit:&str)->Result<(),String>{
    if unit.is_empty()||unit.chars().count()>16||unit.chars().any(char::is_control){return Err("查询结果的单位无效".into());}Ok(())
}
pub fn parse_result(value:&Value,source:&str)->Result<Snapshot,String>{
    let rows:Vec<_>=match value.as_array(){Some(rows)if !rows.is_empty()&&rows.len()<=32=>rows.iter().collect(),Some(_)=>return Err("查询脚本返回的额度列表为空或过多".into()),None=>vec![value]};
    let mut amounts=Vec::new();let mut windows=Vec::new();
    for row in rows{
        if !row.is_object(){return Err("查询脚本应返回对象或对象数组".into());}
        if !row["isValid"].is_null()&&row["isValid"].as_bool()!=Some(true){return Err("查询脚本返回的账号状态无效，请检查凭据与接口".into());}
        for name in ["remaining","total","used"]{if !row[name].is_null()&&super::numeric(&row[name]).is_none(){return Err("查询脚本返回的额度字段无效".into());}}
        for name in ["unit","planName","extra","invalidMessage"]{if !row[name].is_null()&&!row[name].is_string(){return Err("查询脚本返回的说明字段无效".into());}}
        let unit=row["unit"].as_str().unwrap_or("站点单位");validate_unit(unit)?;
        let label=row["planName"].as_str().filter(|s|!s.is_empty()).unwrap_or("剩余额度");
        if label.chars().count()>128||label.chars().any(char::is_control){return Err("查询脚本返回的套餐名称无效".into());}
        let total=super::numeric(&row["total"]);let used=super::numeric(&row["used"]);
        let remaining=super::numeric(&row["remaining"]).or_else(||total.zip(used).map(|(t,u)|t-u)).filter(|n|n.is_finite());
        if let Some(w)=total.zip(remaining).and_then(|(t,r)|window(label,t,r,None,None)){windows.push(w);}
        amounts.push(amount(label,unit,remaining,total,used,false,Scope::Reported));
    }
    Ok(Snapshot{plan:source.into(),relay:Some(Quota{source:source.into(),amounts,expires_at:None}),windows,at:chrono::Utc::now().timestamp(),..Default::default()})
}

pub fn request(agent:&ureq::Agent,base:&str,credential:&Credential,cancelled:impl Fn()->bool)->Result<Snapshot,(String,u64)>{
    if cancelled(){return Err(("已取消连接".into(),0));}
    let base=super::validate_base_url(base).map_err(|e|(e,0))?;
    if credential.script.trim().is_empty()||credential.script.len()>65536{return Err(("请输入 64 KB 以内的查询脚本".into(),0));}
    let mut code=credential.script.clone();
    for (key,value) in [("baseUrl",base.as_str()),("apiKey",credential.api_key.as_str()),("accessToken",credential.access_token.as_str()),("userId",credential.user_id.as_str())]{code=code.replace(&format!("{{{{{key}}}}}"),value);}
    let config=evaluate(&code,None,Duration::from_secs(5)).map_err(|e|(e,0))?;
    let url=url::Url::parse(config["url"].as_str().ok_or(("查询脚本缺少请求地址".into(),0))?).map_err(|_|("查询脚本的请求地址无效".into(),0))?;
    let origin=url::Url::parse(&base).unwrap();
    if url.scheme()!="https"||url.origin()!=origin.origin()||!url.username().is_empty()||url.password().is_some()||url.fragment().is_some(){return Err(("查询脚本必须请求所填站点的 HTTPS 接口".into(),0));}
    let method=config["method"].as_str().unwrap_or("GET");
    if !matches!(method,"GET"|"POST"){return Err(("额度查询脚本仅支持 GET 或 POST".into(),0));}
    let mut request=agent.request(method,url.as_str()).set("Accept","application/json").timeout(Duration::from_secs(15));
    if !config["headers"].is_null(){
        let headers=config["headers"].as_object().ok_or(("查询脚本的 headers 应为对象".into(),0))?;
        for (name,value) in headers{
            let value=value.as_str().ok_or(("查询脚本的请求头应为字符串".into(),0))?;
            if name.is_empty()||!name.bytes().all(|b|b.is_ascii_alphanumeric()||b"!#$%&'*+-.^_`|~".contains(&b))||value.chars().any(char::is_control)||matches!(name.to_ascii_lowercase().as_str(),"host"|"content-length"|"transfer-encoding"|"connection"){return Err(("查询脚本含无效请求头".into(),0));}
            request=request.set(name,value);
        }
    }
    if cancelled(){return Err(("已取消连接".into(),0));}
    let response=if method=="POST"{
        let body=config["body"].as_str().ok_or(("POST 查询脚本需提供字符串 body".into(),0))?;
        super::read_response_result(request.send_string(body))
    }else{read_response(request)}.map_err(|e|e.detail())?;
    if cancelled(){return Err(("已取消连接".into(),0));}
    let value=evaluate(&code,Some(&response),Duration::from_secs(5)).map_err(|e|(e,60))?;
    if cancelled(){return Err(("已取消连接".into(),0));}
    parse_result(&value,"自定义查询").map_err(|e|(e,60))
}

#[cfg(test)]
mod tests{
    use super::*;use serde_json::json;
    #[test]fn cc_switch_scripts_reuse_placeholders_extractors_and_multiple_plans(){
        let credential=Credential{script:DEFAULT_SCRIPT.replace("return { remaining: response.balance, unit: \"USD\" };","return [{planName:'套餐一',remaining:response.balance,unit:'USD'},{planName:'套餐二',used:2,total:10,unit:'USD'}];"),api_key:"synthetic-key".into(),access_token:String::new(),user_id:String::new()};
        let agent=ureq::AgentBuilder::new().middleware(|r:ureq::Request,_:ureq::MiddlewareNext|{
            assert_eq!(r.url(),"https://relay.example/tenant/user/balance");assert_eq!(r.header("Authorization"),Some("Bearer synthetic-key"));ureq::Response::new(200,"OK",r#"{"balance":1.1}"#)
        }).build();
        let snapshot=request(&agent,"https://relay.example/tenant",&credential,||false).unwrap();let q=snapshot.relay.unwrap();
        assert_eq!(q.compact(),"1.1 USD");assert_eq!(q.amounts.len(),2);assert_eq!(q.amounts[1].remaining,Some(8.0));assert!(q.full().contains("套餐二"));
    }
    #[test]fn invalid_script_and_infinite_loop_do_not_expose_credentials(){
        for code in ["throw new Error('synthetic-secret')","for(;;){}"]{
            let error=evaluate(code,None,Duration::from_millis(20)).unwrap_err();assert!(!error.contains("synthetic-secret"));
        }
        for value in [json!([]),json!(1),json!({"remaining":"0"}),json!({"isValid":false,"invalidMessage":"synthetic-secret"})]{assert!(parse_result(&value,"测试").is_err());}
    }
    #[test]fn cc_switch_new_api_script_accepts_access_token_and_user_id_variables(){
        let c=Credential{script:r#"({request:{url:"{{baseUrl}}/api/user/self",method:"GET",headers:{Authorization:"Bearer {{accessToken}}","New-Api-User":"{{userId}}"}},extractor:function(response){return {remaining:response.data.quota/500000,used:response.data.used_quota/500000,unit:"USD"};}})"#.into(),api_key:String::new(),access_token:"synthetic-access-token".into(),user_id:"42".into()};
        let agent=ureq::AgentBuilder::new().middleware(|r:ureq::Request,_:ureq::MiddlewareNext|{
            assert_eq!(r.header("Authorization"),Some("Bearer synthetic-access-token"));assert_eq!(r.header("New-Api-User"),Some("42"));ureq::Response::new(200,"OK",r#"{"data":{"quota":550000,"used_quota":1000000}}"#)
        }).build();assert_eq!(request(&agent,"https://relay.example",&c,||false).unwrap().relay.unwrap().compact(),"1.1 USD");
    }
    #[test]fn custom_post_queries_and_cancellation_are_supported(){
        let c=Credential{script:DEFAULT_SCRIPT.replace("method: \"GET\"","method: \"POST\", body: JSON.stringify({account:42})"),api_key:"synthetic-key".into(),access_token:String::new(),user_id:String::new()};
        let agent=ureq::AgentBuilder::new().middleware(|r:ureq::Request,_:ureq::MiddlewareNext|{assert_eq!(r.method(),"POST");ureq::Response::new(200,"OK",r#"{"balance":0}"#)}).build();
        assert_eq!(request(&agent,"https://relay.example",&c,||false).unwrap().relay.unwrap().compact(),"0 USD");
        assert_eq!(request(&agent,"https://relay.example",&c,||true).unwrap_err().0,"已取消连接");
    }
    #[test]fn custom_scripts_cannot_send_credentials_to_another_origin(){
        let agent=ureq::AgentBuilder::new().middleware(|_:ureq::Request,_:ureq::MiddlewareNext|panic!("不应发送请求")).build();
        for url in ["https://other.example/user/balance","http://relay.example/user/balance","https://user:pass@relay.example/user/balance"]{
            let c=Credential{script:DEFAULT_SCRIPT.replace("{{baseUrl}}/user/balance",url),api_key:"synthetic".into(),access_token:String::new(),user_id:String::new()};assert!(request(&agent,"https://relay.example",&c,||false).is_err());
        }
    }
}
