//! 按显示屏的物理坐标居中岛体，透明画布的额外宽度不参与居中。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds { pub left:i32, pub top:i32, pub right:i32, pub bottom:i32 }

#[derive(Clone, Debug)]
pub struct Display { pub id:String, pub label:String, pub bounds:Bounds,
    /// 工作区顶边：顶栏类软件或置顶的任务栏登记后会比 `bounds.top` 更靠下。
    pub work_top:i32, pub primary:bool }

/// 岛体摆放用的屏幕范围：开启「避开顶栏」时顶边换成工作区顶边。
/// 只认屏幕上方三分之一以内的占用，异常的工作区不会把岛推到屏幕中间。
pub fn placement_bounds(display:&Display, avoid_top_bar:bool)->Bounds {
    let mut b=display.bounds;
    let reserved=display.work_top-b.top;
    if avoid_top_bar && reserved>0 && reserved<=(b.bottom-b.top)/3 {b.top=display.work_top;}
    b
}

pub fn selected<'a>(displays:&'a [Display],id:&str)->Option<&'a Display>{
    displays.iter().find(|d|!id.is_empty()&&d.id==id)
        .or_else(||displays.iter().find(|d|d.primary)).or_else(||displays.first())
}

#[cfg(windows)]
pub fn connected()->Vec<Display>{
    use windows::{core::PCWSTR,Win32::{Foundation::{BOOL,LPARAM,RECT},Graphics::Gdi::*,UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME}};
    fn text(value:&[u16])->String {String::from_utf16_lossy(&value[..value.iter().position(|c|*c==0).unwrap_or(value.len())])}
    unsafe extern "system" fn collect(monitor:HMONITOR,_:HDC,_:*mut RECT,data:LPARAM)->BOOL {
        let mut info=MONITORINFOEXW::default();info.monitorInfo.cbSize=std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor,&mut info.monitorInfo).as_bool(){
            let device=text(&info.szDevice);
            let mut details=DISPLAY_DEVICEW{cb:std::mem::size_of::<DISPLAY_DEVICEW>() as u32,..Default::default()};
            let found=EnumDisplayDevicesW(PCWSTR(info.szDevice.as_ptr()),0,&mut details,EDD_GET_DEVICE_INTERFACE_NAME).as_bool();
            let stable_id=if found{text(&details.DeviceID)}else{String::new()};
            let name=if found{text(&details.DeviceString)}else{String::new()};
            let number=device.trim_start_matches(r"\\.\DISPLAY");let r=info.monitorInfo.rcMonitor;
            let primary=info.monitorInfo.dwFlags&1!=0;
            let label=format!("显示器 {number}{} · {} × {}{}",if name.is_empty(){String::new()}else{format!(" · {name}")},r.right-r.left,r.bottom-r.top,if primary{" · 主屏"}else{""});
            let displays=&mut *(data.0 as *mut Vec<Display>);
            displays.push(Display{id:if stable_id.is_empty(){device}else{stable_id},label,bounds:Bounds{left:r.left,top:r.top,right:r.right,bottom:r.bottom},work_top:info.monitorInfo.rcWork.top,primary});
        }
        BOOL(1)
    }
    let mut displays:Vec<Display>=Vec::new();
    unsafe {let _=EnumDisplayMonitors(None,None,Some(collect),LPARAM(&mut displays as *mut Vec<Display> as isize));}
    displays.sort_by(|a,b|a.label.cmp(&b.label));displays
}

#[cfg(not(windows))]
pub fn connected()->Vec<Display>{Vec::new()}

pub fn island_origin(m:Bounds, island_width:f32, scale:f32)->Option<(i32,i32)> {
    if m.right<=m.left || m.bottom<=m.top || !scale.is_finite() || scale<=0.0 || !island_width.is_finite() || island_width<=0.0 {return None;}
    let width=(island_width as f64*scale as f64).round();
    let x=(m.left as f64+(m.right as f64-m.left as f64-width)/2.0).round();
    Some((x.clamp(i32::MIN as f64,i32::MAX as f64) as i32,m.top))
}

/// 岛体可见内容（展开面板、肩部与阴影）距中心的最大半宽，逻辑点。
/// 水平偏移以此为界，保证展开后面板仍完整留在屏幕内。
pub const ISLAND_HALF_EXTENT: f32 = 240.0;

