//! 设置窗口独立外观，不改变岛体的绘制样式。
use eframe::egui::{self,Color32};

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub enum SettingsTheme { #[default] System, Light, Dark }
impl SettingsTheme {
    pub fn key(self)->&'static str {match self {Self::System=>"system",Self::Light=>"light",Self::Dark=>"dark"}}
    pub fn parse(value:&str)->Self {match value {"light"=>Self::Light,"dark"=>Self::Dark,_=>Self::System}}
    pub fn dark(self,system_dark:bool)->bool {match self {Self::System=>system_dark,Self::Light=>false,Self::Dark=>true}}
}

pub fn system_dark(ctx:&egui::Context)->bool {
    let id=egui::Id::new("settings-system-theme");
    if let Some((at,dark))=ctx.data(|d|d.get_temp::<(std::time::Instant,bool)>(id)) {
        if at.elapsed()<std::time::Duration::from_secs(1){return dark;}
    }
    #[cfg(windows)]
    let dark={
        use windows::{core::w,Win32::System::Registry::*};
        let mut value=1u32;let mut size=4u32;
        unsafe { let _=RegGetValueW(HKEY_CURRENT_USER,w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),RRF_RT_REG_DWORD,None,Some((&mut value as *mut u32).cast()),Some(&mut size)); }
        value==0
    };
    #[cfg(not(windows))]
    let dark=ctx.input(|i|i.raw.system_theme)==Some(egui::Theme::Dark);
    ctx.data_mut(|d|d.insert_temp(id,(std::time::Instant::now(),dark)));
    dark
}

pub fn color(ctx:&egui::Context,dark:Color32,light:Color32)->Color32 {
    if ctx.style().visuals.dark_mode {dark}else{light}
}
