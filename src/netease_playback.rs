//! 网易云原生播放事件：只读增量日志，不依赖 SMTC 或特定 DLL 版本。
use super::{MediaKind,NowPlaying};
use std::{fs::File,io::{Read,Seek,SeekFrom},path::Path,sync::{mpsc,Arc},time::{Duration,Instant}};

const MAX_READ:u64=4*1024*1024;
const MAX_LINE:usize=256*1024;

fn monotonic_millis()->u64 {
    static CLOCK:std::sync::OnceLock<(Instant,u64)>=std::sync::OnceLock::new();
    let (start,epoch)=CLOCK.get_or_init(||(Instant::now(),std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64));
    epoch.saturating_add(start.elapsed().as_millis() as u64)
}

// 初次读取历史记录时，使用可读的本地日期恢复进度；实时推进只依赖 Instant。
fn historical_millis(line:&str,now:u64)->Option<u64> {
    use chrono::{Datelike,TimeZone};
    let date=line.strip_prefix('[')?.split(':').nth(2)?;
    let current=chrono::Local.timestamp_millis_opt(i64::try_from(now).ok()?).single()?;
    for year in [current.year(),current.year()-1] {
        if year<current.year() && !(current.month()==1 && date.starts_with("12")){break;}
        let Some(time)=chrono::NaiveDateTime::parse_from_str(&format!("{year}/{date}"),"%Y/%m%d/%H%M%S").ok()
            .and_then(|date|chrono::Local.from_local_datetime(&date).single()) else{continue;};
        let Ok(millis)=u64::try_from(time.timestamp_millis()) else{continue;};
        if millis<=now.saturating_add(1000){return Some(millis.min(now));}
    }
    None
}

// elog 的逐字节替换格式，参考 https://github.com/344272586-sketch/dsh-pet-qqmusic/blob/main/lib/netease-elog.js
fn decode(byte:u8)->u8 {
    let high=byte>>4;let low=byte&15;
    ((high^((low+8)&15))<<4)|((byte>>6)<<2)|(!high&3)
}

fn song_id(value:&serde_json::Value)->Option<u64>{value.as_u64().or_else(||value.as_str()?.parse().ok()).filter(|id|*id>0)}

fn cover_url(value:&str)->Option<String>{
    if value.len()>4096{return None;}
    let mut url=url::Url::parse(value).ok()?;
    let host=url.host_str()?;
    if !matches!(url.scheme(),"http"|"https") || !(host=="music.126.net" || host.ends_with(".music.126.net")){return None;}
    url.set_scheme("https").ok()?;url.set_query(Some("param=256y256"));url.set_fragment(None);
    Some(url.into())
}

fn track(value:&serde_json::Value)->Option<(NowPlaying,String)>{
    let t=value.get("track").unwrap_or(value);
    let id=song_id(t.get("id")?)?;
    let title=t.get("name")?.as_str()?.trim().to_owned();
    let artist=t.get("artists").and_then(|a|a.as_array()).map(|a|a.iter().filter_map(|a|a.get("name")?.as_str()).collect::<Vec<_>>().join("/")).unwrap_or_default();
    let album=t.pointer("/album/name").and_then(|v|v.as_str()).unwrap_or_default();
    if title.is_empty(){return None;}
    if [title.len(),artist.len(),album.len()].into_iter().any(|size|size>4096){crate::resources::REJECTED_DATA.fetch_add(1,std::sync::atomic::Ordering::Relaxed);return None;}
    let duration=t.get("duration").and_then(|v|v.as_f64()).unwrap_or_default()/1000.0;
    if !duration.is_finite() || !(0.0..86400.0).contains(&duration){return None;}
    let cover=t.pointer("/album/picUrl").and_then(|v|v.as_str()).and_then(cover_url).unwrap_or_default();
    Some((NowPlaying{title,artist,album:album.into(),
        source:"cloudmusic.exe".into(),song_id:Some(id),duration,playback_type:MediaKind::Music,..Default::default()},cover))
}

#[derive(Default)]
struct Playback {media:NowPlaying,artwork_url:String,at:u64,observed_at:u64,ready:bool}
impl Playback {
    fn position(&self,now:u64)->f64{
        let position=self.media.position+if self.media.playing {now.saturating_sub(self.observed_at)as f64/1000.0}else{0.0};
        if self.media.duration>0.0{position.min(self.media.duration)}else{position}
    }
    fn metadata(&mut self,(metadata,cover):(NowPlaying,String)){
        self.media.title=metadata.title;
        if !metadata.artist.is_empty(){self.media.artist=metadata.artist;}
        if !metadata.album.is_empty(){self.media.album=metadata.album;}
        if metadata.duration>0.0{self.media.duration=metadata.duration;}
        self.media.source=metadata.source;self.media.playback_type=metadata.playback_type;
        if !cover.is_empty(){self.artwork_url=cover;}
    }
    fn line(&mut self,line:&str,pid:u32,now:u64){
        let Some((header,body))=line.strip_prefix('[').and_then(|s|s.split_once(']'))else{return;};
        let mut parts=header.split(':');
        if parts.next().and_then(|s|s.parse::<u32>().ok())!=Some(pid){return;}
        // 日志计数只比较事件先后，不能与本机单调时钟相减。
        let Some(at)=parts.nth(2).and_then(|s|s.parse::<u64>().ok()).filter(|at|*at>=self.at)else{return;};
        if body.contains("\"exitApp\""){*self=Self::default();return;}
        // 只解析播放事件；音频签名链接、Cookie 等其他日志内容不会进入状态或缓存。
        let Some((_,body))=body.split_once("【playing】,")else{return;};
        let Some((event,args))=body.trim().strip_prefix('"').and_then(|s|s.split_once("\","))else{return;};
        if matches!(event,"playOneTrackInPlayingList"|"checkPlayPrivilege"){
            let Some(metadata)=serde_json::from_str(args).ok().as_ref().and_then(track)else{return;};
            if self.media.song_id!=metadata.0.song_id {
                if event=="checkPlayPrivilege" && self.media.song_id.is_some(){return;}
                *self=Self{media:metadata.0,artwork_url:metadata.1,at,observed_at:now,ready:false};
            }else{self.metadata(metadata);}
        }else if event.starts_with("native") && event.contains("load完成"){
            let Some(id)=serde_json::from_str::<serde_json::Value>(args).ok().and_then(|v|song_id(v.get("songId")?))else{return;};
            if self.media.song_id!=Some(id){*self=Self::default();self.media.song_id=Some(id);}
            self.media.position=0.0;self.media.playing=true;self.at=at;self.observed_at=now;self.ready=true;
        }else if event=="setPlayingPosition" && self.media.song_id.is_some(){
            let Some(position)=args.split(',').next().and_then(|s|s.trim().parse::<f64>().ok()).filter(|p|p.is_finite() && (0.0..86400.0).contains(p))else{return;};
            self.media.position=position;self.at=at;self.observed_at=now;self.ready=true;
        }else if event.starts_with("native") && event.ends_with("state") && self.media.song_id.is_some(){
            let Some(status)=args.split(',').next().and_then(|s|s.trim().parse::<u8>().ok()).filter(|s|matches!(s,1|2))else{return;};
            self.media.position=self.position(now);self.media.playing=status==1;self.at=at;self.observed_at=now;self.ready=true;
        }
    }
    fn snapshot(&self,now:u64)->Option<NowPlaying>{
        if !self.ready || !self.media.is_music(){return None;}
        let mut media=self.media.clone();media.position=self.position(now);media.sampled_at=Some(std::time::Instant::now());
        media.event_time_ms=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()
            .map(|utc|(utc.as_millis() as u64).saturating_sub(now.saturating_sub(self.observed_at)));Some(media)
    }
}

