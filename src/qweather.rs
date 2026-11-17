//! 和风天气：凭据保存在系统凭据库，所有出站请求先持久化计数。
use crate::weather::{Day,Position,Snapshot,SunTimes};
use serde::{Deserialize,Serialize};
use serde_json::Value;
use std::{path::Path,sync::{Arc,Mutex,atomic::{AtomicBool,Ordering},mpsc::{self,Receiver}}};
#[cfg(not(test))]use std::{io::Read,time::Duration};
use chrono::{Datelike,TimeZone};

pub(crate) const LIMIT:u32=5000;
pub(crate) const CURRENT_INTERVAL:i64=3600;
pub(crate) const DAILY_INTERVAL:i64=21600;

#[derive(Clone,Copy,Default,PartialEq,Eq,Serialize,Deserialize)]
pub(crate) enum Provider {#[default] OpenMeteo,Qweather}
impl Provider {
    pub fn name(self)->&'static str {match self {Self::OpenMeteo=>"Open-Meteo",Self::Qweather=>"和风天气"}}
    pub fn url(self)->&'static str {match self {Self::OpenMeteo=>"https://open-meteo.com/",Self::Qweather=>"https://www.qweather.com/"}}
}

#[derive(Clone,PartialEq,Serialize,Deserialize)]
pub(crate) struct Location {pub position:Position,pub timezone:String}
impl Location {pub fn valid(&self)->bool {self.position.valid()&&self.timezone.parse::<chrono_tz::Tz>().is_ok()}}
pub(crate) struct Search {pub locations:Vec<Location>,pub credits:Vec<String>}

#[derive(Clone,Default,Serialize,Deserialize)]
pub(crate) struct Config {pub provider:Provider,pub host:String,#[serde(default)]pub credential:String,pub location:Option<Location>,pub saved:Vec<Location>}
impl Config {
    pub fn load()->Self {std::fs::read(crate::model::config_dir().join("weather-provider.json")).ok().and_then(|b|serde_json::from_slice::<Self>(&b).ok()).map(|mut c|{c.saved.retain(Location::valid);c.saved.truncate(crate::weather::MAX_CITIES);if !c.location.as_ref().is_some_and(Location::valid){c.location=None;}c}).unwrap_or_default()}
    pub fn save(&self)->Result<(),Error> {let dir=crate::model::config_dir();std::fs::create_dir_all(&dir).map_err(|_|Error::Storage)?;crate::model::atomic_write(&dir.join("weather-provider.json"),&serde_json::to_vec(self).map_err(|_|Error::Storage)?).map_err(|_|Error::Storage)}
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Error {Invalid,Auth,Rate,Limit,Network,Response,Storage,Disabled}
impl Error {
    pub fn text(self)->&'static str {match self {
        Self::Invalid=>"请填写有效的官方 API Host 和 API Key。",Self::Auth=>"凭据被拒绝，请检查 Host、Key 和两类 API 权限。",
        Self::Rate=>"和风请求过于频繁，已暂停自动重试，请稍后手动刷新。",Self::Limit=>"本机本月已达 5,000 次上限，已停止和风请求；可切回 Open-Meteo。",
        Self::Network=>"无法连接和风天气，请检查网络后重试。",Self::Response=>"和风返回的数据无效或暂不可用。",Self::Storage=>"无法安全保存配置或请求计数，已停止请求。",Self::Disabled=>"天气模块已关闭，请先在页面模块中开启。",
    }}
    fn pauses(self)->bool {matches!(self,Self::Auth|Self::Rate|Self::Limit|Self::Storage)}
}

pub(crate) fn normalize_host(input:&str)->Result<String,&'static str> {
    let input=input.trim().to_ascii_lowercase();
    if input.is_empty(){return Err("请填写控制台分配的 API Host。");}
    let host=input.strip_prefix("https://").unwrap_or(&input).to_string();
    if host.len()>253 || !host.ends_with(".qweatherapi.com") || !host.split('.').all(|label|{
        !label.is_empty() && label.len()<=63 && label.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
            && label.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
            && label.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'-')
    }) {return Err("请填写官方 qweatherapi.com 专属域名，不含路径、端口或其他参数。");}
    Ok(host)
}

