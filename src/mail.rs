//! 邮件（v0.5.5）：六类邮箱的账号模型、IMAP 收取、正文解析。
//!
//! 轻量化约束：只用 Rust 原生 crate（`imap` + `native-tls` + `mailparse`），
//! 不引 Node/Go/Electron。凭据一律进 Windows 凭据管理器（`keyring`），
//! 账号清单只存「邮箱 + 类型 + 是否监控」，不存密码。
//!
//! 收信优先 `Session::idle()` 长连接；服务器不支持 IDLE 时退回轮询（60/120/300 秒）。
//! 正文只在内存里解析，不落盘、不缓存。

use std::collections::HashMap;
#[cfg(windows)]
#[path="mail_fetch.rs"]
pub(crate) mod fetch;

pub const SERVICE: &str = "Daozai.Mail";
pub const MICROSOFT_UNSUPPORTED: &str = "Outlook、Hotmail、Live 和 Microsoft 365 云邮箱需要 OAuth 授权，岛仔暂不支持密码直连。";

// ---------------------------------------------------------------- 邮箱类型

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Qq,
    Mail163,
    Mail126,
    Exmail,
    ICloud,
    Gmail,
    Microsoft,
    Custom,
}

impl Default for Provider {
    fn default() -> Self {
        Provider::Qq
    }
}

impl Provider {
    pub const ALL: [Provider; 8] = [
        Provider::Qq,
        Provider::Mail163,
        Provider::Mail126,
        Provider::Exmail,
        Provider::ICloud,
        Provider::Gmail,
        Provider::Microsoft,
        Provider::Custom,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Provider::Qq => "qq",
            Provider::Mail163 => "163",
            Provider::Mail126 => "126",
            Provider::Exmail => "exmail",
            Provider::ICloud => "icloud",
            Provider::Gmail => "gmail",
            Provider::Microsoft => "microsoft",
            Provider::Custom => "custom",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Qq => "QQ 邮箱",
            Provider::Mail163 => "163 邮箱",
            Provider::Mail126 => "126 邮箱",
            Provider::Exmail => "腾讯企业邮",
            Provider::ICloud => "iCloud 邮箱",
            Provider::Gmail => "Gmail",
            Provider::Microsoft => "Outlook / Hotmail（暂不支持）",
            Provider::Custom => "自定义 IMAP",
        }
    }

    pub fn host(self) -> &'static str {
        match self {
            Provider::Qq => "imap.qq.com",
            Provider::Mail163 => "imap.163.com",
            Provider::Mail126 => "imap.126.com",
            Provider::Exmail => "imap.exmail.qq.com",
            Provider::ICloud => "imap.mail.me.com",
            Provider::Gmail => "imap.gmail.com",
            Provider::Microsoft => "outlook.office365.com",
            Provider::Custom => "",
        }
    }

    pub fn port(self) -> u16 {
        993
    }

    /// 163 / 126 在 LOGIN 之前必须发 RFC 2971 的 `ID`，否则会被判 Unsafe Login。
    pub fn needs_id(self) -> bool {
        matches!(self, Provider::Mail163 | Provider::Mail126)
    }

    /// 设置界面的操作指引。
    pub fn hint(self) -> &'static str {
        match self {
            Provider::Microsoft => MICROSOFT_UNSUPPORTED,
            Provider::Custom => "向学校或单位管理员确认 IMAP 主机、TLS 端口及用户名。仅支持连接即加密的 IMAP over TLS（通常 993）与 LOGIN/应用专用密码。严格校验证书；不支持明文、STARTTLS、OAuth2 或网页登录/SSO。修改服务器或用户名必须重新输入该目标的授权码。",
            Provider::Qq => "1. 登录 QQ 邮箱网页版，点右上角「设置」→ 左侧「账号与安全」。\n2. 在新页面选「安全设置」，向下找到「POP3/IMAP/SMTP/Exchange/CardDAV 服务」；未开启时先按提示开启。\n3. 点「生成授权码」，完成验证后复制到下方。这里填写授权码，不是 QQ 密码或邮箱独立密码。",
            Provider::Mail163 | Provider::Mail126 => "1. 登录对应的网易邮箱网页版，点顶部「设置」→「POP3/SMTP/IMAP」。\n2. 确认「IMAP/SMTP 服务」已开启；首次开启时按提示完成验证。\n3. 在「授权密码管理」中点「新增授权密码」，完成验证后复制到下方。填写客户端授权码，不是邮箱登录密码。",
            Provider::Exmail => "1. 登录腾讯企业邮网页版，在「设置」→「收发信设置」检查 IMAP/SMTP 服务。\n2. 常见网页版在「设置」→「邮箱绑定」→「客户端专用密码」→「生成新密码」获取密码；启用安全登录后须使用专用密码。\n3. 企业版本和管理员策略可能不同。找不到入口或服务被禁用时，请让企业管理员确认客户端接入方式，不要照搬 QQ 邮箱的路径。",
            Provider::ICloud => "1. 登录 Apple 账户页面，进入「登录和安全」→「App 专用密码」。\n2. 选择「生成 App 专用密码」，按提示填写名称并完成验证；账户需已开启双重认证。\n3. 邮箱栏填写完整的 iCloud 邮箱地址，下方填写刚生成的 App 专用密码，不是 Apple 账户密码。",
            Provider::Gmail => "1. 登录 Google 账户，进入「安全性与登录」→「您的 Google 账号登录选项」→「开启两步验证」，按提示完成设置。\n2. 打开下方「应用专用密码」页面，输入应用名称（例如：岛仔）并创建，复制生成的 16 位应用专用密码。\n3. 邮箱栏填完整 Gmail 地址，下方填应用专用密码，不是 Google 登录密码。若没有创建入口，请检查组织账号限制、高级保护或仅使用安全密钥的验证设置。",
        }
    }

    pub fn setup_link(self) -> (&'static str, &'static str) {
        match self {
            Provider::Qq => ("打开 QQ 邮箱网页版", "https://mail.qq.com/"),
            Provider::Mail163 => ("打开 163 邮箱网页版", "https://mail.163.com/"),
            Provider::Mail126 => ("打开 126 邮箱网页版", "https://mail.126.com/"),
            Provider::Exmail => ("打开腾讯企业邮网页版", "https://exmail.qq.com/"),
            Provider::ICloud => ("打开 Apple 账户", "https://account.apple.com/"),
            Provider::Gmail => ("打开应用专用密码", "https://myaccount.google.com/apppasswords"),
            Provider::Microsoft => ("打开 Outlook 网页版", "https://outlook.live.com/mail/"),
            Provider::Custom => ("", ""),
        }
    }

    pub fn parse(key: &str) -> Self {
        match key {
            "163" => Provider::Mail163,
            "126" => Provider::Mail126,
            "exmail" => Provider::Exmail,
            "icloud" => Provider::ICloud,
            "gmail" => Provider::Gmail,
            "microsoft" => Provider::Microsoft,
            "custom" => Provider::Custom,
            _ => Provider::Qq,
        }
    }
}

// ---------------------------------------------------------------- 账号与状态

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub email: String,
    pub provider: Provider,
    pub custom: Option<CustomImap>,
    /// 是否监控这个账号（设置里可逐个开关）。
    pub enabled: bool,
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct CustomImap { pub host:String, pub port:u16, pub username:String }
impl Default for CustomImap {fn default()->Self{Self{host:String::new(),port:993,username:String::new()}}}
impl Account {
    pub fn endpoint(&self)->Result<(String,u16,String),String>{
        let email=self.email.trim().to_ascii_lowercase();
        let domain=email.rsplit_once('@').map(|(_,domain)|domain).unwrap_or("");
        if self.provider==Provider::Microsoft||matches!(domain,"outlook.com"|"hotmail.com"|"live.com"|"msn.com"){
            return Err(MICROSOFT_UNSUPPORTED.into());
        }
        if self.provider!=Provider::Custom {return Ok((self.provider.host().into(),self.provider.port(),self.email.clone()));}
        let c=self.custom.as_ref().ok_or("请填写自定义 IMAP 连接配置")?;
        let host=c.host.trim().to_ascii_lowercase();
        if matches!(host.as_str(),"outlook.office365.com"|"imap-mail.outlook.com"|"outlook.live.com"|"imap.outlook.com"){
            return Err(MICROSOFT_UNSUPPORTED.into());
        }
        if host.is_empty() || host.len()>253 || !host.split('.').all(|label|!label.is_empty() && label.len()<=63 && !label.starts_with('-') && !label.ends_with('-') && label.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')) {
            return Err("IMAP 主机必须是域名或 IPv4 地址，不要填写网址、端口、路径或账号".into());
        }
        if c.port==0 || c.port==143{return Err("仅支持 IMAP over TLS（通常 993），不支持 143/STARTTLS 或明文连接".into());}
        let user=if c.username.trim().is_empty(){self.email.trim()}else{c.username.trim()};
        if user.is_empty() || user.len()>256 || user.chars().any(char::is_control){return Err("IMAP 用户名无效".into());}
        Ok((host,c.port,user.into()))
    }
    /// Custom secrets are bound to the TLS endpoint and exact login identity.
    pub fn credential_key(&self)->String{
        if self.provider!=Provider::Custom{return key(self);}
        let identity=match self.endpoint(){Ok((host,port,user))=>format!("{}\0{host}\0{port}\0{user}",key(self)),Err(_)=>"invalid-custom-endpoint".into()};
        let digest=ring::digest::digest(&ring::digest::SHA256,identity.as_bytes());
        format!("custom-{}",digest.as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>())
    }
}

pub fn key(account: &Account) -> String {
    account.email.trim().to_ascii_lowercase()
}

fn accounts_path() -> std::path::PathBuf {
    crate::model::config_dir().join("mail-accounts.json")
}

pub fn load_accounts() -> Vec<Account> {
    match std::fs::read(accounts_path()) {
        Ok(raw) => decode_accounts(&raw),
        Err(_) => Vec::new(),
    }
}

