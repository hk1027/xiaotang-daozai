//! Agent 额度。配置只保存登录引用，独立授权由系统安全保存。
use serde_json::{json, Value};
use std::{collections::HashMap, fs, io::Read, path::{Path, PathBuf}, sync::mpsc, time::{Duration, Instant}};
use base64::Engine;
#[path="grok_quota.rs"]
pub(crate) mod grok;
#[path="gemini_quota.rs"]
pub(crate) mod gemini;
#[path="claude_quota.rs"]
mod claude;
#[path="workbuddy_quota.rs"]
pub(crate) mod workbuddy;
#[path="deepseek_quota.rs"]
pub(crate) mod deepseek;
#[path="zcode_quota.rs"]
pub(crate) mod zcode;
#[path="commandcode_quota.rs"]
pub(crate) mod commandcode;
#[path="native_auth.rs"]
mod native_auth;

pub struct Discovery {pub added:usize,pub notes:Vec<String>}
impl Discovery {
    pub fn message(&self)->String{
        let mut text=if self.added>0{format!("新增 {} 个账号",self.added)}else{"未发现新的本机账号".into()};
        if !self.notes.is_empty(){text.push_str("；");text.push_str(&self.notes.join("；"));}text
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Provider { #[default] Codex, Claude, Kimi, GrokBot, Gemini, ZCode, WorkBuddy, DeepSeek, CommandCode }
impl Provider {
    pub const ALL: [Self; 9] = [Self::Codex, Self::Claude, Self::Kimi, Self::GrokBot, Self::Gemini, Self::ZCode, Self::WorkBuddy, Self::DeepSeek, Self::CommandCode];
    pub fn name(self) -> &'static str { match self {Self::Codex=>"Codex",Self::Claude=>"Claude",Self::Kimi=>"Kimi",Self::GrokBot=>"Grok Bot",Self::Gemini=>"Gemini",Self::ZCode=>"ZCode",Self::WorkBuddy=>"WorkBuddy",Self::DeepSeek=>"DeepSeek",Self::CommandCode=>"Command Code"} }
    pub fn balance(self)->bool{matches!(self,Self::WorkBuddy|Self::DeepSeek)}
    fn parse(s: &str) -> Option<Self> {if s.eq_ignore_ascii_case("grokbot"){return Some(Self::GrokBot);}Self::ALL.into_iter().find(|p|p.name().eq_ignore_ascii_case(s))}
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Accent { #[default] Brand, Violet, Orange, Blue, Cyan, Pink, Gold, Green }
impl Accent {
    pub const ALL:[Self;8]=[Self::Brand,Self::Violet,Self::Orange,Self::Blue,Self::Cyan,Self::Pink,Self::Gold,Self::Green];
    pub fn key(self)->&'static str{match self{Self::Brand=>"brand",Self::Violet=>"violet",Self::Orange=>"orange",Self::Blue=>"blue",Self::Cyan=>"cyan",Self::Pink=>"pink",Self::Gold=>"gold",Self::Green=>"green"}}
    pub fn label(self)->&'static str{match self{Self::Brand=>"跟随厂商",Self::Violet=>"蓝紫",Self::Orange=>"暖橙",Self::Blue=>"湛蓝",Self::Cyan=>"青蓝",Self::Pink=>"玫红",Self::Gold=>"金黄",Self::Green=>"翠绿"}}
    fn parse(key:&str)->Self{Self::ALL.into_iter().find(|c|c.key()==key).unwrap_or_default()}
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {pub id:String, pub provider:Provider, pub alias:String, pub source:PathBuf, pub slot:Option<String>, pub enabled:bool, pub pinned:bool}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {pub enabled:bool, pub compact:bool, pub selected:String, pub accounts:Vec<Account>, pub colors:HashMap<String,Accent>}
impl Default for Config {fn default()->Self {Self{enabled:true,compact:true,selected:String::new(),accounts:vec![],colors:HashMap::new()}}}
impl Config {
    pub fn load()->Self {
        let path=crate::model::config_dir().join("ai-quota.json");
        let Ok(raw)=fs::read(&path) else {return Self::default();};
        serde_json::from_slice(&raw).ok().map(|v|Self::from_json(&v)).unwrap_or_default()
    }
    fn from_json(v:&Value)->Self {
        let accounts=v["accounts"].as_array().into_iter().flatten().filter_map(|a|Some(Account{
            id:a["id"].as_str()?.into(),provider:Provider::parse(a["provider"].as_str()?)?,alias:a["alias"].as_str()?.into(),source:PathBuf::from(a["source"].as_str()?),slot:a["slot"].as_str().map(str::to_owned),enabled:a["enabled"].as_bool().unwrap_or(true),pinned:a["pinned"].as_bool().unwrap_or(false)
        })).collect();
        let colors=v["colors"].as_object().into_iter().flatten().filter_map(|(id,value)|{let color=Accent::parse(value.as_str()?);(color!=Accent::Brand).then(||(id.clone(),color))}).collect();
        Self{enabled:v["enabled"].as_bool().unwrap_or(true),compact:v["compact"].as_bool().unwrap_or(true),selected:v["selected"].as_str().unwrap_or_default().into(),accounts,colors}
    }
    fn json(&self)->Value {let colors:serde_json::Map<String,Value>=self.colors.iter().map(|(id,color)|(id.clone(),json!(color.key()))).collect();json!({"enabled":self.enabled,"compact":self.compact,"selected":self.selected,"colors":colors,"accounts":self.accounts.iter().map(|a|json!({"id":a.id,"provider":a.provider.name(),"alias":a.alias,"source":a.source,"slot":a.slot,"enabled":a.enabled,"pinned":a.pinned})).collect::<Vec<_>>()})}
    pub fn save(&self) {
        let dir=crate::model::config_dir();let _=fs::create_dir_all(&dir);
        if let Ok(raw)=serde_json::to_vec_pretty(&self.json()){let _=crate::model::atomic_write(&dir.join("ai-quota.json"),&raw);}
    }
    pub fn add(&mut self,provider:Provider,alias:String,source:PathBuf)->Result<(),String>{
        if provider==Provider::ZCode{return zcode::add(self,&source,&alias);}
        if provider==Provider::WorkBuddy{return Err("请使用浏览器连接 WorkBuddy".into());}
        if provider==Provider::DeepSeek{return Err("请使用 API Key 连接 DeepSeek".into());}
        if provider==Provider::CommandCode{return Err("请使用 API Key 连接 Command Code".into());}
        if provider==Provider::Gemini{return Err("请使用浏览器或 Cookie 连接 Gemini 网页账号".into());}
        if provider==Provider::GrokBot{return self.import_grok(&source,&alias).map(|_|());}
        if provider==Provider::Claude{return self.import_claude(&source,&alias).map(|_|());}
        if !source.is_file(){return Err("请选择已有的登录 JSON 文件".into());}
        let auth=credentials(provider,&source)?;
        if self.accounts.iter().filter(|a|a.provider==provider).any(|a|account_credentials(a).is_ok_and(|existing|same_login(&existing,&auth))){return Err("这个账号已经添加".into());}
        let path=fs::canonicalize(&source).unwrap_or(source);
        if self.accounts.iter().any(|a|a.provider==provider && a.source.to_string_lossy().eq_ignore_ascii_case(&path.to_string_lossy())){return Err("这个登录文件已经添加".into());}
        let next=self.accounts.iter().filter(|a|a.provider==provider).count()+1;
        let alias=if alias.trim().is_empty(){format!("{} · 账号 {}",provider.name(),next)}else{alias.trim().to_owned()};
        let id=format!("{}-{}",provider.name(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        let pinned=self.accounts.is_empty();if pinned{self.selected=id.clone();}
        self.accounts.push(Account{id,provider,alias,source:path,slot:None,enabled:true,pinned});Ok(())
    }
    pub(crate) fn connect_grok(&mut self,slot:String,alias:String){
        if let Some(a)=self.accounts.iter_mut().find(|a|a.provider==Provider::GrokBot && a.slot.as_deref()==Some(&slot)){
            a.source=PathBuf::new();a.enabled=true;if !alias.trim().is_empty(){a.alias=alias;}self.selected=a.id.clone();return;
        }
        let id=format!("GrokBot-{slot}");let pinned=self.pinned_indices().len()<3;
        let alias=if alias.trim().is_empty(){format!("Grok Bot · 账号 {}",self.accounts.iter().filter(|a|a.provider==Provider::GrokBot).count()+1)}else{alias};
        self.selected=id.clone();self.accounts.push(Account{id,provider:Provider::GrokBot,alias,source:PathBuf::new(),slot:Some(slot),enabled:true,pinned});
    }
    pub(crate) fn connect_workbuddy(&mut self,slot:String,alias:String)->String{
        if let Some(a)=self.accounts.iter_mut().find(|a|a.provider==Provider::WorkBuddy && a.slot.as_deref()==Some(&slot)){
            a.enabled=true;if !alias.trim().is_empty(){a.alias=alias;}self.selected=a.id.clone();return a.id.clone();
        }
        let id=format!("WorkBuddy-{slot}");let pinned=true;
        self.selected=id.clone();self.accounts.push(Account{id:id.clone(),provider:Provider::WorkBuddy,alias,source:PathBuf::new(),slot:Some(slot),enabled:true,pinned});id
    }
    pub(crate) fn connect_gemini(&mut self,slot:String,alias:String)->String{
        if let Some(a)=self.accounts.iter_mut().find(|a|a.provider==Provider::Gemini && a.slot.as_deref()==Some(&slot)){
            a.enabled=true;if !alias.trim().is_empty(){a.alias=alias;}self.selected=a.id.clone();return a.id.clone();
        }
        let id=format!("Gemini-{slot}");let pinned=self.pinned_indices().len()<3;
        self.selected=id.clone();self.accounts.push(Account{id:id.clone(),provider:Provider::Gemini,alias,source:PathBuf::new(),slot:Some(slot),enabled:true,pinned});id
    }
    pub(crate) fn import_grok(&mut self,path:&Path,alias:&str)->Result<usize,String>{
        let profiles=grok::profiles(path)?;let path=fs::canonicalize(path).unwrap_or_else(|_|path.to_owned());let mut added=0;
        for profile in profiles{
            if self.accounts.iter().any(|a|a.provider==Provider::GrokBot && a.slot.as_deref()==Some(&profile.slot)){continue;}
            let id=format!("GrokBot-{}",profile.slot);let pinned=self.pinned_indices().len()<3;if self.accounts.is_empty(){self.selected=id.clone();}
            let label=if alias.trim().is_empty(){profile.label}else{format!("{alias} · {}",added+1)};
            self.accounts.push(Account{id,provider:Provider::GrokBot,alias:label,source:path.clone(),slot:Some(profile.slot),enabled:true,pinned});added+=1;
        }
        Ok(added)
    }
    fn import_claude(&mut self,path:&Path,alias:&str)->Result<usize,String>{
        let path=claude::local_source(path);let profiles=claude::profiles(&path)?;let path=fs::canonicalize(&path).unwrap_or_else(|_|path.parent().and_then(|p|fs::canonicalize(p).ok()).and_then(|p|path.file_name().map(|name|p.join(name))).unwrap_or_else(||path.clone()));let mut added=0;
        for profile in profiles{
            if let Some(existing)=self.accounts.iter_mut().find(|a|a.provider==Provider::Claude&&a.slot.as_deref()==Some(&profile.slot)){
                if claude::fetch(existing).is_err(){existing.source=path.clone();}continue;
            }
            if let Some(existing)=self.accounts.iter_mut().find(|a|a.provider==Provider::Claude&&a.slot.is_none()&&claude::local_source(&a.source)==path){existing.source=path.clone();existing.slot=Some(profile.slot);continue;}
            let id=format!("ClaudeLocal-{}",profile.slot);let pinned=self.accounts.is_empty();if pinned{self.selected=id.clone();}
            let label=if alias.trim().is_empty(){profile.label}else{alias.trim().to_owned()};
            self.accounts.push(Account{id,provider:Provider::Claude,alias:label,source:path.clone(),slot:Some(profile.slot),enabled:true,pinned});added+=1;
        }
        Ok(added)
    }
    pub(crate) fn connect_claude_code(&mut self,alias:&str)->Result<usize,String>{
        let path=claude::code_path();claude::install_code_collector(&path)?;self.import_claude(&path,alias)
    }
    pub(crate) fn connect_claude_desktop(&mut self,alias:&str)->Result<usize,String>{
        let roaming=std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();let local=std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();let mut added=0;let mut found=false;let mut error="请先打开 Claude 桌面版并查看用量".to_string();
        for path in claude::paths(&roaming,&local){match self.import_claude(&path,alias){Ok(n)=>{added+=n;found=true;},Err(e)=>error=e}}
        if found{Ok(added)}else{Err(error)}
    }
    pub fn discover(&mut self)->Discovery {
        let home=std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default();
        let mut candidates=vec![(Provider::Codex,home.join(".codex/auth.json")),(Provider::Claude,claude::code_path()),(Provider::Kimi,home.join(".kimi/credentials/kimi-code.json")),(Provider::Kimi,home.join(".kimi-code/credentials/kimi-code.json"))];
        if let Some(dir)=std::env::var_os("CODEX_HOME").filter(|s|!s.is_empty()){candidates.push((Provider::Codex,PathBuf::from(dir).join("auth.json")));}
        candidates.extend(self.accounts.iter().filter(|a|matches!(a.provider,Provider::Codex|Provider::Kimi)||(a.provider==Provider::Claude&&a.slot.is_none())).map(|a|(a.provider,a.source.clone())));
        let before=self.accounts.len();
        let mut notes=self.discover_files(&candidates);
        if let Some(path)=zcode::path().filter(|p|p.is_file()){if let Err(e)=zcode::add(self,&path,""){notes.push(e);}}
        if let (Some(roaming),Some(local))=(std::env::var_os("APPDATA"),std::env::var_os("LOCALAPPDATA")){
            for path in claude::paths(Path::new(&roaming),Path::new(&local)){
                if claude::is_desktop(&path){if let Err(error)=self.import_claude(&path,""){notes.push(error);}}
            }
        }
        if let Some(path)=grok::native_path(){if let Err(error)=self.import_grok(&path,""){notes.push(error);}}
        notes.sort();notes.dedup();Discovery{added:self.accounts.len()-before,notes}
    }
    fn discover_files(&mut self,candidates:&[(Provider,PathBuf)])->Vec<String>{
        let mut notes=Vec::new();
        for (provider,path) in candidates.iter().filter(|(_,p)|p.is_file()){
            if *provider==Provider::Claude{if let Err(error)=self.import_claude(path,""){notes.push(error);}continue;}
            let auth=match credentials(*provider,path){Ok(auth)=>auth,Err(error)=>{notes.push(format!("{}：{error}",provider.name()));continue;}};
            if let Some(existing)=self.accounts.iter_mut().filter(|a|a.provider==*provider).find(|a|account_credentials(a).is_ok_and(|old|same_login(&old,&auth))){
                if auth.stamp>fs::metadata(&existing.source).and_then(|m|m.modified()).ok(){existing.source=fs::canonicalize(path).unwrap_or_else(|_|path.clone());}
                continue;
            }
            let next=self.accounts.iter().filter(|a|a.provider==*provider).count()+1;
            if let Err(error)=self.add(*provider,format!("{} · 本机 {}",provider.name(),next),path.clone()){notes.push(format!("{}：{error}",provider.name()));}
        }
        notes
    }
    pub fn selected_index(&self)->Option<usize>{self.accounts.iter().position(|a|a.id==self.selected && a.enabled).or_else(||self.accounts.iter().position(|a|a.enabled))}
    pub fn move_account(&mut self,from:&str,to:&str){
        if let (Some(source),Some(target))=(self.accounts.iter().position(|a|a.id==from),self.accounts.iter().position(|a|a.id==to)){
            let account=self.accounts.remove(source);self.accounts.insert(target,account);
        }
    }
    pub fn ordered_indices(&self,pinned:bool)->Vec<usize>{let mut indices:Vec<_>=self.accounts.iter().enumerate().filter_map(|(i,a)|(a.enabled && (!pinned||a.pinned)).then_some(i)).collect();indices.sort_by_key(|i|match self.accounts[*i].provider{Provider::WorkBuddy=>1,Provider::DeepSeek=>2,_=>0});indices}
    pub fn pinned_indices(&self)->Vec<usize>{self.ordered_indices(true)}
    pub fn reconnect_deepseek(&mut self,id:&str,slot:String,alias:String)->Result<String,String>{
        if self.accounts.iter().any(|a|a.id!=id&&a.provider==Provider::DeepSeek&&a.slot.as_deref()==Some(&slot)){return Err("这把 API Key 已连接到另一个账号".into());}
        let account=self.accounts.iter_mut().find(|a|a.id==id&&a.provider==Provider::DeepSeek).ok_or("原账号已移除，请重新添加")?;
        account.slot=Some(slot);account.alias=alias;self.selected=id.into();Ok(id.into())
    }
    pub fn connect_deepseek(&mut self,slot:String,alias:String)->String{
        if let Some(a)=self.accounts.iter_mut().find(|a|a.provider==Provider::DeepSeek&&a.slot.as_deref()==Some(&slot)){a.alias=alias;a.enabled=true;self.selected=a.id.clone();return a.id.clone();}
        let id=format!("DeepSeek-{slot}");self.selected=id.clone();self.accounts.push(Account{id:id.clone(),provider:Provider::DeepSeek,alias,source:PathBuf::new(),slot:Some(slot),enabled:true,pinned:true});id
    }
    pub fn connect_commandcode(&mut self,slot:String,alias:String)->String{
        if let Some(a)=self.accounts.iter_mut().find(|a|a.provider==Provider::CommandCode&&a.slot.as_deref()==Some(&slot)){a.alias=alias;a.enabled=true;self.selected=a.id.clone();return a.id.clone();}
        let id=format!("CommandCode-{slot}");self.selected=id.clone();self.accounts.push(Account{id:id.clone(),provider:Provider::CommandCode,alias,source:PathBuf::new(),slot:Some(slot),enabled:true,pinned:true});id
    }
    pub fn reconnect_commandcode(&mut self,id:&str,slot:String,alias:String)->Result<String,String>{
        if self.accounts.iter().any(|a|a.id!=id&&a.provider==Provider::CommandCode&&a.slot.as_deref()==Some(&slot)){return Err("这把 API Key 已连接到另一个账号".into());}
        let account=self.accounts.iter_mut().find(|a|a.id==id&&a.provider==Provider::CommandCode).ok_or("原账号已移除，请重新添加")?;
        account.slot=Some(slot);account.alias=alias;self.selected=id.into();Ok(id.into())
    }
}
#[derive(Clone, Debug, PartialEq)]
#[derive(serde::Serialize,serde::Deserialize)]
pub struct Window {pub label:String,pub seconds:Option<i64>,pub remaining:f32,pub reset:Option<i64>}
#[derive(Clone, Debug, Default)]
#[derive(serde::Serialize,serde::Deserialize)]
pub struct Snapshot {pub plan:String,pub windows:Vec<Window>,pub extra:Vec<Window>,pub at:i64,pub wallet:Option<deepseek::Wallet>,pub credits:Option<workbuddy::Credits>}
impl Account {
    pub(crate) fn cache_identity_marker(&self)->Value {
        if claude::is_desktop(&self.source){json!(2)}else{json!(true)}
    }
    pub(crate) fn cached_snapshot(&self,cache:&Value)->Option<Snapshot>{
        let entry=&cache[&self.id];
        if entry["provider"].as_str()!=Some(self.provider.name()) || entry["slot"].as_str()!=self.slot.as_deref(){return None;}
        if self.provider==Provider::Claude && (entry["source"].as_str()!=self.source.to_str() || entry["local_identity_checked"]!=self.cache_identity_marker()){return None;}
        let snapshot:Snapshot=serde_json::from_value(entry["snapshot"].clone()).ok()?;
        (snapshot.at>0 && !snapshot.wallet.as_ref().is_some_and(|w|w.balances.is_empty())).then_some(snapshot)
    }
}
impl Snapshot {
    pub fn rings(&self)->Vec<&Window>{let mut all:Vec<_>=self.windows.iter().collect();all.sort_by_key(|w|w.seconds.unwrap_or(i64::MAX));all.truncate(2);all}
}
fn number(v:&Value)->Option<f64>{v.as_f64().or_else(||v.as_str()?.parse().ok()).filter(|n|n.is_finite())}
fn reset(v:&Value)->Option<i64>{v.as_i64().filter(|n|*n>0).or_else(||chrono::DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|d|d.timestamp()))}
fn cadence(seconds:Option<i64>)->String{match seconds{Some(18000)=>"5 小时".into(),Some(604800)=>"本周".into(),Some(86400)=>"今日".into(),Some(s)if s>0 && s%86400==0=>format!("{} 天",s/86400),Some(s)if s>0 && s%3600==0=>format!("{} 小时",s/3600),Some(s)if s>0=>format!("{} 分钟",s/60),_=>"额度周期".into()}}
fn codex_window(v:&Value)->Option<Window>{
    let used=number(&v["used_percent"])?;let seconds=v["limit_window_seconds"].as_i64().filter(|s|*s>0);
    Some(Window{label:cadence(seconds),seconds,remaining:(100.0-used).clamp(0.0,100.0) as f32,reset:reset(&v["reset_at"])})
}
pub fn parse(provider:Provider,v:&Value)->Result<Snapshot,String>{
    if provider==Provider::WorkBuddy{return workbuddy::parse(v);}
    if provider==Provider::DeepSeek{return deepseek::parse(v);}
    if provider==Provider::GrokBot{return grok::parse(v);}
    if provider==Provider::Gemini{return gemini::parse(v);}
    if provider==Provider::CommandCode{return commandcode::parse(v);}
    let mut s=Snapshot{plan:v["plan_type"].as_str().unwrap_or_default().into(),at:chrono::Utc::now().timestamp(),..Default::default()};
    match provider {
        Provider::Codex=>{
            let rate=&v["rate_limit"];if !rate.is_object(){return Err("服务没有返回额度信息".into());}
            for key in ["primary_window","secondary_window"]{if !rate[key].is_null(){let w=codex_window(&rate[key]).ok_or("额度字段不完整，暂时无法判断周期")?;s.windows.push(w);}}
            for a in v["additional_rate_limits"].as_array().into_iter().flatten(){
                for key in ["primary_window","secondary_window"]{if let Some(mut w)=codex_window(&a["rate_limit"][key]){let label=a["limit_name"].as_str().or_else(||a["limit_id"].as_str()).unwrap_or("额外额度");w.label=format!("{label} · {}",w.label);s.extra.push(w);}}
            }
        }
        Provider::Claude=>{
            for(key,seconds)in [("five_hour",18000),("seven_day",604800)]{if let Some(used)=number(&v[key]["utilization"]){s.windows.push(Window{label:cadence(Some(seconds)),seconds:Some(seconds),remaining:(100.0-used).clamp(0.0,100.0) as f32,reset:reset(&v[key]["resets_at"])});}}
        }
        Provider::Kimi=>{
            if let Some(usages)=v["usages"].as_object(){
                for (key,label,seconds) in [("limit_5h","5 小时",Some(18000)),("limit_7d","本周",Some(604800)),("limit_month_total","本月总额度",None),("limit_month_code","本月编程",None)]{
                    let Some(entry)=usages.get(key) else{continue;};
                    let Some(used)=number(&entry["used_ratio"]).filter(|n|(0.0..=1.0).contains(n)) else{continue;};
                    s.windows.push(Window{label:label.into(),seconds,remaining:((1.0-used)*100.0) as f32,reset:reset(&entry["reset_time"])});
                }
                if !s.windows.is_empty(){return Ok(s);}
            }
            if v.as_object().is_some_and(|o|o.is_empty()){return Err("Kimi 已登录，官网暂未返回周期额度".into());}
            let section=|v:&Value,seconds:Option<i64>|->Option<Window>{let limit=number(&v["limit"])?;if limit<=0.0{return None;}let remaining=number(&v["remaining"]).or_else(||Some(limit-number(&v["used"])?))?;Some(Window{label:cadence(seconds),seconds,remaining:(remaining/limit*100.0).clamp(0.0,100.0) as f32,reset:reset(&v["resetTime"])})};
            if let Some(w)=section(&v["usage"],Some(604800)){s.windows.push(w);}
            for limit in v["limits"].as_array().into_iter().flatten(){let duration=limit["window"]["duration"].as_i64();let unit=limit["window"]["timeUnit"].as_str().unwrap_or_default();let factor=match unit{"TIME_UNIT_MINUTE"=>Some(60),"TIME_UNIT_HOUR"=>Some(3600),"TIME_UNIT_DAY"=>Some(86400),"TIME_UNIT_SECOND"=>Some(1),_=>None};let seconds=duration.zip(factor).and_then(|(d,f)|d.checked_mul(f));if let Some(w)=section(&limit["detail"],seconds){if !s.windows.iter().any(|old|old.seconds==w.seconds){s.windows.push(w);}}}
        }
        Provider::ZCode|Provider::GrokBot|Provider::Gemini|Provider::WorkBuddy|Provider::DeepSeek|Provider::CommandCode=>unreachable!(),
    }
    if s.windows.is_empty(){return Err("服务未提供周期额度，暂不绘制进度".into());}
    s.windows.sort_by_key(|w|w.seconds.unwrap_or(i64::MAX));Ok(s)
}

struct Credential {token:String,account_id:Option<String>,identity:Option<String>,stamp:Option<std::time::SystemTime>}
fn same_login(a:&Credential,b:&Credential)->bool{a.token==b.token||a.identity.as_ref().zip(b.identity.as_ref()).is_some_and(|(a,b)|a==b)}
fn login_json(path:&Path)->Result<Value,String>{
    let raw=fs::read(path).map_err(|_|"无法读取登录文件".to_string())?;
    serde_json::from_slice(raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&raw)).map_err(|_|"登录文件不是有效的 JSON".into())
}
fn account_credentials(account:&Account)->Result<Credential,String>{
    credentials(account.provider,&account.source)
}
fn credentials(provider:Provider,path:&Path)->Result<Credential,String>{
    if provider==Provider::Claude{return Err("Claude 仅使用本地用量记录，不读取登录令牌".into());}
    let root=login_json(path)?;
    let v=if provider==Provider::Codex{root.get("auth_json").unwrap_or(&root)}else{&root};
    let token=match provider{Provider::Codex=>v.pointer("/tokens/access_token").or_else(||v.get("access_token")),Provider::Kimi=>v.get("access_token"),Provider::Claude|Provider::ZCode|Provider::GrokBot|Provider::Gemini|Provider::WorkBuddy|Provider::DeepSeek|Provider::CommandCode=>None}.and_then(Value::as_str).filter(|s|!s.trim().is_empty()).ok_or("未找到订阅账号的登录凭据，请选择对应工具的登录文件")?.to_owned();
    let claims=token.split('.').nth(1).and_then(|b|base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b).ok()).and_then(|b|serde_json::from_slice::<Value>(&b).ok()).unwrap_or(Value::Null);
    let account_id=v.pointer("/tokens/account_id").or_else(||v.get("account_id")).and_then(Value::as_str).or_else(||claims.pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id").and_then(Value::as_str)).filter(|s|!s.is_empty()).map(str::to_owned);
    let identity=match provider{
        Provider::Codex=>account_id.as_ref().zip(claims.pointer("/https:~1~1api.openai.com~1auth/chatgpt_user_id").or_else(||claims.get("sub")).and_then(Value::as_str).filter(|s|!s.is_empty())).map(|(account,user)|format!("{account}:{user}")),
        Provider::Kimi=>claims.get("user_id").or_else(||claims.get("sub")).and_then(Value::as_str).filter(|s|!s.is_empty()).map(str::to_owned),
        _=>None,
    };
    Ok(Credential{token,account_id,identity,stamp:fs::metadata(path).and_then(|m|m.modified()).ok()})
}
pub struct Event {pub id:String,pub result:Result<Snapshot,String>}
pub struct Service {
    tx:mpsc::SyncSender<Vec<Account>>,
    rx:mpsc::Receiver<(Account,Result<Snapshot,String>)>,
}
impl Service {
    pub fn start()->Self{
        let(tx,requests)=mpsc::sync_channel::<Vec<Account>>(1);
        let(results,rx)=mpsc::channel();
        std::thread::spawn(move||{
            let mut cooldowns:HashMap<_,(Instant,String)>=HashMap::new();
            while let Ok(accounts)=requests.recv(){
                let agent=http_agent();
                let mut groups=[Vec::new(),Vec::new(),Vec::new()];
                for a in accounts.into_iter().filter(|a|a.enabled){
                    let key=(a.id.clone(),a.slot.clone(),a.source.clone());
                    if let Some((until,message))=cooldowns.get(&key){
                        if *until>Instant::now(){let _=results.send((a,Err(message.clone())));continue;}
                    }
                    let group=match a.provider{Provider::DeepSeek=>1,Provider::WorkBuddy=>2,_=>0};
                    groups[group].push((key,a));
                }
                cooldowns.retain(|_,(until,_)|*until>Instant::now());
                // 同一浏览器会话内串行查询，两个余额服务各自独立完成。
                std::thread::scope(|scope|{
                    let mut workers=Vec::new();
                    for group in groups.into_iter().filter(|g|!g.is_empty()){
                        let agent=&agent;let results=&results;
                        workers.push(scope.spawn(move||{
                            let mut waits=Vec::new();
                            for (key,a) in group{
                                let result=fetch(agent,&a);
                                if let Err((message,wait))=&result{
                                    if *wait>0 {waits.push((key,(Instant::now()+Duration::from_secs(*wait),message.clone())));}
                                }
                                let _=results.send((a,result.map_err(|e|e.0)));
                            }
                            waits
                        }));
                    }
                    for worker in workers{if let Ok(waits)=worker.join(){cooldowns.extend(waits);}}
                });
            }
        });Self{tx,rx}
    }
    pub fn refresh(&self,accounts:Vec<Account>)->bool{self.tx.try_send(accounts).is_ok()}
    pub fn try_recv(&self,config:&Config)->Option<Event>{
        while let Ok((source,result))=self.rx.try_recv(){
            if config.enabled && config.accounts.iter().any(|a|a.enabled&&a.id==source.id&&a.provider==source.provider&&a.source==source.source&&a.slot==source.slot){
                return Some(Event{id:source.id,result});
            }
        }
        None
    }
}
fn http_agent()->ureq::Agent{
    let mut builder=ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).redirects(0);
    let proxy=["HTTPS_PROXY","https_proxy","ALL_PROXY","all_proxy","HTTP_PROXY","http_proxy"].iter().find_map(|key|std::env::var(key).ok().filter(|s|!s.is_empty())).or_else(system_proxy);
    if let Some(proxy)=proxy.and_then(|s|ureq::Proxy::new(s).ok()){builder=builder.proxy(proxy);}
    builder.build()
}
#[cfg(not(windows))] pub(crate) fn system_proxy()->Option<String>{None}
#[cfg(windows)] pub(crate) fn system_proxy()->Option<String>{
    use windows::{core::w,Win32::System::Registry::{RegGetValueW,HKEY_CURRENT_USER,RRF_RT_REG_DWORD,RRF_RT_REG_SZ}};
    let key=w!("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings");let mut enabled=0u32;let mut size=4u32;
    unsafe{if RegGetValueW(HKEY_CURRENT_USER,key,w!("ProxyEnable"),RRF_RT_REG_DWORD,None,Some((&mut enabled as *mut u32).cast()),Some(&mut size)).is_err() || enabled==0{return None;}}
    let mut buffer=[0u16;1024];size=(buffer.len()*2) as u32;
    unsafe{if RegGetValueW(HKEY_CURRENT_USER,key,w!("ProxyServer"),RRF_RT_REG_SZ,None,Some(buffer.as_mut_ptr().cast()),Some(&mut size)).is_err(){return None;}}
    let text=String::from_utf16_lossy(&buffer[..buffer.iter().position(|v|*v==0).unwrap_or(buffer.len())]);
    let server=if text.contains('='){text.split(';').find_map(|s|s.trim().strip_prefix("https=")).or_else(||text.split(';').find_map(|s|s.trim().strip_prefix("http=")))?}else{text.trim()};
    if server.is_empty(){None}else if server.contains("://"){Some(server.into())}else{Some(format!("http://{server}"))}
}
fn fetch(agent:&ureq::Agent,account:&Account)->Result<Snapshot,(String,u64)>{
    if account.provider==Provider::Claude{return claude::fetch(account).map_err(|e|(e,0));}
    if account.provider==Provider::ZCode{return zcode::fetch(agent,account).map_err(|e|(e,60));}
    if account.provider==Provider::WorkBuddy{return workbuddy::fetch(agent,account);}
    if account.provider==Provider::DeepSeek{return deepseek::fetch(agent,account);}
    if account.provider==Provider::GrokBot{return grok::fetch(agent,account);}
    if account.provider==Provider::Gemini{return gemini::fetch(agent,account);}
    if account.provider==Provider::CommandCode{return commandcode::fetch(agent,account);}
    let auth=account_credentials(account).map_err(|e|(e,0))?;
    let url=match account.provider{Provider::Codex=>"https://chatgpt.com/backend-api/wham/usage",Provider::Kimi=>"https://api.kimi.com/coding/v1/usages",Provider::Claude|Provider::ZCode|Provider::GrokBot|Provider::Gemini|Provider::WorkBuddy|Provider::DeepSeek|Provider::CommandCode=>unreachable!()};
    let mut req=agent.get(url).set("Authorization",&format!("Bearer {}",auth.token)).set("Accept","application/json").set("User-Agent","codex-cli/Daozai");
    if account.provider==Provider::Codex{if let Some(id)=&auth.account_id{req=req.set("ChatGPT-Account-Id",id);}}
    let response=req.call().map_err(|e|match e{ureq::Error::Status(401|403,_)=>("登录已失效或权限不足，请在对应工具中重新登录".into(),0),ureq::Error::Status(429,r)=>("请求过于频繁，稍后自动重试".into(),r.header("Retry-After").and_then(|s|s.parse().ok()).unwrap_or(180).clamp(30,3600)),ureq::Error::Status(n,_)=>(format!("额度服务暂不可用（{n}）"),60),_=>("无法连接额度服务，请检查网络或代理".into(),30)})?;
    let mut body=String::new();response.into_reader().take(1024*1024).read_to_string(&mut body).map_err(|_|("读取额度响应失败".into(),30))?;
    if auth.stamp!=fs::metadata(&account.source).and_then(|m|m.modified()).ok(){return Err(("登录信息已变化，请刷新额度".into(),0));}
    let value=serde_json::from_str(&body).map_err(|_|("服务没有返回有效的额度数据".into(),60))?;
    parse(account.provider,&value).map_err(|e|(e,60))
}

#[cfg(test)] mod tests {
    use super::*;
    #[test]
    fn claude_local_config_ignores_retired_reset_fields(){
        let current=json!({"enabled":true,"compact":false,"selected":"desktop","colors":{"desktop":"blue"},"accounts":[{"id":"desktop","provider":"Claude","alias":"离线账号","source":"fixture/config.json","slot":"first","enabled":true,"pinned":true}]});
        let mut legacy=current.clone();legacy["manual_resets"]=json!({"desktop":{"source":"fixture/config.json","slot":"first","five_hour":1000,"seven_day":2000}});
        let config=Config::from_json(&legacy);
        assert_eq!(config,Config::from_json(&current),"旧的手动时间不应影响账号及其他设置");
        assert!(config.json().get("manual_resets").is_none(),"保存配置不应继续保留已撤回的手动时间");
        assert_eq!(Config::from_json(&config.json()),config);
    }
    #[test] #[ignore="连接本机 ZCode 并只读核验套餐"]
    fn zcode_connect_local(){
        let mut config=Config::load();let path=zcode::path().unwrap();
        zcode::add(&mut config,&path,"").unwrap();config.save();
        let a=config.accounts.iter().find(|a|a.provider==Provider::ZCode).unwrap();
        match zcode::fetch(&http_agent(),a){Ok(s)=>println!("ZCode 返回 {} 个周期",s.windows.len()),Err(msg)=>println!("ZCode 查询状态：{msg}")}
    }
    #[test]
    #[ignore="只读验证本机开启的 AI 账号"]
    fn enabled_account_probe(){
        let config=Config::load();
        let agent=http_agent();
        for a in config.accounts.iter().filter(|a|a.enabled){
            match fetch(&agent,a){
                Ok(s)=>println!("{} 查询成功，{} 个周期",a.provider.name(),s.windows.len()),
                Err((reason,_))=>println!("{} 查询失败：{}",a.provider.name(),reason),
            }
        }
    }
    #[test]
    fn native_discovery_keeps_distinct_accounts_and_does_not_change_their_files(){
        let dir=std::env::temp_dir().join(format!("daozai-discovery-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&dir).unwrap();
        let personal=dir.join("personal.json");let work=dir.join("work.json");let duplicate=dir.join("duplicate.json");
        let a=r#"{"access_token":"synthetic-personal"}"#;let b=r#"{"access_token":"synthetic-work"}"#;
        fs::write(&personal,a).unwrap();fs::write(&work,b).unwrap();fs::write(&duplicate,a).unwrap();
        let mut config=Config::default();let paths=vec![(Provider::Kimi,personal.clone()),(Provider::Kimi,work.clone()),(Provider::Kimi,duplicate)];
        config.discover_files(&paths);
        assert_eq!(config.accounts.len(),2,"同一服务的不同有效账号都应被识别，相同账号只添加一次");
        config.accounts[0].enabled=false;config.accounts[0].alias="自定义备注".into();let saved=config.clone();
        config.discover_files(&paths);assert_eq!(config,saved,"重复识别不能覆盖原有账号偏好");
        assert_eq!(fs::read_to_string(personal).unwrap(),a);assert_eq!(fs::read_to_string(work).unwrap(),b);
        let _=fs::remove_dir_all(dir);
    }
    #[test]
    fn migrated_credentials_do_not_duplicate_accounts_or_merge_different_workspace_members(){
        let dir=std::env::temp_dir().join(format!("daozai-identity-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir_all(&dir).unwrap();
        let token=|claims:Value|format!("e30.{}.synthetic",base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string()));
        let old=dir.join("old.json");let new=dir.join("new.json");
        fs::write(&old,json!({"access_token":token(json!({"user_id":"same-user","jti":"old"}))}).to_string()).unwrap();
        fs::File::options().write(true).open(&old).unwrap().set_times(fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH+Duration::from_secs(100))).unwrap();
        fs::write(&new,format!("\u{feff}{}",json!({"access_token":token(json!({"user_id":"same-user","jti":"new"}))}))).unwrap();
        let mut config=Config::default();config.add(Provider::Kimi,"保留备注".into(),old.clone()).unwrap();config.accounts[0].enabled=false;
        config.discover_files(&[(Provider::Kimi,old),(Provider::Kimi,new.clone())]);
        assert_eq!(config.accounts.len(),1);assert_eq!(config.accounts[0].alias,"保留备注");assert!(!config.accounts[0].enabled);assert_eq!(config.accounts[0].source,fs::canonicalize(&new).unwrap());
        for user in ["first-user","second-user"]{
            let path=dir.join(format!("{user}.json"));fs::write(&path,json!({"tokens":{"account_id":"same-workspace","access_token":token(json!({"sub":user}))}}).to_string()).unwrap();
            config.add(Provider::Codex,String::new(),path).unwrap();
        }
        assert_eq!(config.accounts.iter().filter(|a|a.provider==Provider::Codex).count(),2,"同一工作区的不同用户不能合并");
        let _=fs::remove_dir_all(dir);
    }
    #[test]
    #[ignore="按需只读核验本机识别与 Claude 本地额度，不保存配置或续期凭据"]
    fn native_account_read_only_probe(){
        let mut config=Config::load();let before=config.accounts.len();let report=config.discover();
        println!("discovery_added={} notes={:?}",report.added,report.notes);
        for provider in Provider::ALL{println!("provider={} accounts={}",provider.name(),config.accounts.iter().filter(|a|a.provider==provider).count());}
        let agent=http_agent();
        for account in config.accounts.iter().filter(|a|a.provider==Provider::Claude&&a.slot.is_some()){
            let snapshot=fetch(&agent,account).expect("Claude 桌面版本地额度应可只读获取");
            println!("claude_windows={:?}",snapshot.windows.iter().map(|w|(&w.label,w.remaining)).collect::<Vec<_>>());
        }
        for account in config.accounts.iter().skip(before){
            println!("discovered_account={}",json!({"id":account.id,"provider":account.provider.name(),"alias":account.alias,"source":account.source,"slot":account.slot,"enabled":account.enabled,"pinned":account.pinned}));
        }
        let saved=config.clone();assert_eq!(config.discover().added,0);assert_eq!(config,saved,"重复识别不能改变账号和偏好");
    }
    #[test] fn gemini_accounts_keep_colors_selection_and_credentials_separate(){
        let mut c=Config::default();let personal=c.connect_gemini("personal".into(),"个人".into());let work=c.connect_gemini("work".into(),"工作".into());c.colors.insert(personal.clone(),Accent::Cyan);c.colors.insert(work.clone(),Accent::Gold);c.accounts[0].pinned=false;
        c.connect_gemini("personal".into(),"个人更新".into());assert_eq!(c.accounts.len(),2);assert_eq!(c.selected,personal);assert!(!c.accounts[0].pinned);assert_eq!(c.colors[&work],Accent::Gold);assert_eq!(Config::from_json(&c.json()),c);let raw=c.json().to_string();assert!(!raw.contains("cookie"));assert!(!raw.contains("access_token"));assert!(c.add(Provider::Gemini,"错误来源".into(),"oauth_creds.json".into()).is_err());
    }
    #[test] fn grok_slots_share_a_file_but_keep_selection_colors_and_reconnection_separate(){
        let mut config=Config::from_json(&json!({"selected":"work","colors":{"personal":"green","work":"blue"},"accounts":[
            {"id":"personal","provider":"Grok Bot","alias":"个人","source":"sand-secrets.json","slot":"personal","pinned":true},
            {"id":"work","provider":"GrokBot","alias":"工作","source":"sand-secrets.json","slot":"work","pinned":false}
        ]}));
        assert_eq!(Config::from_json(&config.json()),config);let personal=config.accounts[0].clone();
        config.connect_grok("work".into(),"工作".into());
        assert_eq!(config.accounts.len(),2);assert_eq!(config.accounts[0],personal);assert_eq!(config.selected,"work");
        assert!(config.accounts[1].source.as_os_str().is_empty());assert!(!config.accounts[1].pinned);assert_eq!(config.colors["work"],Accent::Blue);
        let raw=config.json().to_string();assert!(!raw.contains("accessToken"));assert!(!raw.contains("refreshToken"));
    }
    #[test] fn old_or_unknown_colors_keep_accounts_and_use_brand_defaults(){
        let old=json!({"selected":"personal","accounts":[{"id":"personal","provider":"Codex","alias":"个人","source":"auth.json"}]});
        let old_config=Config::from_json(&old);assert_eq!(old_config.accounts.len(),1);assert!(old_config.colors.is_empty());
        for value in [json!("unknown"),json!(123),Value::Null]{let mut raw=old.clone();raw["colors"]=json!({"personal":value});let restored=Config::from_json(&raw);assert_eq!(restored.accounts,old_config.accounts);assert!(restored.colors.is_empty());}
    }
    #[test] fn colors_are_saved_per_account_without_changing_credentials_or_selection(){
        let mut c=Config::from_json(&json!({"selected":"personal","accounts":[{"id":"personal","provider":"Codex","alias":"个人","source":"personal/auth.json"},{"id":"work","provider":"Codex","alias":"工作","source":"work/auth.json"}]}));
        let accounts=c.accounts.clone();c.colors.insert("personal".into(),Accent::Violet);c.colors.insert("work".into(),Accent::Orange);
        let restored=Config::from_json(&c.json());assert_eq!(restored,c);assert_eq!(restored.accounts,accounts);assert_eq!(restored.selected,"personal");
        c.colors.remove("personal");let restored=Config::from_json(&c.json());assert!(!restored.colors.contains_key("personal"));assert_eq!(restored.colors["work"],Accent::Orange);
    }
    #[test] fn deepseek_reconnect_preserves_account_preferences(){
        let mut c=Config::default();let id=c.connect_deepseek("old".into(),"工作".into());c.accounts[0].pinned=false;c.accounts[0].enabled=false;c.colors.insert(id.clone(),Accent::Gold);
        assert_eq!(c.reconnect_deepseek(&id,"new".into(),"工作".into()).unwrap(),id);assert_eq!(c.accounts.len(),1);assert_eq!(c.accounts[0].slot.as_deref(),Some("new"));assert!(!c.accounts[0].pinned);assert!(!c.accounts[0].enabled);assert_eq!(c.colors[&id],Accent::Gold);
        c.connect_deepseek("other".into(),"个人".into());assert!(c.reconnect_deepseek(&id,"other".into(),"工作".into()).is_err());assert_eq!(c.accounts[0].slot.as_deref(),Some("new"));
    }
    #[test] fn deepseek_stays_last_and_visible_with_overflow(){
        let mut c=Config::default();c.connect_deepseek("key-reference".into(),"余额".into());
        for n in 0..5{c.accounts.push(Account{id:format!("c{n}"),provider:Provider::Codex,alias:String::new(),source:PathBuf::new(),slot:None,enabled:true,pinned:true});}
        assert_eq!(c.pinned_indices(),vec![1,2,3,4,5,0]);
        c.accounts[0].enabled=false;assert_eq!(c.pinned_indices(),vec![1,2,3,4,5]);
        assert!(!c.json().to_string().contains("api_key"));
    }
    #[test] fn codex_week_only_is_a_single_real_window(){let s=parse(Provider::Codex,&json!({"plan_type":"pro","rate_limit":{"primary_window":null,"secondary_window":{"used_percent":43,"limit_window_seconds":604800,"reset_at":1900000000}}})).unwrap();assert_eq!(s.rings().len(),1);assert_eq!(s.rings()[0].label,"本周");assert_eq!(s.rings()[0].remaining,57.0);}
    #[test] fn pro_plan_does_not_hide_an_existing_five_hour_window(){let s=parse(Provider::Codex,&json!({"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0,"limit_window_seconds":18000},"secondary_window":{"used_percent":25,"limit_window_seconds":604800}}})).unwrap();assert_eq!(s.rings().len(),2);assert_eq!(s.rings()[0].remaining,100.0);}
    #[test] fn primary_can_be_weekly_and_missing_is_not_unlimited(){let s=parse(Provider::Codex,&json!({"rate_limit":{"primary_window":{"used_percent":100,"limit_window_seconds":604800}}})).unwrap();assert_eq!(s.rings()[0].label,"本周");assert_eq!(s.rings()[0].remaining,0.0);assert!(parse(Provider::Codex,&json!({"rate_limit":{}})).is_err());assert!(parse(Provider::Codex,&json!({"rate_limit":{"primary_window":{"reset_at":1900000000}}})).is_err());}
    #[test] fn extra_codex_buckets_do_not_replace_main_windows(){let s=parse(Provider::Codex,&json!({"rate_limit":{"secondary_window":{"used_percent":20,"limit_window_seconds":604800}},"additional_rate_limits":[{"limit_name":"reserve","rate_limit":{"primary_window":{"used_percent":80,"limit_window_seconds":604800}}}]})).unwrap();assert_eq!(s.windows.len(),1);assert_eq!(s.extra[0].remaining,20.0);assert_eq!(s.windows[0].remaining,80.0);}
    #[test] fn kimi_current_usage_and_empty_account(){
        let s=parse(Provider::Kimi,&json!({"usages":{"limit_5h":{"used_ratio":"0.25"},"limit_month_code":{"used_ratio":0}}})).unwrap();
        assert_eq!(s.windows[0].remaining,75.0);assert_eq!(s.windows[1].remaining,100.0);
        assert!(parse(Provider::Kimi,&json!({})).unwrap_err().contains("已登录"));
    }
    #[test] fn claude_null_window_and_kimi_string_numbers(){let c=parse(Provider::Claude,&json!({"five_hour":null,"seven_day":{"utilization":60,"resets_at":"2030-01-01T12:00:00Z"}})).unwrap();assert_eq!(c.rings().len(),1);assert!(c.windows[0].reset.is_some());let k=parse(Provider::Kimi,&json!({"usage":{"limit":"100","remaining":"70"},"limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},"detail":{"limit":"20","used":"5"}}]})).unwrap();assert_eq!(k.rings().len(),2);assert_eq!(k.rings()[0].remaining,75.0);}
    #[test] fn multi_account_config_keeps_sources_and_selection_separate(){let mut c=Config::default();for n in 1..=2{c.accounts.push(Account{id:n.to_string(),provider:Provider::Codex,alias:format!("账号 {n}"),source:PathBuf::from(format!("profile-{n}/auth.json")),slot:None,enabled:true,pinned:true});}c.selected="2".into();let raw=c.json();assert_eq!(Config::from_json(&raw),c);assert_eq!(c.selected_index(),Some(1));assert_eq!(c.pinned_indices(),vec![0,1]);assert!(!raw.to_string().contains("access_token"));}
}