#[derive(Default,Serialize,Deserialize)]
struct Usage {month:String,attempts:u32}
static USAGE_LOCK:Mutex<()>=Mutex::new(());
fn month()->String {chrono::Local::now().format("%Y-%m").to_string()}
fn reserve(path:&Path,month:&str)->Result<(),Error> {
    let _guard=USAGE_LOCK.lock().map_err(|_|Error::Storage)?;
    if let Some(parent)=path.parent(){std::fs::create_dir_all(parent).map_err(|_|Error::Storage)?;}
    let mut usage=match std::fs::read(path) {Ok(b)=>serde_json::from_slice::<Usage>(&b).map_err(|_|Error::Storage)?,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Usage::default(),Err(_)=>return Err(Error::Storage)};
    if usage.month!=month {usage=Usage{month:month.into(),attempts:0};}
    if usage.attempts>=LIMIT {return Err(Error::Limit);}
    usage.attempts+=1;
    crate::model::atomic_write(path,&serde_json::to_vec(&usage).map_err(|_|Error::Storage)?).map_err(|_|Error::Storage)
}
pub fn usage()->u32 {let _g=USAGE_LOCK.lock().unwrap_or_else(|e|e.into_inner());std::fs::read(crate::model::config_dir().join("qweather-usage.json")).ok().and_then(|b|serde_json::from_slice::<Usage>(&b).ok()).filter(|u|u.month==month()).map_or(0,|u|u.attempts)}

