//! Windows 状态采样与经用户授权的通知读取。
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{self, Receiver};
use windows::core::{Result, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetLastInputInfo, LASTINPUTINFO,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{NotificationKinds, UserNotification};

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub volume: Option<(u32, bool)>,
    pub audio_endpoint: Option<String>,
    pub caps: bool,
    pub network: bool,
    pub network_kind: NetworkKind,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Observation {
    pub system: Snapshot,
    pub input: Option<crate::input::State>,
    pub fullscreen: Option<crate::display_layout::Bounds>,
}

/// 跨进程输入法查询和 COM 状态采样留在后台，只在状态改变时唤醒界面。
pub fn observe(ctx:eframe::egui::Context,initial:Snapshot,input:Option<crate::input::State>)->Receiver<Observation> {
    let(tx,rx)=mpsc::sync_channel(1);
    std::thread::Builder::new().name("daozai-system".into()).spawn(move||{
        let mut current=Observation{system:initial,input,fullscreen:None};
        let mut delivered=None;
        let mut sampled=crate::poll_clock::PollClock::default();
        let mut input_at=sampled;
        loop {
            let identity=crate::input::State::identity();
            let changed=identity.map(|state|(state.context,state.layout))!=current.input.map(|state|(state.context,state.layout));
            if changed || crate::input::switch_recent() || input_at.ready(std::time::Duration::from_secs(1)){
                current.input=crate::input::State::read();input_at=crate::poll_clock::PollClock::now();
            }
            if sampled.ready(std::time::Duration::from_millis(500)){
                current.system=Snapshot::read();current.fullscreen=fullscreen();sampled=crate::poll_clock::PollClock::now();
            }
            if delivered.as_ref()!=Some(&current) {
                match tx.try_send(current.clone()) {
                    Ok(())=>{delivered=Some(current.clone());ctx.request_repaint();},
                    Err(mpsc::TrySendError::Disconnected(_))=>return,
                    Err(mpsc::TrySendError::Full(_))=>{},
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }).ok();
    rx
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NetworkKind { Wifi, Ethernet, #[default] Other }
impl Snapshot {
    pub fn read() -> Self {
        unsafe {
            let device=audio_device().ok();
            let audio_endpoint=device.as_ref().and_then(|d|d.GetId().ok()).and_then(|id|{
                let text=id.to_string().ok();
                windows::Win32::System::Com::CoTaskMemFree(Some(id.0 as _));
                text
            });
            let volume = device.and_then(|d|d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL,None).ok()).and_then(|a| {
                Some((
                    (a.GetMasterVolumeLevelScalar().ok()? * 100.0).round() as u32,
                    a.GetMute().ok()?.as_bool(),
                ))
            });
            let profile=windows::Networking::Connectivity::NetworkInformation::GetInternetConnectionProfile().ok();
            let network=profile.as_ref().and_then(|p|p.GetNetworkConnectivityLevel().ok()).is_some_and(|v|v==windows::Networking::Connectivity::NetworkConnectivityLevel::InternetAccess);
            let network_kind=profile.as_ref().map(|p| {
                if p.IsWlanConnectionProfile().unwrap_or(false){NetworkKind::Wifi}
                else if p.NetworkAdapter().ok().and_then(|a|a.IanaInterfaceType().ok())==Some(6){NetworkKind::Ethernet}
                else{NetworkKind::Other}
            }).unwrap_or_default();
            Self{volume,audio_endpoint,network,network_kind,caps:GetKeyState(0x14)&1!=0}
        }
    }
    pub fn changes(&self, old: &Self) -> Vec<(String, String)> {
        let mut out = vec![];
        if self.audio_endpoint == old.audio_endpoint && self.volume != old.volume {
            if let Some((v, mute)) = self.volume {
                out.push((
                    "系统音量".into(),
                    if mute {
                        "已静音".into()
                    } else {
                        format!("音量 {v}%")
                    },
                ));
            }
        }
        if self.caps != old.caps {
            out.push((
                "大写锁定".into(),
                if self.caps { "已开启" } else { "已关闭" }.into(),
            ));
        }
        if self.network != old.network || self.network && self.network_kind != old.network_kind {
            let kind=if self.network {self.network_kind}else{old.network_kind};
            let status=if self.network {"网络已连接"}else{"网络已断开"};
            out.push((
                "网络连接".into(),
                match kind {NetworkKind::Wifi=>format!("Wi-Fi · {status}"),NetworkKind::Ethernet=>format!("以太网 · {status}"),NetworkKind::Other=>status.into()},
            ));
        }
        out
    }
}
unsafe fn audio_device() -> Result<IMMDevice> {    thread_local! {static INITIALIZED:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
    INITIALIZED.with(|v| {
        if !v.replace(true) {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
    });
    // 每个采样线程复用枚举器，但每次仍查询当前默认设备，保留热插拔响应。
    thread_local! {static ENUMERATOR:std::cell::RefCell<Option<IMMDeviceEnumerator>>=const{std::cell::RefCell::new(None)};}
    ENUMERATOR.with(|slot| {
        let mut slot=slot.borrow_mut();
        if slot.is_none(){*slot=Some(CoCreateInstance(&MMDeviceEnumerator,None,CLSCTX_ALL)?);}
        let result=slot.as_ref().unwrap().GetDefaultAudioEndpoint(eRender,eConsole);
        if result.is_err(){*slot=None;}
        result
    })
}
unsafe fn audio() -> Result<IAudioEndpointVolume> {audio_device()?.Activate(CLSCTX_ALL,None)}
/// 设置主音量（0..100）；同时解除静音，拖动音量条时立刻能听到。
#[cfg(windows)]
pub fn set_volume(percent: u32) {
    unsafe {
        if let Ok(a) = audio() {
            let _ = a.SetMasterVolumeLevelScalar((percent.min(100) as f32) / 100.0, std::ptr::null());
            let _ = a.SetMute(false, std::ptr::null());
        }
    }
}

/// 切换静音，返回切换后的静音状态（拿不到设备时返回 None）。
#[cfg(windows)]
pub fn toggle_mute() -> Option<bool> {
    unsafe {
        let a = audio().ok()?;
        let muted = a.GetMute().ok()?.as_bool();
        a.SetMute(!muted, std::ptr::null()).ok()?;
        Some(!muted)
    }
}

/// 当前会话是否锁屏。查询走 WTS（一次本机 RPC），按 2 秒缓存，避免每帧都查。
pub fn screen_locked() -> bool {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use windows::Win32::System::RemoteDesktop::{
        WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW, WTS_CURRENT_SERVER_HANDLE,
        WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK,
    };
    static CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, locked)) = *cache {
        if at.elapsed() < Duration::from_secs(2) { return locked; }
    }
    let locked = unsafe {
        let mut buf = windows::core::PWSTR::null();
        let mut len = 0u32;
        if WTSQuerySessionInformationW(WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTSSessionInfoEx, &mut buf, &mut len).is_ok() && !buf.is_null() {
            let info = &*(buf.0 as *const WTSINFOEXW);
            let locked = info.Level == 1 && info.Data.WTSInfoExLevel1.SessionFlags == WTS_SESSIONSTATE_LOCK as i32;
            WTSFreeMemory(buf.0 as *mut core::ffi::c_void);
            locked
        } else {
            false
        }
    };
    *cache = Some((Instant::now(), locked));
    locked
}

pub fn idle_seconds() -> f64 {
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            ..Default::default()
        };
        if GetLastInputInfo(&mut info).as_bool() {
            windows::Win32::System::SystemInformation::GetTickCount().wrapping_sub(info.dwTime)
                as f64
                / 1000.0
        } else {
            0.0
        }
    }
}
/// 前台窗口在全屏判定里需要的信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Foreground {
    hwnd: isize,
    pid: u32,
    /// 最终所有者窗口（没有所有者时是它自己）。
    root_owner: isize,
    /// 会在任务栏单独显示按钮的普通程序窗口（不是工具窗口、对话框或浮动面板）。
    app_window: bool,
}

/// 前台从全屏窗口切到别的窗口时，是否仍按原来的全屏处理（岛仔保持隐藏）。
///
/// 3ds Max、Photoshop 这类软件全屏（或最大化后盖满整块屏幕）时，插件面板、浮动工具条、
/// 属性对话框都是同一个程序的附属窗口；每点一下前台就在它们和主窗口之间来回切，
/// 过去岛仔会跟着「出现 → 消失」闪一次。只要原来的全屏窗口还盖着屏幕，
/// 而新的前台是它的附属窗口（被它拥有，或同一程序里不单独占任务栏的窗口），就继续隐藏。
/// 切到另一个程序、或同一程序里另开的普通窗口（如浏览器的新窗口）时照常显示。
fn keeps_fullscreen(last: Foreground, still_covers: bool, now: Option<Foreground>) -> bool {
    if !still_covers { return false; }
    match now {
        // 激活切换的瞬间前台可能是空的，不当成离开全屏。
        None => true,
        Some(now) => now.hwnd == last.hwnd || now.root_owner == last.hwnd || (now.pid == last.pid && !now.app_window),
    }
}

unsafe fn foreground_info(hwnd: HWND) -> Foreground {
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let root = GetAncestor(hwnd, GA_ROOTOWNER);
    let root_owner = if root.0.is_null() { hwnd.0 as isize } else { root.0 as isize };
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let owned = !GetWindow(hwnd, GW_OWNER).unwrap_or_default().0.is_null();
    let app_window = ex & WS_EX_APPWINDOW.0 as isize != 0 || (!owned && ex & WS_EX_TOOLWINDOW.0 as isize == 0);
    Foreground { hwnd: hwnd.0 as isize, pid, root_owner, app_window }
}

/// 窗口盖满它所在的整块屏幕时返回该屏幕范围。
unsafe fn covering_monitor(hwnd: HWND) -> Option<crate::display_layout::Bounds> {
    if !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return None;
    }
    let mut r = RECT::default();
    if GetWindowRect(hwnd, &mut r).is_err() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info).as_bool()
    {
        return None;
    }
    let covers=r.left <= info.rcMonitor.left
        && r.top <= info.rcMonitor.top
        && r.right >= info.rcMonitor.right
        && r.bottom >= info.rcMonitor.bottom;
    covers.then_some(crate::display_layout::Bounds {left:info.rcMonitor.left,top:info.rcMonitor.top,right:info.rcMonitor.right,bottom:info.rcMonitor.bottom})
}

