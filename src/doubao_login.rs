//! 豆包独立官网登录及本机客户端导入，不依赖浏览器扩展。
use super::{cookie, ENDPOINT, MAX_BODY};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, net::{Ipv4Addr, SocketAddr, TcpStream}, os::windows::process::CommandExt,
    path::{Path, PathBuf}, process::{Child, Command, Stdio}, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};
use tungstenite::{client::client_with_config, protocol::WebSocketConfig, Message};

const LOCAL_ERROR: &str = "无法读取本机豆包当前登录，请使用浏览器登录；无需安装扩展";
const BROWSER_ERROR: &str = "无法连接豆包登录窗口，请关闭该窗口后重试";
const CANCELLED: &str = "已取消豆包连接";

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) { Err(CANCELLED.into()) } else { Ok(()) }
}

fn header(cookies: &[Value]) -> Result<String, String> {
    let now = chrono::Utc::now().timestamp() as f64;
    let path = url::Url::parse(ENDPOINT).unwrap().path().to_owned();
    let mut cookies: Vec<_> = cookies.iter().filter(|entry| {
        let domain = entry["domain"].as_str().unwrap_or_default();
        let scope = entry["path"].as_str().unwrap_or("/");
        matches!(domain, ".doubao.com" | ".www.doubao.com" | "www.doubao.com")
            && (scope == path || path.strip_prefix(scope).is_some_and(|rest| scope.ends_with('/') || rest.starts_with('/')))
            && entry["expires"].as_f64().is_none_or(|time| time <= 0.0 || time > now)
    }).collect();
    cookies.sort_by_key(|entry| std::cmp::Reverse((entry["path"].as_str().unwrap_or("/").len(), entry["domain"].as_str().unwrap_or_default().len())));
    let mut pairs = BTreeMap::new();
    for entry in cookies {
        let (Some(name), Some(value)) = (entry["name"].as_str(), entry["value"].as_str()) else { continue };
        if name.is_empty() || name.bytes().any(|b| !b.is_ascii_alphanumeric() && !b"_-".contains(&b))
            || value.contains(';') || value.is_empty() { continue; }
        pairs.entry(name).or_insert(value);
    }
    cookie(&pairs.into_iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join("; "))
}

fn find_browser() -> Option<PathBuf> {
    use windows::{core::{w, PCWSTR, PWSTR}, Win32::UI::Shell::{AssocQueryStringW, ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE}};
    let mut buffer = [0u16; 4096]; let mut length = buffer.len() as u32;
    if unsafe { AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, w!("https"), PCWSTR::null(), PWSTR(buffer.as_mut_ptr()), &mut length) }.is_ok() {
        let path = PathBuf::from(String::from_utf16_lossy(&buffer[..buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len())]));
        if path.is_file() && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.eq_ignore_ascii_case("chrome.exe") || name.eq_ignore_ascii_case("msedge.exe")) { return Some(path); }
    }
    ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"].into_iter().filter_map(std::env::var_os)
        .flat_map(|root| ["Microsoft/Edge/Application/msedge.exe", "Google/Chrome/Application/chrome.exe"].map(|name| PathBuf::from(&root).join(name)))
        .find(|path| path.is_file())
}

struct Browser { child: Child, profile: PathBuf }
impl Drop for Browser {
    fn drop(&mut self) {
        // 只结束本次使用独立目录启动的进程树，不按浏览器进程名清理。
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = Command::new("taskkill.exe").args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .creation_flags(0x08000000).stdout(Stdio::null()).stderr(Stdio::null()).status();
            let _ = self.child.kill(); let _ = self.child.wait();
        }
        let _ = fs::remove_dir_all(&self.profile);
    }
}

fn endpoint(raw: &str) -> Result<(u16, String), String> {
    let mut lines = raw.lines();
    let port: u16 = lines.next().unwrap_or_default().parse().ok().filter(|port| *port > 0).ok_or(BROWSER_ERROR)?;
    let path = lines.next().filter(|path| path.starts_with("/devtools/browser/") && !path.contains(['?', '#', '\r', '\n']))
        .ok_or(BROWSER_ERROR)?;
    Ok((port, format!("ws://127.0.0.1:{port}{path}")))
}

