//! 波点 1.1.7：只读已验证播放引擎的属性缓存与本机歌曲数据库。
use super::{MediaKind,NowPlaying};
use std::{ffi::{c_char,c_void,CString},io::Read,path::PathBuf,sync::Arc,time::{Duration,Instant}};

type Handle=*mut c_void;
#[link(name="kernel32")]
extern "system" {
    fn OpenProcess(access:u32,inherit:i32,pid:u32)->Handle;
    fn CloseHandle(handle:Handle)->i32;
    fn ReadProcessMemory(handle:Handle,address:usize,buffer:*mut c_void,size:usize,read:*mut usize)->i32;
    fn K32EnumProcessModules(handle:Handle,modules:*mut usize,size:u32,needed:*mut u32)->i32;
    fn K32GetModuleBaseNameW(handle:Handle,module:usize,name:*mut u16,size:u32)->u32;
}
struct Process(Handle);
impl Drop for Process {fn drop(&mut self){unsafe{CloseHandle(self.0);}}}
impl Process {
    fn open(pid:u32)->Option<Self>{
        if !super::player_process::image_is(pid,"bodian_pc.exe"){return None;}
        let handle=unsafe{OpenProcess(0x410,0,pid)};
        (!handle.is_null()).then_some(Self(handle))
    }
    fn read<const N:usize>(&self,address:usize)->Option<[u8;N]>{
        let mut bytes=[0;N];let mut count=0;
        (unsafe{ReadProcessMemory(self.0,address,bytes.as_mut_ptr().cast(),N,&mut count)}!=0 && count==N).then_some(bytes)
    }
    fn pointer(&self,address:usize)->Option<usize>{Some(u64::from_le_bytes(self.read(address)?) as usize)}
    fn string(&self,address:usize)->Option<String>{
        let mut bytes=Vec::new();
        for offset in (0..4096).step_by(16) {
            let chunk=self.read::<16>(address.checked_add(offset)?)?;
            if let Some(end)=chunk.iter().position(|b|*b==0){bytes.extend_from_slice(&chunk[..end]);return String::from_utf8(bytes).ok();}
            bytes.extend_from_slice(&chunk);
        }
        None
    }
    fn module(&self,base:usize,stamp:u32,size:u32)->bool{
        let Some(pe)=self.read::<4>(base+60).map(u32::from_le_bytes).filter(|offset|*offset<4096)else{return false;};
        self.read::<4>(base+pe as usize+8).map(u32::from_le_bytes)==Some(stamp)
            && self.read::<4>(base+pe as usize+80).map(u32::from_le_bytes)==Some(size)
    }
    fn node(&self,address:usize,depth:u8)->Option<serde_json::Value>{
        if depth>5{return None;}
        let data=self.read::<16>(address)?;
        let pointer=u64::from_le_bytes(data[..8].try_into().ok()?) as usize;
        match u32::from_le_bytes(data[8..12].try_into().ok()?) {
            1=>Some(self.string(pointer)?.into()),
            3=>Some((pointer!=0).into()),
            4=>Some((pointer as u64).into()),
            8=>{
                let count=u32::from_le_bytes(self.read(pointer)?) as usize;
                if count>64{return None;}
                let values=self.pointer(pointer+8)?;
                let keys=self.pointer(pointer+16)?;let mut map=serde_json::Map::new();
                for i in 0..count {
                    let key=self.string(self.pointer(keys+i*8)?)?;
                    // 不读取或保存带签名的音频地址，只取当前歌曲的标识。
                    if matches!(key.as_str(),"title"|"current"|"playing"|"id") {map.insert(key,self.node(values+i*16,depth+1)?);}
                }
                Some(map.into())
            }
            _=>None,
        }
    }

    fn current_track(&self,address:usize,cursor:&mut usize)->Option<serde_json::Value>{
        let node=self.read::<16>(address)?;
        if u32::from_le_bytes(node[8..12].try_into().ok()?)!=7{return None;}
        let list=u64::from_le_bytes(node[..8].try_into().ok()?) as usize;
        let count=usize::try_from(i32::from_le_bytes(self.read(list)?)).ok()?;
        if count==0{return None;}
        let entries=self.pointer(list.checked_add(8)?)?;
        *cursor%=count;
        // 每轮最多读取 128 项；命中后从当前曲目开始下一轮，避免长队列反复全表扫描。
        for _ in 0..count.min(128){
            let entry=self.node(entries.checked_add(*cursor*16)?,0)?;
            if entry["current"].as_bool()==Some(true){return Some(entry);}
            *cursor=(*cursor+1)%count;
        }
        None
    }
}

