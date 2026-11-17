//! 设置窗口独立的位置与大小记录；仅在关闭或退出程序时落盘。
use eframe::egui::{self,Pos2,Rect,Vec2};
use std::{io::Read,path::PathBuf};

const MIN_SIZE:Vec2=egui::vec2(600.0,450.0);
#[derive(Clone,Copy,Debug,PartialEq,serde::Serialize,serde::Deserialize)]
struct Placement {position:[f32;2],size:[f32;2]}
impl Placement {
    fn valid(self)->bool{self.position.iter().all(|v|v.is_finite()) && self.size.iter().all(|v|v.is_finite() && *v>0.0)}
}
#[derive(Clone,Copy)]
struct Area {rect:Rect,scale:f32}
struct Fit {placement:Placement,scale:f32,minimum:Vec2}
fn fit(saved:Placement,areas:&[Area],frame:Vec2)->Option<Fit>{
    if !saved.valid(){return None;}
    let position=Pos2::from(saved.position);
    let area=areas.iter().find(|area|area.rect.contains(position)).or_else(||areas.first())?;
    if !area.scale.is_finite() || area.scale<=0.0 || !area.rect.is_finite() || !area.rect.is_positive(){return None;}
    let available=(area.rect.size()/area.scale-frame).max(Vec2::splat(1.0));
    let minimum=MIN_SIZE.min(available);let size=Vec2::from(saved.size).max(minimum).min(available);
    let limit=area.rect.max-(size+frame)*area.scale;
    let position=position.clamp(area.rect.min,limit.max(area.rect.min));
    Some(Fit{placement:Placement{position:position.into(),size:size.into()},scale:area.scale,minimum})
}

#[derive(Default)]
pub(crate) struct State {path:Option<PathBuf>,saved:Option<Placement>,current:Option<Placement>,opened:bool}
impl State {
    pub(crate) fn load()->Self{
        if cfg!(test){return Self::default();}
        let path=crate::model::config_dir().join("settings-window.json");
        Self::load_at(path)
    }
    fn load_at(path:PathBuf)->Self{
        let saved=(||{let mut bytes=Vec::new();std::fs::File::open(&path).ok()?.take(4097).read_to_end(&mut bytes).ok()?;
            if bytes.len()>4096{return None;}
            serde_json::from_slice::<Placement>(&bytes).ok().filter(|p|p.valid())})();
        Self{path:Some(path),saved,current:saved,opened:false}
    }
    pub(crate) fn open(&mut self)->bool{let first=!self.opened;self.opened=true;first}
    pub(crate) fn restore(&self,ctx:&egui::Context)->bool{
        if self.current.or(self.saved).is_none() || ctx.viewport_id()==egui::ViewportId::ROOT{return false;}
        let mut areas=work_areas();for area in &mut areas{area.scale*=ctx.zoom_factor();}
        self.restore_in(ctx,&areas)
    }
    fn restore_in(&self,ctx:&egui::Context,areas:&[Area])->bool{
        let Some(saved)=self.current.or(self.saved) else{return false;};let ppp=ctx.pixels_per_point();
        let frame=ctx.input(|i|i.viewport().outer_rect.zip(i.viewport().inner_rect).map(|(outer,inner)|(outer.size()-inner.size()).max(Vec2::ZERO))).unwrap_or(egui::vec2(16.0,40.0));
        let Some(fitted)=fit(saved,areas,frame)else{return false;};
        // 命令按当前窗口倍率换算；恢复目标位于另一块屏幕时仍发送正确的物理尺寸。
        let ratio=fitted.scale/ppp;
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::from(fitted.placement.position)/ppp));
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(fitted.minimum*ratio));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::from(fitted.placement.size)*ratio));
        ctx.request_repaint();true
    }
    pub(crate) fn observe(&mut self,ctx:&egui::Context){
        if ctx.viewport_id()==egui::ViewportId::ROOT{return;}
        let ppp=ctx.pixels_per_point();
        let placement=ctx.input(|i|{let v=i.viewport();if v.minimized==Some(true)||v.maximized==Some(true){return None;}
            Some(Placement{position:(v.outer_rect?.min*ppp).into(),size:v.inner_rect?.size().into()})});
        if let Some(placement)=placement.filter(|p|p.valid()){self.current=Some(placement);}
    }
    pub(crate) fn close(&mut self){if self.opened{self.opened=false;self.persist();}}
    fn persist(&mut self){
        if self.current==self.saved{return;}
        let (Some(path),Some(current))=(&self.path,self.current)else{return;};
        if let Some(parent)=path.parent(){if std::fs::create_dir_all(parent).is_err(){return;}}
        if let Ok(bytes)=serde_json::to_vec(&current){if crate::model::atomic_write(path,&bytes).is_ok(){self.saved=Some(current);}}
    }
}
impl Drop for State {fn drop(&mut self){self.persist();}}

