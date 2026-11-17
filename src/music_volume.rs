//! Windows 音量混合器中的播放器音量；只操作匹配的应用会话。
use std::{sync::{mpsc,Arc,Mutex},time::{Duration, Instant}};

type Job=Box<dyn FnOnce()+Send>;

/// 音量混合器的读写都放到这一个独立线程里。枚举音频设备在个别声卡、蓝牙或虚拟声卡驱动上
/// 可能长时间不返回，不能拖住歌曲信息采样（否则岛上一直「暂无音乐」）或播放控制。
/// 线程卡住时队列很快填满，之后的音量操作直接放弃，不再新开线程。
fn submit(job:Job)->bool {
    static WORKER:Mutex<Option<mpsc::SyncSender<Job>>>=Mutex::new(None);
    let mut worker=WORKER.lock().unwrap_or_else(|e|e.into_inner());
    if worker.is_none(){
        let (tx,rx)=mpsc::sync_channel::<Job>(2);
        if crate::resources::VOLUME.spawn(move||{while let Ok(job)=rx.recv(){job();}}).is_none(){return false;}
        *worker=Some(tx);
    }
    worker.as_ref().is_some_and(|tx|tx.try_send(job).is_ok())
}

/// 在音量线程上执行并最多等 2 秒；超时按失败处理。
fn run(job:impl FnOnce(Instant)->bool+Send+'static)->bool {
    let (tx,rx)=mpsc::sync_channel(1);
    let deadline=Instant::now()+Duration::from_secs(2);
    if !submit(Box::new(move||{let _=tx.send(Instant::now()<deadline && job(deadline));})){return false;}
    rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap_or(false)
}

type Latest=Arc<Mutex<Option<(String,Option<(u32,bool)>)>>>;

/// 采样线程用：只取音量线程上次读到的结果，从不等待。
#[derive(Default)]
pub(crate) struct Reader {
    source: String,
    at: Option<Instant>,
    latest: Latest,
}
impl Reader {
    pub fn poll(&mut self, source:&str)->Option<(u32,bool)> {
        if source.is_empty(){self.source.clear();self.at=None;return None;}
        if self.source!=source || self.at.is_none_or(|at|at.elapsed()>=Duration::from_millis(500)) {
            self.source=source.into();self.at=Some(Instant::now());
            let (latest,source)=(self.latest.clone(),source.to_string());
            submit(Box::new(move||{let value=read(&source);*latest.lock().unwrap_or_else(|e|e.into_inner())=Some((source,value));}));
        }
        self.latest.lock().unwrap_or_else(|e|e.into_inner()).as_ref().filter(|(read,_)|read==source).and_then(|(_,value)|*value)
    }
}

fn executable(source:&str)->Option<String> {
    let source=source.to_lowercase();
    match source.as_str() {
        "kugou"=>Some("kugou.exe".into()),
        // QQ 音乐 22.82 起系统媒体会话的身份名改为 Tencent.QQMusic。
        "qqmusic"|"tencent.qqmusic"=>Some("qqmusic.exe".into()),
        "spotify"=>Some("spotify.exe".into()),
        "汽水音乐"=>Some("sodamusic.exe".into()),
        _ if source.ends_with(".exe") && !source.contains(['\\','/'])=>Some(source),
        _=>None,
    }
}

#[cfg(windows)]
mod platform {
    use windows::{core::{Interface,PWSTR},Win32::{
        Foundation::CloseHandle,
        Media::Audio::{AudioSessionStateExpired,IAudioSessionControl2,IAudioSessionManager2,IMMDeviceEnumerator,ISimpleAudioVolume,MMDeviceEnumerator,DEVICE_STATE_ACTIVE,eRender},
        Storage::Packaging::Appx::GetApplicationUserModelId,
        System::{Com::{CoCreateInstance,CoInitializeEx,CLSCTX_ALL,COINIT_MULTITHREADED},Threading::{OpenProcess,PROCESS_QUERY_LIMITED_INFORMATION}},
    }};