fn cdp(port: u16, url: &str, method: &str) -> Result<Value, String> {
    let stream = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), Duration::from_secs(2)).map_err(|_| BROWSER_ERROR)?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).map_err(|_| BROWSER_ERROR)?;
    stream.set_write_timeout(Some(Duration::from_secs(2))).map_err(|_| BROWSER_ERROR)?;
    let config = WebSocketConfig::default().read_buffer_size(8192).max_message_size(Some(MAX_BODY as usize)).max_frame_size(Some(MAX_BODY as usize));
    let (mut socket, _) = client_with_config(url, stream, Some(config)).map_err(|_| BROWSER_ERROR)?;
    socket.send(Message::Text(json!({"id": 1, "method": method, "params": {}}).to_string().into())).map_err(|_| BROWSER_ERROR)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        match socket.read().map_err(|_| BROWSER_ERROR)? {
            Message::Text(raw) => {
                let value: Value = serde_json::from_str(&raw).map_err(|_| BROWSER_ERROR)?;
                if value["id"] == 1 {
                    let _ = socket.close(None);
                    return value.get("result").cloned().ok_or_else(|| BROWSER_ERROR.into());
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    Err(BROWSER_ERROR.into())
}

pub(super) fn browser(cancel: &AtomicBool) -> Result<String, String> {
    check_cancel(cancel)?;
    let exe = find_browser().ok_or("未找到 Chrome 或 Edge，请先安装其中一个浏览器，或从本机豆包客户端导入")?;
    use ring::rand::SecureRandom;
    let mut random = [0u8; 16];
    ring::rand::SystemRandom::new().fill(&mut random).map_err(|_| BROWSER_ERROR)?;
    let name: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let profile = crate::model::config_dir().join("browser-login").join(name);
    fs::create_dir_all(&profile).map_err(|_| "无法创建豆包独立登录目录")?;
    let child = match Command::new(exe).arg(format!("--user-data-dir={}", profile.display()))
        .args(["--remote-debugging-port=0", "--remote-debugging-address=127.0.0.1", "--no-first-run", "--no-default-browser-check", "--disable-background-mode", "--new-window", "https://www.doubao.com/chat/"])
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(child) => child,
        Err(_) => { let _ = fs::remove_dir_all(&profile); return Err("无法打开豆包登录窗口".into()); }
    };
    let mut browser = Browser { child, profile };
    let start = Instant::now();
    let mut address = None;
    while start.elapsed() < Duration::from_secs(600) {
        check_cancel(cancel)?;
        if browser.child.try_wait().map_err(|_| BROWSER_ERROR)?.is_some() { return Err("豆包登录窗口已关闭，未保存登录".into()); }
        if address.is_none() {
            address = fs::read_to_string(browser.profile.join("DevToolsActivePort")).ok().and_then(|raw| endpoint(&raw).ok());
            if start.elapsed() > Duration::from_secs(20) && address.is_none() { return Err(BROWSER_ERROR.into()); }
        }
        if let Some((port, url)) = &address {
            if let Ok(value) = cdp(*port, url, "Storage.getCookies") {
                if let Some(cookies) = value["cookies"].as_array() {
                    if let Ok(header) = header(cookies) {
                        // 登录 Cookie 会分批写入，等同账号标识补齐再验证额度。
                        if super::session_value(&header, "uid_tt").is_some() && super::session_value(&header, "sessionid_ss").is_some() {
                            check_cancel(cancel)?;
                            return Ok(header);
                        }
                    }
                }
            }
        }
        for _ in 0..10 { check_cancel(cancel)?; std::thread::sleep(Duration::from_millis(100)); }
    }
    Err("豆包登录等待超时，请重新登录".into())
}

fn profile(root: &Path) -> Result<PathBuf, String> {
    let raw = fs::read(root.join("Local State")).map_err(|_| LOCAL_ERROR)?;
    let value: Value = serde_json::from_slice(&raw).map_err(|_| LOCAL_ERROR)?;
    let name = value.pointer("/profile/last_used").and_then(Value::as_str).unwrap_or("Default");
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) { return Err(LOCAL_ERROR.into()); }
    Ok(root.join(name).join("Network/Cookies"))
}

fn decrypt(key: &[u8], host: &str, blob: &[u8], version: u32) -> Result<String, String> {
    use ring::{aead, digest};
    if !blob.starts_with(b"v10") || blob.len() < 31 { return Err(LOCAL_ERROR.into()); }
    let nonce = aead::Nonce::try_assume_unique_for_key(&blob[3..15]).map_err(|_| LOCAL_ERROR)?;
    let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, key).map_err(|_| LOCAL_ERROR)?);
    let mut sealed = blob[15..].to_vec();
    let plain = key.open_in_place(nonce, aead::Aad::empty(), &mut sealed).map_err(|_| LOCAL_ERROR)?;
    let plain = if version >= 24 { plain.strip_prefix(digest::digest(&digest::SHA256, host.as_bytes()).as_ref()).ok_or(LOCAL_ERROR)? } else { plain };
    String::from_utf8(plain.to_vec()).map_err(|_| LOCAL_ERROR.into())
}