/// 纯解析：账号清单里只有邮箱、类型、开关，没有任何密码字段。
pub fn decode_accounts(raw: &[u8]) -> Vec<Account> {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    json.as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let email = a.get("email")?.as_str()?.trim().to_string();
            if email.is_empty() {
                return None;
            }
            Some(Account {
                custom:a.get("custom").and_then(|c|Some(CustomImap{host:c.get("host")?.as_str()?.into(),port:u16::try_from(c.get("port")?.as_u64()?).ok()?,username:c.get("username").and_then(|u|u.as_str()).unwrap_or("").into()})),
                provider: Provider::parse(a.get("provider").and_then(|p| p.as_str()).unwrap_or("qq")),
                enabled: a.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true),
                email,
            })
        })
        .collect()
}

pub fn encode_accounts(items: &[Account]) -> Vec<u8> {
    let list: Vec<_> = items
        .iter()
        .map(|a| serde_json::json!({"email": a.email, "provider": a.provider.key(), "enabled": a.enabled,"custom":a.custom.as_ref().map(|c|serde_json::json!({"host":c.host,"port":c.port,"username":c.username}))}))
        .collect();
    serde_json::to_vec(&list).unwrap_or_default()
}

pub fn save_accounts(items: &[Account]) {
    let data = encode_accounts(items);
    if data.is_empty() {
        return;
    }
    let path = accounts_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = crate::model::atomic_write(&path, &data);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Connecting,
    Ready,
    /// 授权码缺失或失效。
    Auth(String),
    Failed(String),
}

impl Status {
    pub fn label(&self) -> String {
        match self {
            Status::Idle => "未连接".into(),
            Status::Connecting => "连接中…".into(),
            Status::Ready => "已连接".into(),
            Status::Auth(msg) => format!("需要授权码：{msg}"),
            Status::Failed(msg) => format!("连接失败：{msg}"),
        }
    }
    pub fn ok(&self) -> bool {
        matches!(self, Status::Ready)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub account: String,
    pub from: String,
    pub subject: String,
    /// 本地时间 `MM-DD HH:MM`
    pub time: String,
    pub code: Option<String>,
    /// IMAP INTERNALDATE, used only to bound code display age.
    pub received_unix: Option<i64>,
    /// 完整纯文字正文，保留分段，仅保存在内存中。
    pub summary: std::sync::Arc<str>,
    /// HTML 原文（无 HTML 版本时为空）。排版层据此还原标题、列表、引用等结构；
    /// `summary` 只作为纯文本邮件的兜底，两者都只在内存中保存。
    pub html: std::sync::Arc<str>,
    pub links: Vec<Link>,
    /// 会议邀请（来自 .ics / text/calendar），没有就是 `None`。
    pub meeting: Option<Meeting>,
    pub uid: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub label: String,
    pub url: String,
}

/// 仅把有效网页链接交给默认浏览器。
pub fn web_url(raw:&str)->Option<String> {
    let parsed=url::Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(),"https"|"http") || parsed.host_str().is_none(){return None;}
    Some(parsed.into())
}

