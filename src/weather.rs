//! 天气：按城市（留空时按网络出口定位）读取 Open-Meteo 的实况与七日预报。
//! 只保留最近一次结果用于离线展示，不读取、也不上传任何账号信息。
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

/// 自动刷新的间隔（秒）：Open-Meteo 的实况本身每 15 分钟更新一次。
pub const INTERVAL: i64 = 900;
/// 自动定位结果的复用时长（秒）。
const LOCATE_TTL: i64 = 3 * 3600;
/// 最多保存的城市数（不含自动定位）。
pub const MAX_CITIES: usize = 5;
/// 多城市缓存的条数上限：自动定位 + 已保存城市，再留一条余量给刚删除的城市。
const CACHE_LIMIT: usize = MAX_CITIES + 2;
pub const FORECAST_DAYS: usize = 7;

/// 天气现象；决定画太阳、云、雨还是雪。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Sun, Moon, Partly, PartlyNight, Cloud, Fog, Drizzle, Rain, Shower, Snow, Thunder }

/// WMO 天气代码 → 图标类型（`day` 决定太阳还是月亮）。
pub fn kind(code: u8, day: bool) -> Kind {
    match code {
        0 => if day { Kind::Sun } else { Kind::Moon },
        1 | 2 => if day {Kind::Partly} else {Kind::PartlyNight},
        3 => Kind::Cloud,
        45 | 48 => Kind::Fog,
        51 | 53 | 55 | 56 | 57 => Kind::Drizzle,
        61 | 63 | 65 | 66 | 67 => Kind::Rain,
        71 | 73 | 75 | 77 => Kind::Snow,
        80 | 81 | 82 => Kind::Shower,
        85 | 86 => Kind::Snow,
        95 | 96 | 97 | 99 => Kind::Thunder,
        _ => Kind::Cloud,
    }
}

/// WMO 天气代码 → 中文描述。
pub fn describe(code: u8) -> &'static str {
    match code {
        0 => "晴",
        1 => "晴间多云",
        2 => "多云",
        3 => "阴",
        45 | 48 => "雾",
        51 => "毛毛雨",
        53 => "毛毛雨",
        55 => "强毛毛雨",
        56 | 57 | 66 | 67 => "冻雨",
        61 => "小雨",
        63 => "中雨",
        65 => "大雨",
        71 => "小雪",
        73 => "中雪",
        75 => "大雪",
        77 => "米雪",
        80 => "阵雨",
        81 => "强阵雨",
        82 => "暴雨",
        85 => "阵雪",
        86 => "强阵雪",
        95 => "雷阵雨",
        97 => "强雷阵雨",
        96 | 99 => "雷暴伴冰雹",
        _ => "多云",
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Day {
    pub label: String,
    #[serde(default)]
    pub date: String,
    pub high: f32,
    pub low: f32,
    pub code: u8,
    pub rain: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SunTimes {
    pub day_start: i64,
    pub day_end: i64,
    pub sunrise: i64,
    pub sunset: i64,
    pub timezone: String,
}

impl SunTimes {
    pub fn midpoint(&self) -> i64 {self.sunrise+(self.sunset-self.sunrise)/2}

    pub fn clock(&self, at: i64) -> String {
        self.timezone.parse::<chrono_tz::Tz>().ok().and_then(|zone| {
            chrono::DateTime::from_timestamp(at,0).map(|time|time.with_timezone(&zone).format("%H:%M").to_string())
        }).unwrap_or_else(||"--:--".into())
    }

    /// 日出到日落之间的时间进度；夜间不在日照轨迹上绘制当前位置。
    pub fn progress(&self, now: i64) -> Option<f32> {
        if !(self.sunrise..=self.sunset).contains(&now) {return None;}
        Some((now-self.sunrise) as f32/(self.sunset-self.sunrise).max(1) as f32)
    }
}

fn parse_sun_times(value:&Value)->Option<SunTimes> {
    use chrono::TimeZone;
    let timezone=value["timezone"].as_str()?;
    let zone=timezone.parse::<chrono_tz::Tz>().ok()?;
    let daily=&value["daily"];
    let date=chrono::NaiveDate::parse_from_str(daily["time"][0].as_str()?,"%Y-%m-%d").ok()?;
    let at=|time|zone.from_local_datetime(&time).earliest().map(|time|time.timestamp());
    let day_start=at(date.and_hms_opt(0,0,0)?)?;
    let day_end=at(date.succ_opt()?.and_hms_opt(0,0,0)?)?;
    let sunrise=at(chrono::NaiveDateTime::parse_from_str(daily["sunrise"][0].as_str()?,"%Y-%m-%dT%H:%M").ok()?)?;
    let sunset=at(chrono::NaiveDateTime::parse_from_str(daily["sunset"][0].as_str()?,"%Y-%m-%dT%H:%M").ok()?)?;
    (day_start<=sunrise && sunrise<sunset && sunset<=day_end).then(||SunTimes{day_start,day_end,sunrise,sunset,timezone:timezone.into()})
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub attributions: Vec<String>,
    pub place: String,
    #[serde(default)]
    pub position: Option<Position>,
    /// 来自自动定位；多城市缓存据此区分「自动定位」与同名的指定城市。
    #[serde(default)]
    pub auto: bool,
    pub temp: f32,
    pub feels: Option<f32>,
    pub humidity: Option<u32>,
    pub wind: Option<f32>,
    pub code: u8,
    pub day: bool,
    pub high: Option<f32>,
    pub low: Option<f32>,
    /// 今日日出与日落（当地时间 HH:MM）；旧缓存或接口缺失时为空。
    #[serde(default)]
    pub sunrise: Option<String>,
    #[serde(default)]
    pub sunset: Option<String>,
    #[serde(default)]
    pub sun_times: Option<SunTimes>,
    pub days: Vec<Day>,
    /// 取得这份数据的 Unix 时间（秒）。
    pub at: i64,
}

impl Snapshot {
    pub fn description(&self)->&str {self.condition.as_deref().unwrap_or_else(||describe(self.code))}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub lat: f64, pub lon: f64, pub label: String,
    #[serde(default)]
    pub region: String,
}

impl Position {
    pub fn valid(&self) -> bool {
        !self.label.trim().is_empty() && self.lat.is_finite() && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat) && (-180.0..=180.0).contains(&self.lon)
    }
}

fn json(response: ureq::Response) -> Result<Value, String> {
    serde_json::from_reader(response.into_reader().take(1024 * 1024)).map_err(|_| "响应格式无效".to_string())
}

fn get(agent: &ureq::Agent, url: &url::Url, service: &str) -> Result<Value, String> {
    get_with_proxy(agent,url,service,network_proxy().as_deref())
}

fn get_with_proxy(agent: &ureq::Agent, url: &url::Url, service: &str, proxy: Option<&str>) -> Result<Value, String> {
    match request(agent,url,service,"direct") {
        Ok(value)=>Ok(value),
        Err((error,retryable))=>{
            if retryable {if let Some(proxy)=proxy {
                return request(&http_agent_for(Some(proxy)),url,service,"proxy")
                    .map_err(|(last,_)|format!("{service}：直连{error}；代理{last}"));
            }}
            Err(format!("{service}：{error}"))
        }
    }
}

fn request(agent: &ureq::Agent, url: &url::Url, service: &str, route: &str) -> Result<Value,(String,bool)> {
    let start=std::time::Instant::now();
    let mut detail="ok".to_string();
    let result=agent.get(url.as_str()).set("Accept", "application/json").set("User-Agent",concat!("Daozai/",env!("CARGO_PKG_VERSION"))).call()
        .map_err(|error| match error {
            ureq::Error::Status(code, _) => {detail=format!("http_{code}");(format!("服务返回 {code}"),code>=500 || code==403 || code==408)},
            ureq::Error::Transport(error) => {
                let io_kind=std::error::Error::source(&error).and_then(|cause|cause.downcast_ref::<std::io::Error>()).map(|error|error.kind());
                detail=format!("{:?}/{io_kind:?}",error.kind());
                let message=match error.kind() {
                    ureq::ErrorKind::Dns => "域名解析失败",
                    ureq::ErrorKind::ProxyConnect => "代理连接失败",
                    ureq::ErrorKind::ProxyUnauthorized => "代理认证失败",
                    _ if io_kind==Some(std::io::ErrorKind::TimedOut) || io_kind==Some(std::io::ErrorKind::WouldBlock) => "请求超时",
                    _ => "网络连接失败，请检查网络或代理",
                };
                (message.to_string(),true)
            }
        })
        .and_then(|response|json(response).map_err(|error|{detail="invalid_response".into();(error,true)}));
    if !cfg!(test) {
        // 只记录环节、域名与错误类别，不记录查询参数、IP、坐标或代理地址。
        crate::resources::append_log(&crate::model::config_dir().join("diagnostics"),"weather-network.log",
            &format!("time={} stage={service} route={route} host={} elapsed_ms={} result={detail}",chrono::Utc::now().timestamp(),url.host_str().unwrap_or_default(),start.elapsed().as_millis()));
    }
    result
}

fn network_proxy() -> Option<String> {
    ["HTTPS_PROXY","https_proxy","ALL_PROXY","all_proxy","HTTP_PROXY","http_proxy"].iter()
        .find_map(|key|std::env::var(key).ok().filter(|value|!value.is_empty())).or_else(crate::quota::system_proxy)
}

fn http_agent_for(proxy: Option<&str>) -> ureq::Agent {
    let mut builder=ureq::AgentBuilder::new().try_proxy_from_env(false).timeout_connect(Duration::from_secs(4)).timeout(Duration::from_secs(12));
    if let Some(proxy)=proxy.and_then(|value|ureq::Proxy::new(value).ok()){builder=builder.proxy(proxy);}
    builder.build()
}

fn http_agent() -> ureq::Agent {http_agent_for(None)}

/// 按网络出口定位；国内常见出口能拿到中文城市名。
pub fn locate(agent: &ureq::Agent) -> Result<Position, String> {
    locate_with_sources(agent,&[("https://ipwho.is/?lang=zh-CN",false),("https://free.freeipapi.com/api/v1/json",true)],network_proxy().as_deref())
}

fn locate_with_sources(agent: &ureq::Agent, sources: &[(&str,bool)], proxy: Option<&str>) -> Result<Position,String> {
    let mut retry=vec![true;sources.len()];
    let mut last_error="自动定位暂不可用，请手动选择城市".to_string();
    // 先尝试两个直连服务，再回退到已配置的代理，避免主服务故障时过早采用代理出口的位置。
    for through_proxy in [false,true] {
        if through_proxy && (proxy.is_none() || !retry.iter().any(|retry|*retry)) {break;}
        let configured;
        let client=if through_proxy {configured=http_agent_for(proxy);&configured}else{agent};
        for (index,&(address,backup)) in sources.iter().enumerate() {
            if !retry[index] {continue;}
            let url=url::Url::parse(address).map_err(|_|"定位服务地址无效")?;
            match request(client,&url,"自动定位",if through_proxy{"proxy"}else{"direct"}) {
                Ok(value)=>{
                    retry[index]=false;
                    match parse_location(&value,backup) {Ok(position)=>return Ok(position),Err(error)=>last_error=error}
                }
                Err((error,retryable))=>{retry[index]=retryable;last_error=format!("自动定位：{error}");}
            }
        }
    }
    Err(last_error)
}

fn parse_location(value: &Value, backup: bool) -> Result<Position,String> {
    if !backup && !value["success"].as_bool().unwrap_or(false) { return Err("自动定位失败，请在设置里填写城市".into()); }
    let lat = value["latitude"].as_f64().ok_or("定位结果缺少坐标")?;
    let lon = value["longitude"].as_f64().ok_or("定位结果缺少坐标")?;
    let fields=if backup {["cityName","regionName","countryName"]}else{["city","region","country"]};
    let label = fields.iter().find_map(|key|value[*key].as_str().filter(|s| !s.trim().is_empty()));
    if backup && label.is_none() {return Err("定位结果缺少地区".into());}
    let label=label.unwrap_or("当地").trim().to_string();
    let region=value[fields[1]].as_str().unwrap_or_default().to_string();
    let position=Position { lat, lon, label, region };
    if !position.valid() {return Err("定位结果坐标无效".into());}
    Ok(position)
}

/// 中文行政区后缀：输入「广州市」时还要查「广州」，地理库里很多城市只收录了不带后缀的名字。
const ADMIN_SUFFIXES: [char; 4] = ['市', '县', '区', '省'];

fn is_cjk(name: &str) -> bool { name.chars().all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) }

