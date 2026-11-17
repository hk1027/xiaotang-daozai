//! 输入提示同时核对切换按键与实际输入状态。
use std::time::{Duration,Instant};
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct State {
    pub context:(usize,usize),
    pub layout:usize,
    pub profile:Option<(u128,u128)>,
    pub native:Option<bool>,
}
impl State {
    fn label(self)->&'static str {
        if self.native==Some(false){return "英语";}
        match self.layout&0x3ff {4=>"中文",9=>"英语",17=>"日语",18=>"韩语",_=>"已切换"}
    }
    #[cfg(windows)]
    pub fn read()->Option<Self>{Self::read_details(true)}
    #[cfg(windows)]
    pub(crate) fn identity()->Option<Self>{Self::read_details(false)}
    #[cfg(windows)]
    fn read_details(details:bool)->Option<Self>{unsafe{
        use windows::Win32::UI::{Input::{Ime::ImmGetDefaultIMEWnd,KeyboardAndMouse::GetKeyboardLayout},WindowsAndMessaging::*};
        use windows::Win32::Foundation::{LPARAM,WPARAM};
        let foreground=GetForegroundWindow();if foreground.0.is_null(){return None;}
        let thread=GetWindowThreadProcessId(foreground,None);
        let mut info=GUITHREADINFO{cbSize:std::mem::size_of::<GUITHREADINFO>()as u32,..Default::default()};
        let focus=if GetGUIThreadInfo(thread,&mut info).is_ok()&&!info.hwndFocus.0.is_null(){info.hwndFocus}else{foreground};
        let layout=GetKeyboardLayout(GetWindowThreadProcessId(focus,None)).0 as usize;
        let mut native=None;
        if details && [4,17,18].contains(&(layout&0x3ff)){
            let ime=ImmGetDefaultIMEWnd(focus);
            if !ime.0.is_null(){
                let query=|command:usize|{
                    let mut value=0usize;
                    let ok=SendMessageTimeoutW(ime,WM_IME_CONTROL,WPARAM(command),LPARAM(0),SMTO_ABORTIFHUNG|SMTO_ERRORONEXIT,30,Some(&mut value));
                    (ok.0!=0).then_some(value)
                };
                // 查询前台输入法线程，避免跨进程获取输入上下文失败。
                native=query(0x0005).and_then(|open|if open==0{Some(false)}else{query(0x0001).map(|mode|mode&1!=0)});
            }
        }
        let profile=if details{active_profile()}else{None};
        if GetForegroundWindow()!=foreground{return None;}
        let current_focus=if GetGUIThreadInfo(thread,&mut info).is_ok()&&!info.hwndFocus.0.is_null(){info.hwndFocus}else{foreground};
        if current_focus!=focus{return None;}
        Some(Self{context:(foreground.0 as usize,focus.0 as usize),layout,profile,native})
    }}
    #[cfg(not(windows))]
    pub fn read()->Option<Self>{None}
}
#[cfg(windows)]
fn active_profile()->Option<(u128,u128)>{
    use windows::Win32::{System::Com::{CoInitializeEx,CoCreateInstance,COINIT_APARTMENTTHREADED,CLSCTX_INPROC_SERVER},UI::TextServices::*};
    thread_local!{static MANAGER:Option<ITfInputProcessorProfileMgr>=unsafe{
        let _=CoInitializeEx(None,COINIT_APARTMENTTHREADED);
        CoCreateInstance(&CLSID_TF_InputProcessorProfiles,None,CLSCTX_INPROC_SERVER).ok()
    };}
    MANAGER.with(|manager|unsafe{
        let mut profile=TF_INPUTPROCESSORPROFILE::default();
        manager.as_ref()?.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD,&mut profile).ok()?;
        (profile.dwProfileType==TF_PROFILETYPE_INPUTPROCESSOR).then_some((profile.clsid.to_u128(),profile.guidProfile.to_u128()))
    })
}
pub struct Monitor {last:Option<State>}
impl Monitor {
    pub fn new(initial:Option<State>)->Self {Self{last:initial}}
    pub fn observe(&mut self,next:Option<State>,manual:bool)->Option<&'static str>{
        let mut next=next.filter(|state|state.layout!=0)?;
        if self.last.is_some_and(|old|old.context!=next.context){
            self.last=Some(next);return None;
        }
        let changed=self.last.is_some_and(|old|old.layout!=next.layout||old.profile.zip(next.profile).is_some_and(|(a,b)|a!=b)||old.native.zip(next.native).is_some_and(|(a,b)|a!=b));
        if let Some(old)=self.last.filter(|old|old.layout==next.layout){
            if next.native.is_none()&&!old.profile.zip(next.profile).is_some_and(|(a,b)|a!=b){next.native=old.native;}
            if next.profile.is_none(){next.profile=old.profile;}
        }
        self.last=Some(next);
        (changed&&manual).then(||next.label())
    }
}