pub fn links_of(parsed:&Parsed)->Vec<Link> {
    use std::sync::OnceLock;
    static ANCHOR:OnceLock<regex::Regex>=OnceLock::new();
    static URL:OnceLock<regex::Regex>=OnceLock::new();
    let anchor=ANCHOR.get_or_init(||regex::Regex::new(r#"(?is)<a\b[^>]*?\s+href\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))[^>]*>(.*?)</a\s*>"#).unwrap());
    let url=URL.get_or_init(||regex::Regex::new(r#"(?i)https?://[^\s<>"'，。；！？、]+"#).unwrap());
    let mut links=Vec::new();let mut seen=std::collections::HashSet::new();
    let mut add=|raw:&str,label:String| {
        let decoded=otp::html_to_text(raw);
        if let Some(url)=web_url(decoded.trim()) {
            if seen.insert(url.clone()) {links.push(Link{label:if label.trim().is_empty(){url.clone()}else{label.trim().into()},url});}
        }
    };
    for cap in anchor.captures_iter(&parsed.html) {
        if let Some(href)=cap.get(1).or_else(||cap.get(2)).or_else(||cap.get(3)) {
            add(href.as_str(),otp::html_to_text(&cap[4]));
        }
    }
    for text in [&parsed.text,&otp::html_to_text(&parsed.html)] {
        for hit in url.find_iter(text) {
            let mut target=hit.as_str().trim_end_matches(['.',',',';',':','!','，','。','；','！','？','）','】','》']);
            for (open,close) in [('(',')'),('[',']'),('{','}')] {
                while target.ends_with(close) && target.chars().filter(|c|*c==close).count()>target.chars().filter(|c|*c==open).count(){target=&target[..target.len()-1];}
            }
            add(target,target.into());
        }
    }
    links
}

/// 会议邀请里界面和提醒要用的字段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meeting {
    pub all_day: bool,
    pub title: String,
    /// 开始时间（Unix 秒，本地时区解释）。
    pub start: i64,
    /// 来源提供的结束时间；缺失或无效时不推测时长。
    pub end: Option<i64>,
    /// 本地时间 `MM-DD HH:MM`，直接显示。
    pub at: String,
    /// iCalendar 的 `UID`；邮件和日历两条来源靠它 + 开始时间去重。
    pub uid: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Status { account: String, status: Status },
    Mail { item: Item, new: bool },
    Test { account: String, ok: bool, note: String },
    Managed(crate::mail_manage::Completed),
}

// ---------------------------------------------------------------- 正文解析

pub struct Parsed {
    pub from: String,
    pub subject: String,
    /// text/plain 正文（没有则空）。
    pub text: String,
    /// text/html 正文转成的纯文本（没有则空）。
    pub html: String,
    /// text/calendar 正文（.ics 原文，没有则空）。
    pub calendar: String,
    pub emphasized: Vec<String>,
}

fn header(mail: &mailparse::ParsedMail, name: &str) -> String {
    mail.headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case(name))
        .map(|h| h.get_value().trim().to_string())
        .unwrap_or_default()
}

fn display_addr(raw: &str) -> String {
    mailparse::addrparse(raw)
        .ok()
        .and_then(|list| {
            list.iter().find_map(|a| match a {
                mailparse::MailAddr::Single(s) => Some(s.display_name.clone().unwrap_or_else(|| s.addr.clone())),
                mailparse::MailAddr::Group(g) => g.addrs.first().map(|s| s.display_name.clone().unwrap_or_else(|| s.addr.clone())),
            })
        })
        .unwrap_or_else(|| raw.trim().trim_matches('"').to_string())
}

fn collect(mail: &mailparse::ParsedMail, plain: &mut String, html: &mut String, calendar: &mut String) {
    if mail.subparts.is_empty() {
        let ct = mail.ctype.mimetype.to_ascii_lowercase();
        if ct == "text/plain" {
            if let Ok(body) = mail.get_body() {
                plain.push_str(&body);
                plain.push('\n');
            }
        } else if ct == "text/html" {
            if let Ok(body) = mail.get_body() {
                html.push_str(&body);
                html.push('\n');
            }
        } else if ct == "text/calendar" || ct == "application/ics" {
            if let Ok(body) = mail.get_body() {
                calendar.push_str(&body);
                calendar.push('\n');
            }
        }
    } else {
        for part in &mail.subparts {
            collect(part, plain, html, calendar);
        }
    }
}

/// 解析一封原始邮件（头部 + 正文），只保留界面上要用的字段。
pub fn parse_message(raw: &[u8]) -> Parsed {
    let Ok(mail) = mailparse::parse_mail(raw) else {
        return Parsed {
            from: String::new(),
            subject: String::new(),
            text: String::from_utf8_lossy(raw).into_owned(),
            html: String::new(),
            calendar: String::new(),
            emphasized: Vec::new(),
        };
    };
    let subject = header(&mail, "Subject");
    let from = display_addr(&header(&mail, "From"));
    let (mut plain, mut html, mut calendar) = (String::new(), String::new(), String::new());
    collect(&mail, &mut plain, &mut html, &mut calendar);
    let emphasized = if plain.trim().is_empty() {
        otp::emphasized(&html)
    } else {
        Vec::new()
    };
    Parsed { from, subject, text: plain, html, calendar, emphasized }
}

/// 优先纯文本；HTML 原文另外保留给阅读界面排版。
pub fn body_of(parsed:&Parsed)->String {
    let body = if !parsed.text.trim().is_empty() {
        parsed.text.clone()
    } else if !parsed.html.trim().is_empty() {
        otp::html_to_text(&parsed.html)
    } else {
        String::new()
    };
    body.replace("\r\n","\n").trim().to_string()
}

/// 紧凑场景使用的短摘要。
pub fn summary_of(parsed: &Parsed) -> String {
    let body=body_of(parsed);
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = flat.chars().take(200).collect();
    if flat.chars().count() > 200 {
        out.push('…');
    }
    out
}

/// 从解析结果里提验证码：先纯文本，纯文本没有再看 HTML 文本。
pub fn code_of(parsed: &Parsed) -> Option<String> {
    if let Some(c) = otp::extract(&parsed.subject, &parsed.text, &parsed.emphasized) {
        return Some(c.text);
    }
    if !parsed.html.trim().is_empty() {
        let text = otp::html_to_text(&parsed.html);
        if let Some(c) = otp::extract(&parsed.subject, &text, &parsed.emphasized) {
            return Some(c.text);
        }
    }
    None
}

/// 从 .ics 文本里取第一个 VEVENT 的标题与开始时间（邮件正文用）。
pub fn meeting_of(calendar: &str) -> Option<Meeting> {
    meetings_of(calendar).into_iter().next()
}

/// 从一段 iCalendar 里取出所有带时刻的 VEVENT。
///
/// CalDAV 的 `calendar-query` 一次会返回多个事件，邮件里的 `.ics` 通常只有一个；
/// 两者共用这一套解析；支持带时刻和全天日期，没有有效 DTSTART 就跳过。
pub fn meetings_of(calendar: &str) -> Vec<Meeting> {
    // RFC 5545 折行：以空格/制表符开头的行是上一行的续行。
    let mut lines: Vec<String> = Vec::new();
    for raw in calendar.lines() {
        let line = raw.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !lines.is_empty() {
            if let Some(last) = lines.last_mut() {
                last.push_str(&line[1..]);
            }
        } else {
            lines.push(line.to_string());
        }
    }

    let mut out = Vec::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut in_event = false;
    let mut cancelled = false;
    let mut nested = 0usize;
    for line in &lines {
        let up = line.to_ascii_uppercase();
        if up=="BEGIN:VCALENDAR" {cancelled=false;}
        if !in_event && up=="METHOD:CANCEL" {cancelled=true;}
        if up=="BEGIN:VEVENT" {
            in_event = true;
            nested=0;
            fields.clear();
            continue;
        }
        if up=="END:VEVENT" {
            if in_event && !cancelled {
                if let Some(m) = event_of(&fields) {
                    out.push(m);
                }
            }
            in_event = false;
            continue;
        }
        if !in_event {
            continue;
        }
        if up.starts_with("BEGIN:"){nested+=1;continue;}
        if up.starts_with("END:"){nested=nested.saturating_sub(1);continue;}
        if nested>0 {continue;}
        let Some((head, value)) = line.split_once(':') else { continue };
        let name = head.split(';').next().unwrap_or("").to_ascii_uppercase();
        if name == "DTSTART" || name == "DTEND" {
            if let Some(tz) = head.split(';').skip(1).find_map(|p| p.split_once('=').filter(|(k, _)| k.eq_ignore_ascii_case("TZID")).map(|(_, v)| v.trim_matches('"'))) {
                fields.push((format!("{name}-TZID"), tz.to_string()));
            }
        }
        fields.push((name, value.trim().to_string()));
    }
    out
}

fn event_of(fields: &[(String, String)]) -> Option<Meeting> {
    use chrono::TimeZone;
    let get = |name: &str| fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    if get("STATUS").is_some_and(|v|v.eq_ignore_ascii_case("CANCELLED")){return None;}
    let parse_time = |name: &str| -> Option<chrono::DateTime<chrono::Local>> {
        let value = get(name)?;
        Some(if let Some(s) = value.strip_suffix('Z') {
            chrono::NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S").ok()?.and_utc().with_timezone(&chrono::Local)
        } else {
            if !value.contains('T') {
                return chrono::NaiveDate::parse_from_str(&value,"%Y%m%d").ok()?.and_hms_opt(0,0,0)?.and_local_timezone(chrono::Local).earliest();
            }
            let n = chrono::NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%S").ok()?;
            if let Some(tz) = get(&format!("{name}-TZID")) {
                let zone: chrono_tz::Tz = tz.parse().ok()?;
                zone.from_local_datetime(&n).single()?.with_timezone(&chrono::Local)
            } else {
                chrono::Local.from_local_datetime(&n).single()?
            }
        })
    };
    let naive = parse_time("DTSTART")?;
    let all_day=get("DTSTART").is_some_and(|value|!value.contains('T'));
    let end = parse_time("DTEND").map(|at|at.timestamp()).filter(|end|*end>naive.timestamp())
        .or_else(||all_day.then(||naive.date_naive().succ_opt()?.and_hms_opt(0,0,0)?.and_local_timezone(chrono::Local).earliest().map(|at|at.timestamp())).flatten());
    let title = get("SUMMARY").map(|v| ics_text(&v)).filter(|t| !t.is_empty()).unwrap_or_else(|| "会议".to_string());
    Some(Meeting {
        all_day,
        title,
        start: naive.timestamp(),
        end,
        at: naive.format(if all_day {"%m-%d 全天"}else{"%m-%d %H:%M"}).to_string(),
        uid: get("UID").unwrap_or_default(),
    })
}

/// iCalendar 文本转义：`\n` / `\,` / `\;` / `\\`。
fn ics_text(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') | Some('N') => out.push(' '),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

use crate::otp;

// ---------------------------------------------------------------- 凭据

#[cfg(windows)]
pub mod secrets {
    /// 凭据只进 Windows 凭据管理器；键是邮箱地址，值是授权码。
    pub fn save(email: &str, password: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(super::SERVICE, &email.to_ascii_lowercase())
            .map_err(|e| format!("凭据管理器不可用：{e}"))?;
        entry.set_password(password).map_err(|e| format!("保存授权码失败：{e}"))
    }

    pub fn load(email: &str) -> Result<String, String> {
        let entry = keyring::Entry::new(super::SERVICE, &email.to_ascii_lowercase())
            .map_err(|e| format!("凭据管理器不可用：{e}"))?;
        entry.get_password().map_err(|_| "没有保存授权码".to_string())
    }

    pub fn delete(email: &str) {
        if let Ok(entry) = keyring::Entry::new(super::SERVICE, &email.to_ascii_lowercase()) {
            let _ = entry.delete_credential();
        }
    }
}

#[cfg(not(windows))]
pub mod secrets {
    pub fn save(_email: &str, _password: &str) -> Result<(), String> {
        Err("仅 Windows 支持凭据管理器".into())
    }
    pub fn load(_email: &str) -> Result<String, String> {
        Err("仅 Windows 支持凭据管理器".into())
    }
    pub fn delete(_email: &str) {}
}

// ---------------------------------------------------------------- 收信服务

#[cfg(windows)]
pub(crate) mod net {
    //! TLS 连接 + 163/126 的 pre-login `ID` 握手。
    //!
    //! `imap` 2.4 只在已登录的 `Session` 上暴露 `run_command_and_check_ok`，
    //! 而网易要求 greeting → ID → LOGIN 的顺序，所以这里手工读问候、发 ID，
    //! 再把「已经读掉的开头字节」原样交回给 imap。

    use super::Account;
    use imap::extensions::idle::SetReadTimeout;
    use native_tls::{TlsConnector, TlsStream};
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    pub type Stream = TlsStream<TcpStream>;

    /// 先吐出预先读到的问候字节，再透传给底层流。
    pub struct Prefixed<S> {
        head: Vec<u8>,
        pos: usize,
        inner: S,
    }

    impl<S> Prefixed<S> {
        fn new(head: Vec<u8>, inner: S) -> Self {
            Self { head, pos: 0, inner }
        }
    }

    impl<S: Read> Read for Prefixed<S> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.pos < self.head.len() {
                let n = (self.head.len() - self.pos).min(buf.len());
                buf[..n].copy_from_slice(&self.head[self.pos..self.pos + n]);
                self.pos += n;
                Ok(n)
            } else {
                self.inner.read(buf)
            }
        }
    }

    impl<S: Write> Write for Prefixed<S> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.inner.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.inner.flush()
        }
    }

    impl SetReadTimeout for Prefixed<Stream> {
        fn set_read_timeout(&mut self, timeout: Option<Duration>) -> imap::Result<()> {
            self.inner
                .get_ref()
                // imap 结束 IDLE 时传入 None；恢复普通命令的超时，避免无限阻塞。
                .set_read_timeout(timeout.or(Some(Duration::from_secs(20))))
                .map_err(imap::Error::Io)
        }
    }

    fn read_line(stream: &mut Stream, buf: &mut Vec<u8>) -> Result<(), String> {
        let mut byte = [0u8; 1];
        loop {
            match stream.read(&mut byte) {
                Ok(0) => return Err("服务器关闭了连接".into()),
                Ok(_) => {
                    buf.push(byte[0]);
                    if buf.ends_with(b"\r\n") {
                        return Ok(());
                    }
                    if buf.len() > 8192 {
                        return Err("服务器响应过长".into());
                    }
                }
                Err(e) => return Err(format!("读取失败：{e}")),
            }
        }
    }

    pub fn connect(account: &Account, password: &str) -> Result<imap::Session<Prefixed<Stream>>, String> {
        let (host,port,username)=account.endpoint()?;
        let tls = TlsConnector::builder()
            .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
            .build()
            .map_err(|e| format!("TLS 初始化失败：{e}"))?;
        let tcp = TcpStream::connect((host.as_str(), port))
            .map_err(|e| format!("无法连接 {host}：{e}"))?;
        let timeout = Some(Duration::from_secs(20));
        let _ = tcp.set_read_timeout(timeout);
        let _ = tcp.set_write_timeout(timeout);
        let mut stream = tls.connect(&host, tcp).map_err(|e| format!("TLS 握手失败：{e}"))?;

        // 1) 读问候，直到 "* OK" / "* PREAUTH"
        let mut greeting = Vec::new();
        loop {
            let mut line = Vec::new();
            read_line(&mut stream, &mut line)?;
            let upper = String::from_utf8_lossy(&line).to_ascii_uppercase();
            greeting.extend_from_slice(&line);
            if upper.starts_with("* BYE") {
                return Err("服务器拒绝连接".into());
            }
            if upper.starts_with("* OK") || upper.starts_with("* PREAUTH") {
                break;
            }
            if greeting.len() > 8192 {
                return Err("服务器问候异常".into());
            }
        }

        // 2) 网易系：LOGIN 之前先发 ID（RFC 2971）
        if account.provider.needs_id() {
            stream
                .write_all(b"A0001 ID (\"name\" \"daozai\" \"version\" \"0.5.5\" \"vendor\" \"daozai\")\r\n")
                .map_err(|e| format!("发送 ID 失败：{e}"))?;
            let _ = stream.flush();
            let mut ok = false;
            for _ in 0..20 {
                let mut line = Vec::new();
                read_line(&mut stream, &mut line)?;
                let text = String::from_utf8_lossy(&line).to_string();
                if text.starts_with("A0001 ") {
                    ok = text.to_ascii_uppercase().contains(" OK");
                    break;
                }
            }
            if !ok {
                return Err("服务器不接受 ID 命令".into());
            }
        }

        // 3) 把问候交回 imap，再正常登录
        let mut client = imap::Client::new(Prefixed::new(greeting, stream));
        client.read_greeting().map_err(|e| format!("服务器问候异常：{e}"))?;
        let session = client
            .login(username, password)
            .map_err(|(e, _)| login_error(e))?;
        Ok(session)
    }

    pub fn login_error(e: imap::Error) -> String {
        match e {
            imap::Error::No(_) | imap::Error::Bad(_) => "登录被拒绝，请检查用户名、授权码和管理员的 IMAP/认证策略（不支持 OAuth-only）".into(),
            imap::Error::Io(_) => "登录时网络连接中断，请重试".into(),
            _ => "IMAP 登录失败，请核对服务器及认证方式".into(),
        }
    }

    /// 测试连接：连上、登录、选 INBOX，然后退出。
    pub fn probe(account: &Account, password: &str) -> Result<(), String> {
        let mut session = connect(account, password)?;
        session.select("INBOX").map_err(|e| format!("无法打开收件箱：{e}"))?;
        let _ = session.logout();
        Ok(())
    }
}

