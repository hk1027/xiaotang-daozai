//! 酷我 9.6：读取已验证音频服务的歌曲、输出时钟与暂停标记。
use super::{MediaKind,NowPlaying};
use std::{ffi::c_void,io::Read,path::PathBuf,sync::Arc,time::{Duration,Instant}};
use windows::{core::w,Win32::{Foundation::{CloseHandle,HWND,LPARAM,RECT,WPARAM},System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot,Process32FirstW,Process32NextW,PROCESSENTRY32W,TH32CS_SNAPPROCESS},UI::{HiDpi::{SetThreadDpiAwarenessContext,DPI_AWARENESS_CONTEXT,DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},WindowsAndMessaging::*}}};

#[link(name="kernel32")]
extern "system" {
    fn OpenProcess(access:u32,inherit:i32,pid:u32)->*mut c_void;
    #[link_name="CloseHandle"]
    fn CloseHandleRaw(handle:*mut c_void)->i32;
    fn ReadProcessMemory(handle:*mut c_void,address:usize,buffer:*mut c_void,size:usize,read:*mut usize)->i32;
    fn K32EnumProcessModulesEx(handle:*mut c_void,modules:*mut usize,size:u32,needed:*mut u32,filter:u32)->i32;
    fn K32GetModuleBaseNameW(handle:*mut c_void,module:usize,name:*mut u16,size:u32)->u32;
    fn MultiByteToWideChar(page:u32,flags:u32,input:*const u8,length:i32,output:*mut u16,capacity:i32)->i32;
    fn GetPrivateProfileStringW(section:*const u16,key:*const u16,default:*const u16,value:*mut u16,size:u32,file:*const u16)->u32;
}
struct Process(*mut c_void);
impl Drop for Process {fn drop(&mut self){unsafe{CloseHandleRaw(self.0);}}}
impl Process {
    fn open(pid:u32,image:&str)->Option<Self>{
        if !super::player_process::image_is(pid,image){return None;}
        let handle=unsafe{OpenProcess(0x410,0,pid)};(!handle.is_null()).then_some(Self(handle))
    }
    fn read<const N:usize>(&self,address:usize)->Option<[u8;N]>{
        let mut bytes=[0;N];let mut count=0;
        (unsafe{ReadProcessMemory(self.0,address,bytes.as_mut_ptr().cast(),N,&mut count)}!=0 && count==N).then_some(bytes)
    }
    fn number(&self,address:usize)->Option<u32>{Some(u32::from_le_bytes(self.read(address)?))}
    fn string(&self,address:usize)->Option<String>{
        let length=self.number(address+16)? as usize;let capacity=self.number(address+20)? as usize;
        if length>4096 || length>capacity{return None;}
        let start=if capacity>15{self.number(address)? as usize}else{address};let mut bytes=Vec::with_capacity(length);
        for i in 0..length{bytes.push(self.read::<1>(start+i)?[0]);}decode(&bytes)
    }
    fn module(&self,base:usize,stamp:u32,size:u32)->bool{
        let Some(pe)=self.number(base+60).filter(|offset|*offset<4096)else{return false;};
        self.number(base+pe as usize+8)==Some(stamp) && self.number(base+pe as usize+80)==Some(size)
    }
    fn named_module(&self,image:&str)->Option<usize>{
        let mut modules=[0usize;512];let mut needed=0;
        if unsafe{K32EnumProcessModulesEx(self.0,modules.as_mut_ptr(),std::mem::size_of_val(&modules) as u32,&mut needed,3)}==0{return None;}
        modules.into_iter().take((needed as usize/std::mem::size_of::<usize>()).min(512)).find(|base|{
            let mut name=[0u16;260];let n=unsafe{K32GetModuleBaseNameW(self.0,*base,name.as_mut_ptr(),260)} as usize;
            String::from_utf16_lossy(&name[..n]).eq_ignore_ascii_case(image)
        })
    }
}
fn decode(bytes:&[u8])->Option<String>{
    if let Ok(text)=std::str::from_utf8(bytes){return Some(text.trim_start_matches('\u{feff}').into());}
    let length=unsafe{MultiByteToWideChar(936,8,bytes.as_ptr(),bytes.len().try_into().ok()?,std::ptr::null_mut(),0)};
    if length<=0{return None;}let mut text=vec![0u16;length as usize];
    if unsafe{MultiByteToWideChar(936,8,bytes.as_ptr(),bytes.len() as i32,text.as_mut_ptr(),length)}!=length{return None;}
    String::from_utf16(&text).ok()
}
fn main_window()->Option<(windows::Win32::Foundation::HWND,u32)>{
    unsafe {
        let window=FindWindowW(w!("kwmusicmaindlg"),None).ok()?;let mut pid=0;GetWindowThreadProcessId(window,Some(&mut pid));
        super::player_process::image_is(pid,"kwmusic.exe").then_some((window,pid))
    }
}
fn service(parent:u32)->Option<u32>{
    unsafe {
        let snapshot=CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS,0).ok()?;
        let mut entry=PROCESSENTRY32W{dwSize:std::mem::size_of::<PROCESSENTRY32W>() as u32,..Default::default()};let mut found=None;
        let mut more=Process32FirstW(snapshot,&mut entry).is_ok();
        while more {
            let len=entry.szExeFile.iter().position(|c|*c==0).unwrap_or(entry.szExeFile.len());
            if entry.th32ParentProcessID==parent && String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case("KwService.exe"){found=Some(entry.th32ProcessID);break;}
            more=Process32NextW(snapshot,&mut entry).is_ok();
        }
        let _=CloseHandle(snapshot);found
    }
}
#[derive(Clone,Copy)]
struct Target {main:u32,pid:u32,core:usize,player:usize,input:usize,output:usize}
impl Target {
    fn valid(&self,process:&Process)->bool{
        process.module(self.core,0x6a1fa2a8,0x2f000) && process.module(self.player,0x6a1fa2a2,0x4f000)
            && process.module(self.input,0x6a1fa1c8,0xd000) && process.module(self.output,0x6a1fa203,0x1e000)
    }
    fn discover()->Option<Self>{
        let (_,main)=main_window()?;let pid=service(main)?;let process=Process::open(pid,"KwService.exe")?;
        let mut modules=[0usize;512];let mut needed=0;
        if unsafe{K32EnumProcessModulesEx(process.0,modules.as_mut_ptr(),std::mem::size_of_val(&modules) as u32,&mut needed,3)}==0{return None;}
        let mut target=Self{main,pid,core:0,player:0,input:0,output:0};
        for base in modules.into_iter().take((needed as usize/8).min(512)) {
            let mut name=[0u16;260];let n=unsafe{K32GetModuleBaseNameW(process.0,base,name.as_mut_ptr(),260)} as usize;
            match String::from_utf16_lossy(&name[..n]).to_ascii_lowercase().as_str(){
                "playercore.dll"=>target.core=base,"ckuwoplayer.dll"=>target.player=base,"in_ffaudio.dll"=>target.input=base,"out_kw_ds.dll"=>target.output=base,_=>{}
            }
        }
        target.valid(&process).then_some(target)
    }
    fn snapshot(&self,process:&Process)->Option<NowPlaying>{
        let core=self.core+0x25750;
        if process.number(core)? as usize!=self.core+0x1e3c8 || process.number(core+0x188)?!=1{return None;}
        let player=process.number(self.player+0x27964)? as usize;
        if process.number(player+0x4c)? as usize!=self.input+0xa048 || process.number(self.input+0xa0dc)? as usize!=self.output+0x1a018{return None;}
        let rid=process.string(core+0x84)?;let title=process.string(core+0xb0)?;let artist=process.string(core+0xc8)?;
        let duration=process.number(core+0xa8)? as f64;
        let decoder=process.number(self.input+0xa4f4)? as usize;let paused=process.read::<1>(decoder+0x40)?[0];
        // 输出插件保留真实位置；服务的普通进度字段在暂停时会被置零。
        let bytes=process.number(self.output+0x1a898)?;let rate=process.number(self.output+0x1a8c8)?;
        let offset=process.number(self.output+0x1a8a4)? as i32;
        if process.string(core+0x84)?!=rid{return None;}
        media(&rid,&title,&artist,duration,bytes,rate,offset,paused)
    }
}
fn media(rid:&str,title:&str,artist:&str,duration:f64,bytes:u32,rate:u32,offset:i32,paused:u8)->Option<NowPlaying>{
    let id=rid.strip_prefix("MUSIC_")?.parse::<u64>().ok().filter(|id|*id>0)?;
    if title.trim().is_empty() || title.len()>4096 || artist.len()>4096 || rate==0 || rate>4_000_000 || paused>1{return None;}
    let position=(bytes as f64*1000.0/rate as f64+offset as f64)/1000.0;
    if !duration.is_finite() || duration<=0.0 || duration>=86400.0 || !(0.0..=duration+0.5).contains(&position){return None;}
    Some(NowPlaying{source:"kwmusic.exe".into(),title:title.into(),artist:artist.into(),song_id:Some(id),duration,position:position.min(duration),
        playing:paused==0,sampled_at:Some(Instant::now()),playback_type:MediaKind::Music,..Default::default()})
}
fn cover_url(value:&str)->Option<String>{
    if value.len()>4096{return None;}let url=url::Url::parse(value).ok()?;
    if !matches!(url.scheme(),"http"|"https") || !url.host_str()?.ends_with(".kuwo.cn"){return None;}
    Some(url.into())
}
fn fetch_cover(id:&str)->Option<Arc<eframe::egui::ColorImage>>{
    let id=id.parse::<u64>().ok().filter(|id|*id>0)?;let agent=ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(5)).build();
    let mut response=String::new();agent.get(&format!("https://artistpicserver.kuwo.cn/pic.web?type=rid_pic&pictype=url&size=300&rid={id}"))
        .call().ok()?.into_reader().take(4097).read_to_string(&mut response).ok()?;
    let url=cover_url(response.trim())?;let mut bytes=Vec::new();
    agent.get(&url).call().ok()?.into_reader().take(2*1024*1024+1).read_to_end(&mut bytes).ok()?;
    if bytes.len()>2*1024*1024{return None;}super::decode_artwork(&bytes).map(Arc::new)
}
#[derive(Default)]
pub(super) struct Reader {target:Option<Target>,discovered:Option<Instant>,cover:super::netease_playback::CoverCache}
impl Reader {
    pub fn poll(&mut self)->Option<NowPlaying>{
        if self.target.is_none() && self.discovered.is_none_or(|at|at.elapsed()>=Duration::from_secs(3)) {
            self.discovered=Some(Instant::now());self.target=Target::discover();
        }
        let target=self.target?;
        if !super::player_process::image_is(target.main,"kwmusic.exe"){self.target=None;return None;}
        let Some(process)=Process::open(target.pid,"KwService.exe").filter(|process|target.valid(process))else{self.target=None;return None;};
        let mut media=target.snapshot(&process)?;
        media.artwork=self.cover.get_with(&media.song_id?.to_string(),fetch_cover);Some(media)
    }
}
fn lyric_directory()->Option<PathBuf>{
    use std::os::windows::ffi::OsStrExt;
    let file=PathBuf::from(std::env::var_os("PROGRAMDATA")?).join("kuwodata/kwmusic2013/Conf/user/config.ini");
    let file=file.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();let mut path=[0u16;32768];
    let len=unsafe{GetPrivateProfileStringW(w!("lyric").as_ptr(),w!("LrcSavePath").as_ptr(),w!("").as_ptr(),path.as_mut_ptr(),path.len() as u32,file.as_ptr())} as usize;
    (len>0 && len<path.len()-1).then(||PathBuf::from(String::from_utf16_lossy(&path[..len])))
}
fn parse_lyrics(bytes:&[u8])->Option<crate::lyrics::Lyrics>{
    if bytes.len()>1024*1024{return None;}let raw=decode(bytes)?;let mut lyrics=crate::lyrics::parse_lrc(&raw);
    let offset=raw.lines().find_map(|line|line.strip_prefix("[offset:")?.strip_suffix(']')?.parse::<i32>().ok()).unwrap_or_default() as f32/1000.0;
    for line in &mut lyrics.lines{line.t=(line.t+offset).max(0.0);}
    lyrics.synced.then_some(lyrics)
}
pub(crate) fn read_lyrics(title:&str,artist:&str)->Option<crate::lyrics::Lyrics>{
    if title.is_empty() || artist.is_empty() || title.contains(['\\','/',':','*','?','"','<','>','|']) || artist.contains(['\\','/',':','*','?','"','<','>','|']){return None;}
    let file=std::fs::File::open(lyric_directory()?.join(format!("{artist}-{title}.lrc"))).ok()?;
    if file.metadata().ok()?.len()>1024*1024{return None;}
    let mut bytes=Vec::new();file.take(1024*1024+1).read_to_end(&mut bytes).ok()?;parse_lyrics(&bytes)
}
struct DpiContext(DPI_AWARENESS_CONTEXT);
impl Drop for DpiContext{fn drop(&mut self){if !self.0.0.is_null(){unsafe{SetThreadDpiAwarenessContext(self.0);}}}}

