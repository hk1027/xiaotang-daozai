//! 豆包网页会员额度。基于 0011-joe 的 #42 实现，登录状态仅在本机加密保存。
use super::{Account, Config, Provider, Snapshot, Window};
use serde_json::{json, Value};
use std::{fs, io::Read, path::PathBuf, sync::{mpsc, Arc, atomic::{AtomicBool, Ordering}}};

#[cfg(windows)]
#[path = "doubao_login.rs"]
mod login;

pub enum LoginSource { Browser, Local, Cookie(String) }

const ENDPOINT: &str = "https://www.doubao.com/alice/commerce/sale/subscription/quota/summary/";
const LOGIN_ERROR: &str = "豆包登录已失效，请重新连接浏览器账号";
const PARSE_ERROR: &str = "豆包额度接口已变化，暂时无法读取";
const MAX_BODY: u64 = 1024 * 1024;
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";

fn integer(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

/// 额度字段兼容数字和带千位分隔符的展示字符串。
fn amount(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.replace(',', "").trim().parse().ok())
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn used_percent(limit: &Value) -> Option<f64> {
    if limit["usage_exhausted"].as_bool() == Some(true) { return Some(100.0); }
    if amount(&limit["total_amount"]) == Some(0.0) { return None; }
    amount(&limit["used_percent"]).or_else(|| {
        let used = amount(&limit["used_amount"])?;
        let total = amount(&limit["total_amount"])?;
        (total > 0.0).then_some(used / total * 100.0)
    }).filter(|used| used.is_finite())
}

fn window(limit: &Value) -> Option<Window> {
    let seconds = match integer(&limit["window_type"])? {
        1 => Some(18000),
        2 => Some(604800),
        3 => None,
        _ => return None,
    };
    let used = used_percent(limit)?;
    let reset = seconds.and_then(|_| integer(&limit["end_time"])).filter(|time| *time > 0)
        .map(|time| if time >= 100_000_000_000 { time / 1000 } else { time });
    let label = match seconds { Some(18000) => "5 小时", Some(604800) => "本周", _ => "其他额度" };
    Some(Window { label: label.into(), seconds, remaining: (100.0 - used).clamp(0.0, 100.0) as f32, reset, amount: None })
}

pub fn parse(value: &Value) -> Result<Snapshot, String> {
    if let Some(code) = value.get("code") {
        let code = integer(code).ok_or(PARSE_ERROR)?;
        if code == 710012001 { return Err(LOGIN_ERROR.into()); }
        if code != 0 { return Err(format!("豆包额度服务返回错误（{code}）")); }
    }
    let data = value.get("data").filter(|data| data.is_object()).ok_or(LOGIN_ERROR)?;
    let mut windows = Vec::new();
    let mut extra = Vec::new();
    for section in ["window_limit_section", "incentive_quota_section"] {
        let groups = data.pointer(&format!("/{section}/window_limit_groups")).and_then(Value::as_array);
        for group in groups.into_iter().flatten() {
            for limit in group["window_limits"].as_array().into_iter().flatten() {
                let Some(mut window) = window(limit) else { continue };
                if section == "incentive_quota_section" {
                    window.label = if window.seconds.is_some() { format!("活动 · {}", window.label) } else { "活动额度".into() };
                }
                if let Some(name) = group["feature_group_name"].as_str().map(str::trim).filter(|name| !name.is_empty()) {
                    window.label = format!("{name} · {}", window.label);
                }
                if section == "incentive_quota_section" || integer(&limit["window_type"]) == Some(3) {
                    extra.push(window);
                } else {
                    windows.push(window);
                }
            }
        }
    }
    // 同周期存在多个权益池时，圆环保留限制最紧的一项，其余仍进入完整明细。
    windows.sort_by(|a, b| a.seconds.cmp(&b.seconds).then_with(|| a.remaining.total_cmp(&b.remaining)));
    let mut primary: Vec<Window> = Vec::new();
    for window in windows {
        if primary.iter().any(|old| old.seconds == window.seconds) { extra.push(window); }
        else { primary.push(window); }
    }
    if primary.is_empty() {
        if extra.is_empty() { return Err("豆包账号暂未提供可读取的订阅额度".into()); }
        primary = std::mem::take(&mut extra);
    }
    let plan = ["/current_subscription/display/short_name", "/current_subscription/display/product_name"]
        .into_iter().find_map(|path| data.pointer(path).and_then(Value::as_str).map(str::trim).filter(|name| !name.is_empty()))
        .unwrap_or_default();
    let plan = plan.strip_prefix("豆包").map(str::trim).unwrap_or(plan).to_owned();
    Ok(Snapshot { plan, windows: primary, extra, at: chrono::Utc::now().timestamp(), ..Default::default() })
}

fn cookie(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let raw = raw.split_once(':').filter(|(name, _)| name.eq_ignore_ascii_case("cookie")).map(|(_, value)| value).unwrap_or(raw).trim();
    if raw.is_empty() || raw.len() > 32768 || !raw.is_ascii() || raw.bytes().any(|byte| byte < 32 || byte == 127) {
        return Err("请粘贴有效的 Cookie 请求头".into());
    }
    let sessions: Vec<_> = raw.split(';').filter_map(|part| part.trim().split_once('='))
        .filter(|(name, _)| *name == "sessionid").map(|(_, value)| value).collect();
    if sessions.len() != 1 || sessions[0].len() != 32 || !sessions[0].bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Cookie 中没有唯一有效的豆包 sessionid，请重新复制".into());
    }
    Ok(raw.into())
}