/// 去掉行政区后缀后的基础名；去掉后不足两个字（如「津市」）时保留原名。
fn base_name(name: &str) -> &str {
    match name.strip_suffix(ADMIN_SUFFIXES) {
        Some(base) if is_cjk(name) && base.chars().count() >= 2 => base,
        _ => name,
    }
}

/// 省级行政区 → 省会（首府）。地理库只收录城市，搜「新疆」「内蒙古」这类地区名查不到，
/// 改为同时搜它的省会，用户能直接选到该地区的代表城市。
const PROVINCE_CAPITALS: [(&str, &str); 34] = [
    ("北京","北京"),("天津","天津"),("上海","上海"),("重庆","重庆"),("河北","石家庄"),("山西","太原"),
    ("辽宁","沈阳"),("吉林","长春"),("黑龙江","哈尔滨"),("江苏","南京"),("浙江","杭州"),("安徽","合肥"),
    ("福建","福州"),("江西","南昌"),("山东","济南"),("河南","郑州"),("湖北","武汉"),("湖南","长沙"),
    ("广东","广州"),("海南","海口"),("四川","成都"),("贵州","贵阳"),("云南","昆明"),("陕西","西安"),
    ("甘肃","兰州"),("青海","西宁"),("台湾","台北"),("内蒙古","呼和浩特"),("广西","南宁"),("西藏","拉萨"),
    ("宁夏","银川"),("新疆","乌鲁木齐"),("香港","香港"),("澳门","澳门"),
];

/// 输入若是省级地区（含「新疆维吾尔自治区」「广东省」等全称），返回它的省会。
fn province_capital(name: &str) -> Option<&'static str> {
    let short=["维吾尔自治区","壮族自治区","回族自治区","自治区","特别行政区","省","市"].iter()
        .find_map(|suffix|name.strip_suffix(suffix)).unwrap_or(name);
    PROVINCE_CAPITALS.iter().find(|(province,_)|*province==short).map(|(_,capital)|*capital)
}

fn city_queries(name: &str) -> Vec<String> {
    let mut queries=vec![name.to_string()];
    if name.chars().count()<2 || !is_cjk(name) {return queries;}
    if let Some(capital)=province_capital(name).filter(|capital|*capital!=base_name(name)) {
        // 「新疆」→ 也查「乌鲁木齐」。直辖市本身就是城市，不额外查。
        queries.push(capital.to_string());
    }
    let base=base_name(name);
    if base!=name {
        // 「广州市」→ 也查「广州」。
        queries.push(base.to_string());
    } else if !name.ends_with('州') {
        // 两字简称只匹配同名乡镇时，也查完整市名，不能直接采用第一个同名地点。
        queries.insert(0,format!("{name}市"));
    }
    queries
}

/// 地理库里的非居住地（机场、车站等）不作为天气地点。
fn skip_feature(code: &str) -> bool {
    ["AIR", "RSTN", "BUS", "PRT", "HTL", "SCH", "HSP"].iter().any(|prefix| code.starts_with(prefix))
}

fn city_candidates(value: &Value) -> Vec<(Position, u64)> {
    value["results"].as_array().into_iter().flatten().filter_map(|item|{
        if skip_feature(item["feature_code"].as_str().unwrap_or_default()) {return None;}
        let label=item["name"].as_str()?.trim().to_string();
        let mut areas=Vec::new();
        for key in ["admin2","admin1","country"] {
            if let Some(area)=item[key].as_str().filter(|s|!s.is_empty() && *s!=label) {
                if !areas.contains(&area) {areas.push(area);}
            }
        }
        let position=Position{lat:item["latitude"].as_f64()?,lon:item["longitude"].as_f64()?,label,region:areas.join(" · ")};
        position.valid().then_some((position,item["population"].as_u64().unwrap_or(0)))
    }).collect()
}

/// 排序：名字与输入（或去掉后缀的基础名）完全一致的排前面，其次按人口从多到少；
/// 坐标几乎重合的同名地点只留一个。
fn rank_cities(name: &str, mut found: Vec<(Position, u64)>) -> Vec<Position> {
    let base=base_name(name);
    let exact=|p:&Position|p.label==name || p.label==base || base_name(&p.label)==base;
    found.sort_by(|(a,pa),(b,pb)|exact(b).cmp(&exact(a)).then(pb.cmp(pa)));
    let mut ranked:Vec<Position>=Vec::new();
    for (position,_) in found {
        let duplicate=ranked.iter().any(|p|p.label==position.label && (p.lat-position.lat).abs()<0.05 && (p.lon-position.lon).abs()<0.05);
        if !duplicate {ranked.push(position);}
    }
    ranked
}

