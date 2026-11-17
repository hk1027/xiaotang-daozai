//! 系统托盘、右键菜单与开机启动。
//!
//! 托盘行为：
//! - `uID=1`，`uFlags=NIF_MESSAGE|NIF_ICON|NIF_TIP`，回调 `WM_APP+9`，提示 `岛仔`；
//! - 图标取 exe 内 `RT_GROUP_ICON #1`，按 `SM_CXSMICON` 走 `LoadImageW`；
//! - 左键、右键都弹同一个菜单，没有双击分支；
//! - 菜单固定中文，含设置入口，前三项每次打开现算勾选态；
//! - 弹菜单前 `SetForegroundWindow`，期间 `SetTimer(12, 16ms)`，收尾 `KillTimer` +
//!   `PostMessage(WM_NULL)` + `DestroyMenu`；
//! - `Exit` 先摘图标再 `ExitProcess`；`TaskbarCreated` 广播时重建图标。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM, ERROR_ALREADY_EXISTS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::System::Threading::{CreateMutexW, ExitProcess};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIF_INFO, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// 原版的托盘回调消息。
pub const WM_TRAY: u32 = WM_APP + 9;
/// 仅构建并导出当前菜单状态，不弹出菜单（自动化取证用）。
pub const WM_TRAY_DUMP: u32 = WM_APP + 2;
/// 在屏幕坐标 (wparam, lparam) 弹出菜单（隐藏桌面取证用）。
pub const WM_TRAY_MENU: u32 = WM_APP + 3;

const TRAY_UID: u32 = 1;
const CMD_EXIT: u32 = 1;
const CMD_START: u32 = 2;
const CMD_MASCOT: u32 = 3;
const CMD_ABOUT: u32 = 4;
const CMD_MINIMAL: u32 = 5;
const CMD_SETTINGS:u32=6;
const TIMER_MENU: usize = 12;

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Daozai");
const MUTEX_NAME: PCWSTR = w!("Local\\Daozai.SingleInstance");

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub pet: bool,
    pub minimal: bool,
    pub autostart: bool,
}

/// 菜单固定中文：启动 / 吉祥物 / 极简 / —— / 设置 / 关于 / 退出。
fn labels() -> [&'static str; 6] {
    ["开机启动", "吉祥物", "极简刘海", "关于岛仔", "退出", "设置…"]
}

struct Ctx {
    tx: Sender<Event>,
    state: Arc<Mutex<State>>,
    taskbar_msg: u32,
    hicon: HICON,
    before_exit: Box<dyn Fn() + Send>,
    added: AtomicBool,
    adds: AtomicU32,
}

impl Ctx {
    fn state(&self) -> State {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    TogglePet,
    OpenSettings,
    OpenCalendar,
    ToggleMinimal,
    ToggleAutostart,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn fill_tip(dst: &mut [u16], s: &str) {
    let src = wide(s);
    let n = (dst.len() - 1).min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
    dst[n] = 0;
}

/// exe 里编入的 `RT_GROUP_ICON #1`，按系统小图标尺寸取最合适的一帧。
fn load_resource_icon() -> Option<HICON> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let cx = GetSystemMetrics(SM_CXSMICON);
        let handle = LoadImageW(module, PCWSTR(1 as *const u16), IMAGE_ICON, cx, cx, LR_DEFAULTCOLOR).ok()?;
        (!handle.is_invalid()).then_some(HICON(handle.0))
    }
}

/// 图标取不到时，直接解析内嵌的岛仔 ICO。
fn load_embedded_icon() -> Option<HICON> {
    let ico: &[u8] = include_bytes!("../assets/daozai.ico");
    if ico.len() < 6 {
        return None;
    }
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let mut best: Option<(i32, u32, u32)> = None;
    for i in 0..count {
        let at = 6 + i * 16;
        if at + 16 > ico.len() {
            break;
        }
        let entry = &ico[at..at + 16];
        let w = if entry[0] == 0 { 256 } else { entry[0] as i32 };
        let bytes = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]);
        let off = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]);
        if off as usize + bytes as usize > ico.len() {
            continue;
        }
        let score = (w - 32).abs();
        if best.is_none_or(|(s, _, _)| score < s) {
            best = Some((score, bytes, off));
        }
    }
    let (_, bytes, off) = best?;
    let data = &ico[off as usize..(off + bytes) as usize];
    unsafe { CreateIconFromResourceEx(data, true, 0x0003_0000, 0, 0, LR_DEFAULTCOLOR).ok() }
}