pub fn fullscreen() -> Option<crate::display_layout::Bounds> {
    use std::sync::Mutex;
    // 上一次判定为全屏的窗口：前台切到它的附属窗口时沿用（见 `keeps_fullscreen`）。
    static LAST: Mutex<Option<Foreground>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let hwnd = GetForegroundWindow();
        let now = (!hwnd.0.is_null()).then(|| foreground_info(hwnd));
        if let Some(info) = now {
            // 岛仔自己的窗口（设置窗口等）不算全屏，也不沿用。
            if info.pid == windows::Win32::System::Threading::GetCurrentProcessId() {
                *last = None;
                return None;
            }
            // 桌面与资源管理器桌面窗口不能当成全屏游戏。
            let mut cls = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut cls);
            let cls = String::from_utf16_lossy(&cls[..n.max(0) as usize]);
            if matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
                *last = None;
                return None;
            }
            if let Some(bounds) = covering_monitor(hwnd) {
                *last = Some(info);
                return Some(bounds);
            }
        }
        if let Some(previous) = *last {
            let bounds = covering_monitor(HWND(previous.hwnd as *mut _));
            if keeps_fullscreen(previous, bounds.is_some(), now) {
                return bounds;
            }
        }
        *last = None;
        None
    }
}

#[cfg(test)]
mod fullscreen_follow_tests {
    use super::*;

    const MAX: Foreground = Foreground { hwnd: 100, pid: 7, root_owner: 100, app_window: true };