/// 搜索多个同名地点，保留行政区供用户确认。
pub fn search_cities(agent: &ureq::Agent, name: &str) -> Result<Vec<Position>, String> {
    let name=name.trim();
    let mut found=Vec::new();
    let mut error=None;
    for query in city_queries(name) {
        let mut url=url::Url::parse("https://geocoding-api.open-meteo.com/v1/search").map_err(|_|"定位服务地址无效")?;
        url.query_pairs_mut().append_pair("name",&query).append_pair("count","10").append_pair("language","zh").append_pair("format","json");
        match get(agent,&url,"城市搜索") {
            Ok(value)=>found.extend(city_candidates(&value)),
            Err(message)=>error=Some(message),
        }
    }
    if found.is_empty() {if let Some(error)=error{return Err(error);}}
    Ok(rank_cities(name,found))
}

fn unique_city(mut candidates: Vec<Position>, name: &str) -> Result<Position, String> {
    match candidates.len() {
        0=>Err(format!("没找到「{name}」，请试试完整市名或英文名")),
        1=>Ok(candidates.remove(0)),
        _=>Err(format!("「{name}」有多个同名地点，请在设置的下拉列表中选择具体地区")),
    }
}

pub fn geocode(agent: &ureq::Agent, name: &str) -> Result<Position, String> {
    unique_city(search_cities(agent,name)?,name)
}

/// 拉一次天气：先定位，再取实况与七日预报。
pub fn fetch(agent: &ureq::Agent, city: &str) -> Result<Snapshot, String> {
    fetch_at(agent,city,None)
}

fn fetch_at(agent: &ureq::Agent, city: &str, selected: Option<&Position>) -> Result<Snapshot, String> {
    let city = city.trim();
    let position = if city.is_empty() { locate(agent)? }
        else if let Some(position)=selected.filter(|p|p.valid() && p.label==city) {position.clone()}
        else { geocode(agent, city)? };
    fetch_forecast(agent, position)
}

/// 已知坐标时只请求一次预报接口。
fn fetch_forecast(agent: &ureq::Agent, position: Position) -> Result<Snapshot, String> {
    let mut url = url::Url::parse("https://api.open-meteo.com/v1/forecast").map_err(|_| "天气服务地址无效")?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("latitude", &format!("{:.4}", position.lat));
        query.append_pair("longitude", &format!("{:.4}", position.lon));
        query.append_pair("current", "temperature_2m,relative_humidity_2m,apparent_temperature,is_day,weather_code,wind_speed_10m");
        query.append_pair("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset");
        query.append_pair("timezone", "auto");
        query.append_pair("forecast_days", &FORECAST_DAYS.to_string());
    }
    let mut snapshot=parse(&position.label, &get(agent, &url, "天气预报")?)?;
    snapshot.position=Some(position);
    Ok(snapshot)
}

fn day_label(index: usize, date: &str) -> String {
    use chrono::Datelike;
    match index {
        0 => "今天".into(),
        1 => "明天".into(),
        _ => chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map(|d| format!("周{}", ["一", "二", "三", "四", "五", "六", "日"][d.weekday().num_days_from_monday() as usize]))
            .unwrap_or_else(|_| date.get(5..).unwrap_or("").to_string()),
    }
}

pub fn parse(place: &str, value: &Value) -> Result<Snapshot, String> {
    let current = &value["current"];
    let temp = current["temperature_2m"].as_f64().ok_or("天气数据缺少温度")? as f32;
    let daily = &value["daily"];
    let dates: Vec<&Value> = daily["time"].as_array().map(|items| items.iter().collect()).unwrap_or_default();
    let codes = daily["weather_code"].as_array();
    let highs = daily["temperature_2m_max"].as_array();
    let lows = daily["temperature_2m_min"].as_array();
    let rains = daily["precipitation_probability_max"].as_array();
    let mut days = Vec::new();
    for (index, date) in dates.iter().enumerate().take(FORECAST_DAYS) {
        let date = date.as_str().unwrap_or_default();
        let pick = |list: Option<&Vec<Value>>| list.and_then(|items| items.get(index)).and_then(Value::as_f64);
        days.push(Day {
            label: day_label(index, date),
            date: date.to_string(),
            high: pick(highs).unwrap_or_default() as f32,
            low: pick(lows).unwrap_or_default() as f32,
            code: codes.and_then(|items| items.get(index)).and_then(Value::as_u64).unwrap_or(2) as u8,
            rain: rains.and_then(|items| items.get(index)).and_then(Value::as_u64).unwrap_or(0) as u32,
        });
    }
    if days.is_empty() { return Err("天气数据不完整".into()); }
    // timezone=auto 时 daily.sunrise/sunset 就是当地时间，直接截 HH:MM。
    let hhmm = |list: Option<&Vec<Value>>| list.and_then(|items| items.get(0)).and_then(Value::as_str)
        .and_then(|s| s.get(11..16)).map(str::to_string);
    Ok(Snapshot {
        condition:None,attributions:Vec::new(),
        place: place.to_string(),
        position: None,
        auto: false,
        temp,
        feels: current["apparent_temperature"].as_f64().map(|n| n as f32),
        humidity: current["relative_humidity_2m"].as_u64().map(|n| n as u32),
        wind: current["wind_speed_10m"].as_f64().map(|n| n as f32),
        code: current["weather_code"].as_u64().unwrap_or(2) as u8,
        day: current["is_day"].as_u64().unwrap_or(1) != 0,
        high: days.first().map(|d| d.high),
        low: days.first().map(|d| d.low),
        sunrise: hhmm(daily["sunrise"].as_array()),
        sunset: hhmm(daily["sunset"].as_array()),
        sun_times: parse_sun_times(value),
        days,
        at: chrono::Utc::now().timestamp(),
    })
}

/// `17:42` 形式的更新时间。
pub fn clock(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_else(|| "--:--".into())
}

/// 输入停顿后再搜索，最多一个在途请求；旧关键词的结果不覆盖新输入。
#[derive(Default)]
pub struct CitySearch {
    pub results: Vec<Position>,
    pub error: Option<String>,
    query: String,
    changed_at: f64,
    requested: Option<String>,
    rx: Option<Receiver<(String,Result<Vec<Position>,String>)>>,
}

impl CitySearch {
    pub fn pending(&self) -> bool {self.error.is_none() && (self.rx.is_some() || self.requested.as_deref()!=Some(self.query.as_str()))}

    #[cfg(test)]
    pub(crate) fn seed(&mut self, query: &str, results: Vec<Position>) {self.query=query.into();self.requested=Some(query.into());self.results=results;}

    pub fn retry(&mut self) {self.requested=None;self.error=None;}

    pub fn update(&mut self, query: &str, t: f64) {
        let query=query.trim();
        if query!=self.query {
            self.query=query.to_string();self.changed_at=t;self.requested=None;self.results.clear();self.error=None;
        }
        if let Some(rx)=&self.rx {
            match rx.try_recv() {
                Ok((query,result))=>{
                    self.rx=None;
                    if query==self.query {self.requested=Some(query);match result {Ok(results)=>{self.results=results;self.error=None;},Err(error)=>self.error=Some(error)}}
                }
                Err(TryRecvError::Disconnected)=>{self.rx=None;if self.requested.as_deref()==Some(self.query.as_str()){self.error=Some("城市搜索没有响应，请重试".into());}}
                Err(TryRecvError::Empty)=>{}
            }
        }
        if !cfg!(test) && self.rx.is_none() && self.requested.as_deref()!=Some(query) && query.chars().count()>=2 && t-self.changed_at>=0.45 {
            let query=query.to_string();self.requested=Some(query.clone());
            let (tx,rx)=mpsc::channel();self.rx=Some(rx);
            std::thread::spawn(move||{let result=search_cities(&http_agent(),&query);let _=tx.send((query,result));});
        }
    }
}