fn session_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').filter_map(|part| part.trim().split_once('='))
        .find(|(key, value)| *key == name && !value.is_empty()).map(|(_, value)| value)
}

fn same_login(first: &str, second: &str) -> bool {
    match (session_value(first, "uid_tt"), session_value(second, "uid_tt")) {
        (Some(first), Some(second)) => first == second,
        _ => session_value(first, "sessionid").zip(session_value(second, "sessionid")).is_some_and(|(first, second)| first == second),
    }
}

fn session_path(slot: &str) -> Result<PathBuf, String> {
    if slot.len() != 64 || !slot.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err(LOGIN_ERROR.into()); }
    Ok(crate::model::config_dir().join("credentials").join(format!("doubao-{slot}.bin")))
}

fn save(slot: &str, header: &str) -> Result<(), String> {
    let path = session_path(slot)?;
    let raw = json!({"slot": slot, "cookie": header}).to_string();
    let encrypted = super::gemini::protect(raw.as_bytes(), true)?;
    fs::create_dir_all(path.parent().ok_or("无法保存豆包登录")?).map_err(|_| "无法保存豆包登录")?;
    crate::model::atomic_write(&path, &encrypted).map_err(|_| "无法保存豆包登录".into())
}

fn load_slot(slot: &str) -> Result<String, String> {
    let encrypted = fs::read(session_path(slot)?).map_err(|_| LOGIN_ERROR)?;
    let raw = super::gemini::protect(&encrypted, false)?;
    let value: Value = serde_json::from_slice(&raw).map_err(|_| LOGIN_ERROR)?;
    if value["slot"].as_str() != Some(slot) { return Err("豆包账号不一致，请重新连接".into()); }
    cookie(value["cookie"].as_str().ok_or(LOGIN_ERROR)?)
}

fn delete_slot(slot: &str) -> Result<(), String> {
    match fs::remove_file(session_path(slot)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("豆包加密登录清理失败，请检查数据目录权限".into()),
    }
}

pub fn forget(account: &Account) -> Result<(), String> {
    if let Some(slot) = &account.slot { delete_slot(slot)?; }
    Ok(())
}