    #[test]
    fn plugin_panels_and_dialogs_of_the_fullscreen_app_keep_the_island_hidden() {
        // 被主窗口拥有的插件面板 / 对话框
        let owned = Foreground { hwnd: 200, pid: 7, root_owner: 100, app_window: false };
        assert!(keeps_fullscreen(MAX, true, Some(owned)));
        // 同一程序里不占任务栏的浮动工具窗口（没有设所有者）
        let tool = Foreground { hwnd: 201, pid: 7, root_owner: 201, app_window: false };
        assert!(keeps_fullscreen(MAX, true, Some(tool)));
        // 插件放在独立进程里、但窗口挂在主窗口下面
        let other_process = Foreground { hwnd: 202, pid: 9, root_owner: 100, app_window: false };
        assert!(keeps_fullscreen(MAX, true, Some(other_process)));
        // 激活切换瞬间前台为空
        assert!(keeps_fullscreen(MAX, true, None));
    }

    #[test]
    fn switching_to_another_app_or_leaving_fullscreen_shows_the_island() {
        let other_app = Foreground { hwnd: 300, pid: 9, root_owner: 300, app_window: true };
        assert!(!keeps_fullscreen(MAX, true, Some(other_app)), "切到别的程序时照常显示");
        // 同一程序新开的普通窗口（如浏览器的第二个窗口）也照常显示
        let second_window = Foreground { hwnd: 301, pid: 7, root_owner: 301, app_window: true };
        assert!(!keeps_fullscreen(MAX, true, Some(second_window)));
        // 原来的全屏窗口已经还原、最小化或关闭
        let owned = Foreground { hwnd: 200, pid: 7, root_owner: 100, app_window: false };
        assert!(!keeps_fullscreen(MAX, false, Some(owned)));
        assert!(!keeps_fullscreen(MAX, false, None));
    }
}

pub(crate) fn is_browser_process(name:&str)->bool {
    ["chrome.exe","msedge.exe","firefox.exe","brave.exe","vivaldi.exe","opera.exe",
        "arc.exe","zen.exe","chromium.exe","librewolf.exe","floorp.exe","waterfox.exe",
        "duckduckgo.exe","360se.exe","360chrome.exe","qqbrowser.exe","sogouexplorer.exe","quark.exe"]
        .iter().any(|browser|name.eq_ignore_ascii_case(browser))
}

fn browser_overlaps_island(browser:RECT,island:RECT,dock:crate::model::Dock)->bool {
    if dock.is_side() {return window_covers_side(browser,island,dock);}
    // 窗口是 CANVAS_W 宽的透明画布，岛体中心固定在画布左侧 WIN_W/2 处（与水平位置、大小无关）。
    let scale=(island.right-island.left) as f32/crate::app::CANVAS_W;
    let center=island.left as f32+crate::app::WIN_W*scale/2.0;
    let half=crate::app::PW*scale/2.0;
    (browser.left as f32)<center+half && (browser.right as f32)>center-half
        && (browser.top as f32)<island.top as f32+crate::app::BAR_H*scale && browser.bottom>island.top
}

/// 左右停靠：画布是 SIDE_W × SIDE_H，岛体贴在画布的左 / 右边、竖直居中。
/// 前台窗口盖住岛所在的那一段屏幕边缘（例如最大化、全屏或贴边摆放）即视为遮挡。
fn window_covers_side(window:RECT,island:RECT,dock:crate::model::Dock)->bool {
    let scale=(island.right-island.left) as f32/crate::side_dock::SIDE_W;
    let cy=island.top as f32+crate::side_dock::SIDE_H*scale/2.0;
    let half=crate::app::PH*scale/2.0;
    let strip=crate::side_dock::SIDE_BAR_W*scale;
    let vertical=(window.top as f32)<cy+half && (window.bottom as f32)>cy-half;
    let horizontal=if dock==crate::model::Dock::Left {(window.left as f32)<island.left as f32+strip && window.right>island.left}
        else {(window.right as f32)>island.right as f32-strip && window.left<island.right};
    vertical && horizontal
}

pub fn browser_at_top(island:HWND,settings:&crate::model::Settings)->Option<bool> {
    unsafe {
        let foreground=GetForegroundWindow();
        if foreground.0.is_null(){return None;}
        let mut pid=0;
        GetWindowThreadProcessId(foreground,Some(&mut pid));
        // 设置、托盘和岛内输入沿用此前的避让状态，避免自身窗口引起反复切换。
        if pid==0 || pid==GetCurrentProcessId(){return None;}
        let mut browser_rect=RECT::default();let mut island_rect=RECT::default();
        if GetWindowRect(foreground,&mut browser_rect).is_err() || GetWindowRect(island,&mut island_rect).is_err(){return None;}
        if !browser_overlaps_island(browser_rect,island_rect,settings.dock){return Some(false);}
        let Ok(process)=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid) else{return Some(false)};
        let mut path=[0u16;1024];let mut length=path.len() as u32;
        let result=QueryFullProcessImageNameW(process,PROCESS_NAME_WIN32,PWSTR(path.as_mut_ptr()),&mut length);
        let _=CloseHandle(process);
        if result.is_err(){return Some(false);}
        let path=String::from_utf16_lossy(&path[..length as usize]);
        Some(avoidance_matches(&path,settings))
    }
}

pub enum Incoming {
    Revoked,
    Status(String),
    Message(String, String, String, Option<crate::messenger::Target>),
}
#[path="toast_source.rs"]
mod toast_source;

