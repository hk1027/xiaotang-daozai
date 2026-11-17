//! 只读 Claude 本地用量；不解密登录令牌，不请求远程接口。
use super::{Account,Snapshot,Window};
use ring::digest;
use serde_json::{json,Value};
use std::{fs,path::{Path,PathBuf}};

#[path="claude_usage_cache.rs"]
mod desktop_cache;

const HISTORY:&str="plan-usage-history.json";
const CODE_USAGE:&str="daozai-usage.json";
const COLLECTOR:&str="daozai-statusline.ps1";
pub(super) struct Profile {pub slot:String,pub label:String}

pub(super) fn paths(roaming:&Path,local:&Path)->Vec<PathBuf>{
    let mut paths=vec![roaming.join("Claude/config.json"),local.join("Claude/config.json")];
    if let Ok(entries)=fs::read_dir(local.join("Claude-Data")){
        for entry in entries.flatten(){if entry.file_name().to_string_lossy().to_ascii_lowercase().starts_with("claude_"){paths.push(entry.path().join("config.json"));}}
    }
    if let Ok(entries)=fs::read_dir(local.join("Packages")){
        for entry in entries.flatten(){if entry.file_name().to_string_lossy().to_ascii_lowercase().starts_with("claude_"){paths.push(entry.path().join("LocalCache/Roaming/Claude/config.json"));}}
    }
    paths.retain(|p|p.is_file());paths.sort();paths.dedup();paths
}
pub(super) fn is_desktop(path:&Path)->bool{matches!(path.file_name().and_then(|s|s.to_str()),Some("config.json"|HISTORY))}
pub(super) fn local_source(path:&Path)->PathBuf{
    if path.file_name().is_some_and(|s|s==".credentials.json"||s=="settings.json"){path.with_file_name(CODE_USAGE)}else{path.to_owned()}
}
fn read_json(path:&Path)->Result<Value,String>{
    let raw=fs::read(path).map_err(|_|"尚无 Claude 本地用量记录，请在官方客户端使用后刷新".to_string())?;
    serde_json::from_slice(raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&raw)).map_err(|_|"Claude 本地用量记录暂不可读，等待客户端更新".into())
}
fn slot(owner:&str,org:&str)->String{digest::digest(&digest::SHA256,format!("{owner}|{org}").as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect()}
fn desktop_identity(path:&Path)->Result<(String,i64),String>{
    // 只反序列化账号标识，忽略同文件中的加密登录缓存。
    #[derive(serde::Deserialize)]
    struct Identity {#[serde(rename="lastKnownAccountUuid")] owner:String}
    use std::io::Read;
    let mut file=fs::File::open(path.with_file_name("config.json")).map_err(|_|"无法确认 Claude 桌面版本地账号")?;
    let stamp=file.metadata().and_then(|m|m.modified()).map_err(|_|"无法确认 Claude 账号配置的更新时间")?;
    let mut raw=Vec::new();file.read_to_end(&mut raw).map_err(|_|"无法读取 Claude 账号配置")?;
    let identity:Identity=serde_json::from_slice(raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&raw)).map_err(|_|"请先在 Claude 桌面版登录".to_string())?;
    if identity.owner.is_empty(){return Err("请先在 Claude 桌面版登录".into());}Ok((identity.owner,chrono::DateTime::<chrono::Utc>::from(stamp).timestamp_millis()))
}
fn code_identity(path:&Path)->Result<(String,String),String>{
    let dir=path.parent().ok_or("未找到 Claude Code 本地目录")?;
    let identity=if dir.join(".claude.json").is_file(){dir.join(".claude.json")}else if dir.file_name().is_some_and(|n|n==".claude"){PathBuf::from(format!("{}.json",dir.to_string_lossy()))}else{dir.join(".claude.json")};
    #[derive(serde::Deserialize)]
    struct Identity {#[serde(rename="oauthAccount")] account:IdentityAccount}
    #[derive(serde::Deserialize)]
    struct IdentityAccount {#[serde(rename="accountUuid")] owner:String,#[serde(rename="organizationUuid")] org:String}
    let raw=fs::read(identity).map_err(|_|"请先在 Claude Code 登录订阅账号".to_string())?;
    let value:Identity=serde_json::from_slice(raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&raw)).map_err(|_|"无法确认 Claude Code 本地账号".to_string())?;
    if value.account.owner.is_empty()||value.account.org.is_empty(){return Err("无法确认 Claude Code 本地账号".into());}
    Ok((value.account.owner,value.account.org))
}
fn desktop_record(path:&Path,account:Option<&Account>,verified_cache:&Path)->Result<(String,Snapshot),String>{
    let (owner,login_stamp)=desktop_identity(path)?;let data=read_json(&path.with_file_name(HISTORY))?;
    if data["version"].as_u64()!=Some(2){return Err("本地记录缺少账号归属，请更新 Claude 桌面版后重试".into());}
    let now=chrono::Utc::now().timestamp_millis();
    let sample=data["samples"].as_array().into_iter().flatten().filter(|s|s["t"].as_i64().is_some_and(|t|t>0&&t<=now)).max_by_key(|s|s["t"].as_i64()).ok_or("请在 Claude 桌面版打开用量页面，等待当前账号产生新记录")?;
    let org=sample["org"].as_str().filter(|s|!s.is_empty()).ok_or("本地用量记录缺少账号归属，等待 Claude 更新")?;
    let account_slot=slot(&owner,org);
    if account.is_some_and(|a|a.slot.as_deref()!=Some(account_slot.as_str())){return Err("Claude 本地账号已变化，请重新连接".into());}
    let mut snapshot=Snapshot{at:sample["t"].as_i64().unwrap()/1000,..Default::default()};
    for (key,label,seconds) in [("fh","5 小时",18000),("sd","本周",604800)]{
        if let Some(used)=sample["u"][key].as_f64().filter(|n|n.is_finite()&&(0.0..=100.0).contains(n)){
            snapshot.windows.push(Window{label:label.into(),seconds:Some(seconds),remaining:(100.0-used) as f32,reset:None,amount:None});
        }
    }
    // 历史和浏览器缓存均按组织保存。账号配置较新时，只能回显岛仔已为该账号核验过的快照。
    if let Some(cached)=desktop_cache::read(&path.with_file_name("Cache").join("Cache_Data"),org,snapshot.at,now/1000)
        .filter(|s|s.at*1000>login_stamp){
        snapshot=cached;
    }else if sample["t"].as_i64().unwrap()<=login_stamp{
        snapshot=account.and_then(|a|{
            let raw=fs::read(verified_cache).ok()?;
            a.cached_snapshot(&serde_json::from_slice(&raw).ok()?)
        }).ok_or("请在 Claude 桌面版打开用量页面，等待当前账号产生新记录")?;
    }
    if snapshot.windows.is_empty(){return Err("Claude 本地记录暂无可用额度".into());}
    if desktop_identity(path)?!=(owner,login_stamp){return Err("Claude 账号配置已变化，请刷新重试".into());}
    Ok((account_slot,snapshot))
}
fn code_slot(path:&Path)->Result<String,String>{let(owner,org)=code_identity(path)?;Ok(format!("code-{}",slot(&owner,&org)))}
pub(super) fn profiles(path:&Path)->Result<Vec<Profile>,String>{
    let path=local_source(path);
    let (slot,label)=if is_desktop(&path){(desktop_record(&path,None,&crate::model::config_dir().join("ai-quota-cache.json"))?.0,"Claude · 桌面版本地记录")}else{(code_slot(&path)?,"Claude · Code 本地记录")};
    Ok(vec![Profile{slot,label:label.into()}])
}
pub(super) fn fetch(account:&Account)->Result<Snapshot,String>{
    let path=local_source(&account.source);
    let (slot,mut snapshot)=if is_desktop(&path){desktop_record(&path,Some(account),&crate::model::config_dir().join("ai-quota-cache.json"))?}else{
        let slot=code_slot(&path)?;let data=read_json(&path)?;
        if data["version"].as_u64()!=Some(1){return Err("Claude Code 本地记录格式不匹配，请启用本地采集".into());}
        let cache_slot=format!("code-{}",self::slot(data["account"].as_str().unwrap_or_default(),data["organization"].as_str().unwrap_or_default()));
        if slot!=cache_slot{return Err("Claude Code 账号已变化，等待新账号的本地记录".into());}
        let at=data["at"].as_i64().filter(|at|*at>0&&*at<=chrono::Utc::now().timestamp()+60).ok_or("Claude 本地记录时间无效")?;
        let mut snapshot=Snapshot{at,..Default::default()};
        for (key,label,seconds) in [("five_hour","5 小时",18000),("seven_day","本周",604800)]{
            let window=&data["rate_limits"][key];
            if let Some(used)=window["used_percentage"].as_f64().filter(|n|n.is_finite()&&(0.0..=100.0).contains(n)){
                let reset=window["resets_at"].as_i64().filter(|t|*t>0);
                if reset.is_some_and(|t|t<=chrono::Utc::now().timestamp()){continue;}
                snapshot.windows.push(Window{label:label.into(),seconds:Some(seconds),remaining:(100.0-used) as f32,reset,amount:None});
            }
        }
        if snapshot.windows.is_empty(){return Err("尚无有效的 Claude Code 本地额度，等待下一次使用".into());}
        (slot,snapshot)
    };
    if account.slot.as_deref()!=Some(slot.as_str()){return Err("Claude 本地账号已变化，请重新连接".into());}
    snapshot.windows.sort_by_key(|w|w.seconds);Ok(snapshot)
}
pub(super) fn code_path()->PathBuf{
    std::env::var_os("CLAUDE_CONFIG_DIR").filter(|s|!s.is_empty()).map(PathBuf::from).unwrap_or_else(||std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default().join(".claude")).join(CODE_USAGE)
}
pub(super) fn install_code_collector(path:&Path)->Result<(),String>{
    code_identity(path)?;let dir=path.parent().ok_or("未找到 Claude Code 目录")?;let settings_path=dir.join("settings.json");
    let mut settings=if settings_path.is_file(){read_json(&settings_path)?}else{json!({})};
    if !settings.is_object(){return Err("Claude Code 的 settings.json 顶层必须是 JSON 对象，请修正后重试".into());}
    let script=dir.join(COLLECTOR);let command=format!("powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File \"{}\"",script.to_string_lossy().replace('\\',"/"));
    if !settings["statusLine"].is_null()&&settings["statusLine"]["command"].as_str()!=Some(command.as_str()){
        return Err("Claude Code 已有状态栏配置，已保留；可选择现有脚本导出的本地用量 JSON".into());
    }
    let needs_status_line=settings["statusLine"].is_null();
    let script_content=format!("\u{feff}{}",include_str!("../scripts/claude-statusline.ps1"));
    crate::model::atomic_write(&script,script_content.as_bytes()).map_err(|_|"无法保存 Claude 本地采集脚本".to_string())?;
    if !needs_status_line{return Ok(());}
    settings["statusLine"]=json!({"type":"command","command":command});
    crate::model::atomic_write(&settings_path,&serde_json::to_vec_pretty(&settings).map_err(|_|"无法保存 Claude Code 设置")?).map_err(|_|"无法更新 Claude Code 状态栏配置".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore="按需验证已打开的 Claude 桌面版本地记录，不发送网络请求"]
    fn local_claude_desktop_probe(){
        let roaming=std::env::var_os("APPDATA").map(PathBuf::from).unwrap();let local=std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap();let mut count=0;
        for path in paths(&roaming,&local){
            let Ok(profiles)=profiles(&path)else{continue;};let before=fs::read(&path).unwrap();let history=fs::read(path.with_file_name(HISTORY)).unwrap();
            for profile in profiles{let a=account(path.clone(),profile.slot);let start=std::time::Instant::now();let snapshot=super::super::fetch(&ureq::Agent::new(),&a).unwrap();println!("desktop_local_windows={:?};record_at={};elapsed_ms={}",snapshot.windows.iter().map(|w|(&w.label,w.remaining,w.reset)).collect::<Vec<_>>(),snapshot.at,start.elapsed().as_millis());count+=1;}
            assert_eq!(fs::read(&path).unwrap(),before);assert_eq!(fs::read(path.with_file_name(HISTORY)).unwrap(),history);
        }
        assert!(count>0,"请先打开 Claude 桌面版的用量界面");
        let config=super::super::Config::load();for a in config.accounts.iter().filter(|a|a.provider==super::super::Provider::Claude&&is_desktop(&a.source)){assert!(fetch(a).is_ok(),"已有桌面版账号应能直接使用本地数据");}
    }
    #[test]
    #[ignore="只读验证已绑定的桌面版历史，不打开客户端或发送网络请求"]
    fn local_claude_closed_desktop_probe(){
        let config=super::super::Config::load();let mut count=0;
        for account in config.accounts.iter().filter(|a|a.provider==super::super::Provider::Claude&&is_desktop(&a.source)){
            let identity=fs::read(&account.source).unwrap();let history_path=account.source.with_file_name(HISTORY);let history=fs::read(&history_path).unwrap();
            let snapshot=fetch(account).expect("已绑定账号关闭客户端后仍应读到本地历史");println!("windows={:?};record_at={}",snapshot.windows.iter().map(|w|(&w.label,w.remaining,w.reset)).collect::<Vec<_>>(),snapshot.at);
            assert_eq!(fs::read(&account.source).unwrap(),identity);assert_eq!(fs::read(history_path).unwrap(),history);count+=1;
        }assert!(count>0,"尚无已绑定的 Claude 桌面版账号");
    }
    fn dir()->PathBuf{let p=std::env::temp_dir().join(format!("daozai-local-claude-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir_all(&p).unwrap();p}
    fn earlier_login(path:&Path){
        let stamp=std::time::SystemTime::now()-std::time::Duration::from_secs(300);
        fs::File::options().write(true).open(path).unwrap().set_modified(stamp).unwrap();
    }
    fn account(path:PathBuf,slot:String)->Account{Account{id:"local-test".into(),provider:super::super::Provider::Claude,alias:"本地测试".into(),source:path,slot:Some(slot),enabled:true,pinned:false}}
    fn code_fixture(dir:&Path)->PathBuf{fs::write(dir.join(".claude.json"),json!({"oauthAccount":{"accountUuid":"code-user","organizationUuid":"code-org"}}).to_string()).unwrap();dir.join(CODE_USAGE)}
    #[test]
    fn local_desktop_exit_keeps_history_for_the_previously_bound_identity(){
        let dir=dir();let path=dir.join("config.json");let now=chrono::Utc::now().timestamp();
        fs::write(&path,json!({"lastKnownAccountUuid":"owner","windowBounds":{"width":900}}).to_string()).unwrap();earlier_login(&path);
        let history=json!({"version":2,"samples":[{"t":(now-60)*1000,"org":"local-org","u":{"fh":24,"sd":84}}]}).to_string();fs::write(dir.join(HISTORY),&history).unwrap();
        let saved=account(path.clone(),profiles(&path).unwrap().remove(0).slot);let before=fetch(&saved).unwrap();
        let cache=dir.join("verified.json");
        fs::write(&cache,json!({&saved.id:{"provider":"Claude","slot":saved.slot,"source":saved.source,"local_identity_checked":saved.cache_identity_marker(),"snapshot":before}}).to_string()).unwrap();
        // 退出只保存窗口状态，账号标识没有变化。
        fs::write(&path,json!({"lastKnownAccountUuid":"owner","windowBounds":{"width":1200}}).to_string()).unwrap();
        let (_,after)=desktop_record(&path,Some(&saved),&cache).expect("已绑定账号退出后仍应显示自己的历史记录");assert_eq!((after.at,&after.windows),(before.at,&before.windows));assert_eq!(fs::read_to_string(dir.join(HISTORY)).unwrap(),history);
        let (_,restored)=desktop_record(&path,Some(&saved.clone()),&cache).unwrap();assert_eq!((restored.at,&restored.windows),(before.at,&before.windows),"岛仔重新启动后也应保留历史额度");
        fs::write(&path,json!({"lastKnownAccountUuid":"different-owner"}).to_string()).unwrap();assert!(fetch(&saved).is_err(),"真正切换账号后不能继续读取当前来源的历史");let _=fs::remove_dir_all(dir);
    }
    #[test]
    fn bound_desktop_login_does_not_adopt_a_peer_record_when_switching_back(){
        let dir=dir();let path=dir.join("config.json");let now=chrono::Utc::now().timestamp();
        let write_owner=|owner:&str,stamp:i64|{
            fs::write(&path,json!({"lastKnownAccountUuid":owner}).to_string()).unwrap();
            fs::File::options().write(true).open(&path).unwrap().set_modified(std::time::UNIX_EPOCH+std::time::Duration::from_secs(stamp as u64)).unwrap();
        };
        let sample=|at:i64,used:i32|fs::write(dir.join(HISTORY),json!({"version":2,"samples":[{"t":at*1000,"org":"shared","u":{"fh":used}}]}).to_string()).unwrap();
        let cache=dir.join("verified.json");
        write_owner("A",now-100);sample(now-90,11);
        let a=account(path.clone(),profiles(&path).unwrap().remove(0).slot);
        let first=fetch(&a).unwrap();
        fs::write(&cache,json!({&a.id:{"provider":"Claude","slot":a.slot,"source":a.source,"local_identity_checked":a.cache_identity_marker(),"snapshot":first}}).to_string()).unwrap();
        write_owner("B",now-60);sample(now-50,86);assert!(fetch(&a).is_err());
        write_owner("A",now-10);
        desktop_cache::fixture(&dir.join("Cache/Cache_Data"),"shared",now-49,br#"{"five_hour":{"utilization":87}}"#);
        let (_,result)=desktop_record(&path,Some(&a),&cache).unwrap();
        assert_eq!(result.at,first.at);assert_eq!(result.windows,first.windows);
        let mut legacy:Value=serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
        legacy[&a.id]["local_identity_checked"]=true.into();fs::write(&cache,legacy.to_string()).unwrap();
        assert!(desktop_record(&path,Some(&a),&cache).is_err());
        fs::remove_file(&cache).unwrap();assert!(desktop_record(&path,Some(&a),&cache).is_err());
        sample(now-2,35);assert_eq!(fetch(&a).unwrap().windows[0].remaining,65.0);
        let _=fs::remove_dir_all(dir);
    }
    #[test]
    fn local_desktop_cache_resets_use_the_same_observation_and_never_request_the_network(){
        let dir=dir();let source=dir.join("config.json");let now=chrono::Utc::now().timestamp();
        let identity=json!({"lastKnownAccountUuid":"owner","oauth:tokenCacheV2":"do-not-use"}).to_string();fs::write(&source,&identity).unwrap();earlier_login(&source);
        let history=json!({"version":2,"samples":[{"t":(now-30)*1000,"org":"local-org","u":{"fh":24,"sd":80}}]}).to_string();fs::write(dir.join(HISTORY),&history).unwrap();
        let cache=dir.join("Cache/Cache_Data");let body=serde_json::to_vec(&json!({"five_hour":{"utilization":26,"resets_at":chrono::DateTime::from_timestamp(now+3600,0).unwrap().to_rfc3339()},"seven_day":{"utilization":84,"resets_at":chrono::DateTime::from_timestamp(now+86400,0).unwrap().to_rfc3339()}})).unwrap();
        desktop_cache::fixture(&cache,"local-org",now-10,&zstd::stream::encode_all(body.as_slice(),1).unwrap());
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();listener.set_nonblocking(true).unwrap();let agent=ureq::AgentBuilder::new().proxy(ureq::Proxy::new(format!("http://{}",listener.local_addr().unwrap())).unwrap()).timeout(std::time::Duration::from_millis(100)).build();let account=account(source.clone(),slot("owner","local-org"));
        let snapshot=super::super::fetch(&agent,&account).unwrap();assert_eq!(snapshot.windows.iter().map(|w|w.remaining).collect::<Vec<_>>(),vec![74.0,16.0]);assert_eq!(snapshot.windows[0].reset,Some(now+3600));assert_eq!(snapshot.at,now-10,"不能把读取时间当成官方记录时间");
        for (org,at,bytes) in [("other-org",now-10,body.clone()),("local-org",now-60,body),("local-org",now-10,b"invalid-response".to_vec())]{desktop_cache::fixture(&cache,org,at,&bytes);let snapshot=super::super::fetch(&agent,&account).unwrap();assert_eq!(snapshot.windows.iter().map(|w|w.remaining).collect::<Vec<_>>(),vec![76.0,20.0]);assert!(snapshot.windows.iter().all(|w|w.reset.is_none()),"不可用缓存必须降级为未知，不能猜测重置时间");}
        assert_eq!(listener.accept().unwrap_err().kind(),std::io::ErrorKind::WouldBlock);assert_eq!(fs::read_to_string(source).unwrap(),identity);assert_eq!(fs::read_to_string(dir.join(HISTORY)).unwrap(),history);let _=fs::remove_dir_all(dir);
    }
    #[test]
    fn local_desktop_switching_organizations_or_users_does_not_reuse_old_records(){
        let dir=dir();let path=dir.join("config.json");fs::write(&path,json!({"lastKnownAccountUuid":"owner"}).to_string()).unwrap();earlier_login(&path);let now=chrono::Utc::now().timestamp_millis();
        let old=account(path.clone(),slot("owner","old-org"));
        fs::write(dir.join(HISTORY),json!({"version":2,"samples":[{"t":now-1000,"org":"old-org","u":{"fh":2}},{"t":now,"org":"new-org","u":{"fh":17,"sd":101}}]}).to_string()).unwrap();
        assert!(fetch(&old).is_err());let current=account(path.clone(),slot("owner","new-org"));let snapshot=fetch(&current).unwrap();assert_eq!(snapshot.windows.len(),1);assert_eq!(snapshot.windows[0].remaining,83.0);
        fs::write(&path,json!({"lastKnownAccountUuid":"other-owner"}).to_string()).unwrap();assert!(fetch(&current).is_err());
        fs::write(dir.join(HISTORY),json!({"version":1,"samples":[{"t":now,"fh":25,"sd":70}]}).to_string()).unwrap();assert!(profiles(&path).is_err(),"缺少账号归属的旧版历史不能导入");let _=fs::remove_dir_all(dir);
    }
    #[test]
    fn switched_desktop_login_waits_for_its_own_usage(){
        let dir=dir();let path=dir.join("config.json");let now=chrono::Utc::now().timestamp();
        let write_owner=|owner:&str,at:u64|{
            fs::write(&path,json!({"lastKnownAccountUuid":owner}).to_string()).unwrap();
            fs::File::options().write(true).open(&path).unwrap().set_modified(std::time::UNIX_EPOCH+std::time::Duration::from_secs(at)).unwrap();
        };
        write_owner("old",(now-120) as u64);
        let write_sample=|at:i64|fs::write(dir.join(HISTORY),json!({"version":2,"samples":[{"t":at*1000,"org":"shared","u":{"fh":30}}]}).to_string()).unwrap();
        write_sample(now-60);assert!(profiles(&path).is_ok());
        write_owner("new",(now-20) as u64);
        desktop_cache::fixture(&dir.join("Cache/Cache_Data"),"shared",now-60,br#"{"five_hour":{"utilization":30}}"#);
        assert!(profiles(&path).is_err());
        desktop_cache::fixture(&dir.join("Cache/Cache_Data"),"shared",now-5,br#"{"five_hour":{"utilization":47}}"#);
        let active=account(path.clone(),slot("new","shared"));assert_eq!(fetch(&active).unwrap().windows[0].remaining,53.0);
        desktop_cache::fixture(&dir.join("Cache/Cache_Data"),"shared",now-60,br#"{"five_hour":{"utilization":30}}"#);
        write_sample(now-10);assert_eq!(profiles(&path).unwrap()[0].slot,slot("new","shared"));
        let _=fs::remove_dir_all(dir);
    }
    #[test]
    fn invalid_code_settings_do_not_install_or_change_files(){
        let dir=dir();let path=code_fixture(&dir);let settings=dir.join("settings.json");
        for raw in ["[]","null","42"] {
            fs::write(&settings,raw).unwrap();assert!(install_code_collector(&path).is_err());
            assert_eq!(fs::read_to_string(&settings).unwrap(),raw);assert!(!dir.join(COLLECTOR).exists());
        }
        let _=fs::remove_dir_all(dir);
    }
    #[test]
    fn local_code_data_keeps_resets_and_rejects_a_previous_login(){
        let dir=dir();let path=code_fixture(&dir);let now=chrono::Utc::now().timestamp();
        let data=json!({"version":1,"account":"code-user","organization":"code-org","at":now-10,"rate_limits":{"five_hour":{"used_percentage":23.5,"resets_at":now+400},"seven_day":{"used_percentage":41.2,"resets_at":now+4000}}});fs::write(&path,data.to_string()).unwrap();
        let account=account(path.clone(),code_slot(&path).unwrap());let snapshot=fetch(&account).unwrap();assert_eq!(snapshot.windows[0].remaining,76.5);assert_eq!(snapshot.windows[0].reset,Some(now+400));assert_eq!(snapshot.at,now-10);
        let mut expired=data.clone();expired["rate_limits"]["five_hour"]["resets_at"]=json!(now-1);fs::write(&path,expired.to_string()).unwrap();assert_eq!(fetch(&account).unwrap().windows.len(),1);
        fs::write(dir.join(".claude.json"),json!({"oauthAccount":{"accountUuid":"other-user","organizationUuid":"code-org"}}).to_string()).unwrap();assert!(fetch(&account).is_err());let _=fs::remove_dir_all(dir);
    }
    #[test]
    fn local_collector_install_preserves_other_settings_and_existing_status_lines(){
        let dir=dir();let path=code_fixture(&dir);let settings_path=dir.join("settings.json");fs::write(&settings_path,json!({"permissions":{"allow":["Read"]},"hooks":{"Stop":[]}}).to_string()).unwrap();
        install_code_collector(&path).unwrap();let first=read_json(&settings_path).unwrap();assert_eq!(first["permissions"]["allow"][0],"Read");assert!(first["hooks"]["Stop"].is_array());install_code_collector(&path).unwrap();assert_eq!(read_json(&settings_path).unwrap(),first);
        let mut customized=first.clone();customized["statusLine"]["padding"]=json!(2);
        let customized=customized.to_string();fs::write(&settings_path,&customized).unwrap();install_code_collector(&path).unwrap();assert_eq!(fs::read_to_string(&settings_path).unwrap(),customized);
        let custom=json!({"statusLine":{"type":"command","command":"my-existing-statusline"},"padding":2}).to_string();fs::write(&settings_path,&custom).unwrap();assert!(install_code_collector(&path).is_err());assert_eq!(fs::read_to_string(&settings_path).unwrap(),custom);let _=fs::remove_dir_all(dir);
    }
    #[cfg(windows)]
    #[test]
    fn local_statusline_collector_saves_only_quota_fields(){
        use std::{io::Write,process::{Command,Stdio}};use std::os::windows::process::CommandExt;
        let dir=dir();let path=code_fixture(&dir);install_code_collector(&path).unwrap();
        let mut child=Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-File"]).arg(dir.join(COLLECTOR)).creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
        let input=json!({"model":{"display_name":"Claude"},"transcript_path":"private-chat-path","messages":["private-chat-content"],"rate_limits":{"five_hour":{"used_percentage":12.5,"resets_at":chrono::Utc::now().timestamp()+1000}}});
        child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();let output=child.wait_with_output().unwrap();assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
        let raw=fs::read_to_string(&path).unwrap();assert!(!raw.contains("private-chat"));let data=read_json(&path).unwrap();assert_eq!(data["account"],"code-user");assert_eq!(data["rate_limits"]["five_hour"]["used_percentage"],12.5);assert!(fetch(&account(path.clone(),code_slot(&path).unwrap())).is_ok());let _=fs::remove_dir_all(dir);
    }
    #[test]
    fn local_desktop_history_works_without_a_login_token_and_keeps_the_record_time(){
        let dir=std::env::temp_dir().join(format!("daozai-local-claude-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&dir).unwrap();let source=dir.join("config.json");
        let identity=serde_json::json!({"lastKnownAccountUuid":"local-user","oauth:tokenCacheV2":"unreadable-token"}).to_string();
        fs::write(&source,&identity).unwrap();earlier_login(&source);
        let at=chrono::Utc::now().timestamp_millis()-60_000;
        let history=serde_json::json!({"version":2,"samples":[{"t":at-1000,"org":"local-org","u":{"fh":8,"sd":70}},{"t":at,"org":"local-org","u":{"fh":11,"sd":76}}]}).to_string();
        fs::write(dir.join("plan-usage-history.json"),&history).unwrap();
        let slot=digest::digest(&digest::SHA256,b"local-user|local-org").as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>();
        let mut account=Account{id:"local-test".into(),provider:super::super::Provider::Claude,alias:"本地测试".into(),source:source.clone(),slot:Some(slot),enabled:true,pinned:false};
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();listener.set_nonblocking(true).unwrap();
        let agent=ureq::AgentBuilder::new().proxy(ureq::Proxy::new(format!("http://{}",listener.local_addr().unwrap())).unwrap()).timeout(std::time::Duration::from_millis(100)).build();
        let snapshot=super::super::fetch(&agent,&account).expect("Claude 必须只读取本地用量，不依赖登录令牌");
        assert_eq!(snapshot.windows.iter().map(|w|w.remaining).collect::<Vec<_>>(),vec![89.0,24.0]);
        assert_eq!(snapshot.at,at/1000);assert!(snapshot.windows.iter().all(|w|w.reset.is_none()),"历史缓存没有重置时间，不能推算");
        account.slot=Some("another-account".into());assert!(super::super::fetch(&ureq::Agent::new(),&account).is_err(),"本地缓存不能串号");
        fs::remove_file(dir.join("plan-usage-history.json")).unwrap();account.slot=None;
        assert!(super::super::fetch(&agent,&account).unwrap_err().0.contains("本地"),"没有缓存也不能回退联网");
        assert_eq!(listener.accept().unwrap_err().kind(),std::io::ErrorKind::WouldBlock,"Claude 本地读取不能发起任何代理连接");
        assert_eq!(fs::read_to_string(&source).unwrap(),identity);let _=fs::remove_dir_all(dir);
    }
}