#[derive(Clone,Copy)]
struct Target {pid:u32,events:usize,mpv:usize,main:usize,keys:usize}
impl Target {
    fn verified(&self,process:&Process)->bool{
        process.module(self.events,0x69fea10a,0xe000) && process.module(self.mpv,0x650fea09,0x2108000)
    }
    fn discover()->Option<Self>{
        for pid in super::player_process::pids_named("bodian_pc.exe") {
            let Some(process)=Process::open(pid)else{continue;};
            let mut modules=[0usize;1024];let mut needed=0;
            if unsafe{K32EnumProcessModules(process.0,modules.as_mut_ptr(),std::mem::size_of_val(&modules) as u32,&mut needed)}==0{continue;}
            let mut target=Self{pid,events:0,mpv:0,main:modules[0],keys:0};
            for base in modules.into_iter().take((needed as usize/8).min(1024)) {
                let mut text=[0u16;260];let n=unsafe{K32GetModuleBaseNameW(process.0,base,text.as_mut_ptr(),260)} as usize;
                match String::from_utf16_lossy(&text[..n]).as_str(){"media_kit_native_event_loop.dll"=>target.events=base,"libmpv-2.dll"=>target.mpv=base,"hotkey_manager_windows_plugin.dll"=>target.keys=base,_=>{}}
            }
            if target.events!=0 && target.mpv!=0 && target.verified(&process){return Some(target);}
        }
        None
    }
}

fn snapshot(values:&serde_json::Value)->Option<(String,u64,f64,f64,bool)>{
    let position=values["time-pos"].as_f64()?;let duration=values["duration"].as_f64()?;
    if !position.is_finite() || !duration.is_finite() || !(0.0..86400.0).contains(&position)
        || !(0.0..86400.0).contains(&duration) || duration<=0.0 || position>duration+0.5{return None;}
    let current=&values["playlist"];
    if current["current"].as_bool()!=Some(true){return None;}
    let title=current["title"].as_str()?.trim();
    if title.is_empty() || title.len()>4096{return None;}
    let playing=!values["pause"].as_bool()? && !values["core-idle"].as_bool()? && !values["idle-active"].as_bool()?;
    Some((title.into(),current["id"].as_u64()?,position.min(duration),duration,playing))
}

fn playback(process:&Process,target:Target,cursor:&mut usize)->Option<(String,u64,f64,f64,bool)>{
    // 已验证版本的事件注册表持有 mpv 客户端；仅遍历这张小表，不扫描进程内存。
    let head=process.pointer(target.events+0xa1d8)?;let mut node=process.pointer(head)?;
    for _ in 0..8 {
        if node==head{return None;}
        let client=process.pointer(node+16)?;
        if process.string(client)?=="main" {
            let properties=process.pointer(client+0x148)?;
            let count=u32::from_le_bytes(process.read(client+0x150)?) as usize;
            if count>64{return None;}
            let mut values=serde_json::Map::new();let mut playlist=None;
            for i in 0..count {
                let property=process.pointer(properties+i*8)?;
                let name=process.string(process.pointer(property+8)?)?;
                if !matches!(name.as_str(),"time-pos"|"duration"|"pause"|"core-idle"|"idle-active"|"playlist"){continue;}
                let bytes=process.read::<104>(property)?;
                if bytes[80]!=1{return None;}
                let format=u32::from_le_bytes(bytes[40..44].try_into().ok()?);
                let value=match (name.as_str(),format) {
                    ("playlist",6)=>{playlist=Some((property,process.pointer(property+88)?));process.current_track(property+88,cursor)?},
                    ("time-pos"|"duration",5)=>serde_json::Value::from(f64::from_le_bytes(bytes[88..96].try_into().ok()?)),
                    ("pause"|"core-idle"|"idle-active",3)=>serde_json::Value::from(u32::from_le_bytes(bytes[88..92].try_into().ok()?)!=0),
                    _=>return None,
                };
                values.insert(name,value);
            }
            let (property,pointer)=playlist?;
            if process.pointer(property+88)?!=pointer{return None;}
            return snapshot(&values.into());
        }
        node=process.pointer(node)?;
    }
    None
}