/// 岛体中心能离开屏幕中线的最大距离（物理像素）。屏幕太窄时为 0，只能居中。
pub fn island_travel(m:Bounds, scale:f32)->f64 {
    if m.right<=m.left || !scale.is_finite() || scale<=0.0 {return 0.0;}
    ((m.right as f64-m.left as f64)/2.0-ISLAND_HALF_EXTENT as f64*scale as f64).max(0.0)
}

/// 按水平偏移定位：`offset` 为 -1（最左）..0（居中）..1（最右），按可移动范围的比例换算，
/// 所以换屏、改系统缩放或调整岛体大小后仍保持相同的相对位置。
pub fn island_origin_offset(m:Bounds, island_width:f32, scale:f32, offset:f32)->Option<(i32,i32)> {
    let (x,y)=island_origin(m,island_width,scale)?;
    let offset=if offset.is_finite(){offset.clamp(-1.0,1.0)}else{0.0};
    let shift=(island_travel(m,scale)*offset as f64).round();
    Some(((x as f64+shift).clamp(i32::MIN as f64,i32::MAX as f64) as i32,y))
}

/// 左右停靠：画布贴着屏幕左 / 右边，竖直方向按 `offset`（-1 最上 … 0 居中 … 1 最下）移动，
/// 可移动范围保证展开面板始终完整留在屏幕内。返回画布左上角（物理像素）。
pub fn side_origin(m:Bounds, dock:crate::model::Dock, canvas:(f32,f32), half_extent:f32, scale:f32, offset:f32)->Option<(i32,i32)> {
    if m.right<=m.left || m.bottom<=m.top || !scale.is_finite() || scale<=0.0 {return None;}
    let (w,h)=((canvas.0*scale) as f64,(canvas.1*scale) as f64);
    let offset=if offset.is_finite(){offset.clamp(-1.0,1.0)}else{0.0} as f64;
    let travel=((m.bottom as f64-m.top as f64)/2.0-(half_extent*scale) as f64).max(0.0);
    let cy=(m.top as f64+m.bottom as f64)/2.0+travel*offset;
    let x=match dock {crate::model::Dock::Right=>m.right as f64-w,_=>m.left as f64};
    Some((x.round() as i32,(cy-h/2.0).round() as i32))
}

/// 当前站位下，OLED 像素偏移往两侧各还能走多少物理像素。
///
/// `half_extent` 是展开面板相对岛体中心的最大半宽（顶部停靠）或半高（左右停靠）。
/// 留出的余量保证展开面板仍完整留在屏幕内：位置拉到最边、或屏幕窄到没有余量时，
/// 返回值会很小甚至为 0，调用方据此少挪或不挪。返回值是 (向小坐标, 向大坐标)。
pub fn shift_slack(m:Bounds, scale:f32, half_extent:f32, offset:f32, dock:crate::model::Dock)->(i32,i32) {
    if m.right<=m.left || m.bottom<=m.top || !scale.is_finite() || scale<=0.0 || !half_extent.is_finite() || half_extent<=0.0 {return (0,0);}
    let offset=if offset.is_finite() {offset.clamp(-1.0,1.0)} else {0.0} as f64;
    let half=half_extent as f64*scale as f64;
    let (low,high)=if dock.is_side() {(m.top as f64,m.bottom as f64)} else {(m.left as f64,m.right as f64)};
    // 与 island_travel / side_origin 用同一个可移动范围，余量才不会和实际站位对不上。
    let travel=((high-low)/2.0-half).max(0.0);
    let center=(low+high)/2.0+travel*offset;
    let back=(center-half-low).max(0.0);
    let forth=(high-center-half).max(0.0);
    (back.floor() as i32,forth.floor() as i32)
}

#[cfg(test)]
mod shift_slack_tests {
    use super::*;
    use crate::model::Dock;

    fn slack(bounds:Bounds,dock:Dock,scale:f32,offset:f32)->(i32,i32){
        let half=if dock.is_side() {crate::side_dock::SIDE_HALF_EXTENT} else {ISLAND_HALF_EXTENT};
        shift_slack(bounds,scale,half,offset,dock)
    }

