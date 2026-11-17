use super::*;
use ring::{aead, digest};

fn encrypted(host: &str, value: &str, version: u32) -> Vec<u8> {
    let mut raw = if version >= 24 { digest::digest(&digest::SHA256, host.as_bytes()).as_ref().to_vec() } else { Vec::new() };
    raw.extend_from_slice(value.as_bytes());
    let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &[7; 32]).unwrap());
    key.seal_in_place_append_tag(aead::Nonce::assume_unique_for_key([3; 12]), aead::Aad::empty(), &mut raw).unwrap();
    [b"v10".as_slice(), &[3; 12], &raw].concat()
}

#[test]
fn native_cookie_decryption_checks_version_host_and_integrity() {
    for version in [23, 24] {
        let blob = encrypted(".doubao.com", "test-session", version);
        assert_eq!(decrypt(&[7; 32], ".doubao.com", &blob, version).unwrap(), "test-session");
        assert!(decrypt(&[8; 32], ".doubao.com", &blob, version).is_err());
        if version >= 24 { assert!(decrypt(&[7; 32], "www.doubao.com", &blob, version).is_err()); }
        let mut damaged = blob; damaged[18] ^= 1;
        assert!(decrypt(&[7; 32], ".doubao.com", &damaged, version).is_err());
    }
    for bad in [b"v20unknown".as_slice(), b"v10", b"plain"] { assert!(decrypt(&[7; 32], ".doubao.com", bad, 24).is_err()); }
}

