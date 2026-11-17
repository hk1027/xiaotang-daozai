//! Exchange ActiveSync 14.0（企业微信、自建或第三方 Exchange）：只读取日历，不发送日程修改命令。
//! 服务器要求设备策略时只如实回报「部分执行」：岛仔不执行锁屏密码、远程擦除等管控。
use super::{Config, Failure, Fetched, Provider};
use chrono::{DateTime, Utc};
use std::io::Read;

// MS-ASWBXML 的 AirSync、Calendar、FolderHierarchy 代码页。
const AIR: u8 = 0;
const CAL: u8 = 4;
const FOLDER: u8 = 7;
const PROVISION: u8 = 14;
/// 设备未登记策略（142）、策略已更新（143）、策略标识无效（144）：需要重新握手。
const NEEDS_PROVISION: [&str; 3] = ["142", "143", "144"];

#[derive(Clone, Debug, Default)]
struct Node { page: u8, tag: u8, value: String, children: Vec<Node> }
impl Node {
    fn child(&self, page: u8, tag: u8) -> Option<&Node> { self.children.iter().find(|n| n.page == page && n.tag == tag) }
    fn text(&self, page: u8, tag: u8) -> &str { self.child(page, tag).map_or("", |n| n.value.as_str()) }
    fn required(&self, page: u8, tag: u8) -> Result<&str, Failure> {
        let value = self.text(page, tag);
        if value.is_empty() { Err(invalid()) } else { Ok(value) }
    }
}
fn invalid() -> Failure { Failure::text("日历服务器返回的数据不完整，请稍后重试") }
fn node(page: u8, tag: u8, value: &str, children: Vec<Node>) -> Node {
    Node { page, tag, value: value.into(), children }
}
fn encode(root: &Node) -> Vec<u8> {
    fn write(n: &Node, page: &mut u8, out: &mut Vec<u8>) {
        if *page != n.page { out.extend([0, n.page]); *page = n.page; }
        let content = !n.value.is_empty() || !n.children.is_empty();
        out.push(n.tag | if content { 0x40 } else { 0 });
        if !n.value.is_empty() { out.push(3); out.extend(n.value.as_bytes()); out.push(0); }
        for child in &n.children { write(child, page, out); }
        if content { out.push(1); }
    }
    let mut bytes = vec![3, 1, 106, 0];
    write(root, &mut 0, &mut bytes);
    bytes
}
fn decode(bytes: &[u8]) -> Result<Node, Failure> {
    fn integer(bytes: &[u8], at: &mut usize) -> Result<usize, Failure> {
        let mut value = 0usize;
        for _ in 0..5 {
            let b = *bytes.get(*at).ok_or_else(invalid)?; *at += 1;
            value = value.checked_mul(128).and_then(|v| v.checked_add((b & 127) as usize)).ok_or_else(invalid)?;
            if b & 128 == 0 { return Ok(value); }
        }
        Err(invalid())
    }
    let mut at = 1;
    if bytes.first() != Some(&3) || integer(bytes, &mut at)? != 1 || integer(bytes, &mut at)? != 106 || integer(bytes, &mut at)? != 0 { return Err(invalid()); }
    let mut page = 0;
    let mut stack = vec![Node::default()];
    while at < bytes.len() {
        let token = bytes[at]; at += 1;
        match token {
            0 => { page = *bytes.get(at).ok_or_else(invalid)?; at += 1; }
            1 => {
                if stack.len() <= 1 { return Err(invalid()); }
                let n = stack.pop().unwrap(); stack.last_mut().unwrap().children.push(n);
            }
            3 | 0xc3 => {
                if stack.len() <= 1 { return Err(invalid()); }
                let len = if token == 3 { bytes[at..].iter().position(|b| *b == 0).ok_or_else(invalid)? } else { integer(bytes, &mut at)? };
                let end = at.checked_add(len).filter(|end| *end <= bytes.len()).ok_or_else(invalid)?;
                stack.last_mut().unwrap().value.push_str(&String::from_utf8_lossy(&bytes[at..end]));
                at = end + usize::from(token == 3);
            }
            t if t & 0x80 == 0 && t & 0x3f >= 5 => {
                let n = node(page, t & 0x3f, "", vec![]);
                if t & 0x40 != 0 {
                    if stack.len() >= 64 { return Err(invalid()); }
                    stack.push(n);
                } else { stack.last_mut().unwrap().children.push(n); }
            }
            _ => return Err(invalid()),
        }
    }
    if stack.len() != 1 || stack[0].children.len() != 1 { return Err(invalid()); }
    Ok(stack.pop().unwrap().children.remove(0))
}

fn endpoint(cfg: &Config) -> Result<url::Url, Failure> {
    let base = url::Url::parse(&cfg.base()).map_err(|_| invalid())?;
    let local = base.host_str().is_some_and(|h| h.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()));
    if !(base.scheme() == "https" || base.scheme() == "http" && local) || !base.username().is_empty() || base.password().is_some() { return Err(Failure::text("日历服务器地址不安全")); }
    // 用户可能直接粘贴完整的 ActiveSync 地址；只填主机名时补上标准路径。
    if base.path().trim_end_matches('/').to_ascii_lowercase().ends_with("/microsoft-server-activesync") {
        let mut full = base; full.set_query(None); full.set_fragment(None);
        return Ok(full);
    }
    base.join("/Microsoft-Server-ActiveSync").map_err(|_| invalid())
}