    fn matches(pid:u32,source:&str)->bool {
        if source.eq_ignore_ascii_case("kwmusic.exe") && crate::smtc::player_process::image_is(pid,"KwService.exe"){return crate::smtc::kuwo::is_service(pid);}
        if let Some(exe)=super::executable(source){return crate::smtc::player_process::image_is(pid,&exe);}
        if pid==0 || !source.contains('!'){return false;}
        unsafe {
            let Ok(process)=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid) else{return false;};
            let mut id=[0u16;512];let mut length=id.len() as u32;
            let result=GetApplicationUserModelId(process,&mut length,PWSTR(id.as_mut_ptr()));
            let _=CloseHandle(process);
            result.is_ok() && length>0 && (length as usize)<=id.len()
                && String::from_utf16_lossy(&id[..length as usize-1]).eq_ignore_ascii_case(source)
        }
    }

    pub fn sessions(source:&str)->Vec<ISimpleAudioVolume> {
        if source.is_empty(){return Vec::new();}
        unsafe {
            // 采样、控制各用自己的 COM 线程与枚举器，不跨线程传递音频接口。
            thread_local!{static ENUMERATOR:Option<IMMDeviceEnumerator>=unsafe{
                let _=CoInitializeEx(None,COINIT_MULTITHREADED);
                CoCreateInstance(&MMDeviceEnumerator,None,CLSCTX_ALL).ok()
            };}
            ENUMERATOR.with(|enumerator|{
                let mut volumes=Vec::new();
                let Some(enumerator)=enumerator else{return volumes;};
                // 应用可能被路由到非默认输出，遍历当前可用的播放设备。
                let Ok(devices)=enumerator.EnumAudioEndpoints(eRender,DEVICE_STATE_ACTIVE) else{return volumes;};
                for d in 0..devices.GetCount().unwrap_or(0) {
                    let Ok(device)=devices.Item(d) else{continue;};
                    let Ok(manager)=device.Activate::<IAudioSessionManager2>(CLSCTX_ALL,None) else{continue;};
                    let Ok(sessions)=manager.GetSessionEnumerator() else{continue;};
                    for s in 0..sessions.GetCount().unwrap_or(0) {
                        let Ok(session)=sessions.GetSession(s) else{continue;};
                        if session.GetState().ok()==Some(AudioSessionStateExpired){continue;}
                        let Ok(control)=session.cast::<IAudioSessionControl2>() else{continue;};
                        if !control.GetProcessId().ok().is_some_and(|pid|matches(pid,source)){continue;}
                        if let Ok(volume)=session.cast::<ISimpleAudioVolume>(){volumes.push(volume);}
                    }
                }
                volumes
            })
        }
    }
}

#[cfg(windows)]
pub fn read(source:&str)->Option<(u32,bool)> {
    platform::sessions(source).iter().filter_map(|session|unsafe {
        Some(((session.GetMasterVolume().ok()?*100.0).round() as u32,session.GetMute().ok()?.as_bool()))
    }).reduce(|a,b|(a.0.max(b.0),a.1 && b.1))
}

#[cfg(windows)]
pub fn set_volume(source:&str,percent:u32)->bool {
    let source=source.to_string();
    run(move|deadline|apply(&source,deadline,|session|unsafe {
        session.SetMasterVolume(percent.min(100) as f32/100.0,std::ptr::null()).is_ok()
            && Instant::now()<deadline && session.SetMute(false,std::ptr::null()).is_ok()
    }))
}

#[cfg(windows)]
pub fn set_mute(source:&str,muted:bool)->bool {
    let source=source.to_string();
    run(move|deadline|apply(&source,deadline,|session|unsafe{session.SetMute(muted,std::ptr::null()).is_ok()}))
}

#[cfg(windows)]
fn apply(source:&str,deadline:Instant,change:impl Fn(&windows::Win32::Media::Audio::ISimpleAudioVolume)->bool)->bool {
    let sessions=platform::sessions(source);
    let mut success=!sessions.is_empty();
    for session in &sessions{
        if Instant::now()>=deadline{return false;}
        success=change(session) && success;
    }
    success
}