#[derive(Default)]
pub(super) struct CoverCache {
    url:String,image:Option<Arc<eframe::egui::ColorImage>>,
    pending:Option<mpsc::Receiver<Option<Arc<eframe::egui::ColorImage>>>>,
    retry_at:Option<Instant>,
    worker:Option<std::thread::JoinHandle<()>>,
}
impl CoverCache {
    fn get(&mut self,url:&str)->Option<Arc<eframe::egui::ColorImage>>{
        self.get_with(url,fetch_cover)
    }
    pub(super) fn get_with(&mut self,url:&str,load:impl FnOnce(&str)->Option<Arc<eframe::egui::ColorImage>>+Send+'static)->Option<Arc<eframe::egui::ColorImage>>{
        // 切歌时丢弃旧请求的接收端，迟到的封面不能覆盖新歌。
        if self.url!=url{*self=Self{url:url.into(),worker:self.worker.take(),..Default::default()};}
        if self.url.is_empty(){return None;}
        if let Some(receiver)=&self.pending{
            match receiver.try_recv(){
                Ok(image)=>{self.image=image;self.pending=None;}
                Err(mpsc::TryRecvError::Disconnected)=>{self.pending=None;}
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
        if self.image.is_none() && self.pending.is_none() && self.worker.as_ref().is_none_or(|worker|worker.is_finished()) && self.retry_at.is_none_or(|at|Instant::now()>=at){
            let (sender,receiver)=mpsc::channel();let url=self.url.clone();
            self.pending=Some(receiver);self.retry_at=Some(Instant::now()+Duration::from_secs(3));
            // 网络与解码不占用媒体轮询线程，歌词时间轴继续正常推进。
            self.worker=crate::resources::COVER.spawn(move||{let _=sender.send(load(&url));});
            if self.worker.is_none(){self.pending=None;}
        }
        self.image.clone()
    }
}

/// 本地日志和队列都没有歌名时（漫游、私人 FM 续载、每日推荐等网易云不落盘的歌），按歌曲 ID 在线查一次。
/// 只保留当前这首；请求在后台进行，不占媒体轮询线程；失败按 10 秒、60 秒再试，第三次失败后这首歌不再请求。
#[derive(Default)]
pub(super) struct DetailCache {
    id:Option<u64>,value:Option<(NowPlaying,String)>,failures:usize,retry_at:Option<Instant>,
    pending:Option<mpsc::Receiver<Option<(NowPlaying,String)>>>,
    worker:Option<std::thread::JoinHandle<()>>,
}
const DETAIL_RETRY:[Duration;2]=[Duration::from_secs(10),Duration::from_secs(60)];
impl DetailCache {
    pub(super) fn get_with(&mut self,id:u64,load:impl FnOnce(u64)->Option<(NowPlaying,String)>+Send+'static)->Option<(NowPlaying,String)>{
        // 换歌时丢弃旧请求的接收端，迟到的结果不能套到新歌上。
        if self.id!=Some(id){*self=Self{id:Some(id),worker:self.worker.take(),..Default::default()};}
        if let Some(receiver)=&self.pending{
            match receiver.try_recv(){
                Ok(value)=>{
                    self.pending=None;
                    match value.filter(|(media,_)|media.song_id==Some(id)) {
                        Some(value)=>self.value=Some(value),
                        None=>{self.failures+=1;self.retry_at=DETAIL_RETRY.get(self.failures-1).map(|wait|Instant::now()+*wait);}
                    }
                }
                Err(mpsc::TryRecvError::Disconnected)=>{self.pending=None;}
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
        let due=self.failures==0 || self.retry_at.is_some_and(|at|Instant::now()>=at);
        if self.value.is_none() && self.pending.is_none() && due && self.worker.as_ref().is_none_or(|worker|worker.is_finished()){
            let (sender,receiver)=mpsc::channel();
            self.worker=crate::resources::COVER.spawn(move||{let _=sender.send(load(id));});
            if self.worker.is_some(){self.pending=Some(receiver);self.retry_at=None;}
        }
        self.value.clone()
    }
}

fn fetch_detail(id:u64)->Option<(NowPlaying,String)>{
    let url=format!("https://music.163.com/api/song/detail/?ids=%5B{id}%5D");
    let response=ureq::AgentBuilder::new().timeout(Duration::from_secs(5)).redirects(0).build().get(&url)
        .set("User-Agent",concat!("Daozai/",env!("CARGO_PKG_VERSION")," (Windows)"))
        .set("Referer","https://music.163.com").set("Accept","application/json").call().ok()?;
    let limit=256*1024;let mut bytes=Vec::new();
    response.into_reader().take(limit+1).read_to_end(&mut bytes).ok()?;
    if bytes.len()>limit as usize{crate::resources::REJECTED_DATA.fetch_add(1,std::sync::atomic::Ordering::Relaxed);return None;}
    detail_from(&serde_json::from_slice(&bytes).ok()?,id)
}

/// `{"code":200,"songs":[{…}]}`：歌曲字段与本地队列一致，只接受 ID 完全相同的那一首。
fn detail_from(value:&serde_json::Value,id:u64)->Option<(NowPlaying,String)>{
    if value.get("code").and_then(|v|v.as_i64())!=Some(200){return None;}
    value.get("songs")?.as_array()?.iter().filter_map(track).find(|(media,_)|media.song_id==Some(id))
}

#[test]
fn detail_response_fills_metadata_only_for_the_same_song(){
    let body=serde_json::json!({"code":200,"songs":[{"id":1316574753u64,"name":"等你等到我心痛","artists":[{"name":"张学友"}],
        "album":{"name":"张学友4合1珍藏集","picUrl":"https://p2.music.126.net/a8p2P1JofrH7C23LLtF_Yw==/109951169235647752.jpg"},"duration":253533}]});
    let (media,cover)=detail_from(&body,1316574753).unwrap();
    assert_eq!((media.title.as_str(),media.artist.as_str(),media.album.as_str()),("等你等到我心痛","张学友","张学友4合1珍藏集"));
    assert!((media.duration-253.533).abs()<1e-6);assert!(cover.ends_with("?param=256y256"),"封面按缩略图尺寸请求");
    assert!(detail_from(&body,1).is_none(),"返回的不是这首歌时不能套用");
    assert!(detail_from(&serde_json::json!({"code":-460,"songs":[]}),1316574753).is_none());
}

#[test]
fn detail_lookup_runs_in_background_retries_then_stops(){
    use std::sync::{Arc as A,atomic::{AtomicUsize,Ordering}};
    let calls=A::new(AtomicUsize::new(0));
    let mut cache=DetailCache::default();
    let wait=|cache:&mut DetailCache|{if let Some(worker)=cache.worker.take(){worker.join().unwrap();}};
    for round in 0..3 {
        let c=calls.clone();
        assert!(cache.get_with(7,move|_|{c.fetch_add(1,Ordering::SeqCst);None}).is_none());
        wait(&mut cache);
        let c=calls.clone();cache.get_with(7,move|_|{c.fetch_add(1,Ordering::SeqCst);None});
        assert_eq!(calls.load(Ordering::SeqCst),round+1,"未到重试时间不重复请求");
        if round<2 {cache.retry_at=Some(Instant::now());}
        if round==0 {
            let busy=[crate::resources::COVER.acquire().unwrap(),crate::resources::COVER.acquire().unwrap()];
            cache.get_with(7,|_|panic!("已有两个后台请求"));
            drop(busy);
        }
    }
    wait(&mut cache);cache.get_with(7,|_|panic!("三次失败后这首歌不再请求"));
    let found=(NowPlaying{title:"新歌".into(),song_id:Some(8),playback_type:MediaKind::Music,..Default::default()},String::new());
    let value=found.clone();cache.get_with(8,move|_|Some(value));wait(&mut cache);
    assert_eq!(cache.get_with(8,|_|panic!("已有结果不再请求")).unwrap().0.title,"新歌","换歌后重新查询");
    let wrong=(NowPlaying{title:"别的歌".into(),song_id:Some(10),..Default::default()},String::new());
    cache.get_with(9,move|_|Some(wrong));wait(&mut cache);
    assert!(cache.get_with(9,|_|None).is_none(),"返回其他歌曲 ID 时不套用");
}

#[test]
fn resource_cache_recreation_cannot_bypass_download_limit(){
    use std::sync::{Condvar,Mutex,atomic::{AtomicUsize,Ordering}};
    let gate=Arc::new((Mutex::new(false),Condvar::new()));let started=Arc::new(AtomicUsize::new(0));
    for _ in 0..30 {
        let mut cache=CoverCache::default();let gate=gate.clone();let started=started.clone();
        cache.get_with("cover",move |_|{
            started.fetch_add(1,Ordering::SeqCst);
            let _released=gate.1.wait_while(gate.0.lock().unwrap(),|open|!*open).unwrap();None
        });
        std::thread::sleep(Duration::from_millis(5));
    }
    let count=started.load(Ordering::SeqCst);
    *gate.0.lock().unwrap()=true;gate.1.notify_all();
    assert!(count<=2,"重建缓存绕过了并发上限，产生 {count} 个下载线程");
}

#[test]
fn stalled_cover_requests_do_not_accumulate_threads_when_tracks_change(){
    use std::sync::{Condvar,Mutex,atomic::{AtomicUsize,Ordering}};
    let gate=Arc::new((Mutex::new(false),Condvar::new()));
    let started=Arc::new(AtomicUsize::new(0));let active=Arc::new(AtomicUsize::new(0));
    let mut cache=CoverCache::default();
    for song in 0..40 {
        let gate=gate.clone();let started=started.clone();let active=active.clone();
        cache.get_with(&format!("cover-{song}"),move |_|{
            active.fetch_add(1,Ordering::SeqCst);started.fetch_add(1,Ordering::SeqCst);
            let (lock,ready)=&*gate;let _guard=ready.wait_while(lock.lock().unwrap(),|released|!*released).unwrap();
            active.fetch_sub(1,Ordering::SeqCst);None
        });
        std::thread::sleep(Duration::from_millis(5));
    }
    let count=started.load(Ordering::SeqCst);
    *gate.0.lock().unwrap()=true;gate.1.notify_all();
    let until=Instant::now()+Duration::from_secs(2);
    while active.load(Ordering::SeqCst)>0 && Instant::now()<until {std::thread::sleep(Duration::from_millis(5));}
    assert_eq!(count,1,"旧封面请求阻塞时，切歌不能继续创建新下载线程");
    assert_eq!(active.load(Ordering::SeqCst),0,"解除阻塞后旧下载线程应结束");
    cache.worker.take().unwrap().join().unwrap();
    let image=Arc::new(eframe::egui::ColorImage::new([2,2],eframe::egui::Color32::BLUE));
    let current=image.clone();
    cache.get_with("cover-39",move |url|{assert_eq!(url,"cover-39");Some(current)});
    cache.worker.take().unwrap().join().unwrap();
    assert!(Arc::ptr_eq(&cache.get_with("cover-39",|_|panic!("已有封面不应再次下载")).unwrap(),&image),"旧请求结束后应继续加载当前歌曲");
}

#[test]
fn cover_failure_can_retry_without_a_minute_of_blank_artwork(){
    let mut cache=CoverCache::default();
    let before=Instant::now();
    cache.get("invalid-test-url");
    assert!(cache.retry_at.unwrap().duration_since(before)<Duration::from_secs(5),"封面暂时失败后不能空白一分钟");
}

pub(super) fn fetch_cover(url:&str)->Option<Arc<eframe::egui::ColorImage>>{
    let url=cover_url(url)?;
    let response=ureq::AgentBuilder::new().timeout(Duration::from_secs(5)).redirects(0).build().get(&url).call().ok()?;
    let limit=2*1024*1024;let mut bytes=Vec::new();
    response.into_reader().take(limit+1).read_to_end(&mut bytes).ok()?;
    if bytes.len()>limit as usize{return None;}
    super::decode_artwork(&bytes).map(Arc::new)
}

#[derive(Default)]
pub(super) struct Reader {
    pid:u32,offset:u64,prefix:Vec<u8>,carry:Vec<u8>,skip_line:bool,
    playback:Playback,lookup:(Option<u64>,u64),cover:CoverCache,
    queue_print:Vec<Option<(std::time::SystemTime,u64)>>,
    /// 本地队列确认查不到歌名的歌曲 ID：只对它们在线补全。
    queue_missed:Option<u64>,detail:DetailCache,
}
impl Reader {
    pub(super) fn pending(&self)->bool {
        self.pid!=0 && self.playback.media.song_id.is_some() && (!self.playback.ready || !self.playback.media.is_music())
    }
    fn ingest(&mut self,bytes:&[u8],now:u64,replay:bool){
        for byte in bytes.iter().copied().map(decode){
            if byte==b'\n'{
                if !self.skip_line{
                    if let Ok(line)=std::str::from_utf8(&self.carry){
                        let observed=if replay{historical_millis(line,now).unwrap_or(now)}else{now};
                        self.playback.line(line,self.pid,observed);
                    }
                }
                self.carry.clear();self.skip_line=false;
            }else if !self.skip_line{
                if self.carry.len()<MAX_LINE{self.carry.push(byte);}else{self.carry.clear();self.skip_line=true;}
            }
        }
    }
    fn read(&mut self,path:&Path,pid:Option<u32>,now:u64)->Option<NowPlaying>{
        let Some(pid)=pid else{*self=Self::default();return None;};
        if self.pid!=pid{*self=Self{pid,..Self::default()};}
        let mut file=match File::open(path){Ok(file)=>file,Err(_)=>{*self=Self{pid,..Self::default()};return None;}};
        let size=file.metadata().ok()?.len();
        let mut prefix=vec![0;64.min(size as usize)];file.read_exact(&mut prefix).ok()?;
        if size<self.offset || !prefix.starts_with(&self.prefix){*self=Self{pid,..Self::default()};}
        self.prefix=prefix;
        if size>self.offset{
            let start=self.offset.max(size.saturating_sub(MAX_READ));
            if start>self.offset{self.carry.clear();self.skip_line=true;}
            file.seek(SeekFrom::Start(start)).ok()?;
            let mut bytes=vec![0;(size-start)as usize];file.read_exact(&mut bytes).ok()?;
            let replay=self.offset==0;
            self.offset=size;self.ingest(&bytes,now,replay);
        }
        // 按歌曲 ID 补充不完整的元数据；有封面、歌名也可能仍缺少时长。
        let id=self.playback.media.song_id;
        if id.is_some() && (!self.playback.media.is_music() || self.playback.media.artist.is_empty() || self.playback.media.duration<=0.0 || self.playback.artwork_url.is_empty()) && (self.lookup.0!=id || now.saturating_sub(self.lookup.1)>=5000){
            let root=path.parent()?;
            // 大歌单的队列文件可达数十 MB：同一首歌且文件没有变化时不重复查找（这首歌本来就没有封面时会一直满足上面的条件）。
            let print=queue_fingerprint(root);
            // 完整读过、确实没有这首歌（搜索、每日推荐等不在队列里的歌）也记下指纹：文件不变就不再整份重读，
            // 否则大歌单下每 5 秒都要解析几十 MB。只有读取失败（被占用、写入中途）才清空指纹，稍后重试。
            if self.lookup.0!=id || self.queue_print!=print {
                match queue_lookup(root,id?) {
                    QueueLookup::Found(metadata)=>{self.playback.metadata(metadata);self.queue_print=print;self.queue_missed=None;}
                    QueueLookup::Absent=>{self.queue_print=print;self.queue_missed=id;}
                    QueueLookup::Unreadable=>{self.queue_print.clear();self.queue_missed=id;}
                }
            }
            self.lookup=(id,now);
        }
        // 本地查不到歌名（漫游等模式）时按 ID 在线补全；每次轮询都取一下结果，查到后下一次采样就能显示。
        if let Some(id)=id.filter(|id|self.queue_missed==Some(*id)) {
            if !self.playback.media.is_music() || self.playback.media.artist.is_empty() || self.playback.media.duration<=0.0 || self.playback.artwork_url.is_empty() {
                let load:fn(u64)->Option<(NowPlaying,String)>=if cfg!(test){|_|None}else{fetch_detail};
                if let Some(metadata)=self.detail.get_with(id,load){self.playback.metadata(metadata);}
            }
        }
        self.playback.snapshot(now)
    }
    pub(super) fn poll(&mut self)->Option<NowPlaying>{
        let path=std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("NetEase/CloudMusic/cloudmusic.elog");
        let sampled=std::time::Instant::now();
        let mut media=self.read(&path,process_id(),monotonic_millis())?;
        if self.cover.url==self.playback.artwork_url{media.artwork=self.cover.image.clone();}
        media.sampled_at=Some(sampled);Some(media)
    }
    pub(super) fn fill_artwork(&mut self,media:&mut NowPlaying){
        if media.source.eq_ignore_ascii_case("cloudmusic.exe") && media.song_id==self.playback.media.song_id && media.artwork.is_none(){
            media.artwork=self.cover.get(&self.playback.artwork_url);
        }
    }
}

const QUEUE_FILES:[(&str,&str);2]=[("playingList","list"),("fmPlay","queue")];

fn queue_fingerprint(root:&Path)->Vec<Option<(std::time::SystemTime,u64)>>{
    QUEUE_FILES.iter().map(|(file,_)|std::fs::metadata(root.join("webdata/file").join(file)).ok().and_then(|meta|Some((meta.modified().ok()?,meta.len())))).collect()
}

/// 队列文件只防异常：长期用户的「我喜欢」等大歌单可达上万首（约 2KB/首）。
const QUEUE_LIMIT:u64=256*1024*1024;

/// 队列查找结果：找到（可能缺时长）、两个队列都完整读过且没有这首歌、或有队列暂时读不了。
#[derive(Debug)]
enum QueueLookup {Found((NowPlaying,String)),Absent,Unreadable}

fn queue_lookup(root:&Path,id:u64)->QueueLookup{
    let (mut partial,mut unreadable)=(None,false);
    for (file,key) in QUEUE_FILES{
        let found=(||{
            let file=match File::open(root.join("webdata/file").join(file)){
                Ok(file)=>file,
                // 没有这个队列（比如从没用过私人 FM）不算读取失败。
                Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
                Err(_)=>return Err(()),
            };
            // 超过上限的文件不变就一直超限，按「没有」处理，不反复尝试。
            if file.metadata().map_err(|_|())?.len()>QUEUE_LIMIT{crate::resources::REJECTED_DATA.fetch_add(1,std::sync::atomic::Ordering::Relaxed);return Ok(None);}
            search_queue(std::io::BufReader::new(file),key,id)
        })();
        let found=match found {Ok(Some(found))=>found,Ok(None)=>continue,Err(())=>{unreadable=true;continue;}};
        if found.0.duration>0.0{return QueueLookup::Found(found);}
        if partial.is_none(){partial=Some(found);}
    }
    match (partial,unreadable) {(Some(found),_)=>QueueLookup::Found(found),(None,true)=>QueueLookup::Unreadable,(None,false)=>QueueLookup::Absent}
}

/// 在 `{"<key>":[歌曲, …]}` 中逐首解析，找到第一首 ID 匹配的歌即停止：
/// 内存只保留一首歌（约 2KB），不再把整个队列解析成 JSON 树。
fn find_in_queue<R:Read>(reader:R,key:&str,id:u64)->Option<(NowPlaying,String)>{search_queue(reader,key,id).ok().flatten()}

/// 与 `find_in_queue` 相同，但区分「完整读完、确实没有这首歌」（`Ok(None)`）
/// 和「格式不符或写入中途截断」（`Err`）：前者文件不变就不必再查，后者要稍后重试。
fn search_queue<R:Read>(reader:R,key:&str,id:u64)->Result<Option<(NowPlaying,String)>,()>{
    use serde::de::{Deserializer as _,DeserializeSeed,Error as _,IgnoredAny,MapAccess,SeqAccess,Visitor};
    const FOUND:&str="daozai:found";
    struct List<'a>{id:u64,found:&'a mut Option<(NowPlaying,String)>}
    impl<'de> DeserializeSeed<'de> for List<'_> {
        type Value=();
        fn deserialize<D:serde::Deserializer<'de>>(self,deserializer:D)->Result<(),D::Error>{deserializer.deserialize_seq(self)}
    }
    impl<'de> Visitor<'de> for List<'_> {
        type Value=();
        fn expecting(&self,f:&mut std::fmt::Formatter)->std::fmt::Result{f.write_str("歌曲数组")}
        fn visit_seq<A:SeqAccess<'de>>(self,mut seq:A)->Result<(),A::Error>{
            while let Some(value)=seq.next_element::<serde_json::Value>()? {
                if let Some(hit)=track(&value).filter(|t|t.0.song_id==Some(self.id)){*self.found=Some(hit);return Err(A::Error::custom(FOUND));}
            }
            Ok(())
        }
    }
    struct Root<'a>{key:&'a str,id:u64,found:&'a mut Option<(NowPlaying,String)>}
    impl<'de> Visitor<'de> for Root<'_> {
        type Value=();
        fn expecting(&self,f:&mut std::fmt::Formatter)->std::fmt::Result{f.write_str("队列对象")}
        fn visit_map<A:MapAccess<'de>>(self,mut map:A)->Result<(),A::Error>{
            while let Some(name)=map.next_key::<String>()? {
                if name==self.key {map.next_value_seed(List{id:self.id,found:&mut *self.found})?;}
                else {map.next_value::<IgnoredAny>()?;}
            }
            Ok(())
        }
    }
    let mut found=None;
    // 找到后以约定错误提前结束解析；其余错误（格式不符、写入中途截断）视为暂时读不了。
    let result=serde_json::Deserializer::from_reader(reader).deserialize_map(Root{key,id,found:&mut found});
    if found.is_some(){return Ok(found);}
    result.map(|_|None).map_err(|_|())
}