/// 一次同步内的连接状态：服务器要求时完成设备策略握手，之后的请求带上策略标识。
struct Session<'a> { agent: ureq::Agent, cfg: &'a Config, password: &'a str, policy: Option<String>, provisioned: bool }
impl<'a> Session<'a> {
    fn new(cfg: &'a Config, password: &'a str) -> Result<Self, Failure> { Ok(Self { agent: super::agent()?, cfg, password, policy: None, provisioned: false }) }
    /// 发送命令；遇到 449 或 142–144 时先握手再重试一次。
    fn send(&mut self, name: &str, root: &Node) -> Result<Node, Failure> {
        match self.raw(name, root) {
            Err(failure) if failure.status == Some(449) && !self.provisioned => { self.provision()?; self.raw(name, root) }
            Ok(node) if !self.provisioned && NEEDS_PROVISION.contains(&top_status(&node)) => { self.provision()?; self.raw(name, root) }
            other => other,
        }
    }
    fn raw(&self, name: &str, root: &Node) -> Result<Node, Failure> {
        command(&self.agent, self.cfg, self.password, name, root, self.policy.as_deref())
    }
    fn provision(&mut self) -> Result<(), Failure> {
        self.provisioned = true;
        let label = self.cfg.provider.label();
        let refused = || Failure::text(format!("{label}要求设备满足安全策略（如锁屏密码、远程擦除），岛仔不执行这些管控；请联系管理员允许不受管控的设备"));
        let first = command(&self.agent, self.cfg, self.password, "Provision", &provision_request(None), None)?;
        // 服务器下发远程擦除时绝不执行，直接停止同步。
        if first.child(PROVISION, 0x0c).is_some() { return Err(refused()); }
        let policy = first.child(PROVISION, 6).and_then(|n| n.child(PROVISION, 7)).ok_or_else(invalid)?;
        if first.text(PROVISION, 0x0b) != "1" || policy.text(PROVISION, 0x0b) != "1" { return Err(refused()); }
        let temporary = policy.required(PROVISION, 9)?.to_string();
        let second = command(&self.agent, self.cfg, self.password, "Provision", &provision_request(Some(&temporary)), Some(&temporary))?;
        let policy = second.child(PROVISION, 6).and_then(|n| n.child(PROVISION, 7)).ok_or_else(refused)?;
        if second.text(PROVISION, 0x0b) != "1" || policy.text(PROVISION, 0x0b) != "1" { return Err(refused()); }
        self.policy = Some(policy.required(PROVISION, 9).map_err(|_| refused())?.to_string());
        Ok(())
    }
}

/// 设备策略请求。确认时如实回报「部分执行」（2）：岛仔只读日历，不执行任何设备管控。
fn provision_request(acknowledge: Option<&str>) -> Node {
    let mut policy = vec![node(PROVISION, 8, "MS-EAS-Provisioning-WBXML", vec![])];
    if let Some(key) = acknowledge { policy.extend([node(PROVISION, 9, key, vec![]), node(PROVISION, 0x0b, "2", vec![])]); }
    node(PROVISION, 5, "", vec![node(PROVISION, 6, "", vec![node(PROVISION, 7, "", policy)])])
}

/// 响应顶层的状态码（FolderSync 与 Sync 的状态标签不同）。
fn top_status(n: &Node) -> &str {
    match n.page { FOLDER => n.text(FOLDER, 0x0c), AIR => n.text(AIR, 0x0e), _ => "" }
}

fn command(agent: &ureq::Agent, cfg: &Config, password: &str, name: &str, root: &Node, policy: Option<&str>) -> Result<Node, Failure> {
    let mut target = endpoint(cfg)?;
    // 固定匿名设备标识，完整快照按顺序读取，不保存同步键或日程正文。
    target.query_pairs_mut().extend_pairs([("Cmd", name), ("User", cfg.username.as_str()), ("DeviceId", "DaozaiCalendar01"), ("DeviceType", "Daozai")]);
    let mut request = agent.post(target.as_str())
        .set("Authorization", &super::basic_auth(&cfg.username, password))
        .set("MS-ASProtocolVersion", "14.0")
        .set("Content-Type", "application/vnd.ms-sync.wbxml");
    if let Some(key) = policy { request = request.set("X-MS-PolicyKey", key); }
    let response = request.send_bytes(&encode(root)).map_err(|e| match e {
            ureq::Error::Status(code, _) => Failure::status(cfg.provider, code),
            error => Failure::transport(&error),
        })?;
    if response.status() != 200 { return Err(Failure::status(cfg.provider, response.status())); }
    let mut bytes = Vec::new();
    response.into_reader().take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|error| Failure::transport(&error))?;
    if bytes.len() > 16 * 1024 * 1024 { return Err(Failure::text(format!("{}单次返回的日程过多，无法完整同步", cfg.provider.label()))); }
    decode(&bytes)
}
fn check_status(n: &Node, page: u8, tag: u8, label: &str) -> Result<(), Failure> {
    match n.required(page, tag)? {
        "1" => Ok(()),
        code if NEEDS_PROVISION.contains(&code) => Err(Failure::text(format!("{label}要求设备满足安全策略，岛仔不执行这些管控；请联系管理员允许不受管控的设备"))),
        code => Err(Failure::text(format!("{label}同步失败（Exchange {code}），请检查日历授权后重试"))),
    }
}
fn sync_request(id: &str, key: &str) -> Node {
    let mut fields = vec![node(AIR, 0x0b, key, vec![]), node(AIR, 0x12, id, vec![])];
    if key != "0" {
        fields.extend([node(AIR, 0x13, "1", vec![]), node(AIR, 0x15, "100", vec![]),
            node(AIR, 0x17, "", vec![node(AIR, 0x18, "0", vec![])])]);
    }
    node(AIR, 5, "", vec![node(AIR, 0x1c, "", vec![node(AIR, 0x0f, "", fields)])])
}

