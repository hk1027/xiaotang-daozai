use super::{Account, Snapshot, Window};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::{aead,digest};
use serde_json::Value;
use std::{io::Read,path::{Path,PathBuf}};

pub(crate) fn path()->Option<PathBuf>{
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(".zcode/v2/credentials.json"))
}
const CREDENTIAL_ERROR:&str="无法读取当前 Windows 用户的 ZCode 登录，请在客户端重新登录";
fn hash(bytes:&[u8])->String{digest::digest(&digest::SHA256,bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect()}
#[cfg(windows)]
fn system_key(path:&Path)->Result<Vec<u8>,String>{
    // ZCode 按绝对路径的 SHA-256 定位系统密钥，不解析符号链接或改变路径大小写。
    let path=std::path::absolute(path).map_err(|_|CREDENTIAL_ERROR)?;
    let path=path.to_str().ok_or(CREDENTIAL_ERROR)?.replace('/',"\\");
    let path=if let Some(unc)=path.strip_prefix("\\\\?\\UNC\\"){format!("\\\\{unc}")}else{path.strip_prefix("\\\\?\\").unwrap_or(&path).to_owned()};
    let encoded=keyring::Entry::new("ZCode Credential Encryption",&hash(path.as_bytes())).and_then(|entry|entry.get_password())
        .map_err(|_|"无法读取 ZCode 的安全存储密钥，请用当前 Windows 用户打开 ZCode 后重试")?;
    if encoded.len()!=43{return Err(CREDENTIAL_ERROR.into());}
    URL_SAFE_NO_PAD.decode(encoded).ok().filter(|key|key.len()==32).ok_or_else(||CREDENTIAL_ERROR.into())
}
#[cfg(not(windows))]
fn system_key(_path:&Path)->Result<Vec<u8>,String>{Err("当前系统不支持读取 ZCode 的 Windows 安全存储密钥".into())}
fn v2_key(path:&Path,source:&str,secret:Option<String>,secret_file:Option<PathBuf>)->Result<Vec<u8>,String>{
    if secret.is_some()&&secret_file.is_some(){return Err("ZCode 的两种凭据密钥配置不能同时使用".into());}
    if source=="os"{
        if secret.is_some()||secret_file.is_some(){return Err("ZCode 凭据的密钥来源与当前环境不一致".into());}
        return system_key(path);
    }
    if let Some(secret)=secret{return Ok(digest::digest(&digest::SHA256,secret.as_bytes()).as_ref().to_vec());}
    let file=secret_file.unwrap_or_else(||path.with_file_name("credential-secret"));
    let mut bytes=Vec::new();std::fs::File::open(file).and_then(|f|f.take(4097).read_to_end(&mut bytes)).map_err(|_|"无法读取 ZCode 的凭据密钥文件")?;
    if !(32..=4096).contains(&bytes.len())||std::str::from_utf8(&bytes).is_err(){return Err("ZCode 的凭据密钥文件无效".into());}
    Ok(digest::digest(&digest::SHA256,&bytes).as_ref().to_vec())
}
fn value(root:&Value,name:&str,path:&Path)->Result<String,String>{
    let raw=root[name].as_str().ok_or("请先在 ZCode 中登录个人账号")?;
    let error=CREDENTIAL_ERROR;
    let (encoded,key)=if let Some(encoded)=raw.strip_prefix("enc:v2:"){
        let parts=encoded.split(':').collect::<Vec<_>>();
        let [source,fingerprint,payload]=parts.as_slice() else{return Err(error.into());};
        if !matches!(*source,"os"|"env")||fingerprint.len()!=24||!fingerprint.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)){return Err(error.into());}
        let key=v2_key(path,source,std::env::var("ZCODE_CREDENTIAL_SECRET").ok().filter(|s|!s.is_empty()),
            std::env::var_os("ZCODE_CREDENTIAL_SECRET_FILE").filter(|s|!s.is_empty()).map(PathBuf::from))?;
        if &hash(&key)[..24]!=*fingerprint{return Err("ZCode 凭据与当前安全存储密钥不匹配，请在客户端确认登录".into());}
        (*payload,key)
    }else if let Some(encoded)=raw.strip_prefix("enc:v1:"){
        let secret=std::env::var("ZCODE_CREDENTIAL_SECRET").ok().filter(|s|!s.trim().is_empty()).unwrap_or_else(||format!(
            "zcode-credential-fallback:win32:{}:{}",std::env::var("USERPROFILE").unwrap_or_default(),std::env::var("USERNAME").unwrap_or_default()));
        (encoded,digest::digest(&digest::SHA256,secret.trim().as_bytes()).as_ref().to_vec())
    }else if raw.starts_with("enc:"){return Err("ZCode 登录凭据格式不受支持，请更新岛仔".into());}else{return Ok(raw.into());};
    let parts=encoded.split('.').map(|s|URL_SAFE_NO_PAD.decode(s)).collect::<Result<Vec<_>,_>>().map_err(|_|error)?;
    let [iv,tag,cipher]=parts.as_slice() else{return Err(error.into());};
    if tag.len()!=16{return Err(error.into());}
    let mut bytes=cipher.clone();bytes.extend_from_slice(tag);
    let key=aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM,key.as_ref()).map_err(|_|error)?);
    let plain=key.open_in_place(aead::Nonce::try_assume_unique_for_key(iv).map_err(|_|error)?,aead::Aad::empty(),&mut bytes).map_err(|_|error)?;
    String::from_utf8(plain.to_vec()).map_err(|_|error.into())
}
struct Login { family:String, slot:String, coding_subject:Option<String>, jwt:String }
fn login(root:&Value,path:&Path)->Result<Login,String>{
    let provider=value(root,"oauth:active_provider",path)?;
    let family=match provider.as_str(){"zai"=>"zai","bigmodel"|"zai_cn"=>"bigmodel",_=>return Err("暂不支持这个 ZCode 登录区域".into())};
    let jwt=value(root,"zcodejwttoken",path)?;
    let payload=jwt.split('.').nth(1).and_then(|s|URL_SAFE_NO_PAD.decode(s).ok()).and_then(|s|serde_json::from_slice::<Value>(&s).ok()).ok_or("ZCode 账号标识无效")?;
    let id=payload["sub"].as_str().or_else(||payload["user_id"].as_str()).ok_or("ZCode 未返回账号标识")?;
    let slot=hash(format!("{family}/{id}").as_bytes());
    // 国内新登录的套餐身份来自显式关联，ZCode JWT 的主体只用于识别本机账号。
    let coding_subject=if provider=="zai_cn"{
        if root.get("oauth:linked_bigmodel").is_some(){
            let linked:Value=serde_json::from_str(&value(root,"oauth:linked_bigmodel",path)?).map_err(|_|"ZCode 关联的 BigModel 账号信息无效")?;
            Some(linked["customerId"].as_str().filter(|s|!s.trim().is_empty()).ok_or("ZCode 关联的 BigModel 账号标识无效")?.to_owned())
        }else{None}
    }else{Some(id.to_owned())};
    Ok(Login{family:family.into(),slot,coding_subject,jwt})
}
pub(crate) fn add(config:&mut super::Config,path:&Path,alias:&str)->Result<(),String>{
    let l=login(&super::login_json(path)?,path)?;
    if config.accounts.iter().any(|a|a.provider==super::Provider::ZCode&&a.slot.as_ref()==Some(&l.slot)){return Ok(());}
    config.accounts.push(Account{id:format!("ZCode-{}",l.slot),provider:super::Provider::ZCode,alias:if alias.trim().is_empty(){"智谱 ZCode".into()}else{alias.trim().into()},source:path.to_owned(),slot:Some(l.slot),enabled:true,pinned:false});Ok(())
}
fn read(agent:&ureq::Agent,url:&str,auth:&str,device_mid:Option<&str>)->Result<Value,String>{
    let mut request=agent.get(url).set("Authorization",auth).set("Accept","application/json");
    if let Some(mid)=device_mid{request=request.set("X-Device-Mid",mid);}
    let response=request.call().map_err(|e|match e{
        ureq::Error::Status(401|403,_)=>"ZCode 授权被拒绝，请在客户端确认登录".into(),
        ureq::Error::Status(code,_)=>format!("ZCode 用量服务返回 {code}"),
        _=>"无法连接 ZCode 用量服务".into(),
    })?;
    serde_json::from_reader(response.into_reader().take(1024*1024)).map_err(|_|"ZCode 用量响应无效".into())
}
fn success(v:&Value)->bool{ matches!(v["code"].as_i64(),Some(0|200)) || (v["code"].is_null()&&v["success"]==true) }
pub(super) fn coding(v:&Value)->Result<Snapshot,String>{
    let mut snapshot=Snapshot{plan:"Coding Plan".into(),at:chrono::Utc::now().timestamp(),..Default::default()};
    for row in v["data"]["limits"].as_array().into_iter().flatten(){
        let Some(used)=super::number(&row["percentage"]).filter(|v|(0.0..=100.0).contains(v)) else{continue;};
        let multiplier=match row["unit"].as_i64(){Some(1)=>Some(86400),Some(3)=>Some(3600),Some(5)=>Some(60),Some(6)=>Some(604800),_=>None};
        let seconds=row["number"].as_i64().filter(|n|*n>0).zip(multiplier).and_then(|(n,m)|n.checked_mul(m));
        let kind=row["type"].as_str().unwrap_or_default();
        let label=if kind=="TIME_LIMIT"{"工具调用".into()}else{super::cadence(seconds)};
        let reset=row["nextResetTime"].as_i64().filter(|x|*x>0).map(|x|if x>10_000_000_000{x/1000}else{x});
        let window=Window{label,seconds,remaining:(100.0-used) as f32,reset,amount:None};
        match kind{"TOKENS_LIMIT"|"CREDIT_LIMIT"=>snapshot.windows.push(window),"TIME_LIMIT"=>snapshot.extra.push(window),_=>{}}
    }
    if snapshot.windows.is_empty(){std::mem::swap(&mut snapshot.windows,&mut snapshot.extra);}
    if snapshot.windows.is_empty(){return Err("ZCode 未提供可显示的套餐比例".into());}
    Ok(snapshot)
}
fn start(v:&Value)->Result<Snapshot,String>{
    let mut snapshot=Snapshot{plan:"Start Plan".into(),at:chrono::Utc::now().timestamp(),..Default::default()};
    let buckets=v["data"]["balances"].as_array().ok_or("ZCode 响应缺少体验额度明细")?;
    if buckets.is_empty(){snapshot.plan="暂无体验额度".into();return Ok(snapshot);}
    for bucket in buckets{
        let Some(total)=super::number(&bucket["total_units"]).filter(|n|*n>0.0) else{continue;};
        let Some(left)=super::number(&bucket["remaining_units"]).filter(|n|*n>=0.0) else{continue;};
        let reset=super::number(&bucket["expires_at"]).map(|n|n as i64);
        if reset.is_some_and(|t|t<=snapshot.at){continue;}
        snapshot.windows.push(Window{label:bucket["show_name"].as_str().filter(|s|!s.is_empty()).unwrap_or("体验额度").into(),seconds:None,remaining:(left/total*100.0).clamp(0.0,100.0) as f32,reset,amount:None});
    }
    if snapshot.windows.is_empty(){return Err("当前 ZCode 账号暂无有效体验额度".into());}
    Ok(snapshot)
}
pub(super) fn fetch(agent:&ureq::Agent,a:&Account)->Result<Snapshot,String>{
    let root=super::login_json(&a.source)?;let l=login(&root,&a.source)?;
    if a.slot.as_ref()!=Some(&l.slot){return Err("ZCode 已切换账号，请重新识别本机账号".into());}
    let key=l.coding_subject.as_ref().map(|id|format!("account-provider:coding-plan:account:{}-individual-coding-plan:account:{}:api-key",l.family,id));
    let mut no_coding_plan=false;
    if let Some(key)=key.filter(|key|root.get(key).is_some()){
        let url=if l.family=="zai"{"https://api.z.ai/api/monitor/usage/quota/limit"}else{"https://bigmodel.cn/api/monitor/usage/quota/limit"};
        let v=read(agent,url,&value(&root,&key,&a.source)?,None)?;
        if success(&v){return coding(&v);}
        let msg=v["msg"].as_str().or_else(||v["message"].as_str()).unwrap_or("").to_lowercase();
        if !(msg.contains("不存在coding plan")||msg.contains("不存在 coding plan")||msg.contains("no coding plan")){return Err(format!("ZCode 编程套餐查询失败（{}）",v["code"]));}
        no_coding_plan=true;
    }
    let mut url=url::Url::parse("https://zcode.z.ai/api/v1/zcode-plan/billing/balance").unwrap();
    // 客户端按版本发布运行时，官网使用 app_version 判断体验套餐能力。
    if let Some(runtime)=a.source.parent().map(|p|p.join("runtime/provider/windows-x86_64")){
        let versions=std::fs::read_dir(runtime).into_iter().flatten().flatten().filter_map(|dir|{
            let name=dir.file_name().into_string().ok()?;
            let parts=name.split('.').map(str::parse::<u32>).collect::<Result<Vec<_>,_>>().ok()?;
            (parts.len()==3).then_some((parts,name))
        });
        if let Some((_,v))=versions.max(){url.query_pairs_mut().append_pair("app_version",&v);}
    }
    let telemetry=a.source.with_file_name("telemetry-state.json");
    let device=super::login_json(&telemetry).ok();
    let mid=device.as_ref().and_then(|v|v["deviceMid"].as_str()).filter(|s|!s.is_empty() && s.bytes().all(|b|b.is_ascii_graphic())).ok_or("请打开 ZCode，等待客户端初始化设备信息")?;
    let v=read(agent,url.as_str(),&format!("Bearer {}",l.jwt),Some(mid)).map_err(|e|if no_coding_plan{format!("未开通 Coding Plan；体验套餐查询失败：{e}")}else{e})?;
    if !success(&v){return Err(format!("ZCode 体验额度暂不可用（{}）",v["code"]));}
    start(&v)
}
#[cfg(test)]
mod tests{
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture{
        fn new(root:&Value)->Self{
            let dir=std::env::temp_dir().join(format!("daozai-zcode-{}-{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap()));
            std::fs::create_dir(&dir).unwrap();let fixture=Self(dir.join("credentials.json"));
            std::fs::write(&fixture.0,root.to_string()).unwrap();std::fs::write(fixture.0.with_file_name("telemetry-state.json"),r#"{"deviceMid":"synthetic-device"}"#).unwrap();fixture
        }
    }
    impl Drop for Fixture{fn drop(&mut self){for name in ["credentials.json","telemetry-state.json","credential-secret"]{let _=std::fs::remove_file(self.0.with_file_name(name));}let _=std::fs::remove_dir(self.0.parent().unwrap());}}
    fn token(subject:&str)->String{format!("e30.{}.synthetic",URL_SAFE_NO_PAD.encode(serde_json::json!({"sub":subject}).to_string()))}
    fn sealed(value:&str,key:&[u8],prefix:&str)->String{
        let iv=[7;12];let mut bytes=value.as_bytes().to_vec();
        let cipher=aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM,key).unwrap());
        let tag=cipher.seal_in_place_separate_tag(aead::Nonce::assume_unique_for_key(iv),aead::Aad::empty(),&mut bytes).unwrap();
        format!("{prefix}{}.{}.{}",URL_SAFE_NO_PAD.encode(iv),URL_SAFE_NO_PAD.encode(tag),URL_SAFE_NO_PAD.encode(bytes))
    }
    fn system_sealed(value:&str,key:&[u8])->String{
        let fingerprint:String=digest::digest(&digest::SHA256,key).as_ref()[..12].iter().map(|b|format!("{b:02x}")).collect();
        sealed(value,key,&format!("enc:v2:os:{fingerprint}:"))
    }
    #[test]fn china_login_is_accepted_without_merging_overseas_accounts(){
        let jwt=token("synthetic-user");
        let path=Path::new("credentials.json");
        let china=login(&serde_json::json!({"oauth:active_provider":"zai_cn","zcodejwttoken":jwt}),path).unwrap();
        let legacy=login(&serde_json::json!({"oauth:active_provider":"bigmodel","zcodejwttoken":jwt}),path).unwrap();
        let global=login(&serde_json::json!({"oauth:active_provider":"zai","zcodejwttoken":jwt}),path).unwrap();
        assert_eq!(china.family,"bigmodel");assert_eq!(china.slot,legacy.slot);assert_ne!(china.slot,global.slot);
    }
    #[test]fn unknown_encryption_is_never_treated_as_plaintext(){
        for raw in ["enc:v9:synthetic-private-value","enc:v2:os:bad-fingerprint:synthetic-private-value","enc:v2:unknown:0123456789abcdef01234567:synthetic-private-value","enc:v1:invalid"]{
            let error=value(&serde_json::json!({"oauth:active_provider":raw}),"oauth:active_provider",Path::new("credentials.json")).unwrap_err();assert!(!error.contains("synthetic-private-value"));
        }
    }
    #[test]fn managed_v2_keys_preserve_secret_bytes_and_reject_source_conflicts(){
        let fixture=Fixture::new(&serde_json::json!({}));let secret="  synthetic-zcode-secret-with-newline\n";
        let key=digest::digest(&digest::SHA256,secret.as_bytes()).as_ref().to_vec();
        std::fs::write(fixture.0.with_file_name("credential-secret"),secret).unwrap();
        assert_eq!(v2_key(&fixture.0,"env",Some(secret.into()),None).unwrap(),key);
        assert_eq!(v2_key(&fixture.0,"env",None,None).unwrap(),key);
        assert_eq!(v2_key(&fixture.0,"env",None,Some(fixture.0.with_file_name("credential-secret"))).unwrap(),key);
        assert!(v2_key(&fixture.0,"env",Some(secret.into()),Some("unused-secret-file".into())).is_err());
        assert!(v2_key(&fixture.0,"os",Some(secret.into()),None).is_err());
        for invalid in [vec![b'x';31],vec![b'x';4097],vec![255;32]]{std::fs::write(fixture.0.with_file_name("credential-secret"),invalid).unwrap();assert!(v2_key(&fixture.0,"env",None,None).is_err());}
    }
    #[test]fn china_and_legacy_logins_query_only_their_own_entitlements(){
        for (provider,linked,encrypted) in [("zai_cn",None,false),("zai_cn",Some("linked-customer"),false),("bigmodel",None,false),("zai",None,false),("zai",None,true)]{
            let family=if provider=="zai"{"zai"}else{"bigmodel"};let jwt=token("synthetic-user");
            let mut root=serde_json::json!({"oauth:active_provider":provider,"zcodejwttoken":jwt,
                "account-provider:coding-plan:account:bigmodel-individual-coding-plan:account:synthetic-user:api-key":"unrelated-stale-key"});
            if let Some(customer)=linked{root["oauth:linked_bigmodel"]=serde_json::json!(serde_json::json!({"customerId":customer,"accessToken":"synthetic-linked-token"}).to_string());}
            let coding=provider!="zai_cn"||linked.is_some();
            if coding{root[format!("account-provider:coding-plan:account:{family}-individual-coding-plan:account:{}:api-key",linked.unwrap_or("synthetic-user"))]=serde_json::json!("synthetic-coding-key");}
            if encrypted{
                let secret=std::env::var("ZCODE_CREDENTIAL_SECRET").ok().filter(|s|!s.trim().is_empty()).unwrap_or_else(||format!("zcode-credential-fallback:win32:{}:{}",std::env::var("USERPROFILE").unwrap_or_default(),std::env::var("USERNAME").unwrap_or_default()));
                let key=digest::digest(&digest::SHA256,secret.trim().as_bytes());for value in root.as_object_mut().unwrap().values_mut(){*value=serde_json::json!(sealed(value.as_str().unwrap(),key.as_ref(),"enc:v1:"));}
            }
            let fixture=Fixture::new(&root);let raw=std::fs::read(&fixture.0).unwrap();let mut config=super::super::Config::default();add(&mut config,&fixture.0,"").unwrap();
            let count=std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));let calls=count.clone();
            let agent=ureq::AgentBuilder::new().try_proxy_from_env(false).middleware(move|request:ureq::Request,_:ureq::MiddlewareNext|{
                calls.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
                assert_eq!(request.url(),if !coding{"https://zcode.z.ai/api/v1/zcode-plan/billing/balance"}else if family=="zai"{"https://api.z.ai/api/monitor/usage/quota/limit"}else{"https://bigmodel.cn/api/monitor/usage/quota/limit"});
                assert_eq!(request.header("Authorization"),Some(if coding{"synthetic-coding-key".into()}else{format!("Bearer {jwt}")}).as_deref());
                assert_eq!(request.header("X-Device-Mid"),if coding{None}else{Some("synthetic-device")});
                let body=if coding{r#"{"code":200,"data":{"limits":[{"type":"TOKENS_LIMIT","percentage":25,"unit":3,"number":5}]}}"#}else{r#"{"code":0,"data":{"balances":[{"show_name":"GLM-5.3-Flash","total_units":100,"remaining_units":75}]}}"#};
                ureq::Response::new(200,"Test",body)
            }).build();
            assert_eq!(fetch(&agent,&config.accounts[0]).unwrap().windows[0].remaining,75.0);assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed),1);assert_eq!(std::fs::read(&fixture.0).unwrap(),raw);
            let mut wrong=config.accounts[0].clone();wrong.slot=Some("another-account".into());assert!(fetch(&agent,&wrong).unwrap_err().contains("切换账号"));assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed),1);
        }
    }
    #[cfg(windows)]
    #[test]fn system_credentials_are_read_without_changing_other_accounts_or_the_store(){
        let dir=std::env::temp_dir().join(format!("daozai-zcode-v2-{}-{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap()));
        std::fs::create_dir(&dir).unwrap();let path=dir.join("credentials.json");
        let identity:String=digest::digest(&digest::SHA256,path.to_str().unwrap().as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect();
        let entry=keyring::Entry::new("ZCode Credential Encryption",&identity).unwrap();
        struct Cleanup(PathBuf,keyring::Entry);impl Drop for Cleanup{fn drop(&mut self){let _=self.1.delete_credential();let _=std::fs::remove_file(self.0.join("credentials.json"));let _=std::fs::remove_dir(&self.0);}}
        let cleanup=Cleanup(dir,entry);let key=[42;32];let encoded=URL_SAFE_NO_PAD.encode(key);cleanup.1.set_password(&encoded).unwrap();
        let raw=serde_json::json!({"oauth:active_provider":system_sealed("zai_cn",&key),"zcodejwttoken":system_sealed(&token("synthetic-user"),&key),
            "oauth:linked_bigmodel":system_sealed(r#"{"customerId":"synthetic-customer"}"#,&key),
            "account-provider:coding-plan:account:bigmodel-individual-coding-plan:account:synthetic-customer:api-key":system_sealed("synthetic-coding-key",&key)}).to_string();
        std::fs::write(&path,&raw).unwrap();
        let mut config=super::super::Config::default();
        for provider in [super::super::Provider::Glm,super::super::Provider::Codex]{config.accounts.push(Account{id:provider.name().into(),provider,alias:"保留备注".into(),source:"synthetic-source".into(),slot:Some("synthetic-slot".into()),enabled:false,pinned:true});}
        config.selected="GLM".into();let before=config.clone();
        add(&mut config,&path,"").unwrap();add(&mut config,&path,"").unwrap();
        assert_eq!(&config.accounts[..2],before.accounts.as_slice());assert_eq!(config.selected,before.selected);assert_eq!(config.accounts.len(),3);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),raw);assert_eq!(cleanup.1.get_password().unwrap(),encoded);
        assert_eq!(system_key(Path::new(&path.to_string_lossy().replace('\\',"/"))).unwrap(),key);
        // 只添加 Windows 扩展路径前缀，保留 ZCode 计算凭据标识时使用的短目录名。
        let spelling=path.to_str().unwrap();
        let extended=if let Some(unc)=spelling.strip_prefix(r"\\"){format!(r"\\?\UNC\{unc}")}else{format!(r"\\?\{spelling}")};
        assert_eq!(system_key(Path::new(&extended)).unwrap(),key);
        let agent=ureq::AgentBuilder::new().try_proxy_from_env(false).middleware(|request:ureq::Request,_:ureq::MiddlewareNext|{
            assert_eq!(request.url(),"https://bigmodel.cn/api/monitor/usage/quota/limit");assert_eq!(request.header("Authorization"),Some("synthetic-coding-key"));
            ureq::Response::new(200,"Test",r#"{"code":200,"data":{"limits":[{"type":"TOKENS_LIMIT","percentage":40}]}}"#)
        }).build();assert_eq!(fetch(&agent,&config.accounts[2]).unwrap().windows[0].remaining,60.0);
        let mut corrupt:Value=serde_json::from_str(&raw).unwrap();corrupt["oauth:active_provider"]=serde_json::json!(system_sealed("zai_cn",&[43;32]));std::fs::write(&path,corrupt.to_string()).unwrap();
        let before=config.clone();assert!(add(&mut config,&path,"").is_err());assert_eq!(config,before);
        cleanup.1.delete_credential().unwrap();std::fs::write(&path,&raw).unwrap();assert!(add(&mut config,&path,"").is_err());assert_eq!(config,before);assert!(matches!(cleanup.1.get_password(),Err(keyring::Error::NoEntry)));
    }
    #[test]fn empty_entitlements_are_valid_but_missing_data_is_an_error(){
        assert!(start(&serde_json::json!({"data":{"balances":[]}})).unwrap().windows.is_empty());
        assert!(start(&serde_json::json!({"data":{}})).is_err());
    }
    #[test]fn zcode_distinguishes_used_percentage_and_remaining_units(){
        let c=coding(&serde_json::json!({"data":{"limits":[{"type":"TOKENS_LIMIT","percentage":31,"unit":3,"number":5,"nextResetTime":1900000000000i64}]}})).unwrap();
        assert_eq!(c.windows[0].remaining,69.0);assert_eq!(c.windows[0].reset,Some(1900000000));
        let s=start(&serde_json::json!({"data":{"balances":[{"total_units":"800","remaining_units":"200","available_units":"100"}]}})).unwrap();assert_eq!(s.windows[0].remaining,25.0);
    }
}