type RowCallback = unsafe extern "C" fn(*mut std::ffi::c_void, std::ffi::c_int, *mut *mut std::ffi::c_char, *mut *mut std::ffi::c_char) -> std::ffi::c_int;
#[link(name = "winsqlite3")]
extern "C" {
    fn sqlite3_open_v2(name: *const std::ffi::c_char, db: *mut *mut std::ffi::c_void, flags: std::ffi::c_int, vfs: *const std::ffi::c_char) -> std::ffi::c_int;
    fn sqlite3_exec(db: *mut std::ffi::c_void, sql: *const std::ffi::c_char, callback: Option<RowCallback>, data: *mut std::ffi::c_void, error: *mut *mut std::ffi::c_char) -> std::ffi::c_int;
    fn sqlite3_close(db: *mut std::ffi::c_void) -> std::ffi::c_int;
}

fn read_native(path: &Path, key: &[u8]) -> Result<String, String> {
    use std::ffi::{CStr, CString, c_char, c_int, c_void};
    let path = CString::new(path.to_string_lossy().as_bytes()).map_err(|_| LOCAL_ERROR)?;
    let now = (chrono::Utc::now().timestamp() + 11_644_473_600) * 1_000_000;
    let sql = CString::new(format!("SELECT host_key,name,path,hex(encrypted_value),value,(SELECT value FROM meta WHERE key='version') FROM cookies WHERE host_key IN ('.doubao.com','.www.doubao.com','www.doubao.com') AND (expires_utc=0 OR expires_utc>{now}) LIMIT 256")).unwrap();
    unsafe extern "C" fn collect(data: *mut c_void, count: c_int, values: *mut *mut c_char, _: *mut *mut c_char) -> c_int {
        if count != 6 || values.is_null() { return 1; }
        let row = (0..count as usize).map(|i| {
            let value = unsafe { *values.add(i) };
            if value.is_null() { String::new() } else { unsafe { CStr::from_ptr(value) }.to_string_lossy().into_owned() }
        }).collect::<Vec<_>>();
        unsafe { &mut *data.cast::<Vec<Vec<String>>>() }.push(row); 0
    }
    let mut db = std::ptr::null_mut(); let mut rows: Vec<Vec<String>> = Vec::new();
    let success = unsafe {
        let success = sqlite3_open_v2(path.as_ptr(), &mut db, 1, std::ptr::null()) == 0
            && sqlite3_exec(db, sql.as_ptr(), Some(collect), (&mut rows as *mut Vec<Vec<String>>).cast(), std::ptr::null_mut()) == 0;
        if !db.is_null() { sqlite3_close(db); } success
    };
    if !success { return Err(LOCAL_ERROR.into()); }
    let mut cookies = Vec::new();
    for row in rows {
        let value = if row[3].is_empty() { row[4].clone() } else {
            let bytes = (0..row[3].len()).step_by(2).map(|i| u8::from_str_radix(&row[3][i..i + 2], 16)).collect::<Result<Vec<_>, _>>().map_err(|_| LOCAL_ERROR)?;
            decrypt(key, &row[0], &bytes, row[5].parse().map_err(|_| LOCAL_ERROR)?)?
        };
        cookies.push(json!({"domain": row[0], "name": row[1], "path": row[2], "value": value}));
    }
    header(&cookies).map_err(|_| LOCAL_ERROR.into())
}

pub(super) fn local() -> Result<String, String> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or(LOCAL_ERROR)?;
    for app in ["Doubao", "DoubaoWork"] {
        let root = Path::new(&base).join(app).join("User Data");
        let Ok(path) = profile(&root) else { continue };
        if !path.is_file() { continue; }
        let Ok(key) = super::super::native_auth::key(&root.join("Local State")) else { continue };
        if let Ok(header) = read_native(&path, &key) { return Ok(header); }
    }
    Err(LOCAL_ERROR.into())
}

#[cfg(test)]
#[path = "doubao_login_tests.rs"]
mod tests;