/// FolderSync 返回的一个文件夹。
struct Folder { id: String, parent: String, kind: String }

/// 默认日历（8）、新建日历（13）一定同步；挂在日历下面的通用（1）或未知类型（18）子文件夹也是日历
/// （共享、订阅日历常这样组织）。邮件、联系人、任务等文件夹一律不碰，不下载邮件内容。
fn is_calendar(folder: &Folder, all: &[Folder]) -> bool {
    if matches!(folder.kind.as_str(), "8" | "13") { return true; }
    if !matches!(folder.kind.as_str(), "1" | "18") { return false; }
    let mut parent = folder.parent.as_str();
    for _ in 0..16 {
        let Some(up) = all.iter().find(|f| f.id == parent) else { return false; };
        if matches!(up.kind.as_str(), "8" | "13") { return true; }
        parent = up.parent.as_str();
    }
    false
}

/// 完整读取一个日历文件夹的全部日程（按页跟随，直到没有更多）。
fn sync_folder(session: &mut Session, id: &str, label: &str) -> Result<std::collections::BTreeMap<String, Node>, Failure> {
    let mut key = "0".to_string();
    let mut items = std::collections::BTreeMap::new();
    for batch in 0..101 {
        if batch == 100 { return Err(Failure::text(format!("{label}日程数量过多，无法完整同步"))); }
        let result = session.send("Sync", &sync_request(id, &key))?;
        if result.child(AIR, 0x0e).is_some() { check_status(&result, AIR, 0x0e, label)?; }
        let collection = result.child(AIR, 0x1c).and_then(|n| n.child(AIR, 0x0f)).ok_or_else(invalid)?;
        check_status(collection, AIR, 0x0e, label)?;
        let next = collection.required(AIR, 0x0b)?.to_string();
        if next == "0" || next == key && collection.child(AIR, 0x14).is_some() { return Err(invalid()); }
        if let Some(commands) = collection.child(AIR, 0x16) {
            for change in &commands.children {
                if change.page != AIR { continue; }
                let server_id = change.required(AIR, 0x0d)?;
                match change.tag {
                    7 | 8 => { items.insert(server_id.to_string(), change.child(AIR, 0x1d).ok_or_else(invalid)?.clone()); }
                    9 | 0x21 => { items.remove(server_id); }
                    _ => return Err(invalid()),
                }
            }
        }
        let initial = key == "0";
        key = next;
        if !initial && collection.child(AIR, 0x14).is_none() { break; }
    }
    Ok(items)
}