fn request(agent: &ureq::Agent, url: &str, header: &str) -> Result<Snapshot, (String, u64)> {
    let response = agent.post(url).set("Cookie", header).set("Origin", "https://www.doubao.com")
        .set("Referer", "https://www.doubao.com/member/quota-management")
        .set("Accept", "application/json, text/plain, */*").set("Content-Type", "application/json")
        .set("User-Agent", USER_AGENT).send_string(r#"{"product_line":"membership"}"#)
        .map_err(|error| match error {
            ureq::Error::Status(401 | 403, _) => (LOGIN_ERROR.into(), 0),
            ureq::Error::Status(429, response) => ("豆包请求过于频繁，稍后自动重试".into(),
                response.header("Retry-After").and_then(|value| value.parse().ok()).unwrap_or(180).clamp(30, 3600)),
            ureq::Error::Status(code, _) => (format!("豆包额度服务暂不可用（{code}）"), 60),
            _ => ("无法连接豆包额度服务，请检查网络或代理".into(), 30),
        })?;
    if (300..400).contains(&response.status()) { return Err((LOGIN_ERROR.into(), 0)); }
    let mut body = Vec::new();
    response.into_reader().take(MAX_BODY + 1).read_to_end(&mut body).map_err(|_| ("读取豆包额度失败".into(), 30))?;
    if body.len() as u64 > MAX_BODY { return Err((PARSE_ERROR.into(), 60)); }
    let value: Value = serde_json::from_slice(&body).map_err(|_| (PARSE_ERROR.into(), 60))?;
    parse(&value).map_err(|error| { let retry = if error == LOGIN_ERROR { 0 } else { 60 }; (error, retry) })
}

pub fn fetch(agent: &ureq::Agent, account: &Account) -> Result<Snapshot, (String, u64)> {
    let header = load_slot(account.slot.as_deref().ok_or((LOGIN_ERROR.into(), 0))?).map_err(|error| (error, 0))?;
    request(agent, ENDPOINT, &header)
}

/// 验证结果不包含已落盘的凭据；只有仍打开的连接窗口才提交保存。
pub struct Validated { alias: String, header: String, snapshot: Snapshot, expected: Option<String> }
pub struct Connected { pub id: String, pub snapshot: Snapshot, pub cleanup_error: Option<String> }
impl Validated {
    pub fn finish(self, config: &mut Config, target: Option<&str>) -> Result<Connected, String> {
        let old = if let Some(id) = target {
            Some(config.accounts.iter().find(|account| account.id == id && account.provider == Provider::Doubao
                && account.slot == self.expected).cloned().ok_or("原账号已变化，请重新连接")?)
        } else {
            if self.expected.is_some() { return Err("原连接窗口已变化，请重新连接".into()); }
            config.accounts.iter().filter(|account| account.provider == Provider::Doubao)
                .find(|account| account.slot.as_deref().and_then(|slot| load_slot(slot).ok()).is_some_and(|header| same_login(&header, &self.header))).cloned()
        };
        use ring::rand::SecureRandom;
        let mut bytes = [0u8; 32];
        ring::rand::SystemRandom::new().fill(&mut bytes).map_err(|_| "无法生成安全凭据引用")?;
        let slot: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let mut candidate = config.clone();
        let label = if self.alias.trim().is_empty() {
            old.as_ref().map(|account| account.alias.clone()).unwrap_or_else(|| "豆包".into())
        } else { self.alias.trim().to_owned() };
        let id = if let Some(old) = &old {
            let account = candidate.accounts.iter_mut().find(|account| account.id == old.id).unwrap();
            account.slot = Some(slot.clone()); account.alias = label; account.enabled = true; old.id.clone()
        } else {
            let id = format!("Doubao-{slot}"); let pinned = candidate.pinned_indices().len() < 3;
            candidate.accounts.push(Account { id: id.clone(), provider: Provider::Doubao, alias: label,
                source: PathBuf::new(), slot: Some(slot.clone()), enabled: true, pinned }); id
        };
        candidate.selected = id.clone();
        save(&slot, &self.header)?;
        if let Err(error) = candidate.save_checked() { let _ = delete_slot(&slot); return Err(error); }
        *config = candidate;
        let cleanup_error = old.as_ref().filter(|old| !config.accounts.iter().any(|account| account.slot == old.slot))
            .and_then(|old| forget(old).err());
        Ok(Connected { id, snapshot: self.snapshot, cleanup_error })
    }
}

static CONNECTING: AtomicBool = AtomicBool::new(false);
struct ConnectionSlot;
impl Drop for ConnectionSlot { fn drop(&mut self) { CONNECTING.store(false, Ordering::Release); } }
pub struct Connect { rx: mpsc::Receiver<Result<Validated, String>>, cancel: Arc<AtomicBool> }
impl Drop for Connect { fn drop(&mut self) { self.cancel.store(true, Ordering::Release); } }
impl Connect {
    pub fn start(alias: String, source: LoginSource, expected: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel(); let cancel = Arc::new(AtomicBool::new(false)); let worker_cancel = cancel.clone();
        if CONNECTING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            let _ = tx.send(Err("上次豆包连接仍在结束，请稍后重试".into())); return Self { rx, cancel };
        }
        std::thread::spawn(move || {
            let _slot = ConnectionSlot;
            let result = (|| {
                let header = match source {
                    LoginSource::Cookie(header) => cookie(&header)?,
                    #[cfg(windows)]
                    LoginSource::Browser => login::browser(&worker_cancel)?,
                    #[cfg(windows)]
                    LoginSource::Local => login::local()?,
                    #[cfg(not(windows))]
                    _ => return Err("豆包官网登录暂仅支持 Windows，也可使用 Cookie 连接".into()),
                };
                if let Some(expected) = &expected {
                    if !same_login(&load_slot(expected)?, &header) { return Err("无法确认原豆包账号，请复制完整 Cookie，或切回原账号后重试".into()); }
                }
                if worker_cancel.load(Ordering::Acquire) { return Err("已取消豆包连接".into()); }
                let snapshot = request(&super::http_agent(), ENDPOINT, &header).map_err(|error| error.0)?;
                if worker_cancel.load(Ordering::Acquire) { return Err("已取消豆包连接".into()); }
                Ok(Validated { alias, header, snapshot, expected })
            })();
            let _ = tx.send(result);
        });
        Self { rx, cancel }
    }

    pub fn poll(&self) -> Option<Result<Validated, String>> { self.rx.try_recv().ok() }

    #[cfg(test)]
    pub(crate) fn pending_test() -> (Self, Arc<AtomicBool>) {
        let (_tx, rx) = mpsc::channel(); let cancel = Arc::new(AtomicBool::new(false));
        (Self { rx, cancel: cancel.clone() }, cancel)
    }

    #[cfg(test)]
    pub(crate) fn ready_test() -> (Self, Arc<AtomicBool>) {
        let (tx, rx) = mpsc::channel(); let cancel = Arc::new(AtomicBool::new(false));
        assert!(tx.send(Ok(Validated { alias: "测试".into(), header: format!("sessionid={}", "a".repeat(32)), snapshot: Snapshot::default(), expected: None })).is_ok());
        (Self { rx, cancel: cancel.clone() }, cancel)
    }
}

#[cfg(test)]
#[path = "doubao_quota_tests.rs"]
mod tests;