fn notification_identity(n:&UserNotification,number:u32,created:i64,known:Option<crate::messenger::Messenger>)->(String,String){
    use crate::messenger::Messenger;
    let info=n.AppInfo().ok();
    let id=info.as_ref().and_then(|app|app.AppUserModelId().ok()).map(|id|id.to_string()).unwrap_or_default();
    let client=if !id.is_empty(){Messenger::from_notification_id(&id)}
        else{known.or_else(||toast_source::sender(number,created).and_then(|id|Messenger::from_notification_id(&id)))};
    if let Some(client)=client{return (client.name().into(),client.adapter().notification_id.into());}
    let name=info.and_then(|app|app.DisplayInfo().ok()).and_then(|display|display.DisplayName().ok())
        .map(|name|name.to_string()).filter(|name|!name.trim().is_empty()).unwrap_or_else(||"Windows 通知".into());
    let id=if id.is_empty(){name.clone()}else{id};
    (name,id)
}
fn notification_body(n: &UserNotification) -> Result<String> {
    let binding = n
        .Notification()?
        .Visual()?
        .GetBinding(&HSTRING::from("ToastGeneric"))?;
    let texts = binding.GetTextElements()?;
    let mut parts = vec![];
    for i in 0..texts.Size()?.min(8) {
        parts.push(texts.GetAt(i)?.Text()?.to_string().chars().take(300).collect::<String>());
    }
    Ok(parts.join(" · "))
}
/// 停止信号独立于消息队列，断开后结束等待，不再读取通知。
pub struct Notifications {
    incoming: Receiver<Incoming>,
    stop: mpsc::Sender<()>,
    taskbar_mask:std::sync::Arc<std::sync::atomic::AtomicU8>,
}
impl Notifications {
    pub fn try_recv(&self)->std::result::Result<Incoming,mpsc::TryRecvError>{self.incoming.try_recv()}
    pub fn taskbar_settings(&self,settings:&crate::model::Settings){self.taskbar_mask.store(crate::messenger::toast_suppression(settings),std::sync::atomic::Ordering::Relaxed);}
}
impl Drop for Notifications {fn drop(&mut self){let _=self.stop.send(());}}

#[derive(Clone,Copy,PartialEq,Eq)]
enum ToastBody {Skipped,Unavailable,Read(u64)}
#[derive(Clone,Copy,PartialEq,Eq)]
struct ToastVersion { created:i64, client:Option<crate::messenger::Messenger>, body:ToastBody }
impl ToastVersion {
    fn new(created:i64,client:Option<crate::messenger::Messenger>,body:Option<&str>)->Self{
        Self{created,client,body:body.map(|body|{let mut hash=std::collections::hash_map::DefaultHasher::new();body.hash(&mut hash);ToastBody::Read(hash.finish())}).unwrap_or(ToastBody::Skipped)}
    }
}
struct NotificationBaseline { previous:Option<HashMap<u32,ToastVersion>>,started:i64,primed:bool }
impl Default for NotificationBaseline {
    fn default()->Self{
        let started=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()
            .and_then(|time|i64::try_from(time.as_nanos()/100+116444736000000000).ok()).unwrap_or(i64::MAX);
        Self{previous:None,started,primed:false}
    }
}
impl NotificationBaseline {
    fn unchanged_other(&self,id:u32,created:i64)->Option<ToastVersion>{
        self.previous.as_ref()?.get(&id).copied().filter(|old|old.created==created && old.client.is_none() && old.body==ToastBody::Skipped)
    }
    fn changed(&self,id:u32,version:ToastVersion)->bool{
        let Some(previous)=&self.previous else{return version.created>self.started;};
        let Some(old)=previous.get(&id) else{return self.primed || version.created>self.started;};
        if old.created!=version.created{return true;}
        // 未读取、读取失败后的首次成功只建立指纹，避免通道切换和恢复读取时重放旧通知。
        matches!((old.body,version.body),(ToastBody::Read(before),ToastBody::Read(after)) if before!=after)
    }
    fn finish(&mut self,mut current:HashMap<u32,ToastVersion>,complete:bool){
        if !complete{if let Some(previous)=&self.previous{for (&id,&old) in previous{current.entry(id).or_insert(old);}}}
        self.previous=Some(current);self.primed|=complete;
    }
    fn observe(&self,id:u32,created:i64,client:Option<crate::messenger::Messenger>,tracked:bool,body:Result<String>)->(ToastVersion,Option<String>){
        let body=match body{
            Ok(body)=>body,
            Err(_)=>{
                if let Some(old)=self.previous.as_ref().and_then(|items|items.get(&id)).filter(|old|old.created==created && old.client==client){return (*old,None);}
                let version=ToastVersion{created,client,body:ToastBody::Unavailable};
                // 已确认是新通知时仍给通用提醒；恢复正文后只补指纹，不重复弹出。
                let message=self.changed(id,version).then(||if tracked{"有新消息".into()}else{"收到新通知".into()});
                return (version,message);
            }
        };
        let version=ToastVersion::new(created,client,tracked.then_some(body.as_str()));
        let message=self.changed(id,version).then(||if tracked && body.trim().is_empty(){"有新消息".into()}else{body});
        (version,message)
    }
}

fn channel_status(client:crate::messenger::Messenger)->String{
    use windows::UI::Notifications::{NotificationSetting,ToastNotificationManager};
    let name=client.name();
    match ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(client.adapter().notification_id)).and_then(|notifier|notifier.Setting()){
        Ok(NotificationSetting::Enabled)=>format!("{name} 系统通知通道可用"),
        Ok(NotificationSetting::DisabledForApplication)=>format!("{name} 系统通知已关闭，请在 Windows 通知设置中开启后重新连接"),
        Ok(NotificationSetting::DisabledForUser)=>"Windows 系统通知已关闭，请开启后重新连接".into(),
        Ok(NotificationSetting::DisabledByGroupPolicy)=>"系统策略禁止通知".into(),
        _=>format!("{name} 系统通知通道未就绪，请检查安装和通知设置后重新连接"),
    }
}

