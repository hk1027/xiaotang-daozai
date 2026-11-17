use super::*;
use std::time::Duration;

fn response() -> Value {
    json!({"code":0,"data":{
        "current_subscription":{"display":{"short_name":"豆包 Pro"}},
        "window_limit_section":{"window_limit_groups":[{"window_limits":[
            {"window_type":1,"used_percent":28,"end_time":1900008000000i64},
            {"window_type":2,"used_percent":56,"end_time":1900604800000i64}
        ]}]},
        "incentive_quota_section":{"window_limit_groups":[{"window_limits":[
            {"window_type":3,"total_amount":"1,000","used_amount":"250","end_time":1900000000000i64}
        ]}]}
    }})
}

fn one_limit(limit: Value) -> Value {
    json!({"data":{"window_limit_section":{"window_limit_groups":[{"window_limits":[limit]}]}}})
}

#[test]
fn parses_five_hour_week_and_bonus_windows() {
    let snapshot = parse(&response()).unwrap();
    assert_eq!(snapshot.plan, "Pro");
    assert_eq!(snapshot.rings().len(), 2);
    assert_eq!(snapshot.rings()[0].label, "5 小时");
    assert_eq!(snapshot.rings()[0].remaining, 72.0);
    assert_eq!(snapshot.rings()[1].label, "本周");
    assert_eq!(snapshot.rings()[1].remaining, 44.0);
    assert_eq!(snapshot.rings()[0].reset, Some(1900008000));
    assert_eq!(snapshot.extra[0].remaining, 75.0);
    assert_eq!(snapshot.extra[0].reset, None, "活动结束时间不能冒充周期重置");
}

#[test]
fn missing_or_zero_quota_never_becomes_full_quota() {
    for limit in [
        json!({"window_type":1}),
        json!({"window_type":1,"usage_exhausted":false}),
        json!({"window_type":1,"used_amount":0,"total_amount":0}),
        json!({"window_type":1,"used_percent":0,"total_amount":0}),
        json!({"window_type":1,"used_percent":-1}),
        json!({"window_type":1,"used_percent":"NaN"}),
        json!({"window_type":1,"used_percent":"inf"}),
        json!({"window_type":1,"used_amount":-1,"total_amount":100}),
        json!({"window_type":9,"used_percent":0}),
    ] { assert!(parse(&one_limit(limit.clone())).is_err(), "{limit}"); }
    assert_eq!(parse(&one_limit(json!({"window_type":1,"used_percent":0}))).unwrap().windows[0].remaining, 100.0);
}

#[test]
fn exhausted_state_overrides_percentage_and_amounts() {
    for limit in [
        json!({"window_type":1,"used_percent":25,"usage_exhausted":true}),
        json!({"window_type":1,"usage_exhausted":true}),
        json!({"window_type":1,"used_amount":0,"total_amount":0,"usage_exhausted":true}),
    ] { assert_eq!(parse(&one_limit(limit)).unwrap().windows[0].remaining, 0.0); }
}

#[test]
fn amount_fallback_and_zero_timestamps_stay_honest() {
    let snapshot = parse(&one_limit(json!({"window_type":"1","total_amount":"3,000","used_amount":"2,100","end_time":0}))).unwrap();
    assert_eq!(snapshot.windows[0].remaining, 30.0);
    assert_eq!(snapshot.windows[0].reset, None);
    assert_eq!(parse(&one_limit(json!({"window_type":1,"used_percent":110,"end_time":1900000000}))).unwrap().windows[0].reset, Some(1900000000));
}

#[test]
fn groups_keep_both_cycles_and_every_extra_pool() {
    let value = json!({"data":{"window_limit_section":{"window_limit_groups":[
        {"feature_group_name":"图片","window_limits":[{"window_type":1,"used_percent":10},{"window_type":2,"used_percent":20}]},
        {"feature_group_name":"任务","window_limits":[{"window_type":1,"used_percent":80},{"window_type":2,"used_percent":70}]}
    ]},"incentive_quota_section":{"window_limit_groups":[{"window_limits":[{"window_type":1,"used_percent":5}]}]}}});
    let snapshot = parse(&value).unwrap();
    assert_eq!(snapshot.rings().iter().map(|window|window.seconds).collect::<Vec<_>>(), vec![Some(18000),Some(604800)]);
    assert_eq!(snapshot.windows[0].label, "任务 · 5 小时");
    assert_eq!(snapshot.windows[0].remaining, 20.0);
    assert_eq!(snapshot.extra.len(), 3);
    assert!(snapshot.extra.iter().any(|window|window.label=="活动 · 5 小时"));
    assert_eq!(snapshot.windows.len()+snapshot.extra.len(), 5);
    let mut single=one_limit(json!({"window_type":1,"used_percent":30}));
    single["data"]["window_limit_section"]["window_limit_groups"][0]["feature_group_name"]=json!("图片生成");
    assert_eq!(parse(&single).unwrap().windows[0].label,"图片生成 · 5 小时","只有一个权益池也必须保留适用范围");
}