fn process_id()->Option<u32>{
    use windows::{core::{w,PCWSTR},Win32::UI::WindowsAndMessaging::{FindWindowW,GetWindowThreadProcessId}};
    unsafe{let window=FindWindowW(w!("OrpheusBrowserHost"),PCWSTR::null()).ok()?;let mut pid=0;GetWindowThreadProcessId(window,Some(&mut pid));(pid!=0).then_some(pid)}
}

fn control_hotkey(command:super::Transport)->Option<&'static str>{
    match command {super::Transport::Toggle=>Some("play_pause_local"),super::Transport::Next=>Some("next_local"),
        super::Transport::Prev=>Some("prev_local"),super::Transport::Seek(_)|super::Transport::SetVolume(_)|super::Transport::SetMute(_)=>None}
}

pub(super) fn control(command:super::Transport){
    use windows::{core::{w,HSTRING,PCWSTR},Win32::{Foundation::{LPARAM,WPARAM},System::DataExchange::GlobalFindAtomW,
        UI::WindowsAndMessaging::{FindWindowW,PostMessageW,WM_HOTKEY}}};
    let Some(name)=control_hotkey(command) else{return;};
    unsafe{if let Ok(window)=FindWindowW(w!("OrpheusBrowserHost"),PCWSTR::null()){
        // 读取网易云当前注册的命令，不缓存随进程变化的编号，也不广播系统媒体键。
        let atom=GlobalFindAtomW(&HSTRING::from(name));
        if atom!=0 {let _=PostMessageW(window,WM_HOTKEY,WPARAM(atom as usize),LPARAM(0));}
    }}
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn log_clock_origin_does_not_change_playback_position(){
        for origin in [0,9_000_000_000] {
            let mut state=Playback::default();
            state.line(&event(7,origin+1000,&metadata()),7,8_000_000);
            state.line(&event(7,origin+2000,"\"native播放资源load完成\",{\"songId\":\"123\"}"),7,8_000_000);
            assert_eq!(state.snapshot(8_001_000).unwrap().position,1.0);
            state.line(&event(7,origin+3000,"\"native播放state\",2"),7,8_002_000);
            assert_eq!(state.snapshot(8_010_000).unwrap().position,2.0);
            state.line(&event(7,origin+4000,"\"setPlayingPosition\",30"),7,8_010_000);
            state.line(&event(7,origin+5000,"\"native播放state\",1"),7,8_011_000);
            assert_eq!(state.snapshot(8_012_000).unwrap().position,31.0);
            state.line(&event(7,origin+2500,"\"setPlayingPosition\",200"),7,8_012_000);
            assert_eq!(state.snapshot(8_013_000).unwrap().position,32.0);
        }
    }
    #[test]
    fn historical_replay_restores_mid_song_and_then_uses_local_elapsed_time(){
        use chrono::TimeZone;
        let now=chrono::Local.with_ymd_and_hms(2026,11,3,17,0,13).single().unwrap().timestamp_millis() as u64;
        let raw=event(7,9_000_000_001,&metadata())
            +&event(7,9_000_000_002,"\"native播放资源load完成\",{\"songId\":\"123\"}")
            +&event(7,9_000_000_003,"\"native播放state\",2").replace("170000","170005")
            +&event(7,9_000_000_004,"\"native播放state\",1").replace("170000","170010");
        let mut reader=Reader{pid:7,..Default::default()};
        reader.ingest(&encode(&raw),now,true);
        assert_eq!(reader.playback.snapshot(now).unwrap().position,8.0);
        reader.ingest(&encode(&event(7,9_000_000_005,"\"setPlayingPosition\",40")),now+1000,false);
        assert_eq!(reader.playback.snapshot(now+3000).unwrap().position,42.0);
    }

    #[test]
    fn historical_date_handles_new_year_without_using_log_counter(){
        use chrono::TimeZone;
        let now=chrono::Local.with_ymd_and_hms(2027,1,1,0,0,2).single().unwrap().timestamp_millis() as u64;
        let line=event(7,42,"\"native播放state\",1").replace("1103/170000","1231/235959");
        assert_eq!(historical_millis(&line,now),Some(now-3000));
        let future=event(7,42,"\"native播放state\",1").replace("1103/170000","0101/000010");
        assert_eq!(historical_millis(&future,now),None,"未来时间不能被误认为去年的事件");
    }
    #[test]
    fn incomplete_artist_does_not_discard_a_known_native_song(){
        let (media,_)=track(&serde_json::json!({"id":123,"name":"待补全的歌曲","duration":120000})).unwrap();
        assert!(media.artist.is_empty() && media.is_music());assert_eq!(media.song_id,Some(123));
    }
    #[test]
    fn transport_uses_only_netease_local_commands(){
        use super::super::Transport;
        assert_eq!(control_hotkey(Transport::Toggle),Some("play_pause_local"));
        assert_eq!(control_hotkey(Transport::Next),Some("next_local"));
        assert_eq!(control_hotkey(Transport::Prev),Some("prev_local"));
        assert_eq!(control_hotkey(Transport::Seek(30.0)),None);
    }
    #[test]
    fn native_track_reads_public_album_cover_and_clears_it_on_song_change(){
        let mut state=Playback::default();
        let data=serde_json::json!({"id":123,"name":"歌曲","artists":[{"name":"歌手"}],"album":{"name":"专辑","picUrl":"http://p3.music.126.net/cover.jpg"}});
        state.line(&event(7,1000,&format!("\"playOneTrackInPlayingList\",{data}")),7,10000);
        assert_eq!(state.artwork_url,"https://p3.music.126.net/cover.jpg?param=256y256");
        state.line(&event(7,2000,"\"native播放资源load完成\",{\"songId\":\"456\"}"),7,10000);
        assert!(state.artwork_url.is_empty());
        for url in ["file:///cover.jpg","https://music.126.net.evil.test/cover.jpg","https://example.com/cover.jpg"]{assert!(cover_url(url).is_none());}
    }
    #[test]
    fn native_cover_cache_reuses_image_and_discards_late_results(){
        let image=Arc::new(eframe::egui::ColorImage::new([2,2],eframe::egui::Color32::BLUE));
        let (tx,rx)=mpsc::channel();
        let mut cache=CoverCache{url:"cover-a".into(),pending:Some(rx),..Default::default()};
        assert!(cache.get("cover-a").is_none(),"封面下载中不能阻塞媒体刷新");
        tx.send(Some(image.clone())).unwrap();
        assert!(Arc::ptr_eq(&cache.get("cover-a").unwrap(),&image));
        assert!(Arc::ptr_eq(&cache.get("cover-a").unwrap(),&image));
        let (tx,rx)=mpsc::channel();cache.pending=Some(rx);
        assert!(cache.get("").is_none());assert!(tx.send(Some(image)).is_err(),"旧请求不得把封面写入新歌");
    }
    #[test]
    fn native_cover_only_fills_the_selected_netease_song(){
        let image=Arc::new(eframe::egui::ColorImage::new([2,2],eframe::egui::Color32::BLUE));
        let mut reader=Reader::default();reader.playback.media.song_id=Some(123);reader.playback.artwork_url="cover".into();
        reader.cover=CoverCache{url:"cover".into(),image:Some(image.clone()),..Default::default()};
        for source in ["SodaMusic.exe","KuGou.exe","QQMusic.exe","Spotify.exe"]{
            let mut media=NowPlaying{source:source.into(),song_id:Some(123),..Default::default()};reader.fill_artwork(&mut media);assert!(media.artwork.is_none());
        }
        let mut media=NowPlaying{source:"cloudmusic.exe".into(),song_id:Some(456),..Default::default()};reader.fill_artwork(&mut media);assert!(media.artwork.is_none());
        media.song_id=Some(123);reader.fill_artwork(&mut media);assert!(Arc::ptr_eq(media.artwork.as_ref().unwrap(),&image));
        let system_image=Arc::new(eframe::egui::ColorImage::new([2,2],eframe::egui::Color32::RED));
        media.artwork=Some(system_image.clone());reader.fill_artwork(&mut media);assert!(Arc::ptr_eq(media.artwork.as_ref().unwrap(),&system_image));
    }
    fn event(pid:u32,at:u64,body:&str)->String{format!("[{pid}:1:1103/170000:{at}:INFO:app.cpp(1)] 【playing】,{body}\n")}
    fn metadata()->String{format!("\"playOneTrackInPlayingList\",{}",serde_json::json!({"track":{"id":"123","name":"测试歌曲","artists":[{"name":"歌手"}],"duration":240000,"album":{"name":"专辑"}}}))}
    fn encode(s:&str)->Vec<u8>{let mut table=[0;256];for b in 0..=255{table[decode(b)as usize]=b;}s.bytes().map(|b|table[b as usize]).collect()}
    #[test]
    fn native_log_tracks_load_pause_resume_seek_and_ignores_other_processes(){
        let mut state=Playback::default();
        state.line(&event(7,1000,&metadata()),7,1000);assert!(state.snapshot(1500).is_none());
        state.line(&event(7,2000,"\"native播放资源load完成，开始播放\",{\"songId\":\"123\"}"),7,2000);
        assert_eq!(state.snapshot(3000).unwrap().position,1.0);
        state.line(&event(8,3500,"\"setPlayingPosition\",99"),7,3500);
        state.line(&event(7,4000,"\"native播放state\",2"),7,4000);
        assert_eq!(state.snapshot(8000).unwrap().position,2.0);
        state.line(&event(7,5000,"\"setPlayingPosition\",30.5"),7,5000);
        state.line(&event(7,6000,"\"native播放state\",1"),7,6000);
        let media=state.snapshot(7000).unwrap();assert!(media.playing);assert_eq!(media.position,31.5);assert_eq!(media.song_id,Some(123));
        state.line(&event(7,8000,"\"setPlayingPosition\",NaN"),7,8000);assert_eq!(state.snapshot(8000).unwrap().position,32.5);
        state.line(&event(7,9000,"\"exitApp\""),7,9000);assert!(state.snapshot(10000).is_none());
    }
    #[test]
    fn native_log_handles_partial_utf8_and_new_process_without_stale_tracks(){
        let dir=std::env::temp_dir().join(format!("daozai-elog-{}",std::process::id()));std::fs::create_dir_all(&dir).unwrap();let path=dir.join("cloudmusic.elog");
        let raw=encode(&(event(7,1000,&metadata())+&event(7,2000,"\"native播放资源load完成\",{\"songId\":\"123\"}")));
        let mut reader=Reader{pid:7,..Default::default()};
        for byte in &raw{reader.ingest(&[*byte],3000,false);}
        assert_eq!(reader.playback.snapshot(3000).unwrap().title,"测试歌曲");
        std::fs::write(&path,&raw).unwrap();reader=Reader::default();
        assert_eq!(reader.read(&path,Some(7),3000).unwrap().position,0.0);
        assert_eq!(reader.read(&path,Some(7),4000).unwrap().position,1.0);
        assert!(reader.read(&path,None,4000).is_none());
        assert!(reader.read(&path,Some(8),4000).is_none(),"新进程不能继承旧进程的歌曲");
        let raw=encode(&(event(8,5000,&metadata())+&event(8,6000,"\"native播放资源load完成\",{\"songId\":\"123\"}")));
        std::fs::write(&path,&raw).unwrap();assert_eq!(reader.read(&path,Some(8),7000).unwrap().position,0.0);
        std::fs::write(&path,b"").unwrap();assert!(reader.read(&path,Some(8),7000).is_none());
        std::fs::remove_file(path).unwrap();std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn native_log_resumes_from_queue_metadata_without_any_smtc_session(){
        let dir=std::env::temp_dir().join(format!("daozai-elog-queue-{}",std::process::id()));let queue=dir.join("webdata/file");std::fs::create_dir_all(&queue).unwrap();
        let data=serde_json::json!({"list":[{"track":{"id":123,"name":"续播歌曲","artists":[{"name":"歌手"}],"duration":240000,"album":{"picUrl":"https://p1.music.126.net/resume.jpg"}}}]});
        std::fs::write(queue.join("playingList"),data.to_string()).unwrap();let path=dir.join("cloudmusic.elog");
        std::fs::write(&path,encode(&(event(9,1000,"\"native播放资源load完成\",{\"songId\":\"123\"}")+&event(9,1100,"\"setPlayingPosition\",80")))).unwrap();
        let mut reader=Reader::default();let media=reader.read(&path,Some(9),2100).unwrap();assert_eq!(media.title,"续播歌曲");assert_eq!(media.position,80.0);
        assert_eq!(reader.playback.artwork_url,"https://p1.music.126.net/resume.jpg?param=256y256");
        std::fs::remove_file(path).unwrap();std::fs::remove_file(queue.join("playingList")).unwrap();std::fs::remove_dir(&queue).unwrap();std::fs::remove_dir(dir.join("webdata")).unwrap();std::fs::remove_dir(dir).unwrap();
    }
    struct QueueFixture {root:std::path::PathBuf,path:std::path::PathBuf}
    impl QueueFixture {
        fn new(case:&str)->Self{
            let root=std::env::temp_dir().join(format!("daozai-elog-{case}-{}",std::process::id()));
            std::fs::create_dir_all(root.join("webdata/file")).unwrap();
            Self{path:root.join("cloudmusic.elog"),root}
        }
        fn queue(&self,name:&str,key:&str,tracks:serde_json::Value){
            std::fs::write(self.root.join("webdata/file").join(name),serde_json::json!({key:tracks}).to_string()).unwrap();
        }
        fn log(&self,track:serde_json::Value){
            let metadata=format!("\"playOneTrackInPlayingList\",{}",serde_json::json!({"track":track}));
            let id=track["id"].clone();
            std::fs::write(&self.path,encode(&(event(9,1000,&metadata)
                +&event(9,1100,&format!("\"native播放资源load完成\",{}",serde_json::json!({"songId":id})))
                +&event(9,1200,"\"setPlayingPosition\",80")))).unwrap();
        }
    }
    impl Drop for QueueFixture {fn drop(&mut self){
        for name in ["playingList","fmPlay"]{let _=std::fs::remove_file(self.root.join("webdata/file").join(name));}
        let _=std::fs::remove_file(&self.path);
        for dir in [self.root.join("webdata/file"),self.root.join("webdata"),self.root.clone()]{let _=std::fs::remove_dir(dir);}
    }}
    fn incomplete_track(id:u64)->serde_json::Value{
        serde_json::json!({"id":id,"name":"已有歌名和封面","artists":[{"name":"已有歌手"}],"album":{"name":"已有专辑","picUrl":"https://p1.music.126.net/test.jpg"}})
    }
    #[test]
    fn native_queue_fills_missing_duration_without_requiring_missing_cover(){
        let files=QueueFixture::new("duration");let mut complete=incomplete_track(123);complete["duration"]=240000.into();
        files.queue("playingList","list",serde_json::json!([complete]));files.log(incomplete_track(123));
        let mut reader=Reader::default();let media=reader.read(&files.path,Some(9),2000).unwrap();
        assert_eq!(media.duration,240.0,"有歌名和封面也必须补齐时长");assert_eq!(media.position,80.0);
        assert_eq!(reader.read(&files.path,Some(9),4000).unwrap().position,82.0,"补时长不能重置播放进度");
    }
    #[test]
    fn sparse_metadata_does_not_erase_known_duration_artist_or_album(){
        let mut state=Playback::default();let mut complete=incomplete_track(123);complete["duration"]=240000.into();
        state.media=track(&complete).unwrap().0;state.media.position=80.0;state.ready=true;
        let sparse=serde_json::json!({"id":123,"name":"已有歌名和封面"});
        state.line(&event(9,1000,&format!("\"playOneTrackInPlayingList\",{sparse}")),9,2000);
        let media=state.snapshot(2000).unwrap();
        assert_eq!(media.duration,240.0);assert_eq!(media.artist,"已有歌手");assert_eq!(media.album,"已有专辑");assert_eq!(media.position,80.0);
        state.line(&event(9,1100,&format!("\"playOneTrackInPlayingList\",{}",incomplete_track(456))),9,3000);
        assert_eq!(state.media.duration,0.0,"新歌曲不得继承上一首的时长");
    }
    #[test]
    fn native_queue_retries_after_a_temporary_read_lock(){
        use std::os::windows::fs::OpenOptionsExt;
        let files=QueueFixture::new("locked");let mut complete=incomplete_track(123);complete["duration"]=240000.into();
        files.queue("playingList","list",serde_json::json!([complete]));
        let queue=files.root.join("webdata/file/playingList");
        let lock=std::fs::OpenOptions::new().write(true).share_mode(2|4).open(&queue).unwrap();
        let before=queue_fingerprint(&files.root);assert!(before[0].is_some());
        assert!(File::open(&queue).is_err(),"测试应复现队列暂时不允许读取");
        // 让原来的补全条件也成立，单独验证读取失败后能否重试。
        let mut partial=incomplete_track(123);partial["album"]=serde_json::json!({});files.log(partial);
        let mut reader=Reader::default();assert_eq!(reader.read(&files.path,Some(9),2000).unwrap().duration,0.0);
        drop(lock);assert_eq!(before,queue_fingerprint(&files.root),"释放读取锁不会改变文件指纹");
        assert_eq!(reader.read(&files.path,Some(9),8000).unwrap().duration,240.0,"读取失败不能永久跳过未变化的队列");
    }
    #[test]
    fn song_missing_from_an_unchanged_queue_is_not_reparsed_every_poll(){
        // 搜索、每日推荐的歌不在播放队列里：完整读过一次后，文件不变就不能每 5 秒再整份解析一遍。
        let files=QueueFixture::new("absent");let mut other=incomplete_track(456);other["duration"]=240000.into();
        files.queue("playingList","list",serde_json::json!([other]));files.log(incomplete_track(123));
        let mut reader=Reader::default();assert_eq!(reader.read(&files.path,Some(9),2000).unwrap().duration,0.0);
        assert_eq!(reader.queue_print,queue_fingerprint(&files.root),"确实没有这首歌时记下队列指纹");
        assert!(matches!(queue_lookup(&files.root,123),QueueLookup::Absent));
        let mut found=incomplete_track(123);found["duration"]=180000.into();
        files.queue("playingList","list",serde_json::json!([other,found]));
        assert_eq!(reader.read(&files.path,Some(9),8000).unwrap().duration,180.0,"队列变化后照常重新查找");
        // 漫游等模式：日志只有歌曲 ID，队列里也没有，界面应显示在线补全的歌名。
        let next=format!("\"native播放资源load完成，开始播放\",{{\"songId\":\"789\"}}");
        { use std::io::Write; std::fs::OpenOptions::new().append(true).open(&files.path).unwrap().write_all(&encode(&event(9,3000,&next))).unwrap(); }
        assert!(reader.read(&files.path,Some(9),9000).is_none(),"只有 ID、没有歌名时不显示");
        assert_eq!(reader.queue_missed,Some(789),"本地确认查不到才在线补全");
        reader.detail=DetailCache{id:Some(789),value:Some((NowPlaying{title:"漫游歌曲".into(),artist:"歌手".into(),song_id:Some(789),duration:200.0,playback_type:MediaKind::Music,source:"cloudmusic.exe".into(),..Default::default()},String::new())),..Default::default()};
        let media=reader.read(&files.path,Some(9),9500).unwrap();
        assert_eq!((media.title.as_str(),media.song_id),("漫游歌曲",Some(789)),"在线补全的歌名套到当前歌曲");
        std::fs::write(files.root.join("webdata/file/playingList"),b"{\"list\":[{\"track\":").unwrap();
        assert!(matches!(queue_lookup(&files.root,123),QueueLookup::Unreadable),"写入中途截断视为暂时读不了，需要重试");
    }
    #[test]
    fn native_queue_prefers_a_known_duration_and_never_uses_another_song(){
        let files=QueueFixture::new("queues");let partial=incomplete_track(123);let mut complete=partial.clone();complete["duration"]=240000.into();
        files.queue("playingList","list",serde_json::json!([partial]));files.queue("fmPlay","queue",serde_json::json!([complete]));files.log(incomplete_track(123));
        let mut reader=Reader::default();assert_eq!(reader.read(&files.path,Some(9),2000).unwrap().duration,240.0,"普通队列缺少时长时还应核对 FM 队列");
        use std::io::Write;
        let next=format!("\"playOneTrackInPlayingList\",{}",incomplete_track(456));
        std::fs::OpenOptions::new().append(true).open(&files.path).unwrap().write_all(&encode(&(event(9,2000,&next)
            +&event(9,2100,"\"native播放资源load完成\",{\"songId\":456}")))).unwrap();
        let media=reader.read(&files.path,Some(9),3000).unwrap();
        assert_eq!(media.song_id,Some(456));assert_eq!(media.duration,0.0,"不得补入其他歌曲的时长");
        let mut new_track=incomplete_track(456);new_track["duration"]=180000.into();
        files.queue("playingList","list",serde_json::json!([new_track]));
        assert_eq!(reader.read(&files.path,Some(9),9000).unwrap().duration,180.0,"队列更新后应补齐新歌曲时长");
    }
    #[test]
    fn queue_duration_fill_preserves_existing_artist_album_and_cover(){
        let files=QueueFixture::new("preserve");files.log(incomplete_track(123));
        files.queue("playingList","list",serde_json::json!([{"id":123,"name":"已有歌名和封面","duration":240000}]));
        let mut reader=Reader::default();let media=reader.read(&files.path,Some(9),2000).unwrap();
        assert_eq!(media.duration,240.0);assert_eq!(media.artist,"已有歌手");assert_eq!(media.album,"已有专辑");
        assert_eq!(reader.playback.artwork_url,"https://p1.music.126.net/test.jpg?param=256y256");
    }
    /// 旧读法：整份解析后取第一首 ID 匹配的歌，用于新旧结果对比。
    fn whole_file(raw:&[u8],key:&str,id:u64)->Option<(NowPlaying,String)>{
        let value:serde_json::Value=serde_json::from_slice(raw).ok()?;
        value.get(key)?.as_array()?.iter().filter_map(track).find(|t|t.0.song_id==Some(id))
    }
    fn same(a:&Option<(NowPlaying,String)>,b:&Option<(NowPlaying,String)>)->bool{
        match (a,b) {
            (None,None)=>true,
            (Some(a),Some(b))=>a.1==b.1 && (&a.0.title,&a.0.artist,&a.0.album,a.0.song_id,a.0.duration)==(&b.0.title,&b.0.artist,&b.0.album,b.0.song_id,b.0.duration),
            _=>false,
        }
    }
    #[test]
    fn large_queue_is_searched_song_by_song_with_the_same_result(){
        // 5000 首、约 2.2KB/首（与本机实测相近）：超过原先 8MB 整份解析的上限。
        let pad="填".repeat(700);
        let tracks:Vec<_>=(1..=5000u64).map(|id|serde_json::json!({"track":{"id":id,"name":format!("歌{id}"),"artists":[{"name":"歌手"}],
            "album":{"name":"专辑","picUrl":"https://p1.music.126.net/a.jpg"},"duration":200000+id,"extra":pad}})).collect();
        let raw=serde_json::json!({"before":{"skip":[1,2,3]},"list":tracks,"after":"x"}).to_string().into_bytes();
        assert!(raw.len()>8*1024*1024,"测试队列应超过旧上限：{}",raw.len());
        for id in [1,2500,5000] {
            let found=find_in_queue(raw.as_slice(),"list",id);
            assert_eq!(found.as_ref().map(|t|t.0.duration),Some((200000+id) as f64/1000.0));
            assert!(same(&found,&whole_file(&raw,"list",id)),"第 {id} 首新旧结果不一致");
        }
        assert!(find_in_queue(raw.as_slice(),"list",5001).is_none());
        assert!(find_in_queue(raw.as_slice(),"queue",1).is_none(),"只在指定的键下查找");
        // 同一 ID 出现两次时取第一首，与旧读法一致。
        let dup=serde_json::json!({"list":[{"id":7,"name":"第一首"},{"id":7,"name":"第二首","duration":1000}]}).to_string();
        assert_eq!(find_in_queue(dup.as_bytes(),"list",7).unwrap().0.title,"第一首");
        // 客户端写入到一半：目标歌曲已完整写出时可用，未写完时不使用残缺数据。
        let cut=&raw[..raw.len()/2];
        assert_eq!(find_in_queue(cut,"list",1).unwrap().0.song_id,Some(1));
        assert!(find_in_queue(cut,"list",5000).is_none());
        for bad in [&b"[]"[..],b"{\"list\":{}}",b"{\"list\":[1,\"x\",null]}",b""] {assert!(find_in_queue(bad,"list",1).is_none());}
    }
    #[test]
    #[ignore="只读对比本机网易云播放队列：逐首对比整份解析与逐首查找的结果"]
    fn live_queue_search_matches_whole_file_parse(){
        let root=std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("NetEase/CloudMusic/webdata/file");
        let mut count=0;
        for (name,key) in QUEUE_FILES {
            let Ok(raw)=std::fs::read(root.join(name)) else{continue;};
            let value:serde_json::Value=serde_json::from_slice(&raw).unwrap();
            let ids:Vec<u64>=value[key].as_array().map(|list|list.iter().filter_map(track).filter_map(|t|t.0.song_id).collect()).unwrap_or_default();
            for id in ids {assert!(same(&find_in_queue(raw.as_slice(),key,id),&whole_file(&raw,key,id)),"{name} 中 ID {id} 不一致");count+=1;}
        }
        assert!(count>0);println!("网易云队列：{count} 首歌两种读法结果一致");
    }
    #[test]
    #[ignore="需要本机网易云正在播放，用于验证无 SMTC 的原生数据和歌词"]
    fn live_native_netease_playback_and_lyrics(){
        let mut reader=Reader::default();let first=reader.poll().expect("未读到网易云原生播放事件");
        println!("歌曲={} 歌手={} ID={:?} 播放={} 进度={:.2}/{:.2}",first.title,first.artist,first.song_id,first.playing,first.position,first.duration);
        let lyrics=crate::lyrics::fetch_for_source(&first.source,&first.title,&first.artist,first.duration,first.song_id).unwrap();
        assert!(lyrics.synced && !lyrics.lines.is_empty());println!("已获取 {} 行同步歌词",lyrics.lines.len());
        std::thread::sleep(std::time::Duration::from_secs(2));let next=reader.poll().unwrap();
        if first.playing && next.playing && first.song_id==next.song_id{assert!(next.position>first.position);}
    }
    #[test]
    #[ignore="需要本机网易云已加载歌曲；短暂播放后恢复原来的暂停状态，不查询在线歌词"]
    fn live_native_netease_duration_and_progress(){
        let mut reader=Reader::default();let initial=reader.poll().expect("未读到网易云原生播放事件");
        let shared=super::super::shared();let _poller=super::super::spawn_poller(shared.clone(),eframe::egui::Context::default());
        struct RestorePause(Option<u64>);
        impl Drop for RestorePause{fn drop(&mut self){
            if let Some(id)=self.0{if Reader::default().poll().is_some_and(|m|m.song_id==Some(id) && m.playing){control(super::super::Transport::Toggle);}}
        }}
        let _restore=RestorePause(if initial.playing{None}else{initial.song_id});
        if !initial.playing{control(super::super::Transport::Toggle);}
        let until=Instant::now()+Duration::from_secs(12);
        let first=loop{
            if let Some(media)=reader.poll().filter(|m|m.playing && m.duration>0.0){break media;}
            assert!(Instant::now()<until,"未取得有效的播放状态和时长");std::thread::sleep(Duration::from_millis(100));
        };
        while !shared.lock().unwrap().playing{
            assert!(Instant::now()<until,"媒体轮询未恢复播放状态");std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_secs(2));let next=reader.poll().expect("播放记录中断");
        assert_eq!(next.song_id,first.song_id);assert!(next.playing && next.position>first.position);
        assert_eq!(next.duration,first.duration);
        let selected=shared.lock().unwrap().clone();assert_eq!(selected.song_id,next.song_id);assert_eq!(selected.duration,next.duration);
        assert!((selected.position-next.position).abs()<2.0,"合并系统媒体信息后不能丢失实际播放位置");
        println!("网易云原生路径：时长 {:.3}s，进度 {:.3}s → {:.3}s，初始播放状态={}，歌曲 ID={:?}",first.duration,first.position,next.position,initial.playing,first.song_id);
    }
    #[test]
    #[ignore="需要本机网易云正在播放，用于验证原生封面与缓存"]
    fn live_native_netease_album_cover(){
        let mut reader=Reader::default();let start=Instant::now();
        loop{
            let mut media=reader.poll().expect("未读到网易云原生播放事件");reader.fill_artwork(&mut media);
            if let Some(image)=&media.artwork{
                assert!(image.size[0]>1 && image.size[0]<=256 && image.size[1]>1 && image.size[1]<=256);
                let mut next=reader.poll().unwrap();reader.fill_artwork(&mut next);
                if next.song_id==media.song_id{assert!(Arc::ptr_eq(next.artwork.as_ref().unwrap(),image));}
                println!("歌曲={} 封面={}x{}，重复采样复用缓存",media.title,image.size[0],image.size[1]);break;
            }
            assert!(start.elapsed()<Duration::from_secs(10),"未获取到网易云专辑封面");std::thread::sleep(Duration::from_millis(100));
        }
    }
}