/// 必须由窗口线程发起权限请求，后台仅读取新增或更新的通知。
pub fn connect(ctx:eframe::egui::Context,filter:crate::messenger::NotificationFilter) -> Result<Notifications> {
    connect_inner(ctx,filter,crate::messenger::toast_suppression(&crate::model::Settings::default()))
}
pub fn connect_with_settings(ctx:eframe::egui::Context,settings:&crate::model::Settings)->Result<Notifications>{
    connect_inner(ctx,crate::messenger::NotificationFilter::for_settings(settings),crate::messenger::toast_suppression(settings))
}
fn connect_inner(ctx:eframe::egui::Context,filter:crate::messenger::NotificationFilter,mask:u8)->Result<Notifications>{
    let baseline=NotificationBaseline::default();
    let listener = UserNotificationListener::Current()?;
    let request = listener.RequestAccessAsync()?;
    let (tx, rx) = mpsc::sync_channel(32);
    let (stop, stopped)=mpsc::channel();
    let taskbar_mask=std::sync::Arc::new(std::sync::atomic::AtomicU8::new(mask));let worker_mask=taskbar_mask.clone();
    std::thread::Builder::new().name("daozai-notifications".into()).spawn(move || {
        let send=|event|{
            let result=tx.send(event);
            if result.is_ok(){ctx.request_repaint();}
            result.is_ok()
        };
        let run = || -> Result<()> {
            unsafe {windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED)}?;
            struct Apartment;
            impl Drop for Apartment {fn drop(&mut self){unsafe{windows::Win32::System::WinRT::RoUninitialize();}}}
            let _apartment=Apartment;
            if request.get()? != UserNotificationListenerAccessStatus::Allowed {
                send(Incoming::Revoked);
                send(Incoming::Status("Windows 未授予通知读取权限，可在系统设置中调整。".into()));
                return Ok(());
            }
            let listener = UserNotificationListener::Current()?;
            let channels=crate::messenger::Messenger::ALL.into_iter().filter(|client|client.adapter().capture==crate::messenger::Capture::SystemToast)
                .filter(|client|filter.allows(client.adapter().notification_id)).map(channel_status).collect::<Vec<_>>().join("\n");
            let mut baseline=baseline;
            let mut accounts=crate::messenger::ToastAccounts::default();accounts.sample();
            let mut last_status=String::new();
            loop {
                if stopped.try_recv()!=Err(mpsc::TryRecvError::Empty){return Ok(());}
                let (items,count)=match listener.GetAccessStatus().and_then(|access|{
                    if access!=UserNotificationListenerAccessStatus::Allowed{return Ok(None);}
                    let items=listener.GetNotificationsAsync(NotificationKinds::Toast)?.get()?;let count=items.Size()?;
                    Ok(Some((items,count)))
                }){
                    Ok(Some(items))=>items,
                    Ok(None)=>{send(Incoming::Revoked);send(Incoming::Status("通知权限已被关闭".into()));return Ok(());}
                    Err(error)=>{
                        let status=format!("Windows 通知暂时读取失败：{} · 将自动重试",error.code());
                        if status!=last_status{send(Incoming::Status(status.clone()));last_status=status;}
                        if stopped.recv_timeout(std::time::Duration::from_secs(2))!=Err(mpsc::RecvTimeoutError::Timeout){return Ok(());}
                        continue;
                    }
                };
                accounts.sample();
                let mut current=HashMap::new();
                let mut complete=true;
                let mut messages=Vec::new();
                for i in 0..count {
                    let Ok(n)=items.GetAt(i) else{complete=false;continue};
                    let (Ok(number),Ok(created))=(n.Id(),n.CreationTime()) else{complete=false;continue};
                    let created=created.UniversalTime;
                    if let Some(old)=baseline.unchanged_other(number,created){current.insert(number,old);continue;}
                    let known=baseline.previous.as_ref().and_then(|old|old.get(&number)).filter(|old|old.created==created).and_then(|old|old.client);
                    let (app,id)=notification_identity(&n,number,created,known);
                    // 微信只走任务栏事件，即使客户端同时发送系统通知也不读取其正文。
                    let client=crate::messenger::Messenger::from_notification_id(&id);
                    if !filter.allows(&id) || crate::messenger::suppresses_toast(&app,&id,worker_mask.load(std::sync::atomic::Ordering::Relaxed)){current.insert(number,ToastVersion::new(created,client,None));continue;}
                    let tracked=client.is_some();
                    // 已适配客户端同时检查正文变化；其他应用的启动历史不需要读取正文。
                    let body=if tracked || baseline.previous.is_some() || created>baseline.started{notification_body(&n)}else{Ok(String::new())};
                    let (version,body)=baseline.observe(number,created,client,tracked,body);
                    current.insert(number,version);
                    if let Some(body)=body{
                        let target=client.filter(|client|client.adapter().capture==crate::messenger::Capture::SystemToast).map(|client|(client,accounts.target(client,number,created)))
                            .and_then(|(client,target)|{
                                if target.is_none(){crate::messenger::windows::trace(format!("通知 {id}·创建 {created} 未能绑定 {} 账号",client.name()));}
                                target
                            });
                        if !body.trim().is_empty(){messages.push((created,app,id,body,target));}
                    }
                }
                baseline.finish(current,complete);
                accounts.retain(|client,id,created|baseline.previous.as_ref().and_then(|items|items.get(&id)).is_some_and(|item|item.created==created && item.client==Some(client)));
                messages.sort_by_key(|item|item.0);
                for (_,app,id,body,target) in messages {
                    if !send(Incoming::Message(app,id,body,target)){return Ok(());}
                }
                let status=match filter{crate::messenger::NotificationFilter::Only(client)=>format!("仅监听 {} · {channels}",client.name()),crate::messenger::NotificationFilter::All=>format!("正在监听 Windows 通知 · 当前可见 {count} 条\n{channels}")};
                if status!=last_status {
                    if !send(Incoming::Status(status.clone())){return Ok(());}
                    last_status=status;
                }
                if stopped.recv_timeout(std::time::Duration::from_secs(1))!=Err(mpsc::RecvTimeoutError::Timeout){return Ok(());}
            }
        };
        if let Err(e) = run() {
            send(Incoming::Status(format!("Windows 暂不允许读取通知：{}。请检查通知访问权限与应用身份。",e.code())));
        }
    }).map_err(|_|windows::core::Error::from_win32())?;
    Ok(Notifications{incoming:rx,stop,taskbar_mask})
}

pub fn pick_files() -> Result<Vec<std::path::PathBuf>> {
    pick_files_for("添加文件到置物架",true)
}