#[test]
fn activity_only_and_empty_plan_names_are_supported() {
    let value = json!({"data":{"current_subscription":{"display":{"short_name":null,"product_name":"豆包 Plus"}},
        "incentive_quota_section":{"window_limit_groups":[{"window_limits":[{"window_type":3,"used_percent":30}]}]}}});
    let snapshot = parse(&value).unwrap();
    assert_eq!(snapshot.plan, "Plus");
    assert_eq!(snapshot.rings()[0].label, "活动额度");
    assert_eq!(snapshot.rings()[0].remaining, 70.0);
    let unclassified = parse(&one_limit(json!({"window_type":3,"used_percent":30,"end_time":1900000000000i64}))).unwrap();
    assert_eq!(unclassified.windows[0].label,"其他额度","未知窗口不能仅按数字推断为活动或月额度");
    assert_eq!(unclassified.windows[0].reset,None);
}

#[test]
fn missing_data_and_business_errors_do_not_echo_private_messages() {
    for bad in [json!({}),json!({"data":{}}),json!({"code":"unknown","data":{}})] { assert!(parse(&bad).is_err()); }
    assert_eq!(parse(&json!({"code":710012001,"message":"private-secret"})).unwrap_err(), LOGIN_ERROR);
    let error = parse(&json!({"code":123,"message":"private-secret","data":response()["data"]})).unwrap_err();
    assert!(error.contains("123") && !error.contains("private-secret"));
}

fn header(session: char, uid: Option<&str>) -> String {
    let mut header = format!("sessionid={}", session.to_string().repeat(32));
    if let Some(uid) = uid { header.push_str(&format!("; uid_tt={uid}")); }
    header
}

#[test]
fn optional_uid_and_session_rotation_do_not_break_reconnection() {
    assert!(same_login(&header('a',None), &header('a',Some("user-1"))));
    assert!(same_login(&header('a',Some("user-1")), &header('a',None)));
    assert!(same_login(&header('a',Some("user-1")), &header('b',Some("user-1"))));
    assert!(!same_login(&header('a',Some("user-1")), &header('b',Some("user-2"))));
    assert!(!same_login(&header('a',None), &header('b',None)));
    assert!(!same_login(&header('a',Some("user-1")), &header('a',Some("user-2"))));
    assert!(!same_login("", ""));
}

#[test]
fn cookie_requires_a_unique_session_and_rejects_header_injection() {
    let valid = header('a',None);
    assert_eq!(cookie(&format!("COOKIE: {valid}")).unwrap(),valid);
    for bad in ["".into(),"foo=bar".into(),"sessionid=short".into(),
        format!("{valid}\r\nX: private"),format!("{valid}; {valid}"),format!("{valid}; x=中文")] {
        assert!(cookie(&bad).is_err(), "{bad:?}");
    }
    assert!(session_path("../../private").is_err());
}

fn server(status: &str, extra_headers: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
    use std::{io::Write, net::TcpListener};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/quota", listener.local_addr().unwrap());
    let response = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",body.len());
    let task = std::thread::spawn(move || {
        let (mut stream,_) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut buffer = [0u8;4096]; let count = stream.read(&mut buffer).unwrap();
            if count == 0 { break; } bytes.extend_from_slice(&buffer[..count]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&bytes[..end]);
                let size = head.lines().find_map(|line|line.to_ascii_lowercase().strip_prefix("content-length:").and_then(|value|value.trim().parse::<usize>().ok())).unwrap_or(0);
                if bytes.len() >= end+4+size { break; }
            }
        }
        let _ = stream.write_all(response.as_bytes());
        String::from_utf8(bytes).unwrap()
    });
    (url,task)
}

fn agent() -> ureq::Agent { ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).redirects(0).build() }