fn load_icon() -> Option<HICON> {
    load_resource_icon().or_else(load_embedded_icon)
}

fn add_icon(hwnd: HWND, hicon: HICON) -> bool {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: hicon,
        ..Default::default()
    };
    fill_tip(&mut nid.szTip, "岛仔");
    unsafe {
        if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
            // explorer 里已有同名残留时，用 MODIFY 顶掉
            return Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool();
        }
        true
    }
}

fn remove_icon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        ..Default::default()
    };
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

fn popup_menu(state: State) -> windows::core::Result<HMENU> {
    let text = labels();
    let wide: Vec<Vec<u16>> = text.iter().map(|s| wide(s)).collect();
    unsafe {
        let menu = CreatePopupMenu()?;
        let append = |flags: MENU_ITEM_FLAGS, id: u32, label: PCWSTR| {
            let _ = AppendMenuW(menu, flags, id as usize, label);
        };
        let check = |on: bool| {
            if on {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            }
        };
        append(check(state.autostart), CMD_START, PCWSTR(wide[0].as_ptr()));
        append(check(state.pet), CMD_MASCOT, PCWSTR(wide[1].as_ptr()));
        append(check(state.minimal), CMD_MINIMAL, PCWSTR(wide[2].as_ptr()));
        append(MF_SEPARATOR, 0, PCWSTR::null());
        append(MF_STRING, CMD_SETTINGS, PCWSTR(wide[5].as_ptr()));
        append(MF_STRING, CMD_ABOUT, PCWSTR(wide[3].as_ptr()));
        append(MF_STRING, CMD_EXIT, PCWSTR(wide[4].as_ptr()));
        Ok(menu)
    }
}

fn menu_items(menu: HMENU) -> Vec<(String, bool, bool)> {
    unsafe {
        let count = GetMenuItemCount(menu);
        (0..count)
            .map(|i| {
                let mut buf = [0u16; 128];
                let flags = GetMenuState(menu, i as u32, MF_BYPOSITION);
                let sep = flags & MF_SEPARATOR.0 != 0;
                let len = if sep {
                    0
                } else {
                    GetMenuStringW(menu, i as u32, Some(&mut buf), MF_BYPOSITION)
                };
                let text = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
                (text, flags & MF_CHECKED.0 != 0, sep)
            })
            .collect()
    }
}

fn about() {
    let text = wide(&format!(
        "岛仔 {}\n\nWindows 桌面灵动岛小助手\n退出请右键托盘图标，选择「退出」。",
        env!("CARGO_PKG_VERSION")
    ));
    let title = wide("关于岛仔");
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn dump_menu(hwnd: HWND, ctx: &Ctx, menu: HMENU) {
    if let Ok(path) = std::env::var("DAOZAI_TRAY_DUMP") {
        let state = ctx.state();
        let mut body = String::new();
        body.push_str(&format!("hwnd=0x{:X}\n", hwnd.0 as usize));
        body.push_str(&format!("icon_added={}\n", ctx.added.load(Ordering::Relaxed)));
        body.push_str(&format!("icon_adds={}\n", ctx.adds.load(Ordering::Relaxed)));
        body.push_str(&format!("autostart={}\n", state.autostart));
        body.push_str(&format!("pet={}\nminimal={}\n", state.pet, state.minimal));
        for (i, (text, checked, sep)) in menu_items(menu).into_iter().enumerate() {
            if sep {
                body.push_str(&format!("item{}=(separator)\n", i + 1));
            } else {
                body.push_str(&format!("item{}={}|checked={}\n", i + 1, text, u8::from(checked)));
            }
        }
        let _ = std::fs::write(path, body);
    }
}

fn show_menu_at(hwnd: HWND, ctx: &Ctx, x: i32, y: i32) {
    let Ok(menu) = popup_menu(ctx.state()) else { return };
    dump_menu(hwnd, ctx, menu);
    unsafe {
        // 不先给前台，菜单会因为焦点切换立刻消失
        let _ = SetForegroundWindow(hwnd);
        let _ = SetTimer(hwnd, TIMER_MENU, 16, None);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            x,
            y,
            0,
            hwnd,
            None,
        )
        .0 as u32;
        let _ = KillTimer(hwnd, TIMER_MENU);
        // 收尾：让菜单彻底消失
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        match cmd {
            CMD_START => {
                let _ = ctx.tx.send(Event::ToggleAutostart);
            }
            CMD_MASCOT => {
                let _ = ctx.tx.send(Event::TogglePet);
            }
            CMD_MINIMAL => {
                let _ = ctx.tx.send(Event::ToggleMinimal);
            }
            CMD_SETTINGS => {let _=ctx.tx.send(Event::OpenSettings);}
            CMD_ABOUT => about(),
            CMD_EXIT => {
                (ctx.before_exit)();
                remove_icon(hwnd);
                ExitProcess(0);
            }
            _ => {}
        }
    }
}