#[cfg(windows)]
static SWITCH_AT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
#[cfg(windows)]
pub(crate) fn switch_recent()->bool {
    let at=SWITCH_AT.load(std::sync::atomic::Ordering::Relaxed);
    at!=0 && unsafe{windows::Win32::System::SystemInformation::GetTickCount64()}.saturating_sub(at)<=500
}

#[derive(Default)]
struct SwitchKeys {
    held:u8,
    used:bool,
    context:(usize,usize),
    pending:Option<((usize,usize),Instant)>,
}
impl SwitchKeys {
    fn key(&mut self,vk:u32,down:bool,context:(usize,usize),now:Instant){
        let bit=match vk {0x10|0xa0|0xa1=>1,0x11|0xa2|0xa3=>2,0x12|0xa4|0xa5=>4,0x5b|0x5c=>8,_=>0};
        if bit==0 {
            if down {
                self.pending=if vk==0x20&&self.held==8{Some((context,now))}else{None};
                self.used=true;
            }
            return;
        }
        if down&&self.held==0{self.used=false;self.context=context;}
        if self.context!=context{self.used=true;self.pending=None;}
        let before=self.held;
        if down{self.held|=bit;}else{self.held&=!bit;}
        let chord=if down{self.held}else{before};
        // Shift、Ctrl+Shift、Alt+Shift；组合中输入过其他键则不视为切换。
        if !self.used&&chord&1!=0&&chord&8==0 {self.pending=Some((context,now));}
    }
    fn allows(&mut self,context:(usize,usize),now:Instant)->bool{
        if self.pending.is_some_and(|(target,at)|target!=context||now.saturating_duration_since(at)>Duration::from_millis(500)){self.pending=None;}
        self.pending.is_some()
    }
    fn clear(&mut self){self.pending=None;self.used=true;}
}

#[cfg(windows)]
mod shortcuts {
    use super::*;
    use std::cell::RefCell;
    use windows::Win32::{Foundation::{HINSTANCE,LPARAM,LRESULT,WPARAM},System::LibraryLoader::GetModuleHandleW,UI::WindowsAndMessaging::*};
    thread_local!{static KEYS:RefCell<SwitchKeys>=RefCell::new(SwitchKeys::default());}
    pub struct Shortcuts(HHOOK);
    impl Shortcuts {
        pub fn start()->Option<Self>{unsafe{
            Some(Self(SetWindowsHookExW(WH_KEYBOARD_LL,Some(on_key),HINSTANCE(GetModuleHandleW(None).ok()?.0),0).ok()?))
        }}
        pub fn allows(&self,context:(usize,usize))->bool{KEYS.with(|keys|keys.borrow_mut().allows(context,Instant::now()))}
        pub fn clear(&self){KEYS.with(|keys|keys.borrow_mut().clear());SWITCH_AT.store(0,std::sync::atomic::Ordering::Relaxed);}
    }
    impl Drop for Shortcuts {
        fn drop(&mut self){unsafe{let _=UnhookWindowsHookEx(self.0);}}
    }
    unsafe extern "system" fn on_key(code:i32,wp:WPARAM,lp:LPARAM)->LRESULT{
        if code==0 {
            let event=&*(lp.0 as *const KBDLLHOOKSTRUCT);
            if !event.flags.contains(LLKHF_INJECTED){
                let foreground=GetForegroundWindow();
                let thread=GetWindowThreadProcessId(foreground,None);
                let mut info=GUITHREADINFO{cbSize:std::mem::size_of::<GUITHREADINFO>()as u32,..Default::default()};
                let focus=if GetGUIThreadInfo(thread,&mut info).is_ok()&&!info.hwndFocus.0.is_null(){info.hwndFocus}else{foreground};
                let down=matches!(wp.0 as u32,WM_KEYDOWN|WM_SYSKEYDOWN);
                KEYS.with(|keys|{
                    let mut keys=keys.borrow_mut();keys.key(event.vkCode,down,(foreground.0 as usize,focus.0 as usize),Instant::now());
                    if keys.pending.is_some(){SWITCH_AT.store(windows::Win32::System::SystemInformation::GetTickCount64(),std::sync::atomic::Ordering::Relaxed);}
                });
            }
        }
        CallNextHookEx(None,code,wp,lp)
    }
}
#[cfg(windows)]
pub use shortcuts::Shortcuts;