#[cfg(not(windows))]
pub fn read(_source:&str)->Option<(u32,bool)>{None}
#[cfg(not(windows))]
pub fn set_volume(_source:&str,_percent:u32)->bool{false}
#[cfg(not(windows))]
pub fn set_mute(_source:&str,_muted:bool)->bool{false}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn player_names_require_an_exact_executable_or_packaged_identity(){
        for (source,exe) in [("QQMusic.exe","qqmusic.exe"),("Tencent.QQMusic","qqmusic.exe"),("kugou","kugou.exe"),("汽水音乐","sodamusic.exe")]{assert_eq!(executable(source).as_deref(),Some(exe));}
        for source in ["","music","QQMusic.exe.bak",r"C:\QQMusic.exe","SpotifyAB.SpotifyMusic_123!Spotify"]{assert_eq!(executable(source),None);}
    }
    #[test]
    fn stuck_audio_driver_does_not_block_song_sampling(){
        let (release,wait)=mpsc::channel::<()>();let (entered,started)=mpsc::channel();
        assert!(submit(Box::new(move||{let _=entered.send(());let _=wait.recv_timeout(Duration::from_secs(10));})));
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut reader=Reader::default();let start=Instant::now();
        for i in 0..20 {assert_eq!(reader.poll(&format!("daozai-stuck-{i}.exe")),None);}
        assert!(start.elapsed()<Duration::from_millis(200),"音量线程卡住时，采样线程不能跟着等待");
        drop(release);
    }

    #[test]
    fn expired_write_is_discarded_when_the_worker_resumes(){
        let (release,wait)=mpsc::channel::<()>();
        assert!(submit(Box::new(move||{let _=wait.recv_timeout(Duration::from_secs(10));})));
        let (ran,observed)=mpsc::channel();
        assert!(!run(move|_|{let _=ran.send(());true}));
        drop(release);
        assert_eq!(observed.recv_timeout(Duration::from_secs(3)),Err(mpsc::RecvTimeoutError::Disconnected));
    }

    #[test]
    fn unknown_player_cannot_change_system_volume(){
        assert_eq!(read("daozai-missing-player.exe"),None);
        assert!(!set_volume("daozai-missing-player.exe",100));
        assert!(!set_mute("",true));
    }

    #[test]
    #[cfg(windows)]
    #[ignore="需要本机音频输出设备；创建无声会话验证 Windows 混音器读写"]
    fn live_windows_sessions_change_together_without_changing_master_volume(){
        use windows::Win32::{Media::Audio::{IAudioClient,IMMDeviceEnumerator,ISimpleAudioVolume,MMDeviceEnumerator,AUDCLNT_SHAREMODE_SHARED,eRender,eConsole},System::Com::{CoCreateInstance,CoCreateGuid,CoInitializeEx,CoTaskMemFree,CLSCTX_ALL,COINIT_MULTITHREADED}};
        unsafe {
            let _=CoInitializeEx(None,COINIT_MULTITHREADED);
            let enumerator:IMMDeviceEnumerator=CoCreateInstance(&MMDeviceEnumerator,None,CLSCTX_ALL).unwrap();
            let device=enumerator.GetDefaultAudioEndpoint(eRender,eConsole).unwrap();
            let create=||{
                let client:IAudioClient=device.Activate(CLSCTX_ALL,None).unwrap();
                let format=client.GetMixFormat().unwrap();let guid=CoCreateGuid().unwrap();
                let result=client.Initialize(AUDCLNT_SHAREMODE_SHARED,0,1_000_000,0,format,Some(&guid));
                CoTaskMemFree(Some(format as _));result.unwrap();
                let volume:ISimpleAudioVolume=client.GetService().unwrap();(client,volume)
            };
            let (_first,a)=create();let (_second,b)=create();
            let source=std::env::current_exe().unwrap().file_name().unwrap().to_str().unwrap().to_owned();
            a.SetMasterVolume(0.31,std::ptr::null()).unwrap();b.SetMasterVolume(0.52,std::ptr::null()).unwrap();
            let before=crate::system::Snapshot::read().volume;
            assert_eq!(read(&source),Some((52,false)));
            assert!(set_volume(&source,27));
            assert_eq!(read(&source),Some((27,false)));
            assert!((a.GetMasterVolume().unwrap()-0.27).abs()<0.001);
            assert!((b.GetMasterVolume().unwrap()-0.27).abs()<0.001);
            assert!(set_mute(&source,true));assert!(a.GetMute().unwrap().as_bool() && b.GetMute().unwrap().as_bool());
            assert!(set_volume(&source,19));assert_eq!(read(&source),Some((19,false)));
            assert!(!set_volume("daozai-missing-player.exe",100));assert_eq!(read(&source),Some((19,false)));
            assert!(set_volume(&source,999));assert_eq!(read(&source),Some((100,false)));
            assert_eq!(crate::system::Snapshot::read().volume,before,"应用音量不能改变系统主音量或静音");
            println!("双会话音量、静音、解除静音、边界和系统音量隔离验证通过");
        }
    }
}