fn show_menu(hwnd: HWND, ctx: &Ctx) {
    unsafe {
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        show_menu_at(hwnd, ctx, p.x, p.y);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
    if ctx_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let ctx = &*ctx_ptr;
    if msg == WM_TRAY {
        let mouse = (lp.0 as u32) & 0xffff;
        // 原版左右键同一个分支，没有双击行为
        if matches!(mouse, WM_LBUTTONUP | WM_RBUTTONUP) {
            show_menu(hwnd, ctx);
        }
        return LRESULT(0);
    }
    if msg == WM_APP+4 {return LRESULT(ctx.tx.send(Event::OpenSettings).is_ok() as isize);}
    if msg == WM_APP+5 {return LRESULT(ctx.tx.send(Event::OpenCalendar).is_ok() as isize);}
    if msg == WM_TRAY_DUMP {
        if let Ok(menu) = popup_menu(ctx.state()) {
            dump_menu(hwnd, ctx, menu);
            unsafe {
                let _ = DestroyMenu(menu);
            }
        }
        return LRESULT(0);
    }
    if msg == WM_TRAY_MENU {
        show_menu_at(hwnd, ctx, wp.0 as i32, lp.0 as i32);
        return LRESULT(0);
    }
    if ctx.taskbar_msg != 0 && msg == ctx.taskbar_msg {
        let added = add_icon(hwnd, ctx.hicon);
        ctx.added.store(added, Ordering::Relaxed);
        ctx.adds.fetch_add(1, Ordering::Relaxed);
        return LRESULT(0);
    }
    match msg {
        WM_CLOSE => {
            remove_icon(hwnd);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

pub struct Tray {
    hwnd: HWND,
    events: Receiver<Event>,
    state: Arc<Mutex<State>>,
    _thread: JoinHandle<()>,
}

impl Tray {
    /// 与托盘菜单「退出」相同：先摘图标再结束进程（用于「立即重启」）。
    pub fn exit_process(&self) -> ! {
        unsafe { remove_icon(self.hwnd); ExitProcess(0); }
    }
}

impl Tray {
    pub fn start(state: State) -> Option<Self> {Self::start_with_exit(state,||{})}

    pub(crate) fn start_with_exit(state: State,before_exit:impl Fn()+Send+'static) -> Option<Self> {
        let shared = Arc::new(Mutex::new(state));
        let (event_tx, event_rx) = channel();
        let (ready_tx, ready_rx) = channel();
        let thread_state = shared.clone();
        let thread = std::thread::Builder::new()
            .name("daozai-tray".into())
            .spawn(move || unsafe {
                let Ok(module) = GetModuleHandleW(None) else {
                    let _ = ready_tx.send(Err("GetModuleHandleW 失败".to_string()));
                    return;
                };
                let hinst = HINSTANCE(module.0);
                let class = w!("DaozaiTray");
                let wc = WNDCLASSW {
                    style: WNDCLASS_STYLES(0),
                    lpfnWndProc: Some(wndproc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinst,
                    hIcon: HICON::default(),
                    hCursor: HCURSOR::default(),
                    hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH::default(),
                    lpszMenuName: PCWSTR::null(),
                    lpszClassName: class,
                };
                let _ = RegisterClassW(&wc);
                let hwnd = match CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    class,
                    w!("岛仔托盘"),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    hinst,
                    None,
                ) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("创建托盘窗口失败：{e}")));
                        return;
                    }
                };
                let Some(hicon) = load_icon() else {
                    let _ = ready_tx.send(Err("读取托盘图标失败".to_string()));
                    return;
                };
                let ctx = Box::new(Ctx {
                    before_exit:Box::new(before_exit),
                    tx: event_tx,
                    state: thread_state,
                    taskbar_msg: RegisterWindowMessageW(w!("TaskbarCreated")),
                    hicon,
                    added: AtomicBool::new(false),
                    adds: AtomicU32::new(0),
                });
                let ctx_ptr = Box::into_raw(ctx);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx_ptr as isize);
                let added = add_icon(hwnd, hicon);
                (*ctx_ptr).added.store(added, Ordering::Relaxed);
                (*ctx_ptr).adds.store(1, Ordering::Relaxed);
                let _ = ready_tx.send(Ok((hwnd.0 as isize, added)));

                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                remove_icon(hwnd);
                let _ = DestroyIcon(hicon);
                drop(Box::from_raw(ctx_ptr));
            });
        let thread = match thread {
            Ok(t) => t,
            Err(_) => return None,
        };
        let (hwnd, _added) = match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                eprintln!("tray: {e}");
                return None;
            }
            Err(_) => {
                eprintln!("tray: 启动超时");
                return None;
            }
        };
        Some(Self { hwnd: HWND(hwnd as *mut _), events: event_rx, state: shared, _thread: thread })
    }

    #[allow(dead_code)]
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn notify(&self,title:&str,body:&str,sound:bool) {
        let mut nid=NOTIFYICONDATAW {cbSize:std::mem::size_of::<NOTIFYICONDATAW>() as u32,hWnd:self.hwnd,uID:TRAY_UID,uFlags:NIF_INFO,dwInfoFlags:NIIF_INFO,..Default::default()};
        fill_tip(&mut nid.szInfoTitle,title);fill_tip(&mut nid.szInfo,body);
        // NIIF_NOSOUND，默认安静提醒。
        if !sound {nid.dwInfoFlags.0|=0x10;}
        unsafe{let _=Shell_NotifyIconW(NIM_MODIFY,&nid);}
    }
    pub fn sync(&self, state: State) {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if *guard != state {
            *guard = state;
        }
    }

    pub fn poll(&self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(e) = self.events.try_recv() {
            out.push(e);
        }
        out
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(self.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// 单实例：已有岛仔实例时返回 false。
pub fn single_instance() -> bool {
    unsafe {
        match CreateMutexW(None, false, MUTEX_NAME) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = CloseHandle(handle);
                    false
                } else {
                    // HANDLE 没有 Drop，故意不 CloseHandle：进程存活期间一直持有互斥体
                    let _ = handle;
                    true
                }
            }
            Err(_) => true,
        }
    }
}