#[cfg(windows)]
mod service {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::Arc;
    use std::time::{Duration,Instant};

    enum Cmd {
        Sync(Vec<Account>, u32),
        Test(Account, String),
    }

    pub struct Service {
        cmd: Sender<Cmd>,
        ev: Receiver<Event>,
        manage: Sender<crate::mail_manage::Request>,
        done: Receiver<crate::mail_manage::Completed>,
    }

    impl Service {
        pub fn start() -> Self {
            let (ev_tx, ev_rx) = channel();
            let (cmd_tx, cmd_rx) = channel();
            let _ = std::thread::Builder::new()
                .name("daozai-mail".into())
                .spawn(move || supervisor(cmd_rx, ev_tx));
            let (manage,done)=crate::mail_manage::start();
            Self { cmd: cmd_tx, ev: ev_rx,manage,done }
        }

        pub fn sync(&self, accounts: &[Account], interval_secs: u32) {
            let _ = self.cmd.send(Cmd::Sync(accounts.to_vec(), interval_secs));
        }

        pub fn test(&self, account: Account, password: String) {
            let _ = self.cmd.send(Cmd::Test(account, password));
        }

        pub fn manage(&self,request:crate::mail_manage::Request){let _=self.manage.send(request);}

        pub fn try_recv(&self) -> Option<Event> {
            self.done.try_recv().ok().map(Event::Managed).or_else(||self.ev.try_recv().ok())
        }
    }