#[test]
fn requests_use_json_and_never_replay_the_authors_device_identity() {
    let (url,task) = server("200 OK","",&response().to_string());
    assert_eq!(request(&agent(),&url,&header('a',None)).unwrap().windows[0].remaining,72.0);
    let raw = task.join().unwrap();
    assert!(raw.starts_with("POST /quota HTTP/1.1"));
    assert!(raw.to_ascii_lowercase().contains("content-type: application/json"));
    assert!(raw.ends_with(r#"{"product_line":"membership"}"#));
    assert!(!raw.contains("177462281012986") && !raw.contains("7662832405736687142"));
    assert!(!ENDPOINT.contains('?'));
}

#[test]
fn http_errors_redirects_and_limits_are_bounded_and_private() {
    for (status,headers,body,retry) in [
        ("401 Unauthorized","","private-secret",0),
        ("403 Forbidden","","private-secret",0),
        ("302 Found","Location: http://127.0.0.1:1/private\r\n","private-secret",0),
        ("429 Too Many Requests","Retry-After: 99999\r\n","private-secret",3600),
        ("503 Unavailable","","private-secret",60),
        ("200 OK","",r#"{"code":710012001,"message":"private-secret"}"#,0),
        ("200 OK","","<html>private-secret</html>",60),
    ] {
        let (url,task) = server(status,headers,body);
        let error = request(&agent(),&url,&header('a',None)).unwrap_err();
        assert_eq!(error.1,retry,"{status}"); assert!(!error.0.contains("private-secret"));
        task.join().unwrap();
    }
    let (url,task) = server("200 OK","","x".repeat(MAX_BODY as usize+10).as_str());
    assert_eq!(request(&agent(),&url,&header('a',None)).unwrap_err().0,PARSE_ERROR);task.join().unwrap();
}

fn with_data_dir(run: impl FnOnce()) {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let dir = root.join(format!("daozai-doubao-tests-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir(&dir).unwrap();
    let previous = std::env::var_os("DAOZAI_DATA_DIR"); std::env::set_var("DAOZAI_DATA_DIR",&dir);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
    match previous { Some(previous)=>std::env::set_var("DAOZAI_DATA_DIR",previous),None=>std::env::remove_var("DAOZAI_DATA_DIR") }
    assert!(dir.canonicalize().unwrap().starts_with(&root));
    fs::remove_dir_all(&dir).unwrap();
    if let Err(error) = result { std::panic::resume_unwind(error); }
}

fn validated(header: String, expected: Option<String>) -> Validated {
    Validated { alias:"测试豆包".into(),header,snapshot:parse(&response()).unwrap(),expected }
}

#[cfg(windows)]
#[test]
fn encrypted_storage_and_duplicate_reconnection_preserve_account_settings() {
    with_data_dir(|| {
        let mut config = Config::default();
        let first = validated(header('a',None),None).finish(&mut config,None).unwrap();
        let old_slot = config.accounts[0].slot.clone().unwrap();
        config.accounts[0].pinned = false; config.colors.insert(first.id.clone(),super::super::Accent::Gold);
        let bytes = fs::read(session_path(&old_slot).unwrap()).unwrap();
        assert!(!bytes.windows(32).any(|part|part == b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert_eq!(load_slot(&old_slot).unwrap(),header('a',None));
        let second = validated(header('a',Some("user-1")),None).finish(&mut config,None).unwrap();
        assert_eq!(first.id,second.id);assert_eq!(config.accounts.len(),1);assert!(!config.accounts[0].pinned);
        assert_eq!(config.colors[&first.id],super::super::Accent::Gold);assert!(!session_path(&old_slot).unwrap().exists());
        let old_slot = config.accounts[0].slot.clone().unwrap();
        let third = validated(header('b',Some("user-1")),Some(old_slot.clone())).finish(&mut config,Some(&first.id)).unwrap();
        assert_eq!(third.id,first.id);assert!(!session_path(&old_slot).unwrap().exists());
        assert!(!fs::read_to_string(crate::model::config_dir().join("ai-quota.json")).unwrap().contains("sessionid"));
        forget(&config.accounts[0]).unwrap();
        assert!(!session_path(config.accounts[0].slot.as_deref().unwrap()).unwrap().exists());
    });
}

#[cfg(windows)]
#[test]
fn config_write_failure_keeps_the_original_session_and_removes_the_unused_one() {
    with_data_dir(|| {
        let mut config = Config::default();
        let connected = validated(header('a',Some("user-1")),None).finish(&mut config,None).unwrap();
        let old = config.clone();let path=crate::model::config_dir().join("ai-quota.json");
        fs::remove_file(&path).unwrap();fs::create_dir(&path).unwrap();
        let slot = old.accounts[0].slot.clone().unwrap();
        assert!(validated(header('b',Some("user-1")),Some(slot.clone())).finish(&mut config,Some(&connected.id)).is_err());
        assert_eq!(config,old);assert_eq!(load_slot(&slot).unwrap(),header('a',Some("user-1")));
        assert_eq!(fs::read_dir(crate::model::config_dir().join("credentials")).unwrap().count(),1);
        let before = config.clone();
        assert!(validated(header('b',Some("user-1")),Some("changed".into())).finish(&mut config,Some(&connected.id)).is_err());
        assert_eq!(config,before);
    });
}

#[test]
fn dropping_a_connection_cancels_it_and_busy_validation_does_not_spawn_more_workers() {
    let (connection,cancel) = Connect::pending_test();drop(connection);assert!(cancel.load(Ordering::Acquire));
    assert!(!CONNECTING.swap(true,Ordering::AcqRel));let _guard=ConnectionSlot;
    let connection = Connect::start(String::new(),LoginSource::Cookie(header('a',None)),None);
    assert!(connection.poll().unwrap().err().unwrap().contains("上次"));
}