#[link(name="winsqlite3")]
extern "C" {
    fn sqlite3_open_v2(name:*const c_char,db:*mut *mut c_void,flags:i32,vfs:*const c_char)->i32;
    fn sqlite3_close(db:*mut c_void)->i32;
    fn sqlite3_busy_timeout(db:*mut c_void,milliseconds:i32)->i32;
    fn sqlite3_prepare_v2(db:*mut c_void,sql:*const c_char,len:i32,statement:*mut *mut c_void,tail:*mut *const c_char)->i32;
    fn sqlite3_step(statement:*mut c_void)->i32;
    fn sqlite3_column_text(statement:*mut c_void,column:i32)->*const u8;
    fn sqlite3_column_bytes(statement:*mut c_void,column:i32)->i32;
    fn sqlite3_finalize(statement:*mut c_void)->i32;
}
struct Database(*mut c_void);
impl Drop for Database {fn drop(&mut self){unsafe{sqlite3_close(self.0);}}}
struct Statement(*mut c_void);
impl Drop for Statement {fn drop(&mut self){unsafe{sqlite3_finalize(self.0);}}}
fn data_root()->Option<PathBuf>{Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("cn.wenyu.bodian/bodian_pc"))}
fn metadata(value:&serde_json::Value,title:&str,duration:f64)->Option<(NowPlaying,String)>{
    if value["name"].as_str()?!=title || (value["duration"].as_f64()?-duration).abs()>2.0{return None;}
    let artist=value["artist"].as_str()?;let album=value["album"].as_str().unwrap_or_default();
    if artist.is_empty() || artist.len()>4096 || album.len()>4096{return None;}
    let song_id=value["id"].as_u64().filter(|id|*id>0)?;
    let cover=value["albumPic120"].as_str().or_else(||value["albumPic"].as_str()).and_then(cover_url).unwrap_or_default();
    Some((NowPlaying{source:"bodian_pc.exe".into(),title:title.into(),artist:artist.into(),album:album.into(),song_id:Some(song_id),
        playback_type:MediaKind::Music,..Default::default()},cover))
}
fn read_metadata(title:&str,duration:f64)->Option<(NowPlaying,String)>{
    let path=CString::new(data_root()?.join("database/songDB.db").to_str()?).ok()?;
    unsafe {
        let mut db=std::ptr::null_mut();let result=sqlite3_open_v2(path.as_ptr(),&mut db,1,std::ptr::null());
        if db.is_null(){return None;}let db=Database(db);if result!=0{return None;}
        sqlite3_busy_timeout(db.0,50);
        let mut statement=std::ptr::null_mut();
        if sqlite3_prepare_v2(db.0,c"SELECT json FROM hist_song ORDER BY time DESC LIMIT 32".as_ptr(),-1,&mut statement,std::ptr::null_mut())!=0{return None;}
        let statement=Statement(statement);
        while sqlite3_step(statement.0)==100 {
            let length=sqlite3_column_bytes(statement.0,0);
            if !(1..=65536).contains(&length){continue;}
            let data=sqlite3_column_text(statement.0,0);if data.is_null(){continue;}
            let Ok(value)=serde_json::from_slice(std::slice::from_raw_parts(data,length as usize))else{continue;};
            if let Some(track)=metadata(&value,title,duration){return Some(track);}
        }
    }
    None
}
fn cover_url(value:&str)->Option<String>{
    if value.len()>4096{return None;}
    let mut url=url::Url::parse(value).ok()?;let host=url.host_str()?;
    if !matches!(url.scheme(),"http"|"https") || !host.ends_with(".kuwo.cn"){return None;}
    url.set_scheme("https").ok()?;Some(url.into())
}
fn fetch_cover(url:&str)->Option<Arc<eframe::egui::ColorImage>>{
    let url=cover_url(url)?;let mut bytes=Vec::new();
    ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(5)).build().get(&url).call().ok()?
        .into_reader().take(2*1024*1024+1).read_to_end(&mut bytes).ok()?;
    if bytes.len()>2*1024*1024{return None;}super::decode_artwork(&bytes).map(Arc::new)
}