fn find_control(process:&Process,object:usize,name:&str,modules:[usize;2],visited:&mut Vec<usize>,depth:usize)->Option<(usize,RECT)>{
    if depth>25 || visited.len()>=600 || visited.contains(&object){return None;}visited.push(object);
    let table=process.number(object)? as usize;
    if !(modules[0]..modules[0]+0x104000).contains(&table) && !(modules[1]..modules[1]+0x5e7000).contains(&table){return None;}
    if process.read::<2>(object+0x8c)?.contains(&0){return None;}
    let rect=RECT{left:process.number(object+0x4c)? as i32,top:process.number(object+0x50)? as i32,right:process.number(object+0x54)? as i32,bottom:process.number(object+0x58)? as i32};
    if rect.right<=rect.left || rect.bottom<=rect.top{return None;}
    let length=process.number(object+0x40)? as usize;let capacity=process.number(object+0x44)? as usize;
    if length>100 || length>capacity{return None;}
    let start=if capacity>7{process.number(object+0x30)? as usize}else{object+0x30};
    let text=(0..length).map(|i|Some(u16::from_le_bytes(process.read(start+i*2)?))).collect::<Option<Vec<_>>>()?;
    if String::from_utf16(&text).ok()?.eq(name){return Some((object,rect));}
    let locator=process.number(table.checked_sub(4)?)? as usize;let descriptor=process.number(locator+12)? as usize;
    let kind=process.read::<96>(descriptor+8)?;let kind=std::str::from_utf8(kind.split(|b|*b==0).next()?).ok()?;
    if !["LayoutUI@","ContainerUI@","ListUI@"].iter().any(|part|kind.contains(part)){return None;}
    let count=process.number(object+0x288)? as usize;let items=process.number(object+0x284)? as usize;
    if count>256{return None;}
    for i in 0..count{if let Some(found)=find_control(process,process.number(items+i*4)? as usize,name,modules,visited,depth+1){return Some(found);}}
    None
}
fn control_bounds(window:HWND,pid:u32,name:&str)->Option<(Process,usize,RECT)>{
    let process=Process::open(pid,"kwmusic.exe")?;
    let dui=process.named_module("DuiLib.dll")?;let player=process.named_module("KwMusicDLL.dll")?;
    if !process.module(dui,0x6a1fa1d4,0x104000) || !process.module(player,0x6a1fa2d3,0x5e7000){return None;}
    unsafe {
        let object=usize::try_from(GetWindowLongPtrW(window,GWLP_USERDATA)).ok()?;
        if process.number(object+0x68)? as usize!=window.0 as usize{return None;}
        let root=process.number(object+0x198)? as usize;
        let (control,rect)=find_control(&process,root,name,[dui,player],&mut Vec::new(),0)?;
        Some((process,control,rect))
    }
}
fn click(window:HWND,rect:RECT,x:i32)->Option<()>{
    unsafe {
        let _dpi=DpiContext(SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2));
        let mut client=RECT::default();GetClientRect(window,&mut client).ok()?;
        if rect.left<0 || rect.top<0 || (!IsIconic(window).as_bool() && (rect.right>client.right || rect.bottom>client.bottom)){return None;}
        if x<rect.left || x>=rect.right{return None;}let y=rect.top+(rect.bottom-rect.top)/2;
        let point=LPARAM(((y as u32&0xffff)<<16 | (x as u32&0xffff)) as isize);
        // 读取原生控件的位置，只向客户端投递点击，不移动鼠标或激活窗口。
        let send=|message,buttons|SendMessageTimeoutW(window,message,WPARAM(buttons),point,SMTO_ABORTIFHUNG|SMTO_ERRORONEXIT,500,None).0!=0;
        if !send(WM_MOUSEMOVE,0){return None;}
        let down=send(WM_LBUTTONDOWN,1);let up=send(WM_LBUTTONUP,0);(down&&up).then_some(())
    }
}
fn click_button(window:HWND,pid:u32,name:&str)->Option<()>{
    let (_,_,rect)=control_bounds(window,pid,name)?;click(window,rect,rect.left+(rect.right-rect.left)/2)
}
fn seek_point(rect:RECT,seconds:f64,maximum:u32,left:u32,right:u32)->Option<i32>{
    if !seconds.is_finite() || maximum==0 || maximum>=86400 || seconds<0.0 || seconds>maximum as f64
        || left>100 || right>100 || rect.right as f64-rect.left as f64<=left as f64+right as f64+10.0 || rect.bottom<=rect.top{return None;}
    let width=rect.right as f64-rect.left as f64-left as f64-right as f64;
    Some((rect.left as f64+left as f64+width*seconds/maximum as f64).round().min((rect.right-1) as f64) as i32)
}
pub(super) fn seek(seconds:f64)->bool{
    (||->Option<()> {
        if !seconds.is_finite() || seconds<0.0{return None;}
        let target=Target::discover()?;let service=Process::open(target.pid,"KwService.exe")?;
        let before=target.snapshot(&service)?;let seconds=seconds.min(before.duration);
        let (window,pid)=main_window()?;if pid!=target.main{return None;}
        let (process,control,rect)=control_bounds(window,pid,"PlayerProgress")?;
        let x=seek_point(rect,seconds,process.number(control+0x304)?,process.number(control+0x328)?,process.number(control+0x330)?)?;
        click(window,rect,x)?;
        let start=Instant::now();let mut pause_requested=false;
        while start.elapsed()<Duration::from_secs(2){
            std::thread::sleep(Duration::from_millis(100));
            let Some(current)=target.snapshot(&service)else{continue;};
            if current.song_id!=before.song_id{return None;}
            if !super::seek_arrived(current.position,seconds,start.elapsed().as_secs_f64()){continue;}
            if current.playing==before.playing{return Some(());}
            if !before.playing && current.playing && !pause_requested {
                if !self::control(super::Transport::Toggle){return None;}pause_requested=true;
            }
        }
        None
    })().is_some()
}
pub(super) fn control(command:super::Transport)->bool{
    let Some((window,pid))=main_window()else{return false;};
    match command{
        super::Transport::Next=>click_button(window,pid,"Next").is_some(),
        super::Transport::Prev=>click_button(window,pid,"Pre").is_some(),
        super::Transport::Toggle=>unsafe{
            let down=PostMessageW(window,WM_KEYDOWN,WPARAM(0x74),LPARAM(0x3f0001)).is_ok();
            let up=PostMessageW(window,WM_KEYUP,WPARAM(0x74),LPARAM(0xc03f0001)).is_ok();down&&up
        },_=>false
    }
}
pub(crate) fn is_service(pid:u32)->bool{main_window().and_then(|(_,main)|service(main))==Some(pid)}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_clock_preserves_paused_position_and_rejects_invalid_samples(){
        let playing=media("MUSIC_3460514","下一个天亮","郭静",29.0,4766460,176400,0,0).unwrap();
        let paused=media("MUSIC_3460514","下一个天亮","郭静",29.0,4766460,176400,0,1).unwrap();
        assert!(playing.playing);assert!(!paused.playing);assert!((paused.position-27.02).abs()<0.001);assert_eq!(playing.position,paused.position);
        for (rid,duration,rate,offset,flag) in [("MUSIC_0",29.0,176400,0,0),("MUSIC_1",0.0,176400,0,0),("MUSIC_1",29.0,0,0,0),
            ("MUSIC_1",29.0,176400,-50000,0),("MUSIC_1",29.0,176400,0,2)]{assert!(media(rid,"歌","手",duration,4766460,rate,offset,flag).is_none());}
    }
    #[test]
    fn lyrics_accept_utf8_and_native_gbk_and_apply_offset(){
        let raw=b"[ver:v1.0]\r\n[offset:200]\r\n[00:01.000]\xb2\xbb\xbd\xab\xbe\xcd";
        let lyrics=parse_lyrics(raw).unwrap();assert_eq!(lyrics.lines[0].text,"不将就");assert!((lyrics.lines[0].t-1.2).abs()<0.001);
        assert!(parse_lyrics("[00:01.000]中文歌词".as_bytes()).unwrap().synced);assert!(parse_lyrics(b"invalid").is_none());
        for url in ["file:///cover.jpg","https://kuwo.cn.example.org/cover","https://example.org/cover"]{assert!(cover_url(url).is_none());}
        assert_eq!(cover_url("http://img1.kwcdn.kuwo.cn/cover.jpg").as_deref(),Some("http://img1.kwcdn.kuwo.cn/cover.jpg"));
    }
    #[test]
    fn seek_coordinates_use_native_duration_and_insets(){
        let rect=RECT{left:448,top:962,right:1053,bottom:990};
        assert_eq!(seek_point(rect,0.0,143,3,6),Some(451));
        assert_eq!(seek_point(rect,71.5,143,3,6),Some(749));
        assert_eq!(seek_point(rect,143.0,143,3,6),Some(1047));
        assert_eq!(seek_point(rect,14.0,240,3,6),Some(486),"试听应使用客户端的完整时长定位");
        for (seconds,total,left,right) in [(f64::NAN,143,3,6),(-1.0,143,3,6),(144.0,143,3,6),(1.0,0,3,6),(1.0,86400,3,6),(1.0,143,1000,6)]{
            assert!(seek_point(rect,seconds,total,left,right).is_none());
        }
        assert!(seek_point(RECT{right:455,..rect},1.0,143,3,6).is_none());
    }
    #[test]
    #[ignore="需要本机酷我正在播放；验证前后跳转、暂停状态并恢复原进度"]
    fn live_kuwo_seek_changes_position(){
        let window=main_window().unwrap().0;let minimized=unsafe{IsIconic(window)};
        let foreground=unsafe{GetForegroundWindow()};
        let mut reader=Reader::default();let first=reader.poll().expect("未读取到酷我播放状态");
        assert!(first.playing && first.duration>=20.0);
        let mut observations=Vec::new();
        for target in [first.duration*0.23,first.duration*0.67]{
            let accepted=super::super::seek(target,"kwmusic.exe");observations.push((target,accepted,reader.poll()));
        }
        assert!(control(super::super::Transport::Toggle));std::thread::sleep(Duration::from_millis(400));
        let was_paused=reader.poll().is_some_and(|m|!m.playing);
        let paused_target=first.duration*0.4;let paused_seek=super::super::seek(paused_target,"kwmusic.exe");let paused=reader.poll();
        if paused.as_ref().is_some_and(|m|!m.playing){let _=control(super::super::Transport::Toggle);std::thread::sleep(Duration::from_millis(300));}
        let restored=super::super::seek(first.position,"kwmusic.exe");
        for (target,accepted,after) in observations{
            assert!(accepted,"酷我未完成实际跳转");let after=after.unwrap();
            assert_eq!(first.song_id,after.song_id);assert!(after.playing);assert!((after.position-target).abs()<3.0);
            println!("酷我：{} 实际进度 {:.2} -> {:.2}，目标 {:.2}",first.title,first.position,after.position,target);
        }
        let paused=paused.unwrap();assert!(was_paused && paused_seek && !paused.playing);
        assert_eq!(paused.song_id,first.song_id);assert!((paused.position-paused_target).abs()<3.0);
        assert!(restored,"应恢复原播放位置");assert_eq!(unsafe{IsIconic(window)},minimized);assert_eq!(unsafe{GetForegroundWindow()},foreground);
        println!("暂停时跳转 {:.2}，保持暂停；已恢复原位置和播放状态，最小化={}，前台未改变",paused.position,minimized.as_bool());
    }
    #[test]
    #[ignore="需要本机酷我正在播放；短暂暂停后恢复，验证暂停位置和前台窗口"]
    fn live_kuwo_pause_retains_position_and_focus(){
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let mut reader=Reader::default();let before=reader.poll().expect("未读取到酷我播放状态");assert!(before.playing);
        assert!(crate::music_volume::read("kwmusic.exe").is_some(),"应读取酷我音频服务的应用音量");
        let foreground=unsafe{GetForegroundWindow()};assert!(control(super::super::Transport::Toggle));
        std::thread::sleep(Duration::from_millis(600));let paused=reader.poll().unwrap();
        std::thread::sleep(Duration::from_millis(600));let later=reader.poll().unwrap();
        let resumed=control(super::super::Transport::Toggle);
        assert!(!paused.playing && !later.playing && resumed);assert_eq!(before.song_id,paused.song_id);
        assert!((later.position-paused.position).abs()<0.01);assert!(paused.position>=before.position);
        assert_eq!(unsafe{GetForegroundWindow()},foreground);
        println!("酷我：{} 暂停位置 {:.2}s，恢复请求已发送",paused.title,paused.position);
    }
}