#[test]
fn session_collection_limits_domain_path_expiry_and_duplicate_scope() {
    let session = |domain, path, value: &str| json!({"name":"sessionid","value":value.repeat(32),"domain":domain,"path":path});
    let cookies = vec![
        session(".doubao.com", "/", "a"), session("www.doubao.com", "/", "b"),
        session(".doubao.com", "/other", "c"), session("doubao.com.evil.invalid", "/", "d"),
        json!({"name":"other","value":"private","domain":"example.com","path":"/"}),
        json!({"name":"root_host_only","value":"private","domain":"doubao.com","path":"/"}),
        json!({"name":"expired","value":"private","domain":".doubao.com","path":"/","expires":1}),
        json!({"name":"injected","value":"x; stolen=y","domain":".doubao.com","path":"/"}),
    ];
    assert_eq!(header(&cookies).unwrap(), format!("sessionid={}", "b".repeat(32)));
    assert!(header(&cookies[2..]).is_err());
    assert!(header(&[session("doubao.com", "/", "a")]).is_err());
    assert!(header(&[session(".www.doubao.com", "/", "b")]).is_ok());
    assert!(header(&[session(".doubao.com", "/alice/commerce/sale/subscription/quot", "e")]).is_err());
    assert!(header(&[session(".doubao.com", "/alice/commerce/", "f")]).is_ok());
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("daozai-doubao-{label}-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
    fs::create_dir_all(&dir).unwrap(); dir
}

#[test]
fn native_import_uses_current_profile_without_reading_other_profiles() {
    let root = temp_dir("profile");
    for bad in ["../other", "C:\\other", "..", "a/b", "a\\b", ""] {
        fs::write(root.join("Local State"), json!({"profile":{"last_used":bad}}).to_string()).unwrap();
        assert!(profile(&root).is_err());
    }
    fs::write(root.join("Local State"), r#"{"profile":{"last_used":"Profile 3"}}"#).unwrap();
    assert_eq!(profile(&root).unwrap(), root.join("Profile 3/Network/Cookies"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_database_import_is_read_only_and_excludes_unrelated_and_expired_cookies() {
    use std::ffi::CString;
    let root = temp_dir("db"); let path = root.join("Cookies");
    let blob: String = encrypted(".doubao.com", &"a".repeat(32), 24).iter().map(|b|format!("{b:02x}")).collect();
    let sql = CString::new(format!("CREATE TABLE meta(key TEXT,value TEXT); INSERT INTO meta VALUES('version','24'); CREATE TABLE cookies(host_key TEXT,name TEXT,path TEXT,encrypted_value BLOB,value TEXT,expires_utc INTEGER); INSERT INTO cookies VALUES('.doubao.com','sessionid','/',X'{blob}','',0),('.doubao.com','expired','/',X'','private',1),('example.com','other','/',X'','private',0),('.doubao.com','wrong_path','/other',X'','private',0);")).unwrap();
    let name = CString::new(path.to_str().unwrap()).unwrap(); let mut db = std::ptr::null_mut();
    unsafe {
        assert_eq!(sqlite3_open_v2(name.as_ptr(), &mut db, 6, std::ptr::null()), 0);
        assert_eq!(sqlite3_exec(db, sql.as_ptr(), None, std::ptr::null_mut(), std::ptr::null_mut()), 0);
        sqlite3_close(db);
    }
    let before = fs::read(&path).unwrap();
    assert_eq!(read_native(&path, &[7; 32]).unwrap(), format!("sessionid={}", "a".repeat(32)));
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_file(&path).unwrap();
    assert!(read_native(&path, &[7; 32]).is_err()); assert!(!path.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn devtools_address_comes_from_the_owned_profile_and_rejects_other_targets() {
    assert_eq!(endpoint("1234\n/devtools/browser/random\n").unwrap(), (1234, "ws://127.0.0.1:1234/devtools/browser/random".into()));
    for bad in ["0\n/devtools/browser/a", "65536\n/devtools/browser/a", "1234\nhttps://evil.invalid", "1234\n/json", "1234\n/devtools/browser/a?redirect=evil"] {
        assert!(endpoint(bad).is_err());
    }
}

#[test]
fn cdp_handles_events_and_fragmented_responses_without_exposing_remote_errors() {
    use std::net::TcpListener;
    use tungstenite::protocol::frame::{Frame, coding::{Data, OpCode}};
    for failure in [false, true] {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap(); let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap(); let mut socket = tungstenite::accept(stream).unwrap();
            let request: Value = serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(request["method"], "Storage.getCookies");
            socket.send(Message::Text(r#"{"method":"event"}"#.into())).unwrap();
            if failure { socket.send(Message::Text(r#"{"id":1,"error":{"message":"private-secret"}}"#.into())).unwrap(); }
            else {
                socket.send(Message::Frame(Frame::message(b"{\"id\":1,".to_vec(), OpCode::Data(Data::Text), false))).unwrap();
                socket.send(Message::Frame(Frame::message(b"\"result\":{\"cookies\":[]}}".to_vec(), OpCode::Data(Data::Continue), true))).unwrap();
            }
        });
        let result = cdp(port, &format!("ws://127.0.0.1:{port}/devtools/browser/test"), "Storage.getCookies");
        if failure { assert!(!result.unwrap_err().contains("private-secret")); }
        else { assert_eq!(result.unwrap(), json!({"cookies":[]})); }
        worker.join().unwrap();
    }
}

#[test]
fn cancellation_prevents_launching_the_browser() {
    assert_eq!(browser(&AtomicBool::new(true)).unwrap_err(), CANCELLED);
}

#[test]
#[ignore = "使用独立无头 Chrome／Edge 验证实际 CDP 通道及进程目录清理"]
fn doubao_browser_transport_local() {
    let root = temp_dir("browser");
    let child = Command::new(find_browser().expect("本机 Chrome／Edge"))
        .arg(format!("--user-data-dir={}", root.display()))
        .args(["--headless=new", "--remote-debugging-port=0", "--remote-debugging-address=127.0.0.1", "--no-first-run", "--no-default-browser-check", "about:blank"])
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let mut browser = Browser { child, profile: root.clone() };
    let deadline = Instant::now() + Duration::from_secs(20);
    let (port, url) = loop {
        assert!(browser.child.try_wait().unwrap().is_none());
        if let Ok(raw) = fs::read_to_string(root.join("DevToolsActivePort")) { break endpoint(&raw).unwrap(); }
        assert!(Instant::now() < deadline, "独立浏览器启动超时");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(cdp(port, &url, "Storage.getCookies").unwrap()["cookies"].as_array().unwrap().is_empty());
    let pid = browser.child.id();
    drop(browser);
    assert!(!root.exists(), "登录浏览器临时目录应已清理");
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err(), "独立浏览器端口应已关闭：{pid}");
}

#[test]
#[ignore = "只读验证本机豆包当前登录及额度，不保存凭据"]
fn doubao_local_live_read_only() {
    let header = local().expect("读取本机豆包当前登录");
    let snapshot = super::super::request(&super::super::super::http_agent(), ENDPOINT, &header).map_err(|error|error.0).expect("只读查询豆包额度");
    assert!(!snapshot.windows.is_empty());
    println!("本机豆包识别及额度查询通过：{} 个周期，{} 项额外额度", snapshot.windows.len(), snapshot.extra.len());
}