/// 不派生 Debug；密钥不进入错误、URL、普通配置或天气缓存。
#[derive(Clone)]
pub(crate) struct Client {
    pub host:String,key:String,pub gate:Arc<AtomicBool>,
    #[cfg(test)]mock:Option<Arc<dyn Fn(&url::Url,&str)->Result<Value,Error>+Send+Sync>>,
    #[cfg(test)]budget:Option<std::path::PathBuf>,
}
impl Client {
    pub fn new(host:&str,key:&str,gate:Arc<AtomicBool>)->Result<Self,Error> {
        let host=normalize_host(host).map_err(|_|Error::Invalid)?;
        if key.trim().is_empty()||key.bytes().any(|c|c<=32||c>=127){return Err(Error::Invalid);}
        Ok(Self{host,key:key.trim().into(),gate,#[cfg(test)]mock:None,#[cfg(test)]budget:None})
    }
    fn request(&self,path:&str,params:&[(&str,&str)])->Result<Value,Error> {
        if normalize_host(&self.host).map_err(|_|Error::Invalid)?!=self.host{return Err(Error::Invalid);}
        if !self.gate.load(Ordering::SeqCst){return Err(Error::Disabled);}
        let mut url=url::Url::parse(&format!("https://{}{path}",self.host)).map_err(|_|Error::Invalid)?;
        url.query_pairs_mut().extend_pairs(params.iter().copied());
        let budget=crate::model::config_dir().join("qweather-usage.json");
        #[cfg(test)]let budget=self.budget.as_ref().unwrap_or(&budget);
        reserve(&budget,&month())?;
        if !self.gate.load(Ordering::SeqCst){return Err(Error::Disabled);}
        #[cfg(test)]{return self.mock.as_ref().ok_or(Error::Network)?(&url,&self.key);}
        #[cfg(not(test))]
        {
        // 禁止重定向及隐式网络重试；每次调用仅对应一次已计数的出站尝试。
        let response=ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(18)).build().get(url.as_str())
            .set("X-QW-Api-Key",&self.key).set("Accept","application/json").call().map_err(|e|match e {
                ureq::Error::Status(401|403,_)=>Error::Auth,ureq::Error::Status(429,_)=>Error::Rate,
                ureq::Error::Status(_,_)=>Error::Response,_=>Error::Network,
            })?;
        if response.status()!=200 {return Err(Error::Response);}
        let value:Value=serde_json::from_reader(response.into_reader().take(1024*1024)).map_err(|_|Error::Response)?;
        match value["code"].as_str(){Some("401"|"402"|"403")=>Err(Error::Auth),Some("429")=>Err(Error::Rate),Some("404")=>Ok(value),Some(c)if c!="200"=>Err(Error::Response),_=>Ok(value)}
        }
    }
    pub fn search(&self,query:&str,adm:&str)->Result<Search,Error> {
        let v=self.request("/geo/v2/city/lookup",&[("location",query),("adm",adm),("number","20"),("lang","zh"),("range","cn")])?;
        let locations=v["location"].as_array().into_iter().flatten().filter_map(|v|{
            let location=Location{position:Position{label:v["name"].as_str()?.into(),lat:v["lat"].as_str()?.parse().ok()?,lon:v["lon"].as_str()?.parse().ok()?,region:["adm1","adm2"].iter().filter_map(|k|v[*k].as_str()).collect::<Vec<_>>().join(" · ")},timezone:v["tz"].as_str()?.into()};
            location.valid().then_some(location)
        }).collect();
        let credits=["sources","license"].iter().flat_map(|k|v["refer"][*k].as_array().into_iter().flatten()).filter_map(Value::as_str).map(str::to_string).collect();
        Ok(Search{locations,credits})
    }
    pub fn weather(&self,location:&Location,daily:bool)->Result<Value,Error> {
        let p=&location.position;
        let mut params=vec![("localTime","true"),("lang","zh")];if daily{params.push(("days","7"));}
        self.request(&format!("/weather/v1/{}/{:.2}/{:.2}",if daily{"daily"}else{"current"},p.lat,p.lon),&params)
    }
    pub fn test(&self)->Result<(),Error> {
        let location=self.search("北京","")?.locations.into_iter().next().ok_or(Error::Response)?;
        parse_current(&self.weather(&location,false)?,&location,chrono::Utc::now().timestamp())?;
        parse_daily(&self.weather(&location,true)?,&location)?;Ok(())
    }
    #[cfg(all(windows,not(test)))]
    pub fn store(&self,slot:&str)->Result<(),Error> {keyring::Entry::new("Daozai.Weather.QWeather",slot).map_err(|_|Error::Storage)?.set_password(&self.key).map_err(|_|Error::Storage)}
    #[cfg(any(not(windows),test))]
    pub fn store(&self,_slot:&str)->Result<(),Error> {if cfg!(test){Ok(())}else{Err(Error::Storage)}}
    pub fn load(host:&str,slot:&str,gate:Arc<AtomicBool>)->Option<Self> {
        #[cfg(all(windows,not(test)))]
        {if slot.is_empty(){return None;}let key=keyring::Entry::new("Daozai.Weather.QWeather",slot).ok()?.get_password().ok()?;Self::new(host,&key,gate).ok()}
        #[cfg(any(not(windows),test))]
        {let _=(host,slot,gate);None}
    }
    pub fn delete(host:&str)->Result<(),Error> {
        #[cfg(all(windows,not(test)))]
        {let entry=keyring::Entry::new("Daozai.Weather.QWeather",host).map_err(|_|Error::Storage)?;match entry.delete_credential(){Ok(())|Err(keyring::Error::NoEntry)=>Ok(()),Err(_)=>Err(Error::Storage)}}
        #[cfg(any(not(windows),test))]
        {let _=host;Ok(())}
    }
}

pub(crate) fn spawn<T:Send+'static>(work:impl FnOnce()->Result<T,Error>+Send+'static)->Receiver<Result<T,Error>> {let(tx,rx)=mpsc::channel();std::thread::spawn(move||{let _=tx.send(work());});rx}

