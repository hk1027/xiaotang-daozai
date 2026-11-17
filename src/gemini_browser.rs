//! 通过 Chrome 本人确认的原生连接读取 Gemini 会话，不启动或关闭浏览器。
use super::{cookie,email,Session};
use serde_json::{json,Value};
use std::{fs,io::ErrorKind,net::{Ipv4Addr,SocketAddr,TcpStream},sync::atomic::{AtomicBool,Ordering},time::{Duration,Instant}};
use tungstenite::{client::client_with_config,handshake::HandshakeError,protocol::WebSocketConfig,Error,Message,WebSocket};

const ERROR:&str="Chrome 授权未完成或连接已断开，请重新连接";
const CANCEL:&str="已取消 Gemini 连接";
const MAX:usize=1024*1024;
const EMAIL_SCRIPT:&str=r#"(()=>{if(location.origin!=='https://gemini.google.com'||!/^\/(?:u\/\d+\/)?usage\/?$/.test(location.pathname))return '';const a=document.querySelector('a[href^="https://accounts.google.com/SignOutOptions"][aria-label]')||document.querySelector('a[aria-label*="@"]');return(a?.getAttribute('aria-label')||'').match(/[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}/i)?.[0]||''})()"#;

struct Endpoint {port:u16,path:String}
impl Endpoint {
    fn parse(raw:&str)->Option<Self>{
        let mut lines=raw.trim().lines();let port=lines.next()?.parse::<u16>().ok()?;let path=lines.next()?;
        let id=path.strip_prefix("/devtools/browser/")?;
        (port>1024&&lines.next().is_none()&&id.len()==36&&id.bytes().enumerate().all(|(i,b)|if matches!(i,8|13|18|23){b==b'-'}else{b.is_ascii_hexdigit()})).then(||Self{port,path:path.into()})
    }
    fn url(&self)->String{format!("ws://127.0.0.1:{}{}",self.port,self.path)}
}
fn check(cancel:&AtomicBool,deadline:Instant,timeout:&str)->Result<(),String>{
    if cancel.load(Ordering::Relaxed){return Err(CANCEL.into());}
    if Instant::now()>=deadline{return Err(timeout.into());}Ok(())
}
struct Socket {socket:WebSocket<TcpStream>,next:u32}
impl Socket {
    fn open(endpoint:&Endpoint,cancel:&AtomicBool,timeout:Duration)->Result<Self,String>{
        let deadline=Instant::now()+timeout;check(cancel,deadline,"等待 Chrome 授权超时，请重新连接")?;
        let stream=TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST,endpoint.port)),Duration::from_secs(2)).map_err(|_|ERROR)?;
        stream.set_nonblocking(true).map_err(|_|ERROR)?;
        let config=WebSocketConfig::default().max_message_size(Some(MAX)).max_frame_size(Some(MAX));
        let mut handshake=client_with_config(endpoint.url(),stream,Some(config));
        loop{match handshake{
            Ok((socket,_))=>return Ok(Self{socket,next:0}),
            Err(HandshakeError::Failure(_))=>return Err(ERROR.into()),
            Err(HandshakeError::Interrupted(pending))=>{
                check(cancel,deadline,"等待 Chrome 授权超时，请重新连接")?;
                std::thread::sleep(Duration::from_millis(20));handshake=pending.handshake();
            }
        }}
    }
    fn call(&mut self,method:&str,params:Value,session:Option<&str>,cancel:&AtomicBool)->Result<Value,String>{
        let deadline=Instant::now()+Duration::from_secs(30);check(cancel,deadline,ERROR)?;
        self.next+=1;let id=self.next;
        let mut message=json!({"id":id,"method":method,"params":params});if let Some(session)=session{message["sessionId"]=json!(session);}
        match self.socket.send(Message::text(message.to_string())){
            Ok(())=>{},Err(Error::Io(e))if e.kind()==ErrorKind::WouldBlock=>{},_=>return Err(ERROR.into())
        }
        loop{
            check(cancel,deadline,ERROR)?;
            match self.socket.flush(){Ok(())=>break,Err(Error::Io(e))if e.kind()==ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(20)),_=>return Err(ERROR.into())}
        }
        loop{
            check(cancel,deadline,ERROR)?;
            match self.socket.read(){
                Ok(Message::Text(raw))=>{
                    let value:Value=serde_json::from_str(&raw).map_err(|_|ERROR)?;
                    if value["id"].as_u64()!=Some(id as u64)||value["sessionId"].as_str()!=session{continue;}
                    if value.get("error").is_some(){return Err(ERROR.into());}
                    return value.get("result").cloned().ok_or_else(||ERROR.into());
                }
                Ok(Message::Ping(_)|Message::Pong(_))=>{},
                Err(Error::Io(e))if e.kind()==ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(20)),
                _=>return Err(ERROR.into())
            }
        }
    }
    fn identity(&mut self,session:&str,cancel:&AtomicBool)->Result<Option<String>,String>{
        let result=self.call("Runtime.evaluate",json!({"expression":EMAIL_SCRIPT,"returnByValue":true}),Some(session),cancel)?;
        Ok(result.pointer("/result/value").and_then(Value::as_str).and_then(|raw|email(raw).ok()))
    }
}
impl Drop for Socket {fn drop(&mut self){let _=self.socket.close(None);}}
fn gemini_page(raw:&str)->bool{
    url::Url::parse(raw).is_ok_and(|u|{
        let path=u.path().strip_suffix('/').unwrap_or(u.path());
        u.scheme()=="https"&&u.host_str()==Some("gemini.google.com")&&u.port_or_known_default()==Some(443)&&u.username().is_empty()&&u.password().is_none()
            &&(path=="/usage"||path.strip_prefix("/u/").and_then(|p|p.strip_suffix("/usage")).is_some_and(|n|!n.is_empty()&&n.bytes().all(|b|b.is_ascii_digit())))
    })
}
fn cookie_header(value:&Value)->Result<String,String>{
    let cookies=value["cookies"].as_array().ok_or(ERROR)?;
    let header=cookies.iter().filter(|c|matches!(c["domain"].as_str(),Some(".google.com"|"google.com"|".gemini.google.com"|"gemini.google.com")))
        .filter_map(|c|Some(format!("{}={}",c["name"].as_str()?,c["value"].as_str()?))).collect::<Vec<_>>().join("; ");
    cookie(&header)
}
pub(super) fn login(address:&str,cancel:&AtomicBool)->Result<Session,String>{
    let target=if address.trim().is_empty(){None}else{Some(email(address)?)};
    let path=std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).ok_or(ERROR)?.join("Google/Chrome/User Data/DevToolsActivePort");
    let endpoint=fs::read_to_string(path).ok().and_then(|raw|Endpoint::parse(&raw)).ok_or("请先打开 Chrome，并在 chrome://inspect/#remote-debugging 开启原生连接")?;
    login_at(&endpoint,target,cancel)
}
fn login_at(endpoint:&Endpoint,target:Option<String>,cancel:&AtomicBool)->Result<Session,String>{
    let mut socket=Socket::open(endpoint,cancel,Duration::from_secs(300))?;
    let result=socket.call("Target.getTargets",json!({}),None,cancel)?;
    let pages=result["targetInfos"].as_array().ok_or(ERROR)?.iter().filter(|t|t["type"]=="page"&&gemini_page(t["url"].as_str().unwrap_or_default())).collect::<Vec<_>>();
    if pages.is_empty(){return Err("请先在 Chrome 打开 gemini.google.com/usage 并登录".into());}
    if pages.len()>1&&target.is_none(){return Err("有多个 Gemini 用量页，请填写要连接的 Google 邮箱".into());}
    for page in pages{
        let result=socket.call("Target.attachToTarget",json!({"targetId":page["targetId"],"flatten":true}),None,cancel)?;
        let session=result["sessionId"].as_str().ok_or(ERROR)?;
        let current=socket.identity(session,cancel)?;
        let Some(current)=current.filter(|email|target.as_ref().is_none_or(|wanted|wanted==email)) else{
            if target.is_none(){return Err("请先在 Chrome 的 Gemini 用量页完成 Google 登录".into());}
            socket.call("Target.detachFromTarget",json!({"sessionId":session}),None,cancel)?;
            continue;
        };
        let result=socket.call("Network.getCookies",json!({"urls":["https://gemini.google.com/"]}),Some(session),cancel)?;
        let header=cookie_header(&result)?;
        if socket.identity(session,cancel)?.as_ref()!=Some(&current){return Err("Google 账号在连接期间发生切换，请重新连接".into());}
        return Ok(Session{email:current,cookie:header});
    }
    Err("未找到此 Google 账号的 Gemini 用量页，请在 Chrome 切回原账号".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{Read,Write},net::TcpListener,sync::Arc};
    fn endpoint(port:u16)->Endpoint{Endpoint{port,path:"/devtools/browser/01234567-89ab-cdef-0123-456789abcdef".into()}}
    #[test]
    fn native_chrome_endpoints_and_pages_are_strictly_scoped(){
        assert!(Endpoint::parse("50000\n/devtools/browser/01234567-89ab-cdef-0123-456789abcdef\n").is_some());
        for raw in ["80\n/devtools/browser/01234567-89ab-cdef-0123-456789abcdef","50000\nhttps://evil.invalid","50000\n/devtools/browser/01234567-89ab-cdef-0123-456789abcdef?x=y","50000\n/devtools/browser/01234567-89ab-cdef-0123-456789abcdef\nInjected"]{assert!(Endpoint::parse(raw).is_none());}
        for url in ["https://gemini.google.com/usage","https://gemini.google.com/u/1/usage/"]{assert!(gemini_page(url));}
        for url in ["https://gemini.google.com.evil.invalid/usage","http://gemini.google.com/usage","https://user@gemini.google.com/usage","https://gemini.google.com:444/usage","https://gemini.google.com/app","https://gemini.google.com/u/abc/usage"]{assert!(!gemini_page(url));}
    }
    #[test]
    fn native_chrome_cookies_exclude_other_domains(){
        let value=json!({"cookies":[{"domain":".google.com","name":"SID","value":"synthetic"},{"domain":"evilgoogle.com","name":"private","value":"unrelated"}]});
        assert_eq!(cookie_header(&value).unwrap(),"SID=synthetic");assert!(cookie_header(&json!({"cookies":[]})).is_err());
    }
    #[test]
    fn native_chrome_cancel_closes_a_pending_permission_request(){
        let server=TcpListener::bind((Ipv4Addr::LOCALHOST,0)).unwrap();let ep=endpoint(server.local_addr().unwrap().port());
        let cancel=Arc::new(AtomicBool::new(false));let stopped=cancel.clone();
        let task=std::thread::spawn(move||{let(mut stream,_)=server.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();let mut buffer=[0u8;4096];assert!(stream.read(&mut buffer).unwrap()>0);stopped.store(true,Ordering::Relaxed);assert_eq!(stream.read(&mut buffer).unwrap(),0);});
        let start=Instant::now();assert_eq!(Socket::open(&ep,&cancel,Duration::from_secs(10)).err().unwrap(),CANCEL);assert!(start.elapsed()<Duration::from_secs(1));task.join().unwrap();
    }
    #[test]
    fn native_chrome_cdp_routes_large_replies_and_only_disconnects(){
        let server=TcpListener::bind((Ipv4Addr::LOCALHOST,0)).unwrap();let ep=endpoint(server.local_addr().unwrap().port());
        let task=std::thread::spawn(move||{
            let(stream,_)=server.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();let mut ws=tungstenite::accept(stream).unwrap();
            let request:Value=serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();assert_eq!(request["method"],"Runtime.evaluate");assert_eq!(request["sessionId"],"page-1");
            ws.send(Message::text(json!({"method":"Runtime.consoleAPICalled"}).to_string())).unwrap();
            ws.send(Message::text(json!({"id":request["id"],"sessionId":"page-1","result":{"text":"x".repeat(20000)}}).to_string())).unwrap();
            assert!(matches!(ws.read().unwrap(),Message::Close(_)),"只能断开连接，不能关闭用户浏览器");
        });
        let cancel=AtomicBool::new(false);let mut socket=Socket::open(&ep,&cancel,Duration::from_secs(2)).unwrap();
        let result=socket.call("Runtime.evaluate",json!({}),Some("page-1"),&cancel).unwrap();assert_eq!(result["text"].as_str().unwrap().len(),20000);drop(socket);task.join().unwrap();
    }
    #[test]
    fn native_chrome_skips_usage_tab_without_an_identity(){
        let server=TcpListener::bind((Ipv4Addr::LOCALHOST,0)).unwrap();let ep=endpoint(server.local_addr().unwrap().port());
        let task=std::thread::spawn(move||{
            let (stream,_)=server.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut ws=tungstenite::accept(stream).unwrap();let mut detached=false;
            while let Message::Text(raw)=ws.read().unwrap(){
                let req:Value=serde_json::from_str(&raw).unwrap();
                let result=match req["method"].as_str().unwrap(){
                    "Target.getTargets"=>json!({"targetInfos":(["empty","ready"].map(|id|json!({"type":"page","targetId":id,"url":"https://gemini.google.com/usage"})))}),
                    "Target.attachToTarget"=>json!({"sessionId":req["params"]["targetId"]}),
                    "Target.detachFromTarget"=>{assert_eq!(req["params"]["sessionId"],"empty");detached=true;json!({})},
                    "Runtime.evaluate"=>json!({"result":{"value":if req["sessionId"]=="ready"{"me@example.com"}else{""}}}),
                    "Network.getCookies"=>{assert!(detached);assert_eq!(req["sessionId"],"ready");json!({"cookies":[{"domain":".google.com","name":"SID","value":"fixture"}]})},
                    _=>panic!("意外的浏览器命令"),
                };
                let mut reply=json!({"id":req["id"],"result":result});if let Some(s)=req.get("sessionId"){reply["sessionId"]=s.clone();}
                ws.send(Message::text(reply.to_string())).unwrap();
            }
        });
        let result=login_at(&ep,Some("me@example.com".into()),&AtomicBool::new(false));task.join().unwrap();
        assert_eq!(result.unwrap().email,"me@example.com");
    }
    #[test]
    fn native_chrome_handshake_does_not_follow_redirects(){
        let server=TcpListener::bind((Ipv4Addr::LOCALHOST,0)).unwrap();let ep=endpoint(server.local_addr().unwrap().port());
        let task=std::thread::spawn(move||{let(mut stream,_)=server.accept().unwrap();let mut buffer=[0u8;4096];let _=stream.read(&mut buffer).unwrap();stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: https://evil.invalid/\r\nContent-Length: 0\r\n\r\n").unwrap();});
        assert!(Socket::open(&ep,&AtomicBool::new(false),Duration::from_secs(2)).is_err());task.join().unwrap();
    }
}