    fn limits(m:Bounds,dock:Dock)->(f64,f64){
        if dock.is_side() {(m.top as f64,m.bottom as f64)} else {(m.left as f64,m.right as f64)}
    }

    #[test]
    fn centered_island_has_the_same_room_on_both_sides(){
        let m=Bounds{left:0,top:0,right:1920,bottom:1080};
        for scale in [1.0,1.25,1.5,2.0] {
            let (back,forth)=slack(m,Dock::Top,scale,0.0);
            assert_eq!(back,forth,"居中时两侧余量相同：{scale}");
            assert_eq!(back,island_travel(m,scale).floor() as i32,"余量应等于可移动范围");
        }
    }

    #[test]
    fn pushing_the_island_to_the_edge_leaves_no_room_outwards(){
        let m=Bounds{left:0,top:0,right:1920,bottom:1080};
        let (back,forth)=slack(m,Dock::Top,1.0,1.0);
        assert_eq!(forth,0,"拉到最右后不能再往右挪，否则展开面板会被裁掉");
        let (back,forth)=slack(m,Dock::Top,1.0,-1.0);
        assert_eq!(back,0,"拉到最左后不能再往左挪");
        assert!(forth>0);
    }

    #[test]
    fn narrow_screens_have_no_room_at_all(){
        assert_eq!(slack(Bounds{left:0,top:0,right:480,bottom:900},Dock::Top,1.0,0.0),(0,0),"宽度不够放展开面板时不挪");
        assert_eq!(slack(Bounds{left:0,top:0,right:1920,bottom:300},Dock::Left,1.0,0.0),(0,0),"高度不够放展开面板时不挪");
    }

    #[test]
    fn side_docks_measure_the_vertical_axis(){
        let m=Bounds{left:0,top:0,right:1920,bottom:1080};
        for dock in [Dock::Left,Dock::Right] {
            let (back,forth)=slack(m,dock,1.0,0.0);
            assert_eq!((back,forth),((540.0-crate::side_dock::SIDE_HALF_EXTENT) as i32,(540.0-crate::side_dock::SIDE_HALF_EXTENT) as i32));
            assert_eq!(slack(m,dock,1.0,1.0).1,0,"贴到最下后不能再往下挪");
        }
    }