#[cfg(test)]
mod tests {
    use super::*;
    fn chinese(native:Option<bool>)->State {State{context:(1,11),layout:0xe0200804,profile:Some((1,2)),native}}
    #[test]
    fn shift_changes_between_chinese_and_english_without_changing_the_input_method(){
        let mut monitor=Monitor::new(Some(chinese(Some(true))));
        assert_eq!(monitor.observe(Some(chinese(Some(false))),true),Some("英语"));
        assert_eq!(monitor.observe(Some(chinese(Some(true))),true),Some("中文"));
        assert_eq!(monitor.observe(Some(chinese(Some(true))),true),None);
    }
    #[test]
    fn ctrl_shift_detects_two_layouts_with_the_same_language_id(){
        let mut monitor=Monitor::new(Some(chinese(Some(true))));
        let next=State{layout:0xe0220804,..chinese(Some(true))};
        assert_eq!(monitor.observe(Some(next),true),Some("中文"));
        assert_eq!(monitor.observe(Some(next),true),None);
    }
    #[test]
    fn ctrl_shift_detects_tsf_profile_changes_even_when_the_layout_is_unchanged(){
        let mut monitor=Monitor::new(Some(chinese(Some(true))));
        assert_eq!(monitor.observe(Some(State{profile:Some((3,4)),..chinese(Some(true))}),true),Some("中文"));
    }
    #[test]
    fn unreadable_samples_and_ordinary_keys_do_not_create_a_language_notice(){
        let mut monitor=Monitor::new(None);
        assert_eq!(monitor.observe(Some(chinese(Some(true))),true),None);
        assert_eq!(monitor.observe(None,true),None);
        assert_eq!(monitor.observe(Some(State::default()),true),None);
        assert_eq!(monitor.observe(Some(chinese(None)),true),None);
        assert_eq!(monitor.observe(Some(chinese(Some(false))),true),Some("英语"));
        for _ in 0..5{assert_eq!(monitor.observe(Some(chinese(Some(false))),true),None);}
    }
    #[test]
    fn switching_to_an_english_keyboard_reports_english(){
        let mut monitor=Monitor::new(Some(chinese(Some(true))));
        assert_eq!(monitor.observe(Some(State{context:(1,11),layout:0x04090409,profile:None,native:None}),true),Some("英语"));
    }
    #[test]
    fn temporary_profile_read_failures_do_not_hide_the_next_profile_switch(){
        let mut monitor=Monitor::new(Some(chinese(Some(true))));
        assert_eq!(monitor.observe(Some(State{profile:None,..chinese(Some(true))}),true),None);
        assert_eq!(monitor.observe(Some(State{profile:Some((3,4)),..chinese(Some(true))}),true),Some("中文"));
    }
    #[test]
    fn changing_app_windows_synchronizes_their_modes_without_notifying(){
        let mut monitor=Monitor::new(Some(chinese(Some(true))));
        let english_window=State{context:(2,22),native:Some(false),..chinese(Some(true))};
        assert_eq!(monitor.observe(Some(english_window),false),None,"切换窗口不应弹出英文切换提示");
        for _ in 0..8{assert_eq!(monitor.observe(Some(english_window),false),None);}
        assert_eq!(monitor.observe(Some(chinese(Some(true))),false),None,"切回中文窗口也应安静同步");
    }
    #[test]
    fn delayed_input_mode_restoration_after_activation_is_silent(){
        let mut monitor=Monitor::new(Some(chinese(Some(true))));
        let window=State{context:(2,22),..chinese(Some(true))};
        assert_eq!(monitor.observe(Some(window),false),None);
        assert_eq!(monitor.observe(Some(window),false),None);
        let restored=State{native:Some(false),profile:Some((3,4)),..window};
        assert_eq!(monitor.observe(Some(restored),false),None,"窗口激活后异步恢复的输入状态不应弹提示");
        for _ in 0..8{assert_eq!(monitor.observe(Some(restored),false),None);}
        assert_eq!(monitor.observe(Some(State{native:Some(true),..restored}),true),Some("中文"),"窗口稳定后仍应识别 Shift 切换");
        assert_eq!(monitor.observe(Some(State{native:Some(true),profile:Some((5,6)),..restored}),true),Some("中文"),"仍应识别 Ctrl+Shift 切换输入法");
    }
    #[test]
    fn changing_focused_controls_does_not_reuse_another_controls_mode(){
        let mut monitor=Monitor::new(Some(chinese(Some(true))));
        assert_eq!(monitor.observe(Some(State{context:(1,12),native:Some(false),..chinese(Some(true))}),false),None);
    }
    #[test]
    fn opening_a_video_without_keys_does_not_report_an_automatic_mode_reset(){
        let mut monitor=Monitor::new(Some(chinese(Some(true))));
        for _ in 0..20{assert_eq!(monitor.observe(Some(chinese(Some(true))),false),None);}
        assert_eq!(monitor.observe(Some(chinese(Some(false))),false),None,"没有切换按键时，同一窗口内的自动输入状态变化也应静默");
        assert_eq!(monitor.observe(Some(chinese(Some(true))),false),None);
    }
    #[test]
    fn delayed_profile_restoration_without_keys_is_silent_after_the_window_settles(){
        let mut monitor=Monitor::new(Some(chinese(Some(true))));
        let window=State{context:(2,22),..chinese(Some(true))};
        for _ in 0..20{assert_eq!(monitor.observe(Some(window),false),None);}
        assert_eq!(monitor.observe(Some(State{profile:Some((3,4)),..window}),false),None);
    }
    #[test]
    fn actual_switch_keys_require_a_matching_state_change_and_are_used_once(){
        let now=Instant::now();let context=(1,11);
        let mut keys=SwitchKeys::default();let mut monitor=Monitor::new(Some(chinese(Some(true))));
        keys.key(0xa0,true,context,now);
        assert_eq!(monitor.observe(Some(chinese(Some(true))),keys.allows(context,now)),None,"只按按键、输入状态未变时不提示");
        keys.key(0xa0,false,context,now);
        assert_eq!(monitor.observe(Some(chinese(Some(false))),keys.allows(context,now)),Some("英语"));
        keys.clear();
        keys.key(0xa0,false,context,now);
        assert_eq!(monitor.observe(Some(chinese(Some(true))),keys.allows(context,now)),None,"一个切换动作只能用于一次提示");
    }
    #[test]
    fn input_shortcuts_are_detected_in_both_modifier_orders(){
        let now=Instant::now();let context=(1,11);
        for (first,second) in [(0xa2,0xa0),(0xa0,0xa2),(0xa4,0xa0),(0x5b,0x20)]{
            let mut keys=SwitchKeys::default();
            keys.key(first,true,context,now);keys.key(second,true,context,now);
            assert!(keys.allows(context,now),"应捕获输入法切换组合键");
        }
    }
    #[test]
    fn uppercase_typing_and_window_shortcuts_cancel_input_switch_intent(){
        let now=Instant::now();let context=(1,11);
        for first in [0xa0,0xa4,0xa2]{
            let mut keys=SwitchKeys::default();
            keys.key(first,true,context,now);keys.key(0x41,true,context,now);keys.key(0x41,false,context,now);keys.key(first,false,context,now);
            assert!(!keys.allows(context,now),"普通组合键不能触发输入切换提示");
        }
        let mut keys=SwitchKeys::default();
        keys.key(0xa2,true,context,now);keys.key(0xa0,true,context,now);keys.key(0x4e,true,context,now);
        keys.key(0xa0,false,context,now);keys.key(0xa2,false,context,now);
        assert!(!keys.allows(context,now),"Ctrl+Shift+字母也不能视为输入法切换");
    }
    #[test]
    fn expired_or_different_window_shortcuts_do_not_authorize_automatic_changes(){
        let now=Instant::now();let context=(1,11);let mut keys=SwitchKeys::default();
        keys.key(0xa0,true,context,now);keys.key(0xa0,false,context,now);
        assert!(!keys.allows(context,now+Duration::from_millis(501)));
        keys.key(0xa0,true,context,now);keys.key(0xa0,false,context,now);
        assert!(!keys.allows((2,22),now));assert!(!keys.allows(context,now));
        keys.key(0xa0,true,context,now);keys.key(0xa0,false,(2,22),now);
        assert!(!keys.allows((2,22),now),"切换窗口后释放旧窗口按键不能重新触发提示");
    }
    #[cfg(windows)]
    #[test]
    fn input_shortcut_listener_can_install_and_remove(){
        let listener=Shortcuts::start().expect("应能注册输入切换按键监听");
        assert!(!listener.allows((1,11)));
        drop(listener);
    }
    #[cfg(windows)]
    #[test]
    #[ignore="按需核对前台输入法的实际状态，不发送按键或修改输入法设置"]
    fn live_input_state_probe(){
        for _ in 0..12{
            let start=std::time::Instant::now();
            if let Some(state)=State::read(){println!("layout={:016X} profile={:?} native={:?} read={:?}",state.layout,state.profile.map(|(a,b)|format!("{a:032X}:{b:032X}")),state.native,start.elapsed());}
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    }
}