pub fn pick_files_for(title:&str,multiple:bool) -> Result<Vec<std::path::PathBuf>> {
    unsafe {
        use windows::Win32::UI::Shell::*;
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL)?;
        let mut options=FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST;
        if multiple{options|=FOS_ALLOWMULTISELECT;}
        dialog.SetOptions(options)?;
        dialog.SetTitle(&HSTRING::from(title))?;
        if let Err(e) = dialog.Show(None) {
            if e.code().0 as u32 == 0x800704c7 {
                return Ok(vec![]);
            }
            return Err(e);
        }
        let items = dialog.GetResults()?;
        let mut out = vec![];
        for i in 0..items.GetCount()? {
            let ptr = items.GetItemAt(i)?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let text = ptr.to_string()?;
            windows::Win32::System::Com::CoTaskMemFree(Some(ptr.0 as _));
            out.push(text.into());
        }
        Ok(out)
    }
}
/// 构建资源管理器可识别的文件数据对象。
fn file_data_object(path: &std::path::Path) -> Result<windows::Win32::System::Com::IDataObject> {
    unsafe {
        use windows::Win32::UI::Shell::*;
        let wide: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let pidl = ILCreateFromPathW(windows::core::PCWSTR(wide.as_ptr()));
        if pidl.is_null() {
            return Err(windows::core::Error::from_win32());
        }
        let result = (|| {
            let mut child = std::ptr::null_mut();
            let folder: IShellFolder = SHBindToParent(pidl, Some(&mut child))?;
            folder.GetUIObjectOf(None, &[child as *const _], None)
        })();
        ILFree(Some(pidl));
        result
    }
}
/// 原生拖出只允许复制，取消拖放不会改变置物架与原文件。
pub fn drag_file(path: &std::path::Path) -> Result<()> {
    unsafe {
        use windows::Win32::System::Ole::{OleInitialize, OleUninitialize, DROPEFFECT_COPY};
        use windows::Win32::UI::Shell::SHDoDragDrop;
        let initialized = OleInitialize(None).is_ok();
        let result = file_data_object(path)
            .and_then(|data| SHDoDragDrop(None, &data, None, DROPEFFECT_COPY).map(|_| ()));
        if initialized {
            OleUninitialize();
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notifications_skip_history_but_detect_reused_ids_and_updated_content(){
        let mut baseline=NotificationBaseline{primed:true,..Default::default()};
        let first=ToastVersion::new(10,Some(crate::messenger::Messenger::Qq),Some("第一条"));
        assert!(!baseline.changed(1,first));
        baseline.previous=Some(HashMap::from([(1,first),(2,ToastVersion::new(10,None,None))]));
        assert!(!baseline.changed(1,first));
        assert!(baseline.changed(1,ToastVersion::new(10,Some(crate::messenger::Messenger::Qq),Some("第二条"))));
        assert!(baseline.changed(1,ToastVersion::new(11,Some(crate::messenger::Messenger::Qq),Some("第一条"))));
        assert!(baseline.unchanged_other(2,10).is_some());
        assert!(baseline.unchanged_other(1,10).is_none());
        assert!(baseline.unchanged_other(2,11).is_none());
        baseline.previous=Some(HashMap::new());assert!(baseline.changed(1,first));
    }
    #[test]
    fn notifications_arriving_during_connection_are_not_treated_as_old_history(){
        let baseline=NotificationBaseline{previous:None,started:100,primed:false};
        let (_,message)=baseline.observe(1,101,Some(crate::messenger::Messenger::Qq),true,Ok("连接期间的新消息".into()));
        assert!(message.is_some(),"连接期间才到达的新通知不能混入启动历史而漏报");
    }
    #[test]
    fn notifications_read_failures_do_not_replay_an_unchanged_qq_message(){
        let client=Some(crate::messenger::Messenger::Qq);
        let original=ToastVersion::new(10,client,Some("同一条消息"));
        let baseline=NotificationBaseline{previous:Some(HashMap::from([(1,original)])),..Default::default()};
        let (version,message)=baseline.observe(1,10,client,true,Err(windows::core::Error::from_win32()));
        assert!(message.is_none(),"临时读取失败不能变成空正文更新而重复提醒");
        assert!(version==original,"读取失败时保留上一轮指纹");
    }
    #[test]
    fn notifications_unreadable_new_toasts_remind_once_and_historical_toasts_stay_silent(){
        let client=Some(crate::messenger::Messenger::Qq);
        for (created,expected) in [(90,false),(110,true)]{
            let mut baseline=NotificationBaseline{previous:None,started:100,primed:false};
            let (version,message)=baseline.observe(1,created,client,true,Err(windows::core::Error::from_win32()));
            assert_eq!(message.is_some(),expected,"历史与连接期间的新通知必须区分");
            baseline.previous=Some(HashMap::from([(1,version)]));
            let (version,message)=baseline.observe(1,created,client,true,Ok("恢复后的正文".into()));
            assert!(message.is_none(),"正文恢复不能重弹同一条通知");
            baseline.previous=Some(HashMap::from([(1,version)]));
            assert!(baseline.observe(1,created,client,true,Ok("下一条消息".into())).1.is_some());
        }
    }
    #[test]
    fn notifications_office_channel_switch_does_not_replay_but_later_body_updates_remind(){
        for client in crate::messenger::Messenger::OFFICE{
            let mut baseline=NotificationBaseline{previous:Some(HashMap::from([(1,ToastVersion::new(10,Some(client),None))])),started:0,primed:true};
            assert!(baseline.unchanged_other(1,10).is_none(),"办公软件的捕获通道会切换，不能永久跳过");
            let (version,message)=baseline.observe(1,10,Some(client),true,Ok("切换前的旧消息".into()));
            assert!(message.is_none());baseline.previous=Some(HashMap::from([(1,version)]));
            assert!(baseline.observe(1,10,Some(client),true,Ok("切换后的新消息".into())).1.is_some());
        }
    }
    #[test]
    fn notifications_partial_snapshots_do_not_replay_old_items_or_drop_new_items(){
        let client=Some(crate::messenger::Messenger::Qq);
        let old=ToastVersion::new(90,client,Some("启动前的旧通知"));let fresh=ToastVersion::new(110,client,Some("连接后的新通知"));
        let mut baseline=NotificationBaseline{previous:None,started:100,primed:false};
        assert!(baseline.changed(2,fresh));baseline.finish(HashMap::from([(2,fresh)]),false);
        assert!(!baseline.changed(1,old),"首次只读到部分条目时，补读到的启动历史不能重放");
        assert!(!baseline.changed(2,fresh));baseline.finish(HashMap::from([(1,old),(2,fresh)]),true);
        baseline.finish(HashMap::from([(2,fresh)]),false);
        assert!(!baseline.changed(1,old),"临时漏读条目不能被当成移除后重新新增");
        let next=ToastVersion::new(120,client,Some("下一条"));assert!(baseline.changed(3,next));
        baseline.finish(HashMap::new(),true);
        assert!(baseline.previous.as_ref().unwrap().is_empty(),"成功读完整空列表后应清理历史，不能永久保留");
    }
    #[test]
    fn notifications_disconnect_signals_stop_without_waiting_for_new_messages(){
        let (_tx,incoming)=mpsc::sync_channel(32);let (stop,stopped)=mpsc::channel();
        drop(Notifications{incoming,stop,taskbar_mask:std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0))});
        assert_eq!(stopped.try_recv(),Ok(()));
    }
    #[test]
    fn browser_minimal_recognizes_browsers_without_matching_electron_apps(){
        for name in ["chrome.exe","MSEDGE.EXE","firefox.exe","brave.exe","zen.exe","QQBrowser.exe","quark.exe","Quark.exe","QUARK.EXE"] {
            assert!(is_browser_process(name),"应识别浏览器 {name}");
        }
        for name in ["Codex.exe","Code.exe","msedgewebview2.exe","explorer.exe","chrome-helper.exe","quark_proxy.exe","quark_pwa_launcher.exe","quark_host_client.exe","quark_swap_util.exe","QuarkUpdaterSetup.exe",""] {
            assert!(!is_browser_process(name),"不能将 {name} 当作浏览器");
        }
    }
    #[test]
    fn browser_minimal_checks_the_actual_top_overlap_on_the_island_monitor(){
        let island=RECT{left:560,top:0,right:1360,bottom:238};
        for browser in [RECT{left:0,top:-8,right:1920,bottom:1200},RECT{left:0,top:0,right:960,bottom:1150},RECT{left:700,top:10,right:1500,bottom:800}] {
            assert!(browser_overlaps_island(browser,island,crate::model::Dock::Top));
        }
        for browser in [RECT{left:100,top:150,right:1700,bottom:1100},RECT{left:1920,top:0,right:3840,bottom:1200},RECT{left:0,top:0,right:600,bottom:1000},RECT{left:0,top:-1200,right:1920,bottom:0}] {
            assert!(!browser_overlaps_island(browser,island,crate::model::Dock::Top));
        }
    }
    #[test]
    fn side_dock_avoidance_checks_the_covered_screen_edge(){
        use crate::model::Dock;
        // 1920×1080 屏幕，100% 缩放：左侧画布 0..500，竖直居中（240..840），岛体中心 y=540。
        let left=RECT{left:0,top:240,right:500,bottom:840};
        let right=RECT{left:1420,top:240,right:1920,bottom:840};
        let maximized=RECT{left:-8,top:-8,right:1928,bottom:1048};
        assert!(browser_overlaps_island(maximized,left,Dock::Left) && browser_overlaps_island(maximized,right,Dock::Right));
        let left_half=RECT{left:0,top:0,right:960,bottom:1040};
        assert!(browser_overlaps_island(left_half,left,Dock::Left) && !browser_overlaps_island(left_half,right,Dock::Right),"贴左半屏只遮住左侧");
        let floating=RECT{left:300,top:100,right:1600,bottom:900};
        assert!(!browser_overlaps_island(floating,left,Dock::Left) && !browser_overlaps_island(floating,right,Dock::Right),"没碰到边缘的窗口不避让");
        let top_strip=RECT{left:0,top:0,right:1920,bottom:300};
        assert!(!browser_overlaps_island(top_strip,left,Dock::Left),"没盖到岛所在的那一段边缘");
    }
    #[test]
    fn network_connection_uses_connected_wording() {
        let old=Snapshot{volume:None,audio_endpoint:None,caps:false,network:false,network_kind:NetworkKind::Other};
        let mut connected=old.clone();connected.network=true;
        assert_eq!(connected.changes(&old),vec![("网络连接".into(),"网络已连接".into())]);
    }
    #[test]
    fn network_switches_transport_and_keeps_the_disconnected_type() {
        let wifi=Snapshot{volume:None,audio_endpoint:None,caps:false,network:true,network_kind:NetworkKind::Wifi};
        let mut cable=wifi.clone();cable.network_kind=NetworkKind::Ethernet;
        assert_eq!(cable.changes(&wifi),vec![("网络连接".into(),"以太网 · 网络已连接".into())]);
        let mut off=cable.clone();off.network=false;off.network_kind=NetworkKind::Other;
        assert_eq!(off.changes(&cable),vec![("网络连接".into(),"以太网 · 网络已断开".into())]);
        assert_eq!(wifi.changes(&off),vec![("网络连接".into(),"Wi-Fi · 网络已连接".into())]);
    }
    #[test]
    fn switching_audio_endpoints_is_not_a_volume_adjustment() {
        let old=Snapshot{volume:Some((20,false)),audio_endpoint:Some("speakers".into()),caps:false,network:true,network_kind:NetworkKind::Other};
        let mut new=old.clone();
        new.audio_endpoint=Some("headphones".into());new.volume=Some((80,false));new.caps=true;
        assert_eq!(new.changes(&old),vec![("大写锁定".into(),"已开启".into())]);
        let mut adjusted=new.clone();adjusted.volume=Some((60,false));
        assert_eq!(adjusted.changes(&new),vec![("系统音量".into(),"音量 60%".into())]);
        adjusted.volume=Some((60,true));
        assert_eq!(adjusted.changes(&new),vec![("系统音量".into(),"已静音".into())]);
        assert!(!old.changes(&new).iter().any(|(title,_)|title=="系统音量"));
    }

    #[test]
    fn dragged_file_exposes_native_file_drop_format() {
        unsafe {
            use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
            use windows::Win32::System::Ole::{OleInitialize, OleUninitialize, CF_HDROP};
            let initialized = OleInitialize(None).is_ok();
            let path =
                std::env::temp_dir().join(format!("daozai-drag-test-{}.txt", std::process::id()));
            std::fs::write(&path, "test").unwrap();
            {
                let data = file_data_object(&path).unwrap();
                let format = FORMATETC {
                    cfFormat: CF_HDROP.0,
                    dwAspect: DVASPECT_CONTENT.0,
                    lindex: -1,
                    tymed: TYMED_HGLOBAL.0 as u32,
                    ..Default::default()
                };
                assert!(data.QueryGetData(&format).is_ok());
            }
            std::fs::remove_file(path).unwrap();
            if initialized {
                OleUninitialize();
            }
        }
    }
}