fn run_key() -> Option<HKEY> {
    unsafe {
        let mut key = HKEY::default();
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );
        (rc.0 == 0).then_some(key)
    }
}

pub fn autostart_enabled() -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_QUERY_VALUE, &mut key).0 != 0 {
            return false;
        }
        let mut size = 0u32;
        let rc = RegQueryValueExW(key, RUN_VALUE, None, None, None, Some(&mut size));
        let _ = RegCloseKey(key);
        rc.0 == 0 && size > 0
    }
}

pub fn set_autostart(on: bool) -> Result<(), String> {
    unsafe {
        if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let value = format!("\"{}\"", exe.display());
            let utf16 = wide(&value);
            let bytes = std::slice::from_raw_parts(utf16.as_ptr() as *const u8, utf16.len() * 2);
            let key = run_key().ok_or_else(|| "打不开 HKCU\\Run".to_string())?;
            let rc = RegSetValueExW(key, RUN_VALUE, 0, REG_SZ, Some(bytes));
            let _ = RegCloseKey(key);
            if rc.0 != 0 {
                return Err(format!("写 HKCU\\Run 失败：{}", rc.0));
            }
        } else {
            let mut key = HKEY::default();
            if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_SET_VALUE, &mut key).0 == 0 {
                let _ = RegDeleteValueW(key, RUN_VALUE);
                let _ = RegCloseKey(key);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_launch_requests_settings_or_calendar() {
        let tray=Tray::start(State::default()).expect("托盘应能启动");
        for (calendar,expected) in [(false,Event::OpenSettings),(true,Event::OpenCalendar)] {
            assert!(request_existing_window(tray.hwnd(),calendar));
            assert_eq!(tray.events.recv_timeout(Duration::from_secs(2)).unwrap(),expected);
        }
        assert!(!request_existing_window(HWND::default(),false));
    }

    #[test]
    fn menu_order_checks_and_separator() {
        for (state, want) in [
            (State { pet: false, minimal: false, autostart: false }, [false, false, false]),
            (State { pet: true, minimal: false, autostart: true }, [true, true, false]),
            (State { pet: false, minimal: true, autostart: false }, [false, false, true]),
        ] {
            let menu = popup_menu(state).expect("创建菜单");
            let items = menu_items(menu);
            assert_eq!(items.len(), 7, "6 项 + 1 分隔线");
            assert!(items[3].2, "第 4 项是分隔线");
            let texts: Vec<_> = items.iter().filter(|i| !i.2).map(|i| i.0.as_str()).collect();
            assert_eq!(texts, ["开机启动", "吉祥物", "极简刘海", "设置…", "关于岛仔", "退出"]);
            assert_eq!([items[0].1, items[1].1, items[2].1], want, "勾选态应与状态一致");
            unsafe {
                let _ = DestroyMenu(menu);
            }
        }
    }

    #[test]
    fn labels_are_fixed_chinese() {
        assert_eq!(labels(), ["开机启动", "吉祥物", "极简刘海", "关于岛仔", "退出", "设置…"]);
    }

    #[test]
    fn icon_resource_is_embedded() {
        // 托盘图标必须来自 exe 资源 #1（rc.exe 编入），不是临时画的
        assert!(load_resource_icon().is_some(), "RT_GROUP_ICON #1 取不到");
    }

    #[test]
    fn embedded_icon_still_parsable_as_fallback() {
        let ico: &[u8] = include_bytes!("../assets/daozai.ico");
        assert_eq!(&ico[..4], &[0, 0, 1, 0]);
        assert!(load_embedded_icon().is_some());
    }

    #[test]
    fn menu_ids_match_original() {
        assert_eq!(
            [CMD_EXIT, CMD_START, CMD_MASCOT, CMD_ABOUT, CMD_MINIMAL],
            [1, 2, 3, 4, 5]
        );
    }
}

/// 重复启动时请求现有实例打开窗口，限制等待时间以免跟随挂起。
fn request_existing_window(hwnd:HWND,calendar:bool)->bool {unsafe{
    if hwnd.0.is_null(){return false;}
    let mut reply=0usize;
    SendMessageTimeoutW(hwnd,if calendar{WM_APP+5}else{WM_APP+4},WPARAM(0),LPARAM(0),
        SMTO_ABORTIFHUNG|SMTO_ERRORONEXIT,1500,Some(&mut reply)).0!=0 && reply==1
}}

pub fn activate_existing(calendar:bool)->bool {unsafe{
    let Ok(tray)=FindWindowW(w!("DaozaiTray"),PCWSTR::null()) else{return false;};
    let Ok(window)=FindWindowW(PCWSTR::null(),w!("岛仔")) else{return false;};
    let(mut tray_pid,mut window_pid)=(0,0);
    GetWindowThreadProcessId(tray,Some(&mut tray_pid));
    GetWindowThreadProcessId(window,Some(&mut window_pid));
    if tray_pid==0 || tray_pid!=window_pid{return false;}
    // 托盘有独立线程，能接收消息不代表界面线程仍可响应。
    if SendMessageTimeoutW(window,WM_NULL,WPARAM(0),LPARAM(0),
        SMTO_ABORTIFHUNG|SMTO_ERRORONEXIT,1500,None).0==0{return false;}
    // 用户再次启动时，把新进程的前台激活权限交给现有实例，无需模拟按键。
    let _=AllowSetForegroundWindow(tray_pid);
    request_existing_window(tray,calendar)
}}