pub(super) fn fetch(cfg: &Config, password: &str, lo: DateTime<Utc>, hi: DateTime<Utc>) -> Result<Fetched, Failure> {
    let label = cfg.provider.label();
    let mut session = Session::new(cfg, password)?;
    let folders = session.send("FolderSync", &node(FOLDER, 0x16, "", vec![node(FOLDER, 0x12, "0", vec![])]))?;
    check_status(&folders, FOLDER, 0x0c, label)?;
    let changes = folders.child(FOLDER, 0x0e).ok_or_else(invalid)?;
    let all: Vec<Folder> = changes.children.iter().filter(|n| n.page == FOLDER && n.tag == 0x0f).map(|n| Folder {
        id: n.text(FOLDER, 8).to_string(), parent: n.text(FOLDER, 9).to_string(),
        kind: n.text(FOLDER, 0x0a).to_string(),
    }).collect();
    let mut events = Vec::new();
    let mut synced=0;
    let mut first_error = None;
    let mut report=super::ReadReport::default();
    for folder in &all {
        if folder.id.is_empty() || !is_calendar(folder, &all) {continue;}
        report.calendars+=1;
        // 单个日历读取失败（权限、只读共享等）不影响其他日历；全部失败才报错。
        let items = match sync_folder(&mut session, &folder.id, label) {
            Ok(items) => items,
            Err(error) => {report.failed+=1;report.status=report.status.or(error.status);report.problem(error.message.clone());first_error.get_or_insert(error);continue;}
        };
        let mut ics = String::from("BEGIN:VCALENDAR\nVERSION:2.0\n");
        for (server_id, item) in &items {
            // 个别日程的重复规则暂不支持时只跳过这一条，不拖垮整个日历。
            match event_ics(item, &format!("{}:{}:{server_id}", cfg.provider.key(), folder.id)) {
                Ok(event)=>ics.push_str(&event),Err(_)=>{report.received+=1;report.invalid+=1;report.problem("部分 Exchange 日程格式暂无法解析");}
            }
        }
        ics.push_str("END:VCALENDAR");
        let xml = format!("<calendar-data>{}</calendar-data>", ics.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"));
        let parsed=super::parse_events_in_range(&xml,lo,hi);synced+=1;events.extend(parsed.events);report.merge(parsed.report);
    }
    if report.calendars == 0 { return Err(Failure::text(if cfg.provider == Provider::WeCom {"企业微信未返回可同步的日历，请确认已开启日程同步".to_string()}else{format!("{label}账号下没有可同步的日历")})); }
    if synced == 0 { return Err(first_error.unwrap_or_else(invalid).with_report(report)); }
    if events.is_empty() && report.invalid>0{return Err(Failure::text(report.warning()).with_report(report));}
    events.sort_by_key(|m| m.start);
    let mut seen = std::collections::HashSet::new();
    events.retain(|m| seen.insert(super::dedupe_key(m)));
    Ok(Fetched { href: endpoint(cfg)?.to_string(), events, report })
}

fn date(value: &str) -> Result<DateTime<Utc>, Failure> {
    chrono::NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ").map(|d| d.and_utc()).map_err(|_| invalid())
}
fn escaped(value: &str) -> String { value.replace('\\', "\\\\").replace('\r', "").replace('\n', "\\n").replace(';', "\\;").replace(',', "\\,") }
fn recurrence(n: &Node) -> Result<String, Failure> {
    let kind = n.required(CAL, 0x1c)?;
    let freq = match kind { "0" => "DAILY", "1" => "WEEKLY", "2" | "3" => "MONTHLY", "5" | "6" => "YEARLY", _ => return Err(Failure::text("暂不支持返回的重复日程类型")) };
    if !matches!(n.text(CAL, 0x37), "" | "0" | "1") { return Err(Failure::text("暂不支持非公历的重复日程")); }
    let mut rule = format!("RRULE:FREQ={freq}");
    for (tag, name) in [(0x1f, "INTERVAL"), (0x1e, "COUNT"), (0x21, "BYMONTHDAY"), (0x23, "BYMONTH")] {
        if !n.text(CAL, tag).is_empty() {
            let value = n.text(CAL, tag).parse::<u32>().map_err(|_| invalid())?;
            if value == 0 { return Err(invalid()); }
            rule.push_str(&format!(";{name}={value}"));
        }
    }
    if !n.text(CAL, 0x1d).is_empty() { rule.push_str(&format!(";UNTIL={}", date(n.text(CAL, 0x1d))?.format("%Y%m%dT%H%M%SZ"))); }
    if !n.text(CAL, 0x20).is_empty() {
        let bits = n.text(CAL, 0x20).parse::<u8>().map_err(|_| invalid())?;
        let days: Vec<_> = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"].into_iter().enumerate().filter_map(|(i, day)| (bits & (1 << i) != 0).then_some(day)).collect();
        if days.is_empty() { return Err(invalid()); }
        rule.push_str(&format!(";BYDAY={}", days.join(",")));
    }
    if matches!(kind, "3" | "6") {
        let week = n.required(CAL, 0x22)?.parse::<i32>().map_err(|_| invalid())?;
        if !(1..=5).contains(&week) { return Err(invalid()); }
        rule.push_str(&format!(";BYSETPOS={}", if week == 5 { -1 } else { week }));
    }
    rule.push('\n');
    Ok(rule)
}
fn event_ics(item: &Node, fallback_uid: &str) -> Result<String, Failure> {
    if matches!(item.text(CAL, 0x18), "5" | "7") { return Ok(String::new()); }
    let uid = if item.text(CAL, 0x28).is_empty() { fallback_uid } else { item.text(CAL, 0x28) };
    let start = date(item.required(CAL, 0x27)?)?;
    let end = date(item.required(CAL, 0x12)?)?;
    let all_day = item.text(CAL, 6) == "1";
    let timezone = item.text(CAL, 5);
    let mut offset = 0i64;
    if !timezone.is_empty() {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(timezone).map_err(|_| invalid())?;
        if bytes.len() != 172 { return Err(invalid()); }
        let number = |at| i32::from_le_bytes(bytes[at..at+4].try_into().unwrap()) as i64;
        offset = -(number(0) + number(84)) * 60;
        if item.child(CAL, 0x1b).is_some() && bytes[154..156] != [0, 0] {
            return Err(Failure::text("暂不支持含夏令时切换的重复日程"));
        }
    }
    // 按日历时区展开重复规则，避免 UTC 日期跨日后把周几或月日算错。
    let recurring = item.child(CAL, 0x1b).is_some();
    let recurrence_zone = if recurring && offset != 0 && !all_day {
        if offset % 3600 != 0 || !(-12*3600..=14*3600).contains(&offset) { return Err(Failure::text("暂不支持此时区的重复日程")); }
        Some(format!("Etc/GMT{:+}", -offset / 3600))
    } else { None };
    let time_line = |name: &str, at: DateTime<Utc>| {
        if all_day { format!("{name};VALUE=DATE:{}\n", (at + chrono::Duration::seconds(offset)).format("%Y%m%d")) }
        else if let Some(zone) = &recurrence_zone { format!("{name};TZID={zone}:{}\n", (at + chrono::Duration::seconds(offset)).format("%Y%m%dT%H%M%S")) }
        else { format!("{name}:{}\n", at.format("%Y%m%dT%H%M%SZ")) }
    };
    let mut result = format!("BEGIN:VEVENT\nUID:{}\nSUMMARY:{}\n{}{}", escaped(uid), escaped(item.text(CAL, 0x26)), time_line("DTSTART", start), time_line("DTEND", end));
    if let Some(rule) = item.child(CAL, 0x1b) { result.push_str(&recurrence(rule)?); }
    result.push_str("END:VEVENT\n");
    if let Some(exceptions) = item.child(CAL, 0x14) {
        for exception in &exceptions.children {
            if exception.page != CAL || exception.tag != 0x13 { continue; }
            let original = date(exception.required(CAL, 0x16)?)?;
            let mut changed = item.clone();
            changed.children.retain(|n| !(n.page == CAL && matches!(n.tag, 0x1b | 0x14)));
            let duration = end - start;
            for (tag, value) in [(0x27, original), (0x12, original + duration)] {
                changed.children.retain(|n| !(n.page == CAL && n.tag == tag));
                changed.children.push(node(CAL, tag, &value.format("%Y%m%dT%H%M%SZ").to_string(), vec![]));
            }
            for field in &exception.children {
                changed.children.retain(|n| !(n.page == field.page && n.tag == field.tag));
                changed.children.push(field.clone());
            }
            let mut instance = event_ics(&changed, uid)?;
            let cancelled = exception.text(CAL, 0x15) == "1" || matches!(exception.text(CAL, 0x18), "5" | "7");
            if cancelled { instance = format!("BEGIN:VEVENT\nUID:{}\n{}STATUS:CANCELLED\nEND:VEVENT\n", escaped(uid), time_line("DTSTART", original)); }
            result.push_str(&instance.replacen("END:VEVENT", &format!("{}END:VEVENT", time_line("RECURRENCE-ID", original)), 1));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore="只读检查已连接企业微信返回的文件夹类型及日程数量，不输出凭据或正文"]
    fn live_wecom_folder_diagnosis(){
        let cfg=super::super::load_configs().into_iter().find(|c|c.provider==Provider::WeCom).expect("需要已连接的企业微信");
        let password=super::super::secrets::load(&cfg).unwrap();let agent=super::super::agent().unwrap();
        let folders=command(&agent,&cfg,&password,"FolderSync",&node(FOLDER,0x16,"",vec![node(FOLDER,0x12,"0",vec![])]),None).unwrap();
        check_status(&folders,FOLDER,0x0c,"企业微信").unwrap();
        for (i,folder) in folders.child(FOLDER,0x0e).unwrap().children.iter().enumerate(){
            println!("folder={i} tag={} type={} selected={}",folder.tag,folder.text(FOLDER,0x0a),matches!(folder.text(FOLDER,0x0a),"8"|"13"));
            if !matches!(folder.text(FOLDER,0x0a),"8"|"13"){continue;}
            let id=folder.required(FOLDER,8).unwrap();let mut key="0".to_owned();
            for page in 0..100{
                let result=command(&agent,&cfg,&password,"Sync",&sync_request(id,&key),None).unwrap();
                let collection=result.child(AIR,0x1c).and_then(|n|n.child(AIR,0x0f)).unwrap();
                check_status(collection,AIR,0x0e,"企业微信").unwrap();
                let count=collection.child(AIR,0x16).map_or(0,|n|n.children.len());
                println!("folder={i} page={page} records={count} more={}",collection.child(AIR,0x14).is_some());
                let initial=key=="0";key=collection.required(AIR,0x0b).unwrap().to_owned();
                if !initial && collection.child(AIR,0x14).is_none(){break;}
            }
        }
    }
    fn sample() -> Node {
        node(AIR, 0x1d, "", vec![node(CAL, 0x28, "test-uid", vec![]), node(CAL, 0x26, "会议<&>\n第二行", vec![]),
            node(CAL, 0x27, "20261103T020000Z", vec![]), node(CAL, 0x12, "20261103T030000Z", vec![])])
    }
    fn parsed(item: &Node) -> Vec<crate::mail::Meeting> {
        let ics = event_ics(item, "fallback").unwrap();
        let xml = format!("<calendar-data>{}</calendar-data>", ics.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"));
        super::super::events_in_range(&xml, date("20261101T000000Z").unwrap(), date("20261201T000000Z").unwrap()).unwrap()
    }
    #[test]
    fn wbxml_roundtrip_rejects_truncation_and_keeps_namespaces() {
        let root = node(AIR, 5, "", vec![sample(), node(AIR, 0x14, "", vec![])]);
        let bytes = encode(&root);
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.children[0].text(CAL, 0x26), "会议<&>\n第二行");
        assert!(decoded.child(AIR, 0x14).is_some());
        for end in 0..bytes.len() { assert!(decode(&bytes[..end]).is_err()); }
        assert!(decode(&[3,1,106,0,0x45,0xc3,0xff,0xff,0xff,0xff,0xff]).is_err());
    }
    #[test]
    fn calendar_recurring_exceptions_and_all_day_keep_correct_dates() {
        let mut item = sample();
        item.children.push(node(CAL, 0x1b, "", vec![node(CAL, 0x1c, "0", vec![]), node(CAL, 0x1e, "3", vec![])]));
        item.children.push(node(CAL, 0x14, "", vec![
            node(CAL, 0x13, "", vec![node(CAL, 0x16, "20261104T020000Z", vec![]), node(CAL, 0x15, "1", vec![])]),
            node(CAL, 0x13, "", vec![node(CAL, 0x16, "20261105T020000Z", vec![]), node(CAL, 0x27, "20261105T050000Z", vec![]), node(CAL, 0x12, "20261105T060000Z", vec![])])
        ]));
        let events = parsed(&item);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].start, date("20261105T050000Z").unwrap().timestamp());
        assert_eq!(events[1].end, Some(date("20261105T060000Z").unwrap().timestamp()));
        use base64::Engine;
        let mut timezone = [0u8; 172]; timezone[..4].copy_from_slice(&(-480i32).to_le_bytes());
        let mut all_day = sample();
        all_day.children.retain(|n| !matches!(n.tag, 0x27 | 0x12));
        all_day.children.extend([node(CAL, 6, "1", vec![]), node(CAL, 5, &base64::engine::general_purpose::STANDARD.encode(timezone), vec![]),
            node(CAL, 0x27, "20261102T160000Z", vec![]), node(CAL, 0x12, "20261103T160000Z", vec![])]);
        let ics = event_ics(&all_day, "fallback").unwrap();
        assert!(ics.contains("DTSTART;VALUE=DATE:20261103\n"));
        assert!(ics.contains("DTEND;VALUE=DATE:20261104\n"));
        assert!(parsed(&all_day)[0].all_day);
    }
    #[test]
    fn exchange_fetch_reads_only_calendar_and_follows_every_page() {
        use std::{io::Write, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for index in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0];
                while !bytes.ends_with(b"\r\n\r\n") { stream.read_exact(&mut byte).unwrap(); bytes.push(byte[0]); }
                let header = String::from_utf8(bytes).unwrap();
                let len = header.lines().find_map(|line| line.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap();
                let mut body = vec![0; len]; stream.read_exact(&mut body).unwrap();
                let request = decode(&body).unwrap();
                assert!(header.starts_with("POST /Microsoft-Server-ActiveSync?"));
                let response = if index == 0 {
                    assert_eq!((request.page, request.tag), (FOLDER, 0x16));
                    node(FOLDER, 0x16, "", vec![node(FOLDER, 0x0c, "1", vec![]), node(FOLDER, 0x0e, "", vec![
                        node(FOLDER, 0x0f, "", vec![node(FOLDER, 8, "inbox", vec![]), node(FOLDER, 0x0a, "2", vec![])]),
                        node(FOLDER, 0x0f, "", vec![node(FOLDER, 8, "calendar", vec![]), node(FOLDER, 0x0a, "8", vec![])])])])
                } else {
                    let collection = request.child(AIR, 0x1c).unwrap().child(AIR, 0x0f).unwrap();
                    assert_eq!(collection.text(AIR, 0x12), "calendar");
                    assert_eq!(collection.text(AIR, 0x0b), (index-1).to_string());
                    assert!(collection.child(AIR, 0x16).is_none(), "只读请求不发送修改命令");
                    let mut children = vec![node(AIR, 0x0e, "1", vec![]), node(AIR, 0x0b, &index.to_string(), vec![])];
                    if index == 2 { children.extend([node(AIR, 0x14, "", vec![]), node(AIR, 0x16, "", vec![node(AIR, 7, "", vec![node(AIR, 0x0d, "event", vec![]), sample()])])]); }
                    node(AIR, 5, "", vec![node(AIR, 0x1c, "", vec![node(AIR, 0x0f, "", children)])])
                };
                let body = encode(&response);
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap(); stream.write_all(&body).unwrap();
            }
        });
        let cfg = Config { provider: Provider::WeCom, username: "demo".into(), href: Some("https://caldav.wecom.work/obsolete".into()), server: Some(base) };
        let result = super::super::fetch_month(&cfg, "test", chrono::NaiveDate::from_ymd_opt(2026,11,1).unwrap()).unwrap();
        assert_eq!(result.events.len(), 1);
        server.join().unwrap();
    }
    /// 本地模拟 Exchange：按顺序处理请求，返回 (状态码, 响应)；记录每个请求的命令、策略标识和正文。
    fn mock(replies: Vec<Box<dyn Fn(&Node) -> (u16, Option<Node>) + Send>>) -> (String, std::thread::JoinHandle<Vec<(String, Option<String>, Node)>>) {
        use std::{io::Write, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
                let (mut bytes, mut byte) = (Vec::new(), [0]);
                while !bytes.ends_with(b"\r\n\r\n") { stream.read_exact(&mut byte).unwrap(); bytes.push(byte[0]); }
                let header = String::from_utf8(bytes).unwrap();
                let len = header.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap();
                let mut body = vec![0; len]; stream.read_exact(&mut body).unwrap();
                let request = decode(&body).unwrap();
                let cmd = header.split("Cmd=").nth(1).unwrap().split('&').next().unwrap().to_string();
                let key = header.lines().find_map(|l| l.to_lowercase().starts_with("x-ms-policykey:").then(|| l.split_once(':').unwrap().1.trim().to_string()));
                let (status, response) = reply(&request);
                let body = response.map(|n| encode(&n)).unwrap_or_default();
                write!(stream, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap(); stream.write_all(&body).unwrap();
                seen.push((cmd, key, request));
            }
            seen
        });
        (base, worker)
    }
    fn policy_reply(status: &str, key: &str, wipe: bool) -> Node {
        let mut children = vec![node(PROVISION, 0x0b, "1", vec![]), node(PROVISION, 6, "", vec![node(PROVISION, 7, "", vec![
            node(PROVISION, 8, "MS-EAS-Provisioning-WBXML", vec![]), node(PROVISION, 0x0b, status, vec![]), node(PROVISION, 9, key, vec![])])])];
        if wipe { children.push(node(PROVISION, 0x0c, "", vec![])); }
        node(PROVISION, 5, "", children)
    }
    fn folders() -> Node {
        node(FOLDER, 0x16, "", vec![node(FOLDER, 0x0c, "1", vec![]), node(FOLDER, 0x0e, "", vec![
            node(FOLDER, 0x0f, "", vec![node(FOLDER, 8, "calendar", vec![]), node(FOLDER, 0x0a, "8", vec![])])])])
    }
    fn sync_page(next: &str, item: bool) -> Node {
        let mut children = vec![node(AIR, 0x0e, "1", vec![]), node(AIR, 0x0b, next, vec![])];
        if item { children.push(node(AIR, 0x16, "", vec![node(AIR, 7, "", vec![node(AIR, 0x0d, "event", vec![]), sample()])])); }
        node(AIR, 5, "", vec![node(AIR, 0x1c, "", vec![node(AIR, 0x0f, "", children)])])
    }
    fn custom(base: &str) -> Config { Config { provider: Provider::Exchange, username: "me@example.com".into(), href: None, server: Some(base.into()) } }

    #[test]
    fn custom_exchange_completes_policy_handshake_and_reports_partial_compliance() {
        let (base, worker) = mock(vec![
            Box::new(|_| (449, None)),
            Box::new(|r| { assert_eq!((r.page, r.tag), (PROVISION, 5)); (200, Some(policy_reply("1", "temp-1", false))) }),
            Box::new(|r| {
                let policy = r.child(PROVISION, 6).unwrap().child(PROVISION, 7).unwrap();
                assert_eq!(policy.text(PROVISION, 9), "temp-1");
                assert_eq!(policy.text(PROVISION, 0x0b), "2", "只读客户端不执行设备管控，必须如实回报部分执行");
                (200, Some(policy_reply("1", "final-9", false)))
            }),
            Box::new(|_| (200, Some(folders()))),
            Box::new(|_| (200, Some(sync_page("1", false)))),
            Box::new(|_| (200, Some(sync_page("2", true)))),
        ]);
        let result = super::super::fetch_month(&custom(&base), "pw", chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()).unwrap();
        assert_eq!(result.events.len(), 1);
        let seen = worker.join().unwrap();
        assert_eq!(seen.iter().map(|(c, _, _)| c.as_str()).collect::<Vec<_>>(), ["FolderSync", "Provision", "Provision", "FolderSync", "Sync", "Sync"]);
        assert_eq!(seen[2].1.as_deref(), Some("temp-1"));
        assert!(seen[3..].iter().all(|(_, key, _)| key.as_deref() == Some("final-9")), "握手后的请求都要带最终策略标识");
    }

    #[test]
    fn exchange_refuses_remote_wipe_and_handshakes_on_unprovisioned_status() {
        let (base, worker) = mock(vec![Box::new(|_| (449, None)), Box::new(|_| (200, Some(policy_reply("1", "temp", true))))]);
        let error = super::super::fetch_month(&custom(&base), "pw", chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()).err().unwrap();
        assert!(error.message.contains("不执行"), "{}", error.message);
        assert_eq!(worker.join().unwrap().len(), 2, "收到远程擦除后不再继续请求");
        // 服务器用状态 142 表示设备未登记策略：同样先握手再同步。
        let (base, worker) = mock(vec![
            Box::new(|_| (200, Some(node(FOLDER, 0x16, "", vec![node(FOLDER, 0x0c, "142", vec![])])))),
            Box::new(|_| (200, Some(policy_reply("1", "temp", false)))),
            Box::new(|_| (200, Some(policy_reply("1", "final", false)))),
            Box::new(|_| (200, Some(folders()))),
            Box::new(|_| (200, Some(sync_page("1", false)))),
            Box::new(|_| (200, Some(sync_page("2", false)))),
        ]);
        assert!(super::super::fetch_month(&custom(&base), "pw", chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()).is_ok());
        assert_eq!(worker.join().unwrap()[3].1.as_deref(), Some("final"));
        // 管理员不允许不受管控的设备：如实提示，不伪装成已执行策略。
        let (base, worker) = mock(vec![Box::new(|_| (449, None)), Box::new(|_| (200, Some(policy_reply("1", "temp", false)))), Box::new(|_| (200, Some(policy_reply("2", "", false))))]);
        let error = super::super::fetch_month(&custom(&base), "pw", chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()).err().unwrap();
        assert!(error.message.contains("管理员"), "{}", error.message);
        worker.join().unwrap();
    }

    #[test]
    fn shared_and_nested_calendars_sync_while_mail_folders_are_never_touched() {
        let folder = |id: &str, parent: &str, kind: &str, name: &str| node(FOLDER, 0x0f, "", vec![node(FOLDER, 8, id, vec![]), node(FOLDER, 9, parent, vec![]), node(FOLDER, 7, name, vec![]), node(FOLDER, 0x0a, kind, vec![])]);
        let hierarchy = node(FOLDER, 0x16, "", vec![node(FOLDER, 0x0c, "1", vec![]), node(FOLDER, 0x0e, "", vec![
            folder("inbox", "0", "2", "收件箱"), folder("personal", "0", "8", "个人日历"), folder("shared", "personal", "18", "同事共享的日历"),
            folder("deny", "0", "13", "无权限日历"), folder("loose", "0", "18", "其他"), folder("mine", "0", "13", "我新建的日历")])]);
        fn id(r: &Node) -> String { r.child(AIR, 0x1c).unwrap().child(AIR, 0x0f).unwrap().text(AIR, 0x12).to_string() }
        fn key(r: &Node) -> String { r.child(AIR, 0x1c).unwrap().child(AIR, 0x0f).unwrap().text(AIR, 0x0b).to_string() }
        fn page(r: &Node, extra: Vec<Node>) -> Node {
            let next = if key(r) == "0" { "1" } else { "2" };
            let mut children = vec![node(AIR, 0x0e, "1", vec![]), node(AIR, 0x0b, next, vec![])];
            if key(r) != "0" && !extra.is_empty() { children.push(node(AIR, 0x16, "", extra)); }
            node(AIR, 5, "", vec![node(AIR, 0x1c, "", vec![node(AIR, 0x0f, "", children)])])
        }
        fn event(server: &str, uid: &str, unsupported: bool) -> Node {
            let mut item = sample();
            item.children.retain(|n| n.tag != 0x28);item.children.push(node(CAL, 0x28, uid, vec![]));
            // 非公历（农历）重复规则目前不支持，用来验证单条解析失败只跳过这一条。
            if unsupported { item.children.push(node(CAL, 0x1b, "", vec![node(CAL, 0x1c, "1", vec![]), node(CAL, 0x37, "15", vec![])])); }
            node(AIR, 7, "", vec![node(AIR, 0x0d, server, vec![]), item])
        }
        let mut replies: Vec<Box<dyn Fn(&Node) -> (u16, Option<Node>) + Send>> = vec![Box::new(move |_| (200, Some(hierarchy.clone())))];
        // 每个被选中的日历两页（初始 + 读取）；无权限日历直接返回失败状态。
        for _ in 0..2 { replies.push(Box::new(move |r| { assert_eq!(id(r), "personal"); (200, Some(page(r, vec![event("a", "uid-personal", false), event("b", "uid-lunar", true)]))) })); }
        for _ in 0..2 { replies.push(Box::new(move |r| { assert_eq!(id(r), "shared"); (200, Some(page(r, vec![event("c", "uid-shared", false)]))) })); }
        replies.push(Box::new(move |r| { assert_eq!(id(r), "deny"); (200, Some(node(AIR, 5, "", vec![node(AIR, 0x1c, "", vec![node(AIR, 0x0f, "", vec![node(AIR, 0x0e, "8", vec![]), node(AIR, 0x0b, "0", vec![])])])]))) }));
        for _ in 0..2 { replies.push(Box::new(move |r| { assert_eq!(id(r), "mine"); (200, Some(page(r, vec![event("d", "uid-mine", false)]))) })); }
        let (base, worker) = mock(replies);
        let cfg = Config { provider: Provider::WeCom, username: "demo".into(), href: None, server: Some(base) };
        let result = super::super::fetch_month(&cfg, "pw", chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()).unwrap();
        let synced: Vec<String> = worker.join().unwrap().iter().filter(|(c, _, _)| c == "Sync").map(|(_, _, r)| id(r)).collect();
        assert!(!synced.iter().any(|f| f == "inbox" || f == "loose"), "邮件和不在日历下的文件夹不能同步：{synced:?}");
        assert_eq!(result.events.len(), 3, "个人、共享、新建日历各一条；不支持的重复日程跳过，无权限日历不影响其他");
    }

    #[test]
    fn exchange_endpoint_accepts_host_or_full_activesync_address() {
        for (input, expected) in [("mail.example.com", "https://mail.example.com/Microsoft-Server-ActiveSync"),
            ("https://mail.example.com/", "https://mail.example.com/Microsoft-Server-ActiveSync"),
            ("https://mail.example.com/Microsoft-Server-ActiveSync", "https://mail.example.com/Microsoft-Server-ActiveSync"),
            ("https://mail.example.com/microsoft-server-activesync/?x=1", "https://mail.example.com/microsoft-server-activesync/")] {
            let cfg = custom(&super::super::normalize_server(input).unwrap());
            assert_eq!(endpoint(&cfg).unwrap().as_str(), expected, "{input}");
        }
    }

    #[test]
    fn exchange_status_errors_and_unsafe_endpoints_are_rejected() {
        assert!(check_status(&node(FOLDER, 0x16, "", vec![node(FOLDER, 0x0c, "9", vec![])]), FOLDER, 0x0c, "Exchange").is_err());
        assert!(check_status(&Node::default(), AIR, 0x0e, "Exchange").is_err());
        let cfg = Config { provider: Provider::WeCom, username: "demo".into(), href: None, server: Some("http://wecom.work".into()) };
        assert!(endpoint(&cfg).is_err());
    }
    #[test]
    fn calendar_weekly_rule_uses_local_weekday_across_utc_midnight() {
        use base64::Engine;
        let mut timezone = [0u8; 172]; timezone[..4].copy_from_slice(&(-480i32).to_le_bytes());
        let mut item = sample();
        item.children.retain(|n| !matches!(n.tag, 0x27 | 0x12));
        item.children.extend([node(CAL, 5, &base64::engine::general_purpose::STANDARD.encode(timezone), vec![]),
            node(CAL, 0x27, "20261102T170000Z", vec![]), node(CAL, 0x12, "20261102T180000Z", vec![]),
            node(CAL, 0x1b, "", vec![node(CAL, 0x1c, "1", vec![]), node(CAL, 0x20, "4", vec![]), node(CAL, 0x1e, "2", vec![])])]);
        let events = parsed(&item);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].start, date("20261102T170000Z").unwrap().timestamp());
        assert_eq!(events[1].start, date("20261109T170000Z").unwrap().timestamp());
    }
    #[test]
    #[ignore = "显式验证企业微信账号；仅输出数量，成功后可按环境变量保存连接"]
    fn live_wecom_connection() {
        let mut cfg = Config { provider: Provider::WeCom, username: std::env::var("DAOZAI_WECOM_USER").unwrap(), href: None, server: None };
        let password = std::env::var("DAOZAI_WECOM_PASSWORD").unwrap();
        let month = super::super::fetch_month(&cfg, &password, chrono::Local::now().date_naive()).unwrap();
        let upcoming = super::super::fetch(&cfg, &password).unwrap();
        eprintln!("wecom month_events={} upcoming_events={}", month.events.len(), upcoming.events.len());
        if std::env::var("DAOZAI_WECOM_SAVE").as_deref() == Ok("1") {
            cfg.href = Some(month.href);
            super::super::save_connection(&cfg, &password, None).unwrap();
        }
    }
}