/// 页面与设置页共享的天气状态；拉取在后台线程，UI 只轮询结果。
#[derive(Default)]
pub struct State {
    pub snapshot: Option<Snapshot>,
    pub error: Option<String>,
    pub loading: bool,
    pub last_start: f64,
    /// 城市输入草稿，确认后才写入设置并发起查询。
    pub city_input: String,
    pub manual_city: bool,
    pub selected_city: Option<Position>,
    pub cities: CitySearch,
    pub active_position: Option<Position>,
    pub solar_open: bool,
    /// 最近一次切换视图的时间（秒），驱动太阳滑入与月亮升起。
    pub solar_at: f64,
    /// 点击涟漪：页面局部坐标与触发时间。
    pub tap: Option<(f32, f32, f64)>,
    /// 连续失败次数：自动刷新按 2、4、8、16、30 分钟退避，成功后清零。
    pub failures: u32,
    /// 自动定位结果与取得时间；3 小时内复用，不必每次刷新都查网络出口。
    located: Option<(Position, i64)>,
    /// 定位时使用的代理；下次请求发现代理变化就重新定位，不增加常驻网络监听。
    location_proxy: Option<String>,
    /// 上一次请求使用的城市。
    last_city: Option<String>,
    last_position: Option<Position>,
    rx: Option<(Receiver<Result<Snapshot, String>>, bool)>,
    cache: Option<PathBuf>,
    /// 各城市最近一次的天气；切换城市时先显示这里的数据，不等网络。
    cached: Vec<Snapshot>,
    /// 岛上城市下拉框是否展开。
    pub picker_open: bool,
}

impl State {
    pub fn load(city: &str, position: Option<&Position>) -> Self {
        let position=position.filter(|p|p.valid() && p.label==city.trim()).cloned();
        let mut state=Self {city_input:city.to_string(),manual_city:!city.trim().is_empty(),selected_city:position.clone(),active_position:position,..Default::default()};
        if cfg!(test) { return state; }
        let dir = crate::model::config_dir();
        let path = dir.join("weather-cities.json");
        state.cached=std::fs::read(&path).ok().and_then(|raw|serde_json::from_slice::<Vec<Snapshot>>(&raw).ok()).unwrap_or_else(||{
            // 旧版只缓存当前城市一份；当时若是自动定位，迁移时补上标记。
            std::fs::read(dir.join("weather.json")).ok().and_then(|raw|serde_json::from_slice::<Snapshot>(&raw).ok())
                .map(|mut snapshot|{snapshot.auto=snapshot.auto||city.trim().is_empty();vec![snapshot]}).unwrap_or_default()
        });
        state.cached.truncate(CACHE_LIMIT);
        state.snapshot=state.cached_for(city,state.active_position.as_ref()).cloned();
        // 快照保留用于展示；其中没有定位时刻和代理信息，启动后重新确认网络出口。
        state.cache=Some(path);
        state
    }

    /// 某个城市（空串为自动定位）最近一次的天气缓存。
    pub fn cached_for(&self, city: &str, position: Option<&Position>) -> Option<&Snapshot> {
        self.cached.iter().find(|snapshot|cache_matches(snapshot,city,position))
    }

    /// 切换到另一个城市：丢弃在途请求，立即换上该城市的缓存；缓存仍新鲜就不再联网，
    /// 否则交给下一次 `should_refresh`（城市变化会立刻触发，不受退避限制）。
    pub fn switch(&mut self, city: &str, position: Option<Position>, now: i64) {
        let city=city.trim().to_string();
        self.rx=None;self.loading=false;self.error=None;self.failures=0;
        self.picker_open=false;self.solar_open=false;
        // 设置页的搜索框是「添加城市」用的草稿，切换城市不改动它。
        self.manual_city=position.is_some();self.active_position=position.clone();
        self.snapshot=self.cached_for(&city,position.as_ref()).cloned();
        if self.snapshot.as_ref().is_some_and(|s|(0..INTERVAL).contains(&(now-s.at))) {
            self.last_city=Some(city);self.last_position=position;
        } else if self.last_city.as_deref()==Some(city.as_str()) && self.last_position==position {
            // 切回刚才请求过的城市：清掉记录，让城市变化重新触发刷新。
            self.last_city=None;
        }
    }

    /// 删除城市时一并清掉它的缓存。
    pub fn forget_city(&mut self, position: &Position) {
        self.cached.retain(|s|s.auto || s.position.as_ref()!=Some(position));
        self.persist();
    }

    fn persist(&self) {
        if let Some(path)=&self.cache {
            if let Ok(raw)=serde_json::to_vec(&self.cached) {let _=crate::model::atomic_write(path,&raw);}
        }
    }

    /// 设置页搜索框里已确认的地区（输入框文字与所选地区一致）。
    pub fn picked_city(&self) -> Option<&Position> {
        self.selected_city.as_ref().filter(|p|p.valid() && p.label==self.city_input.trim())
    }

    /// 自动刷新的退避间隔（秒）：断网时不再每两分钟起一次请求。
    pub fn backoff(&self) -> f64 {
        if self.failures==0 {0.0} else {(120.0*2f64.powi(self.failures.min(5) as i32-1)).min(1800.0)}
    }

    /// 改为自动定位时调用：下次刷新重新识别网络出口。
    pub fn forget_location(&mut self) {self.located=None;}

    /// 设置里的城市是否和上次请求不一致（首次启动也算）。
    pub fn city_changed(&self, city: &str) -> bool {
        self.last_city.as_deref()!=Some(city.trim()) || self.last_position.as_ref()!=self.position_for(city)
    }

    fn position_for(&self, city: &str) -> Option<&Position> {self.active_position.as_ref().filter(|p|!city.trim().is_empty() && p.label==city.trim())}

    pub fn draft_city(&self) -> Option<&str> {
        if !self.manual_city {Some("")}else{self.selected_city.as_ref().filter(|p|p.valid() && p.label==self.city_input.trim()).map(|p|p.label.as_str())}
    }

    pub fn can_apply(&self, t: f64) -> bool {
        self.draft_city().is_some_and(|city|!self.loading &&
            (self.active_position.as_ref()!=self.draft_position() || self.can_refresh(city,t)))
    }

    pub fn draft_position(&self) -> Option<&Position> {self.selected_city.as_ref().filter(|_|self.manual_city && self.draft_city().is_some())}

    pub fn can_refresh(&self, city: &str, t: f64) -> bool {
        !self.loading && (self.city_changed(city) || t-self.last_start>=if self.error.is_some(){120.0}else{30.0})
    }

    pub fn should_refresh(&self, city: &str, t: f64, now: i64, enabled: bool) -> bool {
        enabled && (self.city_changed(city) || (self.stale(now) && t-self.last_start>=self.backoff())) && self.can_refresh(city,t)
    }

    fn cached_location(&mut self, city: &str, now: i64, proxy: Option<&str>, manual: bool) -> Option<Position> {
        if self.location_proxy.as_deref()!=proxy || manual && city.is_empty() {self.forget_location();}
        self.location_proxy=proxy.map(str::to_owned);
        self.located.as_ref().filter(|(p,at)|city.is_empty() && p.valid() && (0..LOCATE_TTL).contains(&(now-at))).map(|(p,_)|p.clone())
    }

    pub fn refresh(&mut self, city: &str, t: f64) -> bool {
        self.refresh_inner(city,t,false)
    }

    /// 用户主动刷新：自动定位模式下同时更新位置，仍遵守加载状态与冷却间隔。
    pub fn refresh_manual(&mut self, city: &str, t: f64) -> bool {
        self.refresh_inner(city,t,true)
    }

    fn refresh_inner(&mut self, city: &str, t: f64, manual: bool) -> bool {
        if !self.can_refresh(city,t) { return false; }
        let city = city.trim().to_string();
        let position=self.position_for(&city).cloned();
        if self.last_city.is_some() && self.city_changed(&city) {
            self.snapshot=self.cached_for(&city,position.as_ref()).cloned();self.error=None;self.failures=0;
        }
        self.last_city = Some(city.clone());
        self.last_position=position.clone();
        let now=chrono::Utc::now().timestamp();
        let proxy=network_proxy();
        let located=self.cached_location(&city,now,proxy.as_deref(),manual);
        let (tx, rx) = mpsc::channel();
        let fresh_location=city.is_empty() && located.is_none();
        self.rx = Some((rx,fresh_location));
        self.loading = true;
        self.last_start = t;
        std::thread::spawn(move || {
            let agent = http_agent();
            let _ = tx.send(match located {
                Some(located) => fetch_forecast(&agent, located),
                None => fetch_at(&agent, &city, position.as_ref()),
            });
        });
        true
    }