fn avoidance_matches(path:&str,settings:&crate::model::Settings)->bool {
    settings.browser_minimal && (crate::avoidance::contains_app(&settings.avoid_apps,path)
        || settings.avoid_browsers && std::path::Path::new(path).file_name().and_then(|name|name.to_str()).is_some_and(is_browser_process))
}

pub(crate) fn avoidance_windows()->std::result::Result<Vec<crate::avoidance::Candidate>,String> {
    use windows::Win32::Foundation::{BOOL,LPARAM};
    unsafe extern "system" fn visit(hwnd:HWND,param:LPARAM)->BOOL {
        if !IsWindowVisible(hwnd).as_bool() || GetWindowLongPtrW(hwnd,GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0!=0 {return BOOL(1);}
        let mut cloaked=0u32;
        let _=windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(hwnd,windows::Win32::Graphics::Dwm::DWMWA_CLOAKED,(&mut cloaked as *mut u32).cast(),4);
        if cloaked!=0{return BOOL(1);}
        let mut title=[0u16;1024];let len=GetWindowTextW(hwnd,&mut title);
        if len==0{return BOOL(1);}
        let mut pid=0;GetWindowThreadProcessId(hwnd,Some(&mut pid));
        if pid==GetCurrentProcessId(){return BOOL(1);}
        let Ok(process)=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid)else{return BOOL(1)};
        let mut path=[0u16;32768];let mut length=path.len() as u32;
        let result=QueryFullProcessImageNameW(process,PROCESS_NAME_WIN32,PWSTR(path.as_mut_ptr()),&mut length);
        let _=CloseHandle(process);
        if result.is_err(){return BOOL(1);}
        let path=String::from_utf16_lossy(&path[..length as usize]);
        let items=&mut *(param.0 as *mut Vec<crate::avoidance::Candidate>);
        if items.iter().any(|a|crate::avoidance::same_app(&a.path,&path)){return BOOL(1);}
        let filename=std::path::Path::new(&path).file_name().and_then(|s|s.to_str()).unwrap_or("");
        let name=executable_description(&path).unwrap_or_else(||filename.trim_end_matches(".exe").to_string());
        items.push(crate::avoidance::Candidate{browser:is_browser_process(filename),path,name,title:String::from_utf16_lossy(&title[..len as usize])});
        BOOL(1)
    }
    let mut items:Vec<crate::avoidance::Candidate>=Vec::new();
    unsafe{EnumWindows(Some(visit),LPARAM(&mut items as *mut _ as isize))}.map_err(|_|"无法读取应用列表，请刷新重试".to_string())?;
    items.sort_by_key(|a|a.name.to_lowercase());
    Ok(items)
}

