//! 酷狗未实现系统跳转时，使用客户端自己的本机播放控制接口。
use std::{path::{Path,PathBuf},time::Duration};
use base64::Engine;

fn player()->Option<(u32,PathBuf)> {
    use windows::{core::w,Win32::{Foundation::CloseHandle,System::Threading::{OpenProcess,QueryFullProcessImageNameW,PROCESS_QUERY_LIMITED_INFORMATION,PROCESS_NAME_WIN32},UI::WindowsAndMessaging::{FindWindowW,GetWindowThreadProcessId}}};
    unsafe {
        let window=FindWindowW(w!("kugou_ui"),None).ok()?;
        let mut pid=0;GetWindowThreadProcessId(window,Some(&mut pid));
        let process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid).ok()?;
        let mut name=[0u16;32768];let mut length=name.len() as u32;
        let result=QueryFullProcessImageNameW(process,PROCESS_NAME_WIN32,windows::core::PWSTR(name.as_mut_ptr()),&mut length);
        let _=CloseHandle(process);result.ok()?;
        let path=PathBuf::from(String::from_utf16_lossy(&name[..length as usize]));
        path.file_name()?.to_str()?.eq_ignore_ascii_case("KuGou.exe").then_some((pid,path))
    }
}

fn endpoint(value:&serde_json::Value,pid:u32)->Option<(String,String)> {
    if value["pid"].as_str()?.parse::<u32>().ok()?!=pid{return None;}
    let port=value["port"].as_u64().filter(|port|(1..=65535).contains(port))?;
    if value["addr"].as_str()?!=format!("127.0.0.1:{port}"){return None;}
    let token=value["token"].as_str().filter(|token|!token.is_empty())?;
    Some((format!("http://127.0.0.1:{port}"),token.to_owned()))
}

fn read_endpoint(path:&Path,pid:u32)->Option<(String,String)> {
    let value=serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    endpoint(&value,pid)
}

fn send_seek(base:&str,token:&str,seconds:f64)->Option<()> {
    let agent=ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(1)).build();
    let body=serde_json::json!({"action":"seek","position_ms":(seconds*1000.0).round() as u64});
    let response=agent.post(&format!("{base}/v1/player/seek"))
        .set("X-Kugou-Token",token).set("Content-Type","application/json")
        .send_string(&body.to_string()).ok()?.into_string().ok()?;
    let result:serde_json::Value=serde_json::from_str(&response).ok()?;
    (result["code"].as_i64()==Some(0) && result["data"]["accepted"].as_bool()==Some(true)).then_some(())
}

pub fn seek(seconds:f64)->bool {
    if !seconds.is_finite() || !(0.0..86400.0).contains(&seconds){return false;}
    // 串行化首次握手和跳转，避免连续拖动产生多个本地服务会话。
    static LOCK:std::sync::Mutex<()>=std::sync::Mutex::new(());
    let Ok(_guard)=LOCK.lock() else{return false;};
    let Some((pid,executable))=player() else{return false;};
    let path=crate::model::config_dir().join("kugou-control.json");
    if let Some((base,token))=read_endpoint(&path,pid) {
        if send_seek(&base,&token,seconds).is_some(){return true;}
    }
    if std::fs::create_dir_all(path.parent().unwrap()).is_err(){return false;}
    let payload=serde_json::json!({"out":path}).to_string();
    let uri=format!("kugou://workbuddy?p={}",base64::engine::general_purpose::STANDARD.encode(payload));
    if crate::child_process::spawn(std::process::Command::new(executable).arg(uri)).is_err(){return false;}
    let started=std::time::Instant::now();
    while started.elapsed()<Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(100));
        if let Some((base,token))=read_endpoint(&path,pid) {
            if send_seek(&base,&token,seconds).is_some(){return true;}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_requires_current_process_and_loopback(){
        let mut value=serde_json::json!({"pid":"12","addr":"127.0.0.1:1234","port":1234,"token":"test"});
        assert!(endpoint(&value,12).is_some());assert!(endpoint(&value,13).is_none());
        value["addr"]="example.com:1234".into();assert!(endpoint(&value,12).is_none());
        for seconds in [f64::NAN,f64::INFINITY,-1.0,86400.0]{assert!(!seek(seconds));}
    }
    #[test]
    fn seek_sends_milliseconds_and_checks_acceptance(){
        use std::{io::{Read,Write},net::TcpListener};
        for accepted in [true,false] {
            let listener=TcpListener::bind("127.0.0.1:0").unwrap();let base=format!("http://{}",listener.local_addr().unwrap());
            let worker=std::thread::spawn(move||{
                let (mut stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut bytes=Vec::new();let mut buffer=[0;1024];
                while !String::from_utf8_lossy(&bytes).contains("\"position_ms\":12500}") {
                    let count=stream.read(&mut buffer).unwrap();assert!(count>0);bytes.extend_from_slice(&buffer[..count]);
                }
                let request=String::from_utf8_lossy(&bytes);assert!(request.starts_with("POST /v1/player/seek "));assert!(request.contains("X-Kugou-Token: test"));
                let body=serde_json::json!({"code":0,"data":{"accepted":accepted}}).to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            });
            assert_eq!(send_seek(&base,"test",12.5).is_some(),accepted);worker.join().unwrap();
        }
    }
}