    /// 是否有新结果（外壳据此立即重绘）。
    pub fn poll(&mut self, city: &str, enabled: bool) -> bool {
        let Some((rx,fresh_location)) = &self.rx else { return false; };
        let fresh_location=*fresh_location;
        match rx.try_recv() {
            Ok(result) => {
                self.rx = None;
                self.loading = false;
                // 关闭模块或确认了别的城市后，旧请求不再更新界面和缓存。
                if !enabled || self.city_changed(city) { return true; }
                match result {
                    Ok(mut snapshot) => {
                        snapshot.auto=city.trim().is_empty();
                        // 同一城市只留最新一份，新数据放最前；超出上限时丢掉最旧的。
                        let key=snapshot.clone();
                        self.cached.retain(|s|!(s.auto==key.auto && (key.auto || s.position==key.position)));
                        self.cached.insert(0,snapshot.clone());self.cached.truncate(CACHE_LIMIT);
                        self.persist();
                        if fresh_location {
                            if let Some(position)=&snapshot.position {self.located=Some((position.clone(),snapshot.at));}
                        }
                        self.snapshot = Some(snapshot);
                        self.error = None;
                        self.failures = 0;
                    }
                    Err(error) => {self.error = Some(error);self.failures=self.failures.saturating_add(1);}
                }
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                self.loading = false;
                if enabled && !self.city_changed(city) {self.error=Some("天气服务没有响应".into());self.failures=self.failures.saturating_add(1);}
                true
            }
        }
    }

    /// 数据是否该刷新了（首次打开也算）。
    pub fn stale(&self, now: i64) -> bool { self.snapshot.as_ref().map_or(true, |s| now - s.at >= INTERVAL) }
}

