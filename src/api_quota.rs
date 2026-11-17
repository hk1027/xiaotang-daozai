//! API-key setup. Config/cache contain only provider, endpoint/region and an opaque vault reference.
use super::{plans::{self,PlanProvider},relay::{self,Kind},Account,Config,Provider,Snapshot};
use std::{path::PathBuf,sync::mpsc};

#[derive(serde::Serialize,serde::Deserialize)]
struct NewApiUserToken {access_token:String,user_id:String}
pub fn new_api_user_id(account:&Account)->Option<String>{
    if account.provider!=Provider::NewApi{return None;}
    let stored=load(account.slot.as_deref()?).ok()?;
    serde_json::from_str::<NewApiUserToken>(&stored).ok().map(|c|c.user_id)
}
pub fn custom_credentials(account:&Account)->Option<relay::script::Credential>{
    if account.provider!=Provider::RelayCustom{return None;}
    serde_json::from_str(&load(account.slot.as_deref()?).ok()?).ok()
}
fn script_path(slot:&str)->Result<PathBuf,String>{
    let id=slot.strip_prefix("script-").filter(|s|s.len()==48&&s.bytes().all(|b|b.is_ascii_hexdigit())).ok_or("查询脚本凭据引用无效")?;
    Ok(crate::model::config_dir().join("credentials").join(format!("relay-script-{id}.bin")))
}