#[cfg(windows)]
fn work_areas()->Vec<Area>{
    use windows::Win32::{Foundation::{BOOL,LPARAM,RECT},Graphics::Gdi::{EnumDisplayMonitors,GetMonitorInfoW,HDC,HMONITOR,MONITORINFO}};
    #[link(name="shcore")]
    extern "system"{fn GetDpiForMonitor(monitor:*mut std::ffi::c_void,kind:i32,x:*mut u32,y:*mut u32)->i32;}
    unsafe extern "system" fn visit(monitor:HMONITOR,_:HDC,_:*mut RECT,data:LPARAM)->BOOL{
        let mut info=MONITORINFO{cbSize:std::mem::size_of::<MONITORINFO>()as u32,..Default::default()};
        if GetMonitorInfoW(monitor,&mut info).as_bool(){
            let(mut x,mut y)=(96,96);if GetDpiForMonitor(monitor.0,0,&mut x,&mut y)<0{x=96;}
            let r=info.rcWork;let areas=&mut *(data.0 as *mut Vec<(bool,Area)>);
            areas.push((info.dwFlags&1!=0,Area{rect:Rect::from_min_max(egui::pos2(r.left as f32,r.top as f32),egui::pos2(r.right as f32,r.bottom as f32)),scale:x as f32/96.0}));
        }
        true.into()
    }
    let mut areas:Vec<(bool,Area)>=Vec::new();unsafe{let _=EnumDisplayMonitors(None,None,Some(visit),LPARAM((&mut areas as *mut Vec<(bool,Area)>)as isize));}
    areas.sort_by_key(|(primary,_)|!*primary);areas.into_iter().map(|(_,area)|area).collect()
}
#[cfg(not(windows))]
fn work_areas()->Vec<Area>{Vec::new()}