fn cache_matches(snapshot: &Snapshot, city: &str, position: Option<&Position>) -> bool {
    if city.trim().is_empty() {return snapshot.auto;}
    !snapshot.auto && position.is_some_and(|p|p.valid() && p.label==city.trim() && snapshot.position.as_ref()==Some(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn local_weather_server(status: u16, body: &str) -> (url::Url,impl FnOnce()->bool) {
        use std::io::Write;
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url=url::Url::parse(&format!("http://{}",listener.local_addr().unwrap())).unwrap();
        listener.set_nonblocking(true).unwrap();
        let response=format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
        let (stop,cancel)=mpsc::channel::<()>();
        let worker=std::thread::spawn(move||{
            loop {
                if !matches!(cancel.try_recv(),Err(TryRecvError::Empty)){return false;}
                if let Ok((mut stream,_))=listener.accept() {
                    // Windows 接受的连接继承监听器的非阻塞状态，先等客户端发出请求。
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(12))).unwrap();
                    if !matches!(stream.read(&mut [0;4096]),Ok(n) if n>0){return false;}
                    return stream.write_all(response.as_bytes()).is_ok();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        (url,move||{drop(stop);worker.join().unwrap()})
    }

    #[test]
    fn direct_weather_success_does_not_depend_on_a_working_proxy(){
        let (url,direct)=local_weather_server(200,"{\"ok\":true}");
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy=format!("http://{}",listener.local_addr().unwrap());drop(listener);
        let result=get_with_proxy(&http_agent_for(None),&url,"天气预报",Some(&proxy));
        let received=direct();
        assert!(result.is_ok(),"失效代理不应阻断可用的直连：{result:?}");
        assert!(received);
    }

    #[test]
    fn direct_weather_default_client_ignores_a_stale_environment_proxy_when_direct_works(){
        let _guard=crate::ui_tests::ENV_LOCK.lock().unwrap_or_else(|error|error.into_inner());
        let (url,worker)=local_weather_server(200,"{\"ok\":true}");
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy=format!("http://{}",listener.local_addr().unwrap());drop(listener);
        let previous=std::env::var_os("HTTPS_PROXY");std::env::set_var("HTTPS_PROXY",proxy);
        let result=get(&http_agent(),&url,"天气预报");
        match previous {Some(value)=>std::env::set_var("HTTPS_PROXY",value),None=>std::env::remove_var("HTTPS_PROXY")}
        assert!(worker());assert_eq!(result.unwrap()["ok"],true);
    }

    #[test]
    fn direct_weather_can_fall_back_to_an_existing_proxy(){
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url=url::Url::parse(&format!("http://{}",listener.local_addr().unwrap())).unwrap();drop(listener);
        let (proxy,worker)=local_weather_server(200,"{\"ok\":true}");
        let result=get_with_proxy(&http_agent_for(None),&url,"天气预报",Some(proxy.as_str()));
        assert!(worker());assert_eq!(result.unwrap()["ok"],true);
    }

    #[test]
    fn direct_weather_does_not_retry_invalid_unauthorized_or_rate_limited_requests_through_a_proxy(){
        for status in [400,401,429] {
            let (url,direct)=local_weather_server(status,"{}");
            let (proxy,fallback)=local_weather_server(200,"{\"ok\":true}");
            let result=get_with_proxy(&http_agent_for(None),&url,"天气预报",Some(proxy.as_str()));
            let direct_received=direct();let proxy_received=fallback();
            assert!(direct_received);assert!(!proxy_received,"服务拒绝或限流后不能通过切换代理重试");
            assert!(result.unwrap_err().contains(&status.to_string()));
        }
    }

    #[test]
    fn direct_weather_keeps_proxy_fallback_for_blocked_or_unavailable_services(){
        for status in [403,503] {
            let (url,direct)=local_weather_server(status,"{}");
            let (proxy,fallback)=local_weather_server(200,"{\"ok\":true}");
            let result=get_with_proxy(&http_agent_for(None),&url,"天气预报",Some(proxy.as_str()));
            assert!(direct());assert!(fallback());
            assert_eq!(result.unwrap()["ok"],true);
        }
    }

    #[test]
    fn direct_weather_uses_backup_location_before_the_proxy_exit(){
        let (primary,first)=local_weather_server(503,"{}");
        let (backup,second)=local_weather_server(200,r#"{"latitude":30.27,"longitude":120.15,"cityName":"杭州","regionName":"浙江","countryName":"中国"}"#);
        let (proxy,third)=local_weather_server(200,r#"{"success":true,"latitude":40.71,"longitude":-74.0,"city":"代理出口"}"#);
        let result=locate_with_sources(&http_agent_for(None),&[(primary.as_str(),false),(backup.as_str(),true)],Some(proxy.as_str()));
        let received=[first(),second(),third()];
        assert_eq!(result.unwrap().label,"杭州");assert_eq!(received,[true,true,false]);
    }

    #[test]
    fn direct_weather_primary_location_success_skips_the_backup(){
        let (primary,first)=local_weather_server(200,r#"{"success":true,"latitude":30.27,"longitude":120.15,"city":"杭州"}"#);
        let (backup,second)=local_weather_server(200,"{}");
        let result=locate_with_sources(&http_agent_for(None),&[(primary.as_str(),false),(backup.as_str(),true)],None);
        let received=[first(),second()];
        assert_eq!(result.unwrap().label,"杭州");assert_eq!(received,[true,false]);
    }

    #[test]
    fn direct_weather_location_keeps_proxy_fallback_after_both_direct_services_fail(){
        let (primary,first)=local_weather_server(503,"{}");
        let (backup,second)=local_weather_server(503,"{}");
        let (proxy,third)=local_weather_server(200,r#"{"success":true,"latitude":30.27,"longitude":120.15,"city":"代理定位"}"#);
        let result=locate_with_sources(&http_agent_for(None),&[(primary.as_str(),false),(backup.as_str(),true)],Some(proxy.as_str()));
        let received=[first(),second(),third()];
        assert_eq!(result.unwrap().label,"代理定位");assert_eq!(received,[true,true,true]);
    }

    #[test]
    fn direct_weather_backup_location_requires_valid_coordinates(){
        let value=json!({"latitude":30.27,"longitude":120.15,"cityName":"杭州","regionName":"浙江","countryName":"中国"});
        let position=parse_location(&value,true).unwrap();
        assert_eq!(position.label,"杭州");assert_eq!(position.region,"浙江");assert_eq!(position.lat,30.27);
        for invalid in [json!({}),json!({"cityName":"杭州","latitude":999,"longitude":120.15}),json!({"cityName":"杭州","latitude":30.27}),
            json!({"latitude":0,"longitude":0,"cityName":"","regionName":"","countryName":""})] {
            assert!(parse_location(&invalid,true).is_err());
        }
        assert!(parse_location(&json!({"success":false,"latitude":30.27,"longitude":120.15,"city":"杭州"}),false).is_err());
    }

    #[test]
    fn regression_weather_proxy_switch_relocates_without_shortening_the_normal_cache(){
        let position=Position{lat:30.0,lon:120.0,label:"杭州".into(),region:String::new()};
        let mut state=State{located:Some((position.clone(),1000)),..Default::default()};
        assert_eq!(state.cached_location("",1100,None,false),Some(position.clone()));
        assert!(state.cached_location("",1100,Some("http://127.0.0.1:7890"),false).is_none(),"开启代理后不能继续使用直连定位");
        state.located=Some((position.clone(),1100));
        assert_eq!(state.cached_location("",1200,Some("http://127.0.0.1:7890"),false),Some(position.clone()));
        assert!(state.cached_location("",1200,None,false).is_none(),"关闭代理后不能保留代理出口的位置");
        state.located=Some((position,1200));
        assert!(state.cached_location("",1200+LOCATE_TTL,None,false).is_none());
    }

    #[test]
    fn regression_weather_manual_refresh_relocates_only_in_automatic_mode(){
        let position=Position{lat:30.0,lon:120.0,label:"杭州".into(),region:String::new()};
        let mut state=State{located:Some((position.clone(),1000)),..Default::default()};
        assert!(state.cached_location("杭州",1100,None,true).is_none());
        assert_eq!(state.cached_location("",1100,None,false),Some(position));
        assert!(state.cached_location("",1100,None,true).is_none(),"手动刷新自动定位时应重新确认网络出口");
    }

    #[test]
    fn manual_relocation_keeps_cooldowns_and_active_requests(){
        let position=Position{lat:30.0,lon:120.0,label:"杭州".into(),region:String::new()};
        let mut state=State{last_city:Some(String::new()),last_start:100.0,located:Some((position,1000)),..Default::default()};
        assert!(!state.refresh_manual("",129.0));assert!(state.located.is_some());
        state.error=Some("旧错误".into());
        assert!(!state.refresh_manual("",219.0));assert!(state.located.is_some());
        state.loading=true;
        assert!(!state.refresh_manual("",1000.0));assert!(state.located.is_some());
    }

    #[test]
    fn weather_requests_use_the_selected_proxy(){
        use std::io::Write;
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy=format!("http://{}",listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let worker=std::thread::spawn(move||{
            let start=std::time::Instant::now();
            while start.elapsed()<Duration::from_secs(3) {
                if let Ok((mut stream,_))=listener.accept() {
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                    let mut request=[0;4096];let n=stream.read(&mut request).unwrap();
                    let request=String::from_utf8_lossy(&request[..n]).into_owned();
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").unwrap();
                    return request;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("天气请求未到达指定代理");
        });
        let url=url::Url::parse("http://127.0.0.1:1/weather").unwrap();
        let result=get(&http_agent_for(Some(&proxy)),&url,"天气预报");
        let request=worker.join().unwrap();
        assert!(request.starts_with("GET http://127.0.0.1:1/weather "));
        assert_eq!(result.unwrap()["ok"],true);
    }

    #[test]
    fn regression_weather_errors_identify_the_failed_service(){
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url=url::Url::parse(&format!("http://{}",listener.local_addr().unwrap())).unwrap();
        drop(listener);
        let agent=ureq::AgentBuilder::new().try_proxy_from_env(false).timeout(Duration::from_secs(1)).build();
        for service in ["自动定位","城市搜索","天气预报"] {
            let error=get_with_proxy(&agent,&url,service,None).unwrap_err();
            assert!(error.contains(service),"错误需要区分失败环节：{error}");
        }
    }

    #[test]
    fn weather_offline_auto_refresh_backs_off_and_recovers(){
        let mut state=State{last_city:Some(String::new()),last_start:0.0,..Default::default()};
        let now=10_000;
        assert!(state.should_refresh("",200.0,now,true),"无数据时照常刷新");
        for (failures,wait) in [(1,120.0),(2,240.0),(3,480.0),(4,960.0),(5,1800.0),(9,1800.0)] {
            state.failures=failures;state.error=Some("网络不可用".into());
            assert_eq!(state.backoff(),wait);
            assert!(!state.should_refresh("",wait-1.0,now,true),"第 {failures} 次失败后 {wait} 秒内不应自动重试");
            assert!(state.should_refresh("",wait,now,true));
        }
        assert!(state.can_refresh("",120.0),"手动刷新不受自动退避限制");
        let (tx,rx)=mpsc::channel();state.rx=Some((rx,true));state.loading=true;
        tx.send(Ok(Snapshot{position:Some(Position{lat:30.0,lon:120.0,label:"杭州".into(),region:String::new()}),at:now,..Default::default()})).unwrap();
        assert!(state.poll("",true));assert_eq!(state.failures,0);assert_eq!(state.backoff(),0.0);
        assert!(state.located.as_ref().is_some_and(|(p,at)|p.label=="杭州"&&*at==now),"自动定位结果应缓存复用");
        let position=state.snapshot.as_ref().unwrap().position.clone();
        let (tx,rx)=mpsc::channel();state.rx=Some((rx,true));
        tx.send(Ok(Snapshot{position:position.clone(),at:now+LOCATE_TTL+1,..Default::default()})).unwrap();
        assert!(state.poll("",true));assert_eq!(state.located.as_ref().unwrap().1,now+LOCATE_TTL+1);
        let (tx,rx)=mpsc::channel();state.rx=Some((rx,false));
        tx.send(Ok(Snapshot{position,at:now+LOCATE_TTL+900,..Default::default()})).unwrap();
        assert!(state.poll("",true));assert_eq!(state.located.as_ref().unwrap().1,now+LOCATE_TTL+1);
        state.forget_location();assert!(state.located.is_none());
    }

    #[test]
    fn weather_manual_city_requires_a_confirmed_region(){
        let mut state=State::load("杭州",None);
        state.city_input="佛山".into();
        assert_eq!(state.draft_city(),None,"同名地点未确认前不能直接应用搜索文字");
    }

    #[test]
    fn weather_location_mode_keeps_blank_manual_input_separate_from_auto(){
        let position=Position{lat:30.27,lon:120.15,label:"杭州".into(),region:"浙江 · 中国".into()};
        let mut state=State::load("杭州",Some(&position));
        assert!(state.manual_city);assert_eq!(state.draft_city(),Some("杭州"));
        state.city_input="  ".into();assert_eq!(state.draft_city(),None);
        state.manual_city=false;assert_eq!(state.draft_city(),Some(""));
        let mut old=serde_json::to_value(parse("杭州",&sample()).unwrap()).unwrap();
        for day in old["days"].as_array_mut().unwrap(){day.as_object_mut().unwrap().remove("date");}
        let cached:Snapshot=serde_json::from_value(old).unwrap();
        assert!(cached.days.iter().all(|day|day.date.is_empty()),"旧缓存缺少日期时不能猜测日期");
    }

    #[test]
    fn weather_foshan_candidates_include_the_full_city_name_and_require_selection(){
        assert_eq!(city_queries("佛山"),vec!["佛山市","佛山"]);
        assert_eq!(city_queries("佛山市"),vec!["佛山市","佛山"]);
        assert_eq!(city_queries("广州市"),vec!["广州市","广州"],"带「市」也要能搜到只收录简称的城市");
        assert_eq!(city_queries("杭州"),vec!["杭州"]);
        assert_eq!(city_queries("新疆"),vec!["新疆市","新疆","乌鲁木齐"],"地区名查不到时用省会代替");
        assert!(city_queries("新疆维吾尔自治区").contains(&"乌鲁木齐".to_string()));
        assert!(city_queries("广东省").contains(&"广州".to_string()));
        assert_eq!(city_queries("北京"),vec!["北京市","北京"],"直辖市不重复查");assert_eq!(city_queries("津市"),vec!["津市市","津市"]);
        assert_eq!(city_queries("London"),vec!["London"]);
        let candidates:Vec<Position>=city_candidates(&json!({"results":[
            {"name":"佛山","latitude":28.73401,"longitude":98.80612,"admin1":"云南","admin2":"迪庆藏族自治州","country":"中国"},
            {"name":"佛山市","latitude":23.02677,"longitude":113.13148,"admin1":"广东","admin2":"佛山市","country":"中国"},
            {"name":"坏坐标","latitude":999,"longitude":113},
            {"name":"缺坐标"},
            {"name":"佛山沙堤机场","latitude":23.08,"longitude":113.07,"feature_code":"AIRP"}
        ]})).into_iter().map(|(p,_)|p).collect();
        assert_eq!(candidates.len(),2);
        assert_eq!(candidates[0].region,"迪庆藏族自治州 · 云南 · 中国");
        assert_eq!(candidates[1].region,"广东 · 中国");
        assert!(unique_city(candidates.clone(),"佛山").unwrap_err().contains("同名"));
        assert_eq!(unique_city(vec![candidates[1].clone()],"佛山市").unwrap().lat,23.02677);
        assert!(unique_city(vec![],"未知城市").is_err());
        // 完全同名的排前面，其次人口多的；坐标重合的同名地点只留一个。
        let town=Position{lat:28.7,lon:98.8,label:"广州镇".into(),region:"云南".into()};
        let city=Position{lat:23.13,lon:113.26,label:"广州".into(),region:"广东".into()};
        let ranked=rank_cities("广州市",vec![(town.clone(),900_000),(city.clone(),13_000_000),(city.clone(),13_000_000)]);
        assert_eq!(ranked,vec![city,town]);
    }

    #[test]
    fn weather_confirmed_coordinates_survive_restart_and_reject_the_wrong_cache(){
        let selected=Position{lat:23.02677,lon:113.13148,label:"佛山市".into(),region:"广东 · 中国".into()};
        let saved:Position=serde_json::from_str(&serde_json::to_string(&selected).unwrap()).unwrap();
        let state=State::load("佛山市",Some(&saved));
        assert_eq!(state.draft_city(),Some("佛山市"));assert_eq!(state.active_position.as_ref(),Some(&selected));
        let mut old=Snapshot{place:"佛山".into(),code:73,temp:-8.7,..Default::default()};
        assert!(!cache_matches(&old,"佛山市",Some(&selected)),"无坐标的旧中雪缓存不能冒充佛山市天气");
        old.position=Some(Position{lat:28.73401,lon:98.80612,label:"佛山市".into(),region:"云南".into()});
        assert!(!cache_matches(&old,"佛山市",Some(&selected)));
        old.position=Some(selected.clone());assert!(cache_matches(&old,"佛山市",Some(&selected)));
        assert!(!cache_matches(&old,"佛山",None));
    }

    #[test]
    fn weather_same_named_regions_trigger_a_refresh_and_discard_the_previous_response(){
        let first=Position{lat:28.73401,lon:98.80612,label:"佛山".into(),region:"云南".into()};
        let second=Position{lat:30.04572,lon:106.41138,label:"佛山".into(),region:"重庆".into()};
        let (tx,rx)=mpsc::channel();
        let mut state=State{last_city:Some("佛山".into()),last_position:Some(first.clone()),active_position:Some(first),selected_city:Some(second.clone()),city_input:"佛山".into(),manual_city:true,last_start:100.0,rx:Some((rx,false)),..Default::default()};
        assert!(state.can_apply(101.0),"同名但不同地区的选择可以立即应用");
        state.active_position=Some(second);assert!(state.city_changed("佛山"));
        tx.send(Ok(Snapshot{code:73,..Default::default()})).unwrap();assert!(state.poll("佛山",true));assert!(state.snapshot.is_none());
    }

    #[test]
    fn weather_search_ignores_results_for_the_previous_input(){
        let (tx,rx)=mpsc::channel();
        let mut search=CitySearch{query:"佛山".into(),requested:Some("佛山".into()),rx:Some(rx),..Default::default()};
        tx.send(("佛山".into(),Ok(vec![Position{lat:23.0,lon:113.0,label:"佛山市".into(),region:"广东".into()}]))).unwrap();
        search.update("杭州",1.0);assert!(search.results.is_empty());assert!(search.error.is_none());assert_eq!(search.changed_at,1.0);
        search.update(" 杭州 ",1.1);assert_eq!(search.changed_at,1.0,"首尾空格不重置输入防抖");
    }

    #[test]
    fn weather_manual_refresh_respects_success_and_failure_cooldowns(){
        for (error,cooldown) in [(None,30.0),(Some("网络不可用".to_string()),120.0)] {
            let mut state=State {last_start:100.0,last_city:Some("杭州".into()),error,..Default::default()};
            state.refresh("杭州",100.0+cooldown-1.0);
            assert!(!state.loading,"手动刷新也应遵守 {cooldown} 秒请求间隔");
            assert!(state.can_refresh("杭州",100.0+cooldown));
        }
    }

    #[test]
    fn weather_scheduler_stops_when_disabled_and_bounds_retries(){
        let mut state=State::default();
        assert!(state.should_refresh("杭州",0.0,1000,true));
        assert!(!state.should_refresh("杭州",0.0,1000,false));
        state.last_city=Some("杭州".into());state.last_start=0.0;
        assert!(!state.can_refresh("杭州",29.0),"启动时刻为零也必须限流");
        assert!(state.should_refresh("杭州",30.0,1000,true));
        state.error=Some("网络不可用".into());
        assert!(!state.should_refresh("杭州",119.0,1000,true));
        assert!(state.should_refresh("杭州",120.0,1000,true));
        assert!(state.can_refresh("北京",1.0),"确认其他城市后可以立即查询");
        state.loading=true;
        assert!(!state.can_refresh("北京",1000.0),"在途请求结束前不能堆积线程");
        state.loading=false;state.error=None;
        state.snapshot=Some(Snapshot{at:1000,..Default::default()});
        assert!(!state.should_refresh("杭州",1000.0,1000+INTERVAL-1,true));
        assert!(state.should_refresh("杭州",1000.0,1000+INTERVAL,true));
    }

    #[test]
    fn weather_late_results_do_not_restore_disabled_or_changed_locations(){
        for (city,enabled) in [("杭州",false),("北京",true)] {
            let (tx,rx)=mpsc::channel();
            let path=std::env::temp_dir().join(format!("daozai-weather-discard-{}-{enabled}.json",std::process::id()));
            let mut state=State{snapshot:Some(Snapshot{place:"上次天气".into(),..Default::default()}),
                loading:true,last_city:Some("杭州".into()),rx:Some((rx,false)),cache:Some(path.clone()),..Default::default()};
            tx.send(Ok(Snapshot{place:"杭州".into(),..Default::default()})).unwrap();
            assert!(state.poll(city,enabled));
            assert!(!state.loading);
            assert_eq!(state.snapshot.as_ref().unwrap().place,"上次天气");
            assert!(state.error.is_none());assert!(!path.exists(),"丢弃的响应不能落盘");
        }
    }

    #[test]
    fn weather_current_result_updates_the_snapshot_and_clears_errors(){
        let (tx,rx)=mpsc::channel();
        let mut state=State{last_city:Some("杭州".into()),loading:true,rx:Some((rx,false)),error:Some("旧错误".into()),..Default::default()};
        tx.send(Ok(Snapshot{place:"杭州".into(),temp:9.0,..Default::default()})).unwrap();
        assert!(state.poll(" 杭州 ",true));
        assert_eq!(state.snapshot.as_ref().unwrap().temp,9.0);
        assert!(!state.loading && state.error.is_none());assert!(!state.poll("杭州",true));
    }

    fn sample() -> Value {
        json!({
            "current": {"temperature_2m": 9.5, "relative_humidity_2m": 58, "apparent_temperature": 5.6, "is_day": 0, "weather_code": 0, "wind_speed_10m": 13.9},
            "daily": {
                "time": ["2026-11-09", "2026-11-10", "2026-11-11", "2026-11-12"],
                "weather_code": [51, 2, 2, 61],
                "temperature_2m_max": [15.2, 14.5, 17.4, 19.2],
                "temperature_2m_min": [6.8, 4.7, 5.0, 8.2],
                "precipitation_probability_max": [60, 0, 0, 25],
                "sunrise": ["2026-11-09T06:52", "2026-11-10T06:53", "2026-11-11T06:54", "2026-11-12T06:55"],
                "sunset": ["2026-11-09T17:21", "2026-11-10T17:20", "2026-11-11T17:19", "2026-11-12T17:18"]
            }
        })
    }

    #[test]
    fn parses_current_and_four_days() {
        let s = parse("杭州", &sample()).unwrap();
        assert_eq!(s.place, "杭州");
        assert_eq!(s.temp, 9.5);
        assert_eq!(s.feels, Some(5.6));
        assert_eq!(s.humidity, Some(58));
        assert_eq!(s.wind, Some(13.9));
        assert!(!s.day, "is_day=0 应识别为夜间");
        assert_eq!(s.high, Some(15.2));
        assert_eq!(s.low, Some(6.8));
        assert_eq!(s.days.len(), 4);
        assert_eq!(s.days[0].label, "今天");
        assert_eq!(s.days[1].label, "明天");
        assert_eq!(s.days[2].label, "周三");
        assert_eq!(s.days[3].rain, 25);
        assert_eq!(s.sunrise.as_deref(), Some("06:52"), "应截取日出时间的 HH:MM");
        assert_eq!(s.sunset.as_deref(), Some("17:21"));
    }

    #[test]
    fn tolerates_payload_without_sunrise() {
        let s = parse("杭州", &json!({"current": {"temperature_2m": 3.0}, "daily": {"time": ["2026-11-09"]}})).unwrap();
        assert_eq!(s.sunrise, None);
        assert_eq!(s.sunset, None, "旧缓存与新接口都应容忍缺日出日落");
    }

    #[test]
    fn weather_parses_seven_days_and_keeps_older_caches_readable() {
        let mut payload=sample();
        payload["daily"]["time"]=json!(["2026-11-09","2026-11-10","2026-11-11","2026-11-12","2026-11-13","2026-11-14","2026-11-15"]);
        payload["daily"]["temperature_2m_max"]=json!([15,14,17,19,20,21,22]);
        payload["daily"]["temperature_2m_min"]=json!([6,4,5,8,9,10,11]);
        let snapshot=parse("杭州",&payload).unwrap();
        assert_eq!(snapshot.days.len(),7);
        assert_eq!(snapshot.days[6].label,"周日");
        assert_eq!(snapshot.days[6].high,22.0);
        let mut cached=serde_json::to_value(parse("杭州",&sample()).unwrap()).unwrap();
        cached.as_object_mut().unwrap().remove("sun_times");
        let older:Snapshot=serde_json::from_value(cached).unwrap();
        assert_eq!(older.days.len(),4);
        assert_eq!(older.sunrise.as_deref(),Some("06:52"));
        assert!(older.sun_times.is_none());
    }

    #[test]
    fn weather_solar_progress_uses_the_location_timezone_and_actual_day_length() {
        use chrono::TimeZone;
        let mut payload=sample();payload["timezone"]=json!("Asia/Shanghai");
        let times=parse("杭州",&payload).unwrap().sun_times.unwrap();
        assert_eq!(times.sunrise,chrono::Utc.with_ymd_and_hms(2026,11,8,22,52,0).unwrap().timestamp());
        assert_eq!(times.clock(times.midpoint()),"12:06");
        assert_eq!(times.sunset-times.sunrise,10*3600+29*60);
        for (at,expected) in [(times.sunrise,0.0),(times.midpoint(),0.5),(times.sunset,1.0)] {
            assert!((times.progress(at).unwrap()-expected).abs()<0.0001);
        }
        assert!(times.progress(times.sunrise-1).is_none());
        assert!(times.progress(times.sunset+1).is_none());
        assert!(times.progress(times.day_start-1).is_none());
        assert!(times.progress(times.day_end).is_none());
        payload["timezone"]=json!("America/New_York");
        payload["daily"]["time"][0]=json!("2026-11-01");
        payload["daily"]["sunrise"][0]=json!("2026-11-01T06:30");
        payload["daily"]["sunset"][0]=json!("2026-11-01T16:50");
        let times=parse("纽约",&payload).unwrap().sun_times.unwrap();
        assert_eq!(times.day_end-times.day_start,25*3600,"夏令时结束当天有25小时");
        assert_eq!(times.sunrise,chrono::Utc.with_ymd_and_hms(2026,11,1,11,30,0).unwrap().timestamp());
        assert_eq!(times.clock(times.sunrise),"06:30");
    }

    #[test]
    fn weather_missing_or_invalid_solar_times_do_not_invent_progress() {
        for (zone,rise,set) in [(json!("Asia/Shanghai"),Value::Null,Value::Null),(json!("Asia/Shanghai"),json!("2026-11-09T18:00"),json!("2026-11-09T06:00")),(Value::Null,json!("2026-11-09T06:52"),json!("2026-11-09T17:21"))] {
            let mut payload=sample();payload["timezone"]=zone;
            payload["daily"]["sunrise"][0]=rise;payload["daily"]["sunset"][0]=set;
            assert!(parse("当地",&payload).unwrap().sun_times.is_none());
        }
    }

    #[test]
    fn rejects_payload_without_temperature() {
        assert!(parse("杭州", &json!({"daily": {"time": ["2026-11-09"]}})).is_err());
        assert!(parse("杭州", &json!({"current": {"temperature_2m": 3.0}})).is_err(), "没有日预报也不应假装成功");
    }

    #[test]
    fn kind_and_text_follow_wmo_codes() {
        assert_eq!(kind(0, true), Kind::Sun);
        assert_eq!(kind(0, false), Kind::Moon);
        assert_eq!(kind(2, false), Kind::PartlyNight);
        assert_eq!(kind(95, true), Kind::Thunder);
        assert_eq!(kind(75, true), Kind::Snow);
        assert_eq!(kind(73, true), Kind::Snow);
        assert_eq!(kind(97, true), Kind::Thunder);
        assert_eq!(describe(0), "晴");
        assert_eq!(describe(63), "中雨");
        assert_eq!(describe(73), "中雪");
        assert_eq!(describe(53), "毛毛雨");
        assert_eq!(describe(97), "强雷阵雨");
        assert_eq!(describe(200), "多云", "未知代码退回多云");
    }

    #[test]
    fn stale_after_fifteen_minutes() {
        let mut state = State::default();
        assert!(state.stale(0), "还没有数据时应刷新");
        state.snapshot = Some(Snapshot { at: 1_000, ..Default::default() });
        assert!(!state.stale(1_000 + INTERVAL - 1));
        assert!(state.stale(1_000 + INTERVAL));
    }

    #[test]
    fn city_change_triggers_a_new_request() {
        let mut state = State::default();
        assert!(state.city_changed(""), "首次启动应请求一次");
        state.last_city = Some(String::new());
        assert!(!state.city_changed(""), "城市没变就不该重复请求");
        assert!(state.city_changed("杭州"));
        state.last_city = Some("杭州".into());
        assert!(!state.city_changed(" 杭州 "), "首尾空格不应算作换城市");
    }


    #[test]
    #[ignore="按需联网验证主定位不可用时的备用定位与天气"]
    fn weather_backup_live_probe(){
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let unavailable=format!("http://{}",listener.local_addr().unwrap());drop(listener);
        let agent=http_agent();
        let position=locate_with_sources(&agent,&[(unavailable.as_str(),false),("https://free.freeipapi.com/api/v1/json",true)],None).expect("备用定位直连失败");
        let snapshot=fetch_forecast(&agent,position).expect("备用定位后的天气查询失败");
        assert_eq!(snapshot.days.len(),FORECAST_DAYS);
        println!("backup_place={} temp={} forecast_days={}",snapshot.place,snapshot.temp,snapshot.days.len());
    }

    #[test]
    #[ignore="按需联网核验本机定位与天气"]
    fn weather_local_probe(){
        let agent = http_agent();
        let auto = fetch(&agent, "").expect("自动定位失败");
        eprintln!("auto_place={} temp={:.1} code={} sunrise={:?} sunset={:?} days={:?}", auto.place, auto.temp, auto.code, auto.sunrise, auto.sunset,
            auto.days.iter().map(|d| (&d.label, d.high, d.low, d.rain)).collect::<Vec<_>>());
        let candidates=search_cities(&agent,"杭州").expect("城市搜索失败");
        let selected=candidates.iter().find(|p|p.label=="杭州" && p.region.contains("浙江")).expect("缺少浙江杭州候选");
        let named = fetch_at(&agent, "杭州", Some(selected)).expect("按城市查询失败");
        println!("named_place={} temp={:.1}", named.place, named.temp);
    }

    #[test]
    #[ignore="按需联网核验佛山的同名候选与确认坐标后的实际天气"]
    fn weather_foshan_live_probe(){
        let agent=http_agent();let candidates=search_cities(&agent,"佛山").expect("搜索失败");
        assert!(unique_city(candidates.clone(),"佛山").is_err());
        let city=candidates.iter().find(|p|p.label=="佛山市" && p.region.contains("广东")).expect("缺少广东佛山市候选");
        let snapshot=fetch_at(&agent,&city.label,Some(city)).expect("天气查询失败");
        assert_eq!(snapshot.position.as_ref(),Some(city));
        println!("city={} region={} lat={} lon={} temp={} code={} text={} days={}",city.label,city.region,city.lat,city.lon,snapshot.temp,snapshot.code,describe(snapshot.code),snapshot.days.len());
    }

    #[test]
    fn clock_formats_local_time() {
        let text = clock(0);
        assert_eq!(text.len(), 5);
        assert!(text.contains(':'));
        let _ = clock(i64::MAX); // 极端时间只应退化成提示，不应崩溃
    }
}