fn executable_description(path:&str)->Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW,GetFileVersionInfoW,VerQueryValueW};
    let path:Vec<u16>=path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let size=GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()),None);if size==0{return None;}
        let mut bytes=vec![0u8;size as usize];GetFileVersionInfoW(PCWSTR(path.as_ptr()),0,size,bytes.as_mut_ptr().cast()).ok()?;
        let mut ptr=std::ptr::null_mut();let mut len=0;
        if !VerQueryValueW(bytes.as_ptr().cast(),windows::core::w!("\\VarFileInfo\\Translation"),&mut ptr,&mut len).as_bool() || len<4{return None;}
        let translation=std::slice::from_raw_parts(ptr as *const u16,2);
        let key:Vec<u16>=format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription",translation[0],translation[1]).encode_utf16().chain(Some(0)).collect();
        if !VerQueryValueW(bytes.as_ptr().cast(),PCWSTR(key.as_ptr()),&mut ptr,&mut len).as_bool() || len<=1{return None;}
        let name=String::from_utf16_lossy(std::slice::from_raw_parts(ptr as *const u16,len as usize)).trim_end_matches('\0').trim().to_string();
        (!name.is_empty()).then_some(name)
    }
}

#[cfg(test)]
mod avoidance_tests {
    use super::*;
    #[test]
    fn avoidance_respects_master_browser_switch_and_custom_app_identity() {
        let mut settings=crate::model::Settings::default();
        settings.avoid_apps=vec![(r"D:\Apps\Editor.exe".into(),"编辑器".into())];
        assert!(!avoidance_matches(r"D:\Apps\Editor.exe",&settings));
        settings.browser_minimal=true;
        assert!(avoidance_matches(r"D:\Quark\quark.exe",&settings));
        assert!(avoidance_matches(r"d:\apps\EDITOR.EXE",&settings));
        assert!(!avoidance_matches(r"C:\Other\Editor.exe",&settings));
        settings.avoid_browsers=false;
        assert!(!avoidance_matches(r"D:\Quark\quark.exe",&settings));
        assert!(avoidance_matches(r"D:\Apps\Editor.exe",&settings));
        settings.avoid_apps.clear();
        assert!(!avoidance_matches(r"D:\Apps\Editor.exe",&settings));
    }
    #[test]
    fn avoidance_window_list_excludes_this_process_and_deduplicates_apps() {
        let entries=avoidance_windows().unwrap();
        let current=std::env::current_exe().unwrap().to_string_lossy().into_owned();
        for (index,app) in entries.iter().enumerate() {
            assert!(!crate::avoidance::same_app(&app.path,&current));
            assert!(!app.name.is_empty() && !app.title.is_empty());
            assert!(!entries[..index].iter().any(|previous|crate::avoidance::same_app(&previous.path,&app.path)));
        }
        let notepad=std::path::Path::new(r"C:\Windows\System32\notepad.exe");
        if notepad.exists(){assert!(executable_description(notepad.to_str().unwrap()).is_some());}
    }
}