fn endpoint(provider:Provider,source:&str)->Result<String,String>{
    if provider.relay(){return relay::validate_base_url(source);}
    match provider {
        Provider::OpenCodeGo=>Ok(String::new()),
        Provider::CommandCode=>Ok(String::new()),
        Provider::MiniMax|Provider::Glm=>match source {""|"cn"=>Ok("cn".into()),"global"=>Ok("global".into()),_=>Err("请选择中国区或国际区，密钥不会自动跨区重试".into())},
        _=>Err("该服务不使用此连接方式".into())
    }
}
fn request(agent:&ureq::Agent,provider:Provider,source:&str,key:&str,cancel:Option<&std::sync::atomic::AtomicBool>)->Result<Snapshot,(String,u64)>{
    if cancel.is_some_and(|c|c.load(std::sync::atomic::Ordering::Acquire)){return Err(("已取消连接".into(),0));}
    let source=endpoint(provider,source).map_err(|e|(e,0))?;
    match provider {
        Provider::OpenCodeGo=>plans::request(agent,PlanProvider::OpenCodeGo,key),
        Provider::CommandCode=>plans::request(agent,PlanProvider::CommandCode,key),
        Provider::MiniMax=>plans::request(agent,if source=="global"{PlanProvider::MiniMaxGlobal}else{PlanProvider::MiniMaxChina},key),
        Provider::Glm=>plans::request(agent,if source=="global"{PlanProvider::GlmGlobal}else{PlanProvider::GlmChina},key),
        Provider::RelayCustom=>{
            let c:relay::script::Credential=serde_json::from_str(key).map_err(|_|("查询脚本凭据无效，请重新连接".into(),0))?;
            relay::script::request(agent,&source,&c,||cancel.is_some_and(|c|c.load(std::sync::atomic::Ordering::Acquire)))
        },
        Provider::NewApi if key.starts_with('{')=>{
            let c:NewApiUserToken=serde_json::from_str(key).map_err(|_|("用户 Token 凭据无效，请重新连接".into(),0))?;
            relay::request_new_api_user(agent,&source,&c.access_token,&c.user_id,||cancel.is_some_and(|c|c.load(std::sync::atomic::Ordering::Acquire)))
        },
        Provider::NewApi|Provider::OneApi|Provider::Sub2Api|Provider::RelayGeneral=>relay::request_cancellable(agent,match provider{Provider::NewApi=>Kind::NewApi,Provider::OneApi=>Kind::OneApi,Provider::RelayGeneral=>Kind::General,_=>Kind::Sub2Api},&source,key,||cancel.is_some_and(|c|c.load(std::sync::atomic::Ordering::Acquire))),
        _=>Err(("不支持的 API 服务".into(),0))
    }
}
#[cfg(windows)]
fn credential(slot:&str)->Result<keyring::Entry,String>{keyring::Entry::new("Daozai.ApiQuota",slot).map_err(|_|"无法打开 Windows 凭据管理器".into())}
#[cfg(windows)]
fn save(slot:&str,key:&str)->Result<(),String>{
    if slot.starts_with("script-"){
        let path=script_path(slot)?;let sealed=super::gemini::protect(key.as_bytes(),true)?;
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|_|"无法创建查询脚本凭据目录")?;
        return crate::model::atomic_write(&path,&sealed).map_err(|_|"无法安全保存查询脚本".into());
    }
    credential(slot)?.set_password(key).map_err(|_|"无法安全保存凭据，未添加账号".into())
}
#[cfg(not(windows))]
fn save(_slot:&str,_key:&str)->Result<(),String>{Err("当前系统不支持安全保存 API Key，未添加账号".into())}
#[cfg(windows)]
fn load(slot:&str)->Result<String,String>{
    if slot.starts_with("script-"){
        let sealed=std::fs::read(script_path(slot)?).map_err(|_|"无法读取查询脚本，请重新连接")?;
        let raw=super::gemini::protect(&sealed,false)?;return String::from_utf8(raw).map_err(|_|"查询脚本凭据无效".into());
    }
    credential(slot)?.get_password().map_err(|_|"无法读取账号凭据，请重新连接".into())
}
#[cfg(not(windows))]
fn load(_slot:&str)->Result<String,String>{Err("当前系统不支持读取 API Key".into())}
fn delete_slot(slot:&str)->Result<(),String>{
    if slot.starts_with("script-"){return match std::fs::remove_file(script_path(slot)?){Ok(())=>Ok(()),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(()),Err(_)=>Err("查询脚本凭据清理未完成".into())};}
    #[cfg(windows)] {match credential(slot)?.delete_credential(){Ok(())|Err(keyring::Error::NoEntry)=>Ok(()),Err(_)=>Err("Windows 凭据清理未完成，将保留引用以便重试".into())}}
    #[cfg(not(windows))] {let _=slot;Err("当前系统不支持清理 Windows 凭据".into())}
}
fn journal_path()->PathBuf{crate::model::config_dir().join("ai-quota-vault-cleanup.json")}
fn journal_read()->Result<Vec<String>,String>{
    match std::fs::read(journal_path()){
        Ok(raw)=>serde_json::from_slice(&raw).map_err(|_|"凭据清理记录损坏，请先检查数据目录".into()),
        Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(vec![]),
        Err(_)=>Err("无法读取凭据清理记录".into())
    }
}
fn journal_save(slots:&[String])->Result<(),String>{
    let path=journal_path();if let Some(dir)=path.parent(){std::fs::create_dir_all(dir).map_err(|_|"无法创建凭据清理记录")?;}
    let raw=serde_json::to_vec(slots).map_err(|_|"无法生成凭据清理记录")?;
    crate::model::atomic_write(&path,&raw).map_err(|_|"无法安全保存凭据清理记录".into())
}
// Journal only opaque references BEFORE modifying vault/config, so every failure
// leaves enough information to retry without storing any secret outside the vault.
pub fn queue_cleanup(slot:&str)->Result<(),String>{
    let mut slots=journal_read()?;if !slots.iter().any(|s|s==slot){slots.push(slot.into());journal_save(&slots)?;}Ok(())
}
fn retained_cleanup(slots:Vec<String>,active:&[&str],mut delete:impl FnMut(&str)->Result<(),String>)->Vec<String>{
    slots.into_iter().filter(|s|!active.contains(&s.as_str())&&delete(s).is_err()).collect()
}
fn persisted_active_slots(value:&serde_json::Value)->Result<Vec<String>,String>{
    let accounts=value["accounts"].as_array().ok_or("额度配置不完整，已暂停凭据清理")?;
    let mut active=Vec::new();
    for row in accounts{
        let provider=row["provider"].as_str().and_then(Provider::parse).ok_or("额度配置含未知账号，已暂停凭据清理")?;
        if row["id"].as_str().is_none()||row["alias"].as_str().is_none()||row["source"].as_str().is_none(){return Err("额度账号配置不完整，已暂停凭据清理".into());}
        if provider.api_key(){active.push(row["slot"].as_str().filter(|s|!s.is_empty()).ok_or("额度凭据引用缺失，已暂停清理")?.into());}
    }
    Ok(active)
}
pub fn cleanup_pending(config:&Config)->Result<(),String>{
    let slots=journal_read()?;if slots.is_empty(){return Ok(());}
    // Never interpret Config::load's recovery default as evidence of no accounts.
    let raw=std::fs::read(crate::model::config_dir().join("ai-quota.json")).map_err(|_|"无法确认已保存账号，已暂停凭据清理")?;
    let value=serde_json::from_slice(&raw).map_err(|_|"额度配置损坏，已暂停凭据清理")?;
    let persisted=persisted_active_slots(&value)?;
    let mut active:Vec<_>=config.accounts.iter().filter(|a|a.provider.api_key()).filter_map(|a|a.slot.as_deref()).collect();
    active.extend(persisted.iter().map(String::as_str));
    let retained=retained_cleanup(slots,&active,delete_slot);journal_save(&retained)?;
    if retained.is_empty(){Ok(())}else{Err("账号配置已保留，部分旧凭据尚未清理；下次启动会重试".into())}
}
pub fn fetch(agent:&ureq::Agent,account:&Account)->Result<Snapshot,(String,u64)>{
    let slot=account.slot.as_deref().ok_or(("请连接 API Key".into(),0))?;
    request(agent,account.provider,&account.source.to_string_lossy(),&load(slot).map_err(|e|(e,0))?,None)
}