#[cfg(test)]
mod tests {
    use super::*;
    fn area(x:f32,y:f32,w:f32,h:f32,scale:f32)->Area{Area{rect:Rect::from_min_size(egui::pos2(x,y),egui::vec2(w,h)),scale}}
    #[test]
    fn settings_window_restoration_fits_current_monitors_and_dpi(){
        let screens=[area(0.0,0.0,1920.0,1040.0,1.0),area(-2560.0,-400.0,2560.0,1400.0,1.5)];
        let saved=Placement{position:[-2200.0,-200.0],size:[900.0,600.0]};
        let restored=fit(saved,&screens,egui::vec2(16.0,40.0)).unwrap();assert_eq!(restored.placement,saved);assert_eq!(restored.scale,1.5);
        let moved=fit(saved,&screens[..1],egui::vec2(16.0,40.0)).unwrap();assert_eq!(moved.placement.position,[0.0,0.0]);
        for scale in [1.0,1.25,1.5,2.0]{
            let screen=area(100.0,50.0,1366.0,700.0,scale);
            let fit=fit(Placement{position:[1400.0,700.0],size:[3000.0,2000.0]},&[screen],egui::vec2(16.0,40.0)).unwrap();
            let p=Pos2::from(fit.placement.position);let end=p+(Vec2::from(fit.placement.size)+egui::vec2(16.0,40.0))*scale;
            assert!(screen.rect.contains(p)&&end.x<=screen.rect.right()+0.1&&end.y<=screen.rect.bottom()+0.1);
        }
        for size in [[f32::NAN,600.0],[900.0,-1.0]]{assert!(fit(Placement{size,..saved},&screens,Vec2::ZERO).is_none());}
    }
    #[test]
    fn settings_window_commands_use_target_screen_scale_without_resizing_the_root(){
        let ctx=egui::Context::default();let id=egui::ViewportId::from_hash_of("settings-restore-test");
        let state=State{path:None,current:Some(Placement{position:[-2000.0,0.0],size:[900.0,600.0]}),..Default::default()};
        let raw=egui::RawInput{viewport_id:id,viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.0),..Default::default()})).collect(),..Default::default()};
        let output=ctx.run(raw,|ctx|assert!(state.restore_in(ctx,&[area(-2560.0,-400.0,2560.0,1400.0,1.5)])));
        let commands=&output.viewport_output[&id].commands;
        assert!(commands.iter().any(|command|matches!(command,egui::ViewportCommand::InnerSize(size) if *size==egui::vec2(1350.0,900.0))));
        assert!(commands.iter().any(|command|matches!(command,egui::ViewportCommand::OuterPosition(position) if *position==egui::pos2(-2000.0,0.0))));
        assert!(!output.viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v|v.commands.iter().any(|command|matches!(command,egui::ViewportCommand::InnerSize(_)))));
    }
    #[test]
    fn settings_window_records_normal_geometry_and_writes_only_at_close(){
        let path=std::env::temp_dir().join(format!("daozai-settings-window-{}.json",std::process::id()));
        let mut state=State{path:Some(path.clone()),..Default::default()};assert!(state.open());assert!(!state.open());
        let ctx=egui::Context::default();let id=egui::ViewportId::from_hash_of("settings-memory-test");
        let observe=|state:&mut State,minimized,maximized|{
            let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(800.0,600.0))),viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.5),inner_rect:Some(Rect::from_min_size(egui::pos2(0.0,0.0),egui::vec2(800.0,600.0))),outer_rect:Some(Rect::from_min_size(egui::pos2(-1000.0,80.0),egui::vec2(816.0,640.0))),minimized:Some(minimized),maximized:Some(maximized),..Default::default()})).collect(),..Default::default()};
            let _=ctx.run(raw,|ctx|state.observe(ctx));
        };
        observe(&mut state,false,false);let normal=state.current;assert_eq!(normal.unwrap().position,[-1500.0,120.0]);assert!(!path.exists());
        observe(&mut state,true,false);observe(&mut state,false,true);assert_eq!(state.current,normal);
        state.close();let bytes=std::fs::read(&path).unwrap();assert_eq!(serde_json::from_slice::<Placement>(&bytes).unwrap(),normal.unwrap());
        assert_eq!(State::load_at(path.clone()).saved,normal,"重启后应读回原始普通窗口位置与大小");
        state.close();assert_eq!(std::fs::read(&path).unwrap(),bytes);std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn invalid_window_records_fall_back_without_rewriting_them(){
        let path=std::env::temp_dir().join(format!("daozai-invalid-window-{}.json",std::process::id()));
        for bytes in [b"invalid".to_vec(),b"{\"position\":[0,0],\"size\":[-1,560]}".to_vec(),vec![b' ';4097]]{
            std::fs::write(&path,&bytes).unwrap();let state=State::load_at(path.clone());assert!(state.saved.is_none());drop(state);
            assert_eq!(std::fs::read(&path).unwrap(),bytes);
        }
        std::fs::remove_file(path).unwrap();
    }
}