#[derive(Clone,Default,Serialize,Deserialize)]
struct Cache {host:String,location:Option<Location>,current:Option<Value>,daily:Option<Value>,current_at:i64,daily_at:i64}
#[derive(Default)]
pub(crate) struct Runtime {
    cache:Cache,current:Option<Receiver<Result<Value,Error>>>,daily:Option<Receiver<Result<Value,Error>>>,
    last_current:i64,last_daily:i64,pub error:Option<Error>,errors:[Option<Error>;2],paused:bool,current_failures:u32,
    snapshot_second:Option<i64>,snapshot_date:Option<chrono::NaiveDate>,limit_check_minute:Option<i64>,
}
impl Runtime {
    pub fn reset(&mut self,host:&str,location:Option<Location>){
        let cache=std::fs::read(crate::model::config_dir().join("qweather-cache.json")).ok().and_then(|b|serde_json::from_slice::<Cache>(&b).ok()).filter(|c|c.host==host&&c.location==location).unwrap_or(Cache{host:host.into(),location,..Default::default()});
        *self=Self{cache,..Default::default()};
        self.day_changed(chrono::Utc::now().timestamp());
    }
    fn day_changed(&mut self,now:i64)->bool {
        if self.snapshot_second==Some(now){return false;}
        self.snapshot_second=Some(now);
        let date=self.cache.location.as_ref().and_then(|l|l.timezone.parse::<chrono_tz::Tz>().ok())
            .and_then(|zone|zone.timestamp_opt(now,0).single()).map(|t|t.date_naive());
        let changed=self.snapshot_date!=date;
        self.snapshot_date=date;
        changed
    }
    pub fn loading(&self)->bool {self.current.is_some()||self.daily.is_some()}
    /// 连续实况失败，或凭据、额度等错误已暂停刷新。
    pub fn current_refresh_failed(&self)->bool {self.current_failures>=2||(self.paused&&self.error.is_some())}
    pub fn can_refresh(&self,now:i64)->bool {!self.loading()&&now-self.last_current.max(self.last_daily)>=30}
    pub fn refresh(&mut self,client:&Client,now:i64,manual:bool){
        if !client.gate.load(Ordering::SeqCst){self.current=None;self.daily=None;return;}
        if manual&&!self.can_refresh(now){return;}
        if manual {self.paused=false;self.error=None;self.errors=[None,None];}
        if self.paused {
            if self.error!=Some(Error::Limit)||self.limit_check_minute==Some(now/60){return;}
            self.limit_check_minute=Some(now/60);
            if usage()>=LIMIT{return;}
            self.paused=false;
        }
        let Some(location)=self.cache.location.as_ref() else{return;};
        let current_due=manual||now-self.cache.current_at>=CURRENT_INTERVAL;
        let daily_due=manual||self.cache.daily.is_none()||now-self.cache.daily_at>=DAILY_INTERVAL;
        let minimum=if manual{30}else{120};
        if current_due&&self.current.is_none()&&now-self.last_current>=minimum {self.last_current=now;let c=client.clone();let l=location.clone();self.current=Some(spawn(move||c.weather(&l,false)));}
        if daily_due&&self.daily.is_none()&&now-self.last_daily>=minimum {self.last_daily=now;let c=client.clone();let l=location.clone();self.daily=Some(spawn(move||c.weather(&l,true)));}
    }
    pub fn poll(&mut self,enabled:bool,now:i64)->bool {
        if !enabled {self.current=None;self.daily=None;return false;}
        let mut changed=false;
        for daily in [false,true]{
            let slot=if daily{&mut self.daily}else{&mut self.current};
            let Some(result)=slot.as_ref().and_then(|rx|rx.try_recv().ok())else{continue;};*slot=None;
            let checked=result.and_then(|v|{let l=self.cache.location.as_ref().ok_or(Error::Response)?;if daily{parse_daily(&v,l)?;}else{parse_current(&v,l,now)?;}Ok(v)});
            match checked {
                Ok(value)=>{self.errors[daily as usize]=None;if daily{self.cache.daily=Some(value);self.cache.daily_at=now;}else{self.cache.current=Some(value);self.cache.current_at=now;self.current_failures=0;}changed=true;},
                Err(e)=>{self.errors[daily as usize]=Some(e);self.paused|=e.pauses();if !daily{self.current_failures=self.current_failures.saturating_add(1);}},
            }
        }
        self.error=self.errors.iter().flatten().copied().next();
        if changed {let _=crate::model::atomic_write(&crate::model::config_dir().join("qweather-cache.json"),&serde_json::to_vec(&self.cache).unwrap_or_default());}
        self.day_changed(now)||changed
    }
    pub fn snapshot(&self)->Option<Snapshot> {
        let l=self.cache.location.as_ref()?;let mut s=parse_current(self.cache.current.as_ref()?,l,self.cache.current_at).ok()?;
        if let Some(daily)=&self.cache.daily {if let Ok((days,sun))=parse_daily(daily,l){s.days=days;s.sun_times=sun;s.attributions.extend(attributions(daily));}}
        if let Some(today)=s.days.first(){s.high=Some(today.high);s.low=Some(today.low);}
        if let Some(sun)=&s.sun_times {s.sunrise=Some(sun.clock(sun.sunrise));s.sunset=Some(sun.clock(sun.sunset));let now=chrono::Utc::now().timestamp();s.day=now>=sun.sunrise&&now<sun.sunset;}
        s.attributions.sort();s.attributions.dedup();Some(s)
    }
    pub fn stamps(&self)->(i64,i64){(self.cache.current_at,self.cache.daily_at)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn directory()->std::path::PathBuf {let p=std::env::temp_dir().join(format!("daozai-qweather-{}-{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap()));std::fs::create_dir_all(&p).unwrap();p}
    fn location()->Location {Location{position:Position{lat:30.27,lon:120.15,label:"杭州".into(),region:"浙江省 · 杭州市".into()},timezone:"Asia/Shanghai".into()}}
    fn current()->Value {json!({"validTime":chrono::Utc::now().to_rfc3339(),"temperature":{"value":21.5,"unit":"°C"},"feelsLike":{"value":293.15,"unit":"K"},"condition":{"code":"305","text":"小雨"},"humidity":0.68,"wind":{"speed":{"value":4.5,"unit":"m/s"}},"metadata":{"attributions":["https://example.invalid/credit"]}})}
    fn daily()->Value {let d=chrono::Utc::now().with_timezone(&chrono_tz::Asia::Shanghai).date_naive();json!({"days":[{"forecastStartTime":format!("{d}T00:00:00+08:00"),"temperatureMax":{"value":25,"unit":"°C"},"temperatureMin":{"value":15,"unit":"°C"},"astro":{"sunrise":format!("{d}T06:10:00+08:00"),"sunset":format!("{d}T17:20:00+08:00")},"daytime":{"condition":{"code":"305"},"precipitation":{"probability":0.25}},"nighttime":{"precipitation":{"probability":0.8}}}]})}
    #[test]
    fn host_rejects_non_official_addresses_and_url_extras(){
        for s in ["","qweatherapi.com","qweatherapi.com.evil.com","evilqweatherapi.com","http://x.qweatherapi.com","https://x.qweatherapi.com/","x.qweatherapi.com:443","user@x.qweatherapi.com","x.qweatherapi.com?key=fake","x.qweatherapi.com#x","-x.qweatherapi.com","x..qweatherapi.com","中.qweatherapi.com"]{assert!(normalize_host(s).is_err(),"{s}");}
        assert_eq!(normalize_host(" HTTPS://EXAMPLE.re.qweatherapi.com ").unwrap(),"example.re.qweatherapi.com");
    }
    #[test]
    fn monthly_limit_persists_handles_concurrency_and_resets_next_month(){
        let dir=directory();let p=dir.join("usage.json");crate::model::atomic_write(&p,&serde_json::to_vec(&Usage{month:"2026-11".into(),attempts:LIMIT-5}).unwrap()).unwrap();
        let workers:Vec<_>=(0..20).map(|_|{let p=p.clone();std::thread::spawn(move||reserve(&p,"2026-11"))}).collect();
        assert_eq!(workers.into_iter().filter(|t|t.thread().id()!=std::thread::current().id()).map(|t|t.join().unwrap()).filter(Result::is_ok).count(),5);
        assert_eq!(reserve(&p,"2026-11"),Err(Error::Limit));assert!(reserve(&p,"2026-12").is_ok());
        let u:Usage=serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();assert_eq!(u.attempts,1);
        std::fs::write(&p,"broken").unwrap();assert_eq!(reserve(&p,"2026-12"),Err(Error::Storage));std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn all_endpoints_share_budget_disabled_calls_never_send_and_key_is_not_in_url(){
        let dir=directory();let p=dir.join("usage.json");let mut c=Client::new("example.re.qweatherapi.com","fake-test-key",Arc::new(AtomicBool::new(true))).unwrap();c.budget=Some(p.clone());
        let sent=Arc::new(std::sync::atomic::AtomicUsize::new(0));let n=sent.clone();
        c.mock=Some(Arc::new(move|url,key|{assert_eq!(key,"fake-test-key");assert!(!url.as_str().contains(key));assert_eq!(url.scheme(),"https");n.fetch_add(1,Ordering::SeqCst);if url.path().contains("lookup"){Ok(json!({"location":[{"name":"余杭","lat":"30.42","lon":"119.99","adm1":"浙江省","adm2":"杭州","tz":"Asia/Shanghai"}]}))}else if url.path().contains("daily"){assert_eq!(url.query_pairs().find(|(k,_)|k=="days").unwrap().1,"7");Ok(daily())}else{Ok(current())}}));
        c.test().unwrap();assert_eq!(sent.load(Ordering::SeqCst),3);
        let results=c.search("余杭","浙江").unwrap();assert_eq!(results.locations[0].position.region,"浙江省 · 杭州");
        c.weather(&location(),false).unwrap();c.weather(&location(),true).unwrap();assert_eq!(sent.load(Ordering::SeqCst),6);
        c.gate.store(false,Ordering::SeqCst);assert_eq!(c.weather(&location(),false),Err(Error::Disabled));assert_eq!(sent.load(Ordering::SeqCst),6);
        c.gate.store(true,Ordering::SeqCst);crate::model::atomic_write(&p,&serde_json::to_vec(&Usage{month:month(),attempts:LIMIT}).unwrap()).unwrap();
        assert!(matches!(c.search("县","省"),Err(Error::Limit)));assert_eq!(c.weather(&location(),false),Err(Error::Limit));assert_eq!(c.test(),Err(Error::Limit));assert_eq!(sent.load(Ordering::SeqCst),6);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn maps_units_codes_timezone_solar_probability_and_credits_without_inventing_missing_times(){
        let mut s=parse_current(&current(),&location(),chrono::Utc::now().timestamp()).unwrap();assert_eq!(s.humidity,Some(68));assert_eq!(s.wind,Some(16.2));assert_eq!(s.feels,Some(20.0));assert_eq!(s.code,61);assert_eq!(s.description(),"小雨");assert_eq!(s.attributions.len(),1);
        let(days,sun)=parse_daily(&daily(),&location()).unwrap();assert_eq!(days[0].rain,80);let sun=sun.unwrap();assert_eq!(sun.clock(sun.sunrise),"06:10");assert_eq!(sun.timezone,"Asia/Shanghai");
        let mut no_sun=daily();no_sun["days"][0]["astro"]=json!({});assert!(parse_daily(&no_sun,&location()).unwrap().1.is_none());
        let mut invalid=current();invalid["temperature"]["unit"]=json!("unknown");assert!(parse_current(&invalid,&location(),1).is_err());s.at=123;assert_eq!(s.at,123);
    }
    #[test]
    fn fresh_short_forecast_obeys_the_success_interval(){
        let now=chrono::Utc::now().timestamp();
        let client=Client::new("example.re.qweatherapi.com","test",Arc::new(AtomicBool::new(true))).unwrap();
        let mut runtime=Runtime::default();
        runtime.cache=Cache{location:Some(location()),current:Some(current()),daily:Some(daily()),current_at:now,daily_at:now,..Default::default()};
        assert_eq!(runtime.snapshot().unwrap().days.len(),1);
        runtime.refresh(&client,now+120,false);
        assert!(!runtime.loading());
    }
    #[test]
    fn display_only_rebuilds_on_new_data_or_city_midnight(){
        let mut runtime=Runtime::default();
        runtime.cache.location=Some(location());
        let midnight=chrono::DateTime::parse_from_rfc3339("2026-11-14T00:00:00+08:00").unwrap().timestamp();
        assert!(runtime.poll(true,midnight-2));
        assert!(!runtime.poll(true,midnight-1));
        assert!(!runtime.poll(false,midnight));
        assert!(runtime.poll(true,midnight));
        assert!(!runtime.poll(true,midnight));
    }
    #[test]
    fn official_minute_precision_dates_keep_their_offset(){
        for text in ["2026-11-14T10:00Z","2026-11-14T18:00+08:00","2026-11-14T18:00+0800"] {assert_eq!(timestamp(&json!(text)),Some(1794650400));}
        let mut v=current();v["validTime"]=json!("2026-11-14T18:00+08:00");assert!(parse_current(&v,&location(),1794650500).is_ok());
    }
    #[test]
    fn cache_is_host_and_location_scoped_and_module_off_discards_inflight_results(){
        let mut r=Runtime::default();r.cache=Cache{host:"example.re.qweatherapi.com".into(),location:Some(location()),current:Some(current()),current_at:123,daily:Some(daily()),daily_at:120};
        let(tx,rx)=mpsc::channel();r.current=Some(rx);tx.send(Ok(current())).unwrap();r.poll(false,999);assert_eq!(r.stamps(),(123,120));assert!(!r.loading());
        let(tx,rx)=mpsc::channel();r.current=Some(rx);tx.send(Err(Error::Auth)).unwrap();r.poll(true,999);assert!(r.paused);assert_eq!(r.stamps(),(123,120));
        r.reset("different.re.qweatherapi.com",Some(location()));assert!(r.snapshot().is_none());
    }

    #[test]
    fn current_refresh_failure_requires_repeated_errors_and_recovers_on_success(){
        let _g=crate::ui_tests::ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
        let previous=std::env::var_os("DAOZAI_DATA_DIR");let dir=directory();std::env::set_var("DAOZAI_DATA_DIR",&dir);
        let mut r=Runtime::default();r.cache.location=Some(location());
        fn deliver(r:&mut Runtime,daily:bool,result:Result<Value,Error>){
            let(tx,rx)=mpsc::channel();if daily{r.daily=Some(rx);}else{r.current=Some(rx);}
            tx.send(result).unwrap();r.poll(true,chrono::Utc::now().timestamp());
        }
        assert!(!r.current_refresh_failed());
        deliver(&mut r,false,Err(Error::Network));assert!(!r.current_refresh_failed(),"一次短暂失败不能让温度变灰");
        deliver(&mut r,true,Err(Error::Network));assert!(!r.current_refresh_failed(),"预报失败不能计入实况失败次数");
        deliver(&mut r,false,Err(Error::Network));assert!(r.current_refresh_failed());
        deliver(&mut r,true,Ok(daily()));assert!(r.current_refresh_failed(),"预报恢复不能清掉实况错误");
        deliver(&mut r,false,Ok(current()));assert!(!r.current_refresh_failed(),"实况恢复应立即清除连续失败状态");
        deliver(&mut r,true,Err(Error::Network));assert!(!r.current_refresh_failed(),"仅预报失败时温度仍正常");
        deliver(&mut r,false,Err(Error::Auth));assert!(r.current_refresh_failed(),"凭据失效导致停止重试时也应提示过期数据");
        r.reset("different.re.qweatherapi.com",Some(location()));assert!(!r.current_refresh_failed(),"切换来源配置后应重新判定");
        if let Some(previous)=previous{std::env::set_var("DAOZAI_DATA_DIR",previous);}else{std::env::remove_var("DAOZAI_DATA_DIR");}
        std::fs::remove_file(dir.join("qweather-cache.json")).unwrap();std::fs::remove_dir(dir).unwrap();
    }
}

fn attributions(v:&Value)->Vec<String> {v["metadata"]["attributions"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()}
fn timestamp(v:&Value)->Option<i64>{let text=v.as_str()?;chrono::DateTime::parse_from_rfc3339(text).or_else(|_|chrono::DateTime::parse_from_str(text,"%Y-%m-%dT%H:%M%#z")).ok().map(|t|t.timestamp())}
fn temperature(v:&Value)->Option<f32>{let n=v["value"].as_f64()?;match v["unit"].as_str()?{"°C"=>Some(n as f32),"K"=>Some((n-273.15)as f32),"°F"=>Some(((n-32.0)/1.8)as f32),_=>None}}
/// 和风天气代码单独转换为现有绘图代码；原始天气文字随实况保留。
fn icon_code(code:&str)->u8 {match code.parse::<u16>().unwrap_or(999){100|150=>0,101|102|103|151|152|153=>2,104=>3,300=>80,301=>81,302=>95,303=>97,304=>99,305|314|399=>61,306|315=>63,307|308|310..=312|316..=318=>65,309=>51,313=>66,400|408|499=>71,401|409=>73,402|403|410=>75,404..=406=>77,407=>85,500..=515=>45,_=>3}}
fn parse_current(v:&Value,l:&Location,at:i64)->Result<Snapshot,Error>{
    let zone=l.timezone.parse::<chrono_tz::Tz>().map_err(|_|Error::Response)?;
    let valid=timestamp(&v["validTime"]).ok_or(Error::Response)?;
    let hour=chrono::DateTime::from_timestamp(valid,0).ok_or(Error::Response)?.with_timezone(&zone).format("%H").to_string().parse::<u32>().unwrap_or(12);
    let wind=&v["wind"]["speed"];
    Ok(Snapshot{place:l.position.label.clone(),position:Some(l.position.clone()),temp:temperature(&v["temperature"]).ok_or(Error::Response)?,feels:temperature(&v["feelsLike"]),humidity:v["humidity"].as_f64().filter(|n|(0.0..=1.0).contains(n)).map(|n|(n*100.0).round()as u32),wind:wind["value"].as_f64().and_then(|n|match wind["unit"].as_str()?{"m/s"=>Some((n*3.6)as f32),"km/h"=>Some(n as f32),_=>None}),code:icon_code(v["condition"]["code"].as_str().unwrap_or("999")),condition:v["condition"]["text"].as_str().map(str::to_string),day:(6..18).contains(&hour),at:at.min(valid),attributions:attributions(v),..Default::default()})
}
fn parse_daily(v:&Value,l:&Location)->Result<(Vec<Day>,Option<SunTimes>),Error>{
    let zone=l.timezone.parse::<chrono_tz::Tz>().map_err(|_|Error::Response)?;let now=chrono::Utc::now().with_timezone(&zone).date_naive();
    let mut sun=None;let mut days=Vec::new();
    for day in v["days"].as_array().ok_or(Error::Response)? {
        let start=timestamp(&day["forecastStartTime"]).ok_or(Error::Response)?;
        // 逐日预报可能从当地早晨开始，以日出所在的当地日期为准；无日出时使用区间起始日期。
        let rise=timestamp(&day["astro"]["sunrise"]);let set=timestamp(&day["astro"]["sunset"]);
        let date=chrono::DateTime::from_timestamp(rise.unwrap_or(start),0).ok_or(Error::Response)?.with_timezone(&zone).date_naive();
        if date<now {continue;}
        if date==now {if let (Some(rise),Some(set),Some(a),Some(b))=(rise,set,zone.from_local_datetime(&date.and_hms_opt(0,0,0).unwrap()).earliest(),zone.from_local_datetime(&date.succ_opt().ok_or(Error::Response)?.and_hms_opt(0,0,0).unwrap()).earliest()) {if a.timestamp()<=rise&&rise<set&&set<=b.timestamp(){sun=Some(SunTimes{day_start:a.timestamp(),day_end:b.timestamp(),sunrise:rise,sunset:set,timezone:l.timezone.clone()});}}}
        let probability=["daytime","nighttime"].iter().filter_map(|p|day[*p]["precipitation"]["probability"].as_f64()).filter(|n|(0.0..=1.0).contains(n)).reduce(f64::max).unwrap_or(0.0);
        days.push(Day{label:if date==now{"今天".into()}else{["周一","周二","周三","周四","周五","周六","周日"][date.weekday().num_days_from_monday()as usize].into()},date:date.to_string(),high:temperature(&day["temperatureMax"]).ok_or(Error::Response)?,low:temperature(&day["temperatureMin"]).ok_or(Error::Response)?,code:icon_code(day["daytime"]["condition"]["code"].as_str().unwrap_or("999")),rain:(probability*100.0).round()as u32});
    }
    if days.is_empty(){return Err(Error::Response);}days.truncate(7);Ok((days,sun))
}