    fn supervisor(cmd_rx: Receiver<Cmd>, ev: Sender<Event>) {
        let mut workers: HashMap<String, (Arc<AtomicBool>, std::thread::JoinHandle<()>,Account)> = HashMap::new();
        let interval = Arc::new(AtomicU64::new(120));
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                Cmd::Sync(accounts, secs) => {
                    interval.store(secs.clamp(30, 900) as u64, Ordering::Relaxed);
                    let wanted: HashMap<String, Account> = accounts
                        .into_iter()
                        .filter(|a| a.enabled)
                        .map(|a| (key(&a), a))
                        .collect();
                    let drop: Vec<String> = workers
                        .keys()
                        .filter(|k| wanted.get(*k)!=workers.get(*k).map(|worker|&worker.2))
                        .cloned()
                        .collect();
                    for k in drop {
                        if let Some((flag, _, _)) = workers.remove(&k) {
                            flag.store(true, Ordering::Relaxed);
                        }
                    }
                    for (k, account) in wanted {
                        if workers.contains_key(&k) {
                            continue;
                        }
                        let stop = Arc::new(AtomicBool::new(false));
                        let flag = stop.clone();
                        let ev = ev.clone();
                        let iv = interval.clone();
                        let name = "daozai-mail".to_string();
                        let running_account=account.clone();
                        if let Ok(handle) = std::thread::Builder::new()
                            .name(name)
                            .spawn(move || worker(account, flag, iv, ev))
                        {
                            workers.insert(k, (stop, handle,running_account));
                        }
                    }
                }
                Cmd::Test(account, password) => {
                    let ev = ev.clone();
                    let _ = std::thread::Builder::new()
                        .name("daozai-mail-test".into())
                        .spawn(move || {
                            let result = net::probe(&account, &password);
                            let _ = ev.send(Event::Test {
                                account: account.email,
                                ok: result.is_ok(),
                                note: result.err().unwrap_or_else(|| "连接成功".into()),
                            });
                        });
                }
            }
        }
        for (_, (flag, _, _)) in workers {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn sleep_stop(stop: &AtomicBool, secs: u64) -> bool {
        sleep_until_stop(stop,Instant::now()+Duration::from_secs(secs))
    }

    fn sleep_until_stop(stop:&AtomicBool,deadline:Instant)->bool{
        while !stop.load(Ordering::Relaxed){
            let left=deadline.saturating_duration_since(Instant::now());
            if left.is_zero(){return false;}
            std::thread::sleep(left.min(Duration::from_millis(500)));
        }
        true
    }

    // 只在轮询分支保活。各类邮箱都在同一个检查截止点按 UID 查新邮件。
    fn wait_for_poll<S:std::io::Read+std::io::Write>(
        session:&mut imap::Session<S>,start:Instant,secs:u64,allow_noop:&mut bool,mut wait_until:impl FnMut(Instant)->bool,
    )->imap::Result<()>{
        const KEEPALIVE:Duration=Duration::from_secs(45);
        let deadline=start+Duration::from_secs(secs);
        let mut previous=start;
        loop{
            let wake=if *allow_noop{(previous+KEEPALIVE).min(deadline)}else{deadline};
            if wait_until(wake) || !*allow_noop{return Ok(());}
            // NOOP 可携带 EXISTS、EXPUNGE、FETCH 等通知；收信以随后的 UID 查询为准。
            match session.run_command_and_check_ok("NOOP"){
                Ok(())=>{},
                // 服务器明确拒绝保活时，保留当前连接，按原间隔用 UID 查询。
                Err(imap::Error::No(_)|imap::Error::Bad(_))=>*allow_noop=false,
                Err(error)=>return Err(error),
            }
            let now=Instant::now();
            if wake==deadline || now>=deadline{return Ok(());}
            previous=wake.max(now);
        }
    }

    fn next_retry_delay(base:u64,failures:&mut u32)->u64 {
        let multiplier=2u64.pow((*failures).min(6));
        let delay=base.saturating_mul(multiplier).min(60);
        *failures=failures.saturating_add(1);
        delay
    }

    fn wait_for_change<T: std::io::Read + std::io::Write + imap::extensions::idle::SetReadTimeout>(session: &mut imap::Session<T>) -> imap::Result<()> {
        // 优先响应服务器推送；定期补查，避免服务端漏推送时长时间没有提醒。
        session.idle()?.wait_with_timeout(Duration::from_secs(10)).map(|_| ())
    }

    #[test]
    fn idle_without_push_returns_for_a_fallback_check() {
        use std::io::{BufRead,Write};
        for push in [true,false] {
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address=listener.local_addr().unwrap();
        let server=std::thread::spawn(move||{
            let (mut socket,_)=listener.accept().unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(14))).unwrap();
            let mut reader=std::io::BufReader::new(socket.try_clone().unwrap());
            write!(socket,"* OK ready\r\n").unwrap();
            let mut line=String::new();reader.read_line(&mut line).unwrap();
            write!(socket,"{} OK logged in\r\n",line.split_whitespace().next().unwrap()).unwrap();
            line.clear();reader.read_line(&mut line).unwrap();
            let tag=line.split_whitespace().next().unwrap().to_string();
            assert!(line.contains("IDLE"));write!(socket,"+ idling\r\n").unwrap();
            if push {write!(socket,"* 1 EXISTS\r\n").unwrap();}
            line.clear();
            if reader.read_line(&mut line).is_err(){return false;}
            assert_eq!(line,"DONE\r\n");
            write!(socket,"{tag} OK idle ended\r\n").unwrap();true
        });
        let mut client=imap::Client::new(std::net::TcpStream::connect(address).unwrap());
        client.read_greeting().unwrap();
        let mut session=client.login("test","test").unwrap();
        let start=std::time::Instant::now();
        let result=wait_for_change(&mut session);
        assert!(server.join().unwrap(),"服务端未推送时，必须在 14 秒内主动结束等待并补查");
        result.unwrap();
        if push {assert!(start.elapsed()<Duration::from_secs(2),"有推送时必须立即处理，不能等待检查间隔");}
        }
    }

    fn worker(account: Account, stop: Arc<AtomicBool>, interval: Arc<AtomicU64>, ev: Sender<Event>) {
        if let Err(error)=account.endpoint(){
            let _=ev.send(Event::Status{account:account.email,status:Status::Failed(error)});return;
        }
        let mut last_uid = None;
        let mut last_validity=None;
        let mut failures=0;
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let _ = ev.send(Event::Status {
                account: account.email.clone(),
                status: Status::Connecting,
            });
            let password = match secrets::load(&account.credential_key()) {
                Ok(p) => p,
                Err(e) => {
                    let _ = ev.send(Event::Status {
                        account: account.email.clone(),
                        status: Status::Auth(e),
                    });
                    if sleep_stop(&stop, next_retry_delay(60,&mut failures)) {
                        return;
                    }
                    continue;
                }
            };
            let mut session = match net::connect(&account, &password) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ev.send(Event::Status {
                        account: account.email.clone(),
                        status: Status::Failed(e),
                    });
                    if sleep_stop(&stop, next_retry_delay(30,&mut failures)) {
                        return;
                    }
                    continue;
                }
            };
            if stop.load(Ordering::Relaxed){let _=session.logout();return;}
            let Ok(mailbox)=session.select("INBOX") else {
                let _ = ev.send(Event::Status {
                    account: account.email.clone(),
                    status: Status::Failed("无法打开收件箱".into()),
                });
                if sleep_stop(&stop, next_retry_delay(30,&mut failures)) {
                    return;
                }
                continue;
            };
            if mailbox.uid_validity.is_none() || mailbox.uid_validity!=last_validity{last_uid=None;}
            last_validity=mailbox.uid_validity;
            let connected_at=std::time::Instant::now();
            match monitor(&mut session,&account,&mut last_uid,&stop,&interval,&ev) {
                Ok(())=>return,
                Err(note)=>{let _=ev.send(Event::Status{account:account.email.clone(),status:Status::Failed(note)});}
            }
            if connected_at.elapsed()>=Duration::from_secs(60){failures=0;}
            if sleep_stop(&stop, next_retry_delay(10,&mut failures)) {
                return;
            }
        }
    }

    fn monitor<S:std::io::Read+std::io::Write+imap::extensions::idle::SetReadTimeout>(
        session:&mut imap::Session<S>,account:&Account,last_uid:&mut Option<u32>,
        stop:&AtomicBool,interval:&AtomicU64,ev:&Sender<Event>,
    )->Result<(),String> {
        match last_uid {
            Some(last)=>fetch_new(session,account,last,ev).map_err(|_|"读取新邮件失败，正在重连")?,
            None=>{*last_uid=Some(recent(session,account,ev).map_err(|_|"读取最近邮件失败，正在重连")?);}
        }
        let mut allow_noop=true;
        let mut idle_supported=session.capabilities().map_err(|_|"读取收信能力失败，正在重连")?.has_str("IDLE");
        let _=ev.send(Event::Status{account:account.email.clone(),status:Status::Ready});
        loop {
            if stop.load(Ordering::Relaxed){let _=session.logout();return Ok(());}
            let previous=*last_uid;
            let cycle_started=std::time::Instant::now();
            if idle_supported {
                match wait_for_change(session) {
                    Ok(())=>{},
                    // 服务器明确拒绝推送时，本次连接改为轮询；重连后重新检查推送能力。
                    Err(imap::Error::No(_)|imap::Error::Bad(_))=>idle_supported=false,
                    Err(_)=>return Err("邮件推送连接中断，正在重连".into()),
                }
            }else{
                wait_for_poll(session,cycle_started,interval.load(Ordering::Relaxed),&mut allow_noop,|at|sleep_until_stop(stop,at))
                    .map_err(|_|"邮件连接中断，正在重连")?;
            }
            if stop.load(Ordering::Relaxed){let _=session.logout();return Ok(());}
            fetch_new(session,account,last_uid.as_mut().unwrap(),ev).map_err(|_|"读取新邮件失败，正在重连")?;
            if idle_supported && *last_uid==previous && cycle_started.elapsed()<Duration::from_secs(5) {
                if sleep_stop(stop,5){let _=session.logout();return Ok(());}
                // 冷却后补读新邮件，并消费等待期间到达的通知，再进入下一轮推送等待。
                fetch_new(session,account,last_uid.as_mut().unwrap(),ev).map_err(|_|"读取新邮件失败，正在重连")?;
            }
        }
    }

    #[cfg(test)]
    mod receiving_tests {
        use super::*;
        use std::{io::{BufRead,Write},net::{TcpListener,TcpStream}};

        #[derive(Clone,Copy)]
        enum Scenario {
            RejectedIdle, SupportedIdle, EmptyIdle, CooldownMail,
            InitialMailbox, DisconnectedIdle, BrokenHistory, Burst, PollKeepalive, BrokenNoop, RefusedNoop,
        }

        fn inbox(scenario:Scenario)->(imap::Session<TcpStream>,Arc<AtomicBool>,std::thread::JoinHandle<Vec<String>>) {
            let listener=TcpListener::bind("127.0.0.1:0").unwrap();
            let address=listener.local_addr().unwrap();
            let stop=Arc::new(AtomicBool::new(false));let server_stop=stop.clone();
            let server=std::thread::spawn(move || {
                let (mut socket,_)=listener.accept().unwrap();
                let timeout=if matches!(scenario,Scenario::EmptyIdle|Scenario::CooldownMail){8}else{2};
                socket.set_read_timeout(Some(Duration::from_secs(timeout))).unwrap();
                let mut reader=std::io::BufReader::new(socket.try_clone().unwrap());
                let mut commands=Vec::new();let mut idle_tag=String::new();let mut idle_started=false;let mut fetches=0;let mut searches=0;
                write!(socket,"* OK ready\r\n").unwrap();
                loop {
                    let mut line=String::new();
                    match reader.read_line(&mut line) {
                        Ok(0)=>break,
                        Ok(_)=>{},
                        Err(error) if error.kind()==std::io::ErrorKind::ConnectionReset=>break,
                        Err(error)=>panic!("模拟收信连接读取失败：{error}"),
                    }
                    let line=line.trim_end();commands.push(line.to_string());
                    if line=="DONE" {
                        if matches!(scenario,Scenario::SupportedIdle|Scenario::EmptyIdle|Scenario::CooldownMail){write!(socket,"{idle_tag} OK idle ended\r\n").unwrap();}
                        else{write!(socket,"{idle_tag} BAD not idling\r\n").unwrap();}
                        continue;
                    }
                    let (tag,command)=line.split_once(' ').unwrap();
                    if command.starts_with("LOGIN ") {
                        write!(socket,"{tag} OK login\r\n").unwrap();
                    }else if command.starts_with("UID SEARCH ") {
                        searches+=1;
                        let uids=match scenario {
                            Scenario::EmptyIdle=>"",
                            Scenario::CooldownMail if searches<3=>"",
                            Scenario::RejectedIdle|Scenario::SupportedIdle if !idle_started=>"",
                            Scenario::BrokenHistory|Scenario::InitialMailbox=>"42",
                            Scenario::Burst=>"1 2 3 4 5 6 7",
                            _=>"41",
                        };
                        if uids.is_empty(){write!(socket,"* SEARCH\r\n{tag} OK search\r\n").unwrap();}
                        else{write!(socket,"* SEARCH {uids}\r\n{tag} OK search\r\n").unwrap();}
                    }else if command.starts_with("UID FETCH ") {
                        if matches!(scenario,Scenario::BrokenHistory){write!(socket,"* BYE connection lost\r\n").unwrap();break;}
                        let uid=command.split_whitespace().nth(2).unwrap();
                        let header=format!("From: sender@example.com\r\nSubject: mail-{uid}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n");
                        let body="ordinary message";
                        write!(socket,"* 1 FETCH (UID {uid} BODY[HEADER] {{{}}}\r\n{header} BODY[TEXT] {{{}}}\r\n{body})\r\n{tag} OK fetch\r\n",header.len(),body.len()).unwrap();
                        fetches+=1;
                        if matches!(scenario,Scenario::SupportedIdle|Scenario::CooldownMail) && fetches==2 {server_stop.store(true,Ordering::Relaxed);}
                    }else if command=="CAPABILITY" {
                        let capabilities=if matches!(scenario,Scenario::InitialMailbox){"IMAP4rev1"}else{"IMAP4rev1 IDLE"};
                        write!(socket,"* CAPABILITY {capabilities}\r\n{tag} OK capabilities\r\n").unwrap();
                    }else if command=="IDLE" {
                        if matches!(scenario,Scenario::EmptyIdle) && idle_started {server_stop.store(true,Ordering::Relaxed);}
                        idle_started=true;idle_tag=tag.to_string();
                        if matches!(scenario,Scenario::RejectedIdle){
                            write!(socket,"{tag} NO IDLE unavailable\r\n").unwrap();
                        }else if matches!(scenario,Scenario::SupportedIdle|Scenario::EmptyIdle|Scenario::CooldownMail){
                            write!(socket,"+ idling\r\n* 1 EXISTS\r\n").unwrap();
                        }else{write!(socket,"* BYE connection lost\r\n").unwrap();break;}
                    }else if command=="NOOP" {
                        if matches!(scenario,Scenario::RefusedNoop){write!(socket,"{tag} BAD unavailable\r\n").unwrap();continue;}
                        if matches!(scenario,Scenario::BrokenNoop){write!(socket,"* BYE closing\r\n").unwrap();break;}
                        if matches!(scenario,Scenario::PollKeepalive){
                            write!(socket,"* 1 EXISTS\r\n* 1 FETCH (FLAGS (\\Seen))\r\n").unwrap();
                        }else{server_stop.store(true,Ordering::Relaxed);}
                        write!(socket,"{tag} OK noop\r\n").unwrap();
                    }else if command=="LOGOUT" {
                        write!(socket,"* BYE logout\r\n{tag} OK logout\r\n").unwrap();break;
                    }else{panic!("意外的收信命令：{command}");}
                }
                commands
            });
            let socket=TcpStream::connect(address).unwrap();socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut client=imap::Client::new(socket);client.read_greeting().unwrap();
            (client.login("synthetic","synthetic").unwrap(),stop,server)
        }

        fn account()->Account {Account{provider:Provider::Mail163,email:"test@163.com".into(),enabled:true,custom:None}}

        #[test]
        fn polling_keepalives_do_not_fetch_until_the_configured_deadline(){
            for (secs,steps) in [(60,vec![45,60]),(120,vec![45,90,120]),(300,vec![45,90,135,180,225,270,300])] {
                let (mut session,_,server)=inbox(Scenario::PollKeepalive);
                let start=Instant::now();
                let mut wakes=Vec::new();
                wait_for_poll(&mut session,start,secs,&mut true,|at|{wakes.push((at-start).as_secs());false}).unwrap();
                let (tx,_)=channel();let mut last=40;fetch_new(&mut session,&account(),&mut last,&tx).unwrap();
                session.logout().unwrap();drop(session);
                let commands=server.join().unwrap();assert_eq!(wakes,steps);
                let before_search=commands.iter().skip(1).take_while(|s|!s.contains("UID SEARCH")).collect::<Vec<_>>();
                assert_eq!(before_search.len(),steps.len());assert!(before_search.iter().all(|s|s.ends_with(" NOOP")));
                assert_eq!(commands.iter().filter(|s|s.contains("UID SEARCH")).count(),1);
                assert_eq!(last,41);
            }
        }
        #[test]
        fn rejected_keepalive_keeps_polling_without_repeated_noops(){
            let (mut session,_,server)=inbox(Scenario::RefusedNoop);
            let mut allow_noop=true;
            let (tx,_)=channel();let mut last=40;
            for steps in [vec![45,120],vec![120]]{
                let start=Instant::now();let mut wakes=Vec::new();
                wait_for_poll(&mut session,start,120,&mut allow_noop,|at|{wakes.push((at-start).as_secs());false}).unwrap();
                assert_eq!(wakes,steps);assert!(!allow_noop);
                fetch_new(&mut session,&account(),&mut last,&tx).unwrap();
            }
            session.logout().unwrap();drop(session);let commands=server.join().unwrap();
            assert_eq!(commands.iter().filter(|s|s.ends_with(" NOOP")).count(),1);
            assert_eq!(commands.iter().filter(|s|s.contains("UID SEARCH")).count(),2);assert_eq!(last,41);
        }

        #[test]
        fn polling_stops_before_sending_or_after_a_broken_keepalive(){
            for cancel in [true,false]{
                let (mut session,_,server)=inbox(Scenario::BrokenNoop);
                let result=wait_for_poll(&mut session,Instant::now(),120,&mut true,|_|cancel);
                assert_eq!(result.is_ok(),cancel);
                drop(session);let commands=server.join().unwrap();
                assert_eq!(commands.iter().filter(|s|s.ends_with(" NOOP")).count(),usize::from(!cancel));
                assert!(!commands.iter().any(|s|s.contains("UID SEARCH")));
            }
        }

        #[test]
        fn reconnect_backoff_is_bounded_and_can_reset() {
            let mut failures=0;
            let delays:Vec<_>=(0..7).map(|_|next_retry_delay(10,&mut failures)).collect();
            assert_eq!(delays,vec![10,20,40,60,60,60,60]);
            failures=0;
            assert_eq!(next_retry_delay(30,&mut failures),30);
            assert_eq!(next_retry_delay(30,&mut failures),60);
            assert_eq!(next_retry_delay(60,&mut failures),60);
        }

        #[test]
        fn cooldown_wait_can_be_cancelled() {
            let stop=Arc::new(AtomicBool::new(false));let worker_stop=stop.clone();
            let started=std::time::Instant::now();
            let worker=std::thread::spawn(move || sleep_stop(&worker_stop,30));
            std::thread::sleep(Duration::from_millis(50));stop.store(true,Ordering::Relaxed);
            assert!(worker.join().unwrap());
            assert!(started.elapsed()<Duration::from_secs(2),"停止监听应打断后台等待");
        }

        #[test]
        fn repeated_empty_pushes_are_throttled() {
            let (mut session,stop,server)=inbox(Scenario::EmptyIdle);
            let (ev,rx)=channel();let mut last=Some(40);
            let started=std::time::Instant::now();
            let result=monitor(&mut session,&account(),&mut last,&stop,&AtomicU64::new(120),&ev);
            let elapsed=started.elapsed();drop(session);let commands=server.join().unwrap();
            assert!(result.is_ok(),"{result:?}");assert_eq!(last,Some(40));
            assert!(elapsed>=Duration::from_secs(5),"两次空推送之间应降频，实际耗时 {elapsed:?}");
            assert_eq!(commands.iter().filter(|command|command.ends_with(" IDLE")).count(),2);
            assert!(!rx.try_iter().any(|event|matches!(event,Event::Mail{..})));
        }

        #[test]
        fn new_mail_arriving_during_cooldown_is_read_before_idling_again() {
            let (mut session,stop,server)=inbox(Scenario::CooldownMail);
            let (ev,rx)=channel();let mut last=Some(40);
            let result=monitor(&mut session,&account(),&mut last,&stop,&AtomicU64::new(120),&ev);
            drop(session);let commands=server.join().unwrap();
            assert!(result.is_ok(),"{result:?}");assert_eq!(last,Some(41));
            assert!(rx.try_iter().any(|event|matches!(event,Event::Mail{item,new:true} if item.uid==41)));
            assert_eq!(commands.iter().filter(|command|command.ends_with(" IDLE")).count(),1,"冷却结束后应先检查期间到达的新邮件");
        }

        #[test]
        fn supported_idle_still_delivers_mail_without_polling() {
            let (mut session,stop,server)=inbox(Scenario::SupportedIdle);
            let (ev,rx)=channel();let mut last=Some(40);
            let result=monitor(&mut session,&account(),&mut last,&stop,&AtomicU64::new(0),&ev);
            drop(session);let commands=server.join().unwrap();
            assert!(result.is_ok(),"{result:?}");assert_eq!(last,Some(41));
            assert!(rx.try_iter().any(|event|matches!(event,Event::Mail{item,new:true} if item.uid==41)));
            assert_eq!(commands.iter().filter(|command|command.as_str()=="DONE").count(),1);
            assert!(!commands.iter().any(|command|command.ends_with(" NOOP")));
        }

        #[test]
        fn initial_mail_is_silent_and_ready_follows_successful_reading() {
            for provider in Provider::ALL {
                let (mut session,stop,server)=inbox(Scenario::InitialMailbox);
                let (ev,rx)=channel();let mut last=None;let account=Account{provider,..account()};
                let result=monitor(&mut session,&account,&mut last,&stop,&AtomicU64::new(0),&ev);
                drop(session);server.join().unwrap();
                assert!(result.is_ok(),"{result:?}");assert_eq!(last,Some(42));
                let events:Vec<_>=rx.try_iter().collect();
                let mail=events.iter().position(|event|matches!(event,Event::Mail{item,new:false} if item.uid==42)).unwrap();
                let ready=events.iter().position(|event|matches!(event,Event::Status{status:Status::Ready,..})).unwrap();
                assert!(mail<ready,"成功读取后再显示已连接");
                assert!(!events.iter().any(|event|matches!(event,Event::Mail{new:true,..})),"首次加载应保持静默");
            }
        }

        #[test]
        fn rejected_idle_still_receives_new_163_mail() {
            let (mut session,stop,server)=inbox(Scenario::RejectedIdle);
            let (ev,rx)=channel();let mut last=Some(40);
            let result=monitor(&mut session,&account(),&mut last,&stop,&AtomicU64::new(0),&ev);
            drop(session);let commands=server.join().unwrap();
            assert!(result.is_ok(),"推送被拒绝后应继续轮询收信：{result:?} commands={commands:?}");
            assert_eq!(last,Some(41));
            assert!(rx.try_iter().any(|event|matches!(event,Event::Mail{item,new:true} if item.uid==41)));
            assert!(!commands.iter().any(|command|command=="DONE"),"尚未进入推送状态时不能发送 DONE");
            assert!(commands.iter().any(|command|command.ends_with(" NOOP")));
        }

        #[test]
        fn reconnect_checks_new_mail_before_waiting_for_push() {
            let (mut session,stop,server)=inbox(Scenario::DisconnectedIdle);
            let (ev,rx)=channel();let mut last=Some(40);
            assert!(monitor(&mut session,&account(),&mut last,&stop,&AtomicU64::new(0),&ev).is_err());
            drop(session);server.join().unwrap();
            assert_eq!(last,Some(41),"重连后应先补读断线期间到达的邮件");
            assert!(rx.try_iter().any(|event|matches!(event,Event::Mail{item,new:true} if item.uid==41)));
        }

        #[test]
        fn interrupted_initial_fetch_keeps_history_retryable() {
            let (mut session,stop,server)=inbox(Scenario::BrokenHistory);
            let (ev,rx)=channel();let mut last=None;
            assert!(monitor(&mut session,&account(),&mut last,&stop,&AtomicU64::new(0),&ev).is_err());
            drop(session);server.join().unwrap();
            assert_eq!(last,None,"最近邮件读取失败后不能提交最大 UID");
            assert!(!rx.try_iter().any(|event|matches!(event,Event::Status{status:Status::Ready,..})));
        }

        #[test]
        fn bursts_do_not_skip_messages_between_batches() {
            let (mut session,_,server)=inbox(Scenario::Burst);
            let (ev,rx)=channel();let mut last=0;
            fetch_new(&mut session,&account(),&mut last,&ev).unwrap();
            fetch_new(&mut session,&account(),&mut last,&ev).unwrap();
            drop(session);server.join().unwrap();
            let uids:Vec<_>=rx.try_iter().filter_map(|event|match event {Event::Mail{item,new:true}=>Some(item.uid),_=>None}).collect();
            assert_eq!(uids,vec![1,2,3,4,5,6,7],"批量读取不能跳过前面的新邮件");
            assert_eq!(last,7);
        }
    }

    /// 首次连接：把最近 20 封读进面板，但不当作新邮件提示。
    fn recent<S:std::io::Read+std::io::Write>(
        session: &mut imap::Session<S>,
        account: &Account,
        ev: &Sender<Event>,
    ) -> Result<u32, ()> {
        let all = session.uid_search("ALL").map_err(|_| ())?;
        let mut uids: Vec<u32> = all.into_iter().collect();
        uids.sort_unstable();
        let max = uids.last().copied().unwrap_or(0);
        let tail: Vec<u32> = uids.iter().rev().take(20).rev().copied().collect();
        for uid in tail {
            match super::fetch::item(session,uid,account){
                Ok(Some(item))=>{let _=ev.send(Event::Mail{item,new:false});},
                Ok(None)=>{},
                Err(())=>return Err(()),
            }
        }
        Ok(max)
    }

    #[test]
    #[ignore = "需要真实 QQ 邮箱及新测试邮件，验证后台监听"]
    fn qq_live_notification_worker() {
        let accounts=load_accounts().into_iter().filter(|a|a.provider==Provider::Qq&&a.enabled).collect::<Vec<_>>();
        assert!(!accounts.is_empty());
        let service=Service::start();service.sync(&accounts,120);
        let start=std::time::Instant::now();
        while start.elapsed()<Duration::from_secs(160) {
            if let Ok(event)=service.ev.recv_timeout(Duration::from_secs(1)) {
                match event {
                    Event::Status{status,..}=>println!("worker_status={status:?}"),
                    Event::Mail{new,item}=>{
                        println!("worker_mail new={new} test_message={}",item.subject.starts_with("岛仔"));
                        if new && item.subject=="岛仔后台监听实测" {
                            service.sync(&[],120);return;
                        }
                    },
                    _=>{}
                }
            }
        }
        service.sync(&[],120);panic!("160 秒内未收到新邮件事件");
    }

    #[test]
    #[ignore="需要本轮已发送的日历邀请和普通日期邮件"]
    fn qq_live_calendar_mail_probe() {
        let account=load_accounts().into_iter().find(|a|a.provider==Provider::Qq&&a.enabled).unwrap();
        let password=secrets::load(&account.credential_key()).unwrap();
        let mut session=net::connect(&account,&password).unwrap();session.examine("INBOX").unwrap();
        let (tx,rx)=std::sync::mpsc::channel();recent(&mut session,&account,&tx).unwrap();
        let items=rx.try_iter().filter_map(|ev|if let Event::Mail{item,..}=ev{Some(item)}else{None}).collect::<Vec<_>>();
        let invite=items.iter().find(|m|m.subject=="【岛仔测试】邮件日程邀请：收件箱合并验证").expect("测试邀请未到达");
        assert_eq!(invite.meeting.as_ref().unwrap().uid,"daozai-mail-test-20261103-1500");
        let plain=items.iter().find(|m|m.subject=="DAOZAI-DATE-TEST-20261103").expect("普通日期邮件未到达");
        assert!(plain.meeting.is_none());session.logout().unwrap();
        println!("invitation_recognized=true plain_date_rejected=true");
    }

    /// 显式运行的真实邮箱只读检查；不输出地址、凭据或邮件内容。
    #[test]
    fn exmail_capability_trailing_spaces_preserve_following_response() {
        let response=b"* CAPABILITY IMAP4 IMAP4rev1 XLIST MOVE IDLE XAPPLEPUSHSERVICE NAMESPACE CHILDREN ID UIDPLUS \r\na2 OK capabilities\r\n";
        let (rest,parsed)=imap_proto::parse_response(response).unwrap();
        assert!(matches!(parsed,imap_proto::Response::Capabilities(caps) if caps.iter().any(|c|matches!(c,imap_proto::Capability::Atom("IDLE")))));
        assert_eq!(rest,b"a2 OK capabilities\r\n");
        assert!(imap_proto::parse_response(b"* CAPABILITY IDLE \r\n").is_err(),"仍需校验 IMAP4rev1");
    }

    #[test]
    #[ignore="本机腾讯企业邮箱只读连接诊断；不输出凭据或邮件内容"]
    fn exmail_live_capability_probe() {
        let account=load_accounts().into_iter().find(|a|a.enabled && a.custom.as_ref().is_some_and(|c|c.host=="imap.exmail.qq.com")).expect("未配置腾讯企业邮箱");
        let password=secrets::load(&account.credential_key()).expect("无法读取本机凭据");
        let mut session=net::connect(&account,&password).expect("登录失败");
        session.examine("INBOX").expect("只读打开失败");
        let (tx,rx)=std::sync::mpsc::channel();
        recent(&mut session,&account,&tx).expect("最近邮件读取失败");
        drop(rx);
        match session.capabilities(){
            Ok(caps)=>println!("capabilities=ok idle={}",caps.has_str("IDLE")),
            Err(error)=>panic!("capabilities_error={error:?}"),
        }
        let _=session.logout();
    }

    #[test]
    #[ignore = "需要本机已连接的 QQ 邮箱，显式执行只读实测"]
    fn qq_live_read_only_probe() {
        let accounts=load_accounts();
        let account=accounts.iter().find(|a|a.provider==Provider::Qq&&a.enabled).expect("未找到已启用 QQ 邮箱");
        let password=secrets::load(&account.credential_key()).expect("读取邮箱凭据失败");
        let mut session=net::connect(account,&password).expect("真实登录失败");
        session.examine("INBOX").expect("只读打开收件箱失败");
        println!("idle_supported={}",session.capabilities().expect("读取能力失败").has_str("IDLE"));
        let before=session.uid_search("UNSEEN").expect("读取未读状态失败");
        let (tx,rx)=std::sync::mpsc::channel();
        let mut last=recent(&mut session,account,&tx).expect("最近邮件读取失败");
        let initial=rx.try_iter().collect::<Vec<_>>();
        assert!(initial.iter().all(|e|matches!(e,Event::Mail{new:false,..})));
        println!("login=ok readonly_inbox=ok recent_parsed={}",initial.len());
        for event in &initial {if let Event::Mail{item,..}=event{
            println!("subject_chars={} sender_chars={} body_bytes={} links={} max_link_chars={}",item.subject.chars().count(),item.from.chars().count(),item.summary.len(),item.links.len(),item.links.iter().map(|l|l.label.chars().count()).max().unwrap_or(0));
        }}
        let received=initial.iter().any(|e|matches!(e,Event::Mail{item,..} if item.subject=="岛仔收信测试"&&item.code.as_deref()==Some("482731")&&!item.links.is_empty()));
        println!("test_message_with_code_and_link={received}");
        fetch_new(&mut session,account,&mut last,&tx).expect("增量收取失败");
        let added=rx.try_iter().count();
        let after=session.uid_search("UNSEEN").expect("检查未读状态失败");
        println!("incremental_fetch=ok new_messages={} unseen_unchanged={}",added,before==after);
        session.logout().expect("退出失败");
    }

    fn fetch_new<S:std::io::Read+std::io::Write>(
        session: &mut imap::Session<S>,
        account: &Account,
        last: &mut u32,
        ev: &Sender<Event>,
    ) -> Result<(), ()> {
        let set = session
            .uid_search(format!("UID {}:*", last.saturating_add(1)))
            .map_err(|_| ())?;
        let mut uids: Vec<u32> = set.into_iter().filter(|u| *u > *last).collect();
        uids.sort_unstable();
        uids.truncate(5);
        for uid in uids {
            if let Some(item)=super::fetch::item(session,uid,account)? {
                let _=ev.send(Event::Mail{item,new:true});
            }
            *last = (*last).max(uid);
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod service {
    use super::*;

    pub struct Service;

    impl Service {
        pub fn start() -> Self {
            Self
        }
        pub fn sync(&self, _accounts: &[Account], _interval_secs: u32) {}
        pub fn test(&self, _account: Account, _password: String) {}
        pub fn manage(&self,_request:crate::mail_manage::Request){}
        pub fn try_recv(&self) -> Option<Event> {
            None
        }
    }
}

pub use service::Service;

#[cfg(test)]
mod tests {

    /// multipart/alternative 邮件必须同时保留 text/plain 与 text/html 原文，
    /// 否则排版层只能拿到被压平的纯文本。
    #[test]
    fn multipart_message_keeps_html_original(){
        let raw=b"From: Team <team@example.com>\r\nSubject: Weekly\r\n\
Content-Type: multipart/alternative; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nWeekly  Flat text\r\n\
--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<h1>Weekly</h1><ul><li>Flat text</li></ul>\r\n--b--\r\n";
        let parsed=super::parse_message(raw);
        assert!(!parsed.text.trim().is_empty(),"text/plain 仍要保留作摘要与兜底");
        assert!(parsed.html.contains("<h1>"),"HTML 原文必须保留");
        let lines=crate::mail_format::lines(&parsed.html,&parsed.text);
        assert_eq!(lines[0].kind,crate::mail_format::Kind::Title,"真实邮件应走 HTML 排版");
        assert!(lines.iter().any(|l|l.kind==crate::mail_format::Kind::Bullet));
    }
    #[test]
    fn quality_cancelled_meetings_are_not_active_events() {
        let event="BEGIN:VEVENT\nUID:cancelled\nDTSTART:20261101T060000Z\nSTATUS:CANCELLED\nEND:VEVENT";
        assert!(super::meetings_of(event).is_empty());
        let message=format!("BEGIN:VCALENDAR\nMETHOD:CANCEL\n{}\nEND:VCALENDAR",event.replace("STATUS:CANCELLED\n",""));
        assert!(super::meetings_of(&message).is_empty());
    }

    #[test]
    fn quality_alarm_fields_do_not_replace_meeting_fields() {
        let event="BEGIN:VEVENT\nDTSTART:20261101T060000Z\nBEGIN:VALARM\nSUMMARY:闹钟文案\nEND:VALARM\nSUMMARY:正式会议\nEND:VEVENT";
        assert_eq!(super::meetings_of(event)[0].title,"正式会议");
    }

    #[test]
    fn quality_ics_unfolding_preserves_content_spaces() {
        let event="BEGIN:VEVENT\nDTSTART:20261101T060000Z\nSUMMARY:项目\n  评审\nEND:VEVENT";
        assert_eq!(super::meetings_of(event)[0].title,"项目 评审");
    }
    #[test]
    fn email_links_preserve_targets_labels_and_query_parameters() {
        let parsed=super::parse_message(b"Content-Type: text/html; charset=utf-8\r\n\r\n<a href=\"https://example.com/review?a=1&amp;b=2\"><b>Review</b></a><a href='https://example.com/review?a=1&amp;b=2'>Again</a><a href=javascript:alert(1)>Unsafe</a>");
        assert_eq!(super::links_of(&parsed),vec![super::Link{label:"Review".into(),url:"https://example.com/review?a=1&b=2".into()}]);
        let plain=super::parse_message(b"Content-Type: text/plain\r\n\r\nRead https://example.com/docs. Then https://example.com/help");
        let links=super::links_of(&plain);
        assert_eq!(links.len(),2);assert_eq!(links[0].url,"https://example.com/docs");
        assert!(super::web_url("file:///C:/Windows/notepad.exe").is_none());
        assert!(super::web_url("https://").is_none());
        let parens=super::parse_message(b"Content-Type: text/plain\r\n\r\nhttps://example.com/Function_(mathematics). (https://example.com/help)");
        let targets=super::links_of(&parens);
        assert_eq!(targets[0].url,"https://example.com/Function_(mathematics)");
        assert_eq!(targets[1].url,"https://example.com/help");
    }
    use super::*;
    use chrono::TimeZone;

    fn raw(subject: &str, body: &str) -> Vec<u8> {
        format!("From: =?UTF-8?B?5bCP54mb?= <hi@example.com>\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}").into_bytes()
    }

    #[test]
    fn parses_subject_sender_and_code() {
        let parsed = parse_message(&raw("验证码", "您的验证码是：834921，10分钟内有效。"));
        assert_eq!(parsed.subject, "验证码");
        assert_eq!(parsed.from, "小牛");
        assert_eq!(code_of(&parsed).as_deref(), Some("834921"));
    }

    #[test]
    fn html_only_mail_still_finds_code() {
        let html = "<html><body><p>您正在登录</p><p>验证码：<b>906213</b></p></body></html>";
        let raw = format!("From: a@b.c\r\nSubject: 登录\r\nContent-Type: text/html; charset=utf-8\r\n\r\n{html}");
        let parsed = parse_message(raw.as_bytes());
        assert_eq!(code_of(&parsed).as_deref(), Some("906213"));
    }

    #[test]
    fn accounts_roundtrip_without_secrets() {
        let items = vec![
            Account { custom: None, email: "a@qq.com".into(), provider: Provider::Qq, enabled: true },
            Account { custom: None, email: "b@163.com".into(), provider: Provider::Mail163, enabled: false },
        ];
        let bytes = encode_accounts(&items);
        assert_eq!(decode_accounts(&bytes), items);
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("password") && !text.contains("auth"));
        assert_eq!(decode_accounts(b"not json"), Vec::new());
    }

    #[test]
    fn provider_presets_cover_six_kinds() {
        assert_eq!(Provider::ALL.len(), 8);
        assert_eq!(Provider::parse("gmail"), Provider::Gmail);
        assert_eq!(Provider::Gmail.host(), "imap.gmail.com");
        assert_eq!(Provider::Gmail.port(), 993);
        let gmail=Account {custom: None, email:"demo@gmail.com".into(),provider:Provider::Gmail,enabled:true};
        assert_eq!(decode_accounts(&encode_accounts(&[gmail.clone()])),vec![gmail]);
        assert!(Provider::Mail163.needs_id() && Provider::Mail126.needs_id());
        assert!(!Provider::Qq.needs_id());
        assert_eq!(Provider::parse("exmail").host(), "imap.exmail.qq.com");
    }

    #[test]
    fn summary_flattens_body_and_caps_length() {
        let parsed = parse_message(&raw("周会", "第一行\n\n  第二行   有空格\r\n第三行"));
        assert_eq!(summary_of(&parsed), "第一行 第二行 有空格 第三行");
        let long = "字".repeat(400);
        let parsed = parse_message(&raw("长", &long));
        let s = summary_of(&parsed);
        assert_eq!(s.chars().count(), 201, "200 字 + 省略号");
        assert!(s.ends_with('…'));
        // 没有正文时摘要为空，界面只显示主题
        assert_eq!(summary_of(&parse_message(&raw("空", ""))), "");
    }

    #[test]
    fn full_body_keeps_paragraphs_and_content_beyond_preview_and_old_fetch_limit() {
        let text=format!("第一段\n\n{}\n\n最后一段：完整正文结束", "正文内容".repeat(10000));
        let parsed=parse_message(&raw("长邮件",&text));
        assert_eq!(body_of(&parsed),text);
        let html=parse_message(b"Content-Type: text/html; charset=utf-8\r\n\r\n<p>First paragraph</p><p>Last paragraph</p>");
        assert_eq!(body_of(&html),"First paragraph\nLast paragraph");
    }

    const ICS: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
BEGIN:VEVENT\r\n\
UID:demo-1@example.com\r\n\
SUMMARY:产品评审会\r\n\
DTSTART;TZID=Asia/Shanghai:20261030T143000\r\n\
DTEND;TZID=Asia/Shanghai:20261030T153000\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

    #[test]
    fn ics_meeting_title_and_start() {
        let m = meeting_of(ICS).expect("应当识别出会议");
        assert_eq!(m.title, "产品评审会");
        let start = chrono::Utc
            .with_ymd_and_hms(2026, 10, 30, 6, 30, 0)
            .earliest()
            .unwrap();
        assert_eq!(m.at, start.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string());
        let expect = start.timestamp();
        assert_eq!(m.start, expect);
        assert_eq!(m.end,Some(expect+3600));
    }

    #[test]
    fn ics_end_time_keeps_its_timezone_and_never_invents_a_duration() {
        let ics="BEGIN:VEVENT\nDTSTART;TZID=Asia/Shanghai:20261030T143000\nDTEND:20261030T073000Z\nEND:VEVENT";
        let meeting=meeting_of(ics).unwrap();
        assert_eq!(meeting.end,Some(meeting.start+3600));
        let ics="BEGIN:VEVENT\nDTSTART:20261030T063000Z\nDTEND;TZID=Asia/Shanghai:20261030T153000\nEND:VEVENT";
        let meeting=meeting_of(ics).unwrap();
        assert_eq!(meeting.end,Some(meeting.start+3600));
        for end in ["","DTEND:20261030T060000Z\n","DTEND:invalid\n","DTEND;TZID=Unknown/Zone:20261030T153000\n"] {
            let meeting=meeting_of(&format!("BEGIN:VEVENT\nDTSTART:20261030T063000Z\n{end}END:VEVENT")).unwrap();
            assert!(meeting.end.is_none(),"缺失或无效的结束时间应保留为空");
        }
    }

    #[test]
    fn ics_utc_and_folded_lines() {
        // 折行：CRLF + 一个空格被删掉，"Design s" + "ync" = "Design sync"
        let utc = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:Design s\n ync\nDTSTART:20261030T063000Z\nEND:VEVENT\nEND:VCALENDAR\n";
        let m = meeting_of(utc).expect("Z 时间应能解析");
        assert_eq!(m.title, "Design sync");
        let local = chrono::DateTime::from_timestamp(m.start, 0)
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%m-%d %H:%M")
            .to_string();
        assert_eq!(m.at, local);
    }

    #[test]
    fn ics_named_time_zone_uses_its_own_offset() {
        let ics = "BEGIN:VEVENT\nSUMMARY:跨时区会议\nDTSTART;TZID=America/New_York:20261030T090000\nEND:VEVENT\n";
        let m = meeting_of(ics).unwrap();
        assert_eq!(m.start, chrono::DateTime::parse_from_rfc3339("2026-10-30T13:00:00Z").unwrap().timestamp());
        let unknown = "BEGIN:VEVENT\nDTSTART;TZID=Unknown/Zone:20261030T090000\nEND:VEVENT\n";
        assert!(meeting_of(unknown).is_none());
    }

    #[test]
    fn all_day_calendar_event_has_exclusive_end() {
        let event=meeting_of("BEGIN:VEVENT\nUID:all-day\nSUMMARY:全天\nDTSTART;VALUE=DATE:20261103\nDTEND;VALUE=DATE:20261105\nEND:VEVENT").expect("全天日程应可展示");
        assert_eq!(event.end.unwrap()-event.start,2*86400);
    }

    #[test]
    fn no_ics_means_no_meeting() {
        // 普通邮件没有日历部分，不从正文猜测日期。
        let all_day = "BEGIN:VEVENT\nSUMMARY:放假\nDTSTART;VALUE=DATE:20261030\nEND:VEVENT\n";
        assert!(meeting_of(all_day).unwrap().all_day);
        assert!(meeting_of("").is_none());
        assert!(meeting_of("会议时间：10月30日 14:30").is_none());
    }

    #[test]
    fn calendar_part_is_collected_from_mime() {
        let raw = format!(
            "From: a@b.c\r\nSubject: 邀请\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=XYZ\r\n\r\n--XYZ\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n会议邀请见附件\r\n--XYZ\r\nContent-Type: text/calendar; charset=utf-8; method=REQUEST\r\n\r\n{ICS}\r\n--XYZ--\r\n"
        );
        let parsed = parse_message(raw.as_bytes());
        assert!(parsed.calendar.contains("产品评审会"));
        let m = meeting_of(&parsed.calendar).unwrap();
        assert_eq!(m.title, "产品评审会");
        assert_eq!(summary_of(&parsed), "会议邀请见附件");
    }
}

#[cfg(test)]
mod custom_endpoint_tests {
 #[test] fn microsoft_cloud_accounts_report_unsupported_before_connecting(){
  let a=Account{email:"test@hotmail.com".into(),provider:Provider::Microsoft,custom:None,enabled:true};
  let error=a.endpoint().expect_err("尚未支持的微软邮箱不能发起连接");
  assert!(error.contains("OAuth")&&error.contains("暂不支持"));
  assert!(Provider::Microsoft.label().contains("暂不支持"));
  for host in ["outlook.office365.com"," IMAP-MAIL.OUTLOOK.COM ","outlook.live.com","imap.outlook.com"]{
   let b=Account{email:"student@example.edu".into(),provider:Provider::Custom,custom:Some(CustomImap{host:host.into(),port:993,username:String::new()}),..a.clone()};
   assert_eq!(b.endpoint().unwrap_err(),error,"微软云主机应显示同一提示");
  }
  for email in ["user@outlook.com","user@hotmail.com","user@live.com"," USER@OUTLOOK.COM "]{
   let b=Account{email:email.into(),provider:Provider::Qq,custom:None,enabled:true};
   assert_eq!(b.endpoint().unwrap_err(),error,"误选服务商也不能引导用户尝试密码");
  }
 }
 use super::*;
 fn account()->Account{Account{email:"student@example.edu".into(),provider:Provider::Custom,enabled:true,custom:Some(CustomImap{host:"imap.example.edu".into(),port:993,username:"student-id".into()})}}
 #[test] fn custom_requires_tls_hostname_and_clean_identity(){
  let a=account();assert_eq!(a.endpoint().unwrap(),("imap.example.edu".into(),993,"student-id".into()));
  for host in ["https://imap.example.edu","a/b","a:993","user@host","bad\\host","-bad.example",""]{let mut b=a.clone();b.custom.as_mut().unwrap().host=host.into();assert!(b.endpoint().is_err());}
  for port in [0,143]{let mut b=a.clone();b.custom.as_mut().unwrap().port=port;assert!(b.endpoint().is_err());}
  let mut b=a.clone();b.custom.as_mut().unwrap().username="user\r\nLOGIN".into();assert!(b.endpoint().is_err());
 }
 #[test] fn credentials_are_bound_to_target_and_login(){
  let a=account();let k=a.credential_key();
  let mut b=a.clone();b.custom.as_mut().unwrap().host="other.example.edu".into();assert_ne!(k,b.credential_key());assert_ne!(a,b);
  let mut b=a.clone();b.custom.as_mut().unwrap().username="other-user".into();assert_ne!(k,b.credential_key());
  let mut b=a.clone();b.custom.as_mut().unwrap().port=1993;assert_ne!(k,b.credential_key());
  let mut b=a.clone();b.custom.as_mut().unwrap().host="IMAP.EXAMPLE.EDU".into();assert_eq!(k,b.credential_key());
  assert!(!k.contains("student"));
 }
 #[test] fn custom_config_roundtrips_without_a_secret(){
  let a=account();let raw=encode_accounts(&[a.clone()]);assert_eq!(decode_accounts(&raw),vec![a]);
  let text=String::from_utf8(raw).unwrap();assert!(!text.contains("password"));assert!(!text.contains("secret"));
 }
}