// Validation never persists a credential. Closing the dialog drops this result and
// the key, even if the in-flight bounded HTTP request finishes afterwards.
pub struct Validated {provider:Provider,source:String,alias:String,key:String,snapshot:Snapshot}
pub struct Connected {pub provider:Provider,pub source:String,pub alias:String,pub slot:String,pub snapshot:Snapshot}
impl Validated {
    pub fn finish(self)->Result<Connected,String>{
        // A random reference prevents config files exposing a reusable key fingerprint.
        use ring::rand::SecureRandom;
        let mut bytes=[0u8;24];ring::rand::SystemRandom::new().fill(&mut bytes).map_err(|_|"无法生成安全凭据引用")?;
        let slot=bytes.iter().map(|b|format!("{b:02x}")).collect::<String>();
        let slot=if self.provider==Provider::RelayCustom{format!("script-{slot}")}else{slot};
        queue_cleanup(&slot)?;
        save(&slot,&self.key)?;
        Ok(Connected{provider:self.provider,source:self.source,alias:self.alias,slot,snapshot:self.snapshot})
    }
}
static VALIDATING:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
struct ValidationSlot;
impl Drop for ValidationSlot{fn drop(&mut self){VALIDATING.store(false,std::sync::atomic::Ordering::Release);}}
pub struct Connect {rx:mpsc::Receiver<Result<Validated,String>>,cancel:std::sync::Arc<std::sync::atomic::AtomicBool>,deadline:Option<std::time::Instant>}
impl Drop for Connect{fn drop(&mut self){self.cancel.store(true,std::sync::atomic::Ordering::Release);}}
impl Connect {
    #[cfg(test)]
    pub(crate) fn pending_test()->(Self,std::sync::Arc<std::sync::atomic::AtomicBool>){
        let (_tx,rx)=mpsc::channel();let cancel=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        (Self{rx,cancel:cancel.clone(),deadline:None},cancel)
    }
    pub fn start(provider:Provider,source:String,alias:String,key:String)->Self{
        Self::start_inner(provider,source,alias,key,false)
    }
    pub fn start_auto(source:String,alias:String,key:String)->Self{
        Self::start_inner(Provider::NewApi,source,alias,key,true)
    }
    pub fn start_new_api_user(source:String,alias:String,token:String,user_id:String)->Self{
        let credential=NewApiUserToken{access_token:token.trim().into(),user_id:user_id.trim().into()};
        Self::start(Provider::NewApi,source,alias,serde_json::to_string(&credential).unwrap())
    }
    pub fn start_custom(source:String,alias:String,credential:relay::script::Credential)->Self{
        Self::start(Provider::RelayCustom,source,alias,serde_json::to_string(&credential).unwrap())
    }
    fn start_inner(provider:Provider,source:String,alias:String,key:String,automatic:bool)->Self{
        let(tx,rx)=mpsc::channel();let cancel=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));let worker_cancel=cancel.clone();
        let deadline=automatic.then(||std::time::Instant::now()+relay::AUTO_TIMEOUT);
        if VALIDATING.compare_exchange(false,true,std::sync::atomic::Ordering::AcqRel,std::sync::atomic::Ordering::Acquire).is_err(){let _=tx.send(Err("上次验证仍在结束，请稍后重试".into()));return Self{rx,cancel,deadline};}
        std::thread::spawn(move||{let _slot=ValidationSlot;
            let result=(||{
                let source=if provider.relay(){relay::normalize_base_url(&source)?}else{endpoint(provider,&source)?};
                let key=key.trim();if !(provider==Provider::RelayCustom||(provider==Provider::NewApi&&!automatic&&key.starts_with('{')))&&(key.is_empty()||key.len()>8192||key.chars().any(char::is_whitespace)||key.chars().any(char::is_control)){return Err("请输入有效的 API Key".into());}
                let (provider,snapshot)=if automatic{
                    let (kind,snapshot)=relay::detect_cancellable(super::http_agent_with_connect_timeout,&source,key,||worker_cancel.load(std::sync::atomic::Ordering::Acquire)).map_err(|e|e.0)?;
                    (match kind{Kind::NewApi=>Provider::NewApi,Kind::OneApi=>Provider::OneApi,Kind::Sub2Api=>Provider::Sub2Api,Kind::General=>Provider::RelayGeneral},snapshot)
                }else{(provider,request(&super::http_agent(),provider,&source,key,Some(&worker_cancel)).map_err(|e|e.0)?)};
                if worker_cancel.load(std::sync::atomic::Ordering::Acquire){return Err("已取消连接".into());}
                let alias=if alias.trim().is_empty(){if automatic{url::Url::parse(&source).ok().and_then(|u|u.host_str().map(str::to_owned)).unwrap_or_else(||"中转站".into())}else{provider.name().into()}}else{alias.trim().to_owned()};
                Ok(Validated{provider,source,alias,key:key.into(),snapshot})
            })();let _=tx.send(result);
        });Self{rx,cancel,deadline}
    }
    pub fn poll(&self)->Option<Result<Validated,String>>{
        if self.deadline.is_some_and(|d|std::time::Instant::now()>=d){self.cancel.store(true,std::sync::atomic::Ordering::Release);return Some(Err("自动识别超时，请稍后重试或在高级设置中手动选择".into()));}
        self.rx.try_recv().ok()
    }
}
impl Config {
    pub fn connect_api(&mut self,connected:&Connected,target:Option<&str>)->Result<String,String>{
        if let Some(id)=target {
            let a=self.accounts.iter_mut().find(|a|a.id==id&&(a.provider==connected.provider||(a.provider.relay()&&connected.provider.relay()))).ok_or("原账号已移除，请重新添加")?;
            if let Some(slot)=&a.slot{queue_cleanup(slot)?;}
            a.provider=connected.provider;a.source=PathBuf::from(&connected.source);a.slot=Some(connected.slot.clone());a.alias=connected.alias.clone();a.enabled=true;
            let id=a.id.clone();self.selected=id.clone();return Ok(id);
        }
        let id=format!("api-{}",connected.slot);self.selected=id.clone();
        self.accounts.push(Account{id:id.clone(),provider:connected.provider,alias:connected.alias.clone(),source:PathBuf::from(&connected.source),slot:Some(connected.slot.clone()),enabled:true,pinned:false});Ok(id)
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test]
    fn explicit_reconnection_can_change_relay_template_and_keep_preferences(){
        let mut config=Config::default();config.accounts.push(Account{id:"original-relay".into(),provider:Provider::NewApi,alias:"原账号".into(),source:"https://relay.example".into(),slot:None,enabled:true,pinned:true});config.colors.insert("original-relay".into(),super::super::Accent::Gold);
        let connected=Connected{provider:Provider::RelayGeneral,source:"https://relay.example".into(),alias:"我的中转站".into(),slot:"synthetic-ref".into(),snapshot:Snapshot::default()};
        assert_eq!(config.connect_api(&connected,Some("original-relay")).unwrap(),"original-relay");assert_eq!(config.accounts[0].provider,Provider::RelayGeneral);assert!(config.accounts[0].pinned);assert_eq!(config.colors["original-relay"],super::super::Accent::Gold);
    }
    #[test]
    fn new_api_user_token_reads_account_balance_instead_of_unlimited_key_quota(){
        let calls=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));let history=calls.clone();
        let agent=ureq::AgentBuilder::new().redirects(0).middleware(move|request:ureq::Request,_:ureq::MiddlewareNext|{
            history.lock().unwrap().push(request.url().to_owned());
            let body=if request.url().ends_with("/api/user/self"){
                assert_eq!(request.header("Authorization"),Some("Bearer synthetic-user-token"));
                assert_eq!(request.header("New-Api-User"),Some("42"));
                r#"{"success":true,"data":{"id":42,"quota":550000,"used_quota":1000000}}"#
            }else{
                assert!(request.url().ends_with("/api/status"));assert!(request.header("Authorization").is_none());
                r#"{"success":true,"data":{"quota_per_unit":500000,"quota_display_type":"USD"}}"#
            };ureq::Response::new(200,"OK",body)
        }).build();
        let credential=serde_json::json!({"access_token":"synthetic-user-token","user_id":"42"}).to_string();
        let snapshot=request(&agent,Provider::NewApi,"https://relay.example/tenant",&credential,None).unwrap();
        assert!(snapshot.windows.is_empty());let q=snapshot.relay.unwrap();
        assert_eq!(q.compact(),"1.1 USD");assert_eq!(q.primary().unwrap().used,Some(2.0));
        assert_eq!(*calls.lock().unwrap(),["https://relay.example/tenant/api/user/self","https://relay.example/tenant/api/status"]);
    }
    #[cfg(windows)]
    #[test]
    fn new_api_user_token_refreshes_from_vault_without_entering_config(){
        let slot=format!("user-token-test-{}-{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap());
        struct Cleanup(String);impl Drop for Cleanup{fn drop(&mut self){let _=delete_slot(&self.0);}}
        let _cleanup=Cleanup(slot.clone());
        let secret=serde_json::to_string(&NewApiUserToken{access_token:"synthetic-saved-user-token".into(),user_id:"42".into()}).unwrap();save(&slot,&secret).unwrap();
        let connected=Connected{provider:Provider::NewApi,source:"https://relay.example".into(),alias:"测试站点".into(),slot,snapshot:Snapshot::default()};
        let mut config=Config::default();config.connect_api(&connected,None).unwrap();let json=config.json();
        assert!(!json.to_string().contains("synthetic-saved-user-token"));let restored=Config::from_json(&json);let account=&restored.accounts[0];
        assert_eq!(new_api_user_id(account).as_deref(),Some("42"));
        let agent=ureq::AgentBuilder::new().middleware(|request:ureq::Request,_:ureq::MiddlewareNext|{
            let body=if request.url().ends_with("/api/user/self"){
                assert_eq!(request.header("Authorization"),Some("Bearer synthetic-saved-user-token"));assert_eq!(request.header("New-Api-User"),Some("42"));
                r#"{"success":true,"data":{"id":42,"quota":550000,"used_quota":1000000}}"#
            }else{r#"{"success":true,"data":{"quota_per_unit":500000,"quota_display_type":"USD"}}"#};ureq::Response::new(200,"OK",body)
        }).build();
        assert_eq!(fetch(&agent,account).unwrap().relay.unwrap().compact(),"1.1 USD");
    }
    #[cfg(windows)]
    #[test]
    fn custom_script_storage_is_encrypted_and_can_refresh_after_reload(){
        let slot=format!("script-{:048x}",chrono::Utc::now().timestamp_nanos_opt().unwrap());
        struct Cleanup(String);impl Drop for Cleanup{fn drop(&mut self){let _=delete_slot(&self.0);}}
        let _cleanup=Cleanup(slot.clone());
        let c=relay::script::Credential{script:format!("{}\n/*{}*/",relay::script::DEFAULT_SCRIPT,"说明".repeat(2000)),api_key:"synthetic-script-secret".into(),access_token:String::new(),user_id:String::new()};
        let raw=serde_json::to_string(&c).unwrap();save(&slot,&raw).unwrap();assert_eq!(load(&slot).unwrap(),raw);
        let sealed=std::fs::read(script_path(&slot).unwrap()).unwrap();assert!(!sealed.windows(c.api_key.len()).any(|w|w==c.api_key.as_bytes()));
        let mut config=Config::default();let connected=Connected{provider:Provider::RelayCustom,source:"https://relay.example".into(),alias:"自定义中转".into(),slot:slot.clone(),snapshot:Snapshot::default()};config.connect_api(&connected,None).unwrap();
        let json=config.json();assert!(!json.to_string().contains("synthetic-script-secret"));let restored=Config::from_json(&json);
        assert_eq!(custom_credentials(&restored.accounts[0]).unwrap().script,c.script);
        let agent=ureq::AgentBuilder::new().middleware(|r:ureq::Request,_:ureq::MiddlewareNext|{assert_eq!(r.header("Authorization"),Some("Bearer synthetic-script-secret"));ureq::Response::new(200,"OK",r#"{"balance":1.1}"#)}).build();
        assert_eq!(fetch(&agent,&restored.accounts[0]).unwrap().relay.unwrap().compact(),"1.1 USD");delete_slot(&slot).unwrap();assert!(!script_path(&slot).unwrap().exists());
        assert!(script_path("script-../../outside").is_err());
    }
    #[test]
    fn expired_detection_discards_even_a_ready_success_and_cancels_the_worker(){
        let (tx,rx)=mpsc::channel();let cancel=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        tx.send(Ok(Validated{provider:Provider::Sub2Api,source:"https://relay.example/prefix".into(),alias:"中转".into(),key:"synthetic".into(),snapshot:Snapshot::default()})).ok().unwrap();
        let connection=Connect{rx,cancel:cancel.clone(),deadline:Some(std::time::Instant::now())};
        assert!(connection.poll().unwrap().err().unwrap().contains("超时"));assert!(cancel.load(std::sync::atomic::Ordering::Acquire));
    }
    #[cfg(windows)]
    #[test]
    fn synthetic_key_roundtrips_without_accessing_saved_accounts(){
        let slot=format!("acceptance-{}-{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap());
        struct Cleanup(String);impl Drop for Cleanup{fn drop(&mut self){let _=delete_slot(&self.0);}}
        let _cleanup=Cleanup(slot.clone());
        save(&slot,"synthetic-quota-acceptance-key").unwrap();
        assert_eq!(load(&slot).unwrap(),"synthetic-quota-acceptance-key");
        delete_slot(&slot).unwrap();assert!(load(&slot).is_err());
    }
    #[test] fn malformed_or_unknown_config_never_proves_a_key_orphaned(){
        for value in [serde_json::json!({}),serde_json::json!({"accounts":[{"provider":"MiniMax"}]}),serde_json::json!({"accounts":[{"id":"a","provider":"FutureProvider","alias":"a","source":"","slot":"keep"}]})]{assert!(persisted_active_slots(&value).is_err());}
        let value=serde_json::json!({"accounts":[{"id":"a","provider":"MiniMax","alias":"a","source":"cn","slot":"keep"}]});assert_eq!(persisted_active_slots(&value).unwrap(),vec!["keep"]);
    }
    #[test] fn overlapping_validation_is_rejected_without_spawning_a_request(){
        assert!(!VALIDATING.swap(true,std::sync::atomic::Ordering::AcqRel));let _guard=ValidationSlot;
        let connection=Connect::start(Provider::OpenCodeGo,String::new(),String::new(),"synthetic".into());
        assert!(connection.poll().unwrap().is_err());
    }
    #[test] fn cleanup_keeps_failed_refs_and_never_deletes_active_keys(){
        let mut calls=vec![];let left=retained_cleanup(vec!["active".into(),"failed".into(),"done".into()],&["active"],|s|{calls.push(s.to_owned());if s=="failed"{Err("vault failure".into())}else{Ok(())}});
        assert_eq!(calls,vec!["failed","done"]);assert_eq!(left,vec!["failed"]);
    }
    #[test] fn regions_are_explicit_and_never_guessed(){
        assert_eq!(endpoint(Provider::MiniMax,"").unwrap(),"cn");assert_eq!(endpoint(Provider::Glm,"global").unwrap(),"global");
        assert!(endpoint(Provider::Glm,"https://evil.example").is_err());assert!(endpoint(Provider::Kimi,"").is_err());
    }
    #[test] fn config_has_only_opaque_reference_and_endpoint(){
        let c=Connected{provider:Provider::NewApi,source:"https://relay.example".into(),alias:"测试".into(),slot:"random-reference".into(),snapshot:Snapshot::default()};
        let mut config=Config::default();let id=config.connect_api(&c,None).unwrap();
        let value=config.json();assert_eq!(value["accounts"][0]["slot"],"random-reference");assert_eq!(config.accounts[0].id,id);
        assert_eq!(Config::from_json(&value),config);
    }
}