#[derive(Default)]
pub(super) struct Reader {
    target:Option<Target>,discovered:Option<Instant>,key:Option<(String,u64)>,metadata:Option<(NowPlaying,String)>,
    playlist_cursor:usize,lookup:Option<Instant>,cover:super::netease_playback::CoverCache,
}
impl Reader {
    pub fn poll(&mut self)->Option<NowPlaying>{
        if self.target.is_none() && discovery_due(self.discovered,window_hint) {
            self.discovered=Some(Instant::now());self.target=Target::discover();
        }
        let target=self.target?;
        let process=Process::open(target.pid).filter(|process|target.verified(process));
        let Some(process)=process else{self.target=None;self.key=None;self.metadata=None;return None;};
        let (title,id,position,duration,playing)=playback(&process,target,&mut self.playlist_cursor)?;let sampled_at=Instant::now();
        let key=(title.clone(),id);
        if self.key.as_ref()!=Some(&key){self.key=Some(key);self.metadata=None;self.lookup=None;}
        if self.metadata.is_none() && self.lookup.is_none_or(|at|at.elapsed()>=Duration::from_secs(1)) {
            self.lookup=Some(Instant::now());self.metadata=read_metadata(&title,duration);
        }
        let (mut media,url)=self.metadata.clone()?;
        media.position=position;media.duration=duration;media.playing=playing;media.sampled_at=Some(sampled_at);
        media.artwork=self.cover.get_with(&url,fetch_cover);Some(media)
    }
}

pub(super) fn control(command:super::Transport)->bool{
    use windows::{core::w,Win32::{Foundation::{LPARAM,WPARAM},UI::WindowsAndMessaging::{FindWindowW,GetWindowThreadProcessId,SendMessageTimeoutW,SMTO_ABORTIFHUNG,SMTO_ERRORONEXIT,WM_HOTKEY}}};
    let id=match command{super::Transport::Toggle=>1,super::Transport::Next=>3,super::Transport::Prev=>4,_=>return false};
    (||->Option<()> {
        let target=Target::discover()?;let process=Process::open(target.pid)?;
        // 仅使用已实测版本的四个原生媒体热键；用户重新注册热键后不猜测新的编号。
        if !process.module(target.main,0x69fea107,0x57000) || !process.module(target.keys,0x69fea0d4,0x1e000){return None;}
        let manager=process.pointer(target.keys+0x19510)?;
        let registrars=process.pointer(manager)?;let registrar=process.pointer(process.pointer(registrars)?+40)?;
        let plugins=process.pointer(registrar+0x20)?;let plugin=process.pointer(process.pointer(plugins)?+32)?;
        if u32::from_le_bytes(process.read(plugin+0x60)?)!=4 || process.pointer(plugin+0x30)?!=4{return None;}
        let head=process.pointer(plugin+0x28)?;let mut node=process.pointer(head)?;let mut ids=0u32;
        for _ in 0..4 {
            if node==head{return None;}
            let registered=u32::from_le_bytes(process.read(node+48)?);
            if !(1..=4).contains(&registered){return None;}ids|=1<<registered;node=process.pointer(node)?;
        }
        if ids!=0x1e || node!=head{return None;}
        unsafe {
            let window=FindWindowW(w!("BODIAN_FLUTTER_WIN32_WINDOW"),None).ok()?;
            let mut pid=0;GetWindowThreadProcessId(window,Some(&mut pid));if pid!=target.pid{return None;}
            (SendMessageTimeoutW(window,WM_HOTKEY,WPARAM(id),LPARAM(0),SMTO_ABORTIFHUNG|SMTO_ERRORONEXIT,500,None).0!=0).then_some(())
        }
    })().is_some()
}