    #[test]
    fn any_offset_and_scale_keeps_the_panel_on_screen(){
        let monitors=[
            Bounds{left:0,top:0,right:2560,bottom:1440},
            Bounds{left:-2560,top:0,right:0,bottom:1440},
            Bounds{left:0,top:0,right:3840,bottom:2160},
            Bounds{left:0,top:0,right:1366,bottom:768},
        ];
        for m in monitors {
            for dock in Dock::ALL {
                for scale in [1.0,1.25,1.5,2.0] {
                    for offset in [-1.0,-0.6,0.0,0.6,1.0] {
                        let (back,forth)=slack(m,dock,scale,offset);
                        assert!(back>=0 && forth>=0,"余量不能为负：{m:?} {dock:?} {scale} {offset}");
                        let half=if dock.is_side() {crate::side_dock::SIDE_HALF_EXTENT} else {ISLAND_HALF_EXTENT} as f64*scale as f64;
                        let (low,high)=limits(m,dock);
                        let travel=((high-low)/2.0-half).max(0.0);
                        let center=(low+high)/2.0+travel*offset as f64;
                        assert!(center-half-back as f64>=low-1.0,"向小坐标挪不能越界：{m:?} {dock:?} {scale} {offset}");
                        assert!(center+half+forth as f64<=high+1.0,"向大坐标挪不能越界");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod top_bar_tests {
    use super::*;
    fn display(bounds:Bounds,work_top:i32)->Display {Display{id:"d".into(),label:"d".into(),bounds,work_top,primary:true}}
    #[test]
    fn avoiding_the_top_bar_starts_below_it_and_keeps_other_edges(){
        for (m,bar) in [(Bounds{left:0,top:0,right:1920,bottom:1080},28),(Bounds{left:-2560,top:-400,right:0,bottom:1040},42)] {
            let d=display(m,m.top+bar);
            assert_eq!(placement_bounds(&d,true),Bounds{top:m.top+bar,..m});
            assert_eq!(placement_bounds(&d,false),m,"关闭后回到屏幕顶边");
            let (x,y)=island_origin_offset(placement_bounds(&d,true),640.0,1.5,0.3).unwrap();
            assert_eq!((x,y-bar),island_origin_offset(m,640.0,1.5,0.3).unwrap(),"只下移，不改变水平位置");
        }
    }
    #[test]
    fn missing_or_unreasonable_work_areas_are_ignored(){
        let m=Bounds{left:0,top:0,right:1920,bottom:1080};
        for work_top in [0,-20,361,1080,2000] {assert_eq!(placement_bounds(&display(m,work_top),true),m,"{work_top}");}
        assert_eq!(placement_bounds(&display(m,360),true).top,360);
    }
}

#[cfg(test)]
mod side_tests {
    use super::*;
    use crate::model::Dock;
    #[test]
    fn side_canvas_hugs_the_edge_and_keeps_the_panel_on_screen(){
        for (m,scale) in [(Bounds{left:0,top:0,right:1920,bottom:1080},1.0),(Bounds{left:-2560,top:-400,right:0,bottom:1040},1.5)] {
            let canvas=(500.0,600.0);
            let (x,_)=side_origin(m,Dock::Left,canvas,150.0,scale,0.0).unwrap();assert_eq!(x,m.left);
            let (x,_)=side_origin(m,Dock::Right,canvas,150.0,scale,0.0).unwrap();assert_eq!(x+(500.0*scale) as i32,m.right);
            for offset in [-1.0,0.0,1.0,7.0] {
                let (_,y)=side_origin(m,Dock::Left,canvas,150.0,scale,offset).unwrap();
                let cy=y as f64+300.0*scale as f64;
                assert!(cy-150.0*scale as f64>=m.top as f64-1.0 && cy+150.0*scale as f64<=m.bottom as f64+1.0,"{m:?} {offset}");
            }
            let (_,center)=side_origin(m,Dock::Left,canvas,150.0,scale,0.0).unwrap();
            assert_eq!(center as f64+300.0*scale as f64,(m.top+m.bottom) as f64/2.0);
        }
        assert!(side_origin(Bounds{left:0,top:0,right:0,bottom:0},Dock::Left,(500.0,600.0),150.0,1.0,0.0).is_none());
    }
}

#[cfg(test)]
mod offset_tests {
    use super::*;
    #[test]
    fn centered_offset_matches_the_original_origin(){
        let m=Bounds{left:-2560,top:-400,right:0,bottom:1040};
        for scale in [1.0,1.25,1.5,2.0] {assert_eq!(island_origin_offset(m,640.0,scale,0.0),island_origin(m,640.0,scale));}
    }
    #[test]
    fn extreme_offsets_keep_the_expanded_panel_on_screen(){
        for (m,scale) in [
            (Bounds{left:0,top:0,right:1920,bottom:1080},1.0),
            (Bounds{left:-2560,top:-400,right:0,bottom:1040},1.5),
            (Bounds{left:1920,top:200,right:3840,bottom:1280},1.25*1.4),
        ] {
            for offset in [-1.0,1.0,-5.0,5.0] {
                let (x,_)=island_origin_offset(m,640.0,scale,offset).unwrap();
                let center=x as f64+320.0*scale as f64;
                let half=(ISLAND_HALF_EXTENT*scale) as f64;
                assert!(center-half>=m.left as f64-1.0 && center+half<=m.right as f64+1.0,"{m:?} {scale} {offset}");
            }
            let left=island_origin_offset(m,640.0,scale,-1.0).unwrap().0;
            let right=island_origin_offset(m,640.0,scale,1.0).unwrap().0;
            let center=island_origin(m,640.0,scale).unwrap().0;
            assert!(left<center && center<right);
            assert!(((center-left)-(right-center)).abs()<=1,"左右对称");
        }
    }
    #[test]
    fn narrow_screens_and_bad_values_stay_centered(){
        let narrow=Bounds{left:0,top:0,right:400,bottom:800};
        assert_eq!(island_travel(narrow,1.5),0.0);
        assert_eq!(island_origin_offset(narrow,640.0,1.5,1.0),island_origin(narrow,640.0,1.5));
        let m=Bounds{left:0,top:0,right:1920,bottom:1080};
        assert_eq!(island_origin_offset(m,640.0,1.0,f32::NAN),island_origin(m,640.0,1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test] fn connected_displays_have_valid_bounds_and_distinct_ids(){
        let displays=connected();
        for (i,display) in displays.iter().enumerate(){
            println!("{}：{:?}",display.label,display.bounds);
            assert!(!display.id.is_empty());assert!(island_origin(display.bounds,640.0,1.0).is_some());
            assert!(!displays[..i].iter().any(|d|d.id==display.id));
        }
        if !displays.is_empty(){assert_eq!(displays.iter().filter(|d|d.primary).count(),1);}
    }
    #[test] fn display_selection_survives_reordering_disconnect_and_reconnect(){
        let primary=Display{id:"internal".into(),label:"内屏".into(),bounds:Bounds{left:0,top:0,right:1920,bottom:1080},work_top:0,primary:true};
        let external=Display{id:"external".into(),label:"外屏".into(),bounds:Bounds{left:-2560,top:-400,right:0,bottom:1040},work_top:-400,primary:false};
        for displays in [vec![primary.clone(),external.clone()],vec![external.clone(),primary.clone()]] {
            assert_eq!(selected(&displays,"").unwrap().id,"internal");
            let target=selected(&displays,"external").unwrap();assert_eq!(target.id,"external");
            assert_eq!(island_origin(target.bounds,640.0,1.5),Some((-1760,-400)));
        }
        assert_eq!(selected(&[primary.clone()],"external").unwrap().id,"internal");
        assert_eq!(selected(&[primary,external],"external").unwrap().id,"external");
        assert!(selected(&[],"external").is_none());
    }
    #[test] fn real_origins_and_dpi_center_only_the_island(){
        for (m,scale,want) in [
            (Bounds{left:0,top:0,right:1920,bottom:1080},1.0,(640,0)),
            (Bounds{left:-2560,top:-400,right:0,bottom:1040},1.5,(-1760,-400)),
            (Bounds{left:300,top:-1440,right:2860,bottom:0},2.0,(940,-1440)),
            (Bounds{left:1920,top:200,right:3840,bottom:1280},1.25,(2480,200)),
        ] {assert_eq!(island_origin(m,640.0,scale),Some(want));}
    }
    #[test] fn changing_primary_unplugging_and_dpi_recompute_without_stale_state(){
        let a=Bounds{left:0,top:0,right:1920,bottom:1080};
        let b=Bounds{left:-2560,top:0,right:0,bottom:1440};
        assert_ne!(island_origin(a,640.0,1.0),island_origin(b,640.0,1.5));
        assert_eq!(island_origin(a,640.0,1.0),Some((640,0)));
        assert_eq!(island_origin(a,640.0,2.0),Some((320,0)));
        assert_eq!(island_origin(a,640.0,f32::NAN),None);
    }
}

/// 只有岛仔所在屏幕的前台全屏窗口才触发隐藏。
pub fn hide_for_fullscreen(enabled:bool,fullscreen:Option<Bounds>,island:Option<Bounds>)->bool {
    enabled && fullscreen.is_some() && fullscreen==island
}

#[cfg(test)]
mod fullscreen_tests {
    use super::*;
    #[test]
    fn fullscreen_on_another_monitor_keeps_island_visible(){
        let primary=Bounds{left:0,top:0,right:1920,bottom:1080};
        for secondary in [Bounds{left:-2560,top:0,right:0,bottom:1440},Bounds{left:1920,top:-200,right:4480,bottom:1240},Bounds{left:0,top:-1440,right:2560,bottom:0}] {
            assert!(!hide_for_fullscreen(true,Some(secondary),Some(primary)));
            assert!(!hide_for_fullscreen(true,Some(primary),Some(secondary)));
            assert!(hide_for_fullscreen(true,Some(secondary),Some(secondary)));
        }
        assert!(hide_for_fullscreen(true,Some(primary),Some(primary)));
        assert!(!hide_for_fullscreen(false,Some(primary),Some(primary)));
        assert!(!hide_for_fullscreen(true,None,Some(primary)));
        assert!(!hide_for_fullscreen(true,None,None));
        assert!(!hide_for_fullscreen(true,Some(primary),None));
    }
}