fn decode_lyrics(bytes:&[u8])->Option<crate::lyrics::Lyrics>{
    if bytes.len()>1024*1024{return None;}
    let raw=std::str::from_utf8(bytes).ok()?;let key=b"BD1BYDH79nXZ";
    let decoded=String::from_utf16(&raw.encode_utf16().enumerate().map(|(i,c)|c^key[i%key.len()] as u16).collect::<Vec<_>>()).ok()?;
    if !decoded.starts_with("[kuwo:"){return None;}
    static WORD:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    let clean=WORD.get_or_init(||regex::Regex::new(r"<\d+,-?\d+>").unwrap()).replace_all(&decoded,"");
    let mut lyrics=crate::lyrics::parse_lrc(&clean);
    let offset=decoded.lines().find_map(|line|line.strip_prefix("[offset:")?.strip_suffix(']')?.parse::<f32>().ok()).unwrap_or(0.0);
    if !offset.is_finite() || offset.abs()>86400000.0{return None;}
    for line in &mut lyrics.lines{line.t=(line.t+offset/1000.0).max(0.0);}
    lyrics.synced.then_some(lyrics)
}
pub(crate) fn read_lyrics(id:u64)->Option<crate::lyrics::Lyrics>{
    if id==0{return None;}
    let file=std::fs::File::open(data_root()?.join("cache/lyric").join(format!("{id}.lrc"))).ok()?;
    if file.metadata().ok()?.len()>1024*1024{return None;}
    let mut bytes=Vec::new();file.take(1024*1024+1).read_to_end(&mut bytes).ok()?;decode_lyrics(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_playback_requires_current_track_and_valid_timeline(){
        let mut value=serde_json::json!({"time-pos":12.5,"duration":256.7,"pause":false,"core-idle":false,"idle-active":false,
            "playlist":{"title":"说好的幸福呢","id":1,"current":true}});
        let track=snapshot(&value).unwrap();assert_eq!(track.0,"说好的幸福呢");assert!(track.4);
        value["pause"]=true.into();assert!(!snapshot(&value).unwrap().4);
        value["time-pos"]=999.into();assert!(snapshot(&value).is_none());
        value["time-pos"]=0.into();value["playlist"]["current"]=false.into();assert!(snapshot(&value).is_none());
    }
    #[test]
    fn long_native_playlist_finds_current_track_after_the_first_64(){
        let current_key=*b"current\0........";
        let keys=[current_key.as_ptr() as usize];
        let flag=[1u64,3];
        let empty_map=[0usize,flag.as_ptr() as usize,keys.as_ptr() as usize];
        let map=[1usize,flag.as_ptr() as usize,keys.as_ptr() as usize];
        let mut entries=vec![[empty_map.as_ptr() as u64,8];201];
        entries[200][0]=map.as_ptr() as u64;
        let list=[entries.len(),entries.as_ptr() as usize,0];
        let root=[list.as_ptr() as u64,7];
        let process=Process(unsafe{OpenProcess(0x410,0,std::process::id())});
        assert!(!process.0.is_null());
        let mut cursor=0;
        assert!(process.current_track(root.as_ptr() as usize,&mut cursor).is_none());
        assert_eq!(cursor,128);
        for _ in 0..2{
            assert_eq!(process.current_track(root.as_ptr() as usize,&mut cursor).unwrap()["current"],true);
            assert_eq!(cursor,200);
        }
    }
    #[test]
    fn history_is_matched_to_the_engine_and_cover_hosts_are_checked(){
        let mut value=serde_json::json!({"id":440623,"name":"说好的幸福呢","artist":"周杰伦","album":"魔杰座","duration":256,
            "albumPic120":"http://img4.kuwo.cn/cover.webp"});
        let (media,url)=metadata(&value,"说好的幸福呢",256.7).unwrap();assert_eq!(media.song_id,Some(440623));assert!(url.starts_with("https://"));
        assert!(metadata(&value,"晴天",256.7).is_none());assert!(metadata(&value,"说好的幸福呢",300.0).is_none());
        value["artist"]="".into();assert!(metadata(&value,"说好的幸福呢",256.7).is_none());
        for url in ["file:///cover.png","https://kuwo.cn.example.org/cover","https://example.org/cover"]{assert!(cover_url(url).is_none());}
    }
    #[test]
    fn native_lyrics_decode_word_tags_and_preserve_text_and_offset(){
        let raw="[kuwo:115]\r\n[offset:200]\r\n[00:01.500]<100,-50>我<200,50>怀念的 [合唱]\r\n[00:03.000]第二句";
        let key=b"BD1BYDH79nXZ";
        let encoded=String::from_utf16(&raw.encode_utf16().enumerate().map(|(i,c)|c^key[i%key.len()] as u16).collect::<Vec<_>>()).unwrap();
        let lyrics=decode_lyrics(encoded.as_bytes()).unwrap();assert_eq!(lyrics.lines[0].text,"我怀念的 [合唱]");
        assert!((lyrics.lines[0].t-1.7).abs()<0.001);assert_eq!(lyrics.lines.len(),2);
        for bytes in [b"invalid".as_slice(),&[255],&[0;16]]{assert!(decode_lyrics(bytes).is_none());}
    }
    #[test]
    #[ignore="需要本机波点音乐正在播放"]
    fn live_bodian_cover_lyrics_and_progress(){
        let mut reader=Reader::default();let start=Instant::now();
        let first=loop {
            if let Some(media)=reader.poll().filter(|m|m.playing && m.artwork.is_some()){break media;}
            assert!(start.elapsed()<Duration::from_secs(15),"未读取到波点播放数据和封面");std::thread::sleep(Duration::from_millis(200));
        };
        let lyrics=read_lyrics(first.song_id.unwrap()).expect("未读取到波点原生歌词");assert!(lyrics.synced);
        std::thread::sleep(Duration::from_secs(3));let next=reader.poll().unwrap();
        assert_eq!(first.title,next.title);assert!(next.position>first.position+1.5);
        println!("波点：{} / {}，封面={}，原生歌词={}行，进度={:.2}→{:.2}",first.title,first.artist,first.artwork.is_some(),lyrics.lines.len(),first.position,next.position);
    }
    #[test]
    #[ignore="需要本机波点正在播放；短暂暂停后恢复"]
    fn live_bodian_pause_restores_without_moving_focus(){
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let mut reader=Reader::default();let before=reader.poll().unwrap();assert!(before.playing);
        let foreground=unsafe{GetForegroundWindow()};assert!(control(super::super::Transport::Toggle));
        std::thread::sleep(Duration::from_millis(600));let paused=reader.poll().unwrap();
        let resumed=control(super::super::Transport::Toggle);
        assert!(!paused.playing && resumed);assert_eq!(before.title,paused.title);
        std::thread::sleep(Duration::from_millis(400));assert!(reader.poll().unwrap().playing);
        assert_eq!(unsafe{GetForegroundWindow()},foreground);
    }
}

/// 进程快照一次要十几毫秒：有波点窗口线索时 3 秒查一次，没有线索时（多数用户没装波点）30 秒才兜底查一次。
const DISCOVER_HINTED:Duration=Duration::from_secs(3);
const DISCOVER_FALLBACK:Duration=Duration::from_secs(30);
fn discovery_due(last:Option<Instant>,hint:impl FnOnce()->bool)->bool{
    let since=last.map(|at|at.elapsed());
    since.is_none_or(|since|since>=DISCOVER_FALLBACK) || since.is_some_and(|since|since>=DISCOVER_HINTED) && hint()
}

/// 便宜的线索：波点（Flutter 程序）的主窗口属于 bodian_pc.exe。窗口类名若被改，仍有 30 秒一次的兜底查找。
fn window_hint()->bool{
    use windows::{core::{w,PCWSTR},Win32::{Foundation::HWND,UI::WindowsAndMessaging::{FindWindowExW,GetWindowThreadProcessId}}};
    let mut after=HWND::default();
    for _ in 0..16 {
        let Ok(window)=(unsafe{FindWindowExW(HWND::default(),after,w!("FLUTTER_RUNNER_WIN32_WINDOW"),PCWSTR::null())})else{return false;};
        let mut pid=0;unsafe{GetWindowThreadProcessId(window,Some(&mut pid));}
        if super::player_process::image_is(pid,"bodian_pc.exe"){return true;}
        after=window;
    }
    false
}

#[test]
fn discovery_without_a_window_hint_falls_back_to_a_slow_check(){
    let ago=|secs|Some(Instant::now()-Duration::from_secs(secs));
    assert!(discovery_due(None,||false),"启动后第一次总要查");
    assert!(!discovery_due(ago(1),||true),"有线索也至少隔 3 秒");
    assert!(discovery_due(ago(4),||true));
    assert!(!discovery_due(ago(4),||false),"没有线索时不做 3 秒一次的进程快照");
    assert!(discovery_due(ago(31),||false),"窗口线索失效也能在 30 秒内找到");
    let mut asked=false;assert!(!discovery_due(ago(1),||{asked=true;true}));assert!(!asked,"未到 3 秒不查窗口");
}
