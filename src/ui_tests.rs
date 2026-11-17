//! 无窗口交互测试：直接向 egui 注入输入事件，验证悬停展开、切页、按钮。
use crate::app::{App, Env, Options, PH, PW, WIN_H, WIN_W};
#[path="messenger/ui_tests.rs"]
mod enterprise_messenger;
#[path="calendar/ui_compatibility_tests.rs"]
mod calendar_compatibility;
#[path="doubao_ui_tests.rs"]
mod doubao;
#[path="weather_source_ui_tests.rs"]
mod weather_source;
#[path="lyrics_switch_ui_tests.rs"]
mod lyrics_switch;
#[path="health_reminder_ui_tests.rs"]
mod health_reminder;
use eframe::egui::{self, Pos2, Rect, Vec2};
use std::sync::Mutex;

pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn gemini_connect_form_opens_default_browser_without_claiming_connection(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dark in [true,false]{
        let mut h=H::new();h.ctx.set_visuals(if dark{egui::Visuals::dark()}else{egui::Visuals::light()});
        h.app.settings_open=true;h.app.quota.provider=crate::quota::Provider::Gemini;h.app.quota.connect_open=true;
        let mut raster=crate::preview_raster::Raster::new(640,760);
        for _ in 0..4{
            let out=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(640.0,760.0))),..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|h.app.quota_settings(ui));});
            raster.update_textures(&out.textures_delta);h.shapes=out.shapes;
        }
        let texts=h.shapes.iter().filter_map(|s|if let egui::Shape::Text(t)=&s.shape{Some(t.galley.text())}else{None}).collect::<Vec<_>>();
        assert!(texts.contains(&"打开默认浏览器登录"),"Gemini 应在系统默认浏览器中登录");
        assert!(texts.contains(&"连接已登录的 Chrome"),"Gemini 应支持本人授权连接现有 Chrome 会话");
        assert!(texts.iter().any(|s|s.contains("chrome://inspect/#remote-debugging")),"首次连接应提供 Chrome 原生设置入口");
        assert!(!texts.iter().any(|s|s.contains("自动连接")),"打开官网不能表示岛仔已获得登录会话");
        assert!(!texts.iter().any(|s|s.contains("Kimi 浏览器扩展")),"Gemini 连接不应要求额外安装扩展");
        assert!(texts.contains(&"Google 邮箱"));
        let original=h.app.quota.config.clone();
        let point=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()=="打开默认浏览器登录"=>Some(t.visual_bounding_rect().center()),_=>None}).unwrap();
        for pressed in [true,false]{
            let _=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(640.0,760.0))),events:vec![egui::Event::PointerMoved(point),egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}],..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|h.app.quota_settings(ui));});
        }
        assert!(h.app.pending.contains(&crate::app::Cmd::OpenUrl("https://gemini.google.com/usage".into())));
        assert!(h.app.quota.gemini_connecting.is_none());assert_eq!(h.app.quota.config,original);
        if let Some(dest)=std::env::var_os("DAOZAI_GEMINI_PREVIEW_DIR"){
            let dest=std::path::PathBuf::from(dest);std::fs::create_dir_all(&dest).unwrap();
            raster.buf.fill([0.08,0.09,0.12,1.0]);raster.draw(&h.ctx.tessellate(h.shapes.clone(),1.0),1.0);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            image::save_buffer(dest.join(if dark{"dark.png"}else{"light.png"}),&rgba,640,760,image::ColorType::Rgba8).unwrap();
        }
    }
}

#[test]
fn gemini_closed_dialogs_discard_completed_logins_without_saving(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for close_settings in [false,true]{
        let mut h=H::new();let original=h.app.quota.config.clone();h.app.settings_open=true;h.app.quota.connect_open=true;
        let (connect,tx)=crate::quota::gemini::Connect::pending();h.app.quota.gemini_connecting=Some(connect);
        tx.send(Ok(crate::quota::gemini::simulated_connected())).unwrap();
        if close_settings{h.app.settings_open=false;}else{h.app.quota_close_connection();}
        h.app.quota_tick();assert!(h.app.quota.gemini_connecting.is_none());assert_eq!(h.app.quota.config,original);
        assert!(h.app.quota.rows.is_empty());assert!(!crate::model::config_dir().join("credentials").exists(),"关闭后不能保存晚到的会话");
    }
}

#[test]
fn weather_content_tap_switches_both_ways_with_a_short_transition(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for (x,y) in [(100.0,75.0),(270.0,82.0),(314.0,178.0)] {
        let mut h=H::new();h.app.settings.pet=false;
        h.app.weather.snapshot=Some(crate::weather::Snapshot{place:"杭州".into(),temp:19.0,sunrise:Some("06:52".into()),sunset:Some("17:21".into()),..Default::default()});
        h.app.show_page(8);let p=H::local(x,y);h.hover(p,1.0);h.click(p);
        assert!(h.app.weather.solar_open,"天气内容区 ({x},{y}) 应可以轻点切换");
        h.hover(p,0.05);
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains("返回天气")&&t.fallback_color.a()>0&&t.fallback_color.a()<255)),"切换时应有短暂淡入动效");
        h.hover(p,0.3);h.click(p);assert!(!h.app.weather.solar_open,"日照内容区也可以轻点返回");
        h.hover(p,0.3);
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="19°"&&t.fallback_color.a()==255)),"动效完成后应恢复清晰的天气概览");
    }
}

#[test]
fn weather_content_tap_keeps_other_controls_independent(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for (x,y,target) in [(375.0,crate::app::WEATHER_H-7.0,8),(394.0,18.0,8),(367.0,18.0,0),(398.0,88.0,0),(20.0,88.0,7)] {
        let mut h=H::new();h.app.settings.pet=false;
        h.media=crate::smtc::NowPlaying{source:"cloudmusic.exe".into(),title:"歌曲".into(),artist:"歌手".into(),playing:true,..Default::default()};
        h.app.weather.snapshot=Some(crate::weather::Snapshot{place:"杭州".into(),temp:19.0,..Default::default()});
        h.app.show_page(8);h.hover(H::local(250.0,65.0),1.0);h.click(H::local(x,y));
        assert!(!h.app.weather.solar_open,"操作 ({x},{y}) 不应触发日照切换");
        assert_eq!(h.app.page(),target,"操作 ({x},{y}) 的页面跳转应保持不变");
        if y>200.0 {assert!(h.cmds.contains(&crate::app::Cmd::OpenUrl("https://open-meteo.com/".into())));}
        if x==394.0 {assert!(h.app.settings_open);}
    }
}

#[test]
fn weather_week_shows_all_seven_days(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.weather.snapshot=Some(crate::weather::Snapshot{place:"杭州".into(),temp:19.0,
        days:["今天","明天","周五","周六","周日","周一","周二"].into_iter().map(|label|crate::weather::Day{label:label.into(),high:23.0,low:15.0,..Default::default()}).collect(),
        at:chrono::Utc::now().timestamp(),..Default::default()});
    h.app.show_page(8);h.hover(H::local(250.0,65.0),1.0);
    for label in ["今天","明天","周五","周六","周日","周一","周二"] {
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==label)),"七天预报缺少 {label}");
    }
}

#[test]
fn weather_week_keeps_each_forecast_date_visible(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.weather.snapshot=Some(crate::weather::parse("杭州",&serde_json::json!({"current":{"temperature_2m":19},
        "daily":{"time":["2026-11-28","2026-11-29","2026-11-30","2026-12-01","2026-12-02","2026-12-03","2026-12-04"]}})).unwrap());
    h.app.show_page(8);h.hover(H::local(250.0,65.0),1.0);
    for date in ["11/28","11/29","11/30","12/01","12/02","12/03","12/04"] {
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==date)),"七天预报缺少日期 {date}");
    }
    for s in &h.shapes {if let egui::Shape::Text(t)=&s.shape {assert!(s.clip_rect.contains_rect(t.visual_bounding_rect()),"日期与预报文字不应裁切：{}",t.galley.text());}}
}

#[test]
fn weather_today_opens_solar_details_and_returns_to_forecast(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.weather.snapshot=Some(crate::weather::Snapshot{place:"杭州".into(),temp:19.0,sunrise:Some("06:52".into()),sunset:Some("17:21".into()),sun_times:None,..Default::default()});
    h.app.show_page(8);h.hover(H::local(250.0,65.0),1.0);h.click(H::local(100.0,75.0));h.hover(H::local(100.0,75.0),0.3);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains("返回天气"))),"点击当天区域后应进入日出日落视图");
    for phrase in ["日出","日落","06:52","17:21","日照进度待更新"] {
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains(phrase))),"旧缓存日照视图缺少 {phrase}");
    }
    assert!(!h.app.weather.loading,"浏览日照视图不应额外发起网络请求");
    h.click(H::local(80.0,44.0));
    assert!(!h.app.weather.solar_open);
    h.hover(H::local(80.0,44.0),0.3);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="日出 06:52")),"返回后应显示天气视图的日照入口");
}

#[test]
fn weather_side_bar_shows_a_clickable_temperature_on_both_edges(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dock in [crate::model::Dock::Left,crate::model::Dock::Right] {
        let mut h=H::new();h.app.settings.dock=dock;
        h.app.weather.snapshot=Some(crate::weather::Snapshot{place:"杭州".into(),temp:-12.0,code:0,day:true,..Default::default()});
        for _ in 0..60{side_frame(&mut h,Some(Pos2::new(250.0,580.0)));}
        let response=h.ctx.read_response(egui::Id::new(("card",true,"weather-side"))).expect("两侧都应提供常驻天气入口");
        let temp=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()=="12°"=>Some((s.clip_rect,t.visual_bounding_rect())),_=>None}).expect("负温度应完整显示");
        assert!(temp.0.contains_rect(temp.1),"{dock:?} 负温度不应裁切");
        let click=response.rect.center();
        side_frame(&mut h,Some(click));
        for pressed in [true,false] {
            h.t+=1.0/60.0;
            let env=Env{t:h.t,clock:"09:41:00".into(),cursor:Some(click),..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(500.0,600.0))),time:Some(h.t),focused:true,
                events:vec![egui::Event::PointerButton{pos:click,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}],..Default::default()};
            let _=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{h.app.frame(ui,ui.max_rect(),&env);});});
        }
        assert_eq!(h.app.page(),8,"{dock:?} 点击侧边温度应进入天气页");
        assert!(!h.app.weather.solar_open);
        h.app.settings.visible_pages[8]=false;
        assert!(!h.app.side_items().contains(&crate::side_dock::SideItem::Weather));
        h.app.settings.visible_pages[8]=true;h.app.settings.minimal=true;
        assert!(!h.app.side_items().contains(&crate::side_dock::SideItem::Weather));
    }
}

#[test]
fn weather_solar_view_preserves_readable_times_across_skins_and_scales(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    use chrono::TimeZone;
    let now=chrono_tz::Asia::Shanghai.with_ymd_and_hms(2026,10,28,9,41,0).single().unwrap().with_timezone(&chrono::Local);
    for skin in crate::model::Skin::ALL {for scale in [1.0,1.25,1.5] {
        let mut h=H::new();h.app.settings.skin=skin;h.ctx.set_pixels_per_point(scale);
        h.app.weather.snapshot=Some(crate::weather::parse("杭州",&serde_json::json!({"timezone":"Asia/Shanghai","current":{"temperature_2m":19},
            "daily":{"time":["2026-10-28"],"sunrise":["2026-10-28T06:52"],"sunset":["2026-10-28T17:21"]}})).unwrap());
        h.app.weather.solar_open=true;h.app.show_page(8);
        for _ in 0..60 {
            h.t+=1.0/60.0;
            let cursor=H::local(180.0,185.0);
            let env=Env{t:h.t,clock:now.format("%H:%M:%S").to_string(),today:now.format("%Y-%m-%d").to_string(),date:now.format("%m月%d日").to_string(),cursor:Some(cursor),bt_ready:true,..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(h.t),focused:true,
                events:vec![egui::Event::PointerMoved(cursor)],..Default::default()};
            h.shapes=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{
                h.app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,WIN_H)),&env);
            });}).shapes;
        }
        for phrase in ["返回天气","距离日落","07:40","白昼中点","12:06","06:52","17:21","白昼 10小时29分"] {
            assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains(phrase))),"{skin:?} / {scale} 日照视图缺少 {phrase}");
        }
        for s in &h.shapes {if let egui::Shape::Text(t)=&s.shape {assert!(s.clip_rect.contains_rect(t.visual_bounding_rect()),"日照文字不应裁切：{}",t.galley.text());}}
    }}
}

#[test]
#[ignore="按需导出七日预报、日照和两侧常驻天气的实际画面"]
fn weather_foundation_preview(){
    use crate::model::{Dock,Skin};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_WEATHER_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for (name,dock,solar,clock,skin) in [
        ("weather-week",Dock::Top,false,"14:08:00",Skin::ALL[0]),
        ("weather-sun",Dock::Top,true,"14:08:00",Skin::ALL[0]),
        ("weather-night",Dock::Top,true,"20:08:00",Skin::ALL[0]),
        ("weather-side-left",Dock::Left,false,"14:08:00",Skin::ALL[0]),
        ("weather-side-right",Dock::Right,false,"14:08:00",Skin::ALL[0]),
        ("weather-week-cream",Dock::Top,false,"14:08:00",Skin::Cream),
        ("weather-sun-cream",Dock::Top,true,"14:08:00",Skin::Cream),
        ("weather-week-glass",Dock::Top,false,"14:08:00",Skin::GlassDark),
        ("weather-sun-glass",Dock::Top,true,"14:08:00",Skin::GlassDark),
        ("weather-city-picker",Dock::Top,false,"14:08:00",Skin::Classic),
    ] {
        let mut h=H::new();h.app.settings.dock=dock;h.app.settings.skin=skin;h.app.settings.pet=false;
        let mut snapshot=crate::weather::parse("杭州",&serde_json::json!({"timezone":"Asia/Shanghai",
            "current":{"temperature_2m":19,"apparent_temperature":18,"relative_humidity_2m":58,"wind_speed_10m":12,"weather_code":2,"is_day":1},
            "daily":{"time":["2026-11-11","2026-11-12","2026-11-13","2026-11-14","2026-11-15","2026-11-16","2026-11-17"],
                "weather_code":[2,0,3,61,80,2,0],"temperature_2m_max":[23,24,22,19,18,20,22],"temperature_2m_min":[15,16,14,13,12,12,14],
                "precipitation_probability_max":[10,0,20,70,55,10,0],"sunrise":["2026-11-11T06:52"],"sunset":["2026-11-11T17:21"]}})).unwrap();
        if clock=="20:08:00" {snapshot.day=false;}
        h.app.weather.snapshot=Some(snapshot);
        if name=="weather-city-picker" {
            let city=|label:&str,region:&str,lat:f64|crate::weather::Position{lat,lon:120.0,label:label.into(),region:region.into()};
            h.app.settings.weather_saved=vec![city("杭州","浙江 · 中国",30.27),city("上海","上海 · 中国",31.23),city("哈尔滨","黑龙江 · 中国",45.75)];
            h.app.settings.weather_city="杭州".into();h.app.settings.weather_position=Some(h.app.settings.weather_saved[0].clone());
            h.app.weather.snapshot.as_mut().unwrap().position=h.app.settings.weather_position.clone();
            h.app.weather.picker_open=true;
        }
        if dock==Dock::Top {h.app.weather.solar_open=solar;h.app.show_page(8);}
        let (w,hgt)=if dock==Dock::Top {(crate::app::CANVAS_W,WIN_H)}else{(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H)};
        let mut raster=feature_raster_sized(w,hgt);
        let frames=90;
        for frame in 0..frames {
            h.t+=1.0/60.0;
            let cursor=Some(if dock==Dock::Top{H::local(180.0,185.0)}else{Pos2::new(250.0,580.0)});
            let env=Env{t:h.t,clock:clock.into(),today:"2026-11-11".into(),date:"11月11日 周三".into(),cursor,battery:Some(76),bt_ready:true,..Default::default()};
            let events=vec![egui::Event::PointerMoved(cursor.unwrap())];
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt))),time:Some(h.t),focused:true,
                events,viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
            let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{
                h.app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(if dock==Dock::Top{WIN_W}else{w},hgt)),&env);
            });});
            raster.update_textures(&output.textures_delta);
            if frame==frames-1 {
                raster.buf.fill([0.20,0.24,0.32,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                let image=image::RgbaImage::from_raw((w*2.0)as u32,(hgt*2.0)as u32,rgba).unwrap();
                let (x,y,crop_w,crop_h)=if dock==Dock::Top {(200,0,880,(h.app.expanded_height()*2.0+24.0)as u32)}else{
                    let rect=h.app.island_rect(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt)),h.app.w.pos,h.app.h.pos);
                    (if dock==Dock::Left{0}else{880},((rect.top()-12.0)*2.0)as u32,120,((rect.height()+24.0)*2.0)as u32)
                };
                let file=format!("{name}.png");
                image::imageops::crop_imm(&image,x,y,crop_w,crop_h).to_image().save(dest.join(file)).unwrap();
            }
            raster.free_textures(&output.textures_delta);
        }
    }
}

#[test]
#[ignore="按需导出进入日照视图时的涟漪与太阳滑入过程"]
fn weather_solar_entry_preview(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_WEATHER_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    let mut h=H::new();h.app.settings.pet=false;
    h.app.weather.snapshot=Some(crate::weather::parse("杭州",&serde_json::json!({"timezone":"Asia/Shanghai",
        "current":{"temperature_2m":19,"apparent_temperature":18,"relative_humidity_2m":58,"wind_speed_10m":12,"weather_code":2,"is_day":1},
        "daily":{"time":["2026-11-11"],"weather_code":[2],"temperature_2m_max":[23],"temperature_2m_min":[15],
            "precipitation_probability_max":[10],"sunrise":["2026-11-11T06:52"],"sunset":["2026-11-11T17:21"]}})).unwrap());
    h.app.show_page(8);
    let (w,hgt)=(crate::app::CANVAS_W,WIN_H);
    let mut raster=feature_raster_sized(w,hgt);
    let click=H::local(330.0,94.0);
    // 第 60 帧按下、61 帧松开；之后按时间点导出。
    let shots=[(70,"weather-entry-a"),(85,"weather-entry-b"),(110,"weather-entry-c"),(160,"weather-entry-d")];
    for frame in 0..161 {
        h.t+=1.0/60.0;
        let env=Env{t:h.t,clock:"14:08:00".into(),today:"2026-11-11".into(),date:"11月11日 周三".into(),cursor:Some(click),battery:Some(76),bt_ready:true,..Default::default()};
        let mut events=vec![egui::Event::PointerMoved(click)];
        if frame==60||frame==61 {events.push(egui::Event::PointerButton{pos:click,button:egui::PointerButton::Primary,pressed:frame==60,modifiers:Default::default()});}
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt))),time:Some(h.t),focused:true,events,
            viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{
            h.app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,hgt)),&env);
        });});
        raster.update_textures(&output.textures_delta);
        if let Some((_,name))=shots.iter().find(|(at,_)|*at==frame) {
            raster.buf.fill([0.20,0.24,0.32,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
            let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            let image=image::RgbaImage::from_raw((w*2.0)as u32,(hgt*2.0)as u32,rgba).unwrap();
            image::imageops::crop_imm(&image,200,0,880,(h.app.expanded_height()*2.0+24.0)as u32).to_image().save(dest.join(format!("{name}.png"))).unwrap();
        }
        raster.free_textures(&output.textures_delta);
    }
    assert!(h.app.weather.solar_open,"点击天气视图后应进入日照视图");
}

#[test]
fn island_meshes_stay_valid_after_long_uptime_and_repeated_hover(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.page=0;h.power=(Some(100),true);
    let mut frames=0;
    for days in [0.0,1.0,3.0,7.0,30.0] {
        h.t=days*86400.0;
        for skin in crate::model::Skin::ALL {
            h.app.settings.skin=skin;
            for scale in [1.0,1.25,1.5] {
                h.ctx.set_pixels_per_point(scale);
                for frame in 0..120 {
                    let point=if frame%60<30{H::local(200.0,15.0)}else{Pos2::new(900.0,250.0)};
                    h.cursor=Some(point);h.frame(vec![egui::Event::PointerMoved(point)]);
                    let meshes=h.ctx.tessellate(h.shapes.clone(),h.ctx.pixels_per_point());
                    for primitive in meshes {if let egui::epaint::Primitive::Mesh(mesh)=primitive.primitive {
                        assert!(mesh.is_valid(),"运行 {days} 天后的网格索引必须有效");
                        assert!(mesh.vertices.iter().all(|v|v.pos.is_finite()&&v.uv.is_finite()),"运行 {days} 天后展开不能出现非有限坐标");
                        assert!(mesh.vertices.iter().all(|v|v.pos.x.abs()<10000.0&&v.pos.y.abs()<10000.0),"运行 {days} 天后顶点不能拉到窗口远处");
                    }}
                    frames+=1;
                }
            }
        }
    }
    eprintln!("长运行绘制检查：{frames} 帧；覆盖 0、1、3、7、30 天及全部皮肤、三种缩放。");
}


#[test]
fn weather_hidden_page_opens_its_settings_without_panicking(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.set_page_visible(8,false);h.app.show_page(8);
    assert!(h.app.settings_open);assert_eq!(h.app.settings_tab,12);
}

#[test]
fn weather_settings_maps_to_the_module_switch(){
    assert_eq!(crate::extras::settings_tab_module(12),Some(8));
}

#[test]
fn weather_city_draft_keeps_the_saved_location_until_confirmed(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.weather_city="杭州".into();h.app.settings.wecom_notifications=true;
    h.app.settings.feishu_notifications=true;h.app.settings.dingtalk_notifications=true;
    h.app.settings.save();h.app.weather.city_input="北京".into();
    h.frame(vec![]);
    assert_eq!(h.app.settings.weather_city,"杭州");
    let saved=crate::model::Settings::load();
    assert_eq!(saved.weather_city,"杭州");
    assert!(saved.wecom_notifications && saved.feishu_notifications && saved.dingtalk_notifications);
    h.app.set_page_visible(8,false);h.app.add_weather_city();
    assert_eq!(h.app.settings.weather_city,"杭州");assert!(!h.app.weather.loading);
}

#[test]
fn weather_cities_switch_from_the_island_title_and_keep_their_own_cache(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.pet=false;
    let hz=crate::weather::Position{lat:30.27,lon:120.15,label:"杭州".into(),region:"浙江 · 中国".into()};
    let bj=crate::weather::Position{lat:39.90,lon:116.40,label:"北京".into(),region:"北京 · 中国".into()};
    // 设置页添加两个城市：只加入列表，搜索框清空。
    for city in [&hz,&bj] {
        h.app.weather.city_input=city.label.clone();h.app.weather.selected_city=Some(city.clone());
        assert!(h.app.add_weather_city());
        assert!(h.app.settings.weather_city.is_empty(),"添加城市只加入列表，不切换当前城市");
        assert!(h.app.weather.city_input.is_empty());
    }
    assert_eq!(h.app.settings.weather_saved,vec![hz.clone(),bj.clone()]);
    h.app.weather.city_input=hz.label.clone();h.app.weather.selected_city=Some(hz.clone());
    assert!(!h.app.add_weather_city(),"重复城市不再添加");
    assert_eq!(crate::model::Settings::load().weather_saved,vec![hz.clone(),bj.clone()],"城市列表应保存到设置");
    // 打开天气页，点标题展开下拉框，选「杭州」。
    h.app.weather.snapshot=Some(crate::weather::Snapshot{place:"北京".into(),position:Some(bj.clone()),temp:-3.0,code:71,day:true,at:chrono::Utc::now().timestamp(),..Default::default()});
    h.app.show_page(8);h.hover(H::local(250.0,65.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="当地")),"标题保持「当地天气」");
    h.click(H::local(115.0,112.0));h.hover(H::local(330.0,200.0),0.4);
    assert!(h.app.weather.picker_open,"点地点行展开城市下拉");assert!(!h.app.weather.solar_open,"点地点行不应切到日照");
    for phrase in ["自动定位","杭州","北京","添加或管理城市"] {
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains(phrase))),"下拉框缺少 {phrase}");
    }
    let row=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()=="杭州"=>Some(t.visual_bounding_rect().center()),_=>None}).unwrap();
    h.click(row);h.hover(row,0.3);
    assert_eq!(h.app.settings.weather_city,"杭州");assert!(!h.app.weather.picker_open);assert!(!h.app.weather.solar_open);
    assert!(h.app.weather.snapshot.is_none(),"杭州没有缓存时不能显示北京的天气");
    // 点面板外收起，不触发日照视图。
    h.click(H::local(209.0,147.0));h.hover(H::local(330.0,200.0),0.4);assert!(h.app.weather.picker_open,"无数据时入口在页面中下方");
    h.click(H::local(330.0,200.0));h.hover(H::local(330.0,200.0),0.3);
    assert!(!h.app.weather.picker_open);assert!(!h.app.weather.solar_open,"点外面只收起下拉框");
    // 删除当前城市：回到自动定位。
    h.app.remove_weather_city(&hz);
    assert_eq!(h.app.settings.weather_saved,vec![bj.clone()]);assert!(h.app.settings.weather_city.is_empty());assert!(h.app.settings.weather_position.is_none());
}

#[test]
fn weather_negative_temperature_draws_a_hyphen_length_minus_on_the_digit_midline(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.pet=false;
    h.app.weather.snapshot=Some(crate::weather::Snapshot{place:"哈尔滨".into(),temp:-6.0,code:71,day:true,at:chrono::Utc::now().timestamp(),..Default::default()});
    for _ in 0..60{h.frame(vec![]);}
    let digits=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()=="6°"=>Some(t.visual_bounding_rect()),_=>None}).expect("胶囊应显示 6°");
    let bar=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Rect(r) if (digits.left()-r.rect.right()).abs()<4.0 && r.rect.right()<digits.center().x && r.rect.width()>r.rect.height()*1.5 && r.rect.height()<4.0=>Some(r.rect),_=>None}).expect("6° 左侧应有手绘减号");
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains('−'))),"不再使用过长的排版减号");
    assert!(bar.width()<digits.height()*0.6,"减号长度与连字符相当");
    assert!((bar.center().y-digits.center().y).abs()<digits.height()*0.2,"减号在数字中线上");
}

#[test]
fn weather_city_dropdown_selects_the_region_without_applying_the_draft(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let correct=crate::weather::Position{lat:23.02677,lon:113.13148,label:"佛山市".into(),region:"广东 · 中国".into()};
    let wrong=crate::weather::Position{lat:28.73401,lon:98.80612,label:"佛山".into(),region:"迪庆藏族自治州 · 云南 · 中国".into()};
    h.app.settings.weather_city="杭州".into();h.app.weather.manual_city=true;h.app.weather.city_input="佛山".into();
    h.app.weather.cities.seed("佛山",vec![correct.clone(),wrong]);
    fn draw(h:&mut H,events:Vec<egui::Event>){
        h.t+=1.0/60.0;h.app.t=h.t;
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(560.0,380.0))),time:Some(h.t),events,..Default::default()};
        h.shapes=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().show(ctx,|ui|{h.app.weather_city_picker(ui);});}).shapes;
    }
    for _ in 0..3 {draw(&mut h,vec![]);}
    h.ctx.memory_mut(|m|m.open_popup(egui::Id::new("weather-city-options")));
    for _ in 0..3 {draw(&mut h,vec![]);}
    let point=h.shapes.iter().find_map(|s|match &s.shape {egui::Shape::Text(text) if text.galley.text()=="佛山市"=>Some(text.visual_bounding_rect().center()),_=>None}).expect("下拉框必须显示完整城市名");
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="广东 · 中国")),"下拉框必须显示省份");
    draw(&mut h,vec![egui::Event::PointerMoved(point),egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
    draw(&mut h,vec![egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
    assert_eq!(h.app.weather.selected_city.as_ref(),Some(&correct));assert_eq!(h.app.weather.draft_city(),Some("佛山市"));
    assert_eq!(h.app.settings.weather_city,"杭州","选择候选后仍需应用确认");assert!(!h.app.weather.loading);
    assert!(!h.ctx.memory(|m|m.is_popup_open(egui::Id::new("weather-city-options"))));
    h.app.settings.weather_city="佛山市".into();h.app.settings.weather_position=Some(correct.clone());h.app.settings.save();
    let saved=crate::model::Settings::load();assert_eq!(saved.weather_position,Some(correct));
    h.app.weather.city_input="伦敦".into();assert_eq!(h.app.weather.draft_city(),None,"改写输入后必须重新选择地区");
}

#[test]
fn weather_attribution_opens_its_source_without_changing_pages(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.show_page(8);h.hover(H::local(300.0,60.0),1.0);
    h.click(H::local(375.0,crate::app::WEATHER_H-7.0));
    assert!(h.cmds.contains(&crate::app::Cmd::OpenUrl("https://open-meteo.com/".into())));
    assert_eq!(h.app.page(),8);
}

#[test]
fn weather_refresh_keeps_music_and_settings_shortcuts_separate(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"cloudmusic.exe".into(),title:"歌曲".into(),artist:"歌手".into(),playing:true,..Default::default()};
    h.app.show_page(8);h.hover(H::local(300.0,60.0),1.0);
    let rect=|name|h.ctx.read_response(egui::Id::new(("card",true,name))).unwrap().rect;
    let refresh=rect("weather-refresh");
    assert!(!refresh.intersects(rect("header-music")));
    assert!(!refresh.intersects(rect("page-settings")));
}

#[test]
fn resized_font_atlases_stop_reuploading_and_release_unused_sizes(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let ctx=egui::Context::default();crate::theme::load_fonts(&ctx);
    ctx.set_embed_viewports(false);
    let child=egui::ViewportId::from_hash_of("font-upload-settings");
    let children=std::sync::Arc::new(Mutex::new(Vec::new()));let capture=children.clone();
    egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
        let id=viewport.ids.this;
        let input=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(640.0,270.0))),max_texture_side:Some(2048),
            viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.0),..Default::default()})).collect(),..Default::default()};
        capture.lock().unwrap().push(ctx.run(input,|ctx|(viewport.viewport_ui_cb)(ctx)));
    });
    let frame=|ppp:f32,settings:bool|{
        let mut viewports=std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect::<egui::ViewportIdMap<_>>();
        if settings{viewports.insert(child,egui::ViewportInfo{native_pixels_per_point:Some(1.0),..Default::default()});}
        let input=egui::RawInput{viewports,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(640.0,270.0))),max_texture_side:Some(2048),..Default::default()};
        let draw=|ctx:&egui::Context|{egui::CentralPanel::default().show(ctx,|ui|{ui.label("专注时间 25:00 · 休息 · 音乐与歌词");});};
        let root=ctx.run(input,|ctx|{draw(ctx);if settings{ctx.show_viewport_immediate(child,Default::default(),|ctx,_|draw(ctx));}});
        let mut outputs=std::mem::take(&mut *children.lock().unwrap());outputs.push(root);
        outputs
    };
    for settings in [false,true,false] {
        for ppp in [1.0,1.4,1.2,0.8,1.4,1.0] {
            for _ in 0..3{frame(ppp,settings);}
            let start=std::time::Instant::now();let mut bytes=0usize;
            for _ in 0..60 {for output in frame(ppp,settings){
                bytes+=output.textures_delta.set.iter().map(|(_,delta)|delta.image.width()*delta.image.height()*4).sum::<usize>();
            }}
            println!("缩放={ppp:.2} 设置={settings} 60帧上传={bytes}字节 耗时={:?}",start.elapsed());
            assert_eq!(bytes,0,"大小稳定后不能重复上传字形图集，包括设置同时打开时");
            let textures=ctx.tex_manager();let textures=textures.read();
            let fonts=textures.allocated().filter(|(_,meta)|meta.name.starts_with("egui_font_texture")).count();
            assert!(fonts<=if settings&&ppp!=1.0{2}else{1},"旧倍率的字体贴图应释放：{fonts}");
        }
    }
}

#[test]
fn focus_durations_support_direct_input_and_keep_the_current_timer(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.open_settings_category(2);h.hover(H::local(200.0,60.0),1.0);
    let remaining=h.app.pomo.remaining;
    for (label,input,expected,rest) in [("25 分钟","45",45,false),("5 分钟","12",12,true),("45 分钟","999",180,false),("12 分钟","0",1,true),("180 分钟","abc",180,false)]{
        h.cmds.clear();h.click_text(label);h.frame(vec![]);
        assert!(h.app.is_editing(),"单击时长应进入输入状态");
        assert!(h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Focus)),"岛内输入必须取得原生键盘焦点");
        h.frame(vec![egui::Event::Text(input.into())]);
        h.frame(vec![egui::Event::Key{key:egui::Key::Enter,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
        h.frame(vec![]);
        let settings=crate::model::Settings::load();
        assert_eq!(if rest{settings.rest_minutes}else{settings.focus_minutes},expected,"输入需校验范围并保存");
        assert!(!h.app.is_editing());assert_eq!(h.app.pomo.remaining,remaining,"调整时长不能重置当前计时");
    }
    h.click_text("180 分钟");h.frame(vec![]);h.frame(vec![egui::Event::Text("30".into())]);
    h.frame(vec![egui::Event::Key{key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
    h.frame(vec![]);assert_eq!(h.app.settings.focus_minutes,180,"Esc 应取消本次输入");
    h.click_text("180 分钟");h.frame(vec![]);h.frame(vec![egui::Event::Text("50".into())]);
    h.click(H::local(268.0,62.0));h.frame(vec![]);
    assert_eq!(h.app.settings.focus_minutes,51,"点击加号应先保存输入，再增加一分钟");
    assert_eq!(crate::model::Settings::load().focus_minutes,51);
    h.click_text("51 分钟");h.frame(vec![]);h.frame(vec![egui::Event::Text("40".into())]);
    h.click(H::local(50.0,65.0));h.frame(vec![]);
    assert_eq!(h.app.settings.focus_minutes,40,"点击别处应保存输入");
    assert!(!h.app.is_editing());
}

#[test]
fn enlarged_island_preserves_font_resolution_with_a_separate_settings_window(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.open_settings_category(2);h.hover(H::local(200.0,60.0),1.0);
    h.app.open_settings();h.app.settings_tab=2;h.ctx.set_embed_viewports(false);
    let captured=std::sync::Arc::new(Mutex::new(None));let capture=captured.clone();
    egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
        let id=viewport.ids.this;
        let input=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(690.0,560.0))),
            viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.0),..Default::default()})).collect(),..Default::default()};
        *capture.lock().unwrap()=Some(ctx.run(input,|ctx|(viewport.viewport_ui_cb)(ctx)));
    });
    let mut raster=crate::preview_raster::Raster::new(WIN_W as usize,WIN_H as usize);
    raster.update_textures(&egui::TexturesDelta{set:vec![(egui::TextureId::default(),egui::epaint::ImageDelta::full(h.ctx.fonts(|fonts|fonts.image()),egui::TextureOptions::LINEAR))],..Default::default()});
    for scale in [1.0,1.2,1.4] {
        raster.w=(WIN_W*scale) as usize;raster.h=(WIN_H*scale) as usize;raster.buf.resize(raster.w*raster.h,[0.0;4]);
        for frame in 0..3 {
            h.t+=1.0/60.0;
            let input=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),
                time:Some(h.t),events:vec![egui::Event::PointerMoved(H::local(70.0,40.0))],viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(scale),..Default::default()})).collect(),..Default::default()};
            let root=h.ctx.run(input,|ctx|{
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{
                    let win=Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,WIN_H));
                    h.app.frame(ui,win,&Env{t:h.t,cursor:Some(H::local(70.0,40.0)),clock:"09:41:00".into(),..Default::default()});
                });
                h.app.settings_ui(ctx,false);
            });
            let child=captured.lock().unwrap().take().unwrap();
            assert_eq!(child.pixels_per_point,1.0,"设置窗口不跟随岛体变大");
            for (shapes,ppp) in [(&root.shapes,scale),(&child.shapes,1.0)] {
                for shape in shapes {if let egui::Shape::Text(text)=&shape.shape {
                    assert!((text.galley.pixels_per_point-ppp).abs()<0.0001,"各窗口应使用各自物理尺寸的字形");
                }}
            }
            raster.update_textures(&child.textures_delta);raster.update_textures(&root.textures_delta);
            if frame==2 {if let Some(dest)=std::env::var_os("DAOZAI_FOCUS_SCALE_PREVIEW") {
                let dest=std::path::PathBuf::from(dest);std::fs::create_dir_all(&dest).unwrap();
                raster.buf.fill([0.18,0.20,0.24,1.0]);raster.draw(&h.ctx.tessellate(root.shapes,root.pixels_per_point),root.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                image::save_buffer(dest.join(format!("focus-size-{}.png",(scale*100.0) as u32)),&rgba,(WIN_W*scale) as u32,(WIN_H*scale) as u32,image::ColorType::Rgba8).unwrap();
            }}
            raster.free_textures(&root.textures_delta);
            raster.free_textures(&child.textures_delta);
        }
    }
}

#[test]
fn opaque_surface_gets_a_contour_with_every_skin_and_dock(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for skin in crate::model::Skin::ALL {for dock in crate::model::Dock::ALL {for island_shape in crate::model::IslandShape::ALL {for opaque_surface in [false,true] {
        let mut h=H::new();h.app.settings.skin=skin;h.app.settings.dock=dock;h.app.settings.island_shape=island_shape;h.app.settings.glass_blur=false;
        let size=if dock.is_side(){Vec2::new(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H)}else{Vec2::new(WIN_W,WIN_H)};
        let mut outlined=false;
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),..Default::default()};
        let _=h.ctx.run(raw,|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let win=Rect::from_min_size(ui.max_rect().min,size);
                let out=h.app.frame(ui,win,&Env{opaque_surface,..Default::default()});
                outlined=out.glass.is_some_and(|shapes|!shapes.is_empty());
            });
        });
        assert_eq!(outlined,opaque_surface,"皮肤={skin:?}，停靠={dock:?}，形状={island_shape:?}");
    }}}}
}

#[test]
fn changing_qq_metadata_cannot_spawn_lyrics_workers_while_one_is_running(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let (release,wait)=std::sync::mpsc::channel();
    let worker=std::thread::spawn(move||{let _=wait.recv();});
    let id=worker.thread().id();h.app.ly_worker=Some(worker);
    h.app.opts.fetch_lyrics=true;h.app.settings.island_lyrics=true;
    for step in 0..1440 {
        h.media=crate::smtc::NowPlaying{source:"QQMusic.exe".into(),title:format!("歌曲{}",step%2),artist:"歌手".into(),duration:120.0,playing:true,..Default::default()};
        h.t=step as f64*0.25;h.frame(vec![]);
        assert_eq!(h.app.ly_worker.as_ref().unwrap().thread().id(),id,"旧请求未结束时不得另开取词线程");
        assert!(h.app.ly.rx.is_none(),"不能为抖动的歌曲堆积请求");
    }
    release.send(()).unwrap();h.app.ly_worker.take().unwrap().join().unwrap();
}

#[test]
fn top_bar_modes_preserve_legacy_settings_and_save_the_selected_display_mode(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let _h=H::new();
    use crate::model::Settings;
    let path=crate::model::config_dir().join("settings.txt");std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bounds=crate::display_layout::Bounds{left:-1920,top:-400,right:0,bottom:680};
    let display=crate::display_layout::Display{id:"test".into(),label:"测试屏".into(),bounds,work_top:-368,primary:true};
    for enabled in [false,true] {
        std::fs::write(&path,format!("avoid_top_bar={}\n",enabled as u8)).unwrap();
        let mut settings=Settings::load();
        assert_eq!(settings.avoid_top_bar,enabled);assert!(!settings.top_bar_on_top,"旧配置默认沿用避让方式");
        for on_top in [false,true] {
            settings.top_bar_on_top=on_top;settings.save();let saved=Settings::load();
            assert_eq!((saved.avoid_top_bar,saved.top_bar_on_top),(enabled,on_top));
            assert_eq!(crate::display_layout::placement_bounds(&display,saved.avoids_top_bar()).top,if enabled&&!on_top{-368}else{-400},"置顶方式应回到屏幕顶边；关闭适配后不避让");
        }
    }
}

#[test]
fn top_bar_settings_switch_modes_and_respect_the_master_switch(){
    fn draw(h:&mut H,events:Vec<egui::Event>,raster:&mut crate::preview_raster::Raster)->egui::FullOutput {
        h.t+=0.1;let time=h.t;
        let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;
            let raw=egui::RawInput{viewport_id:id,time:Some(time),focused:true,events:events.clone(),screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,1800.0))),viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.0),..Default::default()})).collect(),..Default::default()};
            *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        let root=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));raster.update_textures(&root.textures_delta);
        let child=output.lock().unwrap().take().unwrap();raster.update_textures(&child.textures_delta);child
    }
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dark in [true,false] {
        let mut h=H::new();h.app.open_settings();h.ctx.set_embed_viewports(false);
        h.app.settings.settings_theme=if dark{crate::settings_theme::SettingsTheme::Dark}else{crate::settings_theme::SettingsTheme::Light};
        let mut raster=crate::preview_raster::Raster::new(690,1800);
        for _ in 0..3{let _=draw(&mut h,vec![],&mut raster);}
        for (on_top,label) in [(true,"岛仔置顶"),(false,"避开顶栏")] {
            let output=draw(&mut h,vec![],&mut raster);
            let position=output.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==label=>{
                assert!(s.clip_rect.contains_rect(t.visual_bounding_rect()),"显示方式按钮不能裁切");Some(t.visual_bounding_rect().center())
            },_=>None}).expect("显示方式应提供避让和置顶两个选项");
            for pressed in [true,false] {let _=draw(&mut h,vec![egui::Event::PointerMoved(position),egui::Event::PointerButton{pos:position,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}],&mut raster);}
            assert_eq!(h.app.settings.top_bar_on_top,on_top);assert_eq!(crate::model::Settings::load().top_bar_on_top,on_top,"切换后立即保存");
            if let Some(dest)=std::env::var_os("DAOZAI_TOP_BAR_PREVIEW_DIR") {
                let output=draw(&mut h,vec![],&mut raster);raster.buf.fill(if dark{[0.08,0.08,0.09,1.0]}else{[0.96,0.96,0.97,1.0]});
                raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);let dest=std::path::PathBuf::from(dest);std::fs::create_dir_all(&dest).unwrap();
                image::save_buffer(dest.join(format!("top-bar-{}-{}.png",if dark{"dark"}else{"light"},if on_top{"on-top"}else{"avoid"})),&rgba,690,1800,image::ColorType::Rgba8).unwrap();
            }
        }
        h.app.settings.avoid_top_bar=false;
        let output=draw(&mut h,vec![],&mut raster);
        assert!(!output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="岛仔置顶")),"关闭适配后收起显示方式选项");
    }
}

#[test]
fn v0649_settings_and_mail_accounts_survive_upgrade(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let _h=H::new();
    let directory=crate::model::config_dir();
    std::fs::create_dir_all(&directory).unwrap();
    let legacy=concat!("settings_theme=dark\npet=0\nminimal=1\nbrowser_minimal=1\n",
        "visible_pages=10111111\npage_order=5,2,0,1,3,4,6,7\nshow_battery=0\n",
        "system_hud=0\nread_notifications=1\nnotifications=0\nsound=1\n",
        "lyrics_enabled=0\nisland_lyrics=1\nlyric_delay_ms=-650\nhide_fullscreen=0\n",
        "focus_minutes=45\nrest_minutes=10\nwater_minutes=60\nserving_ml=300\nwater_start=8\nwater_end=23\n",
        "mail_enabled=0\nmail_code=0\nmail_interval=300\nmail_ics=30\ncalendar_prompt=0\ncalendar_auto_sync=0\n");
    std::fs::write(directory.join("settings.txt"),legacy).unwrap();
    let upgraded=crate::model::Settings::load();
    assert!(upgraded.display_id.is_empty() && upgraded.avoid_apps.is_empty());
    assert!(upgraded.avoid_browsers && upgraded.bluetooth_notifications);
    assert!(!upgraded.clock_12h && !upgraded.notification_preview);
    assert_eq!(upgraded.clock_period,0);assert_eq!(upgraded.mail_code_hide_seconds,60);
    for _ in 0..2 {
        crate::model::Settings::load().save();
        let saved=std::fs::read_to_string(directory.join("settings.txt")).unwrap();
        for line in legacy.lines().filter(|line|!line.starts_with("visible_pages=")&&!line.starts_with("page_order=")){assert!(saved.lines().any(|value|value==line),"升级保存不能改变旧设置：{line}");}
        assert!(saved.lines().any(|value|value=="visible_pages=101111111"),"旧页面开关应保留，并默认开启天气页");
        assert!(saved.lines().any(|value|value=="page_order=5,2,0,1,3,4,6,7,8"),"旧顺序应保留，并把天气页接在最后");
    }
    std::fs::write(directory.join("mail-accounts.json"),br#"[{"email":"OldUser@qq.com","provider":"qq","enabled":true},{"email":"old@icloud.com","provider":"icloud","enabled":false}]"#).unwrap();
    let accounts=crate::mail::load_accounts();
    assert_eq!(accounts.len(),2);assert!(accounts.iter().all(|account|account.custom.is_none()));
    assert_eq!(accounts[0].credential_key(),"olduser@qq.com");
    assert_eq!(accounts[0].endpoint().unwrap(),("imap.qq.com".into(),993,"OldUser@qq.com".into()));
    assert!(!accounts[1].enabled);
    crate::mail::save_accounts(&accounts);assert_eq!(crate::mail::load_accounts(),accounts);
}

fn editing_local_calendar_clears_outdated_reminders() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for reschedule in [false,true] {
        let mut h=H::new();
        let mut draft=crate::local_calendar::Draft::new(chrono::Local::now().date_naive()+chrono::Duration::days(1));
        draft.title="原日程".into();
        let entry=draft.entry().unwrap();
        h.app.calendar.local=vec![entry.clone()];
        h.app.alert("专注计时","休息",1,false);
        h.frame(vec![]);
        h.app.meeting_alert(entry.event.clone(),"本机日程".into(),7);
        let mut other=entry.event.clone();other.uid="另一日程".into();
        h.app.meeting_alert(other,"本机日程".into(),7);
        h.app.open_calendar_editor(Some(&entry.event.uid));
        let draft=h.app.calendar.draft.as_mut().unwrap();
        if reschedule {
            let day=(chrono::Local::now().date_naive()+chrono::Duration::days(2)).to_string();
            draft.start_date=day.clone();draft.end_date=day;
        } else {draft.reminder=None;}
        h.app.save_calendar_editor();
        assert!(h.app.calendar.draft.is_none());
        assert!(!h.app.notice.as_ref().is_some_and(|a|a.meeting.as_ref().is_some_and(|(m,_)|m.uid==entry.event.uid)),"修改后不能继续显示旧提醒");
        assert!(!h.app.alerts.iter().any(|a|a.meeting.as_ref().is_some_and(|(m,_)|m.uid==entry.event.uid)),"修改后不能弹出排队中的旧提醒");
        assert!(h.app.alerts.iter().any(|a|a.meeting.as_ref().is_some_and(|(m,_)|m.uid=="另一日程")));
    }
}

#[test]
fn meeting_notice_can_close_without_deleting_event_or_other_reminders(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.frame(vec![]);
    let event=crate::mail::Meeting{all_day:false,title:"当前会议".into(),start:chrono::Local::now().timestamp()+600,end:None,at:String::new(),uid:"dismiss-current".into()};
    h.app.calendar.events.push(event.clone());
    h.app.mail.fired.insert(crate::calendar::dedupe_key(&event));
    h.app.meeting_alert(event,"飞书".into(),7);
    h.app.meeting_alert(crate::mail::Meeting{all_day:false,title:"下一场会议".into(),start:chrono::Local::now().timestamp()+1200,end:None,at:String::new(),uid:"dismiss-next".into()},"钉钉".into(),7);
    for _ in 0..45{h.frame(vec![]);}
    let page=h.app.page;
    let button=h.ctx.read_response(egui::Id::new(("card",true,"meeting-dismiss"))).expect("会议提醒必须有关闭按钮").rect;
    h.click(button.center());
    assert!(h.app.notice.is_none());assert_eq!(h.app.page,page);
    assert_eq!(h.app.calendar.events.len(),1,"关闭提醒不能删除日程");
    h.frame(vec![]);
    assert_eq!(h.app.notice.as_ref().unwrap().meeting.as_ref().unwrap().0.uid,"dismiss-next");
}

#[test]
fn calendar_hides_ended_events_and_keeps_all_day_out_of_featured_card() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let day=chrono::NaiveDate::from_ymd_opt(2026,10,28).unwrap();
    let at=|hour|day.and_hms_opt(hour,0,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap().timestamp();
    h.app.calendar.view.day=day;h.app.calendar.view.month=crate::calendar::month_start(day);h.app.calendar.view.loaded=Some(h.app.calendar.view.month);
    h.app.calendar.view.events=vec![
        crate::mail::Meeting{uid:"all-day".into(),title:"全天安排".into(),start:at(0),end:None,all_day:true,at:String::new()},
        crate::mail::Meeting{uid:"past".into(),title:"过去的安排".into(),start:at(7),end:Some(at(8)),all_day:false,at:String::new()},
        crate::mail::Meeting{uid:"future".into(),title:"下一场安排".into(),start:at(11),end:Some(at(12)),all_day:false,at:String::new()},
    ];
    h.app.open_calendar(None);h.hover(H::local(300.0,40.0),1.0);
    let contains=|h:&H,text:&str|h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(t) if t.galley.text()==text));
    assert!(!contains(&h,"过去的安排"));assert!(contains(&h,"全天安排"));
    assert!(h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(t) if t.galley.text()=="11:00" && t.galley.job.sections[0].format.font_id.size>=23.0)));
    h.click(H::local(270.0,43.0));h.frame(vec![]);assert!(contains(&h,"过去的安排"));
    h.click(H::local(270.0,43.0));h.frame(vec![]);assert!(!contains(&h,"过去的安排"));
}

#[test]
fn local_calendar_failed_save_keeps_draft_and_does_not_change_events() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    std::fs::create_dir_all(crate::model::config_dir().join("local-calendar.json")).unwrap();
    h.app.open_calendar_editor(None);h.app.calendar.draft.as_mut().unwrap().title="不能丢失的草稿".into();
    h.app.save_calendar_editor();
    assert!(h.app.calendar.local.is_empty());let draft=h.app.calendar.draft.as_ref().unwrap();
    assert_eq!(draft.title,"不能丢失的草稿");assert!(!draft.error.is_empty());
}

#[test]
#[ignore="按需测量 ICS 导入后日历页的绘制耗时"]
fn imported_ics_calendar_frame_cost() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for (count,same_day) in [(0,false),(100,false),(1000,false),(1000,true),(5000,true)] {
        let mut h=H::new();h.app.settings.pet=false;
        let start=chrono::Local::now().date_naive().and_hms_opt(12,0,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap().with_timezone(&chrono::Utc);
        let mut ics=String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n");
        for index in 0..count {
            let at=start+chrono::Duration::days(if same_day {0} else {index%365});
            ics.push_str(&format!("BEGIN:VEVENT\r\nUID:import-{index}\r\nSUMMARY:导入日程 {index}\r\nDTSTART:{}\r\nDTEND:{}\r\nEND:VEVENT\r\n",at.format("%Y%m%dT%H%M%SZ"),(at+chrono::Duration::hours(1)).format("%Y%m%dT%H%M%SZ")));
        }
        ics.push_str("END:VCALENDAR\r\n");
        let at=std::time::Instant::now();h.app.import_ics(&[("sample.ics".into(),ics)]);let import=at.elapsed();
        assert_eq!(h.app.calendar.local.len(),count as usize);
        h.app.open_calendar(None);h.hover(H::local(300.0,40.0),0.2);
        let at=std::time::Instant::now();for _ in 0..3 {h.frame(vec![]);}let frames=at.elapsed()/3;
        h.app.show_page(0);h.hover(H::local(300.0,40.0),0.2);
        let at=std::time::Instant::now();for _ in 0..3 {h.frame(vec![]);}let other=at.elapsed()/3;
        println!("count={count} same_day={same_day} import_ms={:.2} calendar_frame_ms={:.2} other_frame_ms={:.2}",import.as_secs_f64()*1000.0,frames.as_secs_f64()*1000.0,other.as_secs_f64()*1000.0);
    }
}

#[test]
fn ics_import_preserves_recurring_overnight_and_all_day_events_on_reimport() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let day=chrono::Local::now().date_naive();
    let utc=|hour,minute|day.and_hms_opt(hour,minute,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap().with_timezone(&chrono::Utc);
    let start=utc(12,0);let overnight=utc(23,30);
    let ics=format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:daily\r\nSUMMARY:每日安排\r\nDTSTART:{}\r\nDTEND:{}\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:overnight\r\nSUMMARY:跨夜安排\r\nDTSTART:{}\r\nDTEND:{}\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:all-day\r\nSUMMARY:两天休假\r\nDTSTART;VALUE=DATE:{}\r\nDTEND;VALUE=DATE:{}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",start.format("%Y%m%dT%H%M%SZ"),(start+chrono::Duration::hours(1)).format("%Y%m%dT%H%M%SZ"),overnight.format("%Y%m%dT%H%M%SZ"),(overnight+chrono::Duration::hours(1)).format("%Y%m%dT%H%M%SZ"),day.format("%Y%m%d"),(day+chrono::Duration::days(2)).format("%Y%m%d"));
    let files=vec![("sample.ics".into(),ics)];
    h.app.import_ics(&files);assert_eq!(h.app.calendar.local.len(),5);
    h.app.import_ics(&files);assert_eq!(h.app.calendar.local.len(),5,"重复导入应跳过已有日程");
    h.app.calendar.local=crate::local_calendar::load().unwrap();
    assert_eq!(h.app.calendar_entries(day).len(),3);
    assert_eq!(h.app.calendar_entries(day+chrono::Duration::days(1)).len(),3);
    assert_eq!(h.app.calendar_entries(day+chrono::Duration::days(2)).len(),1);
    assert!(h.app.calendar_entries(day+chrono::Duration::days(3)).is_empty());
}

#[test]
fn local_calendar_editor_makes_storage_scope_visible_with_connected_accounts() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let cfg=crate::calendar::Config{provider:crate::calendar::Provider::Feishu,username:"test".into(),href:None,server:None};
    crate::calendar::save_config(&cfg).unwrap();h.app.calendar=crate::app::CalendarState::load();
    let remote=crate::mail::Meeting{all_day:false,title:"飞书会议".into(),start:chrono::Local::now().timestamp()+3600,end:None,at:String::new(),uid:"remote-event".into()};
    h.app.calendar.events.push(remote.clone());
    h.app.open_calendar(None);h.hover(H::local(300.0,40.0),1.0);
    h.click(H::local(124.0,18.0));h.hover(H::local(300.0,40.0),0.3);
    assert!(h.app.calendar.draft.is_some());
    let visible=|h:&H,text:&str|h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(t) if t.galley.text()==text));
    assert!(visible(&h,"仅保存在本机"));assert!(visible(&h,"保存到本机"));
    h.click(H::local(334.0,199.0));h.frame(vec![]);
    assert!(!h.app.calendar.draft.as_ref().unwrap().error.is_empty());
    assert!(visible(&h,"仅保存在本机"),"填写错误时仍需说明保存范围");
    h.app.calendar.draft.as_mut().unwrap().title="自己的安排".into();
    h.click(H::local(334.0,199.0));h.frame(vec![]);
    assert!(h.app.calendar.draft.is_none());assert_eq!(h.app.calendar.local.len(),1);
    assert_eq!(crate::local_calendar::load().unwrap()[0].event.title,"自己的安排");
    assert_eq!(h.app.calendar.events.len(),1);assert_eq!(h.app.calendar.events[0].uid,remote.uid);
    assert_eq!(crate::calendar::load_config().unwrap(),cfg);
    let uid=h.app.calendar.local[0].event.uid.clone();h.app.open_calendar_editor(Some(&uid));h.frame(vec![]);
    assert!(visible(&h,"仅保存在本机"));assert!(visible(&h,"保存到本机"));
}

#[test]
fn local_calendar_can_create_edit_reload_and_remind_without_accounts() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.open_calendar(None);h.app.open_calendar_editor(None);
    assert!(h.app.is_editing());
    h.app.save_calendar_editor();assert!(h.app.calendar.local.is_empty());
    assert!(!h.app.calendar.draft.as_ref().unwrap().error.is_empty());
    let start=chrono::Local::now()+chrono::Duration::minutes(5);
    let end=start+chrono::Duration::hours(1);
    let draft=h.app.calendar.draft.as_mut().unwrap();draft.title="自己的安排".into();
    draft.start_date=start.format("%Y-%m-%d").to_string();draft.start_time=start.format("%H:%M").to_string();
    draft.end_date=end.format("%Y-%m-%d").to_string();draft.end_time=end.format("%H:%M").to_string();
    h.app.save_calendar_editor();
    assert!(h.app.calendar.draft.is_none());assert_eq!(h.app.calendar.local.len(),1);
    let uid=h.app.calendar.local[0].event.uid.clone();
    assert_eq!(crate::local_calendar::load().unwrap()[0].event.title,"自己的安排");
    let entries=h.app.calendar_entries(start.date_naive());assert_eq!(entries.len(),1);assert!(matches!(entries[0].sources.as_slice(),[crate::calendar_ui::Source::Local]));
    h.frame(vec![]);
    assert!(h.app.history.iter().any(|alert|alert.meeting.as_ref().is_some_and(|(event,source)|event.uid==uid && source=="本机日程")));
    h.app.open_calendar_editor(Some(&uid));h.app.calendar.draft.as_mut().unwrap().title="改后的安排".into();h.app.save_calendar_editor();
    let saved=crate::local_calendar::load().unwrap();assert_eq!(saved.len(),1);assert_eq!(saved[0].event.uid,uid);assert_eq!(saved[0].event.title,"改后的安排");
    h.app.open_calendar_editor(Some(&uid));h.hover(H::local(300.0,40.0),1.0);
    h.click(H::local(84.0,199.0));assert_eq!(h.app.calendar.local.len(),1,"删除先确认");
    h.click(H::local(84.0,199.0));assert!(h.app.calendar.local.is_empty());assert!(crate::local_calendar::load().unwrap().is_empty());
}

#[test]
fn calendar_month_errors_remain_visible_until_retry_or_month_change(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let cfg=crate::calendar::Config{provider:crate::calendar::Provider::WeCom,username:"test".into(),href:None,server:None};
    crate::calendar::save_config(&cfg).unwrap();h.app.calendar=crate::app::CalendarState::load();
    h.app.request_calendar_month(true);h.app.pending.clear();
    let month=h.app.calendar.view.month;
    h.app.calendar_event(crate::calendar::Event::MonthLoaded{config:cfg,month,result:Err("日程时间无法解析".into())});
    h.app.request_calendar_month(false);
    assert!(!h.app.calendar.view.loading,"失败后不能逐帧重试，覆盖错误状态");
    assert!(h.app.pending.is_empty());
    h.app.request_calendar_month(true);
    assert!(h.app.calendar.view.loading,"用户手动重试应立即生效");
}

#[test]
fn calendar_combines_platforms_and_settings_tabs_do_not_filter_events() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    let feishu=crate::calendar::Config{provider:crate::calendar::Provider::Feishu,username:"飞书账号".into(),href:None,server:None};
    let ding=crate::calendar::Config{provider:crate::calendar::Provider::DingTalk,username:"钉钉账号".into(),..feishu.clone()};
    crate::calendar::save_config(&feishu).unwrap();crate::calendar::save_config(&ding).unwrap();
    h.app.calendar=crate::app::CalendarState::load();
    let day=chrono::Local::now().date_naive();
    let event=|uid:&str|crate::mail::Meeting{all_day:false,end:None,title:uid.into(),start:chrono::Local::now().timestamp()+60,at:"18:00".into(),uid:uid.into()};
    for (cfg,uid) in [(&feishu,"飞书会议"),(&ding,"钉钉会议")] {
        h.app.calendar_event(crate::calendar::Event::Synced{config:cfg.clone(),events:vec![event(uid)],at:"17:00".into()});
    }
    assert_eq!(h.app.calendar_entries(day).len(),2);
    h.frame(vec![]);
    for provider in [feishu.provider,ding.provider] {
        assert!(h.app.history.iter().any(|alert|alert.meeting.as_ref().is_some_and(|(_,source)|source==provider.label())),"所有平台都应提醒");
    }
    h.app.select_calendar_provider(feishu.provider);
    assert_eq!(h.app.calendar_entries(day).len(),2);
    assert_eq!(h.app.calendar.form.username,feishu.username);
    h.app.select_calendar_provider(crate::calendar::Provider::WeCom);
    assert_eq!(h.app.calendar_entries(day).len(),2);
    assert!(h.app.calendar.form.username.is_empty());
    h.app.request_calendar_month(true);
    h.app.calendar_event(crate::calendar::Event::MonthLoaded{config:feishu.clone(),month:crate::calendar::month_start(day),result:Ok(vec![event("飞书月历")])});
    assert!(h.app.calendar.view.loading,"另一平台尚未返回，仍应保持同步状态");
    h.app.calendar_event(crate::calendar::Event::MonthLoaded{config:ding.clone(),month:crate::calendar::month_start(day),result:Err("钉钉暂时不可用".into())});
    assert!(!h.app.calendar.view.loading);
    assert_eq!(h.app.calendar_entries(day).len(),2,"单平台失败不能清掉另一平台或自己的提醒缓存");
    let shared=event("同一个会议");
    for config in [&feishu,&ding] {
        h.app.calendar_event(crate::calendar::Event::MonthLoaded{config:config.clone(),month:crate::calendar::month_start(day),result:Ok(vec![shared.clone()])});
    }
    let entries=h.app.calendar_entries(day);
    assert_eq!(entries.len(),1);
    assert_eq!(entries[0].sources.len(),2,"同一个会议只显示一次，但保留两个平台的来源");
    h.app.select_calendar_provider(feishu.provider);
    h.app.forget_calendar();
    assert_eq!(h.app.calendar_entries(day).len(),1);
    h.app.calendar_event(crate::calendar::Event::Synced{config:feishu,events:vec![event("旧连接结果")],at:"17:01".into()});
    assert_eq!(h.app.calendar_entries(day).len(),1);
}

#[test]
fn switching_music_source_discards_previous_lyrics() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"cloudmusic.exe".into(),title:"Track".into(),artist:"Artist".into(),duration:120.0,..Default::default()};
    h.frame(vec![]);
    for source in ["QQMusic.exe","cloudmusic.exe","汽水音乐","KuGou.exe","QQMusic.exe"] {
        h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:01]旧来源歌词"));
        h.media.source=source.into();
        h.frame(vec![]);
        assert!(h.app.ly.lyrics.is_none(),"切换到 {source} 后必须重新选择歌词来源");
    }
}

#[test]
fn closing_a_player_then_starting_native_netease_restores_music_and_lyrics(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{source:"SodaMusic.exe".into(),title:"旧歌曲".into(),artist:"歌手".into(),playing:true,duration:120.0,..Default::default()};h.frame(vec![]);
    h.app.set_lyrics("旧歌曲","歌手",120.0,crate::lyrics::parse_lrc("[00:01]旧歌词"));
    h.media=Default::default();h.frame(vec![]);assert!(h.app.media.title.is_empty());assert!(h.app.ly.lyrics.is_none());
    h.media=crate::smtc::NowPlaying{source:"cloudmusic.exe".into(),song_id:Some(123),title:"新歌曲".into(),artist:"歌手".into(),playing:true,position:7.0,duration:120.0,..Default::default()};h.frame(vec![]);
    h.app.set_lyrics("新歌曲","歌手",120.0,crate::lyrics::parse_lrc("[00:05]新歌词\n[00:10]下一句"));h.frame(vec![]);
    assert_eq!(h.app.media.title,"新歌曲");assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("新歌词"));
    h.media.song_id=Some(124);h.frame(vec![]);assert!(h.app.ly.lyrics.is_none(),"歌名相同但歌曲 ID 不同仍需重新取词");
}

#[test]
fn settings_keep_dark_buttons_when_system_theme_changes() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let h=H::new();
    for theme in [egui::Theme::Light,egui::Theme::Dark,egui::Theme::Light] {
        let _=h.ctx.run(egui::RawInput{system_theme:Some(theme),..Default::default()},|ctx| {
            assert!(ctx.style().visuals.widgets.inactive.weak_bg_fill.r()<100,"系统浅色模式不能把深色设置里的按钮变成白底");
        });
    }
}

#[test]
fn expanded_settings_routes_preserve_inline_controls(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for (page,tab) in crate::app::PAGE_SETTINGS_TABS.into_iter().enumerate(){
        let mut h=H::new();h.app.show_page(page);h.hover(H::local(200.0,40.0),1.0);
        h.click(H::local(394.0,18.0));
        assert!(h.app.settings_open,"页面 {page} 应打开设置窗口");
        assert_eq!(h.app.settings_tab,tab);assert_eq!(h.app.page,page);
        assert!(h.app.subpage.is_none());
    }
    for (page,point) in [(1,H::local(111.0,18.0)),(2,H::local(149.0,110.0))]{
        let mut h=H::new();h.app.show_page(page);h.hover(H::local(200.0,40.0),1.0);
        h.click(point);assert_eq!(h.app.subpage,Some(page));assert!(!h.app.settings_open);
        h.click(H::local(394.0,18.0));assert!(h.app.settings_open);
        assert_eq!(h.app.subpage,Some(page),"打开完整设置后应保留岛内调整页");
    }
    let mut h=H::new();h.app.show_page(3);h.hover(H::local(200.0,40.0),1.0);
    h.click(H::local(330.0,18.0));assert!(h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::AddFiles)));
    assert!(!h.app.settings_open);
    h.app.show_page(6);h.hover(H::local(200.0,40.0),1.0);h.click(H::local(319.0,18.0));
    assert!(h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::QuotaRefresh)));assert!(!h.app.settings_open);
}

#[test]
fn quality_late_calendar_response_does_not_restore_disconnected_account() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let config=crate::calendar::Config{provider:crate::calendar::Provider::Feishu,username:"old".into(),href:None,server:None};
    h.app.forget_calendar();
    h.app.calendar_event(crate::calendar::Event::Synced{config:config.clone(),events:vec![],at:"12:00".into()});
    assert!(!h.app.calendar.connected && h.app.calendar.last_sync.is_empty());
    h.app.calendar.config=Some(crate::calendar::Config{username:"new".into(),..config.clone()});
    h.app.calendar.connected=true;
    h.app.calendar_event(crate::calendar::Event::Failed{config,message:"旧连接失败".into()});
    assert!(h.app.calendar.connected && h.app.calendar.error.is_empty());
}

#[test]
fn quality_distinct_reminders_are_queued_instead_of_overwritten() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let meeting=crate::mail::Meeting{ all_day:false,title:"会议一".into(),start:chrono::Local::now().timestamp()+600,end:None,at:String::new(),uid:"one".into()};
    h.app.meeting_alert(meeting.clone(),"飞书".into(),7);
    h.app.meeting_alert(crate::mail::Meeting{ all_day:false,title:"会议二".into(),uid:"two".into(),..meeting.clone()},"飞书".into(),7);
    assert_eq!(h.app.alerts.len(),2,"同时到期的两场会议都需要提醒");
    h.app.meeting_alert(meeting,"飞书".into(),7);assert_eq!(h.app.alerts.len(),2,"同一会议重复更新应合并");
    h.app.alert("待办到时间了","任务一",4,true);h.app.alert("待办到时间了","任务二",4,true);
    assert_eq!(h.app.alerts.iter().filter(|a|a.title=="待办到时间了").count(),2);
    h.app.alert("系统音量","10%",0,false);h.app.alert("系统音量","20%",0,false);
    assert_eq!(h.app.alerts.iter().filter(|a|a.title=="系统音量").count(),1);
}

#[test]
fn quality_removed_mail_accounts_ignore_in_flight_results() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.mail_event(crate::mail::Event::Status{account:"removed@example.com".into(),status:crate::mail::Status::Ready});
    h.app.mail_event(crate::mail::Event::Mail{item:crate::mail::Item {received_unix: None, account:"removed@example.com".into(),from:"发件人".into(),subject:"延迟邮件".into(),time:String::new(),code:None,html: String::new().into(), summary:String::new().into(),links:vec![],meeting:None,uid:1},new:true});
    assert!(h.app.mail.status.is_empty() && h.app.mail.recent.is_empty());
    assert!(h.app.notification_bell_until<=h.t);
}

#[test]
fn glass_pill_rim_stays_on_outline_while_collapsing() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for ppp in [1.0,1.25,1.5,2.0] {
        let mut h=H::new();
        h.app.settings.skin=crate::model::Skin::GlassLight;
        h.app.settings.island_shape=crate::model::IslandShape::Pill;
        h.ctx.set_zoom_factor(ppp);
        h.hover(Pos2::new(WIN_W/2.0,15.0),1.5);
        assert!(h.app.progress()>0.99);
        h.cursor=None;
        let mut raster=crate::preview_raster::Raster::new((crate::app::CANVAS_W*ppp) as usize,(WIN_H*ppp) as usize);
        let mut checked=0;
        for frame in 0..100 {
            h.frame(vec![egui::Event::PointerGone]);
            let rims=h.shapes.iter().filter_map(|s|match &s.shape {
                egui::Shape::Rect(r) if r.rect.width()>100.0 && r.rect.height()>=10.0=>Some((s.clone(),r.rect,if r.stroke.is_empty(){r.fill.a() as f32/255.0}else{0.0})),
                _=>None,
            }).collect::<Vec<_>>();
            for (shape,rect,alpha) in rims {
                checked+=1;
                raster.buf.fill([0.0;4]);
                raster.draw(&h.ctx.tessellate(vec![shape],ppp),ppp);
                let y=(rect.center().y*ppp).floor() as usize;
                let left=((rect.left()+4.0)*ppp).ceil() as usize;
                let right=((rect.right()-4.0)*ppp).floor() as usize;
                for x in left..right {
                    assert!((raster.buf[y*raster.w+x][3]-alpha).abs()<0.02,"胶囊中心出现接缝：ppp={ppp}, frame={frame}, rect={rect:?}, x={x}, alpha={alpha}, actual={}",raster.buf[y*raster.w+x][3]);
                }
            }
        }
        assert!(checked>=100,"应覆盖完整收起动画");
    }
}

#[test]
fn oled_shift_waits_for_the_panel_to_finish_closing() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.oled_shift=true;
    h.app.show_page(0);
    h.hover(H::local(200.0,60.0),5.0);
    assert!(h.app.open);
    h.cursor=Some(Pos2::new(600.0,180.0));
    for _ in 0..120 {
        h.frame(vec![]);
        if !h.app.open {break;}
    }
    assert!(!h.app.open);
    assert!(h.app.progress()>0.1,"测试应停在收起动画尚未完成时");
    assert!(!h.app.oled_shift_idle(&h.ctx),"收起动画仍有 {:.1}% 展开程度时不能恢复位移",h.app.progress()*100.0);
    h.hover(Pos2::new(600.0,180.0),1.0);
    assert!(h.app.oled_shift_idle(&h.ctx),"动画完成后应恢复位移");
}

#[test]
fn oled_shift_pauses_during_hover_intent() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.oled_shift=true;
    h.hover(Pos2::new(600.0,180.0),1.0);
    assert!(h.app.oled_shift_idle(&h.ctx));
    h.hover(Pos2::new(WIN_W/2.0,12.0),0.05);
    assert!(!h.app.open,"悬停意图缓冲期间尚未展开");
    assert!(h.hit.contains(Pos2::new(WIN_W/2.0,12.0)),"鼠标已进入岛体交互区域");
    assert!(!h.app.oled_shift_idle(&h.ctx),"悬停交互期间不能移动岛体");
}

#[test]
fn oled_shift_pauses_on_the_first_pointer_press_frame() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();let away=Pos2::new(600.0,180.0);
    h.hover(away,1.0);assert!(h.app.oled_shift_idle(&h.ctx));
    h.frame(vec![egui::Event::PointerButton{pos:away,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
    assert!(!h.app.oled_shift_idle(&h.ctx),"按下的首帧必须暂停位移");
    h.frame(vec![]);assert!(!h.app.oled_shift_idle(&h.ctx),"按住期间必须暂停位移");
    h.frame(vec![egui::Event::PointerButton{pos:away,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
    assert!(h.app.oled_shift_idle(&h.ctx),"松开且没有交互后应恢复位移");
}

#[test]
fn oled_shift_waits_for_side_panels_to_finish_closing() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dock in [crate::model::Dock::Left,crate::model::Dock::Right] {
        let mut h=H::new();h.app.settings.dock=dock;h.app.show_page(0);
        let inside=Some(Pos2::new(if dock==crate::model::Dock::Left {100.0}else{400.0},300.0));
        for _ in 0..300 {side_frame(&mut h,inside);}
        assert!(h.app.open);
        for _ in 0..120 {side_frame(&mut h,None);if !h.app.open {break;}}
        assert!(!h.app.open && h.app.progress()>0.1);
        assert!(!h.app.oled_shift_idle(&h.ctx),"{dock:?} 收起动画期间不能移动");
        for _ in 0..120 {side_frame(&mut h,None);}
        assert!(h.app.oled_shift_idle(&h.ctx),"{dock:?} 动画完成后应恢复位移");
    }
}

struct H {
    ctx: egui::Context,
    app: App,
    t: f64,
    cursor: Option<Pos2>,
    cmds:Vec<crate::app::Cmd>,
    region:Rect,
    hit:Rect,
    animating:bool,
    shapes:Vec<egui::epaint::ClippedShape>,
    bt: Vec<crate::bt::Device>,
    media: crate::smtc::NowPlaying,
    power: (Option<u8>, bool),
    network: Option<bool>,
    system_idle: Option<f64>,
    screen_locked: bool,
    fullscreen: bool,
    browser_at_top: Option<bool>,
    file_drop_zone:Option<Rect>,
    hovered_files:Vec<egui::HoveredFile>,
    dropped_files:Vec<egui::DroppedFile>,
}

impl H {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("daozai-test-{}-{}", std::process::id(), rand_id()));
        std::env::set_var("DAOZAI_DATA_DIR", &dir);
        let ctx = egui::Context::default();
        let app = App::new(&ctx, Options { fetch_lyrics: false, pet_idle: 1e9 });
        Self { ctx, app, t: 0.0, cursor: None,cmds:vec![],region:Rect::NOTHING,hit:Rect::NOTHING,animating:false,shapes:Vec::new(), bt:Vec::new(),media:Default::default(),power:(None,false),network:None,system_idle:None,screen_locked:false,fullscreen:false,browser_at_top:None,file_drop_zone:None,hovered_files:vec![],dropped_files:vec![] }
    }
    fn frame(&mut self, events: Vec<egui::Event>) {
        self.t += 1.0 / 60.0;
        let mut raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(crate::app::CANVAS_W, WIN_H))),
            time: Some(self.t),
            events,
            hovered_files:self.hovered_files.clone(),
            dropped_files:std::mem::take(&mut self.dropped_files),
            focused: true,
            ..Default::default()
        };
        self.app.filter_todo_input(&mut raw);
        let env = Env {
            t: self.t,
            clock: "09:41:00".into(),
            today: "2026-10-28".into(),
            date: "10月28日 周三".into(),
            cursor: self.cursor,
            bt: self.bt.clone(),
            bt_ready: true,
            media:self.media.clone(),
            battery:self.power.0,
            charging:self.power.1,
            network:self.network,
            idle_seconds:self.system_idle,
            screen_locked:self.screen_locked,
            fullscreen_hidden:self.fullscreen && self.app.settings.hide_fullscreen,
            browser_at_top:self.browser_at_top,
            ..Default::default()
        };
        let cmds=&mut self.cmds;
        let region=&mut self.region;
        let hit=&mut self.hit;
        let animating=&mut self.animating;
        let app = &mut self.app;
        let file_drop_zone=&mut self.file_drop_zone;
        let output = self.ctx.run(raw, |ctx| {
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
                let win = Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,WIN_H));
                let frame=app.frame(ui, win, &env);*region=frame.region;*hit=frame.hit;*animating=frame.animating;*file_drop_zone=frame.file_drop_zone;cmds.extend(frame.cmds);
            });
        });
        self.shapes=output.shapes;
    }
    fn hover(&mut self, p: Pos2, secs: f64) {
        self.cursor = Some(p);
        let n = (secs * 60.0) as usize;
        for _ in 0..n {
            self.frame(vec![egui::Event::PointerMoved(p)]);
        }
    }
    fn click(&mut self, p: Pos2) {
        self.cursor = Some(p);
        self.frame(vec![egui::Event::PointerMoved(p)]);
        self.frame(vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        self.frame(vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
    }
    fn click_text(&mut self,label:&str){
        if !self.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==label))&&self.app.notice.is_some(){self.click_notice_badge();}
        let point=self.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==label&&t.opacity_factor>0.9&&s.clip_rect.contains(t.visual_bounding_rect().center())=>Some(t.visual_bounding_rect().center()),_=>None}).unwrap_or_else(||panic!("未找到可点击文字：{label}"));
        self.click(point);
    }
    fn click_notice_badge(&mut self){if self.app.compact_notices(){self.click(Pos2::new(WIN_W/2.0+(self.app.current_width()-44.0)/2.0+26.0,15.0));}}
    fn wheel(&mut self,delta:f32) {
        self.frame(vec![egui::Event::MouseWheel{unit:egui::MouseWheelUnit::Point,delta:Vec2::new(0.0,delta),modifiers:Default::default()}]);
    }
    fn picker_frame(&mut self,events:Vec<egui::Event>){
        self.frame(events);
    }
    fn picker_click(&mut self,label:&str){
        for _ in 0..60 {self.picker_frame(vec![]);if (self.app.progress()-1.0).abs()<0.01 {break;}}
        self.picker_frame(vec![]);
        let find=|h:&H|h.shapes.iter().find_map(|shape|match &shape.shape{
            egui::Shape::Text(text) if text.galley.text()==label && shape.clip_rect.contains(text.visual_bounding_rect().center())=>Some(text.visual_bounding_rect().center()),_=>None,
        });
        let point=find(self).unwrap_or_else(||panic!("未找到可见选项：{label}"));
        assert!(self.hit.contains(point),"选项 {label} 超出岛体命中区域");
        self.picker_frame(vec![egui::Event::PointerMoved(point)]);
        for pressed in [true,false]{self.picker_frame(vec![egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);}
    }
    fn local(x: f32, y: f32) -> Pos2 {
        Pos2::new((WIN_W - PW) / 2.0 + x, y)
    }
    fn quota_accounts(&mut self,count:usize) {
        use crate::quota::{Account,Provider,Snapshot,Window};
        for i in 0..count {
            let id=format!("preview-{i}");
            self.app.quota.config.accounts.push(Account{id:id.clone(),provider:Provider::ALL[i%3],alias:format!("账号 {}",i+1),source:format!("preview-{i}.json").into(),slot:None,enabled:true,pinned:true});
            self.app.quota_event(crate::quota::Event{id,result:Ok(Snapshot{windows:vec![Window{label:"本周".into(),seconds:Some(604800),remaining:100.0,reset:None,amount:None}],..Default::default()})});
        }
    }
    fn quota_centers(&self)->Vec<f32> {
        fn collect(shape:&egui::Shape,out:&mut Vec<f32>) {
            match shape {
                egui::Shape::Vec(shapes)=>for shape in shapes {collect(shape,out);},
                egui::Shape::Path(path) if path.points.len()>50=>{
                    let min=path.points.iter().map(|p|p.x).fold(f32::INFINITY,f32::min);
                    let max=path.points.iter().map(|p|p.x).fold(f32::NEG_INFINITY,f32::max);
                    if (20.0..28.0).contains(&(max-min)) {out.push((min+max)/2.0);}
                },
                egui::Shape::Circle(circle) if (10.0..14.0).contains(&circle.radius) && circle.stroke.width>0.0 && (circle.center.y-15.0).abs()<0.5=>out.push(circle.center.x),
                _=>{},
            }
        }
        let mut out=Vec::new();for shape in &self.shapes {collect(&shape.shape,&mut out);}
        out.sort_by(f32::total_cmp);out
    }
}

fn rand_id() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64
}

fn island_drag_frame(h:&mut H,bounds:crate::display_layout::Bounds,ppp:f32,pointer:Option<Pos2>,pressed:Option<bool>)->Rect {
    use crate::{display_layout,side_dock};
    let dock=h.app.settings.dock;
    let (width,height)=if dock.is_side(){(side_dock::SIDE_W,side_dock::SIDE_H)}else{(WIN_W,WIN_H)};
    let offset=h.app.placement_offset();
    let (x,y)=if dock.is_side(){display_layout::side_origin(bounds,dock,(width,height),side_dock::SIDE_HALF_EXTENT,ppp,offset)}
        else{display_layout::island_origin_offset(bounds,width,ppp,offset)}.unwrap();
    let screen=Rect::from_min_max(Pos2::new((bounds.left-x) as f32/ppp,(bounds.top-y) as f32/ppp),Pos2::new((bounds.right-x) as f32/ppp,(bounds.bottom-y) as f32/ppp));
    let cursor=pointer.map(|p|p+screen.min.to_vec2());
    let mut events=cursor.map(|p|vec![egui::Event::PointerMoved(p)]).unwrap_or_default();
    if let (Some(pos),Some(pressed))=(cursor,pressed){events.push(egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()});}
    h.t+=1.0/60.0;
    let env=Env{t:h.t,clock:"09:41:00".into(),today:"2026-10-28".into(),date:"10月28日 周三".into(),cursor,screen:Some(screen),bt_ready:true,..Default::default()};
    let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W.max(width),height))),time:Some(h.t),events,focused:true,
        viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
    let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{
        let out=h.app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(width,height)),&env);h.region=out.region;h.hit=out.hit;h.cmds.extend(out.cmds);
    });});
    h.shapes=output.shapes;
    screen
}

fn island_drag_header(h:&mut H,bounds:crate::display_layout::Bounds,ppp:f32)->Pos2 {
    h.app.show_page(1);
    let mut screen=Rect::NOTHING;
    for _ in 0..90{screen=island_drag_frame(h,bounds,ppp,None,None);}
    h.shapes.iter().find_map(|shape|match &shape.shape{
        egui::Shape::Text(text) if text.galley.text()=="10月28日 周三"=>Some(text.visual_bounding_rect().center()-screen.min.to_vec2()),_=>None,
    }).expect("展开后的日期应可见")
}

#[test]
fn island_position_drag_tracks_screen_coordinates_and_saves_on_release(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let bounds=crate::display_layout::Bounds{left:-2560,top:-400,right:0,bottom:1040};
    for dock in crate::model::Dock::ALL {for ppp in [1.0,1.5] {
        let mut h=H::new();h.app.settings.dock=dock;h.app.settings.island_offset=-40;h.app.settings.save();
        // 起点离居中足够远，90 点位移不会落进居中吸附范围。
        let start=island_drag_header(&mut h,bounds,ppp);
        let before=island_drag_frame(&mut h,bounds,ppp,Some(start),Some(true));
        let delta=if dock.is_side(){Vec2::new(0.0,90.0)}else{Vec2::new(90.0,0.0)};
        let end=start+delta;
        island_drag_frame(&mut h,bounds,ppp,Some(end),None);
        assert!(h.app.settings.island_offset>-40,"{dock:?} / {ppp}：拖动应同步现有位置参数");
        assert_eq!(crate::model::Settings::load().island_offset,-40,"拖动途中不能反复写配置");
        let offset=h.app.settings.island_offset;
        for _ in 0..30{island_drag_frame(&mut h,bounds,ppp,Some(end),None);}
        let after=island_drag_frame(&mut h,bounds,ppp,Some(end),None);
        let movement=before.min-after.min;
        assert!(((if dock.is_side(){movement.y}else{movement.x})-90.0).abs()<6.0,"{dock:?} / {ppp}：窗口应跟随实际鼠标位移，允许整数位置刻度带来的取整误差");
        assert_eq!(if dock.is_side(){movement.x}else{movement.y},0.0,"拖动不能脱离停靠边缘");
        assert_eq!(h.app.settings.island_offset,offset,"窗口跟随鼠标移动后，原地持有不能发生累积漂移");
        island_drag_frame(&mut h,bounds,ppp,Some(end),Some(false));
        assert_eq!(crate::model::Settings::load().island_offset,offset,"松手后应保存位置");
        assert_eq!((h.app.settings.dock,h.app.settings.display_id.as_str()),(dock,""));
    }}
}

#[test]
fn island_position_drag_clamps_to_existing_limits_and_stays_resident(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let bounds=crate::display_layout::Bounds{left:0,top:0,right:1920,bottom:1080};
    for dock in crate::model::Dock::ALL {for shape in crate::model::IslandShape::ALL {
        let mut h=H::new();h.app.settings.dock=dock;h.app.settings.island_shape=shape;
        let start=island_drag_header(&mut h,bounds,1.0);
        island_drag_frame(&mut h,bounds,1.0,Some(start),Some(true));
        let end=if dock.is_side(){Pos2::new(start.x,3000.0)}else{Pos2::new(1855.0,start.y)};
        for _ in 0..120{island_drag_frame(&mut h,bounds,1.0,Some(end),None);}
        assert!(!h.app.open,"{dock:?} 拖动期间应保持常驻形态");
        assert!(h.app.progress()<0.01,"拖动期间不应保留展开面板");
        assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="10月28日 周三")),"拖动期间不应显示展开顶栏");
        assert_eq!(h.app.settings.island_offset,crate::model::ISLAND_OFFSET_MAX);
        let end=if dock.is_side(){Pos2::new(start.x,65.0)}else{Pos2::new(65.0,start.y)};
        island_drag_frame(&mut h,bounds,1.0,Some(end),None);
        assert_eq!(h.app.settings.island_offset,-crate::model::ISLAND_OFFSET_MAX);
        assert_eq!(h.app.settings.dock,dock);
        island_drag_frame(&mut h,bounds,1.0,Some(end),Some(false));
        let offset=h.app.settings.island_offset;
        for _ in 0..40{island_drag_frame(&mut h,bounds,1.0,Some(start),None);}
        assert_eq!(h.app.settings.island_offset,offset,"松手后不能继续跟着鼠标移动");
    }}
}

#[test]
fn island_position_drag_below_a_top_bar_switches_edges_like_the_bare_screen(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    use crate::model::{Dock,Settings};
    let monitor=crate::display_layout::Bounds{left:0,top:0,right:1920,bottom:1080};
    let display=crate::display_layout::Display{id:"d".into(),label:"d".into(),bounds:monitor,work_top:28,primary:true};
    let bounds=crate::display_layout::placement_bounds(&display,true);
    for to in [Dock::Left,Dock::Right] {
        let mut h=H::new();h.app.settings.save();
        let start=island_drag_header(&mut h,bounds,1.0);
        island_drag_frame(&mut h,bounds,1.0,Some(start),Some(true));
        let size=Vec2::new(1920.0,1052.0);
        let end=if to==Dock::Left{Pos2::new(0.0,size.y*0.6)}else{Pos2::new(size.x,size.y*0.6)};
        for _ in 0..30{island_drag_frame(&mut h,bounds,1.0,Some(end),None);}
        assert_eq!(h.app.settings.dock,to);
        let back=Pos2::new(size.x*0.5,0.0);
        for _ in 0..30{island_drag_frame(&mut h,bounds,1.0,Some(back),None);}
        assert_eq!(h.app.settings.dock,Dock::Top,"拖回顶栏下沿应能回到顶部");
        island_drag_frame(&mut h,bounds,1.0,Some(back),Some(false));
        assert_eq!(Settings::load().dock,Dock::Top);
        assert!(Settings::load().avoid_top_bar,"拖动不改变避开顶栏设置");
    }
}

#[test]
fn island_position_drag_switches_between_all_three_edges_and_saves_on_release(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    use crate::model::{Dock,IslandShape,Settings};
    let bounds=crate::display_layout::Bounds{left:-2560,top:-400,right:0,bottom:1040};
    for from in Dock::ALL {for to in Dock::ALL.into_iter().filter(|to|*to!=from) {for ppp in [1.0,1.5] {for shape in IslandShape::ALL {
        let mut h=H::new();h.app.settings.dock=from;h.app.settings.island_shape=shape;h.app.settings.save();
        let start=island_drag_header(&mut h,bounds,ppp);
        island_drag_frame(&mut h,bounds,ppp,Some(start),Some(true));
        let size=Vec2::new(2560.0,1440.0)/ppp;
        let end=match to {Dock::Top=>Pos2::new(size.x*0.6,0.0),Dock::Left=>Pos2::new(0.0,size.y*0.6),Dock::Right=>Pos2::new(size.x,size.y*0.6)};
        for _ in 0..30{island_drag_frame(&mut h,bounds,ppp,Some(end),None);}
        assert_eq!(h.app.settings.dock,to,"{from:?} → {to:?} / {ppp} / {shape:?}");
        assert!(!h.app.open && h.app.progress()<0.01,"切换停靠边后仍应显示常驻形态");
        assert_eq!(Settings::load().dock,from,"拖动中不能写入停靠设置");
        let offset=h.app.settings.island_offset;
        for _ in 0..15{island_drag_frame(&mut h,bounds,ppp,Some(end),None);}
        assert_eq!(h.app.settings.island_offset,offset,"换边后持有不能漂移");
        island_drag_frame(&mut h,bounds,ppp,Some(end),Some(false));
        let saved=Settings::load();assert_eq!((saved.dock,saved.island_offset),(to,offset));
        for _ in 0..30{island_drag_frame(&mut h,bounds,ppp,Some(end),None);}
        assert!(!h.app.open,"松手后应留在常驻形态");
        let screen=island_drag_frame(&mut h,bounds,ppp,Some(end),None);
        let resident=h.hit.center()-screen.min.to_vec2();
        for _ in 0..30{island_drag_frame(&mut h,bounds,ppp,Some(resident),None);}
        assert!(!h.app.open,"松手后光标留在岛上不能立即展开");
        for _ in 0..30{island_drag_frame(&mut h,bounds,ppp,None,None);}
        for _ in 0..60{island_drag_frame(&mut h,bounds,ppp,Some(resident),None);}
        assert!(h.app.open,"光标离开再悬停，应能正常展开");
        assert_eq!(h.app.settings.display_id,"");
    }}}}
}

#[test]
fn island_position_drag_keeps_the_edge_in_the_middle_and_does_not_flap_at_corners(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    use crate::model::Dock;
    let bounds=crate::display_layout::Bounds{left:0,top:0,right:1920,bottom:1080};
    let mut h=H::new();let start=island_drag_header(&mut h,bounds,1.0);
    island_drag_frame(&mut h,bounds,1.0,Some(start),Some(true));
    for (point,dock) in [(Pos2::new(500.0,500.0),Dock::Top),(Pos2::new(0.0,500.0),Dock::Left),
        (Pos2::new(960.0,540.0),Dock::Left),(Pos2::new(1250.0,540.0),Dock::Right),(Pos2::new(1000.0,540.0),Dock::Right),
        (Pos2::new(700.0,540.0),Dock::Left),(Pos2::new(4.0,3.0),Dock::Left),
        (Pos2::new(3.0,4.0),Dock::Left),(Pos2::new(100.0,0.0),Dock::Top),
        (Pos2::new(1920.0,500.0),Dock::Right),(Pos2::new(1918.0,3.0),Dock::Right),
        (Pos2::new(1500.0,0.0),Dock::Top)] {
        for _ in 0..5{island_drag_frame(&mut h,bounds,1.0,Some(point),None);}
        assert_eq!(h.app.settings.dock,dock,"光标={point:?}");
        assert!(!h.app.open);
    }
    island_drag_frame(&mut h,bounds,1.0,None,Some(false));
    island_drag_frame(&mut h,bounds,1.0,Some(start),Some(false));
    let dock=h.app.settings.dock;let offset=h.app.settings.island_offset;
    island_drag_frame(&mut h,bounds,1.0,Some(Pos2::new(0.0,500.0)),None);
    assert_eq!((h.app.settings.dock,h.app.settings.island_offset),(dock,offset),"松手后不能继续拖动");
}

#[test]
fn island_position_drag_lifts_on_hold_glides_smoothly_and_snaps_to_center(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let bounds=crate::display_layout::Bounds{left:0,top:0,right:1920,bottom:1080};
    let mut h=H::new();h.app.settings.island_offset=-30;h.app.settings.save();
    let start=island_drag_header(&mut h,bounds,1.0);
    island_drag_frame(&mut h,bounds,1.0,Some(start),Some(true));
    // 原地按住片刻即拿起：面板开始收起，位置不变。
    for _ in 0..20{island_drag_frame(&mut h,bounds,1.0,Some(start),None);}
    assert!(!h.app.open && h.app.progress()<0.5,"按住不动也应拿起岛体");
    assert_eq!(h.app.settings.island_offset,-30);
    // 窗口平滑跟随：一帧跳过去的光标，窗口要分几帧追上，且不按整数刻度跳。
    let travel=1920.0/2.0-crate::display_layout::ISLAND_HALF_EXTENT;
    let to=|units:f32|Pos2::new(start.x+(units+30.0)/100.0*travel,start.y);
    island_drag_frame(&mut h,bounds,1.0,Some(to(-10.0)),None);
    island_drag_frame(&mut h,bounds,1.0,Some(to(-10.0)),None);
    let mid=h.app.placement_offset();
    assert!(mid> -0.30 && mid< -0.10,"窗口应在途中，而不是瞬间到位：{mid}");
    for _ in 0..40{island_drag_frame(&mut h,bounds,1.0,Some(to(-10.0)),None);}
    assert!((h.app.placement_offset()+0.10).abs()<0.003,"持有后应追上光标");
    // 居中附近吸附，离开吸附范围后恢复跟随。
    assert!(h.app.center_guide>0.2 && h.app.center_guide<0.9 && h.app.center_lock_a<0.05,"接近居中时准线渐显，但还没点亮：{}",h.app.center_guide);
    island_drag_frame(&mut h,bounds,1.0,Some(to(2.0)),None);
    assert_eq!(h.app.settings.island_offset,0,"靠近居中应吸附");
    assert!(h.app.center_locked,"吸附后应标记为已居中");
    for _ in 0..30{island_drag_frame(&mut h,bounds,1.0,Some(to(2.0)),None);}
    assert!(h.app.center_guide>0.9 && h.app.center_lock_a>0.9,"居中时准线应清晰可见并切换为已居中配色");
    // 吸住后有滞回：稍微拖出一点仍停在中间，拖得更远才松开。
    island_drag_frame(&mut h,bounds,1.0,Some(to(7.0)),None);
    assert_eq!(h.app.settings.island_offset,0,"居中后轻微偏移不应松开");
    island_drag_frame(&mut h,bounds,1.0,Some(to(12.0)),None);
    assert_eq!(h.app.settings.island_offset,12);
    assert!(!h.app.center_locked);
    island_drag_frame(&mut h,bounds,1.0,Some(to(-28.5)),None);
    assert_eq!(h.app.settings.island_offset,-30,"靠近原位置应吸附回去");
    island_drag_frame(&mut h,bounds,1.0,Some(to(2.0)),None);
    island_drag_frame(&mut h,bounds,1.0,Some(to(2.0)),Some(false));
    assert_eq!(crate::model::Settings::load().island_offset,0,"松手保存吸附后的位置");
    for _ in 0..90{island_drag_frame(&mut h,bounds,1.0,Some(to(2.0)),None);}
    assert_eq!(h.app.placement_offset(),0.0,"落位后回到保存的位置");
}

#[test]
fn island_position_drag_ignores_jitter_and_preserves_header_buttons(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let bounds=crate::display_layout::Bounds{left:0,top:0,right:1920,bottom:1080};
    let mut h=H::new();let start=island_drag_header(&mut h,bounds,1.0);
    island_drag_frame(&mut h,bounds,1.0,Some(start),Some(true));
    island_drag_frame(&mut h,bounds,1.0,Some(start+Vec2::splat(1.0)),None);
    island_drag_frame(&mut h,bounds,1.0,Some(start+Vec2::splat(1.0)),Some(false));
    assert_eq!(h.app.settings.island_offset,0,"轻微手抖不能改变位置");
    let screen=island_drag_frame(&mut h,bounds,1.0,Some(start),None);
    let gear=H::local(394.0,18.0)-screen.min.to_vec2();
    island_drag_frame(&mut h,bounds,1.0,Some(gear),Some(true));
    island_drag_frame(&mut h,bounds,1.0,Some(gear),Some(false));
    assert!(h.app.settings_open,"时间区域拖动不能拦截设置按钮");
    assert_eq!(h.app.settings.island_offset,0);
    h.app.settings_open=false;
    for (page,x,expected) in [(3,330.0,crate::app::Cmd::AddFiles),(6,319.0,crate::app::Cmd::QuotaRefresh)] {
        h.app.show_page(page);
        for _ in 0..60{island_drag_frame(&mut h,bounds,1.0,Some(start),None);}
        let screen=island_drag_frame(&mut h,bounds,1.0,Some(start),None);
        let point=H::local(x,18.0)-screen.min.to_vec2();
        island_drag_frame(&mut h,bounds,1.0,Some(point),Some(true));
        island_drag_frame(&mut h,bounds,1.0,Some(point),Some(false));
        assert!(h.cmds.contains(&expected),"顶部操作按钮应正常响应");
        assert_eq!(h.app.settings.island_offset,0);
    }
}
#[test]
fn shelf_drop_zone_leaves_normal_hover_and_text_selection_unchanged(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    for _ in 0..90{h.frame(vec![]);}let zone=h.file_drop_zone.unwrap();
    assert_eq!(zone.width(),PW);assert_eq!(zone.height(),96.0);
    let point=zone.right_bottom()-Vec2::splat(12.0);assert!(!h.hit.contains(point));
    h.hover(point,0.8);assert!(!h.app.open);
    h.frame(vec![egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
    h.hover(point,0.8);assert!(!h.app.open,"按住鼠标拖选文字不应打开置物架");
    h.frame(vec![egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
    h.app.settings.visible_pages[3]=false;h.frame(vec![]);assert!(h.file_drop_zone.is_none());
}

#[test]
fn shelf_file_drag_opens_below_the_island_and_cancellation_restores_the_page(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.go(1,1.0);for _ in 0..90{h.frame(vec![]);}
    let point=h.file_drop_zone.unwrap().right_bottom()-Vec2::splat(12.0);
    assert!(!h.hit.contains(point));h.cursor=Some(point);
    h.hovered_files.push(egui::HoveredFile{path:Some(crate::model::config_dir().join("拖入文件.txt")),..Default::default()});
    h.frame(vec![]);assert!(h.app.open && h.app.page==3 && h.app.drag_hot,"文件进入感应区应立即展开置物架");
    for _ in 0..60{h.frame(vec![]);}assert!(h.app.open);
    h.hovered_files.clear();h.cursor=None;for _ in 0..60{h.frame(vec![]);}
    assert!(!h.app.drag_hot && !h.app.open);assert_eq!(h.app.page,1);
    assert!(h.app.shelf.items.is_empty(),"取消拖入不应保存文件");
}

#[test]
fn shelf_quick_drop_in_the_capture_zone_shows_the_added_file(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    let path=crate::model::config_dir().join("快速拖入.txt");std::fs::create_dir_all(path.parent().unwrap()).unwrap();std::fs::write(&path,"test").unwrap();
    h.cursor=Some(h.file_drop_zone.unwrap().right_bottom()-Vec2::splat(12.0));
    h.dropped_files.push(egui::DroppedFile{path:Some(path.clone()),..Default::default()});
    for _ in 0..90{h.frame(vec![]);}
    assert!(h.app.open && h.app.page==3);assert_eq!(h.app.shelf.items,vec![path.clone()]);assert!(path.exists());
    let mut h=H::new();for _ in 0..90{h.frame(vec![]);}
    h.cursor=Some(h.file_drop_zone.unwrap().right_bottom()-Vec2::splat(12.0));h.app.settings.visible_pages[3]=false;
    h.dropped_files.push(egui::DroppedFile{path:Some(path),..Default::default()});h.frame(vec![]);
    assert!(!h.app.open && !h.app.settings_open,"关闭置物架后应忽略拖入");
}

#[test]
fn settings_opening_restores_focus_once_without_becoming_topmost() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.settings_theme=crate::settings_theme::SettingsTheme::Dark;
    h.ctx.set_embed_viewports(false);
    let frame=|h:&mut H,focused:bool| {
        egui::Context::set_immediate_viewport_renderer(move |ctx,mut viewport| {
            let id=viewport.ids.this;
            let mut raw=egui::RawInput{viewport_id:id,focused,system_theme:Some(egui::Theme::Light),..Default::default()};
            raw.viewports.insert(id,egui::ViewportInfo{focused:Some(focused),minimized:Some(!focused),..Default::default()});
            let _=ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx));
        });
        let id=egui::ViewportId::from_hash_of("daozai-settings");
        let mut raw=egui::RawInput::default();
        raw.viewports.insert(id,egui::ViewportInfo{focused:Some(focused),..Default::default()});
        let mut output=h.ctx.run(raw,|ctx|h.app.settings_ui(ctx,false));
        let viewport=output.viewport_output.remove(&id).unwrap();
        assert!(viewport.commands.iter().any(|cmd|matches!(cmd,egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark))),"浅色系统下窗口标题栏和边框也应使用深色");
        (viewport.builder,viewport.commands)
    };
    h.app.open_mail_settings();
    let (builder,commands)=frame(&mut h,false);
    assert_eq!(builder.visible,Some(false),"设置窗口应隐藏创建，避免首帧绘制前露出白色背景");
    assert_eq!(builder.active,Some(false),"隐藏窗口不能提前激活");
    let show=commands.iter().position(|cmd|matches!(cmd,egui::ViewportCommand::Visible(true))).expect("首帧绘制后应显示设置窗口");
    let focus=commands.iter().position(|cmd|matches!(cmd,egui::ViewportCommand::Focus)).unwrap();
    let theme=commands.iter().position(|cmd|matches!(cmd,egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark))).unwrap();
    assert!(theme<show,"显示窗口前应设置深色边框，避免白边闪现");
    assert!(show<focus,"应先显示已绘制的窗口，再获取焦点");
    assert_eq!(builder.window_level,Some(egui::WindowLevel::Normal),"打开设置时应保持普通窗口层级");
    assert!(commands.iter().any(|cmd|matches!(cmd,egui::ViewportCommand::Minimized(false))),"应恢复已最小化的设置窗口：{commands:?}");
    assert!(commands.iter().any(|cmd|matches!(cmd,egui::ViewportCommand::Focus)),"应将焦点交给设置窗口");
    let (builder,commands)=frame(&mut h,true);
    assert!(!commands.iter().any(|cmd|matches!(cmd,egui::ViewportCommand::Visible(_))),"正常重绘不应重复显示窗口，避免打断最小化");
    assert_eq!(builder.window_level,Some(egui::WindowLevel::Normal),"操作设置时也不能切换到置顶层");
    assert!(!commands.iter().any(|cmd|matches!(cmd,egui::ViewportCommand::Focus)),"正常绘制不应反复抢焦点");
    let (builder,commands)=frame(&mut h,false);
    assert_eq!(builder.window_level,Some(egui::WindowLevel::Normal),"切回其他程序后应解除设置置顶");
    assert!(!commands.iter().any(|cmd|matches!(cmd,egui::ViewportCommand::Focus)));
    h.app.open_mail_settings();
    let (builder,commands)=frame(&mut h,false);
    assert_eq!(builder.window_level,Some(egui::WindowLevel::Normal),"再次唤起设置仍须保持普通窗口层级");
    assert!(commands.iter().any(|cmd|matches!(cmd,egui::ViewportCommand::Minimized(false))));
    assert!(commands.iter().any(|cmd|matches!(cmd,egui::ViewportCommand::Focus)),"设置已打开时仍应响应再次唤起");
    h.app.settings_open=false;
    let _=h.ctx.run(egui::RawInput::default(),|ctx|h.app.settings_ui(ctx,false));
    h.app.open_settings();
    let (builder,commands)=frame(&mut h,false);
    assert_eq!(builder.visible,Some(false),"关闭后重新创建也应隐藏到首帧完成");
    assert!(commands.iter().any(|cmd|matches!(cmd,egui::ViewportCommand::Visible(true))),"重开设置应正常显示");
}

#[test]
fn web_video_is_hidden_without_changing_music_preferences(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.opts.fetch_lyrics=true;h.app.show_lyrics=true;
    h.app.ly.key="Previous Song|Artist|120".into();
    h.app.ly.lyrics=Some(crate::lyrics::parse_lrc("[00:01.00]previous song"));
    h.app.ly.error=Some(crate::lyrics::FetchError::Unavailable("503".into()));
    let (tx,rx)=std::sync::mpsc::channel();h.app.ly.rx=Some(rx);
    h.media=crate::smtc::NowPlaying{title:"小唐的Token的抖音 - 抖音".into(),playing:true,duration:17.0,..Default::default()};
    h.frame(vec![]);
    assert!(h.app.media.title.is_empty() && !h.app.media.playing,"视频标题和播放状态不应进入岛内音乐页面");
    assert!(h.app.ly.key.is_empty(),"视频不应建立歌曲取词任务");
    assert!(h.app.ly.lyrics.is_none() && h.app.ly.error.is_none(),"切到视频应清除上一首歌词和歌词错误");
    assert!(tx.send(Ok((crate::lyrics::parse_lrc("[00:01.00]late song"),false))).is_err(),"视频不能接收旧歌曲迟到的查询结果");
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    h.click(H::local(74.0,118.0));
    assert!(h.app.show_lyrics,"视频页面的标签不应切换用户的歌曲歌词偏好");
    h.click(H::local(210.5,118.0));
    assert!(!h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Media(_))),"音乐页不应控制被隐藏的视频");
    assert!(!matches!(h.app.pet_state(),crate::extras::PetState::Music),"视频不应触发听歌表情");
    h.app.opts.fetch_lyrics=false;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),duration:120.0,..Default::default()};
    h.hover(Pos2::new(WIN_W/2.0,12.0),0.5);
    h.click(H::local(74.0,118.0));
    assert!(!h.app.show_lyrics,"回到歌曲时应恢复原来的词按钮");
}

#[test]
fn web_video_does_not_override_the_minimal_island(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.minimal=true;
    h.media=crate::smtc::NowPlaying{title:"小唐的Token的抖音 - 抖音".into(),playing:true,duration:17.0,..Default::default()};
    for _ in 0..90 {h.frame(vec![]);}
    assert!(h.region.height()<20.0,"视频不能触发歌曲专用的收起态");
}

#[test]
fn minimal_mode_stays_a_line_with_music_and_after_media_changes(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.minimal=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:120.0,..Default::default()};
    for _ in 0..90 {h.frame(vec![]);}
    assert!(h.region.height()<20.0,"播放音乐时仍应遵守极简开关");
    assert!(h.hit.height()<=crate::app::BAR_H_MIN+0.1,"鼠标命中范围也应收回细线");
    h.media.title="Another Track".into();h.media.playing=false;
    for _ in 0..90 {h.frame(vec![]);}
    assert!(h.region.height()<20.0,"暂停或切歌不能覆盖极简开关");
    h.media=Default::default();
    for _ in 0..90 {h.frame(vec![]);}
    assert!(h.region.height()<20.0,"没有媒体时恢复用户选择的极简状态");
}

#[test]
fn toggling_minimal_changes_the_island_while_media_is_present(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:120.0,..Default::default()};
    for _ in 0..90 {h.frame(vec![]);}assert!(h.region.height()>30.0);
    h.app.settings.minimal=true;for _ in 0..90 {h.frame(vec![]);}
    assert!(h.region.height()<20.0,"开启极简后应收成细线");
    h.app.settings.minimal=false;for _ in 0..90 {h.frame(vec![]);}
    assert!(h.region.height()>30.0,"关闭极简后应恢复普通岛体");
}

#[test]
fn minimal_mode_keeps_focus_running_and_returns_after_notifications(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.minimal=true;h.app.pomo.running=true;h.app.pomo.remaining=120.0;
    for _ in 0..90 {h.frame(vec![]);}assert!(h.region.height()<20.0,"专注不能覆盖极简开关");
    h.app.alert("系统音量","音量 50%",5,false);
    for _ in 0..60 {h.frame(vec![]);}assert!(h.region.height()>30.0,"临时提醒仍应可见");
    for _ in 0..240 {h.frame(vec![]);}assert!(h.region.height()<20.0,"提醒结束后应回到细线");
    assert!(h.app.pomo.running&&h.app.pomo.remaining<115.0,"极简仅改变显示，计时继续运行");
}

#[test]
fn delayed_media_samples_keep_the_live_position_and_follow_pause_and_seek(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:9.2,duration:120.0,playing:true,sampled_at:Some(std::time::Instant::now()-std::time::Duration::from_millis(600)),..Default::default()};
    h.frame(vec![]);
    assert!(h.app.media.position>=9.79,"媒体位置必须包含送到界面之前经过的时间");
    h.media.playing=false;h.media.position=9.4;h.frame(vec![]);
    assert_eq!(h.app.media.position,9.4,"暂停不能继续外推");
    h.media.playing=true;h.media.position=4.0;h.media.sampled_at=Some(std::time::Instant::now());h.frame(vec![]);
    assert!((4.0..4.2).contains(&h.app.media.position),"跳转后应使用新的播放位置");
    h.media.position=119.9;h.media.sampled_at=Some(std::time::Instant::now()-std::time::Duration::from_secs(1));h.frame(vec![]);
    assert_eq!(h.app.media.position,120.0,"外推不能超过歌曲结尾");
}

#[test]
fn kugou_external_seek_updates_lyrics_in_both_directions(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{source:"KuGou.exe".into(),title:"Track".into(),artist:"Artist".into(),duration:120.0,playing:true,..Default::default()};
    h.frame(vec![]);
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]开头\n[00:30]中段\n[01:00]后段"));
    for (position,line) in [(65.0,"后段"),(10.0,"开头"),(35.0,"中段")] {
        h.media.position=position;h.media.sampled_at=Some(std::time::Instant::now());h.frame(vec![]);
        assert_eq!(h.app.island_lyric().map(|(_,text)|text),Some(line),"酷狗跳转后应立即切换到对应歌词");
    }
}

#[test]
fn music_clients_keep_lyrics_across_two_repeat_cycles(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for source in ["kugou","QQMusic.exe","cloudmusic.exe","汽水音乐","Spotify.exe","kwmusic.exe"] {
        let mut h=H::new();h.app.settings.island_lyrics=true;
        h.media=crate::smtc::NowPlaying{source:source.into(),title:"循环测试".into(),artist:"歌手".into(),duration:120.0,playing:true,..Default::default()};
        h.frame(vec![]);h.app.set_lyrics("循环测试","歌手",120.0,crate::lyrics::parse_lrc("[00:00]开头\n[00:05]第一段\n[01:50]尾声"));
        for (position,line) in [(119.0,"尾声"),(0.0,"开头"),(8.0,"第一段"),(119.0,"尾声"),(0.0,"开头"),(8.0,"第一段")] {
            h.media.position=position;h.media.sampled_at=Some(std::time::Instant::now());h.frame(vec![]);
            assert_eq!(h.app.island_lyric().map(|(_,text)|text),Some(line),"{source} 第二、三遍也必须显示相应歌词");
        }
    }
}

#[test]
fn music_without_a_timeline_does_not_invent_elapsed_time(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"QQMusic.exe".into(),title:"歌曲".into(),playing:true,playback_type:crate::smtc::MediaKind::Music,..Default::default()};
    for _ in 0..240{h.frame(vec![]);}
    assert_eq!(h.app.media.title,"歌曲");assert_eq!(h.app.media.position,0.0);assert_eq!(h.app.media.duration,0.0);
}

#[test]
fn kugou_position_advances_without_duration_and_respects_pause_and_seek(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"kugou".into(),title:"Track".into(),artist:"Artist".into(),position:42.0,duration:0.0,playing:true,sampled_at:Some(std::time::Instant::now()-std::time::Duration::from_millis(600)),..Default::default()};
    h.frame(vec![]);
    assert!(h.app.media.position>=42.59,"未知总时长不能阻止真实进度外推");
    h.media.playing=false;h.frame(vec![]);assert_eq!(h.app.media.position,42.0);
    h.media.playing=true;h.media.position=12.0;h.media.sampled_at=Some(std::time::Instant::now());h.frame(vec![]);
    assert!((12.0..12.2).contains(&h.app.media.position),"拖动后必须跟随新的实际位置");
}

#[test]
fn music_volume_popup_opens_and_mute_restores_the_selected_players_level(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"QQMusic.exe".into(),title:"Track".into(),artist:"Artist".into(),duration:120.0,volume:Some((60,false)),..Default::default()};
    h.app.volume_ui=Some((45,false));
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    h.click(H::local(367.0,118.0));
    assert!(h.app.music_volume_open);
    assert!(h.cmds.is_empty(),"打开浮条不应改变音量或静音");
    h.hover(H::local(367.0,92.0),0.2);h.click(H::local(367.0,92.0));
    assert!(h.cmds.contains(&crate::app::Cmd::MusicVolume(crate::smtc::Transport::SetMute(true),"QQMusic.exe".into())));
    assert!(!h.cmds.contains(&crate::app::Cmd::ToggleMute));
    assert_eq!(h.app.music_volume_ui,Some((60,true)));
    assert_eq!(h.app.volume_ui,Some((45,false)));
    h.click(H::local(367.0,92.0));
    assert_eq!(h.app.music_volume_ui,Some((60,false)),"恢复时沿用静音前的音量");
    assert!(h.cmds.contains(&crate::app::Cmd::MusicVolume(crate::smtc::Transport::SetMute(false),"QQMusic.exe".into())));
}

#[test]
fn music_volume_popup_slider_unmutes_and_scrolls_without_changing_pages(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"QQMusic.exe".into(),title:"Track".into(),artist:"Artist".into(),volume:Some((60,true)),..Default::default()};
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.click(H::local(367.0,118.0));h.hover(H::local(367.0,53.5),0.2);
    h.click(H::local(367.0,53.5));
    assert_eq!(h.app.music_volume_ui,Some((50,false)));
    assert!(h.cmds.contains(&crate::app::Cmd::MusicVolume(crate::smtc::Transport::SetVolume(50),"QQMusic.exe".into())));
    h.wheel(120.0);assert_eq!(h.app.music_volume_ui,Some((54,false)));assert_eq!(h.app.page,0);
    h.click(H::local(367.0,32.0));assert_eq!(h.app.music_volume_ui,Some((100,false)));
    h.click(H::local(367.0,75.0));assert_eq!(h.app.music_volume_ui,Some((0,false)));
    h.click(H::local(387.0,68.0));assert_eq!(h.app.page,0,"浮条遮挡部分不能穿透触发翻页");
    h.click(H::local(387.0,18.0));assert!(!h.app.settings_open,"浮条遮挡部分不能穿透打开设置");
}

#[test]
fn music_volume_popup_closes_on_escape_outside_click_source_change_and_collapse(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"QQMusic.exe".into(),title:"Track".into(),artist:"Artist".into(),volume:Some((60,false)),..Default::default()};
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.click(H::local(367.0,118.0));
    h.frame(vec![egui::Event::Key{key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
    assert!(!h.app.music_volume_open);assert!(h.app.open,"Esc 只关闭浮条，保留音乐面板");
    h.click(H::local(367.0,118.0));h.click(H::local(180.0,45.0));assert!(!h.app.music_volume_open);
    h.click(H::local(367.0,118.0));h.media.source="cloudmusic.exe".into();h.frame(vec![]);assert!(!h.app.music_volume_open);
    h.click(H::local(367.0,118.0));h.app.show_page(1);assert!(!h.app.music_volume_open);
    h.app.show_page(0);h.hover(H::local(180.0,45.0),1.0);h.click(H::local(367.0,118.0));
    h.hover(Pos2::new(0.0,250.0),5.0);assert!(!h.app.music_volume_open);
}

#[test]
#[ignore="按需导出音乐音量浮条实际画面：DAOZAI_MUSIC_VOLUME_PREVIEW=目录"]
fn music_volume_popup_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_MUSIC_VOLUME_PREVIEW").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for (name,muted,lyrics) in [("volume",false,false),("muted",true,false),("lyrics",false,true)] {
        let mut h=H::new();h.ctx.set_pixels_per_point(2.0);h.app.settings.pet=false;h.app.show_lyrics=lyrics;
        h.media=crate::smtc::NowPlaying{source:"cloudmusic.exe".into(),title:"晴天".into(),artist:"周杰伦".into(),position:92.0,duration:269.0,volume:Some((62,muted)),..Default::default()};
        h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
        if lyrics{h.app.set_lyrics("晴天","周杰伦",269.0,crate::lyrics::parse_lrc("[00:00]午后的阳光落在窗边\n[01:30]风把回忆吹向远方\n[01:40]慢慢走过这段时光"));}
        h.click(H::local(367.0,118.0));h.hover(H::local(330.0,112.0),0.25);
        let ppp=h.ctx.pixels_per_point();let mut raster=crate::preview_raster::Raster::new((crate::app::CANVAS_W*ppp) as usize,(WIN_H*ppp) as usize);
        raster.update_textures(&egui::TexturesDelta{set:vec![(egui::TextureId::default(),egui::epaint::ImageDelta::full(h.ctx.fonts(|f|f.image()),egui::TextureOptions::LINEAR))],..Default::default()});
        raster.buf.fill([0.10,0.12,0.16,1.0]);raster.draw(&h.ctx.tessellate(h.shapes.clone(),ppp),ppp);
        let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
        let image=image::RgbaImage::from_raw(raster.w as u32,raster.h as u32,rgba).unwrap();
        image::imageops::crop_imm(&image,((WIN_W-crate::app::PW)/2.0*ppp-12.0) as u32,0,((crate::app::PW+12.0)*ppp+12.0) as u32,((crate::app::PH+12.0)*ppp) as u32).to_image().save(dest.join(format!("{name}.png"))).unwrap();
    }
}

#[test]
fn music_volume_wheel_and_player_switch_keep_system_volume_untouched(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"QQMusic.exe".into(),title:"Track".into(),artist:"Artist".into(),volume:Some((60,true)),..Default::default()};
    h.app.volume_ui=Some((45,false));h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.hover(H::local(367.0,118.0),0.2);h.wheel(120.0);
    assert!(h.cmds.contains(&crate::app::Cmd::MusicVolume(crate::smtc::Transport::SetVolume(64),"QQMusic.exe".into())),"音量={:?} 命令={:?} 页={} 指针={:?}",h.app.music_volume_ui,h.cmds,h.app.page,h.cursor);
    assert_eq!(h.app.music_volume_ui,Some((64,false)));
    h.media.source="cloudmusic.exe".into();h.media.volume=Some((22,false));h.frame(vec![]);
    assert_eq!(h.app.music_volume_ui,Some((22,false)),"换播放器立即更新独立音量");
    assert_eq!(h.app.volume_ui,Some((45,false)));
    h.media.volume=None;h.cmds.clear();h.click(H::local(367.0,118.0));h.wheel(120.0);
    assert!(h.cmds.is_empty(),"没有应用会话时不可回退到系统音量");
}

#[test]
fn music_uses_system_artwork_and_clears_it_when_the_next_song_has_none(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    let image=std::sync::Arc::new(egui::ColorImage::new([32,32],egui::Color32::BLUE));
    h.media=crate::smtc::NowPlaying{title:"First".into(),artist:"Artist".into(),artwork:Some(image.clone()),..Default::default()};
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    let (cached,texture)=h.app.music_artwork.as_ref().expect("播放页应使用系统封面");
    assert!(std::sync::Arc::ptr_eq(cached,&image));
    let texture_id=texture.id();
    h.frame(vec![]);
    assert_eq!(h.app.music_artwork.as_ref().unwrap().1.id(),texture_id,"进度更新不应重复上传纹理");
    h.media=crate::smtc::NowPlaying{title:"Second".into(),artist:"Artist".into(),..Default::default()};
    h.frame(vec![]);
    assert!(h.app.music_artwork.is_none(),"缺少封面的下一首歌应恢复占位图");
}

#[test]
fn failed_lyrics_can_be_retried_from_the_music_page(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),duration:120.0,..Default::default()};
    h.app.ly.key="Track|Artist|120".into();
    h.app.ly.error=Some(crate::lyrics::FetchError::Unavailable("HTTP 502".into()));
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="暂未获取歌词，请重试")),"展开内应保留歌词错误提示");
    h.click(H::local(310.0,68.0));
    assert!(h.app.ly.key.is_empty(),"点击错误提示后应允许重新请求当前歌曲");
}

#[test]
fn transient_lyric_failures_retry_but_missing_songs_do_not_loop(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for (error,retry) in [(crate::lyrics::FetchError::Unavailable("503".into()),true),(crate::lyrics::FetchError::NotFound,false),(crate::lyrics::FetchError::IncompleteMetadata,false)] {
        let mut h=H::new();
        h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),duration:120.0,..Default::default()};
        h.app.opts.fetch_lyrics=true;h.app.show_lyrics=true;
        h.app.ly.key="Track|Artist|120".into();
        let (tx,rx)=std::sync::mpsc::channel();
        h.app.ly.rx=Some(rx);h.app.ly.loading=true;
        tx.send(Err(error.clone())).unwrap();
        h.frame(vec![]);
        assert_eq!(h.app.ly.error,Some(error));
        assert_eq!(h.app.ly.retry_at.is_some(),retry);
        assert!(!h.app.ly.loading);
        if let Some(at)=h.app.ly.retry_at {assert!((at-h.t-30.0).abs()<0.001,"网络恢复重试应有间隔");}
    }
}

#[test]
fn music_lyrics_show_in_a_separate_right_column() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:7.0,duration:120.0,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:01]上一句\n[00:05]当前歌词\n[00:10]下一句"));
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    let current=h.shapes.iter().find_map(|shape|match &shape.shape {
        egui::Shape::Text(text) if text.galley.text()=="当前歌词"=>Some(text.pos),_=>None,
    }).expect("展开页应显示同步歌词");
    assert!(current.x>=H::local(243.0,0.0).x,"歌词必须位于播放器右侧");
    let title=h.shapes.iter().find_map(|shape|match &shape.shape {
        egui::Shape::Text(text) if text.galley.text()=="Track"=>Some(text.pos),_=>None,
    }).unwrap();
    assert!(title.x<H::local(233.0,0.0).x);
    h.click(H::local(74.0,118.0));
    assert!(!h.app.show_lyrics);
    h.frame(vec![]);
    assert!(!h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text()=="当前歌词")));
}

#[test]
fn disabling_lyrics_disconnects_pending_workers(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"KuGou.exe".into(),title:"测试歌曲".into(),artist:"歌手".into(),..Default::default()};
    h.frame(vec![]);h.app.opts.fetch_lyrics=true;
    h.app.show_lyrics=false;h.app.settings.island_lyrics=false;
    h.app.ly.key="测试歌曲|歌手|0".into();h.app.ly.loading=true;
    let (network,rx)=std::sync::mpsc::channel();h.app.ly.rx=Some(rx);
    let (native,rx)=std::sync::mpsc::channel();h.app.ly.native_rx=Some(rx);
    h.frame(vec![]);
    assert!(native.send(None).is_err(),"关闭歌词后应停止本机轮询");
    assert!(network.send(Err(crate::lyrics::FetchError::NotFound)).is_err());
    assert!(h.app.ly.key.is_empty(),"再次开启时应能重新加载歌词");
}

#[test]
fn late_native_lyrics_bypass_pending_network_for_both_clients(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for source in ["KuGou.exe","SodaMusic.exe"] {
        let mut h=H::new();
        h.media=crate::smtc::NowPlaying{source:source.into(),title:"测试歌曲".into(),artist:"歌手".into(),..Default::default()};
        h.frame(vec![]);h.app.opts.fetch_lyrics=true;h.app.show_lyrics=true;
        h.app.ly.key="测试歌曲|歌手|0".into();h.app.ly.loading=true;
        let (network,rx)=std::sync::mpsc::channel();h.app.ly.rx=Some(rx);
        let (native,rx)=std::sync::mpsc::channel();h.app.ly.native_rx=Some(rx);
        native.send(Some(crate::lyrics::parse_lrc("[00:01.00]本机歌词"))).unwrap();h.frame(vec![]);
        assert!(!h.app.ly.loading,"本机歌词不能等待在线请求结束");
        assert_eq!(h.app.ly.lyrics.as_ref().unwrap().lines[0].text,"本机歌词");
        assert!(network.send(Err(crate::lyrics::FetchError::NotFound)).is_err(),"迟到的网络结果必须丢弃");
    }
}

#[test]
fn kugou_fallback_waits_for_late_native_lyrics(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"kugou".into(),title:"测试歌曲".into(),artist:"歌手".into(),position:22.8,..Default::default()};
    h.frame(vec![]);h.app.opts.fetch_lyrics=true;h.app.show_lyrics=true;h.app.ly.key="测试歌曲|歌手|0".into();
    let (tx,rx)=std::sync::mpsc::channel();h.app.ly.rx=Some(rx);
    tx.send(Ok((crate::lyrics::parse_lrc("[00:23.930]测试歌词"),true))).unwrap();h.frame(vec![]);
    assert!(h.app.ly.retry_at.is_some(),"临时通用歌词必须继续等待迟到的酷狗原生歌词");
    h.t=h.app.ly.retry_at.unwrap()+0.1;h.frame(vec![]);
    assert_eq!(h.app.ly.lyrics.as_ref().unwrap().lines[0].t,23.930,"接回原生歌词期间不能清空现有歌词");assert!(!h.app.ly.loading);
    let (tx,rx)=std::sync::mpsc::channel();h.app.ly.rx=Some(rx);
    tx.send(Ok((crate::lyrics::parse_lrc("[00:22.991]测试歌词"),false))).unwrap();h.frame(vec![]);
    assert!((h.app.ly.lyrics.as_ref().unwrap().lines[0].t-22.991).abs()<0.001);assert!(h.app.ly.retry_at.is_none(),"接回原生歌词后应停止重查");
}

#[test]
fn kugou_missing_lyrics_rechecks_late_cache_without_flashing_loading() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"kugou".into(),title:"不存在的测试歌曲".into(),artist:"测试歌手".into(),duration:120.0,..Default::default()};
    h.frame(vec![]);h.app.opts.fetch_lyrics=true;h.app.show_lyrics=true;
    h.app.ly.key="不存在的测试歌曲|测试歌手|120".into();
    let (tx,rx)=std::sync::mpsc::channel();h.app.ly.rx=Some(rx);
    tx.send(Err(crate::lyrics::FetchError::NotFound)).unwrap();h.frame(vec![]);
    let retry=h.app.ly.retry_at.expect("酷狗尚未写入缓存时仍需自动检查");
    assert!((retry-h.t-5.0).abs()<0.001);
    // 歌词通道全进程只有 1 个名额，前面测试的歌词线程可能还在查网络；占着时这一帧会直接跳过。
    let until=std::time::Instant::now()+std::time::Duration::from_secs(20);
    while crate::resources::LYRICS.active()>0 && std::time::Instant::now()<until{std::thread::sleep(std::time::Duration::from_millis(10));}
    assert_eq!(crate::resources::LYRICS.active(),0,"歌词名额被其他测试长期占用");
    h.t=retry+0.1;h.frame(vec![]);
    assert!(!h.app.ly.loading,"检查本机缓存不能反复闪出加载提示");
    assert_eq!(h.app.ly.error,Some(crate::lyrics::FetchError::NotFound));
    let (tx,rx)=std::sync::mpsc::channel();h.app.ly.rx=Some(rx);
    tx.send(Ok((crate::lyrics::parse_lrc("[00:01.00]迟到的原生歌词"),false))).unwrap();h.frame(vec![]);
    assert!(h.app.ly.lyrics.is_some());assert!(h.app.ly.error.is_none());assert!(h.app.ly.retry_at.is_none());
}

#[test]
fn island_lyrics_pause_returns_to_resident_and_resume_follows_current_position(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|error|error.into_inner());
    let visible=|h:&H,label:&str|h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text()==label));
    for minimal in [false,true] {for synced in [true,false] {
        let mut h=H::new();h.app.settings.minimal=minimal;
        h.power=(Some(75),false);h.app.pomo.running=true;h.app.pomo.remaining=600.0;
        h.app.weather.snapshot=Some(crate::weather::Snapshot{temp:21.0,..Default::default()});
        h.media=crate::smtc::NowPlaying{source:"cloudmusic.exe".into(),title:"Track".into(),artist:"Artist".into(),position:7.0,duration:120.0,..Default::default()};
        for _ in 0..90 {h.frame(vec![]);}
        let resident_width=h.app.current_width();let resident_height=h.hit.height();
        h.app.settings.island_lyrics=true;h.media.playing=true;
        let lyrics=if synced {crate::lyrics::parse_lrc("[00:05]当前歌词\n[00:10]下一句歌词")}else{crate::lyrics::from_plain("没有时间戳的歌词")};
        h.app.set_lyrics("Track","Artist",120.0,lyrics);
        for _ in 0..90 {h.frame(vec![]);}
        assert!(visible(&h,if synced{"当前歌词"}else{"Track"}));
        assert_eq!(h.app.media_poll_interval(),crate::smtc::POLL_LYRICS);
        h.media.playing=false;h.frame(vec![]);
        assert!(h.app.island_lyric().is_none(),"暂停后应退出收起态歌词显示");
        for _ in 0..90 {h.frame(vec![]);}
        assert!(!visible(&h,"当前歌词") && !visible(&h,"Track"));
        assert!((h.app.current_width()-resident_width).abs()<2.0,"暂停后应恢复原常驻宽度");
        assert!((h.hit.height()-resident_height).abs()<0.1);
        assert_eq!(visible(&h,"09:41"),!minimal);
        if !minimal {
            assert!(visible(&h,"21°"),"普通常驻仍应显示天气");
            let secs=h.app.pomo.remaining.ceil() as u32;
            assert!(visible(&h,&format!("{:02}:{:02}",secs/60,secs%60)),"暂停音乐不能隐藏专注倒计时");
        }
        assert!(h.app.settings.island_lyrics && h.app.ly.lyrics.is_some(),"暂停只切换显示形态，应保留开关和歌词缓存");
        assert_eq!(h.app.media_poll_interval(),crate::smtc::POLL_BACKGROUND);
        h.media.position=12.0;for _ in 0..30 {h.frame(vec![]);}
        assert!(!visible(&h,"下一句歌词"));
        h.media.playing=true;for _ in 0..90 {h.frame(vec![]);}
        assert!(visible(&h,if synced{"下一句歌词"}else{"Track"}),"继续播放后应按最新进度恢复歌词或歌名");
        assert!(!visible(&h,"09:41"));
        assert_eq!(h.app.media_poll_interval(),crate::smtc::POLL_LYRICS);
    }}
}

#[test]
fn music_lyrics_island_toggle_is_independent_and_persistent() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.show_lyrics=false;h.app.settings.lyrics_enabled=false;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:7.0,duration:120.0,playing:true,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:05]当前歌词\n[00:10]下一句"));
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    h.click(H::local(313.0,118.0));
    assert!(h.app.settings.island_lyrics);
    assert!(!h.app.show_lyrics && !h.app.settings.lyrics_enabled);
    let saved=crate::model::Settings::load();
    assert!(saved.island_lyrics && !saved.lyrics_enabled);
    h.hover(Pos2::new(WIN_W/2.0,PH+30.0),1.0);
    assert!(!h.app.open);
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("当前歌词"));
    let pet_width=if h.app.pet_enabled(){48.0}else{0.0};
    assert!((h.app.current_width()-crate::app::LYRIC_BAR_W-pet_width).abs()<2.0);
}

#[test]
fn music_lyric_timing_follows_saved_delay_and_preserves_playback_position(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:9.8,duration:120.0,playing:true,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]第一句\n[00:05]当前歌词\n[00:10]下一句"));
    h.frame(vec![]);
    assert_eq!(h.app.settings.lyric_delay_ms,-350,"未设置时应保留原有提前 0.35 秒的预设");
    assert!((h.app.lyric_position()-10.15).abs()<0.001,"默认应提前 0.35 秒显示，且只应用一次");
    for (delay,position,expected) in [(500,10.2,Some("当前歌词")),(-500,9.8,Some("下一句")),(0,9.8,Some("当前歌词")),(500,0.2,None),(500,0.6,Some("第一句"))]{
        std::fs::write(crate::model::config_dir().join("settings.txt"),format!("lyric_delay_ms={delay}\nisland_lyrics=1\n")).unwrap();
        h.app.settings=crate::model::Settings::load();
        h.media.position=position;h.frame(vec![]);
        assert_eq!(h.app.island_lyric().map(|(_,line)|line),expected,"延时 {delay} ms，播放位置 {position}");
        assert!((h.app.media.position-position).abs()<0.001,"调整歌词不应改变播放进度");
        h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
        for text in ["第一句","当前歌词","下一句"]{
            let active=h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(t) if t.galley.text()==text && t.galley.job.sections[0].format.font_id.size==12.0));
            assert_eq!(active,expected==Some(text),"展开歌词也应使用相同的显示时间");
        }
        h.app.settings.save();
        assert_eq!(crate::model::Settings::load().lyric_delay_ms,delay,"重启后应保留时间调整");
    }
    for (input,expected) in [("invalid",-350),("999999",5000),("-999999",-5000),("",-350)]{
        std::fs::write(crate::model::config_dir().join("settings.txt"),format!("lyric_delay_ms={input}\n")).unwrap();
        assert_eq!(crate::model::Settings::load().lyric_delay_ms,expected);
    }
}

#[test]
fn music_lyric_timing_controls_match_focus_save_and_fit_the_small_settings_window(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings_tab=1;h.app.open_settings();h.ctx.set_embed_viewports(false);
    fn frame(h:&mut H,events:Vec<egui::Event>)->Vec<egui::epaint::ClippedShape>{
        let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;
            let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(600.0,450.0))),
                viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.25),focused:Some(true),..Default::default()})).collect(),
                focused:true,events:events.clone(),..Default::default()};
            *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        let raw=egui::RawInput{viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(1.25),..Default::default()})).collect(),..Default::default()};
        let _=h.ctx.run(raw,|ctx|h.app.settings_ui(ctx,false));
        let child=output.lock().unwrap().take().unwrap();child.shapes
    }
    let text_rect=|shapes:&[egui::epaint::ClippedShape],text:&str|shapes.iter().find_map(|shape|match &shape.shape{
        egui::Shape::Text(t)if t.galley.text()==text=>Some(t.visual_bounding_rect()),_=>None,
    }).unwrap_or_else(||panic!("设置中缺少 {text}"));
    for _ in 0..3{frame(&mut h,vec![]);}
    let music=frame(&mut h,vec![]);
    text_rect(&music,"0.35 秒");
    assert_eq!(h.app.settings.lyric_delay_ms,-350,"仅打开设置不能将 0.35 秒舍入为 0.4 秒");
    for button in ["−","+"]{text_rect(&music,button);}
    h.app.settings_tab=2;
    for _ in 0..3{frame(&mut h,vec![]);}
    let focus=frame(&mut h,vec![]);
    for button in ["−","+"]{
        let a=text_rect(&music,button);let b=text_rect(&focus,button);
        assert!((a.left()-b.left()).abs()<0.1 && (a.width()-b.width()).abs()<0.1 && (a.height()-b.height()).abs()<0.1,"歌词和专注应共用同样的数值控件及右侧对齐");
    }
    h.app.settings_tab=1;
    for _ in 0..3{frame(&mut h,vec![]);}
    for (button,expected) in [("+",-450),("−",-350),("−",-250),("+",-350)]{
        let shapes=frame(&mut h,vec![]);
        let rect=text_rect(&shapes,button);
        assert!(Rect::from_min_max(egui::pos2(180.0,0.0),egui::pos2(600.0,450.0)).contains_rect(rect),"时间调整按钮应完整显示");
        let pos=rect.center();
        frame(&mut h,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
        frame(&mut h,vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
        assert_eq!(h.app.settings.lyric_delay_ms,expected);
        assert_eq!(crate::model::Settings::load().lyric_delay_ms,expected,"按钮调整后应自动保存");
        text_rect(&frame(&mut h,vec![]),&format!("{:.2} 秒",-expected as f64/1000.0));
    }
}

#[test]
fn island_lyrics_arrive_just_before_vocals_and_finish_fading_before_the_timestamp(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:9.4,duration:120.0,playing:true,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:05]当前歌词\n[00:10]下一句"));
    for _ in 0..60 {h.frame(vec![]);}
    for step in 1..=30 {
        h.media.position=9.4+step as f64/60.0;
        h.frame(vec![]);
        if step==6 {assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("当前歌词"),"不能提前半秒就跳到下一句");}
    }
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("下一句"),"下一句应在开唱前提前出现");
    let text=h.shapes.iter().find_map(|shape|match &shape.shape {
        egui::Shape::Text(text) if text.galley.text()=="下一句"=>Some(text),_=>None,
    }).expect("开唱前应能看到下一句");
    assert!(text.opacity_factor>0.95,"开唱前应完成淡入，不能等到开唱后才清晰");
    h.media.playing=false;h.frame(vec![]);
    assert!(h.app.island_lyric().is_none(),"暂停时应退出收起态歌词显示");
    h.media.playing=true;h.frame(vec![]);
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("下一句"),"继续播放时不应跳回上一句");
    h.media.position=4.5;h.frame(vec![]);
    assert!(h.app.island_lyric().is_none(),"前奏不能过早显示第一句");
    h.media.position=4.75;h.frame(vec![]);
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("当前歌词"),"第一句也应提前出现");
}

#[test]
fn expanded_and_segmented_lyrics_share_the_early_display_time(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:9.8,duration:120.0,playing:true,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:05]当前歌词\n[00:10]下一句"));
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    let next=h.shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Text(text)if text.galley.text()=="下一句"=>Some(text),_=>None}).unwrap();
    assert_eq!(next.galley.job.sections[0].format.font_id.size,12.0,"展开歌词也应在开唱前高亮下一句");
    h.hover(Pos2::new(WIN_W/2.0,PH+30.0),1.0);
    let line="愿每一个认真生活的人，都能在平凡的日子里找到属于自己的光";
    h.media.position=6.0;h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc(&format!("[00:05]{line}\n[00:15]下一句")));
    for _ in 0..60{h.frame(vec![]);}
    let shown=|h:&H|h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if line.contains(t.galley.text())&&!t.galley.text().is_empty()=>Some(t.galley.text().to_owned()),_=>None}).unwrap();
    let first=shown(&h);
    let boundary=5.0+10.0*first.chars().count()as f64/line.chars().count()as f64;
    h.media.position=boundary-0.2;for _ in 0..30{h.frame(vec![]);}
    assert_ne!(shown(&h),first,"长句的后半段也应使用相同的提前量");
}

#[test]
fn lyric_short_gap_and_line_switch_keep_text_visible(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:2.0,playing:true,..Default::default()};
    h.app.set_lyrics("Track","Artist",0.0,crate::lyrics::parse_lrc("[00:01]上一句\n[00:05]下一句"));
    for _ in 0..60{h.frame(vec![]);}
    for (position,expected) in [(3.0,"上一句"),(4.64,"上一句"),(4.7,"下一句"),(4.72,"下一句"),(4.9,"下一句")]{
        h.media.position=position;h.frame(vec![]);
        let text=h.shapes.iter().find_map(|shape|match &shape.shape{
            egui::Shape::Text(text)if text.galley.text()==expected=>Some(text),_=>None,
        }).expect("短停顿及换句时必须持续显示歌词");
        assert!(text.opacity_factor>0.95,"换句不能淡出歌词，位置 {position}，透明度 {}",text.opacity_factor);
    }
}

#[test]
fn island_lyrics_center_segments_in_a_fixed_height(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:7.0,duration:120.0,playing:true,..Default::default()};
    for line in ["晚风","愿每一个认真生活的人，都能在平凡的日子里找到属于自己的光","晚风"] {
        h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc(&format!("[00:05]{line}\n[00:15]下一句")));
        for _ in 0..90 {
            h.frame(vec![]);
            assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="09:41")),"切换显示段时应保持歌词布局");
        }
        let text=h.shapes.iter().find_map(|shape|match &shape.shape {
            egui::Shape::Text(text) if !text.galley.text().is_empty() && line.contains(text.galley.text())=>Some(text),_=>None,
        }).expect("岛上应显示当前歌词的一段");
        let center=text.pos.x+text.galley.rect.center().x;
        let offset=if h.app.pet_enabled(){24.0}else{0.0};
        assert!((center-WIN_W/2.0-offset).abs()<1.5,"歌词应在内容区居中");
        assert!(!text.galley.elided,"显示段应完整显示");
        assert_eq!(text.galley.rows.len(),1,"每次只显示一段歌词");
        assert!((h.hit.height()-(crate::app::BAR_H+4.0)).abs()<0.1,"岛应保持固定高度");
        assert!(text.visual_bounding_rect().bottom()<=h.hit.bottom(),"文字应完整显示在岛内");
    }
}

#[test]
fn island_lyric_segments_follow_playback_pause_and_seek(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.island_lyrics=true;
    let line="愿每一个认真生活的人，都能在平凡的日子里找到属于自己的光";
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:6.0,duration:120.0,playing:true,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc(&format!("[00:05]{line}\n[00:15]下一句")));
    let shown=|h:&H|h.shapes.iter().find_map(|s|match &s.shape {egui::Shape::Text(t)=>Some(t.galley.text().to_owned()),_=>None}).unwrap();
    for _ in 0..90 {h.frame(vec![]);}let first=shown(&h);
    h.media.position=14.0;for _ in 0..30 {h.frame(vec![]);}let last=shown(&h);
    assert_ne!(first,last,"播放到后半句时应切换显示段");
    assert_eq!(format!("{first}{last}"),line,"前后显示段应覆盖完整歌词");
    h.media.playing=false;for _ in 0..120 {h.frame(vec![]);}
    assert!(h.app.island_lyric().is_none(),"暂停时应恢复常驻形态");
    h.media.playing=true;for _ in 0..30 {h.frame(vec![]);}assert_eq!(shown(&h),last,"继续播放时应恢复对应显示段");
    h.media.position=6.0;for _ in 0..30 {h.frame(vec![]);}assert_eq!(shown(&h),first,"拖回前半句应恢复对应显示段");
    h.media.position=16.0;for _ in 0..30 {h.frame(vec![]);}assert_eq!(shown(&h),"下一句");
}

#[test]
fn island_lyrics_keep_title_fallback_and_intro_and_gaps(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.minimal=true;h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:1.0,duration:120.0,playing:true,..Default::default()};
    for state in 0..6 {
        h.app.ly=Default::default();
        h.app.ly.key="Track|Artist|120".into();
        h.media.position=if state==1 {7.0}else{1.0};
        match state {
            0=>h.app.ly.lyrics=Some(crate::lyrics::parse_lrc("[00:05]第一句")),
            1=>h.app.ly.lyrics=Some(crate::lyrics::parse_lrc("[00:01]第一句\n[00:05]\n[00:10]下一句")),
            2=>h.app.ly.loading=true,
            3=>h.app.ly.error=Some(crate::lyrics::FetchError::NotFound),
            4=>h.app.ly.error=Some(crate::lyrics::FetchError::Unavailable("503".into())),
            _=>h.app.ly.lyrics=Some(crate::lyrics::from_plain("没有时间戳")),
        }
        for _ in 0..90 {h.frame(vec![]);}
        if let Some(error)=&h.app.ly.error {
            assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()==error.label())),"歌词错误提示只应在展开内显示");
        }
        assert!((h.app.current_width()-crate::app::LYRIC_BAR_W).abs()<2.0,"状态 {state} 应保持音乐信息岛宽度");
        assert!(h.region.height()>30.0);
        if state>=2 {
            assert!(h.app.compact_notices(),"歌名兜底仍属于岛上歌词模式");
            assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="Track"&&t.opacity_factor>0.95)),"状态 {state} 应显示当前歌名");
        } else {
            let label=if state==0{"前奏"}else{"间奏"};
            assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()==label)),"同步歌词的前奏和间奏仍应显示状态");
        }
    }
}

#[test]
fn island_lyrics_title_fallback_shows_cover_and_elides_long_names(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.minimal=true;h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"没有歌词的钢琴曲".repeat(20),artist:"示例演奏者".into(),playing:true,artwork:Some(std::sync::Arc::new(egui::ColorImage::new([8,8],egui::Color32::BLUE))),..Default::default()};
    for _ in 0..90 {h.frame(vec![]);}
    let text=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==h.media.title=>Some(t),_=>None}).expect("无歌词时应绘制当前歌名");
    assert_eq!(text.galley.rows.len(),1,"长歌名只显示一行");
    assert!(text.galley.elided,"长歌名应在末尾省略");
    assert!(h.region.contains_rect(text.visual_bounding_rect()),"长歌名不能超出岛体");
    let cover=h.app.music_artwork.as_ref().unwrap().1.id();
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Rect(r)if r.brush.as_ref().is_some_and(|b|b.fill_texture_id==cover)&&h.region.contains_rect(r.rect))),"无歌词时仍应绘制封面");
}

#[test]
fn island_lyrics_title_fallback_follows_track_changes_and_normal_mode(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.minimal=true;h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"上一首".into(),artist:"示例歌手".into(),position:7.0,duration:120.0,playing:true,..Default::default()};
    h.app.set_lyrics("上一首","示例歌手",120.0,crate::lyrics::parse_lrc("[00:05]上一首的歌词"));
    for _ in 0..90 {h.frame(vec![]);}
    h.media.title="下一首纯音乐".into();h.media.playing=false;
    for _ in 0..90 {h.frame(vec![]);}
    assert!(h.app.island_lyric().is_none(),"换歌后不能显示上一首歌词");
    assert!(h.region.height()<20.0,"暂停后应恢复极简常驻");
    h.media.playing=true;for _ in 0..60 {h.frame(vec![]);}
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==h.media.title)),"继续播放且歌词未到达时应显示新歌名");
    h.media.playing=false;
    h.app.set_lyrics("下一首纯音乐","示例歌手",120.0,crate::lyrics::parse_lrc("[00:05]新歌的当前歌词"));
    for _ in 0..30 {h.frame(vec![]);}
    assert!(h.app.island_lyric().is_none(),"暂停时歌词到达也应保持常驻形态");
    h.media.playing=true;for _ in 0..30 {h.frame(vec![]);}
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="新歌的当前歌词")),"继续播放后应自动恢复当前歌词");
    for mode in 0..3 {
        h.app.settings.island_lyrics=mode!=0;
        h.app.settings.visible_pages[0]=mode!=1;
        if mode==2 {h.media=Default::default();}
        for _ in 0..90 {h.frame(vec![]);}
        assert!(h.region.height()<20.0,"关闭歌词、隐藏音乐页或播放器退出后应恢复极简常驻，状态 {mode}");
        assert!(!h.app.compact_notices());
    }
}

#[test]
#[ignore="按需导出岛上歌词的实际渲染画面"]
fn island_lyrics_layout_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_LYRIC_PREVIEW_DIR").expect("需指定预览目录"));
    std::fs::create_dir_all(&dest).unwrap();
    let mut cover=egui::ColorImage::new([64,64],egui::Color32::BLACK);
    for y in 0..64 {for x in 0..64 {
        let moon=(x as i32-44).pow(2)+(y as i32-18).pow(2)<64;
        cover[(x,y)]=if moon {egui::Color32::from_rgb(255,226,174)}
            else if y>44+x/5 {egui::Color32::from_rgb(29,61,70)}
            else {egui::Color32::from_rgb(68+y as u8,114+y as u8,151+y as u8/2)};
    }}
    let cover=std::sync::Arc::new(cover);
    for name in ["lyrics","loading","missing","no-cover","long-title","paused","intro","gap","idle"] {
        let mut h=H::new();h.app.settings.island_lyrics=true;
        h.media=crate::smtc::NowPlaying{title:"晚风与星光".into(),artist:"示例歌手".into(),position:7.0,duration:120.0,playing:name!="paused",artwork:Some(cover.clone()),..Default::default()};
        if name=="long-title" {h.media.title="愿每一个认真生活的人都能在平凡的日子里找到属于自己的光（钢琴独奏版）".into();}
        h.app.ly.key=format!("{}|{}|120",h.media.title,h.media.artist);
        match name {
            "lyrics"|"paused"=>h.app.set_lyrics("晚风与星光","示例歌手",120.0,crate::lyrics::parse_lrc("[00:05]愿你在平凡的日子里，找到自己的光")),
            "loading"=>h.app.ly.loading=true,
            "intro"=>h.app.set_lyrics("晚风与星光","示例歌手",120.0,crate::lyrics::parse_lrc("[00:20]晚风")),
            "gap"=>h.app.set_lyrics("晚风与星光","示例歌手",120.0,crate::lyrics::parse_lrc("[00:01]第一句\n[00:05]\n[00:10]下一句")),
            "idle"=>h.media=Default::default(),
            _=>h.app.ly.error=Some(crate::lyrics::FetchError::NotFound),
        }
        if name=="no-cover" {h.media.artwork=None;}
        let mut raster=crate::preview_raster::Raster::new(1280,540);
        for frame in 0..90 {
            let t=frame as f64/60.0;
            let env=Env{t,clock:"09:41:00".into(),today:"2026-11-06".into(),date:"11月6日 周五".into(),media:h.media.clone(),..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H))),time:Some(t),
                viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
            let output=h.ctx.run(raw,|ctx|{
                egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let win=ui.max_rect();h.app.frame(ui,win,&env);});
            });
            raster.update_textures(&output.textures_delta);
            if frame==89 {
                raster.buf.fill([0.95,0.96,0.98,1.0]);
                raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                let image=image::RgbaImage::from_raw(1280,540,rgba).unwrap();
                image::imageops::crop_imm(&image,220,0,840,110).to_image().save(dest.join(format!("{name}.png"))).unwrap();
            }
            raster.free_textures(&output.textures_delta);
        }
    }
}

#[test]
fn music_lyrics_keep_syncing_when_only_the_collapsed_lyrics_are_enabled() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.opts.fetch_lyrics=true;h.app.show_lyrics=false;h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:7.0,duration:120.0,playing:true,..Default::default()};
    let (tx,rx)=std::sync::mpsc::channel();
    h.app.ly.key="Track|Artist|120".into();h.app.ly.loading=true;h.app.ly.rx=Some(rx);
    tx.send(Ok((crate::lyrics::parse_lrc("[00:05]当前歌词\n[00:10]下一句"),false))).unwrap();
    h.frame(vec![]);
    assert!(!h.app.ly.loading);
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("当前歌词"));
    h.media.position=12.0;h.frame(vec![]);
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("下一句"));
}

#[test]
fn music_lyrics_respect_the_switch_and_do_not_show_stale_or_unsynced_lines() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.minimal=true;h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:7.0,duration:120.0,playing:true,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:05]当前歌词"));
    for _ in 0..90 {h.frame(vec![]);}
    assert!(h.region.height()>30.0,"主动开启岛上歌词后应可在收起状态阅读");
    h.app.settings.island_lyrics=false;
    for _ in 0..90 {h.frame(vec![]);}
    assert!(h.app.island_lyric().is_none() && h.region.height()<20.0);
    h.app.settings.island_lyrics=true;
    h.media.title="Another track".into();h.frame(vec![]);
    assert!(h.app.island_lyric().is_none(),"换歌后不能显示上一首的歌词");
    h.app.set_lyrics("Another track","Artist",120.0,crate::lyrics::from_plain("没有时间戳的歌词"));h.frame(vec![]);
    assert!(h.app.island_lyric().is_none(),"不能将纯文本歌词伪装成当前句");
}

#[test]
fn hover_expands_with_overshoot_and_leave_collapses() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = H::new();
    h.hover(Pos2::new(WIN_W / 2.0, 12.0), 0.1);
    assert!(h.app.progress() < 0.1, "意图缓冲期内不应展开");
    let mut peak: f32 = 0.0;
    for _ in 0..60 {
        h.hover(Pos2::new(WIN_W / 2.0, 12.0), 1.0 / 60.0);
        peak = peak.max(h.app.progress());
    }
    assert!(peak > 1.01 && peak < 1.12, "展开应有轻微过冲, peak={peak}");
    assert!((h.app.progress() - 1.0).abs() < 0.02);
    h.cursor = Some(Pos2::new(WIN_W / 2.0, PH + 30.0));
    for _ in 0..40 {
        h.frame(vec![]);
    }
    assert!(h.app.progress() < 0.02, "移开后应收起");
}

#[test]
fn browser_minimal_preserves_preferences_and_restores_the_previous_mode(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.pet=true;h.app.settings.minimal=false;h.browser_at_top=Some(true);
    for _ in 0..90 {h.frame(vec![]);}
    assert!(h.hit.height()>20.0,"开关默认关闭时不应自动改变模式");
    h.app.settings.browser_minimal=true;h.app.settings.save();h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:7.0,duration:120.0,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:05]当前歌词"));
    for _ in 0..90 {h.frame(vec![]);}
    assert!(h.hit.height()<10.0,"浏览器顶部受遮挡时应收为细刘海，歌词和吉祥物不能撑大它");
    assert!(h.app.settings.pet && !h.app.settings.minimal,"自动极简不能修改手动模式");
    let saved=crate::model::Settings::load();
    assert!(saved.browser_minimal && saved.pet && !saved.minimal,"应只持久化开关，保留原模式");
    h.browser_at_top=None;for _ in 0..30 {h.frame(vec![]);}
    assert!(h.hit.height()<10.0,"打开岛仔设置或托盘时应保留浏览器避让状态");
    h.browser_at_top=Some(false);for _ in 0..90 {h.frame(vec![]);}
    assert!(h.app.pet_enabled() && h.hit.height()>20.0,"离开浏览器或还原窗口后应恢复");
    h.browser_at_top=Some(true);for _ in 0..90 {h.frame(vec![]);}
    h.app.settings.browser_minimal=false;for _ in 0..90 {h.frame(vec![]);}
    assert!(h.app.pet_enabled() && h.hit.height()>20.0,"关闭开关后应立即恢复");
    h.app.settings.island_lyrics=false;h.app.set_minimal(true);h.browser_at_top=Some(false);for _ in 0..90 {h.frame(vec![]);}
    assert!(h.app.settings.minimal && h.hit.height()<10.0,"离开浏览器不能撤销手动极简");
}

#[test]
fn browser_minimal_closes_the_open_island_but_still_allows_deliberate_hover(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.browser_minimal=true;
    h.app.show_page(0);h.hover(H::local(300.0,60.0),0.8);
    assert!(h.app.open);
    h.browser_at_top=Some(true);h.frame(vec![]);
    assert!(!h.app.open,"切到浏览器时不能继续保留展开状态遮挡标签栏");
    assert!(!h.hit.contains(Pos2::new(WIN_W/2.0,12.0)),"收起动画期间也应让标签栏接收点击");
    h.hover(Pos2::new(WIN_W/2.0,2.0),0.25);
    assert!(!h.app.open,"短暂碰到刘海不能误触");
    h.hover(Pos2::new(WIN_W/2.0,2.0),0.6);
    assert!(h.app.open,"仍应能主动悬停打开岛");
}

#[test]
fn minimal_hover_excludes_the_surrounding_browser_tabs() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.minimal=true;
    for _ in 0..90 {h.frame(vec![]);}
    for cursor in [
        Pos2::new(WIN_W/2.0,crate::app::BAR_H_MIN+1.0),
        Pos2::new(WIN_W/2.0+crate::app::BAR_W_MIN/2.0+2.0,2.0),
        Pos2::new(WIN_W/2.0-crate::app::BAR_W_MIN/2.0-2.0,2.0),
    ] {
        assert!(!h.hit.contains(cursor),"刘海外的标签栏应接收鼠标操作");
        h.hover(cursor,1.0);
        assert!(!h.app.open,"鼠标未触碰极简刘海时不得展开");
    }
}

#[test]
fn minimal_hover_requires_continuous_deliberate_contact() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.minimal=true;
    for _ in 0..90 {h.frame(vec![]);}
    let inside=Pos2::new(WIN_W/2.0,2.0);
    h.hover(inside,0.25);
    assert!(!h.app.open,"短暂经过极简刘海不得展开");
    h.hover(Pos2::new(WIN_W/2.0,8.0),0.1);
    h.hover(inside,0.25);
    assert!(!h.app.open,"移出刘海应重新计算停留时间");
    h.hover(inside,0.7);
    assert!(h.app.open,"在刘海内持续停留仍应展开");
    h.click(H::local(398.0,68.0));
    assert_eq!(h.app.page,1,"展开后应能正常操作面板");
}

#[test]
fn minimal_hover_does_not_catch_tabs_during_collapse() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.minimal=true;
    h.hover(Pos2::new(WIN_W/2.0,2.0),1.0);
    assert!(h.app.open);
    h.cursor=Some(Pos2::new(WIN_W/2.0,PH+30.0));
    for _ in 0..30 {
        h.frame(vec![]);
        if !h.app.open {break;}
    }
    assert!(!h.app.open);
    assert!(h.app.progress()>0.04,"检查收起动画仍在进行时的命中范围");
    let tab=Pos2::new(WIN_W/2.0,12.0);
    assert!(!h.hit.contains(tab),"收起动画不得继续拦截标签栏");
    h.hover(tab,1.0);
    assert!(!h.app.open);
}

#[test]
fn music_header_shortcut_returns_from_every_page_without_controlling_playback() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for playing in [true,false] {
        let mut h=H::new();h.app.settings.page_order=[5,2,0,1,3,4,6,7,8];
        h.media=crate::smtc::NowPlaying{source:"cloudmusic.exe".into(),title:"Track".into(),artist:"Artist".into(),playing,duration:120.0,..Default::default()};
        for page in 1..crate::app::PAGE_NAMES.len() {
            h.app.show_page(page);h.hover(H::local(300.0,60.0),0.8);h.cmds.clear();
            h.click(H::local(367.0,18.0));
            assert_eq!(h.app.page,0,"播放状态 {playing} 时应从页面 {page} 直达音乐页");
            assert!(h.app.open);assert!(!h.app.settings_open);assert_eq!(h.app.media.playing,playing);
            assert!(h.cmds.iter().all(|cmd|!matches!(cmd,crate::app::Cmd::Media(_)|crate::app::Cmd::MusicVolume(..))),"返回音乐页不能改变播放或音量");
        }
    }
}

#[test]
fn music_header_shortcut_keeps_hidden_music_page_disabled() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:120.0,..Default::default()};
    h.app.set_page_visible(0,false);h.app.show_page(4);h.hover(H::local(300.0,60.0),0.8);
    h.click(H::local(367.0,18.0));
    assert_eq!(h.app.page,4);assert!(!h.app.settings.visible_pages[0]);
    assert!(h.app.settings_open);assert_eq!(h.app.settings_tab,1);
}

#[test]
fn music_header_shortcut_is_absent_without_music_and_keeps_settings_clickable() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for media in [crate::smtc::NowPlaying::default(),crate::smtc::NowPlaying{title:"Video".into(),artist:"Channel".into(),playing:true,playback_type:crate::smtc::MediaKind::Video,..Default::default()}] {
        let mut h=H::new();h.media=media;h.app.show_page(4);h.hover(H::local(300.0,60.0),0.8);
        h.click(H::local(367.0,18.0));assert_eq!(h.app.page,4);assert!(!h.app.settings_open);
        h.click(H::local(394.0,18.0));assert!(h.app.settings_open);assert_eq!(h.app.settings_tab,4);
    }
}

#[test]
fn arrows_and_dots_switch_pages() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = H::new();
    h.hover(Pos2::new(WIN_W / 2.0, 12.0), 1.0);
    h.click(H::local(398.0, 68.0));
    assert_eq!(h.app.page, 1);
    h.hover(H::local(300.0, 60.0), 0.8);
    h.click(H::local(21.0, 68.0));
    assert_eq!(h.app.page, 0);
    h.hover(H::local(300.0, 60.0), 0.8);
    h.click(H::local(398.0, 68.0));
    h.hover(H::local(300.0, 60.0), 0.8);
    h.click(H::local(398.0, 68.0));
    assert_eq!(h.app.page, 2);
}

#[test]
fn wheel_navigation_reaches_every_page_in_both_directions(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.shelf.items.clear();h.app.todos.items.clear();
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    h.hover(H::local(210.0,18.0),0.3);
    for (delta,pages) in [(-40.0,[1,2,3,4,5,6,7,8,0]),(40.0,[8,7,6,5,4,3,2,1,0])] {
        for page in pages {
            h.wheel(delta);
            assert_eq!(h.app.page,page,"滚轮翻页不能被空的置物架或待办卡住");
            h.hover(H::local(210.0,18.0),0.25);
        }
    }
}

#[test]
fn wheel_scrolls_lists_only_inside_content_and_respects_edges(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.todos.items=(0..4).map(|i|crate::model::TodoItem{text:format!("任务 {i}"),done_on:None,due:None,reminded:false,..Default::default()}).collect();
    h.app.shelf.items=(0..6).map(|i|std::path::PathBuf::from(format!("scroll-test-{i}.txt"))).collect();
    for page in [3,4] {
        h.app.show_page(page);h.hover(H::local(300.0,60.0),1.0);
        h.wheel(-40.0);
        assert_eq!(h.app.page,page,"有更多内容时应先滚动列表");
        assert_eq!(if page==3 {h.app.shelf_first}else{h.app.todo_first},1);
        h.hover(H::local(300.0,60.0),0.25);h.wheel(-40.0);
        assert_eq!(h.app.page,page,"内容溢出时滚到底也应留在本页");
        h.app.show_page(page);h.hover(H::local(210.0,18.0),0.3);h.wheel(-40.0);
        assert_eq!(h.app.page,page+1,"顶栏滚轮不应被内容列表拦截");
    }
}

#[test]
fn wheel_overflowing_shelf_stays_at_both_edges(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    for count in [6,9]{
        h.app.shelf.items=(0..count).map(|i|std::path::PathBuf::from(format!("scroll-test-{i}.txt"))).collect();
        h.app.shelf_first=0;h.app.show_page(3);
        let last=count-5;
        for (delta,first) in [(40.0_f32,0),(-40.0,1),(-400.0,last),(-40.0,last),(40.0,last-1),(400.0,0),(40.0,0)]{
            h.hover(H::local(300.0,60.0),0.7);
            for _ in 0..(delta.abs() as usize/40){h.wheel(delta.signum()*40.0);}
            assert_eq!(h.app.page,3,"置物架溢出后连续滚动或停顿再滚都应留在本页，条数 {count}");
            assert_eq!(h.app.shelf_first,first);
        }
        for (delta,page) in [(-40.0,4),(40.0,2)]{
            h.hover(H::local(300.0,60.0),0.7);
            h.click(H::local(if delta<0.0 {398.0}else{21.0},68.0));
            assert_eq!(h.app.page,page,"置物架溢出后仍应允许箭头切页");
            h.app.show_page(3);h.hover(H::local(210.0,18.0),0.3);h.wheel(delta);
            assert_eq!(h.app.page,page,"置物架顶栏仍应允许滚轮翻页");
            h.app.show_page(3);
        }
    }
}

#[test]
fn wheel_overflowing_todos_stay_at_both_edges(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    for count in [4,8]{
        h.app.todos.items=(0..count).map(|i|crate::model::TodoItem{text:format!("任务 {i}"),..Default::default()}).collect();
        h.app.todo_first=0;h.app.show_page(4);
        let last=count-3;
        for (delta,first) in [(40.0_f32,0),(-40.0,1),(-400.0,last),(-40.0,last),(40.0,last-1),(400.0,0),(40.0,0)]{
            h.hover(H::local(300.0,60.0),0.3);
            for _ in 0..(delta.abs() as usize/40){h.wheel(delta.signum()*40.0);}
            assert_eq!(h.app.page,4,"待办溢出后滚动到首尾都不应翻页，条数 {count}");
            assert_eq!(h.app.todo_first,first);
        }
        for (delta,page) in [(-40.0,5),(40.0,3)]{
            h.app.show_page(4);h.hover(H::local(100.0,60.0),0.3);h.wheel(delta);
            assert_eq!(h.app.page,page,"待办列表外仍应允许滚轮翻页");
        }
    }
}

#[test]
fn wheel_clipboard_stays_at_both_edges() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.show_page(3);
    h.app.shelf_tab=1;
    for count in [crate::pages::CLIP_ROWS+1] {
        h.app.clipboard.lock().items.clear();
        for i in 0..count { h.app.clipboard.lock().push(&i.to_string(), "09:00"); }
        let last=count.saturating_sub(crate::pages::CLIP_ROWS);
        for (delta, first) in [(40.0,0),(-40.0,last),(-40.0,last),(40.0,0),(40.0,0)] {
            h.hover(H::local(300.0,60.0),0.4);
            h.wheel(delta);
            assert_eq!(h.app.page,3);
            assert_eq!(h.app.clip_first,first);
        }
        for (delta,page) in [(-40.0,4),(40.0,2)]{
            h.app.show_page(3);h.hover(H::local(210.0,18.0),0.4);h.wheel(delta);
            assert_eq!(h.app.page,page,"剪贴板列表外仍应允许滚轮翻页");
        }
    }
}
#[test]
fn wheel_handles_small_deltas_without_skipping_pages_on_bursts(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.hover(H::local(210.0,18.0),0.3);
    for _ in 0..16 {h.wheel(-0.5);}
    assert_eq!(h.app.page,1,"高精度滚轮的小幅输入应能累积成一次翻页");
    h.hover(H::local(210.0,18.0),0.3);
    for _ in 0..5 {h.wheel(-40.0);}
    assert_eq!(h.app.page,2,"一次连续滚动不能瞬间跳过多个页面");
}

#[test]
fn navigation_wraps_between_nine_pages_and_notification_settings_remain_accessible() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.click(H::local(21.0,68.0));assert_eq!(h.app.page,8);
    h.hover(H::local(300.0,60.0),0.8);h.click(H::local(398.0,88.0));assert_eq!(h.app.page,0);
    h.app.open_settings_category(3);assert!(h.app.settings_open);assert_eq!(h.app.settings_tab,6);
}

#[test]
fn resident_quota_moves_left_and_releases_battery_space(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.pet=false;
    h.power=(Some(80),true);h.quota_accounts(1);
    for _ in 0..90 {h.frame(vec![]);}
    let before=h.quota_centers()[0];let width=h.app.current_width();assert!(before>WIN_W/2.0);
    h.app.settings.show_battery=false;for _ in 0..90 {h.frame(vec![]);}
    let after=h.quota_centers()[0];assert!(after<WIN_W/2.0,"关闭电量后，额度应补到时间左侧");
    assert!(h.app.current_width()<width-20.0,"关闭组件后应释放实际占位");
    h.app.settings.show_battery=true;h.power=(None,false);for _ in 0..90 {h.frame(vec![]);}
    assert!((h.quota_centers()[0]-after).abs()<0.2,"无电池设备应采用相同补位布局");
    h.click(Pos2::new(after,15.0));assert_eq!(h.app.page(),6,"补位后仍可点击额度圆环");
}

#[test]
fn resident_quotas_coexist_with_music_and_focus(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.pet=false;h.quota_accounts(3);
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:120.0,..Default::default()};
    h.app.pomo.running=true;h.app.pomo.remaining=600.0;
    for _ in 0..90 {h.frame(vec![]);}
    assert_eq!(h.quota_centers().len(),3,"播放和专注状态不应挤掉常驻额度");
    assert!(h.app.current_width()<PW,"多组件应在单行内排布");
    h.app.settings.minimal=true;for _ in 0..90 {h.frame(vec![]);}
    assert!(h.quota_centers().is_empty());assert!(h.region.height()<20.0,"仍须尊重极简开关");
}

#[test]
fn resident_fifth_account_stays_clear_of_clock_and_opens(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.pet=false;h.app.settings.show_battery=false;h.quota_accounts(5);
    for _ in 0..90 {h.frame(vec![]);}
    let text_rect=|label:&str|h.shapes.iter().find_map(|shape|match &shape.shape{
        egui::Shape::Text(text) if text.galley.text()==label=>Some(text.visual_bounding_rect()),_=>None,
    }).expect("常驻文字应可见");
    let last=h.app.resident.current().x[crate::bar_layout::quota_slot(4)].unwrap()+WIN_W/2.0;
    assert!(last+13.5+8.0<text_rect("09:41").left());
    h.click(Pos2::new(last,h.region.center().y));assert_eq!(h.app.page(),6);
}

#[test]
#[ignore = "按需导出常驻组件排布与补位动画"]
fn resident_layout_gallery(){
    fn capture(h:&mut H,raster:&mut crate::preview_raster::Raster,t:f64,draw:bool)->Vec<u8>{
        let env=Env{t,clock:"09:41:00".into(),today:"2026-10-31".into(),date:"10月31日 周六".into(),battery:h.power.0,charging:h.power.1,media:h.media.clone(),bt_ready:true,..Default::default()};
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H))),time:Some(t),
            viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let win=ui.max_rect();h.app.frame(ui,win,&env);});});
        raster.update_textures(&output.textures_delta);let mut rgba=Vec::new();
        if draw {raster.buf.fill([0.09,0.11,0.18,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);raster.to_rgba8(&mut rgba);}
        raster.free_textures(&output.textures_delta);rgba
    }
    fn demo(battery:bool,accounts:usize,music:bool,focus:bool,pet:bool)->H{
        let mut h=H::new();h.app.settings.show_battery=battery;h.app.settings.pet=pet;h.power=(Some(80),true);h.quota_accounts(accounts);
        for (i,a) in h.app.quota.config.accounts.iter().enumerate(){
            if let Some(Ok(snapshot))=h.app.quota.rows.get_mut(&a.id){
                snapshot.windows[0].remaining=82.0-i as f32*11.0;
                if i%2==0 {snapshot.windows.push(crate::quota::Window{label:"5 小时".into(),seconds:Some(18000),remaining:58.0,reset:None,amount:None});}
            }
        }
        if music {h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:120.0,..Default::default()};}
        if focus {h.app.pomo.running=true;h.app.pomo.remaining=1283.0;}
        if pet {h.app.pet.react(crate::extras::PetState::Happy);}
        h
    }
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_RESIDENT_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    let scenes=[(true,1,false,false,false),(false,1,false,false,false),(true,3,false,false,false),(false,3,true,false,false),
        (true,3,true,false,false),(false,3,true,true,false),(false,5,false,false,false),(false,2,false,false,true)];
    for (i,(battery,accounts,music,focus,pet)) in scenes.into_iter().enumerate(){
        let mut h=demo(battery,accounts,music,focus,pet);let mut raster=crate::preview_raster::Raster::new(1280,380);
        for frame in 0..90 {
            let rgba=capture(&mut h,&mut raster,frame as f64/60.0,frame==89);
            if frame==89 {image::save_buffer(dest.join(format!("layout-{i}.png")),&rgba,1280,380,image::ColorType::Rgba8).unwrap();}
        }
    }
    let mut h=demo(true,3,true,false,false);let mut raster=crate::preview_raster::Raster::new(1280,380);
    for frame in 0..240 {
        if frame==60 {h.app.settings.show_battery=false;}if frame==180 {h.app.settings.show_battery=true;}
        let rgba=capture(&mut h,&mut raster,frame as f64/60.0,frame%3==0);
        if frame%3==0 {
            let image=image::RgbaImage::from_raw(1280,380,rgba).unwrap();
            image::imageops::crop_imm(&image,200,0,880,80).to_image().save(dest.join(format!("frame-{frame:03}.png"))).unwrap();
        }
    }
    eprintln!("常驻组件预览：{}",dest.display());
}

#[test]
fn quota_text_stays_inside_the_inner_ring_at_zero_and_full_usage() {
    use crate::quota::{Event,Snapshot,Window};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dual in [true,false] {for remaining in [72.0,0.0,100.0] {
        let mut h=H::new();h.quota_accounts(1);
        let reset=Some((chrono::Local::now()+chrono::Duration::days(400)).timestamp());
        let mut windows=vec![Window{label:"本周".into(),seconds:Some(604800),remaining,reset,amount:None}];
        if dual {windows.insert(0,Window{label:"5 小时".into(),seconds:Some(18000),remaining,reset,amount:None});}
        h.app.quota_event(Event{id:"preview-0".into(),result:Ok(Snapshot{windows,..Default::default()})});
        h.app.show_page(6);h.hover(H::local(300.0,60.0),1.0);
        let center=H::local(103.0,105.0);
        let inner=h.shapes.iter().filter_map(|shape|match &shape.shape {
            egui::Shape::Circle(circle) if circle.center.distance(center)<0.5 && circle.radius>15.0 && circle.stroke.width>0.0=>Some(circle.radius-circle.stroke.width/2.0),
            egui::Shape::Path(path) if path.points.len()>2 && path.stroke.width>0.0=>{
                let radius=path.points[0].distance(center);
                ((15.0..35.0).contains(&radius) && path.points.iter().all(|point|(point.distance(center)-radius).abs()<0.5)).then_some(radius-path.stroke.width/2.0)
            },
            _=>None,
        }).reduce(f32::min).unwrap_or(35.0);
        for shape in &h.shapes {if let egui::Shape::Text(text)=&shape.shape {
            let rect=text.visual_bounding_rect();
            if rect.center().distance(center)<35.0 {
                for corner in [rect.left_top(),rect.right_top(),rect.left_bottom(),rect.right_bottom()] {
                    assert!(corner.distance(center)+0.5<inner,"{} 的文字不得碰到额度环，双周期={dual}，剩余={remaining}",text.galley.text());
                }
            }
        }}
        let texts:Vec<_>=h.shapes.iter().filter_map(|shape|match &shape.shape {
            egui::Shape::Text(text) if text.visual_bounding_rect().left()>=H::local(157.0,0.0).x && text.visual_bounding_rect().top()>=68.0 && text.visual_bounding_rect().bottom()<=144.0=>Some((text.galley.text().to_owned(),text.visual_bounding_rect())),_=>None,
        }).collect();
        for (i,(a,rect)) in texts.iter().enumerate(){for (b,other) in &texts[i+1..]{assert!(!rect.intersects(*other),"{a} 与 {b} 不得重叠");}}
        let label=if dual {"5 小时"}else{"本周剩余"};
        assert!(h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text()==label)),"周期说明应完整可见");
    }}
}

fn kiro_snapshot(remaining:f64,limit:f64)->crate::quota::Snapshot{
    crate::quota::parse(crate::quota::Provider::Kiro,&serde_json::json!({"userInfo":{"userId":"ui-kiro-user"},"subscriptionInfo":{"subscriptionTitle":"KIRO PRO"},
        "usageBreakdownList":[{"resourceType":"CREDIT","usageLimitWithPrecision":limit,"currentUsageWithPrecision":limit-remaining,"nextDateReset":4102444800_i64}]})).unwrap()
}

#[test]
fn kiro_cancelled_or_closed_connections_cannot_save_late_results(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for close_settings in [false,true]{
        let mut h=H::new();let saved=h.app.quota.config.clone();h.app.settings_open=true;h.app.quota.connect_open=true;
        let(connect,tx)=crate::quota::kiro::Connect::pending(None);h.app.quota.kiro_connecting=Some(connect);
        if close_settings{h.app.settings_open=false;}else{h.app.quota.connect_open=false;}
        h.app.quota_tick();assert!(h.app.quota.kiro_connecting.as_ref().unwrap().cancelled());
        tx.send(Ok(crate::quota::kiro::Connected{source:"kiro.json".into(),label:"Kiro".into(),snapshot:kiro_snapshot(900.0,1000.0)})).unwrap();
        h.app.quota_tick();assert_eq!(h.app.quota.config,saved);assert!(h.app.quota.kiro_connecting.is_none());assert!(h.app.quota.rows.is_empty());
    }
}

#[test]
fn kiro_connection_save_failure_keeps_the_original_account_and_cache(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings_open=true;h.app.quota.connect_open=true;
    let snapshot=kiro_snapshot(900.0,1000.0);let owner=snapshot.kiro.as_ref().unwrap().owner.clone();
    let account=crate::quota::Account{id:"kiro".into(),provider:crate::quota::Provider::Kiro,alias:"工作".into(),source:"old.json".into(),slot:Some(owner),enabled:true,pinned:true};
    h.app.quota.config.accounts.push(account.clone());h.app.quota.rows.insert(account.id.clone(),Ok(snapshot.clone()));let saved=h.app.quota.config.clone();
    let(connect,tx)=crate::quota::kiro::Connect::pending(Some(account));h.app.quota.kiro_connecting=Some(connect);
    tx.send(Ok(crate::quota::kiro::Connected{source:"new.json".into(),label:"新备注".into(),snapshot})).unwrap();
    let original=std::env::var_os("DAOZAI_DATA_DIR").unwrap();std::fs::create_dir_all(&original).unwrap();let blocked=std::path::PathBuf::from(&original).join("kiro-blocked");std::fs::write(&blocked,"blocked").unwrap();std::env::set_var("DAOZAI_DATA_DIR",&blocked);
    h.app.quota_tick();std::env::set_var("DAOZAI_DATA_DIR",original);
    assert_eq!(h.app.quota.config,saved);assert!(h.app.quota.note.contains("配置目录"));assert_eq!(h.app.quota.rows["kiro"].as_ref().unwrap().windows[0].amount.as_ref().unwrap().remaining,900.0);
}

#[test]
fn kiro_ring_uses_logo_for_all_point_balances_and_skins(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for skin in crate::model::Skin::ALL{for remaining in [0.0,0.01,99.9,1000.0,10000.0]{
        let mut h=H::new();h.app.settings.skin=skin;let snapshot=kiro_snapshot(remaining,10000.0);let full=format!("{} 点",snapshot.windows[0].value_text());
        h.app.quota.config.accounts.push(crate::quota::Account{id:"kiro".into(),provider:crate::quota::Provider::Kiro,alias:"Kiro".into(),source:"kiro.json".into(),slot:None,enabled:true,pinned:true});h.app.quota.config.selected="kiro".into();
        h.app.quota_event(crate::quota::Event{id:"kiro".into(),result:Ok(snapshot)});h.app.show_page(6);h.hover(H::local(300.0,60.0),0.7);
        let find=|wanted:&str|h.shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Text(t)if t.galley.text()==wanted=>Some(t.visual_bounding_rect()),_=>None}).unwrap_or_else(||panic!("缺少文本 {wanted}"));
        let center=H::local(103.0,105.0);
        let logo=h.shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Mesh(mesh)=>{let rect=mesh.calc_bounds();(rect.center().distance(center)<0.8&&(30.0..36.0).contains(&rect.width())).then_some(rect)},_=>None}).expect("圆环中心应显示 Kiro 标识");
        for corner in [logo.left_top(),logo.right_top(),logo.left_bottom(),logo.right_bottom()]{assert!(corner.distance(center)<27.5,"{skin:?} 的标识应位于圆环内");}
        assert!(!find("月度额度剩余").intersects(find(&full)),"{skin:?} {remaining} 点的标签重叠");
    }}
}

#[test]
fn kiro_logo_and_point_details_fit_the_card(){
    use crate::quota::{Amount,Event,Provider,Snapshot,Window};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.quota.config.accounts.push(crate::quota::Account{id:"kiro".into(),provider:Provider::Kiro,alias:"Kiro".into(),source:"kiro.json".into(),slot:None,enabled:true,pinned:true});
    h.app.quota.config.selected="kiro".into();
    let reset=Some((chrono::Local::now()+chrono::Duration::days(20)).timestamp());
    h.app.quota_event(Event{id:"kiro".into(),result:Ok(Snapshot{kiro:kiro_snapshot(904.09,1000.0).kiro,plan:"KIRO PRO".into(),at:chrono::Utc::now().timestamp(),
        windows:vec![Window{label:"月度额度".into(),seconds:None,remaining:90.4,reset,amount:Some(Amount::new(904.09,1000.0))}],..Default::default()})});
    h.app.show_page(6);h.hover(H::local(300.0,60.0),1.0);
    let center=H::local(103.0,105.0);
    let inner=h.shapes.iter().filter_map(|shape|match &shape.shape{
        egui::Shape::Circle(circle) if circle.center.distance(center)<0.5 && circle.radius>15.0 && circle.stroke.width>0.0=>Some(circle.radius-circle.stroke.width/2.0),
        egui::Shape::Path(path) if path.points.len()>2 && path.stroke.width>0.0=>{let radius=path.points[0].distance(center);((15.0..35.0).contains(&radius) && path.points.iter().all(|point|(point.distance(center)-radius).abs()<0.5)).then_some(radius-path.stroke.width/2.0)},
        _=>None}).reduce(f32::min).unwrap_or(35.0);
    let text=|wanted:&str|h.shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Text(text)if text.galley.text()==wanted=>Some(text.visual_bounding_rect()),_=>None});
    let logo=h.shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Mesh(mesh)=>{let rect=mesh.calc_bounds();(rect.center().distance(center)<0.8&&(30.0..36.0).contains(&rect.width())).then_some(rect)},_=>None}).expect("圆环中心应显示 Kiro 标识");
    for corner in [logo.left_top(),logo.right_top(),logo.left_bottom(),logo.right_bottom()]{assert!(corner.distance(center)+0.5<inner,"标识应位于额度环内");}
    let center_area=Rect::from_center_size(center,Vec2::splat(40.0));
    assert!(!h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(t) if center_area.intersects(t.visual_bounding_rect()))),"圆环中心应只显示 Kiro 标识");
    let label=text("月度额度剩余").expect("周期说明");let value=text("904.1 / 1000 点").expect("剩余量与总额");
    assert!(!label.intersects(value),"说明与数值不得重叠");
    assert!(h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text()=="Kiro · KIRO PRO")),"套餐名应显示");
    if let Some(dest)=std::env::var_os("DAOZAI_KIRO_PREVIEW_DIR"){
        let mut raster=crate::preview_raster::Raster::new((WIN_W*2.0) as usize,(WIN_H*2.0) as usize);
        let t=h.t+1.0/60.0;
        let env=Env{t,clock:"09:41:00".into(),today:"2026-10-31".into(),date:"10月31日 周六".into(),battery:h.power.0,charging:h.power.1,media:h.media.clone(),bt_ready:true,..Default::default()};
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H))),time:Some(t),
            viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let win=ui.max_rect();h.app.frame(ui,win,&env);});});
        raster.update_textures(&output.textures_delta);
        raster.buf.fill([0.09,0.11,0.18,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
        let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
        image::save_buffer(std::path::PathBuf::from(&dest).join("kiro-credits.png"),&rgba,raster.w as u32,raster.h as u32,image::ColorType::Rgba8).unwrap();
        raster.free_textures(&output.textures_delta);
    }
}

#[test]
fn kiro_credits_follow_the_ring_on_the_resident_bar(){
    use crate::quota::{Amount,Event,Provider,Snapshot,Window};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.pet=false;
    h.app.quota.config.accounts.push(crate::quota::Account{id:"kiro".into(),provider:Provider::Kiro,alias:"Kiro".into(),source:"kiro.json".into(),slot:None,enabled:true,pinned:true});
    h.app.quota.config.selected="kiro".into();
    h.app.quota_event(Event{id:"kiro".into(),result:Ok(Snapshot{kiro:kiro_snapshot(904.09,1000.0).kiro,plan:"KIRO PRO".into(),at:chrono::Utc::now().timestamp(),
        windows:vec![Window{label:"月度额度".into(),seconds:None,remaining:90.4,reset:None,amount:Some(Amount::new(904.09,1000.0))}],..Default::default()})});
    for _ in 0..60{h.frame(vec![]);}
    assert_eq!(h.app.quota_bar_indices().len(),1,"点数的 Kiro 账号应常驻");
    let slot=crate::bar_layout::quota_slot(0);
    let layout=h.app.resident.current();
    let extra=layout.widths[slot]-27.0;
    assert!(extra>10.0,"环右侧要为点数让出位置：{:?}",layout.widths[slot]);
    let ring=WIN_W/2.0+layout.x[slot].unwrap()-extra/2.0;
    let text=h.shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Text(t)if t.galley.text()=="904"=>Some(t.visual_bounding_rect()),_=>None}).expect("常驻栏应在环右侧显示剩余点数");
    assert!(text.center().y<30.0,"点数应与图标同一行");
    assert!((text.left()-ring-15.5).abs()<2.5,"点数紧跟环右侧：ring={ring} text={:?}",text);
    h.click(text.center());
    assert_eq!(h.app.page(),6,"点数字也能打开额度页");
}

#[test]
#[ignore="按需导出 Kiro 积分池、万元额套餐及设置明细"]
fn kiro_credit_pools_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_KIRO_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for(skin,name)in [(crate::model::Skin::Classic,"dark"),(crate::model::Skin::GlassLight,"light")]{for pools in [false,true]{
        let mut h=H::new();h.app.settings.skin=skin;let mut snapshot=kiro_snapshot(10000.0,10000.0);
        if pools{snapshot=crate::quota::parse(crate::quota::Provider::Kiro,&serde_json::json!({"userInfo":{"userId":"ui-kiro-user"},"subscriptionInfo":{"subscriptionTitle":"KIRO PRO"},"usageBreakdownList":[{"resourceType":"CREDIT","usageLimitWithPrecision":1000,"currentUsageWithPrecision":1000,"nextDateReset":1796083200_i64,"freeTrialInfo":{"freeTrialStatus":"ACTIVE","usageLimit":500,"currentUsage":100,"freeTrialExpiry":1827619200_i64},"bonuses":[{"status":"ACTIVE","displayName":"加购点数","usageLimit":125,"currentUsage":25,"expiresAt":1827619200_i64}]}]})).unwrap();}
        let id="kiro".to_owned();h.app.quota.config.accounts.push(crate::quota::Account{id:id.clone(),provider:crate::quota::Provider::Kiro,alias:"Kiro".into(),source:"kiro.json".into(),slot:None,enabled:true,pinned:true});h.app.quota.config.selected=id.clone();h.app.quota_event(crate::quota::Event{id,result:Ok(snapshot)});h.app.show_page(6);
        let mut raster=feature_raster();shoot_feature(&mut h,&mut raster,&dest,&format!("kiro-{name}-{}",if pools{"pools"}else{"power"}),75,Some(H::local(300.0,60.0)),"2026-11-11");
        if pools{
            h.app.settings.settings_theme=if name=="light"{crate::settings_theme::SettingsTheme::Light}else{crate::settings_theme::SettingsTheme::Dark};h.app.settings_tab=11;h.app.open_settings();h.ctx.set_embed_viewports(false);
            let size=egui::vec2(650.0,650.0);let ppp=1.5;let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
            egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{let id=viewport.ids.this;let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};*capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));});
            let mut raster=crate::preview_raster::Raster::new(975,975);
            for frame in 0..6{let root=h.ctx.run(egui::RawInput::default(),|ctx|h.app.settings_ui(ctx,false));raster.update_textures(&root.textures_delta);let child=output.lock().unwrap().take().unwrap();raster.update_textures(&child.textures_delta);
                if frame==5{raster.buf.fill([0.1,0.1,0.1,1.0]);raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join(format!("kiro-{name}-settings.png")),&rgba,975,975,image::ColorType::Rgba8).unwrap();}raster.free_textures(&root.textures_delta);raster.free_textures(&child.textures_delta);
            }
        }
    }}
}

#[test]
#[ignore = "按需导出 Kiro 常驻栏预览"]
fn kiro_resident_bar_preview(){
    use crate::quota::{Amount,Event,Provider,Snapshot,Window};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_KIRO_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    let mut h=H::new();h.app.settings.pet=false;h.app.settings.show_battery=true;h.quota_accounts(1);
    h.app.quota.config.accounts.push(crate::quota::Account{id:"kiro".into(),provider:Provider::Kiro,alias:"Kiro".into(),source:"kiro.json".into(),slot:None,enabled:true,pinned:true});
    h.app.quota_event(Event{id:"kiro".into(),result:Ok(Snapshot{kiro:kiro_snapshot(904.09,1000.0).kiro,plan:"KIRO PRO".into(),at:chrono::Utc::now().timestamp(),
        windows:vec![Window{label:"月度额度".into(),seconds:None,remaining:90.4,reset:None,amount:Some(Amount::new(904.09,1000.0))}],..Default::default()})});
    let wb=h.app.quota.config.connect_workbuddy("preview".into(),"WorkBuddy".into());
    h.app.quota_event(Event{id:wb,result:crate::quota::parse(Provider::WorkBuddy,&serde_json::json!({"code":0,"data":{"Packages":[{"CycleRemainCapacity":"1970"}],"CreditGroups":[{"Kind":"plan_base","CycleRemainCapacity":"1970"}]}}))});
    let mut raster=crate::preview_raster::Raster::new(1280,380);
    for frame in 0..90{
        let t=frame as f64/60.0;let env=Env{t,clock:"12:17:00".into(),battery:Some(80),charging:true,..Default::default()};
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(640.0,190.0))),time:Some(t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{h.app.frame(ui,ui.max_rect(),&env);});});
        raster.update_textures(&output.textures_delta);
        if frame==89{
            raster.buf.fill([0.09,0.11,0.18,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            image::save_buffer(dest.join("kiro-bar.png"),&rgba,1280,380,image::ColorType::Rgba8).unwrap();
        }
    }
}

#[test]
#[ignore="按需导出 Kiro 模拟数据与连接状态界面，不读取真实账号"]
fn kiro_simulated_states_gallery(){
    use crate::quota::{Account,Event,Provider};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_KIRO_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    let at=chrono::DateTime::parse_from_rfc3339("2026-11-11T01:41:00Z").unwrap().timestamp();
    let expires=chrono::DateTime::parse_from_rfc3339("2027-11-01T00:00:00Z").unwrap().timestamp();
    for(skin,theme)in [(crate::model::Skin::Classic,"dark"),(crate::model::Skin::GlassLight,"light")]{
        for scene in ["monthly","pools","overage","small","power","loading","expired","stale","details","connecting","cancelled","identity"]{
            let mut h=H::new();h.app.settings.skin=skin;h.app.settings.pet=false;
            let mut value=serde_json::json!({"userInfo":{"userId":"simulation-user-a"},"subscriptionInfo":{"subscriptionTitle":"KIRO PRO"},"usageBreakdownList":[{"resourceType":"CREDIT","usageLimitWithPrecision":1000.0,"currentUsageWithPrecision":95.91,"nextDateReset":1796083200_i64}]});
            if matches!(scene,"pools"|"details"){
                value["usageBreakdownList"][0]["currentUsageWithPrecision"]=serde_json::json!(1000.0);
                value["usageBreakdownList"][0]["freeTrialInfo"]=serde_json::json!({"freeTrialStatus":"ACTIVE","usageLimit":500,"currentUsage":100,"freeTrialExpiry":expires});
                value["usageBreakdownList"][0]["bonuses"]=serde_json::json!([{"status":"ACTIVE","displayName":"加购点数","usageLimit":125,"currentUsage":25,"expiresAt":expires}]);
            }else if scene=="overage"{
                value["usageBreakdownList"][0]["currentUsageWithPrecision"]=serde_json::json!(1080.0);
                value["usageBreakdownList"][0]["currentOveragesWithPrecision"]=serde_json::json!(80.0);
                value["usageBreakdownList"][0]["overageCapWithPrecision"]=serde_json::json!(200.0);
                value["overageConfiguration"]=serde_json::json!({"overageStatus":"ENABLED"});
            }else if scene=="small"{value["usageBreakdownList"][0]["currentUsageWithPrecision"]=serde_json::json!(999.99);}
            else if scene=="power"{value["subscriptionInfo"]["subscriptionTitle"]=serde_json::json!("KIRO POWER");value["usageBreakdownList"][0]["usageLimitWithPrecision"]=serde_json::json!(10000.0);value["usageBreakdownList"][0]["currentUsageWithPrecision"]=serde_json::json!(0.0);}
            let mut snapshot=crate::quota::parse(Provider::Kiro,&value).unwrap();snapshot.at=at;
            let id="kiro-simulation".to_owned();let owner=snapshot.kiro.as_ref().unwrap().owner.clone();
            h.app.quota.config.accounts.push(Account{id:id.clone(),provider:Provider::Kiro,alias:"Kiro".into(),source:"simulation-kiro.json".into(),slot:Some(owner),enabled:true,pinned:true});h.app.quota.config.selected=id.clone();
            if !matches!(scene,"loading"|"expired"|"connecting"|"cancelled"){
                h.app.quota_event(Event{id:id.clone(),result:Ok(snapshot)});
            }
            match scene{
                "loading"=>{h.app.quota.busy.insert(id.clone());},
                "expired"=>h.app.quota_event(Event{id:id.clone(),result:Err("Kiro 登录已过期，请打开 Kiro 重新登录后刷新额度".into())}),
                "stale"=>{h.app.quota_event(Event{id:id.clone(),result:Err("无法连接 Kiro 额度服务，请检查网络或代理".into())});assert_eq!(h.app.quota.rows[&id].as_ref().unwrap().at,at);assert!(h.app.quota.stale.contains_key(&id));},
                "identity"=>{
                    let account=h.app.quota.config.accounts[0].clone();let mut other=value.clone();other["userInfo"]["userId"]=serde_json::json!("simulation-user-b");let other=crate::quota::parse(Provider::Kiro,&other).unwrap();
                    let error=crate::quota::kiro::check_owner(&account,&other).unwrap_err();h.app.quota_event(Event{id:id.clone(),result:Err(error)});
                    assert_eq!(h.app.quota.rows[&id].as_ref().unwrap().kiro.as_ref().unwrap().owner,account.slot.unwrap());
                },
                _=>{}
            }
            let settings=matches!(scene,"details"|"connecting"|"cancelled"|"identity");
            if settings{
                h.app.settings.settings_theme=if theme=="light"{crate::settings_theme::SettingsTheme::Light}else{crate::settings_theme::SettingsTheme::Dark};h.app.settings_tab=11;h.app.open_settings();h.ctx.set_embed_viewports(false);
                let _pending=if matches!(scene,"connecting"|"cancelled"){
                    h.app.quota.config.accounts.clear();h.app.quota.provider=Provider::Kiro;h.app.quota.alias="个人账号".into();h.app.quota.connect_open=true;
                    let(mut connect,tx)=crate::quota::kiro::Connect::pending(None);if scene=="cancelled"{connect.cancel();}h.app.quota.kiro_connecting=Some(connect);Some(tx)
                }else{None};
                let size=egui::vec2(690.0,620.0);let ppp=1.5;let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
                egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{let id=viewport.ids.this;let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};*capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));});
                let mut raster=crate::preview_raster::Raster::new(1035,930);
                for frame in 0..8{
                    let root=h.ctx.run(egui::RawInput::default(),|ctx|h.app.settings_ui(ctx,false));raster.update_textures(&root.textures_delta);let child=output.lock().unwrap().take().unwrap();raster.update_textures(&child.textures_delta);
                    if frame==7{
                        if scene=="identity"{let count=child.shapes.iter().filter_map(|s|match &s.shape{egui::Shape::Text(t)=>Some(t.galley.text().matches("月度额度").count()),_=>None}).sum::<usize>();assert_eq!(count,1,"普通套餐只显示一行月度额度");}
                        raster.buf.fill([0.20,0.24,0.32,1.0]);raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join(format!("{theme}-{scene}.png")),&rgba,1035,930,image::ColorType::Rgba8).unwrap();}
                    raster.free_textures(&root.textures_delta);raster.free_textures(&child.textures_delta);
                }
            }else{
                h.app.show_page(6);let mut raster=feature_raster();
                for frame in 0..75{
                    h.t+=1.0/60.0;let cursor=Some(H::local(300.0,60.0));
                    let env=Env{t:h.t,clock:"09:41:00".into(),today:"2026-11-11".into(),date:"11月11日 周三".into(),cursor,bt_ready:true,..Default::default()};
                    let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(h.t),events:vec![egui::Event::PointerMoved(cursor.unwrap())],viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
                    let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,WIN_H)),&env);});});raster.update_textures(&output.textures_delta);
                    if frame==74{
                        let texts=output.shapes.iter().filter_map(|s|match &s.shape{egui::Shape::Text(t)=>Some(t.galley.text()),_=>None}).collect::<Vec<_>>().join(" ");
                        if scene=="small"{assert!(texts.contains("0.01"));}if scene=="loading"{assert!(texts.contains("正在更新"));}if scene=="expired"{assert!(texts.contains("登录已过期"));}
                        raster.buf.fill([0.20,0.24,0.32,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                        let image=image::RgbaImage::from_raw(raster.w as u32,raster.h as u32,rgba).unwrap();image::imageops::crop_imm(&image,200,0,880,380).to_image().save(dest.join(format!("{theme}-{scene}.png"))).unwrap();
                    }raster.free_textures(&output.textures_delta);
                }
            }
        }
    }
}

#[test]
#[ignore="按需导出 Kiro 常驻刘海岛和胶囊岛，不读取真实账号"]
fn kiro_resident_states_gallery(){
    use crate::quota::{Account,Event,Provider};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_KIRO_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for(skin,theme)in [(crate::model::Skin::Classic,"dark"),(crate::model::Skin::GlassLight,"light")]{for mixed in [false,true]{
        let mut h=H::new();h.app.settings.skin=skin;h.app.settings.pet=false;h.app.settings.show_battery=true;
        h.app.settings.island_shape=if mixed{crate::model::IslandShape::Pill}else{crate::model::IslandShape::Notch};
        if mixed{h.quota_accounts(1);}
        let id="kiro-resident".to_owned();let snapshot=kiro_snapshot(904.09,1000.0);
        h.app.quota.config.accounts.push(Account{id:id.clone(),provider:Provider::Kiro,alias:"Kiro".into(),source:"simulation-kiro.json".into(),slot:Some(snapshot.kiro.as_ref().unwrap().owner.clone()),enabled:true,pinned:true});
        h.app.quota_event(Event{id:id.clone(),result:Ok(snapshot)});
        if mixed{let wb=h.app.quota.config.connect_workbuddy("preview".into(),"WorkBuddy".into());h.app.quota_event(Event{id:wb,result:crate::quota::parse(Provider::WorkBuddy,&serde_json::json!({"code":0,"data":{"Packages":[{"CycleRemainCapacity":"1970"}],"CreditGroups":[{"Kind":"plan_base","CycleRemainCapacity":"1970"}]}}))});}
        h.app.quota.config.selected=id.clone();let mut raster=crate::preview_raster::Raster::new(1280,380);
        for frame in 0..90{
            let t=frame as f64/60.0;let env=Env{t,clock:"12:17:00".into(),today:"2026-11-11".into(),date:"11月11日 周三".into(),battery:Some(80),charging:true,..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(640.0,190.0))),time:Some(t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
            let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{h.app.frame(ui,ui.max_rect(),&env);});});raster.update_textures(&output.textures_delta);
            if frame==89{
                assert_eq!(h.app.quota_bar_indices().len(),if mixed{3}else{1});
                assert!(output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="904")),"常驻栏应显示剩余点数");
                raster.buf.fill([0.20,0.24,0.32,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                let image=image::RgbaImage::from_raw(1280,380,rgba).unwrap();let width=(h.app.current_width()*2.0+72.0).ceil().clamp(300.0,1280.0) as u32;
                image::imageops::crop_imm(&image,(1280-width)/2,0,width,130).to_image().save(dest.join(format!("{theme}-{}.png",if mixed{"residentmixed"}else{"resident"}))).unwrap();
            }raster.free_textures(&output.textures_delta);
        }
    }}
}

#[test]
fn quota_update_status_is_visible_for_every_provider_and_keeps_failed_data(){
    use crate::quota::{Provider,Event,Snapshot,Window};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let at=chrono::Utc::now().timestamp()-60;
    let time=chrono::DateTime::from_timestamp(at,0).unwrap().with_timezone(&chrono::Local).format("%H:%M").to_string();
    for provider in Provider::ALL{
        let mut h=H::new();h.quota_accounts(1);h.app.quota.config.accounts[0].provider=provider;
        let mut snapshot=match provider{
            Provider::Kiro=>kiro_snapshot(720.0,1000.0),
            Provider::DeepSeek=>crate::quota::parse(provider,&serde_json::json!({"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"4.18","granted_balance":"0","topped_up_balance":"4.18"}]})).unwrap(),
            Provider::WorkBuddy=>crate::quota::parse(provider,&serde_json::json!({"code":0,"data":{"Packages":[{"CycleRemainCapacity":"2000"}]}})).unwrap(),
            _=>Snapshot{windows:vec![Window{label:"5 小时".into(),seconds:Some(18000),remaining:72.0,reset:None,amount:None},Window{label:"本周".into(),seconds:Some(604800),remaining:44.0,reset:None,amount:None}],..Default::default()},
        };
        snapshot.at=at;h.app.quota_event(Event{id:"preview-0".into(),result:Ok(snapshot)});
        h.app.show_page(6);
        if let Some(dest)=std::env::var_os("DAOZAI_QUOTA_STATUS_PREVIEW_DIR"){
            let dest=std::path::PathBuf::from(dest);std::fs::create_dir_all(&dest).unwrap();let mut raster=feature_raster();
            shoot_feature(&mut h,&mut raster,&dest,&format!("quota-{}",provider.name()),90,Some(H::local(300.0,60.0)),"2026-11-07");
        }
        h.hover(H::local(300.0,60.0),1.0);
        assert_eq!(h.app.expanded_height(),176.0,"{provider:?} 额度页应采用同宽加高的布局");
        assert!((h.app.current_width()-PW).abs()<0.1,"{provider:?} 额度页应保持原有宽度：{}",h.app.current_width());
        let expected=if provider==Provider::Claude{format!("本地记录 · {time}")}else if provider==Provider::DeepSeek{format!("CNY · {time} 更新")}else{format!("{time} 更新")};
        let status=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==expected=>Some(t.visual_bounding_rect()),_=>None}).unwrap_or_else(||panic!("{provider:?} 缺少更新时间：{expected}"));
        assert!(status.bottom()+6.0<=H::local(0.0,h.app.expanded_height()-9.2).y,"{provider:?} 更新时间 {status:?} 应与分页圆点留出间距");
        for shape in &h.shapes{if let egui::Shape::Text(text)=&shape.shape{
            if !text.galley.text().is_empty()&&text.galley.text()!=expected&&text.visual_bounding_rect().left()>=H::local(125.0,0.0).x{
                assert!(shape.clip_rect.contains(text.visual_bounding_rect().center()),"{provider:?} 的 {} 应完整显示在面板内",text.galley.text());
                assert!(status.top()-text.visual_bounding_rect().bottom()>=8.0,"{provider:?} 的更新时间 {status:?} 应与 {} {:?} 留出独立行距",text.galley.text(),text.visual_bounding_rect());
            }
        }}
        h.app.quota.busy.insert("preview-0".into());h.frame(vec![]);
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains(&time)&&t.galley.text().contains(if provider==Provider::Claude{"读取中"}else{"正在更新"}))),"刷新中仍须保留已有记录时间");
        h.app.quota_event(Event{id:"preview-0".into(),result:Err("连接超时".into())});h.frame(vec![]);
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==format!("上次记录 · {time}"))),"{provider:?} 刷新失败后应明确旧数据时间");
        assert_eq!(h.app.quota.rows["preview-0"].as_ref().unwrap().at,at,"刷新失败不能刷新记录时间");
    }
}

#[test]
fn quota_page_navigation_tracks_the_taller_panel(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.quota_accounts(1);
    h.app.show_page(6);h.hover(H::local(300.0,60.0),0.8);
    assert!((h.hit.height()-178.0).abs()<0.2,"额度页新增的底部空间应属于命中区域");
    let dot=h.shapes.iter().find_map(|shape|match &shape.shape{
        egui::Shape::Rect(rect) if (rect.rect.width()-4.0).abs()<0.1&&(rect.rect.height()-4.0).abs()<0.1&&(rect.rect.center().y-168.8).abs()<0.1=>Some(rect.rect.center()),_=>None,
    }).expect("分页圆点应随加高的面板下移");
    h.click(dot);assert_ne!(h.app.page,6,"新增底部区域中的分页应可点击");
    h.app.show_page(6);h.hover(H::local(300.0,60.0),0.8);
    h.click(H::local(21.0,88.0));assert_eq!(h.app.page,5);
    h.hover(H::local(300.0,60.0),0.8);assert_eq!(h.app.expanded_height(),PH);
    h.click(H::local(398.0,68.0));assert_eq!(h.app.page,6);
    h.hover(H::local(300.0,60.0),0.8);
    h.click(H::local(398.0,88.0));assert_eq!(h.app.page,crate::app::CALENDAR_PAGE);
}

#[test]
fn quota_update_status_marks_previous_dates_and_never_shows_epoch_time(){
    use crate::quota::{Event,Snapshot,Window};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.quota_accounts(1);
    let at=chrono::Local::now()-chrono::Duration::days(1);
    let snapshot=Snapshot{at:at.timestamp(),windows:vec![Window{label:"本周".into(),seconds:Some(604800),remaining:44.0,reset:None,amount:None}],..Default::default()};
    h.app.quota_event(Event{id:"preview-0".into(),result:Ok(snapshot.clone())});h.app.show_page(6);h.hover(H::local(300.0,60.0),0.6);
    let expected=format!("{} 更新",at.format("%m/%d %H:%M"));
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==expected)),"跨天记录应包含日期");
    h.app.quota_event(Event{id:"preview-0".into(),result:Ok(Snapshot{at:0,..snapshot})});h.frame(vec![]);
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains("1970")||t.galley.text().ends_with(" 更新"))),"缺少时间时不能伪造更新时间");
}

#[test]
fn claude_local_quota_shows_the_record_time_and_does_not_invent_resets(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.quota_accounts(1);h.app.quota.config.accounts[0].provider=crate::quota::Provider::Claude;
    let at=chrono::Utc::now().timestamp()-60;h.app.quota_event(crate::quota::Event{id:"preview-0".into(),result:Ok(crate::quota::Snapshot{at,windows:vec![
        crate::quota::Window{label:"5 小时".into(),seconds:Some(18000),remaining:89.0,reset:None,amount:None},crate::quota::Window{label:"本周".into(),seconds:Some(604800),remaining:24.0,reset:None,amount:None}],..Default::default()})});
    h.app.show_page(6);let mut raster=feature_raster();let mut shapes=Vec::new();
    for _ in 0..90{
        h.t+=1.0/60.0;let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(h.t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
        let env=Env{t:h.t,clock:"20:30:00".into(),date:"11月6日 周五".into(),today:"2026-11-06".into(),cursor:Some(Pos2::new(WIN_W/2.0,12.0)),..Default::default()};
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,WIN_H)),&env);});});raster.update_textures(&output.textures_delta);shapes=output.shapes;raster.free_textures(&output.textures_delta);
    }
    let texts:Vec<_>=shapes.iter().filter_map(|s|match &s.shape{egui::Shape::Text(t)=>Some(t),_=>None}).collect();
    let label=texts.iter().find(|t|t.galley.text()=="Claude · 本地记录").expect("应明确标注本地来源");
    let time=chrono::DateTime::from_timestamp(at,0).unwrap().with_timezone(&chrono::Local).format("%H:%M").to_string();
    let timestamp=texts.iter().find(|t|t.galley.text()==format!("本地记录 · {time}")).expect("应显示官方缓存的实际记录时间");assert!(!label.visual_bounding_rect().intersects(timestamp.visual_bounding_rect()));assert_eq!(texts.iter().filter(|t|t.galley.text()=="重置时间未知").count(),2);
    if let Some(path)=std::env::var_os("DAOZAI_CLAUDE_LOCAL_PREVIEW"){
        raster.buf.fill([0.15,0.15,0.15,1.0]);raster.draw(&h.ctx.tessellate(shapes,2.0),2.0);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);let image=image::RgbaImage::from_raw((crate::app::CANVAS_W*2.0) as u32,(WIN_H*2.0) as u32,rgba).unwrap();image::imageops::crop_imm(&image,200,0,880,350).to_image().save(path).unwrap();
    }
}

#[test]
fn quota_accounts_switch_independently_and_hide_with_the_page() {
    use crate::quota::{Account,Provider};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    for id in ["personal","work"] {h.app.quota.config.accounts.push(Account{id:id.into(),provider:Provider::Codex,alias:id.into(),source:format!("{id}.json").into(),slot:None,enabled:true,pinned:true});}
    h.app.quota.config.selected="personal".into();
    h.app.quota_event(crate::quota::Event{id:"personal".into(),result:crate::quota::parse(Provider::Codex,&serde_json::json!({"rate_limit":{"secondary_window":{"used_percent":20,"limit_window_seconds":604800}}}))});
    h.app.quota_event(crate::quota::Event{id:"work".into(),result:Err("登录已失效".into())});
    assert_eq!(h.app.quota.rows["personal"].as_ref().unwrap().windows[0].remaining,80.0);
    assert!(h.app.quota.rows["work"].is_err());assert_eq!(h.app.quota_bar_indices().len(),2);
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.app.show_page(6);h.hover(H::local(300.0,60.0),0.7);
    h.click(H::local(202.0,39.0));assert_eq!(h.app.quota.config.selected,"work");
    assert_eq!(crate::quota::Config::load().selected,"work");
    h.app.set_page_visible(6,false);assert!(h.app.quota_bar_indices().is_empty());assert_ne!(h.app.page,6);
    h.app.quota.config.accounts.retain(|a|a.id!="work");h.app.quota_event(crate::quota::Event{id:"work".into(),result:Ok(Default::default())});
    assert!(h.app.quota.rows["work"].is_err(),"移除账号后不得接收晚到的额度数据");
}

#[test]
fn page_visibility_is_saved_and_sets_the_starting_page() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    assert_eq!(h.app.settings.visible_pages,[true;9]);
    assert!(h.app.settings.show_battery);
    h.app.settings.show_battery=false;
    h.app.set_page_visible(0,false);
    h.app.set_page_visible(3,false);
    h.app.settings.save();
    let settings=crate::model::Settings::load();
    assert!(!settings.show_battery);
    assert_eq!(settings.visible_pages,[false,true,true,false,true,true,true,true,true]);
    let restarted=App::new(&h.ctx,Options{fetch_lyrics:false,pet_idle:1e9});
    assert_eq!(restarted.page(),1);
    assert_eq!(restarted.settings.visible_pages,settings.visible_pages);
    assert!(!restarted.settings.show_battery);
    std::fs::write(crate::model::config_dir().join("settings.txt"),"visible_pages=000000\n").unwrap();
    assert_eq!(crate::model::Settings::load().visible_pages,[true,false,false,false,false,false,false,false,false]);
    std::fs::write(crate::model::config_dir().join("settings.txt"),"visible_pages=1000000\n").unwrap();
    assert_eq!(crate::model::Settings::load().visible_pages,[true,false,false,false,false,false,false,true,true],"旧版页面开关应保留，并开启新增的日程页与天气页");
    std::fs::write(crate::model::config_dir().join("settings.txt"),"visible_pages=invalid\n").unwrap();
    assert_eq!(crate::model::Settings::load().visible_pages,[true;9]);
}

#[test]
fn battery_module_hides_power_in_collapsed_and_expanded_headers() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for expanded in [false,true] {
        let mut h=H::new();
        h.app.settings.pet=false;
        h.app.settings.show_battery=false;
        if expanded {h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);} else {h.frame(vec![]);}
        let baseline=h.shapes.clone();
        h.power=(Some(80),true);
        h.t-=1.0/60.0;h.frame(vec![]);
        assert!(h.shapes==baseline,"关闭电量后，本机电池状态不应影响界面布局");
        h.app.settings.show_battery=true;
        h.t-=1.0/60.0;h.frame(vec![]);
        assert!(h.shapes!=baseline,"开启后应显示电量");
        h.power=(None,true);
        h.t-=1.0/60.0;h.frame(vec![]);
        assert!(h.shapes==baseline,"没有电池的设备应自动隐藏电量并释放占位");
    }
}

#[test]
fn battery_module_can_disable_only_the_power_warning() {
    use crate::extras::PetState;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.show_battery=false;
    h.power=(Some(10),false);h.frame(vec![]);
    assert_eq!(h.app.pet_state(),PetState::Awake);
    h.network=Some(false);h.frame(vec![]);
    assert_eq!(h.app.pet_state(),PetState::Worried);
}

#[test]
fn page_visibility_skips_hidden_pages_in_every_navigation_control() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    for page in [1,3,4,6,7,8] {h.app.set_page_visible(page,false);}
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    for expected in [2,5,0] {
        h.click(H::local(398.0,68.0));
        assert_eq!(h.app.page,expected);
        h.hover(H::local(300.0,60.0),0.7);
    }
    h.click(H::local(PW/2.0+6.0,132.8));
    assert_eq!(h.app.page,2,"底部圆点应仅包含启用的页面");
    h.hover(H::local(300.0,60.0),0.7);
    h.frame(vec![egui::Event::Key{key:egui::Key::ArrowRight,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
    assert_eq!(h.app.page,5);
    h.frame(vec![egui::Event::Key{key:egui::Key::ArrowRight,physical_key:None,pressed:false,repeat:false,modifiers:Default::default()}]);
    h.hover(H::local(300.0,60.0),0.7);
    h.frame(vec![egui::Event::MouseWheel{unit:egui::MouseWheelUnit::Point,delta:Vec2::new(0.0,40.0),modifiers:Default::default()}]);
    assert_eq!(h.app.page,2,"滚轮应跳过隐藏页面");
    h.click(H::local(21.0,68.0));
    assert_eq!(h.app.page,0);
}

#[test]
fn page_visibility_moves_away_from_hidden_page_and_keeps_one_page() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.go(4,1.0);
    h.app.adding=true;
    h.app.set_page_visible(4,false);
    assert_eq!(h.app.page,0);
    assert!(!h.app.is_editing());
    for page in [0,1,3,5,6,7,8] {h.app.set_page_visible(page,false);}
    assert_eq!(h.app.page,2);
    h.app.set_page_visible(2,false);
    assert!(h.app.settings.visible_pages[2],"不能关闭最后一个页面");
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    h.click(H::local(398.0,68.0));
    h.click(H::local(21.0,68.0));
    assert_eq!(h.app.page,2);
    h.app.set_page_visible(5,true);
    h.hover(H::local(300.0,60.0),0.7);
    h.click(H::local(398.0,68.0));
    assert_eq!(h.app.page,5,"重新开启的页面应恢复导航");
}

#[test]
fn page_visibility_does_not_reopen_hidden_pages_through_shortcuts_or_drops() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.set_page_visible(5,false);
    h.app.show_page(5);
    assert_eq!(h.app.page,0);
    assert!(h.app.settings_open);
    assert_eq!(h.app.settings_tab,7,"隐藏页面的入口应进入对应设置");
    h.app.set_page_visible(3,false);
    let file=crate::model::config_dir().join("shelf-hidden.txt");
    std::fs::create_dir_all(crate::model::config_dir()).unwrap();
    std::fs::write(&file,"test").unwrap();
    h.app.on_drop(vec![file]);
    assert_eq!(h.app.page,0);
    assert!(h.app.shelf.items.is_empty());
}

#[test]
fn alphabetic_mail_codes_reach_the_card_without_changing_case() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for (kind,value) in ["text/plain","text/html"].into_iter().flat_map(|kind|["AbCdEf","WWWWWWWW"].map(|value|(kind,value))) {
        let mut h=H::new();
        h.app.mail.accounts=vec![crate::mail::Account{custom:None,email:"test@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
        let body=if kind=="text/html"{format!("<p>Your verification code:</p><p><b>{value}</b></p>")}else{format!("Your verification code:\n{value}")};
        let raw=format!("From: Example <security@example.invalid>\r\nSubject: Your verification code\r\nContent-Type: {kind}; charset=utf-8\r\n\r\n{body}");
        let parsed=crate::mail::parse_message(raw.as_bytes());
        let code=crate::mail::code_of(&parsed);
        assert_eq!(code.as_deref(),Some(value));
        h.app.mail_event(crate::mail::Event::Mail{new:true,item:crate::mail::Item{html:String::new().into(),received_unix:None,account:"test@qq.com".into(),from:"Example".into(),subject:parsed.subject.clone(),time:"09:41".into(),code,summary:crate::mail::body_of(&parsed).into(),links:vec![],meeting:None,uid:43}});
        h.app.show_page(5);h.hover(H::local(300.0,70.0),1.0);
        let text=h.shapes.iter().find_map(|s|match &s.shape {egui::Shape::Text(t) if t.galley.text()==value=>Some(t),_=>None}).expect("字母验证码应出现在大卡上");
        assert!(text.galley.size().x<=120.5,"最长字母码不能挤入邮件列表：{}",text.galley.size().x);
        if value=="AbCdEf"{assert!(text.galley.job.sections.iter().any(|s|s.format.font_id.size>=23.0));}
        h.cmds.clear();h.click(H::local(60.0,70.0));
        assert!(h.cmds.iter().any(|c|matches!(c,crate::app::Cmd::Copy(copied) if copied==value)));
        h.t+=61.0;h.frame(vec![]);
        assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()==value)));
    }
}

#[test]
fn regression_microsoft_traditional_mail_creates_code_card() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for kind in ["text/plain","text/html"] {
        let mut h=H::new();
        h.app.mail.accounts=vec![crate::mail::Account{custom:None,email:"test@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
        let body="我們收到您要申請一次性驗證碼來與 Microsoft 帳戶搭配使用。\n您的一次性驗證碼為：431872\n僅在官方網站或應用程式上輸入此代碼。不要與任何人分享。";
        let body=if kind=="text/html" {format!("<p>{}</p>",body.replace('\n',"</p><p>"))}else{body.into()};
        let raw=format!("From: Microsoft <account-security@example.invalid>\r\nSubject: 您的一次性驗證碼\r\nMIME-Version: 1.0\r\nContent-Type: {kind}; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}");
        let parsed=crate::mail::parse_message(raw.as_bytes());
        let code=crate::mail::code_of(&parsed);
        assert_eq!(code.as_deref(),Some("431872"),"{kind} 邮件应提取出验证码");
        h.app.mail_event(crate::mail::Event::Mail{new:true,item:crate::mail::Item{html:String::new().into(),received_unix:None,account:"test@qq.com".into(),from:parsed.from.clone(),subject:parsed.subject.clone(),time:"08:59".into(),code,summary:crate::mail::body_of(&parsed).into(),links:vec![],meeting:None,uid:42}});
        h.app.show_page(5);h.hover(H::local(300.0,70.0),1.0);
        assert!(h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(t) if t.galley.text()=="431872"&&t.galley.job.sections.iter().any(|s|s.format.font_id.size>=23.0))),"新收到的繁体验证码应出现在大卡里");
        h.cmds.clear();h.click(H::local(60.0,70.0));
        assert!(h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Copy(value) if value=="431872")),"大卡应支持点击复制");
        h.t+=61.0;h.frame(vec![]);
        assert!(!h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(t) if t.galley.text()=="431872")),"繁体验证码同样应按隐私设置自动隐藏");
    }
}

#[test]
fn mail_page_shows_code_and_copies_on_click() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = H::new();
    h.app.mail.accounts = vec![crate::mail::Account {
        custom: None, email: "a@qq.com".into(),
        provider: crate::mail::Provider::Qq,
        enabled: true,
    }];
    h.app.mail_event(crate::mail::Event::Mail {
        item: crate::mail::Item {
            received_unix: None, account: "a@qq.com".into(),
            from: "GitHub".into(),
            subject: "verify".into(),
            time: "09:41".into(),
            code: Some("482913".into()),
            html: String::new().into(), summary: "Enter 482913 to finish signing in.".into(),
            links: vec![],
            meeting: None,
            uid: 42,
        },
        new: true,
    });
    assert!(h.app.notification_bell_until > 0.0, "新邮件应点亮铃铛");
    assert!(h.app.alerts.iter().any(|a|a.title=="新邮件"&&a.page==5),"新邮件应进入通知队列，不能只有收起时才可见的铃铛");
    h.app.show_page(5);
    h.hover(Pos2::new(WIN_W / 2.0, 12.0), 0.6);
    h.hover(H::local(300.0, 60.0), 0.4);
    h.click(H::local(60.0, 70.0));
    assert!(
        h.cmds.iter().any(|c| matches!(c, crate::app::Cmd::Copy(t) if t == "482913")),
        "点验证码应复制: {:?}",
        h.cmds
    );
}

#[test]
fn mail_row_opens_summary_and_back_returns() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = H::new();
    h.app.mail.accounts = vec![crate::mail::Account {
        custom: None, email: "a@qq.com".into(),
        provider: crate::mail::Provider::Qq,
        enabled: true,
    }];
    h.app.mail_event(crate::mail::Event::Mail {
        item: crate::mail::Item {
            received_unix: None, account: "a@qq.com".into(),
            from: "GitHub".into(),
            subject: "verify".into(),
            time: "09:41".into(),
            code: Some("482913".into()),
            html: String::new().into(), summary: "Enter 482913 to finish signing in.".into(),
            links: vec![],
            meeting: None,
            uid: 42,
        },
        new: true,
    });
    h.app.show_page(5);
    h.hover(Pos2::new(WIN_W / 2.0, 12.0), 0.6);
    h.hover(H::local(300.0, 50.0), 0.4);
    h.click(H::local(300.0, 50.0));
    assert_eq!(h.app.mail.open_uid, Some(("a@qq.com".to_string(), 42)), "点邮件行应展开摘要");
    h.hover(H::local(350.0, 48.0), 0.4);
    h.click(H::local(350.0, 48.0));
    assert_eq!(h.app.mail.open_uid, None, "点返回应回到列表");
}

#[test]
fn empty_todo_draft_can_be_cancelled_by_clicking_elsewhere(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for text in ["","   "]{
        let mut h=H::new();let mut r=feature_raster();h.app.todos.items=vec![crate::model::TodoItem{text:"保留任务".into(),due:None,done_on:None,reminded:false,..Default::default()}];h.app.todos.save();
        h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click(H::local(70.0,109.0));
        assert!(h.app.adding,"点击添加后应保留空白输入框");
        if !text.is_empty(){h.frame(vec![egui::Event::Text(text.into())]);}
        h.cmds.clear();h.click(H::local(65.0,60.0));
        assert!(!h.app.is_editing(),"空白草稿点击岛内其他位置应退出");
        assert!(h.app.buf.is_empty());assert_eq!(h.app.todos.items.len(),1);assert_eq!(h.app.todos.items[0].text,"保留任务");
        assert_eq!(crate::model::TodoList::load().items.len(),1,"不能保存空白待办");
        h.hover(H::local(65.0,60.0),0.3);
        assert!(!h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Focus)),"取消后不能反复激活窗口");
    }
}

#[test]
fn todo_ime_first_backspace_keeps_committed_text(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click(H::local(70.0,109.0));
    h.frame(vec![egui::Event::Text("保留文字".into())]);
    h.frame(vec![egui::Event::Key{key:egui::Key::Backspace,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()},egui::Event::Ime(egui::ImeEvent::Enabled)]);
    assert_eq!(h.app.buf,"保留文字","开始组合输入的同一帧，退格只能交给输入法");
    h.frame(vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
    h.frame(vec![egui::Event::Key{key:egui::Key::Backspace,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
    assert_eq!(h.app.buf,"保留文","结束组合输入后普通退格仍应正常删除");
}

#[test]
fn todo_ime_focus_change_preserves_empty_draft(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click(H::local(70.0,109.0));h.cmds.clear();
    h.frame(vec![egui::Event::Ime(egui::ImeEvent::Enabled),egui::Event::WindowFocused(false)]);
    assert!(h.app.adding,"输入法候选窗口造成的失焦不能取消空白待办");
    assert!(h.app.open);
    assert!(!h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Focus)),"不能和输入法窗口反复争抢焦点");
    h.app.foreground_changed();h.frame(vec![]);
    assert!(h.app.adding,"输入法组合输入期间的前台切换应保留草稿");
    h.frame(vec![egui::Event::WindowFocused(true),egui::Event::Ime(egui::ImeEvent::Preedit("zhong".into()))]);
    h.frame(vec![egui::Event::Ime(egui::ImeEvent::Commit("中文".into())),egui::Event::Ime(egui::ImeEvent::Disabled)]);
    assert_eq!(h.app.buf,"中文");assert!(h.app.todos.items.is_empty());
}

#[test]
fn todo_ime_confirmation_does_not_submit_or_cancel_the_draft(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for key in [egui::Key::Enter,egui::Key::Escape]{
        let mut h=H::new();h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click(H::local(70.0,109.0));
        h.frame(vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
        h.frame(vec![egui::Event::Key{key,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()},egui::Event::Ime(egui::ImeEvent::Commit("中文".into())),egui::Event::Ime(egui::ImeEvent::Disabled)]);
        assert!(h.app.adding,"输入法确认或取消候选不能同时结束待办编辑");
        assert_eq!(h.app.buf,"中文","输入法确认后应保留正文，按键 {key:?}");assert!(h.app.todos.items.is_empty());
        h.frame(vec![egui::Event::Key{key,physical_key:None,pressed:false,repeat:false,modifiers:Default::default()}]);
        h.frame(vec![egui::Event::Key{key:egui::Key::Enter,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
        assert_eq!(h.app.todos.items.len(),1,"退出组合输入后回车应正常保存");assert_eq!(h.app.todos.items[0].text,"中文");
    }
}

#[test]
fn todo_ime_candidate_closing_does_not_discard_the_empty_draft(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click(H::local(70.0,109.0));
    h.frame(vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
    h.frame(vec![egui::Event::Ime(egui::ImeEvent::Disabled),egui::Event::WindowFocused(false)]);
    assert!(h.app.adding,"关闭输入法候选窗口的同一帧不能取消空白草稿");
    h.frame(vec![egui::Event::WindowFocused(true)]);h.app.foreground_changed();h.frame(vec![]);
    assert!(!h.app.adding,"之后真正切换应用时仍应取消空白草稿");
}

#[test]
fn todo_ime_event_variants_preserve_mixed_text_and_resume_plain_input(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let cases=vec![
        ("无显式开始事件",vec![vec![egui::ImeEvent::Preedit("ni".into())],vec![egui::ImeEvent::Commit("你".into()),egui::ImeEvent::Disabled]],"你"),
        ("一次提交多个字符",vec![vec![egui::ImeEvent::Enabled,egui::ImeEvent::Preedit("にほん".into())],vec![egui::ImeEvent::Commit("日本".into()),egui::ImeEvent::Disabled]],"日本"),
        ("同帧提交后继续组合",vec![vec![egui::ImeEvent::Enabled,egui::ImeEvent::Preedit("하".into())],vec![egui::ImeEvent::Commit("한".into()),egui::ImeEvent::Disabled,egui::ImeEvent::Enabled,egui::ImeEvent::Preedit("그".into())],vec![egui::ImeEvent::Commit("글".into()),egui::ImeEvent::Disabled]],"한글"),
        ("取消候选",vec![vec![egui::ImeEvent::Enabled,egui::ImeEvent::Preedit("pin".into())],vec![egui::ImeEvent::Preedit(String::new()),egui::ImeEvent::Disabled]],""),
    ];
    for (name,frames,expected) in cases {
        let mut h=H::new();h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click(H::local(70.0,109.0));
        h.frame(vec![egui::Event::Text("中文 ABC ".into())]);
        for events in frames {h.frame(events.into_iter().map(egui::Event::Ime).collect());}
        assert_eq!(h.app.buf,format!("中文 ABC {expected}"),"事件模式：{name}");
        assert!(h.app.adding);assert!(h.app.todos.items.is_empty());
        h.frame(vec![egui::Event::Text("x".into())]);
        h.frame(vec![egui::Event::Key{key:egui::Key::Backspace,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
        assert_eq!(h.app.buf,format!("中文 ABC {expected}"),"组合输入结束后应恢复普通退格：{name}");
        h.frame(vec![egui::Event::Key{key:egui::Key::Enter,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
        assert_eq!(h.app.todos.items.len(),1,"组合输入结束后应能保存：{name}");
    }
}

#[test]
fn blank_todo_draft_closes_when_window_focus_is_lost(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click(H::local(70.0,109.0));
    h.cmds.clear();h.frame(vec![egui::Event::WindowFocused(false)]);
    assert!(!h.app.is_editing(),"点击其他软件后空白草稿应自动退出");
    assert!(h.app.todos.items.is_empty());
    assert!(!h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Focus)),"失去焦点时不能把窗口再次激活");
}

#[test]
fn todo_input_requests_window_focus_once_and_preserves_typed_drafts(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.cmds.clear();h.click(H::local(70.0,109.0));
    assert_eq!(h.cmds.iter().filter(|cmd|matches!(cmd,crate::app::Cmd::Focus)).count(),1,"新建时只应请求一次窗口焦点");
    h.click(H::local(280.0,46.0));assert!(h.app.adding,"点击输入框不能取消草稿");
    h.frame(vec![egui::Event::Text("尚未保存的内容".into())]);h.cmds.clear();
    h.click(H::local(65.0,60.0));h.frame(vec![egui::Event::WindowFocused(false)]);
    for _ in 0..20{h.frame(vec![]);}
    assert!(h.app.adding);assert_eq!(h.app.buf,"尚未保存的内容");assert!(h.app.todos.items.is_empty());
    assert!(!h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Focus)),"已有内容失焦后应保留草稿，不能抢回焦点");
}

#[test]
fn switching_apps_cancels_only_empty_new_todos(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for text in ["","   ","保留未提交的内容"]{
        let mut h=H::new();h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click(H::local(70.0,109.0));
        if !text.is_empty(){h.frame(vec![egui::Event::Text(text.into())]);}
        h.cmds.clear();h.app.foreground_changed();h.frame(vec![]);
        assert_eq!(h.app.adding,!text.trim().is_empty());
        assert_eq!(h.app.buf,if text.trim().is_empty(){""}else{text});
        assert!(h.app.todos.items.is_empty());
        assert!(!h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Focus)));
    }
}

#[test]
fn drink_adds_water_and_todo_can_be_added() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = H::new();
    h.hover(Pos2::new(WIN_W / 2.0, 12.0), 1.0);
    h.app.go(2, 1.0);
    h.hover(H::local(300.0, 60.0), 0.8);
    h.click(H::local(70.0, 110.0));
    assert_eq!(h.app.water.drunk_ml, 250);
    assert_eq!(h.app.water.hours[9], 250);

    h.app.go(4, 1.0);
    h.hover(H::local(300.0, 60.0), 0.8);
    h.click(H::local(70.0, 109.0));
    assert!(h.app.adding);
    h.hover(H::local(300.0, 60.0), 0.4);
    h.frame(vec![egui::Event::Text("写周报".into())]);
    h.frame(vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]);
    h.hover(H::local(300.0, 60.0), 0.1);
    assert_eq!(h.app.todos.items.first().map(|t| t.text.as_str()), Some("写周报"));
    assert!(!h.app.adding);
}

#[test]
fn timer_counts_elapsed_time_after_sleep() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = H::new();
    h.frame(vec![]);
    h.app.pomo.running = true;
    h.app.pomo.remaining = 120.0;
    h.t += 60.0;
    h.frame(vec![]);
    assert!(h.app.pomo.remaining < 61.0, "挂起恢复后必须补算实际流逝时间");
}

#[test]
fn shelf_remove_keeps_original_file() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let file=crate::model::config_dir().join("保留文件.txt");std::fs::create_dir_all(file.parent().unwrap()).unwrap();std::fs::write(&file,"内容").unwrap();
    h.app.shelf.add(&file).unwrap();h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.app.go(3,1.0);h.hover(H::local(300.0,60.0),0.8);
    h.click(H::local(233.0,39.0));assert!(h.app.shelf.items.is_empty());assert!(file.exists());assert!(crate::model::Shelf::load().items.is_empty());
}

#[test]
fn shelf_duplicate_drop_reports_existing_files_and_preserves_the_current_page() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let dir=crate::model::config_dir();std::fs::create_dir_all(&dir).unwrap();
    let files:Vec<_>=(0..7).map(|i|{let path=dir.join(format!("文件-{i}.txt"));std::fs::write(&path,"内容").unwrap();path}).collect();
    h.app.on_drop(files.clone());h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.app.shelf_first=0;
    h.app.on_drop(vec![files[0].clone()]);h.hover(H::local(300.0,60.0),0.3);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="该文件已在置物架中")));
    h.app.on_drop(vec![files[0].clone(),files[1].clone()]);h.hover(H::local(300.0,60.0),0.3);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="这些文件已在置物架中")));
    assert_eq!(h.app.shelf_first,0,"重复添加不应将列表跳到最后");
    assert_eq!(h.app.shelf.items,files);assert_eq!(crate::model::Shelf::load().items,files);
}

#[test]
fn shelf_drop_distinguishes_new_duplicate_and_missing_files() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let dir=crate::model::config_dir();std::fs::create_dir_all(&dir).unwrap();
    let old=dir.join("已添加.txt");let new=dir.join("新文件.txt");let missing=dir.join("已移动.txt");
    for file in [&old,&new]{std::fs::write(file,"内容").unwrap();}
    h.app.on_drop(vec![old.clone()]);h.app.on_drop(vec![old.clone(),new.clone(),new.clone(),missing.clone()]);
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="已添加 1 个文件，2 个已存在，1 个无法添加")));
    assert_eq!(h.app.shelf.items,vec![old.clone(),new]);
    std::fs::remove_file(&old).unwrap();h.app.on_drop(vec![old]);h.hover(H::local(300.0,60.0),0.3);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="文件不存在或无法访问")));
}
#[test]
fn reminders_are_visible_while_collapsed_and_can_be_disabled(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.t+=2701.0;h.frame(vec![]);assert!(h.app.notice.is_some());assert!(h.cmds.iter().any(|c|matches!(c,crate::app::Cmd::Notify(t,_)if t.contains("喝水"))));
    h.app.water.remind=false;h.app.settings.sit_remind=false;h.app.notice=None;h.cmds.clear();h.t+=2800.0;h.frame(vec![]);assert!(h.app.notice.is_none());
}
#[test]
fn water_serving_and_undo_are_consistent(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.app.go(2,1.0);h.hover(H::local(300.0,60.0),0.8);
    h.app.settings.serving_ml=350;h.click(H::local(70.0,110.0));assert_eq!(h.app.water.drunk_ml,350);assert_eq!(h.app.water.hours[9],350);
    h.click(H::local(191.0,110.0));assert_eq!(h.app.water.drunk_ml,0);assert_eq!(h.app.water.hours[9],0);
}
#[test]
fn mascot_rest_and_focus_have_distinct_states(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.opts.pet_idle=45.0;h.frame(vec![]);
    assert_eq!(h.app.pet_state(),crate::extras::PetState::Awake);
    h.t+=50.0;h.frame(vec![]);assert_eq!(h.app.pet_state(),crate::extras::PetState::Dozing);
    h.t+=140.0;h.frame(vec![]);assert_eq!(h.app.pet_state(),crate::extras::PetState::Sleeping);
    h.app.pomo.running=true;assert_eq!(h.app.pet_state(),crate::extras::PetState::Focused);
}

#[test]
fn mascot_stays_beside_the_clock_and_all_resident_components(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.pet=false;h.power=(Some(80),true);h.quota_accounts(5);
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:120.0,..Default::default()};
    h.app.pomo.running=true;h.app.pomo.remaining=1200.0;
    for _ in 0..120 {h.frame(vec![]);}let width=h.app.current_width();
    h.app.settings.pet=true;for _ in 0..120 {h.frame(vec![]);}
    assert!((h.app.current_width()-width-44.0).abs()<0.1,"岛仔应有独立常驻位置");
    assert_eq!(h.quota_centers().len(),5,"五个常驻账号均显示额度环");
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="09:41")),"岛仔与时钟共存");
    assert!(h.shapes.iter().filter(|s|matches!(&s.shape,egui::Shape::Path(p) if p.points.len()==13 && (p.stroke.width-3.6).abs()<0.01)).count()>=2,"没有闲置或悬停也应显示眼睛");
}

#[test]
fn mascot_and_album_share_the_single_line_lyric_island(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.pet=true;h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,
        artwork:Some(std::sync::Arc::new(egui::ColorImage::new([32,32],egui::Color32::from_rgb(70,120,220)))),..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:05]晚风吹过了山岗\n[00:15]下一句"));
    for _ in 0..120 {h.frame(vec![]);}
    assert!((h.app.current_width()-348.0).abs()<0.1,"专辑、岛仔与原有歌词宽度一起保留");
    let texture=h.app.music_artwork.as_ref().unwrap().1.id();
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Rect(r) if r.brush.as_ref().is_some_and(|b|b.fill_texture_id==texture))),"专辑封面不能被岛仔替代");
    let text=h.shapes.iter().find_map(|s|match &s.shape {egui::Shape::Text(t) if t.galley.text()=="晚风吹过了山岗"=>Some(t),_=>None}).unwrap();
    assert_eq!(text.galley.rows.len(),1,"保留单行歌词");
    assert!((text.pos.x+text.galley.rect.center().x-WIN_W/2.0-24.0).abs()<1.5,"歌词在封面和岛仔右侧居中");
    assert!(h.shapes.iter().filter(|s|matches!(&s.shape,egui::Shape::Path(p) if p.points.len()==13 && (2.79..=3.61).contains(&p.stroke.width))).count()>=2);
}

#[test]
fn singing_microphone_moves_relative_to_the_face_and_disappears_on_pause(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.pet=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:120.0,..Default::default()};
    for _ in 0..120 {h.frame(vec![]);}
    let head=|shapes:&[egui::epaint::ClippedShape]|shapes.iter().find_map(|s|match &s.shape {
        egui::Shape::Circle(circle) if (circle.radius-2.6).abs()<0.01=>Some(circle.center),_=>None,
    });
    let mut offsets=Vec::new();
    for _ in 0..120 {
        h.frame(vec![]);
        let head=head(&h.shapes).expect("演唱应显示球头麦克风");
        let eyes:Vec<_>=h.shapes.iter().filter_map(|s|match &s.shape {
            egui::Shape::Path(p) if p.points.len()==13 && (p.stroke.width-2.8).abs()<0.01=>Some(p.points.iter().map(|p|p.y).sum::<f32>()/13.0),_=>None,
        }).collect();
        assert_eq!(eyes.len(),2);offsets.push(head.y-(eyes[0]+eyes[1])/2.0);
    }
    if crate::bar_layout::animations_enabled(){
        let range=offsets.iter().copied().fold(f32::NEG_INFINITY,f32::max)-offsets.iter().copied().fold(f32::INFINITY,f32::min);
        assert!(range>0.4,"麦克风需要有独立动态");
    }
    h.media.playing=false;for _ in 0..90 {h.frame(vec![]);}
    assert!(head(&h.shapes).is_none(),"暂停后应收起麦克风");
}

#[test]
fn music_mascot_puts_away_the_microphone_during_notices_and_their_fade_out(){
    use crate::extras::PetState;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.pet=true;h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:05]晚风吹过了山岗\n[00:15]下一句"));
    let microphone=|h:&H|h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Circle(c)if(c.radius-2.6).abs()<0.01));
    for _ in 0..90{h.frame(vec![]);}
    assert_eq!(h.app.pet_state(),PetState::Music);assert!(microphone(&h));
    for (title,body) in [("大写锁定","已开启"),("大写锁定","已关闭"),("键盘输入语言","中文"),("系统音量","音量 60%"),("网络连接","Wi-Fi · 已连接"),("电源已连接","当前电量 80%"),("蓝牙设备","耳机")]{
        h.app.alert(title,body,5,false);
        for _ in 0..60{h.frame(vec![]);assert_eq!(h.app.pet_state(),PetState::Awake,"{title} 出现时应使用普通表情");assert!(!microphone(&h),"{title} 期间不能带着演唱道具");}
        h.t+=2.0;h.frame(vec![]);
        assert!(h.app.notice.is_none());
        assert_eq!(h.app.pet_state(),PetState::Awake,"提示淡出期间也不能提前开始演唱");assert!(!microphone(&h));
        for _ in 0..90{h.frame(vec![]);}
        assert_eq!(h.app.pet_state(),PetState::Music);assert!(microphone(&h),"提示完全结束后应恢复听歌动作");
        assert!(h.media.playing);assert!(h.app.island_lyric().is_some());
    }
    h.app.pet.react(PetState::Satisfied);h.app.alert("大写锁定","已开启",5,false);h.frame(vec![]);
    assert_eq!(h.app.pet_state(),PetState::Satisfied,"已有的喝水反馈应保留优先级");
}

#[test]
fn music_mascot_plays_new_expressions_and_resumes_after_interruptions(){
    use crate::extras::PetState;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.pet=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:300.0,..Default::default()};
    for _ in 0..1080 {h.frame(vec![]);}
    let expression=h.app.pet.expressions.current().expect("听歌状态也应显示新表情");
    assert_eq!(h.app.pet_state(),PetState::Music);
    let atlas=h.ctx.tex_manager().read().allocated().find_map(|(id,meta)|(meta.name=="daozai-expressions").then_some(*id)).unwrap();
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Mesh(mesh)if mesh.texture_id==atlas)));
    h.app.alert("系统音量","音量 60%",5,false);h.frame(vec![]);
    assert_eq!(h.app.pet_state(),PetState::Awake);assert!(h.app.pet.expressions.current().is_none());
    for _ in 0..300 {h.frame(vec![]);if h.app.pet_state()==PetState::Music {break;}}
    assert_eq!(h.app.pet.expressions.current(),Some(expression),"通知结束后继续原来的听歌表情");
    h.media.playing=false;h.frame(vec![]);assert!(h.app.pet.expressions.current().is_none());
    for _ in 0..60 {h.frame(vec![]);}
    h.media.playing=true;h.frame(vec![]);assert_eq!(h.app.pet.expressions.current(),Some(expression));
    h.app.pomo.running=true;h.frame(vec![]);assert_eq!(h.app.pet_state(),PetState::Focused);
    assert!(h.app.pet.expressions.current().is_none());
    h.app.pomo.running=false;h.frame(vec![]);assert_eq!(h.app.pet.expressions.current(),Some(expression));
    h.app.settings.pet=false;h.frame(vec![]);assert!(h.app.pet.expressions.current().is_none());
}

#[test]
fn selecting_permanent_mascot_or_minimal_applies_the_selected_mode(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.set_minimal(true);for _ in 0..90 {h.frame(vec![]);}
    assert!(h.app.settings.minimal && !h.app.settings.pet && h.region.height()<20.0);
    h.app.set_pet(true);for _ in 0..90 {h.frame(vec![]);}
    assert!(h.app.settings.pet && !h.app.settings.minimal && h.region.height()>30.0);
    let saved=crate::model::Settings::load();assert!(saved.pet && !saved.minimal);
}
#[test]
fn mascot_wakes_before_returning_to_idle(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.opts.pet_idle=45.0;
    h.frame(vec![]);h.t+=190.0;for _ in 0..60 {h.frame(vec![]);}
    assert_eq!(format!("{:?}",h.app.pet_state()),"Sleeping");
    h.cursor=Some(Pos2::new(550.0,100.0));h.frame(vec![egui::Event::PointerMoved(h.cursor.unwrap())]);
    assert_eq!(format!("{:?}",h.app.pet_state()),"Waking");
    for _ in 0..180 {h.frame(vec![]);}
    assert_eq!(format!("{:?}",h.app.pet_state()),"Awake");
}
#[test]
fn mascot_reacts_to_music_and_respects_focus(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:120.0,..Default::default()};h.frame(vec![]);
    assert_eq!(format!("{:?}",h.app.pet_state()),"Music");
    h.app.pomo.running=true;h.frame(vec![]);assert_eq!(format!("{:?}",h.app.pet_state()),"Focused");
    h.app.pomo.running=false;h.media.playing=false;h.frame(vec![]);assert_eq!(format!("{:?}",h.app.pet_state()),"Awake");
}
#[test]
fn mascot_keeps_task_celebration_until_panel_closes(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.todos.items.push(crate::model::TodoItem{text:"完成测试任务".into(),done_on:None,due:None,reminded:false,..Default::default()});
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.app.go(4,1.0);h.hover(H::local(300.0,60.0),1.0);h.click(H::local(250.0,47.0));
    assert!(h.app.todos.items[0].done_on.is_some());
    h.hover(H::local(300.0,60.0),4.0);assert_eq!(format!("{:?}",h.app.pet_state()),"Happy");
    h.cursor=None;for _ in 0..240 {h.frame(vec![]);}
    assert_eq!(format!("{:?}",h.app.pet_state()),"Awake");
}
#[test]
fn mascot_drink_nods_and_focus_completion_celebrates(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);h.app.drink_water();
    assert_eq!(format!("{:?}",h.app.pet_state()),"Satisfied");
    for _ in 0..240 {h.frame(vec![]);}
    h.app.pomo.running=true;h.app.pomo.remaining=0.01;h.frame(vec![]);
    assert_eq!(format!("{:?}",h.app.pet_state()),"Happy");
}
#[test]
fn mascot_power_and_network_recover_without_stale_warning(){
    use crate::extras::PetState;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.power=(Some(15),false);h.frame(vec![]);assert_eq!(h.app.pet_state(),PetState::Worried);
    h.power.1=true;h.frame(vec![]);assert_eq!(h.app.pet_state(),PetState::Awake);
    h.network=Some(false);h.frame(vec![]);assert_eq!(h.app.pet_state(),PetState::Worried);
    h.network=Some(true);h.frame(vec![]);assert_eq!(h.app.pet_state(),PetState::Awake);
    h.power=(None,false);h.network=None;h.frame(vec![]);assert_eq!(h.app.pet_state(),PetState::Awake);
}
#[test]
fn mascot_system_idle_wakes_once_then_follows_pointer(){
    use crate::extras::PetState;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.opts.pet_idle=45.0;
    h.system_idle=Some(200.0);h.frame(vec![]);assert_eq!(h.app.pet_state(),PetState::Sleeping);
    h.system_idle=Some(0.0);h.frame(vec![]);assert_eq!(h.app.pet_state(),PetState::Waking);
    for _ in 0..180 {h.frame(vec![]);}assert_eq!(h.app.pet_state(),PetState::Awake);
    h.cursor=Some(Pos2::new(WIN_W/2.0,50.0));
    for _ in 0..60 {h.frame(vec![]);}assert_eq!(h.app.pet_state(),PetState::Curious);assert!(!h.app.open);
}
#[test]
fn mascot_feedback_plays_beside_notice_and_respects_switches(){
    use crate::extras::PetState;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.drink_water();h.app.alert("待办到时间了","测试提醒",4,false);
    for _ in 0..60 {h.frame(vec![]);}assert_eq!(h.app.pet_state(),PetState::Satisfied);
    assert!(h.app.notice.is_some());
    let atlas=h.ctx.tex_manager().read().allocated().find_map(|(id,meta)|(meta.name=="daozai-expressions").then_some(*id)).expect("表情图集已加载");
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Mesh(mesh) if mesh.texture_id==atlas)),"通知期间岛仔仍应可见");
    for _ in 0..240 {h.frame(vec![]);}assert_eq!(h.app.pet_state(),PetState::Awake);
    h.app.notice=None;for _ in 0..240 {h.frame(vec![]);}assert_eq!(h.app.pet_state(),PetState::Awake);
    h.app.drink_water();h.app.settings.minimal=true;h.frame(vec![]);assert!(h.app.pet.reaction.is_none());
    h.app.settings.minimal=false;h.app.settings.pet=false;h.app.drink_water();h.frame(vec![]);assert!(h.app.pet.reaction.is_none());
}

#[test]
#[ignore = "按需导出吉祥物的实际渲染画面"]
fn mascot_expression_gallery(){
    use crate::extras::PetState;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::env::var_os("DAOZAI_PET_PREVIEW_DIR").map(std::path::PathBuf::from)
        .unwrap_or_else(||std::env::temp_dir().join(format!("daozai-pet-preview-{}",std::process::id())));
    std::fs::create_dir_all(&dest).unwrap();
    let states=[("awake",PetState::Awake),("curious",PetState::Curious),("waking",PetState::Waking),
        ("happy",PetState::Happy),("satisfied",PetState::Satisfied),("music",PetState::Music),
        ("worried",PetState::Worried),("focused",PetState::Focused),("dozing",PetState::Dozing),("sleeping",PetState::Sleeping)];
    for (name,state) in states {
        let mut h=H::new();h.app.pet.react(state);
        let mut raster=crate::preview_raster::Raster::new(1280,380);
        let last=if state==PetState::Waking{42}else{79};
        for frame in 0..=last {
            let t=frame as f64/60.0;
            let env=Env{t,clock:"09:41:00".into(),today:"2026-10-31".into(),date:"10月31日 周六".into(),..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H))),time:Some(t),
                viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
            let output=h.ctx.run(raw,|ctx|{
                egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{
                    let win=ui.max_rect();h.app.frame(ui,win,&env);
                });
            });
            raster.update_textures(&output.textures_delta);
            if frame==last {
                raster.buf.fill([0.09,0.11,0.18,1.0]);
                raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                assert!(rgba.chunks_exact(4).any(|p|p[0]>230&&p[1]>230&&p[2]>230),"表情画面不能空白：{name}");
                image::save_buffer(dest.join(format!("{name}.png")),&rgba,1280,380,image::ColorType::Rgba8).unwrap();
            }
            raster.free_textures(&output.textures_delta);
        }
    }
    eprintln!("表情预览：{}",dest.display());
}

#[test]
fn mascot_atlas_respects_visibility_and_skin_switches(){
    use crate::pet_expressions::Expression;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.pet=true;h.app.settings.minimal=false;
    for _ in 0..90 {h.frame(vec![]);}
    for skin in [crate::model::Skin::Classic,crate::model::Skin::Cream,crate::model::Skin::GlassLight] {
        h.app.settings.skin=skin;
        for expression in Expression::ALL {
            h.app.pet.expressions.play(expression);for _ in 0..30 {h.frame(vec![]);}
            assert_eq!(h.app.pet.expressions.current(),Some(expression));
            let atlas=h.ctx.tex_manager().read().allocated().find_map(|(id,meta)|(meta.name=="daozai-expressions").then_some(*id)).unwrap();
            let drawn=h.shapes.iter().find_map(|shape|match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id==atlas=>Some(mesh),_=>None,
            }).expect("新表情应画在常驻岛上");
            assert!(h.region.contains_rect(drawn.calc_bounds()),"新表情不能撑出岛体");
            // 每个表情单独一张序列图：横向铺满整张，纹理只有一列大小。
            assert!(drawn.vertices[0].uv.x.abs()<0.001);
            assert_eq!(h.ctx.tex_manager().read().meta(atlas).unwrap().size,[144,1440],"只保留正在播放的表情");
        }
    }
    h.app.settings.pet=false;h.frame(vec![]);assert!(h.app.pet.expressions.current().is_none());
    h.app.settings.pet=true;h.app.pet.expressions.play(Expression::Heart);h.app.settings.minimal=true;
    h.frame(vec![]);assert!(h.app.pet.expressions.current().is_none());
}

#[test]
#[ignore="按需导出接入程序后的十二组表情"]
fn mascot_integrated_expression_gallery(){
    use crate::pet_expressions::Expression;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_PET_INTEGRATED_PREVIEW").expect("需指定预览目录"));
    std::fs::create_dir_all(&dest).unwrap();
    for (name,skin) in [("classic",crate::model::Skin::Classic),("cream",crate::model::Skin::Cream),("glass",crate::model::Skin::GlassLight)] {
        let mut h=H::new();h.app.settings.pet=true;h.app.settings.minimal=false;h.app.settings.skin=skin;
        let mut raster=crate::preview_raster::Raster::new(1280,540);
        let mut sequence=0;
        for (index,expression) in Expression::ALL.into_iter().enumerate() {
            for frame in 0..=(expression.duration()*20.0).ceil() as usize {
                if frame==1 {h.app.pet.expressions.play(expression);}
                let t=sequence as f64/20.0;sequence+=1;
                let env=Env{t,clock:"09:41:00".into(),today:"2026-11-06".into(),date:"11月6日 周五".into(),idle_seconds:Some(0.0),..Default::default()};
                let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H))),time:Some(t),
                    viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
                let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{
                    let win=ui.max_rect();h.app.frame(ui,win,&env);
                });});
                raster.update_textures(&output.textures_delta);
                if frame>0 {
                    raster.buf.fill([0.95,0.96,0.98,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                    let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);let image=image::RgbaImage::from_raw(1280,540,rgba).unwrap();
                    image::imageops::crop_imm(&image,440,0,400,100).to_image().save(dest.join(format!("{name}-{index:02}-{frame:03}.png"))).unwrap();
                }
                raster.free_textures(&output.textures_delta);
            }
        }
    }
}
#[test]
fn minimal_island_hides_messenger_notices_until_expanded(){
    // 群友截图：极简刘海下来电 / 新消息后，细线变成带微信图标的方块。极简刘海照常弹横幅，收起后只留细线，未读不常挂。
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    use crate::app::Messenger;
    for (source,shape) in [(Messenger::Wechat,crate::model::IslandShape::Notch),(Messenger::Wechat,crate::model::IslandShape::Pill),(Messenger::Qq,crate::model::IslandShape::Pill)] {
        let mut h=H::new();
        h.app.settings.wechat_notifications=true;h.app.settings.read_notifications=true;h.app.settings.minimal=true;h.app.settings.island_shape=shape;
        for _ in 0..60 {h.frame(vec![]);}
        let idle=h.region;
        match source {
            Messenger::Wechat=>h.app.receive_wechat_reminder_from(4242,0x1234),
            Messenger::Qq=>h.app.receive_notification("QQ".into(),"QQ".into(),"QQ · 新消息".into()),
            Messenger::WeCom|Messenger::Feishu|Messenger::DingTalk=>h.app.receive_messenger_reminder(crate::app::Unread::of(source)),
        }
        for _ in 0..30 {h.frame(vec![]);}
        assert!(h.app.notice.as_ref().and_then(|a|a.messenger())==Some(source),"{source:?} {shape:?}：极简刘海照常弹横幅");
        for _ in 0..(8*60) {h.frame(vec![]);}
        assert!(h.app.notice.is_none(),"{source:?}：横幅按时收起");
        assert_eq!(h.app.pending_messengers.len(),1,"{source:?}：未读仍要记住");
        assert_eq!(h.region,idle,"{source:?} {shape:?}：收起时仍只有细线");
        // 鼠标移上去展开：未读胶囊出现在面板旁，可以点。
        h.hover(Pos2::new(WIN_W/2.0,2.0),0.8);h.hover(Pos2::new(WIN_W/2.0,40.0),1.0);
        assert!(h.app.open && h.app.progress()>0.9,"{source:?}：悬停应展开");
        assert!(h.app.messenger_appear()>0.9 && h.app.pending_messengers.len()==1,"{source:?}：展开后未读胶囊在面板旁显示");
    }
}
/// 微信语音来电：响铃期间任务栏持续闪动，按秒渲染岛体，排查样式问题。
#[test]
#[ignore="生成来电响铃时的岛体逐帧画面，供人工查看"]
fn preview_wechat_call_ringing(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::env::var_os("DAOZAI_CALL_PREVIEW_DIR").map(std::path::PathBuf::from)
        .unwrap_or_else(||std::env::temp_dir().join(format!("daozai-call-preview-{}",std::process::id())));
    std::fs::create_dir_all(&dest).unwrap();
    // 单条提醒 / 响铃期间反复提醒 / 来电窗口在另一个进程里（被当成第二个账号）。
    for (scenario,minimal) in [("single",false),("single",true),("ringing",false),("ringing",true),("twoproc",false),("twoproc",true)] {
        let mode=format!("{scenario}-{}",if minimal{"minimal"}else{"normal"});
        let mut h=H::new();h.app.settings.wechat_notifications=true;h.app.settings.minimal=minimal;
        let mut raster=crate::preview_raster::Raster::new(1280,380);
        for frame in 0..=(16*60) {
            let t=frame as f64/60.0;
            match scenario {
                "single" if frame==60=>h.app.receive_wechat_reminder_from(4242,0x1234),
                "ringing" if frame>=60 && t<12.0 && frame%330==60=>h.app.receive_wechat_reminder_from(4242,0x1234),
                "twoproc" if frame==60=>h.app.receive_wechat_reminder_from(4242,0x1234),
                "twoproc" if frame==120=>h.app.receive_wechat_reminder_from(4343,0x5678),
                _=>{}
            }
            let env=Env{t,clock:"09:41:00".into(),today:"2026-10-31".into(),date:"10月31日 周六".into(),..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H))),time:Some(t),
                viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
            let output=h.ctx.run(raw,|ctx|{
                egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let win=ui.max_rect();h.app.frame(ui,win,&env);});
            });
            raster.update_textures(&output.textures_delta);
            if frame%60==0 {
                raster.buf.fill([0.09,0.11,0.18,1.0]);
                raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                image::save_buffer(dest.join(format!("{mode}-{:04.1}s.png",t)),&rgba,1280,380,image::ColorType::Rgba8).unwrap();
            }
            raster.free_textures(&output.textures_delta);
        }
    }
    eprintln!("来电预览：{}",dest.display());
}
#[test]
fn todo_due_roundtrip_and_fires_only_once(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.todos.items.push(crate::model::TodoItem{text:"会议 | 准备".into(),done_on:None,due:Some("2026-10-28 09:40".into()),reminded:false,..Default::default()});h.app.todos.save();
    let loaded=crate::model::TodoList::load();assert_eq!(loaded.items[0].text,"会议 | 准备");assert_eq!(loaded.items[0].due.as_deref(),Some("2026-10-28 09:40"));
    h.frame(vec![]);assert!(h.app.todos.items[0].reminded);let n=h.cmds.len();h.frame(vec![]);assert_eq!(n,h.cmds.len());assert!(crate::model::TodoList::load().items[0].reminded);
}
#[test]
fn quiet_water_hours_cover_overnight(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.water_start=22;h.app.settings.water_end=7;
    assert!(h.app.water_hours(23));assert!(h.app.water_hours(2));assert!(!h.app.water_hours(12));
}
#[test]
fn repeated_system_changes_merge_without_losing_history(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.alert("系统音量","音量 20%",5,false);h.frame(vec![]);
    h.app.alert("系统音量","音量 30%",5,false);h.frame(vec![]);
    assert_eq!(h.app.notice.as_ref().unwrap().body,"音量 30%");assert_eq!(h.app.history.len(),2);assert!(h.app.alerts.is_empty());
}
#[test]
fn windows_notification_echo_does_not_duplicate_own_reminders(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.alert("喝水时间到了","喝点水吧",2,true);
    h.app.settings.read_notifications=true;h.app.receive_notification("Windows 通知".into(),"test".into(),"喝水时间到了 · 喝点水吧".into());assert_eq!(h.app.history.len(),1);
    assert_eq!(h.app.alerts.len(),1,"岛仔发出的系统通知不能再次进入岛内通知队列");
    h.app.notification_apps.items.push(("mail".into(),"邮件".into(),true));h.app.receive_notification("邮件".into(),"mail".into(),"收到一封新邮件".into());assert_eq!(h.app.alerts.len(),2);assert!(h.app.notification_bell_until>h.app.t);
}

#[test]
fn minimal_takes_precedence_over_mascot(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.pet=true;h.app.settings.minimal=true;
    for _ in 0..120 {h.frame(vec![]);}assert!(h.region.height()<20.0,"启用极简和吉祥物时必须显示细线");
    h.hover(Pos2::new(WIN_W/2.0,2.0),1.0);assert!(h.app.progress()>0.9,"极简线仍要能悬停展开");
}
#[test]
fn water_settings_are_in_the_island_and_editable(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    h.app.go(2,1.0);h.hover(H::local(300.0,60.0),0.8);h.click(H::local(145.0,110.0));assert_eq!(h.app.page,2);assert_eq!(h.app.subpage,Some(2));assert_eq!(h.app.settings_category,1);
    h.hover(H::local(300.0,60.0),0.8);h.click(H::local(357.0,69.0));assert_eq!(h.app.settings.serving_ml,300);
    // 标题旁的切换在设置子页同样可用：切到久坐后第一行就是久坐时长，最后一行时段两边共用。
    h.click(H::local(136.0,19.0));assert!(h.app.sit_mode);for _ in 0..40{h.frame(vec![]);}
    h.hover(H::local(300.0,60.0),0.3);h.click(H::local(357.0,47.0));assert_eq!(h.app.settings.sit_minutes,65);
    h.hover(H::local(300.0,60.0),0.3);h.click(H::local(357.0,113.0));assert!(h.app.health_time,"最后一行进入提醒时段二级页");
    let end=h.app.settings.water_end;h.hover(H::local(300.0,60.0),0.3);h.click(H::local(357.0,47.0));assert_eq!(h.app.settings.water_end,(end+1)%24);
}
#[test]
fn lunch_break_silences_water_and_sit_reminders(){
    use crate::app::SIT_TITLE;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.system_idle=Some(0.0);h.frame(vec![]);
    // 测试时钟固定在 09:41，把午休设成 09:30–10:00。
    h.app.settings.lunch_rest=true;h.app.settings.lunch_start=9*60+30;h.app.settings.lunch_end=10*60;
    h.app.water.remind=true;h.app.last_reminder=h.app.t-2.0*3600.0;h.app.sit_start=h.app.t-2.0*3600.0;
    h.frame(vec![]);assert!(h.app.notice.is_none(),"午休中不应弹喝水或久坐提醒");
    h.app.settings.lunch_rest=false;h.frame(vec![]);
    assert!(h.app.notice.is_some(),"关闭午休后应恢复提醒");
    h.app.notice=None;h.app.alerts.clear();h.app.water.remind=false;h.app.confirm_health_reminder(crate::extras::HealthReminder::Sit);h.app.sit_start=h.t-2.0*3600.0;
    h.app.settings.lunch_rest=true;h.app.settings.lunch_start=10*60;h.app.settings.lunch_end=13*60;
    h.frame(vec![]);assert!(h.app.notice.as_ref().is_some_and(|a|a.title==SIT_TITLE),"午休以外照常提醒");
}
#[test]
fn health_time_subpage_edits_lunch_and_backs_out_one_level(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    h.app.open_settings_category(1);for _ in 0..30{h.frame(vec![]);}
    h.hover(H::local(300.0,60.0),0.3);h.click(H::local(200.0,113.0));assert!(h.app.health_time,"点提醒时段行进入二级页");
    let start=h.app.settings.lunch_start;h.hover(H::local(300.0,60.0),0.3);h.click(H::local(225.0,91.0));assert_eq!(h.app.settings.lunch_start,start+15);
    h.hover(H::local(300.0,60.0),0.3);h.click(H::local(353.0,69.0));assert!(!h.app.settings.lunch_rest,"午休开关可关闭");
    h.hover(H::local(300.0,60.0),0.3);h.click(H::local(20.0,18.0));assert!(!h.app.health_time);assert_eq!(h.app.subpage,Some(2),"返回先回到健康提醒设置");
}
#[test]
fn sit_reminder_waits_for_confirmation_before_restarting(){
    use crate::app::SIT_TITLE;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.system_idle=Some(0.0);h.frame(vec![]);h.app.water.remind=false;
    h.t+=59.0*60.0;h.frame(vec![]);assert!(h.app.notice.is_none(),"未满 60 分钟不提醒: {:?}",h.app.notice.as_ref().map(|a|(a.title.clone(),a.body.clone())));
    h.t+=61.0;h.frame(vec![]);
    assert!(h.app.notice.as_ref().is_some_and(|a|a.title==SIT_TITLE));
    assert!(h.cmds.iter().any(|c|matches!(c,crate::app::Cmd::Notify(t,_)if t==SIT_TITLE)));
    let count=h.app.history.len();
    h.screen_locked=true;h.system_idle=Some(40.0*60.0);h.t+=500.0*60.0;h.frame(vec![]);
    assert!(h.app.notice.is_none());assert_eq!(h.app.history.len(),count,"锁屏和长时间未处理都只保留一次提醒");
    h.app.confirm_health_reminder(crate::extras::HealthReminder::Sit);assert_eq!(h.app.sat_minutes(),0);
    h.screen_locked=false;h.system_idle=Some(0.0);h.t+=59.0*60.0;h.frame(vec![]);assert!(h.app.notice.is_none());
    h.t+=61.0;h.frame(vec![]);assert!(h.app.notice.as_ref().is_some_and(|a|a.title==SIT_TITLE));
}
#[test]
fn sit_reminder_waits_for_focus_and_stops_with_module_or_switch(){
    use crate::app::{Phase,SIT_TITLE,WATER_PAGE};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.system_idle=Some(0.0);h.frame(vec![]);h.app.water.remind=false;
    h.app.pomo.phase=Phase::Focus;h.app.pomo.remaining=1e7;h.app.pomo.running=true;
    h.t+=61.0*60.0;h.frame(vec![]);assert!(h.app.notice.is_none(),"专注中不打扰: {:?}",h.app.notice.as_ref().map(|a|(a.title.clone(),a.body.clone())));
    h.app.pomo.running=false;h.frame(vec![]);assert!(h.app.notice.as_ref().is_some_and(|a|a.title==SIT_TITLE),"专注结束后补上提醒");
    h.app.notice=None;h.app.settings.sit_remind=false;h.t+=2.0*3600.0;h.frame(vec![]);assert!(h.app.notice.is_none());
    h.app.settings.sit_remind=true;h.app.set_page_visible(WATER_PAGE,false);h.t+=2.0*3600.0;h.frame(vec![]);assert!(h.app.notice.is_none(),"模块关闭即不再提醒");
}
#[test]
fn sit_notice_and_page_buttons_reset_the_timer(){
    use crate::app::SIT_TITLE;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.system_idle=Some(0.0);h.frame(vec![]);
    h.app.sit_start=h.t-70.0*60.0;h.app.alert(SIT_TITLE,"已连续坐了 70 分钟",2,false);for _ in 0..60 {h.frame(vec![]);}
    h.click_text("知道了");assert!(h.app.notice.is_none());assert!((h.app.sit_due()-h.t-3600.0).abs()<5.0);assert_eq!(h.app.water.drunk_ml,0);
    h.app.alert(SIT_TITLE,"已连续坐了 70 分钟",2,false);for _ in 0..60 {h.frame(vec![]);}
    h.click_text("起身了");assert!(h.app.notice.is_none());assert_eq!(h.app.sat_minutes(),0);assert_eq!(h.app.water.drunk_ml,0);
    // 健康页：点「久坐」分段切换，同一位置的主按钮变成「起身了」、开关管久坐；点「喝水」切回
    h.app.sit_start=h.t-30.0*60.0;
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.app.go(2,1.0);h.hover(H::local(300.0,60.0),0.8);
    h.click(H::local(124.0,22.0));assert!(h.app.sit_mode);h.hover(H::local(300.0,90.0),0.8);
    h.click(H::local(70.0,110.0));assert_eq!(h.app.sat_minutes(),0);assert_eq!(h.app.water.drunk_ml,0);
    let water_remind=h.app.water.remind;
    h.click(H::local(179.0,46.0));assert!(!h.app.settings.sit_remind);assert_eq!(h.app.water.remind,water_remind,"久坐模式的开关只管久坐");
    assert!(!crate::model::Settings::load().sit_remind,"重新加载后应保留久坐开关");
    h.click(H::local(97.0,22.0));assert!(!h.app.sit_mode);h.hover(H::local(300.0,90.0),0.8);
    h.click(H::local(70.0,110.0));assert_eq!(h.app.water.drunk_ml,250,"回到喝水后按钮可用");
}
#[test]
fn water_notice_buttons_work_without_opening_panel(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.alert("喝水时间到了","该喝水了",2,false);for _ in 0..60 {h.frame(vec![]);}
    h.click_text("喝一口");assert_eq!(h.app.water.drunk_ml,250);assert!(h.app.notice.is_none());
}
#[test]
fn focus_completion_offers_starting_the_break(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);h.app.pomo.running=true;h.app.pomo.remaining=1.0;h.t+=2.0;h.frame(vec![]);
    assert!(h.app.pomo.phase==crate::app::Phase::Rest);assert!(!h.app.pomo.running);assert!(h.app.notice.is_some());
    for _ in 0..60 {h.frame(vec![]);}h.click_text("开始");assert!(h.app.pomo.running);
}
#[test]
fn todo_completed_items_move_down_and_undo_restores_the_original_order(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.todos.items=["已完成甲","待做甲","周一重复","已完成乙","每日待办"].into_iter().map(crate::model::TodoItem::new).collect();
    h.app.todos.items[0].done_on=Some("2026-10-28".into());h.app.todos.items[3].done_on=Some("2026-10-28".into());h.app.todos.items[2].repeat=1;h.app.todos.items[4].repeat=crate::model::REPEAT_ALL;
    let original=h.app.todos.items.iter().map(|t|t.text.clone()).collect::<Vec<_>>();assert_eq!(h.app.todos.display_order("2026-10-28"),vec![1,4,2,0,3],"今天未完成优先，其次其他日期任务，已完成沉底");
    h.app.todos.items[0].toggle("2026-10-28");assert_eq!(h.app.todos.display_order("2026-10-28"),vec![0,1,4,2,3],"取消完成应恢复原有相对顺序");
    h.app.todos.items[4].toggle("2026-10-28");assert_eq!(h.app.todos.display_order("2026-10-28"),vec![0,1,2,3,4]);assert_eq!(h.app.todos.display_order("2026-10-29"),vec![0,1,4,2,3],"重复任务到下一天应回到未完成区");
    assert_eq!(h.app.todos.items.iter().map(|t|t.text.clone()).collect::<Vec<_>>(),original,"只改变显示顺序，不能移动原数据");
}

#[test]
fn todo_edit_choice_can_edit_other_rows_and_completed_items(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.todos.items=["第一项","第二项","已完成任务"].into_iter().map(crate::model::TodoItem::new).collect();h.app.todos.items[2].done_on=Some("2026-10-28".into());
    h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click_text("改一下");
    assert!(h.app.editing.is_none(),"多条待办应先选择，不能默认修改第一项");h.frame(vec![]);h.click_text("第二项");
    assert_eq!(h.app.editing,Some(1));h.app.buf="第二项已修改".into();h.frame(vec![egui::Event::Key{key:egui::Key::Enter,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
    assert_eq!(h.app.todos.items[0].text,"第一项");assert_eq!(h.app.todos.items[1].text,"第二项已修改");assert!(h.app.todos.items[2].done_on.is_some());
    h.frame(vec![]);h.click_text("改一下");h.frame(vec![]);h.click_text("已完成任务");assert_eq!(h.app.editing,Some(2));
    h.frame(vec![egui::Event::Key{key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);assert!(!h.app.is_editing());assert!(h.app.todos.items[2].done_on.is_some());
}

#[test]
fn todo_edit_choice_keeps_a_single_completed_task_editable(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.todos.items=vec![crate::model::TodoItem::new("已完成任务")];h.app.todos.items[0].done_on=Some("2026-10-28".into());
    h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click_text("改一下");assert_eq!(h.app.editing,Some(0),"只有一条时可直接编辑，完成状态不能禁用编辑");
}

#[test]
fn todo_edit_choice_reaches_scrolled_items_and_cancels_without_changes(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.todos.items=(0..7).map(|i|crate::model::TodoItem::new(format!("任务 {i}"))).collect();
    h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click_text("改一下");assert!(h.app.editing.is_none());
    h.hover(H::local(300.0,60.0),0.2);for _ in 0..7{h.wheel(-24.0);}assert_eq!(h.app.page(),4,"选择事项时滚到末尾也不能翻页");h.frame(vec![]);h.click_text("任务 6");assert_eq!(h.app.editing,Some(6));
    h.frame(vec![egui::Event::Key{key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);assert!(!h.app.is_editing());assert_eq!(h.app.todos.items[6].text,"任务 6");assert!(h.app.todos.items.iter().all(|t|t.done_on.is_none()));
}

#[test]
fn todo_completed_reordering_does_not_complete_the_next_item_on_a_double_click(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.todos.items=["任务甲","任务乙","任务丙"].into_iter().map(crate::model::TodoItem::new).collect();h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);
    let point=H::local(212.0,46.0);h.click(point);h.frame(vec![]);h.click(point);h.frame(vec![]);
    assert!(h.app.todos.items[0].done_on.is_some());assert!(h.app.todos.items[1].done_on.is_none(),"首项下移后，快速连点不能误完成下一项");
    h.click_text("任务甲");h.frame(vec![]);assert!(h.app.todos.items.iter().all(|t|t.done_on.is_none()));assert_eq!(h.app.todos.display_order("2026-10-28"),vec![0,1,2]);
}

#[test]
fn todo_edit_choice_can_cancel_without_toggling_or_deleting_items(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.todos.items=["任务甲","任务乙"].into_iter().map(crate::model::TodoItem::new).collect();h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);
    h.click_text("改一下");h.frame(vec![]);h.click_text("取消");assert!(!h.app.is_editing());assert_eq!(h.app.todos.items.len(),2);assert!(h.app.todos.items.iter().all(|t|t.done_on.is_none()));
    h.frame(vec![]);h.click_text("改一下");h.frame(vec![egui::Event::Key{key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);assert!(!h.app.is_editing());assert_eq!(h.app.todos.items.len(),2);
}

#[test]
fn todo_edit_choice_keeps_the_selected_item_when_an_earlier_item_is_deleted(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.todos.items=["第一项","第二项","第三项"].into_iter().map(crate::model::TodoItem::new).collect();h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);
    h.click_text("改一下");h.frame(vec![]);h.click_text("第二项");h.frame(vec![]);h.click(h.ctx.read_response(egui::Id::new(("card",true,("todo-delete",0usize)))).unwrap().rect.center());
    assert_eq!(h.app.editing,Some(0),"删除前面的条目后，编辑目标仍须是第二项");h.app.buf="第二项已修改".into();h.frame(vec![egui::Event::Key{key:egui::Key::Enter,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);assert_eq!(h.app.todos.items.iter().map(|t|t.text.as_str()).collect::<Vec<_>>(),["第二项已修改","第三项"]);
}

#[test]
#[ignore="按需导出待办完成排序和点选编辑的实际画面"]
fn todo_edit_choice_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_TODO_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for skin in [crate::model::Skin::Classic,crate::model::Skin::Cream]{for choosing in [false,true]{
        let mut h=H::new();h.app.settings.skin=skin;h.app.todos.items=["整理会议资料","确认本周安排","已经完成的任务"].into_iter().map(crate::model::TodoItem::new).collect();h.app.todos.items[2].done_on=Some("2026-11-06".into());h.app.show_page(4);h.app.todo_choosing=choosing;
        let mut raster=feature_raster();shoot_feature(&mut h,&mut raster,&dest,&format!("todo-{}-{}",skin.key(),if choosing{"choose"}else{"list"}),90,Some(H::local(175.0,70.0)),"2026-11-06");
    }}
}

#[test]
fn task_editor_saves_and_cancels_inside_island(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    h.app.todos.items.push(crate::model::TodoItem{text:"准备会议".into(),done_on:None,due:None,reminded:false,..Default::default()});h.app.go(4,1.0);h.hover(H::local(300.0,60.0),0.8);h.app.open_task_dialog(0);
    h.hover(H::local(300.0,60.0),0.2);assert!(h.app.is_editing());h.click(H::local(326.0,110.0));assert!(!h.app.is_editing());assert_eq!(h.app.todos.items[0].text,"准备会议");
    h.app.open_task_dialog(0);h.frame(vec![egui::Event::Key{key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);assert!(!h.app.is_editing());
}
#[test]
fn focus_timer_gets_room_in_the_collapsed_island(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();for _ in 0..90 {h.frame(vec![]);}
    let before=h.app.current_width();h.app.pomo.running=true;
    for _ in 0..100 {h.frame(vec![]);}assert!(h.app.current_width()>before+20.0);assert!(h.app.pomo.remaining<1500.0);
}

#[test]
fn switching_to_rest_waits_for_the_start_button(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.app.go(1,1.0);h.hover(H::local(260.0,70.0),1.0);
    h.click(H::local(296.0,106.5));
    assert!(h.app.pomo.phase==crate::app::Phase::Rest);
    assert!(!h.app.pomo.running,"切换阶段应等待用户点击开始");
}

#[test]
fn starting_timer_keeps_lyrics_album_and_mascot_with_a_compact_countdown(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.island_lyrics=true;h.app.settings.pet=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,
        artwork:Some(std::sync::Arc::new(egui::ColorImage::new([32,32],egui::Color32::from_rgb(70,120,220)))),..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:05]晚风吹过了山岗\n[00:15]下一句"));
    for _ in 0..120 {h.frame(vec![]);}
    for phase in [crate::app::Phase::Focus,crate::app::Phase::Rest] {
        h.app.pomo.phase=phase;h.app.pomo.remaining=h.app.phase_secs(phase);h.app.pomo.running=true;
        for _ in 0..120 {
            h.frame(vec![]);
            assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("晚风吹过了山岗"),"计时不能关闭岛上歌词");
            assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="晚风吹过了山岗" && t.galley.rows.len()==1)));
            assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="09:41")));
        }
        let texture=h.app.music_artwork.as_ref().unwrap().1.id();
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Rect(r) if r.brush.as_ref().is_some_and(|b|b.fill_texture_id==texture))));
        let secs=h.app.pomo.remaining.ceil() as u32;let countdown=format!("{:02}:{:02}",secs/60,secs%60);
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()==countdown)),"歌词模式中也应显示计时");
        assert!(h.app.current_width()<=PW+0.1);assert!((h.hit.height()-34.0).abs()<0.1);
    }
    h.app.pomo.running=false;for _ in 0..90 {h.frame(vec![]);}
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("晚风吹过了山岗"));
}

#[test]
fn timer_keeps_the_existing_lyric_segments_and_content_bounds(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let line="愿每一个认真生活的人，都能在平凡的日子里找到属于自己的光";
    for pet in [true,false] {
        let mut h=H::new();h.app.settings.island_lyrics=true;h.app.settings.pet=pet;
        h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:6.0,duration:120.0,playing:true,..Default::default()};
        h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc(&format!("[00:05]{line}\n[00:15]下一句")));
        let shown=|h:&H|h.shapes.iter().find_map(|s|match &s.shape {
            egui::Shape::Text(t) if !t.galley.text().is_empty() && line.contains(t.galley.text())=>Some((t.galley.text().to_owned(),t.visual_bounding_rect(),t.galley.rows.len(),t.galley.elided)),_=>None,
        }).unwrap();
        for position in [6.0,14.0] {
            h.media.position=position;h.app.pomo.running=false;
            for _ in 0..90 {h.frame(vec![]);}let expected=shown(&h).0;
            h.app.pomo.running=true;for _ in 0..120 {h.frame(vec![]);}
            let (text,bounds,rows,elided)=shown(&h);assert_eq!(text,expected,"计时不应改变歌词分段");
            assert_eq!(rows,1);assert!(!elided);
            let secs=h.app.pomo.remaining.ceil() as u32;let countdown=format!("{:02}:{:02}",secs/60,secs%60);
            let timer=h.shapes.iter().find_map(|s|match &s.shape {egui::Shape::Text(t) if t.galley.text()==countdown=>Some(t.visual_bounding_rect()),_=>None}).unwrap();
            assert!(bounds.right()+20.0<timer.left(),"歌词与倒计时之间应留出波形和图标的位置");
            assert!(timer.right()<h.hit.right() && bounds.top()>=h.hit.top() && bounds.bottom()<h.hit.bottom());
            assert!((h.app.current_width()-if pet{418.0}else{370.0}).abs()<0.1);
        }
    }
}

#[test]
fn focus_page_selects_stopped_phases_and_starts_only_from_play(){
    use crate::app::Phase;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.focus_minutes=30;h.app.settings.rest_minutes=7;
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.app.go(1,1.0);h.hover(H::local(260.0,70.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="休息")));
    h.click(H::local(336.0,106.5));
    assert!(h.app.pomo.phase==Phase::Rest && !h.app.pomo.running);
    assert_eq!(h.app.pomo.remaining,420.0,"休息采用设置中的时长");
    h.click(H::local(264.0,106.5));assert!(h.app.pomo.running);
    h.click(H::local(264.0,106.5));assert!(!h.app.pomo.running);
    let paused=h.app.pomo.remaining;for _ in 0..30 {h.frame(vec![]);}assert_eq!(h.app.pomo.remaining,paused);
    h.click(H::local(264.0,106.5));assert!(h.app.pomo.running && h.app.pomo.phase==Phase::Rest);
    h.click(H::local(336.0,106.5));assert!(h.app.pomo.running,"点击当前阶段不重置计时");
    h.click(H::local(192.0,106.5));assert!(h.app.pomo.phase==Phase::Focus && !h.app.pomo.running);
    assert_eq!(h.app.pomo.remaining,1800.0);
    h.click(H::local(264.0,106.5));assert!(h.app.pomo.running);
    h.click(H::local(336.0,106.5));assert!(h.app.pomo.phase==Phase::Rest && !h.app.pomo.running);
    h.click(H::local(264.0,106.5));assert!(h.app.pomo.running);
    h.click(H::local(329.0,50.0));assert!(!h.app.pomo.running && h.app.pomo.phase==Phase::Rest);
    assert_eq!(h.app.pomo.remaining,420.0,"重置应保留休息阶段");
}

#[test]
fn collapsed_focus_uses_tomato_and_rest_uses_a_white_cup(){
    use crate::app::Phase;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    for phase in [Phase::Focus,Phase::Rest] {
        h.app.pomo.phase=phase;h.app.pomo.remaining=h.app.phase_secs(phase);h.app.pomo.running=true;
        for _ in 0..120 {h.frame(vec![]);}
        let secs=h.app.pomo.remaining.ceil() as u32;let text=format!("{:02}:{:02}",secs/60,secs%60);
        let color=if phase==Phase::Focus{crate::theme::RED_TOP}else{crate::theme::SOFT_GREEN};
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()==text && t.fallback_color==color)),"常驻倒计时颜色应跟随阶段");
        let tomato=h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Mesh(m) if m.vertices.iter().any(|v|v.color==crate::theme::RED_TOP)));
        if phase==Phase::Focus {assert!(tomato,"专注时应显示红色番茄水位");}
        else {
            assert!(!tomato,"休息时应替换番茄");
            assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Rect(r) if r.fill==egui::Color32::from_rgb(245,242,234) && (7.0..13.0).contains(&r.rect.width()) && (5.0..10.0).contains(&r.rect.height()))),"休息时应显示白瓷茶杯");
        }
        assert!((h.hit.height()-34.0).abs()<0.1,"图标不能撑高常驻岛");
    }
}

#[test]
fn focus_and_rest_completion_prepare_the_configured_next_phase_without_starting(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.settings.rest_minutes=7;h.app.settings.focus_minutes=30;
    for _ in 0..5 {
        h.app.pomo.phase=crate::app::Phase::Focus;h.app.pomo.running=true;h.app.pomo.remaining=0.01;h.frame(vec![]);
        assert!(h.app.pomo.phase==crate::app::Phase::Rest && !h.app.pomo.running);
        assert_eq!(h.app.pomo.remaining,420.0);assert!(h.app.notice.is_some());
        h.app.pomo.running=true;h.app.pomo.remaining=0.01;h.frame(vec![]);
        assert!(h.app.pomo.phase==crate::app::Phase::Focus && !h.app.pomo.running);
        assert_eq!(h.app.pomo.remaining,1800.0);
    }
}

#[test]
fn rest_settings_keep_the_existing_duration_and_save_the_new_key(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let _h=H::new();
    assert_eq!(crate::model::Settings::default().rest_minutes,5);
    let path=crate::model::config_dir().join("settings.txt");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path,"focus_minutes=30\nshort_minutes=7\nlong_minutes=17\n").unwrap();
    let settings=crate::model::Settings::load();assert_eq!(settings.rest_minutes,7);
    settings.save();let saved=std::fs::read_to_string(&path).unwrap();
    assert!(saved.lines().any(|line|line=="rest_minutes=7"));
    assert!(!saved.contains("short_minutes") && !saved.contains("long_minutes"));
    let reloaded=crate::model::Settings::load();assert_eq!(reloaded.rest_minutes,7);assert_eq!(reloaded.focus_minutes,30);
}

#[test]
fn collapsed_focus_animates_without_music_or_mascot_and_stops_in_minimal_mode(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.pet=false;h.app.pomo.running=true;
    for _ in 0..120 {h.frame(vec![]);}
    if crate::bar_layout::animations_enabled(){assert!(h.animating,"关闭吉祥物后，常驻番茄的水面仍应轻微波动");}
    h.app.settings.minimal=true;for _ in 0..120 {h.frame(vec![]);}
    assert!(!h.animating,"极简细线隐藏计时图标后，应停止其动画刷新");
}
#[test]
fn app_notifications_show_temporary_bell_in_minimal_mode(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.minimal=true;h.frame(vec![]);
    h.app.settings.read_notifications=true;h.app.receive_notification("邮件".into(),"mail.app".into(),"私人内容".into());
    assert!(h.app.notification_apps.items[0].2);assert!(h.app.notification_bell_until>h.app.t);assert_eq!(h.app.alerts.len(),1);
    assert!(!std::fs::read_to_string(crate::model::config_dir().join("notification-apps.json")).unwrap().contains("私人内容"));
    for _ in 0..90 {h.frame(vec![]);}assert!(h.region.height()>20.0,"极简模式收到通知时应临时展开");
    h.t+=5.0;for _ in 0..90 {h.frame(vec![]);}assert!(h.region.height()<20.0,"铃铛消失后应恢复极简模式");
    h.app.notification_apps.items[0].2=false;let until=h.app.notification_bell_until;
    h.app.settings.read_notifications=true;h.app.receive_notification("邮件".into(),"mail.app".into(),"私人内容".into());assert_eq!(h.app.notification_bell_until,until);
}

#[test]
fn fluid_music_seek_draws_solid_played_segment(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for lyrics in [false,true] {
        let mut h=H::new();
        h.app.settings.skin=crate::model::Skin::Fluid;
        h.app.show_lyrics=lyrics;
        h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),position:30.0,duration:120.0,..Default::default()};
        h.hover(Pos2::new(WIN_W/2.0,12.0),1.2);
        let (x,w,y)=if lyrics{(44.0,176.0,82.0)}else{(96.0,221.0,92.6)};
        let start=H::local(x,y);
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Rect(r)
            if (r.rect.left()-start.x).abs()<1.0 && (r.rect.center().y-start.y).abs()<1.0
            && (r.rect.width()-w*0.25).abs()<1.0 && r.fill.a()==255 && r.fill.r()>160)));
    }
}

#[test]
fn music_seek_drag_previews_and_commits_on_release(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for (source,lyrics) in [("",false),("",true),("cloudmusic.exe",false),("cloudmusic.exe",true),("bodian_pc.exe",false),("bodian_pc.exe",true),("kwmusic.exe",false),("kwmusic.exe",true)] {
        let mut h=H::new();h.app.show_lyrics=lyrics;
        h.media=crate::smtc::NowPlaying{source:source.into(),title:"Track".into(),artist:"Artist".into(),position:12.0,duration:120.0,..Default::default()};
        h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
        let (x,w,y)=if lyrics{(44.0,176.0,82.0)}else{(96.0,221.0,92.6)};
        let press=H::local(x+w*0.1,y);h.cursor=Some(press);
        h.frame(vec![egui::Event::PointerMoved(press)]);
        h.frame(vec![egui::Event::PointerButton{pos:press,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
        let to=H::local(x+w*0.75,y);h.cursor=Some(to);
        h.frame(vec![egui::Event::PointerMoved(to)]);
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="01:30")),"{source} 拖动中应显示目标时间");
        assert!(!h.cmds.iter().any(|c|matches!(c,crate::app::Cmd::Media(crate::smtc::Transport::Seek(_)))),"拖动中不应反复发送跳转");
        h.frame(vec![egui::Event::PointerButton{pos:to,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
        assert!(h.cmds.iter().any(|c|matches!(c,crate::app::Cmd::Media(crate::smtc::Transport::Seek(t)) if (*t-90.0).abs()<0.1)),"释放应提交目标进度");
    }
}

#[test]
fn old_and_new_qqmusic_seek_and_hold_the_target_after_release(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for source in ["QQMusic.exe","Tencent.QQMusic"] {
        let mut h=H::new();h.app.show_lyrics=false;
        h.media=crate::smtc::NowPlaying{source:source.into(),title:"Track".into(),artist:"Artist".into(),position:12.0,duration:120.0,playback_type:crate::smtc::MediaKind::Music,..Default::default()};
        h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
        let (x,w,y)=(96.0,221.0,92.6);
        let press=H::local(x+w*0.1,y);h.cursor=Some(press);
        h.frame(vec![egui::Event::PointerMoved(press)]);
        h.frame(vec![egui::Event::PointerButton{pos:press,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
        let to=H::local(x+w*0.75,y);h.cursor=Some(to);
        h.frame(vec![egui::Event::PointerMoved(to)]);
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="01:30")),"{source} 拖动中显示目标时间");
        h.frame(vec![egui::Event::PointerButton{pos:to,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
        assert!(h.cmds.iter().any(|c|matches!(c,crate::app::Cmd::Media(crate::smtc::Transport::Seek(t)) if (*t-90.0).abs()<0.1)),"{source} 松手提交跳转");
        // 播放器还没回报新位置：松手后仍显示目标时间，不弹回 00:12。
        h.frame(vec![]);
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="01:30")),"{source} 松手后保持目标时间");
        assert!(crate::smtc::is_qqmusic(source));
    }
}

#[test]
fn volume_hud_drag_maps_pointer_to_percentage(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.alert("系统音量","音量 0%",5,false);
    for _ in 0..60 {h.frame(vec![]);}
    assert!(!h.app.open);
    h.click_notice_badge();
    // 缩短后的系统音量条仍支持点击和拖动。
    let (x0,y)=if h.app.compact_notices(){(WIN_W/2.0+(h.app.current_width()-44.0)/2.0+8.0+crate::app::HUD_TRACK_X,55.0)}else{(WIN_W/2.0-h.app.current_width()/2.0+h.app.pet_space()+crate::app::HUD_TRACK_X,19.0)};
    let press=Pos2::new(x0+52.0,y);
    h.cursor=Some(press);h.frame(vec![egui::Event::PointerMoved(press)]);
    h.frame(vec![egui::Event::PointerButton{pos:press,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
    let to=Pos2::new(x0+105.0,y);
    h.cursor=Some(to);h.frame(vec![egui::Event::PointerMoved(to)]);
    h.frame(vec![egui::Event::PointerButton{pos:to,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
    let last=h.cmds.iter().rev().find_map(|c|match c{crate::app::Cmd::SetVolume(v)=>Some(*v),_=>None});
    assert_eq!(last,Some(70));
}

#[test]
fn bluetooth_connection_soon_after_initial_scan_is_not_suppressed(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.frame(vec![]);
    h.t=2.0;
    h.bt=vec![crate::bt::Device{id:"shokz".into(),name:"Shokz".into(),battery:Some(100),kind:crate::bt::DeviceKind::Bone}];
    h.frame(vec![]);
    assert_eq!(h.app.notice.as_ref().map(|a|a.title.as_str()),Some("蓝牙设备"));
}

#[test]
fn bluetooth_audio_connection_takes_precedence_over_volume_notice(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for volume_already_visible in [false,true] {
        let mut h=H::new();h.frame(vec![]);
        h.app.alert("系统音量","音量 80%",5,false);
        if volume_already_visible {h.frame(vec![]);}
        h.bt=vec![crate::bt::Device{id:"audio".into(),name:"Shokz".into(),battery:Some(80),kind:crate::bt::DeviceKind::Bone}];
        h.frame(vec![]);
        assert_eq!(h.app.notice.as_ref().map(|a|a.title.as_str()),Some("蓝牙设备"),"耳机连接应优先于音量提示");
        assert!(!h.app.alerts.iter().any(|a|a.title=="系统音量"),"切换耳机产生的音量提示不应延后补弹");
        h.t+=3.0;h.frame(vec![]);
        h.app.alert("系统音量","音量 60%",5,false);h.frame(vec![]);
        assert_eq!(h.app.notice.as_ref().map(|a|a.body.as_str()),Some("音量 60%"),"连接后手动调节音量应正常提示");
    }
}

#[test]
fn bluetooth_keyboard_connection_keeps_unrelated_volume_notice(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.frame(vec![]);
    h.app.alert("系统音量","音量 60%",5,false);h.frame(vec![]);
    h.bt=vec![crate::bt::Device{id:"keyboard".into(),name:"Keyboard".into(),battery:None,kind:crate::bt::DeviceKind::Keyboard}];h.frame(vec![]);
    assert_eq!(h.app.notice.as_ref().map(|a|a.title.as_str()),Some("系统音量"));
    assert!(h.app.alerts.iter().any(|a|a.title=="蓝牙设备"));
}

#[test]
fn bluetooth_notices_track_multiple_devices_independently(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    let audio=crate::bt::Device{id:"audio".into(),name:"Shokz".into(),battery:None,kind:crate::bt::DeviceKind::Bone};
    let keyboard=crate::bt::Device{id:"keyboard".into(),name:"QingLong 87".into(),battery:None,kind:crate::bt::DeviceKind::Keyboard};
    h.bt=vec![audio.clone()];h.frame(vec![]);
    h.bt.push(keyboard.clone());h.t=3.0;h.frame(vec![]);
    let notice=h.app.notice.as_ref().unwrap();
    assert_eq!(notice.body,"QingLong 87");
    assert_eq!(notice.bluetooth,Some((crate::bt::DeviceKind::Keyboard,true)));
    h.t=7.0;h.bt=vec![keyboard];h.frame(vec![]);
    let notice=h.app.notice.as_ref().unwrap();
    assert_eq!(notice.body,"Shokz · 已断开");
    assert_eq!(notice.bluetooth,Some((crate::bt::DeviceKind::Bone,false)));
}

#[test]
fn display_choice_saves_and_keeps_disconnected_selection(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings_open=true;h.ctx.set_embed_viewports(false);
    let bounds=crate::display_layout::Bounds{left:0,top:0,right:1920,bottom:1080};
    h.app.displays=vec![
        crate::display_layout::Display{id:"internal".into(),label:"显示器 1 · 主屏".into(),bounds,work_top:0,primary:true},
        crate::display_layout::Display{id:r"\\?\DISPLAY#external".into(),label:"显示器 2 · 外接屏".into(),bounds,work_top:0,primary:false},
    ];
    let mut raster=crate::preview_raster::Raster::new(690,560);
    fn frame(h:&mut H,raster:&mut crate::preview_raster::Raster,events:Vec<egui::Event>){
        h.t+=1.0/60.0;let time=h.t;
        let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;
            *capture.lock().unwrap()=Some(ctx.run(egui::RawInput{time:Some(time),viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,560.0))),events:events.clone(),viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.0),..Default::default()})).collect(),..Default::default()},|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        let root=h.ctx.run(egui::RawInput{time:Some(time),..Default::default()},|ctx|h.app.settings_ui(ctx,false));let child=output.lock().unwrap().take().unwrap();
        raster.update_textures(&root.textures_delta);raster.update_textures(&child.textures_delta);h.shapes=child.shapes;
    }
    // 设置页内容超过一屏时先向下滚动，直到目标完整出现在窗口内。
    fn reveal(h:&mut H,raster:&mut crate::preview_raster::Raster,label:&str)->Option<Rect>{
        let find=|h:&H|h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==label=>Some(t.visual_bounding_rect()),_=>None}).filter(|r|r.bottom()<550.0);
        for _ in 0..20 {
            if let Some(r)=find(h){return Some(r);}
            frame(h,raster,vec![egui::Event::PointerMoved(Pos2::new(450.0,300.0)),egui::Event::MouseWheel{unit:egui::MouseWheelUnit::Point,delta:Vec2::new(0.0,-60.0),modifiers:Default::default()}]);
            for _ in 0..6{frame(h,raster,vec![]);}
        }
        find(h)
    }
    fn click(h:&mut H,raster:&mut crate::preview_raster::Raster,label:&str){
        let pos=reveal(h,raster,label).map(|r|r.center()).unwrap_or_else(||panic!("未找到：{label}"));
        for pressed in [true,false]{frame(h,raster,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);}
        frame(h,raster,vec![]);
    }
    for _ in 0..3{frame(&mut h,&mut raster,vec![]);}
    assert!(h.app.settings.display_id.is_empty());click(&mut h,&mut raster,"跟随主屏");
    for _ in 0..20{frame(&mut h,&mut raster,vec![]);}
    if let Some(path)=std::env::var_os("DAOZAI_DISPLAY_PREVIEW"){
        raster.buf.fill([0.0,0.0,0.0,1.0]);raster.draw(&h.ctx.tessellate(h.shapes.clone(),1.0),1.0);
        let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(path,&rgba,690,560,image::ColorType::Rgba8).unwrap();
    }
    click(&mut h,&mut raster,"显示器 2 · 外接屏");assert_eq!(h.app.settings.display_id,r"\\?\DISPLAY#external");
    assert_eq!(crate::model::Settings::load().display_id,h.app.settings.display_id);
    h.app.displays.pop();frame(&mut h,&mut raster,vec![]);
    reveal(&mut h,&mut raster,"所选屏幕未连接");
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="所选屏幕未连接")));
    assert_eq!(crate::display_layout::selected(&h.app.displays,&h.app.settings.display_id).unwrap().id,"internal");
    click(&mut h,&mut raster,"所选屏幕未连接");click(&mut h,&mut raster,"跟随主屏");
    assert!(crate::model::Settings::load().display_id.is_empty());
}

#[test]
fn closing_app_notification_clears_its_bell_and_keeps_queued_messages(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for lyrics in [false,true]{
        let mut h=H::new();h.app.settings.island_lyrics=lyrics;h.app.settings.read_notifications=true;h.app.settings.notification_preview=true;
        if lyrics {
            h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,..Default::default()};
            h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]晚风吹过了山岗"));
        }
        for _ in 0..60{h.frame(vec![]);}
        h.app.receive_notification("测试消息".into(),"notice.test".into(),"第一条消息".into());
        for _ in 0..60{h.frame(vec![]);}
        let bell=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Path(p)if p.points.len()==17=>Some(p.visual_bounding_rect().center()),_=>None}).unwrap();
        let close=h.ctx.read_response(egui::Id::new(("card",true,"external-dismiss"))).unwrap().rect;
        assert!(bell.x>close.right(),"通知铃铛应位于岛内最右侧");
        h.app.receive_notification("测试消息".into(),"notice.test".into(),"第二条消息".into());
        h.click(close.center());
        assert!(h.app.notice.is_none());assert!(h.app.notification_bell_until<=h.t,"关闭正文也应结束铃铛提醒");
        h.frame(vec![]);assert_eq!(h.app.notice.as_ref().unwrap().body,"第二条消息");
        for _ in 0..30{h.frame(vec![]);}
        h.click_text("关闭");h.cursor=None;
        for _ in 0..60{h.frame(vec![]);}
        assert!(h.app.notice.is_none());assert!(h.app.hud_notice.is_none());
        assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Circle(c)if c.fill==egui::Color32::from_rgb(255,72,72))),"关闭后不能残留铃铛红点");
    }
}

#[test]
fn lyric_app_notification_respects_preview_and_restores_current_lyrics(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for preview in [false,true]{
        let mut h=H::new();h.app.settings.island_lyrics=true;h.app.settings.read_notifications=true;h.app.settings.notification_preview=preview;
        h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,..Default::default()};
        h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]首句歌词\n[00:20]后续歌词"));
        for _ in 0..60{h.frame(vec![]);}
        h.app.receive_notification("测试消息".into(),"notice.test".into(),"下午一起开会".into());
        h.frame(vec![]);let at=h.app.notice.as_ref().unwrap().at;
        let mut slopes=Vec::new();
        for elapsed in [0.35,0.60,1.0]{
            h.t=at+elapsed;h.frame(vec![]);
            let slope=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Path(p)if p.points.len()==17=>Some(p.points[13].y-p.points[16].y),_=>None}).expect("应绘制通知铃铛");
            slopes.push(slope);
        }
        assert!(slopes[0]*slopes[1]<0.0,"铃铛应向左右两个方向摆动");assert!(slopes[2].abs()<0.01,"摆动结束后应回正");
        for _ in 0..60{h.frame(vec![]);}
        let visible=|h:&H,label:&str|h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==label&&t.opacity_factor>0.9&&s.clip_rect.contains_rect(t.visual_bounding_rect())));
        assert_eq!(visible(&h,"下午一起开会"),preview);
        assert_eq!(visible(&h,"首句歌词"),!preview);
        assert_eq!(h.app.compact_notices(),!preview);
        assert!(h.region.height()<70.0,"通知应留在岛内，不能自动弹出下方卡片");
        assert!(!h.app.open);
        if !preview {assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Circle(c)if c.fill==egui::Color32::from_rgb(255,72,72))),"通知铃铛应有红点");}
        let active=h.app.notice.as_ref().unwrap();h.t=active.at+active.duration-0.1;h.media.position=25.0;
        h.frame(vec![]);assert!(h.app.notice.is_some());
        for _ in 0..90{
            h.frame(vec![]);
            if h.app.notice.is_none(){assert!(!visible(&h,"Track")&&!visible(&h,"Artist"),"恢复歌词不能闪出播放器");}
        }
        assert!(h.app.notice.is_none());assert!(!visible(&h,"下午一起开会"));assert!(visible(&h,"后续歌词"));
    }
}

#[test]
#[ignore="按需导出歌词模式下正文开关的通知画面"]
fn lyric_app_notification_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_APP_NOTICE_PREVIEW").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for preview in [false,true]{
        let mut h=H::new();h.app.settings.island_lyrics=true;h.app.settings.read_notifications=true;h.app.settings.notification_preview=preview;
        h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,..Default::default()};
        h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]晚风吹过了山岗"));
        let mut raster=crate::preview_raster::Raster::new(1440,405);
        for frame in 0..90{
            if frame==30{h.app.receive_notification("微信".into(),"wechat.preview".into(),"下午三点开会，记得带上电脑".into());}
            let t=frame as f64/60.0;
            let env=Env{t,clock:"09:41:00".into(),media:h.media.clone(),..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(1.5),..Default::default()})).collect(),..Default::default()};
            let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H)),&env);});});
            raster.update_textures(&output.textures_delta);
            if frame==89{
                raster.buf.fill([0.94,0.95,0.97,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);let image=image::RgbaImage::from_raw(1440,405,rgba).unwrap();
                image::imageops::crop_imm(&image,105,0,960,150).to_image().save(dest.join(if preview{"正文开启.png"}else{"正文关闭.png"})).unwrap();
            }
            raster.free_textures(&output.textures_delta);
        }
    }
}

#[test]
fn full_notifications_return_when_island_lyrics_are_not_visible(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for (lyrics,music) in [(false,false),(false,true),(true,false)]{
        let mut h=H::new();h.app.settings.pet=true;h.app.settings.island_lyrics=lyrics;
        if music {h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:120.0,..Default::default()};}
        for _ in 0..90{h.frame(vec![]);}h.app.alert("大写锁定","已开启",5,false);for _ in 0..60{h.frame(vec![]);}
        for label in ["大写锁定","已开启"]{assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==label&&t.opacity_factor>0.9&&s.clip_rect.contains(t.visual_bounding_rect().center()))),"非岛上歌词状态应直接显示通知文案：{label}");}
        assert!((h.app.current_width()-crate::app::HUD_W-44.0).abs()<1.0,"完整提示保留吉祥物与文案的宽度");
    }
}

#[test]
fn notification_style_follows_the_current_lyric_mode_without_a_new_notice(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]晚风吹过了山岗"));
    h.app.alert("大写锁定","已开启",5,false);
    for (lyrics,playing) in [(false,true),(true,true),(true,false),(true,true),(false,true)]{
        h.app.settings.island_lyrics=lyrics;h.media.playing=playing;for _ in 0..25{h.frame(vec![]);}
        assert_eq!(h.app.compact_notices(),lyrics&&playing);
        let has_title=h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="大写锁定"&&t.opacity_factor>0.9));
        assert_eq!(has_title,!(lyrics&&playing),"当前通知应跟随岛上歌词的显示状态切换样式");
    }
}

#[test]
#[ignore="按需导出普通模式与岛上歌词模式的通知画面"]
fn notice_mode_layout_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_NOTICE_MODES_PREVIEW").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for lyrics in [false,true]{
        let mut h=H::new();h.app.settings.pet=true;h.app.settings.island_lyrics=lyrics;h.app.pomo.running=lyrics;
        h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,..Default::default()};
        h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]晚风吹过了山岗"));
        let mut raster=crate::preview_raster::Raster::new(1920,405);
        for frame in 0..120{
            let t=frame as f64/60.0;if frame==30{h.app.alert("大写锁定","已开启",5,false);}
            let env=Env{t,clock:"09:41:00".into(),today:"2026-11-01".into(),date:"11月1日 周日".into(),media:h.media.clone(),..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(1.5),..Default::default()})).collect(),..Default::default()};
            let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H)),&env);});});
            raster.update_textures(&output.textures_delta);
            if frame==119{
                raster.buf.fill([0.94,0.95,0.97,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);let image=image::RgbaImage::from_raw(1920,405,rgba).unwrap();
                image::imageops::crop_imm(&image,105,0,960,140).to_image().save(dest.join(if lyrics{"岛上歌词模式.png"}else{"普通模式.png"})).unwrap();
            }
            raster.free_textures(&output.textures_delta);
        }
    }
}

#[test]
fn lyric_caps_lock_notice_keeps_one_glass_outline(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for skin in [crate::model::Skin::GlassLight,crate::model::Skin::GlassDark]{
    for shape in crate::model::IslandShape::ALL{for ppp in [1.0,1.25,1.5,2.0]{
        let mut h=H::new();h.app.settings.pet=false;h.app.settings.skin=skin;h.app.settings.island_shape=shape;h.app.settings.island_lyrics=true;h.app.settings.glass_blur=true;
        h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,..Default::default()};
        h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]晚风吹过了山岗"));
        let mut raster=crate::preview_raster::Raster::new((crate::app::CANVAS_W*ppp) as usize,(WIN_H*ppp) as usize);
        for frame in 0..360{
            h.t=frame as f64/60.0;
            if frame==60{h.app.alert("大写锁定","已开启",5,false);}if frame==105{h.app.alert("大写锁定","已关闭",5,false);}
            let env=Env{t:h.t,clock:"09:41:00".into(),media:h.media.clone(),backdrop:Some([0.94,0.95,0.97]),glass_blur:true,..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(h.t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
            let mut glass=None;
            let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{glass=h.app.frame(ui,Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H)),&env).glass;});});
            let contours=glass.unwrap();
            assert_eq!(contours.len(),1,"{skin:?} {shape:?} 倍率 {ppp} 第 {frame} 帧：提示与原岛应共用外轮廓，避免拼接处出现内圈");
            if shape==crate::model::IslandShape::Pill{
                let rims=output.shapes.iter().filter(|s|matches!(&s.shape,egui::Shape::Rect(r) if r.stroke.width>0.0&&r.rect.width()>50.0&&r.rect.height()>=20.0)).count();
                assert_eq!(rims,1,"胶囊连接处不能重复绘制玻璃描边");
            }
            raster.update_textures(&output.textures_delta);let primitives=h.ctx.tessellate(output.shapes,ppp);
            if ppp==1.5{if let Some(name)=match frame{45=>Some("before"),90=>Some("on"),135=>Some("off"),359=>Some("after"),_=>None}{if let Some(dest)=std::env::var_os("DAOZAI_CAPS_LOCK_PREVIEW_DIR"){
                let dest=std::path::PathBuf::from(dest);std::fs::create_dir_all(&dest).unwrap();raster.buf.fill([0.94,0.95,0.97,1.0]);raster.draw(&primitives,ppp);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);let image=image::RgbaImage::from_raw(raster.w as u32,raster.h as u32,rgba).unwrap();
                image::imageops::crop_imm(&image,(140.0*ppp) as u32,0,(440.0*ppp) as u32,(80.0*ppp) as u32).to_image().save(dest.join(format!("{}-{}-{name}.png",skin.key(),shape.key()))).unwrap();
            }}}
            raster.free_textures(&output.textures_delta);
        }
        assert!(h.app.notice.is_none());assert!((h.app.current_width()-crate::app::LYRIC_BAR_W).abs()<1.0,"提示结束后应恢复原有宽度");
    }}}
}

#[test]
fn lyric_notifications_extend_right_and_keep_the_lyrics_in_place(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let text=|h:&H,label:&str|h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==label&&t.opacity_factor>0.9&&s.clip_rect.contains(t.visual_bounding_rect().center())=>Some(t.visual_bounding_rect()),_=>None}).unwrap_or_else(||panic!("应保留可见内容：{label}"));
        let mut h=H::new();h.app.settings.pet=true;h.app.settings.island_lyrics=true;h.power=(Some(75),false);
        h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,..Default::default()};
        h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]晚风吹过了山岗\n[01:00]下一句"));
        for _ in 0..90{h.frame(vec![]);}
        let label="晚风吹过了山岗";let original=text(&h,label);let right=WIN_W/2.0+h.app.current_width()/2.0;
        h.app.alert("大写锁定","已开启",5,false);
        for _ in 0..60{h.frame(vec![]);assert!(text(&h,label).center().distance(original.center())<0.2,"提示不能挤动或替换原内容");}
        assert!((h.app.current_width()-(right-WIN_W/2.0)*2.0-44.0).abs()<1.0,"通知图标占用 44 点宽度");
        assert!(h.region.right()<right+64.0);assert!(!h.app.open);
        h.t+=3.0;for _ in 0..100{h.frame(vec![]);}
        assert!(text(&h,label).center().distance(original.center())<0.2);
}

#[test]
fn an_open_page_and_lyrics_can_coexist_with_notifications_and_the_bell(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]晚风吹过了山岗\n[01:00]下一句"));
    for _ in 0..90{h.frame(vec![]);}
    h.app.notification_apps.items.push(("mail.test".into(),"邮件".into(),true));h.app.settings.read_notifications=true;h.app.receive_notification("邮件".into(),"mail.test".into(),"收到新邮件".into());h.frame(vec![]);
    assert!(h.app.island_lyric().is_some(),"通知铃铛不能关闭岛上歌词");
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.hover(H::local(300.0,60.0),0.5);assert!(h.app.open);
    h.app.alert("键盘输入语言","中文",5,false);for _ in 0..60{h.frame(vec![]);}
    assert!(h.app.open);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="Track")),"展开页面保留歌曲信息");
    assert!(!h.app.compact_notices(),"展开页面时不使用岛上歌词的小图标提示");
}

#[test]
#[ignore="按需录制右侧通知与原岛内容同时显示的实际画面"]
fn right_notice_preview(){
    use std::io::Write;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_RIGHT_NOTICE_PREVIEW").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    h.app.settings.pet=true;h.app.settings.island_lyrics=true;h.app.pomo.running=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:120.0,..Default::default()};
    h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]晚风吹过了山岗\n[01:00]下一句"));
    let mut video=std::process::Command::new(std::env::var_os("DAOZAI_FFMPEG").unwrap_or_else(||"ffmpeg".into())).args(["-y","-loglevel","error","-f","rawvideo","-pix_fmt","rgba","-s","1920x405","-r","30","-i","-","-vf","crop=1440:210:105:0","-c:v","libx264","-crf","19","-pix_fmt","yuv420p"]).arg(dest.join("右侧通知演示.mp4")).stdin(std::process::Stdio::piped()).spawn().unwrap();
    let mut pipe=video.stdin.take().unwrap();let mut raster=crate::preview_raster::Raster::new(1920,405);
    for frame in 0..420{
        let t=frame as f64/30.0;
        if frame==90{h.app.alert("大写锁定","已开启",5,false);}
        if frame==240{h.app.alert("蓝牙设备","OpenMove by Shokz · 电量 80%",5,false);}
        let env=Env{t,clock:"09:41:00".into(),today:"2026-11-01".into(),date:"11月1日 周日".into(),media:h.media.clone(),..Default::default()};
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(1.5),..Default::default()})).collect(),..Default::default()};
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H)),&env);});});
        raster.update_textures(&output.textures_delta);raster.buf.fill([0.94,0.95,0.97,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
        let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);pipe.write_all(&rgba).unwrap();
        if let Some(name)=match frame{60=>Some("原岛"),125=>Some("大小写提示"),270=>Some("蓝牙提示"),390=>Some("恢复原岛"),_=>None}{let image=image::RgbaImage::from_raw(1920,405,rgba).unwrap();image::imageops::crop_imm(&image,105,0,1440,210).to_image().save(dest.join(format!("{name}.png"))).unwrap();}
        raster.free_textures(&output.textures_delta);
    }
    drop(pipe);assert!(video.wait().unwrap().success());
}

#[test]
fn hud_expands_smoothly_from_collapsed_width(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    for _ in 0..60 { h.frame(vec![]); }
    let before=h.app.current_width();
    h.app.alert("大写锁定","已开启",5,false);
    h.frame(vec![]);
    assert!(h.app.current_width()-before < 30.0, "提示条不能在第一帧突然跳到目标宽度");
}

#[test]
fn bluetooth_notification_keeps_its_full_message_outside_lyric_mode(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();for _ in 0..90 {h.frame(vec![]);}
    h.app.alert("蓝牙设备","OpenMove by Shokz · 电量 80%",5,false);
    for _ in 0..60 {h.frame(vec![]);}
    assert!((h.app.current_width()-crate::app::HUD_W-h.app.pet_space()).abs()<1.0);
    assert!(h.region.height()>60.0,"两行蓝牙提示保持完整高度");
    h.click_notice_badge();
    for label in ["OpenMove by Shokz","已连接","80%"]{assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==label)),"点击后可查看设备信息：{label}");}
}

#[test]
fn hud_repeated_volume_changes_do_not_restart_entrance(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.frame(vec![]);
    h.app.alert("系统音量","音量 20%",5,false);
    for _ in 0..45 {h.frame(vec![]);}
    let before=h.app.current_width();
    h.app.alert("系统音量","音量 30%",5,false);
    h.frame(vec![]);
    assert!((h.app.current_width()-before).abs()<5.0);
    assert_eq!(h.app.notice.as_ref().unwrap().body,"音量 30%");
}

#[test]
fn hud_shrinks_smoothly_after_expiration(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    for _ in 0..90 {h.frame(vec![]);}let collapsed=h.app.current_width();
    h.app.alert("大写锁定","已开启",5,false);
    for _ in 0..60 {h.frame(vec![]);}
    let before=h.app.current_width();
    h.t+=2.0;
    h.frame(vec![]);
    assert!(h.app.notice.is_none());
    assert!(h.app.current_width()>collapsed+5.0);
    assert!(h.app.current_width()<before);
}

#[test]
fn bluetooth_badge_follows_connect_and_disconnect(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    // 首次扫描已连接的设备，记录但不作为新连接提醒。
    h.bt=vec![crate::bt::Device{id:"buds".into(),name:"Buds".into(),battery:Some(80),kind:crate::bt::DeviceKind::Earbuds}];h.frame(vec![]);
    assert!(h.app.notice.is_none());
    assert_eq!(h.app.bt_shown.first().map(|d|d.name.as_str()),Some("Buds"));
    // 之后断开、再连上，各弹一条 HUD 通知。
    h.t=20.0;h.bt.clear();h.frame(vec![]);
    assert_eq!(h.app.notice.as_ref().map(|a|a.body.as_str()),Some("Buds · 已断开"));
    assert!(h.app.bt_shown.is_empty());
    h.t=30.0;h.bt=vec![crate::bt::Device{id:"buds".into(),name:"Buds".into(),battery:Some(80),kind:crate::bt::DeviceKind::Earbuds}];h.frame(vec![]);
    assert_eq!(h.app.notice.as_ref().map(|a|a.body.as_str()),Some("Buds · 电量 80%"));
    // 没有设备时不留残留状态。
    h.t=40.0;h.bt.clear();h.frame(vec![]);
    assert!(h.app.bt_shown.is_empty()&&h.app.notice.as_ref().is_none_or(|a|a.title!="蓝牙设备"||a.body=="Buds · 已断开"));
}

#[test]
fn focus_settings_are_inside_focus_page(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);h.app.go(1,1.0);h.hover(H::local(300.0,60.0),0.8);h.click(H::local(110.0,18.0));assert_eq!(h.app.page,1);assert_eq!(h.app.subpage,Some(1));
}

#[test]
fn calendar_notice_opens_the_calendar_instead_of_mail() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    assert!(h.app.settings.calendar_auto_sync);
    h.app.settings.calendar_auto_sync=false;
    h.app.settings.save();
    assert!(!crate::model::Settings::load().calendar_auto_sync,"自动同步选择应持久保存");
    let start=chrono::Local::now().timestamp()+600;
    h.app.calendar.config=Some(crate::calendar::Config{provider:crate::calendar::Provider::Feishu,username:"demo".into(),href:None,server:None});
    h.app.calendar_event(crate::calendar::Event::Synced{config:h.app.calendar.config.clone().unwrap(),events:vec![crate::mail::Meeting{ all_day:false, end:None,title:"产品评审".into(),start,at:"10-31 15:00".into(),uid:"calendar-review".into()}],at:"10-31 14:50".into()});
    for _ in 0..60 {h.frame(vec![]);}
    h.click_notice_badge();
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="查看")),"会议提醒按钮使用「查看」");
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="产品评审")),"会议名称应独立显示");
    h.click_text("查看");
    assert!(!h.app.settings_open,"会议提醒不应打开设置页");
    assert_eq!(h.app.page,7,"查看会议必须展开岛上的日程页");
    assert!(h.app.open);
    let date=chrono::DateTime::from_timestamp(start,0).unwrap().with_timezone(&chrono::Local).date_naive();
    assert_eq!(h.app.calendar.view.day,date);
    assert_eq!(h.app.calendar.view.focus.as_deref(),Some(crate::calendar::dedupe_key(&h.app.calendar.events[0])).as_deref());
    assert_ne!(h.app.page,5);
}

#[test]
fn mail_meeting_notice_opens_its_calendar_day() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.mail.recent.push(crate::mail::Item {received_unix: None, account:"demo@example.com".into(),from:"项目组".into(),subject:"会议邀请".into(),html: String::new().into(), summary:String::new().into(),links:vec![],code:None,time:"14:50".into(),uid:9,
        meeting:Some(crate::mail::Meeting{ all_day:false, end:None,title:"产品评审".into(),start:chrono::Local::now().timestamp()+600,at:"10-31 15:00".into(),uid:"mail-review".into()})});
    for _ in 0..60 {h.frame(vec![]);}
    h.click_text("查看");
    assert_eq!(h.app.page,7);
    assert!(h.app.open);
    assert_ne!(h.app.page,5);
    let entries=h.app.calendar_entries(h.app.calendar.view.day);
    assert_eq!(entries.len(),1);
    assert!(matches!(entries[0].sources[0],crate::calendar_ui::Source::Mail("demo@example.com")));
    assert!(!h.app.settings_open);
}

#[test]
fn calendar_merges_matching_invites_and_preserves_each_source() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let start=chrono::Local::now().timestamp()+600;
    let event=crate::mail::Meeting{ all_day:false, end:None,title:"产品评审".into(),start,at:String::new(),uid:"shared-review".into()};
    h.app.calendar.events=vec![event.clone()];h.app.calendar.view.events=vec![event.clone()];
    for account in ["work@example.com","personal@example.com"] {
        h.app.mail.recent.push(crate::mail::Item {received_unix: None, account:account.into(),from:"项目组".into(),subject:"会议邀请".into(),html: String::new().into(), summary:String::new().into(),links:vec![],code:None,time:String::new(),uid:9,meeting:Some(event.clone())});
    }
    let day=chrono::DateTime::from_timestamp(start,0).unwrap().with_timezone(&chrono::Local).date_naive();
    for provider in crate::calendar::Provider::ALL {
        h.app.calendar.config=Some(crate::calendar::Config{provider,username:"demo".into(),href:None,server:None});
        let entries=h.app.calendar_entries(day);
        assert_eq!(entries.len(),1,"同一场会议只占一行");
        assert_eq!(entries[0].sources.len(),3,"工作日历及两个邮箱应保留各自来源");
        assert!(matches!(entries[0].sources[0],crate::calendar_ui::Source::Calendar(Some(p)) if p==provider));
    }
    h.app.calendar.events.clear();h.app.calendar.view.events.clear();
    for (i,item) in h.app.mail.recent.iter_mut().enumerate(){let event=item.meeting.as_mut().unwrap();event.uid.clear();event.title=format!("不同会议 {i}");}
    assert_eq!(h.app.calendar_entries(day).len(),2,"缺少会议编号时不能只按相同时间误合并");
}

#[test]
fn calendar_dates_are_readable_and_source_labels_do_not_take_a_second_line() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let day=chrono::NaiveDate::from_ymd_opt(2026,11,15).unwrap();
    let event=crate::mail::Meeting{ all_day:false, end:None,title:"产品评审".into(),start:day.and_hms_opt(14,0,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap().timestamp(),at:"11-15 14:00".into(),uid:"readable-calendar".into()};
    h.app.calendar.config=Some(crate::calendar::Config{provider:crate::calendar::Provider::Feishu,username:"demo".into(),href:None,server:None});
    h.app.calendar.view.events=vec![event.clone()];
    h.app.mail.recent.push(crate::mail::Item {received_unix: None, account:"work@example.com".into(),from:"项目组".into(),subject:"会议邀请".into(),html: String::new().into(), summary:String::new().into(),links:vec![],code:None,time:String::new(),uid:1,meeting:Some(event.clone())});
    h.app.open_calendar(Some(event));h.hover(H::local(300.0,40.0),1.0);
    let date=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()=="15"=>Some(t),_=>None}).expect("月历应显示日期");
    assert!(date.galley.job.sections[0].format.font_id.size>=10.0,"日期数字需要明显放大");
    let date_rect=|label:&str|h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()==label=>Some(t.visual_bounding_rect()),_=>None}).expect("日期应可见");
    let (ten,eleven,seventeen)=(date_rect("10"),date_rect("11"),date_rect("17"));
    assert!(eleven.left()-ten.right()>=6.0,"相邻日期需要横向留白：{ten:?} {eleven:?}");
    assert!(seventeen.top()-ten.bottom()>=7.0,"相邻周需要纵向留白：{ten:?} {seventeen:?}");
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="飞书" || t.galley.text().starts_with("邮件 · "))),"来源应使用标题前的 Logo，详细文字保留在悬停提示中");
}

#[test]
fn calendar_first_schedule_has_a_large_time_and_keeps_the_lower_list_scrollable() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let day=chrono::NaiveDate::from_ymd_opt(2026,10,28).unwrap();
    let start=day.and_hms_opt(10,0,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap().timestamp();
    h.app.calendar.view.day=day;h.app.calendar.view.month=crate::calendar::month_start(day);h.app.calendar.view.loaded=Some(h.app.calendar.view.month);
    h.app.calendar.view.events=(0..7).map(|i|crate::mail::Meeting{ all_day:false,title:format!("会议 {i}"),start:start+i*3600,end:Some(start+i*3600+2700),at:String::new(),uid:format!("layout-{i}")}).collect();
    h.app.open_calendar(None);h.hover(H::local(300.0,40.0),1.0);
    let text=|label:&str|h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()==label=>Some(t),_=>None}).unwrap();
    assert!(text("10:00").galley.job.sections[0].format.font_id.size>=23.0);
    assert!(text("会议 0").galley.job.sections[0].format.font_id.size>text("会议 1").galley.job.sections[0].format.font_id.size);
    assert!(text("10:45 结束").visual_bounding_rect().bottom()<text("会议 1").visual_bounding_rect().top());
    assert!(text("会议 4").visual_bounding_rect().bottom()<H::local(0.0,205.0).y);
    h.hover(H::local(300.0,194.0),0.25);h.wheel(-40.0);
    assert_eq!(h.app.calendar.view.first,1,"列表底部也应滚动日程");assert_eq!(h.app.page,7);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="会议 0")),"滚动不能切换大卡片");
    h.click(H::local(260.0,138.0));h.hover(H::local(300.0,194.0),0.2);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="12:00" && t.galley.job.sections[0].format.font_id.size>=23.0)),"点击才切换大卡片");
    h.wheel(-40.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="12:00" && t.galley.job.sections[0].format.font_id.size>=23.0)),"选中后滚动仍保持详情");
    h.app.open_calendar(Some(h.app.calendar.view.events[6].clone()));h.hover(H::local(300.0,40.0),0.3);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="会议 6")),"提醒定位的末条日程应完整可见");
}

#[test]
fn calendar_month_navigation_and_today_select_the_right_day() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.open_calendar(None);
    h.app.select_calendar_day(chrono::NaiveDate::from_ymd_opt(2026,12,31).unwrap());
    h.hover(H::local(300.0,60.0),1.0);
    h.click(H::local(129.0,80.0));
    assert_eq!(h.app.calendar.view.month,chrono::NaiveDate::from_ymd_opt(2027,1,1).unwrap());
    let point=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()=="15"=>Some(t.visual_bounding_rect().center()),_=>None}).expect("日期应显示在岛上");
    h.click(point);
    assert_eq!(h.app.calendar.view.day,chrono::NaiveDate::from_ymd_opt(2027,1,15).unwrap());
    h.click(H::local(67.0,18.0));
    assert_eq!(h.app.calendar.view.day,chrono::NaiveDate::from_ymd_opt(2026,10,28).unwrap());
    assert_eq!(h.app.page,7);assert!(!h.app.settings_open);
}

#[test]
fn calendar_month_results_preserve_reminders_and_ignore_old_requests() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.calendar.config=Some(crate::calendar::Config{provider:crate::calendar::Provider::Feishu,username:"demo".into(),href:None,server:None});
    let upcoming=crate::mail::Meeting{ all_day:false, end:None,title:"明天会议".into(),start:chrono::Local::now().timestamp()+3600,at:String::new(),uid:"upcoming".into()};
    h.app.calendar.events=vec![upcoming.clone()];
    let month=chrono::NaiveDate::from_ymd_opt(2024,2,1).unwrap();
    h.app.select_calendar_day(month);
    let config=h.app.calendar.config.clone().unwrap();
    h.app.calendar_event(crate::calendar::Event::MonthLoaded{config:config.clone(),month,result:Ok(vec![])});
    assert_eq!(h.app.calendar.events,vec![upcoming]);
    assert_eq!(h.app.calendar.view.loaded,Some(month));
    let next=chrono::NaiveDate::from_ymd_opt(2024,3,1).unwrap();h.app.select_calendar_day(next);
    h.app.calendar_event(crate::calendar::Event::MonthLoaded{config:config.clone(),month,result:Ok(vec![])});
    assert!(h.app.calendar.view.loading,"旧请求不能结束新月份的加载");
    let mut other=config.clone();other.username="another-account".into();
    h.app.calendar_event(crate::calendar::Event::MonthLoaded{config:other,month:next,result:Ok(vec![])});
    assert!(h.app.calendar.view.loading,"其他账号的旧结果应被忽略");
    h.app.calendar_event(crate::calendar::Event::MonthLoaded{config,month:next,result:Err("连接暂不可用".into())});
    assert!(!h.app.calendar.view.loading);assert!(!h.app.calendar.view.error.is_empty());
}

#[test]
fn calendar_island_collapses_and_reopens_on_the_same_page() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.open_calendar(None);h.hover(H::local(300.0,60.0),1.0);
    assert_eq!(h.app.page,7);assert!(h.app.open);
    assert!(h.hit.bottom()>=crate::app::CALENDAR_H,"月历最后一行必须位于可操作区域内");
    h.hover(Pos2::new(600.0,WIN_H+30.0),5.0);assert!(!h.app.open);
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);assert!(h.app.open);
    assert_eq!(h.app.page,7);assert!(!h.app.settings_open);
}

#[test]
#[ignore="按需导出岛内日程的实际渲染画面"]
fn calendar_layout_gallery() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_CALENDAR_PREVIEW_DIR").expect("需指定预览目录"));
    std::fs::create_dir_all(&dest).unwrap();
    for mode in ["agenda","dingtalk","wecom","empty","loading","failed","disconnected","local-editor","local","local-all-day","mixed"] {
        let mut h=H::new();
        let day=chrono::NaiveDate::from_ymd_opt(2026,11,2).unwrap();
        h.app.calendar.view.day=day;h.app.calendar.view.month=crate::calendar::month_start(day);
        h.app.open_calendar(None);
        let provider=match mode{"dingtalk"=>crate::calendar::Provider::DingTalk,"wecom"=>crate::calendar::Provider::WeCom,_=>crate::calendar::Provider::Feishu};
        if !["disconnected","local-editor","local","local-all-day"].contains(&mode){h.app.calendar.config=Some(crate::calendar::Config{provider,username:"demo".into(),href:None,server:None});h.app.calendar.connected=true;}
        h.app.calendar.view.loaded=Some(crate::calendar::month_start(day));
        if ["agenda","dingtalk","wecom","mixed"].contains(&mode) {
            for (i,(hour,minute,title)) in [(14,30,"产品方案评审"),(16,0,"下一阶段的设计方案与开发排期对齐"),(17,0,"项目进度同步")].into_iter().enumerate(){
                let at=day.and_hms_opt(hour,minute,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap();
                h.app.calendar.view.events.push(crate::mail::Meeting{ all_day:false,end:Some(at.timestamp()+2700),title:title.into(),start:at.timestamp(),at:at.format("%m-%d %H:%M").to_string(),uid:format!("sample-{i}")});
            }
            for d in [4,9,12,18,20,25] {let date=chrono::NaiveDate::from_ymd_opt(2026,11,d).unwrap();h.app.calendar.view.events.push(crate::mail::Meeting{ all_day:false, end:None,title:"项目安排".into(),start:date.and_hms_opt(10,0,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap().timestamp(),at:String::new(),uid:format!("other-{d}")});}
            for (i,(hour,title,account)) in [(15,"客户沟通：合同条款确认","work@example.com"),(16,"交付验收会议","team@example.com")].into_iter().enumerate(){
                let at=day.and_hms_opt(hour,30,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap();
                h.app.mail.recent.push(crate::mail::Item {received_unix: None, account:account.into(),from:"项目组".into(),subject:"会议邀请".into(),html: String::new().into(), summary:String::new().into(),links:vec![],code:None,time:String::new(),uid:i as u32,
                    meeting:Some(crate::mail::Meeting{ all_day:false, end:None,title:title.into(),start:at.timestamp(),at:at.format("%m-%d %H:%M").to_string(),uid:format!("mail-{i}")})});
            }
            h.app.mail.recent.push(crate::mail::Item {received_unix: None, account:"work@example.com".into(),from:"项目组".into(),subject:"会议邀请".into(),html: String::new().into(), summary:String::new().into(),links:vec![],code:None,time:String::new(),uid:3,meeting:Some(h.app.calendar.view.events[1].clone())});
        }
        if ["local","local-all-day","mixed"].contains(&mode){
            let mut draft=crate::local_calendar::Draft::new(day);draft.title="散步与读书".into();draft.start_time="15:00".into();draft.end_time="16:00".into();
            draft.all_day=mode!="local";h.app.calendar.local.push(draft.entry().unwrap());
        }
        if mode=="mixed" {
            let start=day.and_hms_opt(9,0,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap().timestamp();
            h.app.calendar.view.events.push(crate::mail::Meeting{all_day:false,title:"早上的会议".into(),start,end:Some(start+3600),at:String::new(),uid:"past".into()});
        }
        if mode=="local-editor"{h.app.open_calendar_editor(None);}
        if mode=="loading"{h.app.calendar.view.loaded=None;h.app.calendar.view.loading=true;}
        if mode=="failed"{h.app.calendar.view.loaded=None;h.app.calendar.view.error="连不上服务器，检查网络".into();}
        let mut raster=crate::preview_raster::Raster::new(1280,600);
        for f in 0..90 {
            let t=f as f64/60.0;
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(WIN_W,WIN_H))),time:Some(t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
            let env=Env{t,today:"2026-11-02".into(),date:"11月2日 周一".into(),clock:"14:15:00".into(),battery:Some(76),charging:true,cursor:Some(H::local(300.0,40.0)),..Default::default()};
            let out=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{let win=ui.max_rect();h.app.frame(ui,win,&env);});});
            raster.update_textures(&out.textures_delta);
            if f==89 {
                if mode=="local-all-day" {
                    assert!(out.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(t) if t.galley.text()=="散步与读书")),"只有全天日程时也必须显示普通日程行");
                }
                raster.buf.fill([0.035,0.055,0.09,1.0]);raster.draw(&h.ctx.tessellate(out.shapes,out.pixels_per_point),out.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);let image=image::RgbaImage::from_raw(1280,600,rgba).unwrap();
                image::imageops::crop_imm(&image,180,0,920,510).to_image().save(dest.join(format!("calendar-{mode}.png"))).unwrap();
            }
            raster.free_textures(&out.textures_delta);
        }
    }
}

#[test]
fn calendar_reminder_fires_once_and_dedupes_with_mail() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = H::new();
    h.app.mail.accounts=vec![crate::mail::Account {custom: None, email:"a@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
    h.app.calendar.config=Some(crate::calendar::Config{provider:crate::calendar::Provider::Feishu,username:"demo".into(),href:None,server:None});
    h.app.settings.calendar_prompt = false;
    let start = chrono::Local::now().timestamp() + 600;
    let at = "10-30 10:00".to_string();
    let meeting = |title: &str| crate::mail::Meeting { all_day:false, end:None, title: title.into(), start, at: at.clone(), uid: "evt-shared-1".into() };
    // 邮件里先来一份，日历再来同一个 UID + DTSTART：只弹一次。
    h.app.mail_event(crate::mail::Event::Mail {
        item: crate::mail::Item {
            received_unix: None, account: "a@qq.com".into(),
            from: "腾讯企业邮".into(),
            subject: "周会".into(),
            time: "09:41".into(),
            code: None,
            html: String::new().into(), summary: String::new().into(),
            links: vec![],
            meeting: Some(meeting("周会")),
            uid: 7,
        },
        new: true,
    });
    h.app.calendar_event(crate::calendar::Event::Synced {
        config:h.app.calendar.config.clone().unwrap(),
        events: vec![meeting("周会（飞书）")],
        at: "10-30 09:00".into(),
    });
    h.frame(vec![]);
    h.frame(vec![]);
    let hits = h.app.history.iter().filter(|a| a.title == "会议提醒").count();
    assert_eq!(hits, 1, "同一场会只提醒一次");
    assert_eq!(h.app.history.iter().find(|a|a.title=="会议提醒").unwrap().page,7,"会议提醒应指向岛内日程页");
    // 再同步一次、再过几帧，也不重复。
    h.app.calendar_event(crate::calendar::Event::Synced { config:h.app.calendar.config.clone().unwrap(),events: vec![meeting("周会（飞书）")], at: "10-30 09:05".into() });
    h.frame(vec![]);
    assert_eq!(h.app.history.iter().filter(|a| a.title == "会议提醒").count(), 1);
}

#[test]
fn calendar_setup_does_not_overlay_music() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = H::new();
    h.app.settings.calendar_prompt = true;
    h.app.calendar.config = None;
    h.hover(Pos2::new(WIN_W / 2.0, 12.0), 1.0);
    h.hover(H::local(300.0, 60.0), 0.6);
    h.click(H::local(40.0, 16.0));
    assert!(!h.app.settings_open, "音乐页不应出现日历接入悬浮入口");
    h.app.open_settings_category(3);
    assert!(h.app.settings_open, "仍可从设置访问日程");
    assert_eq!(h.app.settings_tab, 6, "日程位于通知、邮件与日程分组");
}

#[test]
fn language_notice_text_is_vertically_centered() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.frame(vec![]);
    h.app.alert("键盘输入语言","英语",5,false);
    for _ in 0..60 {h.frame(vec![]);}
    h.click_notice_badge();
    fn find(shapes:&[egui::epaint::ClippedShape])->Option<Rect>{
        shapes.iter().find_map(|s|match &s.shape {
            egui::Shape::Text(text) if text.galley.text()=="英语"=>Some(text.visual_bounding_rect()),
            _=>None,
        })
    }
    let text=find(&h.shapes).expect("输入语言状态应可见");
    assert!((text.center().y-19.0).abs()<1.0,"文字应与图标垂直居中：{text:?}");
}

#[test]
fn todo_notice_view_locates_the_reminded_task() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.todos.items=(0..7).map(|i|crate::model::TodoItem{text:format!("任务 {i}"),done_on:None,due:if i==6{Some("2026-10-28 09:40".into())}else{None},reminded:false,..Default::default()}).collect();
    for _ in 0..60 {h.frame(vec![]);}
    h.click_text("查看");
    assert_eq!(h.app.page,4);
    assert!(h.app.todo_first>=4,"打开待办页时应能看到收到提醒的任务");
}

#[test]
fn mail_hyperlink_click_requests_the_default_browser() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.mail.accounts=vec![crate::mail::Account {custom: None, email:"a@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
    h.app.mail.recent=vec![crate::mail::Item {received_unix: None, account:"a@qq.com".into(),from:"产品团队".into(),subject:"评审文档".into(),time:"09:41".into(),code:None,html: String::new().into(), summary:"请查看以下文档。".into(),links:vec![crate::mail::Link{label:"查看文档".into(),url:"https://example.com/review?a=1&b=2".into()}],meeting:None,uid:10}];
    h.app.mail.open_uid=Some(("a@qq.com".into(),10));h.app.show_page(5);
    h.hover(H::local(300.0,60.0),1.0);
    let link=h.shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Text(text)if text.galley.text().contains("查看文档")=>Some(text.visual_bounding_rect()),_=>None}).expect("应展示邮件的超链接");
    h.click(link.center());
    assert!(h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::OpenUrl(url)if url=="https://example.com/review?a=1&b=2")),"点击后应通过默认浏览器打开完整地址");
}

#[test]
fn new_notification_sways_twice_then_stops_and_restarts_on_the_next_one() {
    for (i,expected_positive) in [true,false,true,false].into_iter().enumerate(){
        let angle=crate::paint::bell_swing(0.95*(i as f64*0.25+0.125));
        assert_eq!(angle>0.0,expected_positive);
    }
    assert_eq!(crate::paint::bell_swing(1.0),0.0);
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.settings.read_notifications=true;h.app.receive_notification("测试通知".into(),"test.notification".into(),"第一条".into());
    let first=h.app.notification_bell_at;
    for _ in 0..90{h.frame(vec![]);}
    h.app.settings.read_notifications=true;h.app.receive_notification("测试通知".into(),"test.notification".into(),"第二条".into());
    assert!(h.app.notification_bell_at>first+1.0);
    h.frame(vec![]);
    assert!(h.app.notification_bell_until>h.t);
}

#[test]
fn hud_titles_and_states_share_the_icon_center_without_overlapping() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for (title,body,left,right) in [
        ("网络连接","Wi-Fi · 网络已连接","Wi-Fi","网络已连接"),
        ("网络连接","以太网 · 网络已断开","以太网","网络已断开"),
        ("大写锁定","已开启","大写锁定","已开启"),
        ("键盘输入语言","中文","输入语言","中文"),
        ("电源已连接","当前电量 76%","电源已连接","当前电量 76%"),
        ("已切换电池供电","当前电量 76%","电池供电","当前电量 76%"),
    ] {
        let mut h=H::new();h.frame(vec![]);h.app.alert(title,body,5,false);
        for _ in 0..60{h.frame(vec![]);}
        h.click_notice_badge();
        let find=|needle:&str|h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==needle=>Some(t.visual_bounding_rect()),_=>None}).unwrap_or_else(||panic!("未显示状态文字：{needle}"));
        let l=find(left);let r=find(right);
        assert!((l.center().y-19.0).abs()<1.0 && (r.center().y-19.0).abs()<1.0,"{title} 应垂直居中");
        assert!(l.right()+8.0<=r.left(),"{title} 的标题和状态应留出间距");
    }
}

#[test]
fn low_power_notice_dismisses_without_opening_the_mail_page() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.frame(vec![]);h.app.alert("电量偏低","剩余 18% · 请连接电源",5,false);
    for _ in 0..60{h.frame(vec![]);}
    h.click_text("知道了");
    assert!(h.app.notice.is_none());
    assert_eq!(h.app.page,0,"低电量提示只需确认关闭");
}

#[test]
fn code_copy_confirmation_stays_below_the_island_and_does_not_capture_hover() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for detail in [false,true] {
        let mut h=H::new();
        h.app.mail.accounts=vec![crate::mail::Account {custom: None, email:"a@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
        h.app.mail.recent=vec![crate::mail::Item {received_unix: None, account:"a@qq.com".into(),from:"GitHub".into(),subject:"登录验证码".into(),time:"09:41".into(),code:Some("482913".into()),html: String::new().into(), summary:"请勿向他人透露验证码。".into(),links:vec![],meeting:None,uid:42}];
        for item in h.app.mail.recent.clone(){h.app.mail.privacy.observe(&item,true,h.app.t,chrono::Utc::now().timestamp());}
        if detail {h.app.mail.open_uid=Some(("a@qq.com".into(),42));}
        h.app.show_page(5);h.hover(H::local(300.0,60.0),4.2);
        let copy=H::local(80.0,if detail{212.0}else{70.0});
        h.click(copy);h.hover(copy,0.35);
        let confirmation=|h:&H|h.shapes.iter().find_map(|shape|match &shape.shape {
            egui::Shape::Text(text) if text.galley.text()=="验证码已复制"=>Some(text.visual_bounding_rect()),_=>None,
        });
        let bounds=confirmation(&h).expect("复制后应显示确认提示");
        assert!(bounds.top()>PH+4.0,"确认提示应在岛下方：{bounds:?}");
        assert!(h.region.contains_rect(bounds),"原生窗口区域应包含提示，避免被裁掉");
        assert!(!h.hit.intersects(bounds),"确认提示不应拦截鼠标或维持岛的展开");
        h.hover(Pos2::new(600.0,180.0),0.8);
        assert!(h.app.progress()<0.02);
        assert!(confirmation(&h).is_some(),"岛收起后确认提示仍应显示到正常结束");
        h.hover(Pos2::new(600.0,180.0),2.0);
        assert!(confirmation(&h).is_none());
        assert!(h.region.bottom()<100.0,"提示结束后应释放额外窗口区域");
    }
}

#[test]
fn adding_a_todo_saves_content_then_offers_an_optional_reminder() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for remind in [false,true] {
        let mut h=H::new();h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);
        h.click(H::local(70.0,109.0));h.frame(vec![egui::Event::Text("提交产品方案".into())]);
        h.frame(vec![egui::Event::Key{key:egui::Key::Enter,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
        assert_eq!(h.app.todos.items[0].text,"提交产品方案");
        assert!(h.app.todos.items[0].due.is_none());
        assert!(h.app.task_dialog.is_some(),"保存内容后应进入可选提醒步骤");
        assert_eq!(crate::model::TodoList::load().items[0].text,"提交产品方案");
        h.frame(vec![egui::Event::Key{key:egui::Key::Enter,physical_key:None,pressed:false,repeat:false,modifiers:Default::default()}]);
        if remind {
            let initial=chrono::NaiveDateTime::parse_from_str(&h.app.task_dialog.as_ref().unwrap().picker.due(),"%Y-%m-%d %H:%M").unwrap();
            h.hover(H::local(235.0,77.0),0.4);h.wheel(-40.0);h.picker_click("完成");
            let expected=(initial+chrono::Duration::days(1)).format("%Y-%m-%d %H:%M").to_string();
            assert_eq!(h.app.todos.items[0].due.as_deref(),Some(expected.as_str()));
        }else{h.picker_click("暂不提醒");assert!(h.app.todos.items[0].due.is_none());}
        assert!(h.app.task_dialog.is_none());
        assert!(h.app.todos.items[0].done_on.is_none());
        assert_eq!(crate::model::TodoList::load().items[0].due,h.app.todos.items[0].due);
    }
}

#[test]
fn todo_clock_button_sets_a_reminder_without_completing_the_task() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.todos.items=vec![crate::model::TodoItem{text:"已有任务".into(),done_on:None,due:None,reminded:false,..Default::default()}];
    h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);h.click(H::local(343.0,46.0));
    assert!(h.app.task_dialog.is_some(),"每项待办应有可见的提醒入口");
    assert!(h.app.todos.items[0].done_on.is_none(),"时钟入口不能同时勾选任务");
    h.frame(vec![egui::Event::Key{key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
    assert!(h.app.task_dialog.is_none());assert_eq!(h.app.todos.items[0].text,"已有任务");
}

#[test]
fn mail_details_show_the_receiving_account_when_two_accounts_share_a_uid() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.mail.accounts=vec![crate::mail::Account {custom: None, email:"first@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true},crate::mail::Account {custom: None, email:"work@icloud.com".into(),provider:crate::mail::Provider::ICloud,enabled:true}];
    h.app.mail.recent=["first@qq.com","work@icloud.com"].into_iter().map(|account|crate::mail::Item {received_unix: None, account:account.into(),from:"产品团队".into(),subject:"评审安排".into(),time:"09:41".into(),code:None,html: String::new().into(), summary:"请查看会议安排。".into(),links:vec![],meeting:None,uid:42}).collect();
    h.app.show_page(5);h.hover(H::local(300.0,60.0),1.0);
    for (row,account) in ["first@qq.com","work@icloud.com"].into_iter().enumerate(){
        h.hover(H::local(280.0,64.0+row as f32*21.5),0.6);h.click(H::local(280.0,64.0+row as f32*21.5));h.hover(H::local(300.0,90.0),0.1);
        assert_eq!(h.app.mail.open_uid,Some((account.into(),42)));
        assert!(h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text)if text.galley.text()==format!("收件邮箱：{account}"))),"详情必须明确标出这封邮件所属的收件邮箱");
        h.click(H::local(350.0,48.0));
    }
}

#[test]
fn scrolling_mail_details_keeps_the_current_message_open() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.mail.accounts=vec![crate::mail::Account {custom: None, email:"work@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
    h.app.mail.recent=vec![crate::mail::Item {received_unix: None, account:"work@qq.com".into(),from:"产品团队".into(),subject:"项目方案".into(),time:"09:41".into(),code:None,html: String::new().into(), summary:"需要滚动阅读的邮件正文。".repeat(20).into(),links:vec![],meeting:None,uid:42}];
    h.app.mail.open_uid=Some(("work@qq.com".into(),42));h.app.show_page(5);h.hover(H::local(270.0,150.0),1.0);
    let body_clip=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text().starts_with("需要滚动阅读")=>Some(s.clip_rect),_=>None}).expect("正文应显示");
    assert!(body_clip.height()>=80.0,"详情正文应有足够阅读空间，而不是仅能容纳一行");
    h.frame(vec![egui::Event::MouseWheel{unit:egui::MouseWheelUnit::Point,delta:egui::vec2(0.0,-120.0),modifiers:Default::default()}]);
    assert_eq!(h.app.page(),5,"邮件正文滚动不能切换功能页");
    assert_eq!(h.app.mail.open_uid,Some(("work@qq.com".into(),42)));
}

#[test]
fn todo_reminder_picker_is_embedded_in_the_island(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.todos.items=vec![crate::model::TodoItem{text:"准备会议".into(),done_on:None,due:None,reminded:false,..Default::default()}];
    h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);
    h.ctx.set_embed_viewports(false);
    egui::Context::set_immediate_viewport_renderer(|_,_|panic!("待办提醒必须在岛内选择，不能创建独立窗口"));
    h.click(H::local(343.0,46.0));
    for _ in 0..90 {h.frame(vec![]);}
    assert_eq!(h.app.expanded_height(),PH,"右侧滚轮应保持待办页原来的高度");
    assert!(h.hit.height()<=PH+2.0,"选择时间不能撑大岛体");
    for label in ["准备会议","已完成","取消","完成","暂不提醒"] {
        let (rect,clip)=h.shapes.iter().find_map(|s|match &s.shape {
            egui::Shape::Text(t) if t.galley.text()==label=>Some((t.visual_bounding_rect(),s.clip_rect)),_=>None,
        }).unwrap_or_else(||panic!("岛内缺少选项：{label}"));
        assert!(h.hit.contains_rect(rect),"选项 {label} 应在岛体命中区域内");
        assert!(h.region.contains_rect(rect) && clip.contains_rect(rect),"选项 {label} 不能被窗口或岛体裁剪");
    }
    h.frame(vec![egui::Event::Key{key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);
    assert!(h.app.task_dialog.is_none());assert!(h.app.todos.items[0].due.is_none());
}

#[test]
fn reminder_wheels_select_date_hour_and_minute_without_typing(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let day=chrono::Local::now().date_naive()+chrono::Duration::days(2);let original=format!("{day} 09:00");
    h.app.todos.items=vec![crate::model::TodoItem{text:"准备方案".into(),due:Some(original.clone()),done_on:None,reminded:false,..Default::default()}];
    h.app.open_task_reminder(0);
    for x in [235.0,290.0,346.0] {h.hover(H::local(x,77.0),0.4);h.wheel(-40.0);}
    assert_eq!(h.app.todos.items[0].due,Some(original),"调整时不能提前写入提醒");
    h.picker_click("完成");
    let next=day+chrono::Duration::days(1);
    assert_eq!(h.app.todos.items[0].due,Some(format!("{next} 10:01")));
    assert!(h.app.task_dialog.is_none());
}

#[test]
fn reminder_wheels_support_clicks_and_dragging(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let day=chrono::Local::now().date_naive()+chrono::Duration::days(2);
    h.app.todos.items=vec![crate::model::TodoItem{text:"确认方案".into(),due:Some(format!("{day} 10:30")),done_on:None,reminded:false,..Default::default()}];
    h.app.open_task_reminder(0);h.hover(H::local(300.0,60.0),1.0);
    h.click(H::local(346.0,96.0));h.click(H::local(290.0,58.0));
    let start=H::local(235.0,77.0);h.cursor=Some(start);
    h.frame(vec![egui::Event::PointerMoved(start),egui::Event::PointerButton{pos:start,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
    for y in [59.0,41.0]{let point=H::local(235.0,y);h.cursor=Some(point);h.frame(vec![egui::Event::PointerMoved(point)]);}
    let end=H::local(235.0,41.0);
    h.frame(vec![egui::Event::PointerButton{pos:end,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
    h.picker_click("完成");
    assert_eq!(h.app.todos.items[0].due,Some(format!("{} 09:31",day+chrono::Duration::days(2))));
    assert!(h.app.todos.items[0].done_on.is_none());
}

#[test]
fn reminder_wheels_accumulate_small_trackpad_movements(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let day=chrono::Local::now().date_naive()+chrono::Duration::days(2);
    h.app.todos.items=vec![crate::model::TodoItem{text:"检查进度".into(),due:Some(format!("{day} 10:30")),done_on:None,reminded:false,..Default::default()}];
    h.app.open_task_reminder(0);h.hover(H::local(346.0,77.0),1.0);
    for _ in 0..80{h.wheel(-0.5);}
    h.picker_click("完成");
    assert_eq!(h.app.todos.items[0].due,Some(format!("{day} 10:31")));
    assert_eq!(h.app.page(),4,"操作时间滚轮不能触发翻页");
}

#[test]
fn reminder_picker_rejects_past_time_and_cancel_preserves_the_task(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let old=format!("{} 09:00",chrono::Local::now().date_naive()-chrono::Duration::days(1));
    h.app.todos.items=vec![crate::model::TodoItem{text:"保留任务".into(),due:Some(old.clone()),done_on:None,reminded:true,..Default::default()}];
    h.app.open_task_reminder(0);h.picker_click("完成");h.picker_frame(vec![]);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="请选择晚于当前的时间")));
    assert_eq!(h.app.todos.items[0].due,Some(old.clone()));
    h.picker_click("取消");assert!(h.app.task_dialog.is_none());assert_eq!(h.app.todos.items[0].due,Some(old));
}

#[test]
fn full_mail_reader_keeps_the_entire_body_and_scrolls_independently(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let body=format!("开头：这是第一段。\n\n{}\n最后一段：全文结束", "这是一段需要阅读的完整邮件正文。\n\n".repeat(200));
    let html=body.split('\n').map(|line|format!("<p>{line}</p>")).collect::<String>();
    let item=crate::mail::Item {received_unix: None, account:"work@qq.com".into(),from:"产品团队".into(),subject:"完整方案".into(),time:"09:41".into(),code:None,html:html.into(), summary:body.clone().into(),links:vec![],meeting:None,uid:42};
    h.app.mail.accounts=vec![crate::mail::Account {custom: None, email:item.account.clone(),provider:crate::mail::Provider::Qq,enabled:true}];
    h.app.mail.recent=vec![item];h.app.mail.open_uid=Some(("work@qq.com".into(),42));h.app.show_page(5);h.hover(H::local(300.0,90.0),1.0);
    h.click(H::local(293.0,48.0));assert_eq!(h.app.mail.reading.as_ref().unwrap().summary.as_ref(),body);
    h.app.mail.recent.clear();
    let render=|h:&mut H,events|{
        h.ctx.set_embed_viewports(true);h.t+=1.0/60.0;
        let out=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(700.0,620.0))),time:Some(h.t),events,focused:true,..Default::default()},|ctx|h.app.mail_reader_ui(ctx));h.shapes=out.shapes;
    };
    // 正文按块渲染：视口外的块不生成文字形状，所以「末尾可见」只能滚动到底再断言。
    let text_top=|h:&H,needle:&str|h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==needle=>Some(t.visual_bounding_rect().top()),_=>None});
    let wheel=|dy:f32|vec![egui::Event::PointerMoved(egui::pos2(350.0,300.0)),egui::Event::MouseWheel{unit:egui::MouseWheelUnit::Point,delta:egui::vec2(0.0,dy),modifiers:Default::default()}];
    render(&mut h,vec![]);render(&mut h,vec![]);
    let first=text_top(&h,"开头：这是第一段。").expect("首屏应显示正文开头");
    assert!(text_top(&h,"最后一段：全文结束").is_none(),"长邮件末尾应在首屏之外");
    for _ in 0..5{render(&mut h,wheel(-400.0));}
    let moved=text_top(&h,"开头：这是第一段。");
    assert!(moved.is_none_or(|top|top<first-100.0),"阅读窗口的滚轮应向下滚动正文");
    for _ in 0..60{render(&mut h,wheel(-400.0));}
    assert!(text_top(&h,"最后一段：全文结束").is_some(),"滚动到底应能看到邮件末尾");
    assert!(h.app.mail.reading.is_some(),"新邮件列表变化不能打断已经打开的正文");
}

#[test]
#[ignore="按需导出多格式邮件正文的实际画面"]
fn mail_reader_rich_html_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_MAIL_GALLERY_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    let mut h=H::new();h.ctx.set_embed_viewports(true);
    let html=r#"<html><head><style>body{font-family:Arial}</style></head><body>
        <div style="display:none;max-height:0;overflow:hidden">&#8203;&#8203; 这是一段只为收件箱预览存在的填充文字 &#8203;</div>
        <table role="presentation" cellpadding="0" cellspacing="0" width="100%"><tr><td><img src="https://example.com/logo.png" alt="logo"> 通知中心</td><td>在浏览器中查看</td></tr></table>
        <h1>订单已确认</h1>
        <p>您好，<strong>订单 A20261107</strong> 已支付成功，我们会在 24 小时内发货。</p>
        <p><img src="https://example.com/order.png" alt="订单物流状态截图" width="600"></p>
        <h3>订单明细</h3>
        <table><tr><th>商品</th><th>数量</th><th>金额</th></tr><tr><td>机械键盘</td><td>1</td><td>¥499</td></tr><tr><td>腕托</td><td>2</td><td>¥78</td></tr></table>
        <h3>接下来会发生什么</h3>
        <ol><li>仓库拣货并出库</li><li>快递揽收后短信通知</li><li>签收后 7 天内可申请退换</li></ol>
        <ul><li>发票为电子发票，发货后发到本邮箱</li><li>如需修改地址，请在发货前回复本邮件</li></ul>
        <blockquote>注意：本邮件由系统自动发送，请勿直接回复。</blockquote>
        <hr>
        <p>感谢支持 · <a href="https://example.com/orders">查看订单详情</a></p>
    </body></html>"#;
    h.app.mail.reading=Some(crate::mail::Item{received_unix:None,account:"reader@example.com".into(),from:"通知中心".into(),
        subject:"订单 A20261107 已确认".into(),time:"11-07 09:41".into(),code:None,
        html:html.into(),
        summary:"订单已确认 您好，订单 A20261107 已支付成功，我们会在 24 小时内发货。订单明细 商品 数量 金额 机械键盘 1 ¥499 腕托 2 ¥78".into(),
        links:vec![crate::mail::Link{label:"查看订单详情".into(),url:"https://example.com/orders".into()}],meeting:None,uid:7});
    let (w,height)=(700,620);let ppp=1.5;
    let mut raster=crate::preview_raster::Raster::new((w as f32*ppp)as usize,(height as f32*ppp)as usize);
    for f in 0..=8{
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(w as f32,height as f32))),time:Some(f as f64/60.0),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
        let out=h.ctx.run(raw,|ctx|h.app.mail_reader_ui(ctx));raster.update_textures(&out.textures_delta);
        if f==8{raster.buf.fill([0.0,0.0,0.0,1.0]);raster.draw(&h.ctx.tessellate(out.shapes,out.pixels_per_point),out.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join("mail-reader-rich.png"),&rgba,(w as f32*ppp)as u32,(height as f32*ppp)as u32,image::ColorType::Rgba8).unwrap();}
        raster.free_textures(&out.textures_delta);
    }
}

#[test]
fn mail_plain_text_keeps_literal_stars_in_the_reader(){
    let h=H::new();h.ctx.set_embed_viewports(true);
    let lines=crate::mail_format::lines("","保留 ** 原始字符");
    let out=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(700.0,620.0))),..Default::default()},|ctx|{
        egui::CentralPanel::default().show(ctx,|ui|crate::mail_reader::body_ui(ui,&lines,14.0));
    });
    assert!(out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="保留 ** 原始字符")));
}

#[test]
fn mail_detail_and_reader_keep_their_own_cached_body(){
    let mut h=H::new();
    let item=|uid,body:&str|crate::mail::Item{html:format!("<p>{body}</p>").into(),summary:body.to_string().into(),uid,
        account:"test@example.com".into(),from:String::new(),subject:String::new(),time:String::new(),code:None,received_unix:None,links:vec![],meeting:None};
    let first=item(1,"第一封邮件");let second=item(2,"第二封邮件");
    let before=h.app.mail_body_lines(&first);h.app.mail_body_lines(&second);
    let after=h.app.mail_body_lines(&first);
    assert!(std::sync::Arc::ptr_eq(&before,&after),"详情与独立阅读窗口不能让另一封邮件的排版缓存每帧失效");
}

#[test]
fn mail_html_headings_scale_with_the_body_font(){
    let h=H::new();let lines=crate::mail_format::lines("<h1>缩放标题</h1>","");
    for (base,expected) in [(14.0,18.0),(28.0,36.0)]{
        let out=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(700.0,620.0))),..Default::default()},|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|crate::mail_reader::body_ui(ui,&lines,base));
        });
        let size=out.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()=="缩放标题"=>Some(t.galley.job.sections[0].format.font_id.size),_=>None}).unwrap();
        assert_eq!(size,expected,"正文缩放时标题应保持相同的字号比例");
    }
}

#[test]
fn mail_microsoft_settings_only_explains_unsupported_access(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.open_mail_settings();h.ctx.set_embed_viewports(false);
    h.app.mail.accounts.clear();h.app.mail.form.provider=crate::mail::Provider::Microsoft;
    let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
    egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
        let id=viewport.ids.this;
        let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,1300.0))),viewports:std::iter::once((id,egui::ViewportInfo::default())).collect(),..Default::default()};
        *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
    });
    for _ in 0..3{let _=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));}
    let child=output.lock().unwrap().take().unwrap();
    let texts=child.shapes.iter().filter_map(|s|match &s.shape{egui::Shape::Text(t)=>Some(t.galley.text()),_=>None}).collect::<Vec<_>>();
    assert!(texts.iter().any(|t|t.contains("OAuth")&&t.contains("暂不支持")),"应明确说明微软邮箱的支持状态");
    for absent in ["使用 Microsoft 登录","可留空，在浏览器选择账号","输入邮箱授权码","测试连接","保存并开始监控","应用 ID"]{
        assert!(!texts.iter().any(|t|t.contains(absent)),"暂不支持时不应出现误导入口：{absent}");
    }
    assert!(h.app.mail.accounts.is_empty());
    assert!(!h.app.pending.iter().any(|cmd|matches!(cmd,crate::app::Cmd::MailTest(..)|crate::app::Cmd::OpenUrl(_))));
}

#[test]
fn mail_microsoft_custom_host_and_address_disable_password_actions(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.open_mail_settings();h.ctx.set_embed_viewports(false);h.app.mail.accounts.clear();
    let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
    let events=std::sync::Arc::new(Mutex::new(Vec::new()));let input=events.clone();
    egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
        let id=viewport.ids.this;
        let raw=egui::RawInput{viewport_id:id,focused:true,events:std::mem::take(&mut *input.lock().unwrap()),
            screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,1300.0))),
            viewports:std::iter::once((id,egui::ViewportInfo::default())).collect(),..Default::default()};
        *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
    });
    for (provider,email,host) in [(crate::mail::Provider::Custom,"student@example.edu","outlook.office365.com"),(crate::mail::Provider::Qq,"user@hotmail.com","")]{
        h.app.mail.form.provider=provider;h.app.mail.form.email=email.into();h.app.mail.form.custom.host=host.into();
        h.app.mail.form.password.clear();h.app.mail.note.clear();
        for _ in 0..3{let _=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));}
        let child=output.lock().unwrap().take().unwrap();
        assert!(child.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==crate::mail::MICROSOFT_UNSUPPORTED)));
        for label in ["测试连接","保存并开始监控"]{
            let point=child.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==label=>Some(t.visual_bounding_rect().center()),_=>None}).unwrap();
            for pressed in [true,false]{
                *events.lock().unwrap()=vec![egui::Event::PointerMoved(point),egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}];
                let _=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));
            }
            assert!(h.app.mail.note.is_empty(),"微软云连接的按钮应停用，点击不应再提示填写授权码");
        }
        assert!(h.app.mail.accounts.is_empty());
        assert!(!h.app.pending.iter().any(|cmd|matches!(cmd,crate::app::Cmd::MailTest(..)|crate::app::Cmd::MailSync)));
    }
}

#[test]
#[ignore="按需导出微软邮箱暂不支持提示的实际画面"]
fn mail_microsoft_settings_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_MAIL_GALLERY_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for custom in [false,true]{
        let mut h=H::new();h.app.open_mail_settings();h.ctx.set_embed_viewports(false);
        h.app.mail.accounts.clear();
        h.app.mail.form.provider=if custom{crate::mail::Provider::Custom}else{crate::mail::Provider::Microsoft};
        h.app.mail.form.email="student@example.edu".into();h.app.mail.form.custom.host="outlook.office365.com".into();
        let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,1300.0))),
                viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.5),..Default::default()})).collect(),..Default::default()};
            *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        let mut raster=crate::preview_raster::Raster::new(1035,1950);
        for frame in 0..4{
            let root=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));raster.update_textures(&root.textures_delta);
            let child=output.lock().unwrap().take().unwrap();raster.update_textures(&child.textures_delta);
            if frame==3{
                raster.buf.fill([0.08,0.08,0.09,1.0]);raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join(if custom{"microsoft-custom-unsupported.png"}else{"microsoft-unsupported.png"}),&rgba,1035,1950,image::ColorType::Rgba8).unwrap();
            }raster.free_textures(&child.textures_delta);
        }
    }
}

#[test]
#[ignore="按需导出提醒选择器与邮件全文的实际画面"]
fn reminder_and_reader_layout_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_READER_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for picker in [true,false]{
        let mut h=H::new();h.ctx.set_embed_viewports(true);
        h.app.todos.items=vec![crate::model::TodoItem{text:"提交产品方案".into(),due:None,done_on:None,reminded:false,..Default::default()}];h.app.open_task_reminder(0);
        let body=(1..=12).map(|n|format!("第 {n} 项 · 工作安排\n请确认需求与交付范围，整理设计方案和本周待办，并在评审前同步进度。相关意见请汇总后反馈给产品团队。\n\n")).collect::<String>();
        h.app.mail.reading=Some(crate::mail::Item {received_unix: None, account:"work@icloud.com".into(),from:"产品团队".into(),subject:"项目方案与下阶段工作安排".into(),time:"10-31 09:41".into(),code:None,html: String::new().into(), summary:format!("各位好，\n\n以下是项目安排与交付要求，请逐项确认。\n\n{body}最后一段\n请在周五前确认，感谢配合。").into(),links:vec![crate::mail::Link{label:"查看项目资料".into(),url:"https://example.com/project".into()}],meeting:None,uid:42});
        let (w,height)=if picker{(WIN_W as u32,WIN_H as u32)}else{(700,620)};let ppp=1.5;
        let mut raster=crate::preview_raster::Raster::new((w as f32*ppp)as usize,(height as f32*ppp)as usize);
        let last=if picker{89}else{7};
        for f in 0..=last{
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(w as f32,height as f32))),time:Some(f as f64/60.0),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
            let out=h.ctx.run(raw,|ctx|if picker{
                let env=Env{t:f as f64/60.0,clock:"09:41:00".into(),today:chrono::Local::now().date_naive().to_string(),date:"周日".into(),..Default::default()};
                egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let win=ui.max_rect();h.app.frame(ui,win,&env);});
            }else{h.app.mail_reader_ui(ctx)});raster.update_textures(&out.textures_delta);
            if f==last{raster.buf.fill(if picker{[0.07,0.09,0.13,1.0]}else{[0.0,0.0,0.0,1.0]});raster.draw(&h.ctx.tessellate(out.shapes,out.pixels_per_point),out.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join(if picker{"reminder-picker.png"}else{"mail-reader.png"}),&rgba,(w as f32*ppp)as u32,(height as f32*ppp)as u32,image::ColorType::Rgba8).unwrap();}
            raster.free_textures(&out.textures_delta);
        }
    }
}

#[test]
fn remove_icons_are_centered_in_their_buttons_at_each_scale(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for scale in [1.0,1.25,1.5,2.0]{
        let mut h=H::new();h.ctx.set_pixels_per_point(scale);
        h.app.todos.items=["第一项","第二项","第三项"].into_iter().map(|text|crate::model::TodoItem{text:text.into(),due:None,done_on:None,reminded:false,..Default::default()}).collect();
        h.app.show_page(4);h.hover(H::local(300.0,60.0),1.0);
        for i in 0..3usize{
            let button=h.ctx.read_response(egui::Id::new(("card",true,("todo-delete",i)))).unwrap().rect;
            let ink=h.shapes.iter().filter_map(|s|match &s.shape{
                egui::Shape::Text(t) if t.galley.text()=="×" && button.contains(t.visual_bounding_rect().center())=>Some(t.visual_bounding_rect()),
                egui::Shape::LineSegment{points,..} if points.iter().all(|p|button.shrink(2.0).contains(*p)) && (points[0].x-points[1].x).abs()>2.0 && (points[0].y-points[1].y).abs()>2.0=>Some(Rect::from_two_pos(points[0],points[1])),
                _=>None,
            }).fold(Rect::NOTHING,|a,b|a.union(b));
            assert!(ink.is_positive(),"删除按钮应有可见图案");
            assert!(ink.center().distance(button.center())<0.4,"缩放 {scale}：删除图案中心 {:?} 偏离按钮中心 {:?}",ink.center(),button.center());
        }
        h.click(H::local(361.5,68.0));
        assert_eq!(h.app.todos.items.iter().map(|t|t.text.as_str()).collect::<Vec<_>>(),["第一项","第三项"]);
        assert!(h.app.todos.items.iter().all(|t|t.done_on.is_none()),"删除入口不能同时勾选其他待办");
    }
}

#[test]
#[ignore="按需检查全部页面与设置的实际排版"]
fn ui_alignment_layout_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_UI_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    fn fixture()->H{
        let mut h=H::new();h.app.settings.pet=false;h.app.settings.island_lyrics=false;h.power=(Some(76),false);
        h.media=crate::smtc::NowPlaying{title:"愿每一个认真生活的人，都能在平凡的日子里找到属于自己的光".into(),artist:"The Lanterns · Live Session".into(),duration:240.0,position:65.0,playing:true,..Default::default()};
        h.app.todos.items=["整理会议资料","Review design details","确认项目需求与下一阶段开发排期"].into_iter().map(|text|crate::model::TodoItem{text:text.into(),due:None,done_on:None,reminded:false,..Default::default()}).collect();
        h.app.shelf.items=["项目设计方案与交付说明.pdf","design-preview.png","本周工作安排.docx","project-files.zip","notes.md"].into_iter().map(std::path::PathBuf::from).collect();
        h.app.mail.accounts=vec![crate::mail::Account {custom: None, email:"product-team@example.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
        h.app.mail.recent=(0..3).map(|uid|crate::mail::Item {received_unix: None, account:"product-team@example.com".into(),from:"产品与设计协作团队".into(),subject:"请确认下阶段的设计方案和开发排期，评审前同步最新进度".into(),time:"11-01 09:41".into(),code:if uid==0{Some("123456".into())}else{None},html: String::new().into(), summary:"这是一段邮件正文，需要确认当前需求与后续的产品设计安排。".repeat(20).into(),links:vec![],meeting:None,uid}).collect();
        h.quota_accounts(3);h
    }
    let collapsed=["collapsed","volume","caps","language","power","network","meeting","meeting-dingtalk","meeting-wecom","bt-headphones","bt-keyboard","bt-mouse","bt-gamepad","bt-phone","bt-other","bt-speaker"];
    let scenes=["page-0","page-1","page-2","page-3","page-4","page-5","page-6","page-7","lyrics","todo-empty","shelf-empty","mail-empty","mail-detail","reminder","water-chart","focus-running"];
    for ppp in [1.25,2.0]{for name in scenes.into_iter().chain(collapsed){
        let mut h=fixture();let page=name.strip_prefix("page-").and_then(|p|p.parse::<usize>().ok()).unwrap_or(match name{"lyrics"=>0,"shelf-empty"=>3,"mail-empty"|"mail-detail"=>5,_=>4});
        if name=="todo-empty"{h.app.todos.items.clear();}if name=="shelf-empty"{h.app.shelf.items.clear();}if name=="mail-empty"{h.app.mail.accounts.clear();}
        if !collapsed.contains(&name){h.app.show_page(page);}
        if name=="water-chart"{h.app.show_page(2);h.app.water_view=1;h.app.water.drunk_ml=1250;h.app.water.hours[9]=250;h.app.water.hours[8]=500;h.app.water.hours[7]=500;}
        if name=="focus-running"{h.app.show_page(1);h.app.pomo.running=true;h.app.pomo.remaining=1283.0;}
        if let Some(kind)=name.strip_prefix("bt-"){
            let kind=match kind{"headphones"=>crate::bt::DeviceKind::Headphones,"keyboard"=>crate::bt::DeviceKind::Keyboard,"mouse"=>crate::bt::DeviceKind::Mouse,"gamepad"=>crate::bt::DeviceKind::Gamepad,"phone"=>crate::bt::DeviceKind::Phone,"speaker"=>crate::bt::DeviceKind::Speaker,_=>crate::bt::DeviceKind::Other};
            h.app.bluetooth_alert(&crate::bt::Device{id:"preview".into(),name:"我的蓝牙设备 · 超长设备名称与连接状态".into(),battery:Some(100),kind},true);
        }
        match name{"volume"=>{h.app.volume_ui=Some((64,false));h.app.alert("系统音量","64%",0,false);},"caps"=>h.app.alert("大写锁定","已开启",0,false),"language"=>h.app.alert("键盘输入语言","中文（简体，中国）",0,false),"network"=>h.app.alert("网络连接","Wi-Fi · 网络已连接",0,false),"power"=>h.app.alert("电源已连接","当前电量 76%",0,false),"meeting"=>h.app.meeting_alert(crate::mail::Meeting{ all_day:false,title:"产品方案评审与下阶段开发排期确认".into(),start:chrono::Local::now().timestamp()+600,end:None,at:"11-01 10:00".into(),uid:"preview".into()},"邮件 · product-and-design-team@company-example.com".into(),5),_=>{}}
        if name=="meeting-dingtalk" || name=="meeting-wecom" {
            h.app.meeting_alert(crate::mail::Meeting{all_day:false,title:"项目评审与下阶段安排".into(),start:chrono::Local::now().timestamp()+600,end:None,at:String::new(),uid:name.into()},if name=="meeting-dingtalk"{"钉钉"}else{"企业微信"}.into(),7);
        }
        if name=="lyrics"{h.app.show_lyrics=true;h.app.set_lyrics(&h.media.title,&h.media.artist,240.0,crate::lyrics::parse_lrc("[00:00]慢慢走过这段时光\n[01:00]愿每个认真生活的人都能找到属于自己的光\n[01:10]愿风带你去远方"));}
        if name=="mail-detail"{h.app.mail.open_uid=Some(("product-team@example.com".into(),0));}
        if name=="reminder"{h.app.open_task_reminder(0);}
        let (width,height)=((WIN_W*ppp)as u32,(WIN_H*ppp)as u32);let mut raster=crate::preview_raster::Raster::new(width as usize,height as usize);
        for f in 0..90{
            let t=f as f64/60.0;let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(WIN_W,WIN_H))),time:Some(t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
            let env=Env{t,clock:"09:41:00".into(),today:"2026-11-01".into(),date:"11月1日 周日".into(),cursor:if collapsed.contains(&name){None}else{Some(H::local(150.0,40.0))},media:h.media.clone(),battery:h.power.0,bt_ready:true,..Default::default()};
            let out=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{let win=ui.max_rect();h.app.frame(ui,win,&env);});});raster.update_textures(&out.textures_delta);
            if f==89{raster.buf.fill([0.07,0.09,0.13,1.0]);raster.draw(&h.ctx.tessellate(out.shapes,out.pixels_per_point),out.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join(format!("{name}-{}.png",(ppp*100.0)as u32)),&rgba,width,height,image::ColorType::Rgba8).unwrap();}
            raster.free_textures(&out.textures_delta);
        }
    }}
    for width in [600.0,690.0]{for tab in 0..12{
        let mut h=fixture();h.app.settings_tab=tab;h.app.open_settings();h.ctx.set_embed_viewports(false);
        let size=egui::vec2(width,if width==600.0{450.0}else{560.0});let ppp=1.25;
        let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{let id=viewport.ids.this;let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(ppp),focused:Some(true),..Default::default()})).collect(),focused:true,..Default::default()};*capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));});
        let (w,height)=((size.x*ppp)as u32,(size.y*ppp)as u32);let mut raster=crate::preview_raster::Raster::new(w as usize,height as usize);
        for f in 0..5{let root=h.ctx.run(egui::RawInput::default(),|ctx|h.app.settings_ui(ctx,false));raster.update_textures(&root.textures_delta);let child=output.lock().unwrap().take().unwrap();raster.update_textures(&child.textures_delta);
            if f==4{raster.buf.fill([0.0,0.0,0.0,1.0]);raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join(format!("settings-{tab}-{}.png",width as u32)),&rgba,w,height,image::ColorType::Rgba8).unwrap();}
            raster.free_textures(&root.textures_delta);raster.free_textures(&child.textures_delta);
        }
    }}
}

#[test]
fn meeting_notice_prioritizes_the_start_time_and_keeps_actions_aligned(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for ppp in [1.0,1.25,1.5,2.0]{
        let mut h=H::new();h.ctx.set_pixels_per_point(ppp);h.app.settings.pet=false;
        let start=chrono::Local::now()+chrono::Duration::minutes(10);
        h.app.meeting_alert(crate::mail::Meeting{ all_day:false,title:"产品评审".into(),start:start.timestamp(),end:None,at:start.format("%m-%d %H:%M").to_string(),uid:"meeting-layout".into()},"邮件 · product-and-design-team@company-example.com".into(),7);
        for _ in 0..90{h.frame(vec![]);}
        h.click_notice_badge();
        assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().starts_with("邮件 · "))),"软件名称仅用于识别图标，不占用提醒正文");
        let clock=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()==start.format("%H:%M").to_string()=>Some((t.visual_bounding_rect(),s.clip_rect,t.pos.x,t.galley.job.sections[0].format.font_id.size)),_=>None}).unwrap();
        let countdown=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text().ends_with("分钟后")=>Some((t.visual_bounding_rect(),t.galley.job.sections[0].format.color)),_=>None}).unwrap();
        assert!(clock.0.right()+12.0<=countdown.0.left(),"缩放 {ppp}：开始时刻与倒计时应保持间距");
        assert!(clock.1.contains_rect(clock.0),"大号时刻不能被提醒条裁切");
        assert_eq!(countdown.1,egui::Color32::WHITE,"倒计时使用白色，强调色保留给来源 Logo");
        let title=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()=="产品评审"=>Some((t.pos.x,t.galley.job.sections[0].format.font_id.size)),_=>None}).unwrap();
        assert!(clock.3>=28.0 && clock.3>title.1,"开始时刻应比事项名称更醒目");
        assert!((title.0-clock.2).abs()<0.5,"开始时刻与标题应沿同一列对齐");
        let date=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text().contains('月') && t.galley.text().contains('日')=>Some((t.visual_bounding_rect(),t.pos.x)),_=>None}).unwrap();
        let button=h.ctx.read_response(egui::Id::new(("card",true,"meeting-action"))).unwrap().rect;
        assert!((date.0.center().y-button.center().y).abs()<0.5,"日期与查看按钮应垂直居中对齐");
        assert!(((date.1-clock.1.left())-(clock.1.right()-button.right())).abs()<0.5,"卡片左右应保留相同边距");
    }
}

#[test]
fn meeting_long_titles_scroll_to_the_end_while_short_titles_stay_still(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for title in ["简短评审","产品设计方案评审及下一阶段开发排期与交付验收安排确认"]{
        let mut h=H::new();h.app.settings.pet=false;
        h.app.meeting_alert(crate::mail::Meeting{ all_day:false,title:title.into(),start:chrono::Local::now().timestamp()+600,end:None,at:String::new(),uid:title.into()},"飞书".into(),7);
        for _ in 0..45{h.frame(vec![]);}
        let title_shape=|h:&H|h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()==title=>Some((t.pos.x,t.visual_bounding_rect(),s.clip_rect)),_=>None}).expect("标题应保留完整内容，不应提前省略");
        let initial=title_shape(&h);let height=h.hit.height();
        h.t+=0.2;h.frame(vec![]);assert!((title_shape(&h).0-initial.0).abs()<0.5,"开始滚动前应停留片刻");
        h.t+=5.0;h.frame(vec![]);let middle=title_shape(&h);
        h.t+=4.0;h.frame(vec![]);let end=title_shape(&h);
        if title=="简短评审" {assert!((end.0-initial.0).abs()<0.5,"短标题保持静止");}
        else {
            assert!(middle.0<initial.0-20.0,"长标题应向左滚动显示后半段");
            assert_eq!(initial.2,end.2,"滚动不应改变标题的显示范围");
            assert!(end.1.right()<=end.2.right()+1.0 && end.1.right()>=end.2.right()-5.0,"应能看到标题末尾");
            h.t+=0.6;h.frame(vec![]);assert!((title_shape(&h).0-end.0).abs()<0.5,"滚到末尾后应停留，不能继续滑出");
        }
        assert!((h.hit.height()-height).abs()<0.5,"滚动不应改变提醒高度");
    }
}

#[test]
fn calendar_notice_overrides_island_lyrics_and_restores_the_current_line(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.pet=false;h.app.settings.island_lyrics=true;
    h.media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,position:7.0,duration:180.0,..Default::default()};
    h.app.set_lyrics("Track","Artist",180.0,crate::lyrics::parse_lrc("[00:00]首句歌词\n[00:20]后续歌词"));
    for _ in 0..60{h.frame(vec![]);}
    assert!(h.app.compact_notices());
    h.app.meeting_alert(crate::mail::Meeting{ all_day:false,title:"优先会议".into(),start:chrono::Local::now().timestamp()+600,end:None,at:String::new(),uid:"priority".into()},"飞书".into(),7);
    for _ in 0..90{h.frame(vec![]);}
    assert!(!h.app.compact_notices(),"日程提醒不应缩成歌词旁的小图标");
    assert!(h.hit.height()>130.0,"应直接展示完整 A 版提醒");
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="优先会议")));
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="查看")));
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="首句歌词")));
    let active=h.app.notice.as_ref().unwrap();h.t=active.at+active.duration-1.0/30.0;h.media.position=25.0;
    for _ in 0..90{
        h.frame(vec![]);
        if h.app.notice.is_none(){assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="Track" || t.galley.text()=="Artist")),"恢复歌词时不能闪出展开的播放器");}
    }
    assert!(h.app.notice.is_none() && h.app.settings.island_lyrics && h.app.compact_notices());
    assert!(h.hit.height()<35.0,"提醒结束后应恢复歌词条高度");
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="后续歌词")),"应恢复当前进度的歌词");
}

#[test]
fn shelf_remove_icon_stays_centered_on_hover_and_preserves_the_file(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for ppp in [1.0,1.25,1.5,2.0]{
        let mut h=H::new();h.ctx.set_pixels_per_point(ppp);
        let path=crate::model::config_dir().join("设计说明.txt");std::fs::create_dir_all(path.parent().unwrap()).unwrap();std::fs::write(&path,"保留原文件").unwrap();
        h.app.shelf.items=vec![path.clone()];h.app.show_page(3);
        for pointer in [H::local(300.0,60.0),H::local(233.0,39.0)]{
            h.hover(pointer,1.0);
            let button=h.ctx.read_response(egui::Id::new(("card",true,("shelf-remove",&path)))).unwrap().rect;
            let ink=h.shapes.iter().filter_map(|s|match &s.shape{
                egui::Shape::LineSegment{points,..} if points.iter().all(|p|button.shrink(2.0).contains(*p)) && (points[0].x-points[1].x).abs()>2.0 && (points[0].y-points[1].y).abs()>2.0=>Some(Rect::from_two_pos(points[0],points[1])),_=>None,
            }).fold(Rect::NOTHING,|a,b|a.union(b));
            assert!(ink.is_positive() && ink.center().distance(button.center())<0.4,"置物架移除图案应与按钮同心");
        }
        h.click(H::local(233.0,39.0));assert!(h.app.shelf.items.is_empty());assert!(path.exists());
    }
}

#[test]
fn resident_components_can_be_dragged_saved_and_reset(){
    use crate::bar_layout::{CLOCK,BATTERY,WEATHER};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    fn draw(h:&mut H,events:Vec<egui::Event>,slot:usize)->[Pos2;2]{
        let mut points=[Pos2::ZERO;2];
        h.t+=0.02;
        let out=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(600.0,300.0))),time:Some(h.t),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|{h.app.resident_order_settings(ui);for (index,slot) in [slot,BATTERY].into_iter().enumerate(){points[index]=ui.ctx().read_response(ui.id().with(("resident-icon",slot))).expect("暂未显示的组件也应可以独立拖动").rect.center();}});});
        h.shapes=out.shapes;points
    }
    fn point(h:&H,label:&str)->Pos2{h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()==label=>Some(t.visual_bounding_rect().center()),_=>None}).unwrap()}
    for slot in [CLOCK,WEATHER] {
    let mut h=H::new();
    draw(&mut h,vec![],slot);let [from,to]=draw(&mut h,vec![],slot);
    let button=|pos,pressed|egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()};
    draw(&mut h,vec![egui::Event::PointerMoved(from),button(from,true)],slot);
    draw(&mut h,vec![egui::Event::PointerMoved(from+egui::vec2(0.0,10.0))],slot);
    draw(&mut h,vec![egui::Event::PointerMoved(to)],slot);
    for _ in 0..3{draw(&mut h,vec![],slot);}
    draw(&mut h,vec![button(to,false)],slot);
    assert_eq!(h.app.settings.resident_order.first(),Some(&slot));
    assert!(h.app.settings.resident_order.contains(&BATTERY));
    h.app.settings.save();assert_eq!(crate::model::Settings::load().resident_order,h.app.settings.resident_order);
    let reset=point(&h,"恢复默认");draw(&mut h,vec![egui::Event::PointerMoved(reset),button(reset,true)],slot);draw(&mut h,vec![button(reset,false)],slot);
    assert!(h.app.settings.resident_order.is_empty());
    }
}

#[test]
fn weather_resident_settings_preserve_old_orders_and_the_status_switch(){
    use crate::bar_layout::{BATTERY,CLOCK,FOCUS,MUSIC,PET,QUOTA,WEATHER};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let path=crate::model::config_dir().join("settings.txt");std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path,"resident_order=7,6,2,0,9,1\nshow_weather=0\n").unwrap();
    h.app.settings=crate::model::Settings::load();
    assert_eq!(h.app.settings.resident_order,[FOCUS,MUSIC,QUOTA,CLOCK,WEATHER,PET,BATTERY],"旧排列应在时钟后插入天气并保留其余顺序");
    h.app.weather.snapshot=Some(crate::weather::Snapshot{temp:9.0,..Default::default()});
    assert!(h.app.weather_pill().is_none(),"状态开关关闭后天气不占常驻位置");
    assert!(h.app.module_on(8),"关闭常驻天气应保留天气页面");
    h.app.settings.save();h.app.settings=crate::model::Settings::load();
    assert!(h.app.weather_pill().is_none(),"重启后应保留常驻天气开关");
    for invalid in ["0,0,2,5,6,7,9","0,1,2,5,6,7,99","0,1,2","invalid",""] {
        std::fs::write(&path,format!("resident_order={invalid}\n")).unwrap();
        assert!(crate::model::Settings::load().resident_order.is_empty(),"无效排列应使用默认位置");
    }
}

#[test]
fn weather_status_switch_controls_the_badge_and_keeps_the_page_available(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    fn draw(h:&mut H,events:Vec<egui::Event>){
        let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();h.t+=0.02;let time=h.t;
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;
            let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,1000.0))),time:Some(time),events:events.clone(),viewports:std::iter::once((id,egui::ViewportInfo::default())).collect(),..Default::default()};
            *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        let _=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));
        h.shapes=output.lock().unwrap().take().unwrap().shapes;
    }
    for theme in [crate::settings_theme::SettingsTheme::Dark,crate::settings_theme::SettingsTheme::Light] {
        let mut h=H::new();h.app.settings.settings_theme=theme;h.app.settings_tab=10;h.app.open_settings();h.ctx.set_embed_viewports(false);
        h.app.weather.snapshot=Some(crate::weather::Snapshot{temp:9.0,..Default::default()});
        for _ in 0..3 {draw(&mut h,vec![]);}
        let text_rect=|h:&H,label:&str|h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()==label=>Some(t.visual_bounding_rect()),_=>None}).unwrap();
        let heading=text_rect(&h,"状态组件");let bottom=text_rect(&h,"功能页面").top();
        let title=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()=="天气"&&t.pos.x>=heading.left()&&t.pos.y>heading.bottom()&&t.pos.y<bottom=>Some(t.visual_bounding_rect()),_=>None}).expect("状态组件应有独立的天气开关");
        let point=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Rect(r) if (r.rect.width()-34.0).abs()<0.01&&(r.rect.height()-20.0).abs()<0.01&&r.rect.center().y>title.top()&&r.rect.center().y<title.bottom()+24.0=>Some(r.rect.center()),_=>None}).unwrap();
        for expected in [false,true] {
            for pressed in [true,false] {draw(&mut h,vec![egui::Event::PointerMoved(point),egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);}
            assert_eq!(h.app.weather_pill().is_some(),expected);
            assert_eq!(h.app.side_items().contains(&crate::side_dock::SideItem::Weather),expected);
            assert!(h.app.module_on(8),"关闭状态组件后天气页仍可使用");
            h.app.settings=crate::model::Settings::load();assert_eq!(h.app.weather_pill().is_some(),expected,"天气开关应自动保存");
        }
    }
}

#[test]
fn mailbox_scroll_reaches_older_mail_without_changing_pages() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.mail.accounts=vec![crate::mail::Account {custom: None, email:"test@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
    h.app.mail.recent=(1..=8).map(|uid|crate::mail::Item {received_unix: None, account:"test@qq.com".into(),from:format!("来源 {uid}"),subject:format!("邮件 {uid}"),time:"11-03 09:41".into(),code:Some(format!("{uid:06}")),html: String::new().into(), summary:String::new().into(),links:vec![],meeting:None,uid}).collect();
    for item in h.app.mail.recent.clone(){h.app.mail.privacy.observe(&item,true,h.app.t,chrono::Utc::now().timestamp());}
    h.app.show_page(5);h.hover(H::local(200.0,90.0),1.0);
    h.frame(vec![egui::Event::MouseWheel{unit:egui::MouseWheelUnit::Point,delta:egui::vec2(0.0,-900.0),modifiers:Default::default()}]);
    h.hover(H::local(200.0,90.0),0.6);
    assert_eq!(h.app.page(),5,"滚动邮箱列表不能切换页面");
    let row=h.ctx.read_response(egui::Id::new(("card",true,("mail-row",8u32,"test@qq.com")))).expect("第八封邮件应可访问");
    h.click(row.rect.center());
    assert_eq!(h.app.mail.open_uid,Some(("test@qq.com".into(),8)));
}

#[test]
#[ignore="按需检查邮箱列表和验证码来源的实际画面"]
fn mailbox_readability_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_MAIL_PREVIEW").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for variant in 0..5 {
        let multiple=variant==1;let detail=variant==2;
        let mut h=H::new();h.app.settings.pet=false;h.app.settings.mail_sync_operations=true;
        h.app.mail.accounts=vec![crate::mail::Account {custom: None, email:"work@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
        if multiple {h.app.mail.accounts.push(crate::mail::Account {custom: None, email:"personal@gmail.com".into(),provider:crate::mail::Provider::Gmail,enabled:true});}
        h.app.mail.recent=[("GitHub","登录验证码 · 本次登录有效期 10 分钟",Some("274630")),("Figma","产品设计评审 · 查看本轮修改意见",None),("Notion","确认你的邮箱地址",Some("815926")),("产品团队","周五项目安排与交付清单",None),("Microsoft","你的账户安全通知",None)].into_iter().enumerate().map(|(i,(from,subject,code))|crate::mail::Item {html:String::new().into(),received_unix: None, account:if multiple&&i%2==1{"personal@gmail.com"}else{"work@qq.com"}.into(),from:from.into(),subject:subject.into(),time:format!("11-03 09:{:02}",41-i),code:code.map(str::to_string),summary:String::new().into(),links:vec![],meeting:None,uid:i as u32+1}).collect();
        if detail {
            h.app.mail.recent[0].summary="你好，\n\n你的登录验证码是 274630，有效期为 10 分钟。\n\n如果这不是你本人的操作，请忽略这封邮件。请勿将验证码透露给他人。\n\n这封邮件由系统自动发送，无需回复。".into();
            h.app.mail.open_uid=Some(("work@qq.com".into(),1));
        }
        if variant==4{for item in h.app.mail.recent.clone(){h.app.mail.privacy.observe(&item,true,0.0,chrono::Utc::now().timestamp());}}
        for account in h.app.mail.accounts.clone(){
            h.app.mail.manage.identities.insert(account.email.clone(),account.clone());
            let entries=h.app.mail.recent.iter().filter(|m|m.account==account.email).map(|m|(m.uid,m.uid%2==0)).collect();
            h.app.mail.manage.data.insert((account.email,crate::mail_manage::Folder::Inbox),crate::mail_manage::Snapshot{validity:7,entries,items:vec![]});
        }
        if variant==3{
            h.app.mail.manage.trash=h.app.mail.recent.iter().take(2).cloned().map(|mut m|{m.code=None;m}).collect();
            h.app.mail.manage.trash_open=true;
            h.app.mail.manage.data.insert(("work@qq.com".into(),crate::mail_manage::Folder::Trash),crate::mail_manage::Snapshot{validity:9,entries:vec![(1,true),(2,true)],items:vec![]});
        }
        h.app.show_page(5);
        let ppp=1.5;let mut raster=crate::preview_raster::Raster::new((crate::app::CANVAS_W*ppp)as usize,(WIN_H*ppp)as usize);
        for frame in 0..100 {
            let t=frame as f64/60.0;let env=Env{t,clock:"09:41:00".into(),cursor:Some(H::local(if variant==1{350.0}else{200.0},64.0)),..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(crate::app::CANVAS_W,WIN_H))),time:Some(t),events:vec![egui::Event::PointerMoved(H::local(if variant==1{350.0}else{200.0},64.0))],viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
            let out=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(Pos2::ZERO,egui::vec2(WIN_W,WIN_H)),&env);});});
            raster.update_textures(&out.textures_delta);
            if frame==99 {raster.buf.fill([0.05,0.07,0.10,1.0]);raster.draw(&h.ctx.tessellate(out.shapes,out.pixels_per_point),out.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);let image=image::RgbaImage::from_raw((crate::app::CANVAS_W*ppp)as u32,(WIN_H*ppp)as u32,rgba).unwrap();image::imageops::crop_imm(&image,((WIN_W-PW)/2.0*ppp-10.0)as u32,0,(PW*ppp+20.0)as u32,if detail{370}else{225}).to_image().save(dest.join(if detail{"邮件详情.png"}else if variant==3{"回收站.png"}else if variant==4{"验证码分栏.png"}else if multiple{"悬停操作.png"}else{"邮箱列表.png"})).unwrap();}
            raster.free_textures(&out.textures_delta);
        }
    }
}

#[test]
fn mailbox_large_code_card_preserves_copy_and_identifies_source(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.mail.accounts=["work@qq.com","personal@gmail.com"].into_iter().map(|email|crate::mail::Account {custom: None, email:email.into(),provider:crate::mail::Provider::Qq,enabled:true}).collect();
    h.app.mail.recent=[("work@qq.com","GitHub","274630"),("personal@gmail.com","Notion","815926")].into_iter().map(|(account,from,code)|crate::mail::Item {received_unix: None, account:account.into(),from:from.into(),subject:"登录验证码".into(),time:"09:41".into(),code:Some(code.into()),html: String::new().into(), summary:String::new().into(),links:vec![],meeting:None,uid:42}).collect();
    for item in h.app.mail.recent.clone(){h.app.mail.privacy.observe(&item,true,h.app.t,chrono::Utc::now().timestamp());}
    h.app.show_page(5);h.hover(H::local(200.0,90.0),1.0);
    assert!(h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(t) if t.galley.text()=="GitHub")));
    h.cmds.clear();h.click(H::local(60.0,70.0));
    assert!(h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Copy(value) if value=="274630")));
    assert!(h.app.mail.open_uid.is_none(),"复制验证码不能误开邮件详情");
    assert_eq!(h.app.expanded_height(),PH,"邮箱沿用录屏里的面板尺寸");
}

#[test]
fn hidden_components_do_not_keep_static_pages_animating(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.pet=false;
    h.app.show_page(4);h.hover(H::local(300.0,60.0),2.0);
    assert!(!h.animating);
    let mut moving=crate::anim::Spring::new(0.0,230.0,0.9);moving.target=200.0;
    h.app.shelf_x.insert("hidden.txt".into(),moving);
    h.frame(vec![]);
    assert!(!h.animating,"离开置物页后，未完成的文件动画不能让其他页面持续刷新");
    h.app.shelf_x.clear();h.app.pomo.running=true;
    h.hover(H::local(300.0,60.0),1.5);
    assert!(!h.animating,"待办页没有显示专注动画，计时继续但不应强制连续刷新");
}

#[test]
fn minimal_music_stops_hidden_playback_animations(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.set_minimal(true);
    h.media=crate::smtc::NowPlaying{title:"歌曲".into(),artist:"歌手".into(),playing:true,..Default::default()};
    for _ in 0..180{h.frame(vec![]);}
    assert!(!h.app.open);
    assert!(!h.animating,"极简模式隐藏媒体组件后，不应继续为播放动画刷新");
}

#[test]
fn native_app_icon_uses_bounded_resolution(){
    let icon=crate::theme::app_icon();
    assert!(icon.width<=128 && icon.height<=128,"原生窗口图标不需要常驻完整的 1024 像素品牌原图");
    assert_eq!(icon.rgba.len(),(icon.width*icon.height*4)as usize);
    assert!(std::sync::Arc::ptr_eq(&icon,&crate::theme::app_icon()));
}

#[test]
fn long_mail_lists_only_layout_the_visible_rows(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.pet=false;
    h.app.mail.accounts=vec![crate::mail::Account {custom: None, email:"work@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
    h.app.mail.recent=(0..60).map(|i|crate::mail::Item {received_unix: None, account:"work@qq.com".into(),from:format!("发件人 {i}"),subject:format!("第 {i} 封邮件标题"),time:"09:41".into(),code:None,html: String::new().into(), summary:String::new().into(),links:vec![],meeting:None,uid:i}).collect();
    h.app.show_page(5);h.hover(H::local(300.0,60.0),1.0);
    let count=h.ctx.fonts(|fonts|fonts.num_galleys_in_cache());
    assert!(count<100,"60 封邮件不应全部排版，当前缓存了 {count} 个文字布局");
    for _ in 0..15{h.wheel(-40.0);h.hover(H::local(300.0,60.0),0.1);}
    assert_eq!(h.app.page,5,"列表虚拟化后仍应在邮箱页滚动");
}

#[test]
fn calendar_overnight_appears_on_both_days_but_not_after_end() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let first=chrono::NaiveDate::from_ymd_opt(2026,11,3).unwrap();
    h.app.calendar.view.loaded=Some(crate::calendar::month_start(first));
    let start=first.and_hms_opt(23,30,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap().timestamp();
    h.app.calendar.view.events.push(crate::mail::Meeting{ all_day:false,title:"跨夜".into(),start,end:Some(start+3600),at:String::new(),uid:"overnight".into()});
    assert_eq!(h.app.calendar_entries(first).len(),1);
    assert_eq!(h.app.calendar_entries(first.succ_opt().unwrap()).len(),1);
    assert_eq!(h.app.calendar_entries(first+chrono::Duration::days(2)).len(),0);
}

#[test]
fn calendar_month_refresh_removes_cancelled_cached_event() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let day=chrono::NaiveDate::from_ymd_opt(2026,11,3).unwrap();
    let start=day.and_hms_opt(16,0,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap().timestamp();
    h.app.calendar.events.push(crate::mail::Meeting{all_day:false,title:"已取消".into(),start,end:None,at:String::new(),uid:"removed".into()});
    h.app.calendar.view.loaded=Some(crate::calendar::month_start(day));
    h.app.calendar.view.events.clear();
    assert!(h.app.calendar_entries(day).is_empty(),"月份同步后不能让提醒缓存恢复已取消日程");
}

#[test]
fn ignored_mail_event_hides_stops_reminders_persists_and_restores() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let now=chrono::Local::now();let day=now.date_naive();
    let meeting=crate::mail::Meeting{all_day:false,title:"测试邀请".into(),start:now.timestamp()+600,end:None,at:String::new(),uid:"ignore-me".into()};
    h.app.mail.recent.push(crate::mail::Item {received_unix: None, account:"test@qq.com".into(),from:"测试".into(),subject:"邀请".into(),time:String::new(),code:None,html: String::new().into(), summary:String::new().into(),links:vec![],meeting:Some(meeting.clone()),uid:99});
    h.app.toggle_mail_event(&meeting);
    assert!(h.app.calendar_entries(day).is_empty());
    assert!(crate::model::Settings::load().ignored_mail_events.contains(&crate::calendar::dedupe_key(&meeting)));
    h.frame(vec![]);assert!(!h.app.history.iter().any(|a|a.title=="会议提醒"));
    assert_eq!(h.app.mail.recent.len(),1,"忽略不删除邮件");
    h.app.calendar.events.push(meeting.clone());
    assert_eq!(h.app.calendar_entries(day)[0].sources.len(),1,"飞书来源不受邮件忽略影响");
    h.app.calendar.events.clear();h.app.toggle_mail_event(&meeting);
    assert_eq!(h.app.calendar_entries(day).len(),1);
    h.frame(vec![]);assert!(h.app.history.iter().any(|a|a.title=="会议提醒"));
}

#[test]
fn calendar_sync_result_does_not_enqueue_another_sync() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let config=crate::calendar::Config{provider:crate::calendar::Provider::Feishu,username:"test".into(),href:None,server:None};
    h.app.calendar.config=Some(config.clone());h.app.open_calendar(None);h.app.pending.clear();
    h.app.calendar.view.loading=false;
    h.app.calendar_event(crate::calendar::Event::Synced{config,events:vec![],at:"12:00".into()});
    assert!(!h.app.pending.iter().any(|cmd|matches!(cmd,crate::app::Cmd::CalendarMonth(_))),"同步结果不能触发下一轮同步，否则后台会持续请求");
}

#[test]
fn page_order_persists_and_controls_navigation() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.page_order=[5,2,0,1,3,4,6,7,8];
    h.app.settings.save();
    let settings=crate::model::Settings::load();
    assert_eq!(settings.page_order,[5,2,0,1,3,4,6,7,8]);
    let restarted=App::new(&h.ctx,Options{fetch_lyrics:false,pet_idle:1e9});
    assert_eq!(restarted.page(),5);
    for page in [1,3,4,6,7,8] {h.app.set_page_visible(page,false);}
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    for expected in [5,2,0] {
        h.click(H::local(398.0,68.0));
        assert_eq!(h.app.page,expected);
        h.hover(H::local(300.0,60.0),0.7);
    }
    h.click(H::local(21.0,68.0));
    assert_eq!(h.app.page,2);
    h.app.set_page_visible(2,false);
    assert_eq!(h.app.page,5);
    for invalid in ["0,0,1,2,3,4,5,6","8,1,2,3,4,5,6,7","0,1","invalid"] {
        std::fs::write(crate::model::config_dir().join("settings.txt"),format!("page_order={invalid}\n")).unwrap();
        assert_eq!(crate::model::Settings::load().page_order,[0,1,2,3,4,5,6,7,8]);
    }
}

#[test]
fn page_order_drag_applies_on_release_and_saves() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings_tab=10;h.app.open_settings();h.ctx.set_embed_viewports(false);
    fn draw(h:&mut H,events:Vec<egui::Event>){
        let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
        h.t+=0.02;let time=h.t;
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;
            let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,1000.0))),time:Some(time),events:events.clone(),viewports:std::iter::once((id,egui::ViewportInfo::default())).collect(),..Default::default()};
            *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        let _=h.ctx.run(egui::RawInput::default(),|ctx|h.app.settings_ui(ctx,false));
        let child=output.lock().unwrap().take().unwrap();h.shapes=child.shapes;
    }
    for _ in 0..3 {draw(&mut h,vec![]);}
    let point=|h:&H,label:&str|h.shapes.iter().find_map(|s|match &s.shape {egui::Shape::Text(t) if t.galley.text()==label=>Some(t.visual_bounding_rect().left_center()),_=>None}).unwrap();
    let from=point(&h,"播放控制与同步歌词")+egui::vec2(-18.0,-6.0);
    let to=point(&h,"喝水与久坐提醒");
    let button=|pos,pressed|egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()};
    draw(&mut h,vec![egui::Event::PointerMoved(from),button(from,true)]);
    draw(&mut h,vec![egui::Event::PointerMoved(from+egui::vec2(0.0,10.0))]);
    draw(&mut h,vec![egui::Event::PointerMoved(to)]);
    assert_eq!(h.app.settings.page_order,[0,1,2,3,4,5,6,7,8]);
    draw(&mut h,vec![button(to,false)]);
    assert_eq!(h.app.settings.page_order,[1,2,0,3,4,5,6,7,8]);
    assert_eq!(crate::model::Settings::load().page_order,h.app.settings.page_order);
}

#[test]
fn settings_theme_follows_system_and_preserves_island_style() {
    use crate::settings_theme::SettingsTheme;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.open_settings();h.ctx.set_embed_viewports(false);
    assert_eq!(h.app.settings.settings_theme,SettingsTheme::System);
    assert_eq!(SettingsTheme::parse("unknown"),SettingsTheme::System);
    let mut raster=crate::preview_raster::Raster::new(690,1240);
    for (mode,system,dark) in [(SettingsTheme::System,false,false),(SettingsTheme::System,true,true),
        (SettingsTheme::System,false,false),(SettingsTheme::Dark,false,true),(SettingsTheme::Light,true,false)] {
        h.app.settings.settings_theme=mode;
        h.app.settings.save();assert_eq!(crate::model::Settings::load().settings_theme,mode);
        h.ctx.data_mut(|d|d.insert_temp(egui::Id::new("settings-system-theme"),(std::time::Instant::now(),system)));
        let output=std::sync::Arc::new(std::sync::Mutex::new(None));let capture=output.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;
            let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,1240.0))),
                viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.0),..Default::default()})).collect(),..Default::default()};
            *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        for tab in 0..12 {
            h.app.settings_tab=tab;
            // AI 额度的添加账号表单默认收起，点「＋ 添加账号」才展开；展开后再检查表单行对齐。
            h.app.quota.connect_open=tab==11;
            for frame in 0..3 {
                h.ctx.data_mut(|d|d.insert_temp(egui::Id::new("settings-system-theme"),(std::time::Instant::now(),system)));
                let root=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));
                assert!(h.ctx.style().visuals.dark_mode,"设置页主题不能泄漏到岛体");
                let commands=&root.viewport_output[&egui::ViewportId::from_hash_of("daozai-settings")].commands;
                assert!(commands.iter().any(|c|matches!(c,egui::ViewportCommand::SetTheme(t) if *t==if dark{egui::SystemTheme::Dark}else{egui::SystemTheme::Light})),"原生边框必须与页面一致");
                let child=output.lock().unwrap().take().unwrap();
                if frame==2 {
                    let pairs:&[(&str,&str)]=match tab {
                        1=>&[("时间偏移","0.35 秒")],
                        2=>&[("专注","25 分钟"),("休息","5 分钟")],
                        3=>&[("每日目标","2000 ml"),("每次记录","250 ml"),("提醒间隔","45 分钟"),("提醒时段","9 时")],
                        7=>&[("验证码隐藏","60 秒"),("检查间隔","120 秒"),("会议提前","15 分钟"),("邮箱","输入邮箱地址"),("授权码","输入邮箱授权码")],
                        8=>&[("用户名","输入用户名或粘贴账号信息"),("密码","输入平台生成的密码")],
                        11=>&[("服务","Codex"),("备注","例如：个人、工作、备用"),("选择该账号的登录文件","选择…")],
                        _=>&[],
                    };
                    for (label,value) in pairs {
                        let centers=|needle:&str|child.shapes.iter().filter_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==needle=>Some(t.visual_bounding_rect().center().y),_=>None}).collect::<Vec<_>>();
                        let distance=centers(label).iter().flat_map(|a|centers(value).into_iter().map(move|b|(a-b).abs())).fold(f32::INFINITY,f32::min);
                        assert!(distance<=1.0,"设置页 {tab} 的 {label} 与 {value} 未居中：{distance}");
                    }
                }
                if tab==0 && frame==2 {
                    let center=|label:&str|child.shapes.iter().find_map(|s|match &s.shape {
                        egui::Shape::Text(t) if t.galley.text()==label=>Some(t.visual_bounding_rect().center().y),_=>None,
                    }).unwrap();
                    for (label,choices) in [("显示屏",vec!["跟随主屏"]),("时间制式",vec!["24 小时","12 小时"]),("时段文字",vec!["不显示","上午 / 下午","详细时段"])] {
                        for choice in choices {assert!((center(label)-center(choice)).abs()<1.0,"{label}与{choice}应垂直居中：{} / {}",center(label),center(choice));}
                    }
                }
                let background=if dark{egui::Color32::from_rgb(25,25,27)}else{egui::Color32::from_rgb(246,247,249)};
                assert!(child.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Rect(r) if r.fill==background && r.rect.width()>400.0)));
                raster.update_textures(&root.textures_delta);raster.update_textures(&child.textures_delta);
                if frame==2 {
                    if let Some(dest)=std::env::var_os("DAOZAI_THEME_PREVIEW_DIR") {
                        let dest=std::path::PathBuf::from(dest);std::fs::create_dir_all(&dest).unwrap();
                        raster.buf.fill([0.0,0.0,0.0,1.0]);raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);
                        let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                        image::save_buffer(dest.join(format!("settings-{}-{tab}.png",if dark{"dark"}else{"light"})),&rgba,690,1240,image::ColorType::Rgba8).unwrap();
                    }
                }
                raster.free_textures(&root.textures_delta);raster.free_textures(&child.textures_delta);
            }
        }
    }
}

#[test]
#[ignore = "按需导出关于页支持作者预览"]
fn about_support_preview() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_SUPPORT_PREVIEW_DIR").expect("需指定预览目录"));
    std::fs::create_dir_all(&dest).unwrap();
    let mut h=H::new();h.app.open_settings();h.app.settings_tab=9;h.ctx.set_embed_viewports(false);
    let output=std::sync::Arc::new(std::sync::Mutex::new(None));let capture=output.clone();
    egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
        let id=viewport.ids.this;
        let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,560.0))),
            viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.0),..Default::default()})).collect(),..Default::default()};
        *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
    });
    let mut raster=crate::preview_raster::Raster::new(690,560);
    for dark in [true,false] { for open in [false,true] {
        h.app.settings.settings_theme=if dark{crate::settings_theme::SettingsTheme::Dark}else{crate::settings_theme::SettingsTheme::Light};
        h.ctx.data_mut(|data|data.insert_temp(egui::Id::new("support-author-open"),open));
        for frame in 0..4 {
            let root=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));
            let child=output.lock().unwrap().take().unwrap();
            raster.update_textures(&root.textures_delta);raster.update_textures(&child.textures_delta);
            if frame==3 {
                let has_wechat=child.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text()=="微信支付"));
                assert_eq!(has_wechat,open);
                for label in [if open{"返回"}else{"请喝咖啡 · 支持作者"},"完全自愿 · 感谢每一份支持 · 不影响功能使用"] {
                    let shape=child.shapes.iter().find(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text()==label)).expect("按钮和说明必须可见");
                    assert!(shape.clip_rect.contains_rect(shape.shape.visual_bounding_rect()),"默认窗口中内容不能被裁切：{label}");
                }
                raster.buf.fill([0.0,0.0,0.0,1.0]);
                raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                image::save_buffer(dest.join(format!("about-{}-{}.png",if dark{"dark"}else{"light"},if open{"open"}else{"closed"})),&rgba,690,560,image::ColorType::Rgba8).unwrap();
            }
            raster.free_textures(&root.textures_delta);raster.free_textures(&child.textures_delta);
        }
    } }
}


#[test]
fn mail_report_open_settings_matrix() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.ctx.set_embed_viewports(false);
    for scale in [1.0,1.25,1.5,2.0] {
        for size in [egui::vec2(600.0,450.0),egui::vec2(690.0,560.0)] {
            egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
                let id=viewport.ids.this;
                let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(scale),..Default::default()})).collect(),..Default::default()};
                let _=ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx));
            });
            for count in [0,1,6] {
                h.app.mail.accounts=crate::mail::Provider::ALL.into_iter().take(count).enumerate().map(|(i,provider)|crate::mail::Account {custom: None, email:format!("probe{i}@example.invalid"),provider,enabled:false}).collect();
                for provider in crate::mail::Provider::ALL {
                    h.app.mail.form.provider=provider;h.app.open_mail_settings();
                    for _ in 0..3 {let _=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));}
                    assert!(h.app.settings_open);assert_eq!(h.app.settings_tab,7);
                    h.app.settings_open=false;let _=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));
                }
            }
        }
    }
}

#[test]
fn mail_report_initial_fetch_unicode_message() {
    for space in ['\u{00a0}','\u{2002}','\u{2003}','\u{202f}'] {
        for kind in ["text/plain","text/html"] {
            let raw=format!("From: probe@example.invalid\r\nSubject: verification\r\nMIME-Version: 1.0\r\nContent-Type: {kind}; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n验证码：{space}834921");
            let parsed=crate::mail::parse_message(raw.as_bytes());
            assert_eq!(crate::mail::code_of(&parsed),Some("834921".into()));
        }
    }
}

#[test]
fn mail_report_save_and_failure_paths() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.open_mail_settings();h.ctx.set_embed_viewports(false);
    fn frame(h:&mut H,events:Vec<egui::Event>){
        let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;
            let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(900.0,1800.0))),events:events.clone(),viewports:std::iter::once((id,egui::ViewportInfo::default())).collect(),..Default::default()};
            *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        let _=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));
        h.shapes=output.lock().unwrap().take().unwrap().shapes;
    }
    fn click(h:&mut H,label:&str){
        frame(h,vec![]);frame(h,vec![]);
        let point=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==label=>Some(t.visual_bounding_rect().center()),_=>None}).unwrap();
        frame(h,vec![egui::Event::PointerMoved(point)]);
        for pressed in [true,false]{frame(h,vec![egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);}
    }
    for button in ["测试连接","保存并开始监控"] {
        h.app.mail.form.email.clear();h.app.mail.form.password.clear();click(&mut h,button);
        assert_eq!(h.app.mail.note,"请先填邮箱地址");
        h.app.mail.form.email="probe@example.invalid".into();click(&mut h,button);
        assert!(h.app.mail.note.contains("请先填授权码"));assert!(h.app.mail.accounts.is_empty());
    }
    let email=format!("daozai-crash-probe-{}@example.invalid",std::process::id());
    struct Cleanup(String);impl Drop for Cleanup{fn drop(&mut self){crate::mail::secrets::delete(&self.0);}}
    let _cleanup=Cleanup(email.clone());
    h.app.mail.form.email=email.clone();h.app.mail.form.password="synthetic-test-secret".into();
    click(&mut h,"保存并开始监控");
    assert_eq!(h.app.mail.accounts.len(),1);assert!(h.app.mail.form.password.is_empty());
    assert_eq!(crate::mail::load_accounts()[0].email,email);
    assert_eq!(crate::mail::secrets::load(&email).unwrap(),"synthetic-test-secret");
    h.app.mail_event(crate::mail::Event::Status{account:email.clone(),status:crate::mail::Status::Failed("模拟认证失败".into())});frame(&mut h,vec![]);
    h.app.mail.form.password="synthetic-test-secret".into();click(&mut h,"测试连接");
    assert_eq!(h.app.mail.testing.as_deref(),Some(email.as_str()));
    h.app.mail_event(crate::mail::Event::Test{account:email,ok:false,note:"模拟网络超时".into()});frame(&mut h,vec![]);
    assert!(h.app.mail.testing.is_none());assert!(h.app.mail.note.contains("模拟网络超时"));
}

#[test]
fn hover_tooltips_fit_native_region_and_release_it_after_leaving(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for shelf in [false,true] {
        let mut h=H::new();
        let label;
        let pointer;
        if shelf {
            let path=crate::model::config_dir().join("开发项目源代码与当前维护的项目文件夹");
            std::fs::create_dir_all(&path).unwrap();
            label=path.display().to_string();h.app.shelf.add(&path).unwrap();
            h.app.show_page(3);h.hover(H::local(300.0,60.0),1.0);
            pointer=H::local(210.0,66.0);
        } else {
            h.app.settings.island_lyrics=true;
            h.media.title="测试歌曲".into();h.media.artist="歌手".into();h.media.duration=240.0;h.media.playing=true;
            h.app.set_lyrics("测试歌曲","歌手",240.0,crate::lyrics::parse_lrc("[00:00]测试歌词"));
            h.app.alert("邮件验证码","收到邮件，验证码 123456，请在十分钟内使用",5,false);
            h.hover(Pos2::new(600.0,220.0),1.0);
            label="邮件验证码\n收到邮件，验证码 123456，请在十分钟内使用".to_string();
            pointer=Pos2::new(WIN_W/2.0+(h.app.current_width()-44.0)/2.0+26.0,15.0);
        }
        h.hover(pointer,0.4);
        let hit_before_tooltip=h.hit;
        h.hover(pointer,1.1);
        let bounds=h.shapes.iter().find_map(|s|match &s.shape {
            egui::Shape::Text(t) if t.galley.text()==label=>Some(t.visual_bounding_rect()),_=>None,
        }).expect("悬停后应显示完整提示");
        assert!(h.region.contains_rect(bounds),"悬停提示不能被原生窗口裁掉：{bounds:?} / {:?}",h.region);
        assert_eq!(h.hit,hit_before_tooltip,"提示不应扩大岛体交互区域");
        h.hover(Pos2::new(900.0,250.0),3.0);
        assert!(h.region.bottom()<100.0,"提示消失后应恢复收起区域");
    }
}

#[test]
fn clock_seconds_formats_and_persists(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();assert!(!h.app.settings.clock_seconds);
    assert_eq!(h.app.settings.clock_text("23:59:58"),"23:59");
    h.app.settings.clock_seconds=true;
    for (input,expected) in [("00:00:00","00:00:00"),("23:59:59","23:59:59"),("13:05","13:05:00"),("invalid","--:--:--")] {
        assert_eq!(h.app.settings.clock_text(input),expected);
    }
    h.app.settings.clock_12h=true;h.app.settings.clock_period=1;
    for (input,expected) in [("00:00:01","上午 12:00:01"),("12:00:00","下午 12:00:00"),("13:05:08","下午 1:05:08")] {
        assert_eq!(h.app.settings.clock_text(input),expected);
    }
    h.app.settings.clock_period=2;
    assert_eq!(h.app.settings.clock_text("23:59:59"),"晚上 11:59:59");
    for enabled in [true,false] {
        h.app.settings.clock_seconds=enabled;h.app.settings.save();
        assert_eq!(crate::model::Settings::load().clock_seconds,enabled);
    }
}

#[test]
fn clock_residency_only_hides_the_collapsed_clock(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for (seconds,twelve,period) in [(false,false,0),(true,false,0),(true,true,2)] {
        let mut h=H::new();h.app.settings.pet=false;h.power=(Some(76),false);h.quota_accounts(2);
        h.app.settings.clock_seconds=seconds;h.app.settings.clock_12h=twelve;h.app.settings.clock_period=period;
        h.app.weather.snapshot=Some(crate::weather::Snapshot{temp:19.0,..Default::default()});
        let digits=if seconds {if twelve{"9:41:00"}else{"09:41:00"}}else{"09:41"};
        let visible=|h:&H|h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==digits));
        for _ in 0..90{h.frame(vec![]);}assert!(visible(&h));let width=h.app.resident.target.width;
        h.app.settings.show_clock=false;
        for _ in 0..90{h.frame(vec![]);}
        assert!(!visible(&h));assert!(h.app.resident.target.x[crate::bar_layout::CLOCK].is_none());
        assert!(h.app.resident.target.width<width,"关闭时间应收回常驻占位");
        h.hover(Pos2::new(WIN_W/2.0,15.0),1.0);assert!(visible(&h),"展开顶栏仍应显示时间");
        h.cursor=None;for _ in 0..120{h.frame(vec![]);}
        for dock in [crate::model::Dock::Left,crate::model::Dock::Right] {
            h.app.settings.dock=dock;
            for _ in 0..90{side_frame(&mut h,Some(Pos2::new(250.0,580.0)));}
            assert!(!h.app.side_items().contains(&crate::side_dock::SideItem::Clock));
            assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="41")),"侧边收起态也应隐藏时间");
        }
        h.app.settings.show_clock=true;h.app.settings.dock=crate::model::Dock::Top;
        for _ in 0..90{h.frame(vec![]);}
        assert!(visible(&h));assert_eq!(h.app.resident.target.width,width,"重新开启时间应恢复布局");
    }
}

#[test]
fn clock_residency_switch_in_page_management_saves(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    fn draw(h:&mut H,raster:&mut crate::preview_raster::Raster,events:Vec<egui::Event>){
        let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();h.t+=0.02;let time=h.t;
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;
            *capture.lock().unwrap()=Some(ctx.run(egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(780.0,1400.0))),time:Some(time),events:events.clone(),viewports:std::iter::once((id,egui::ViewportInfo::default())).collect(),..Default::default()},|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        let root=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));let child=output.lock().unwrap().take().unwrap();
        raster.update_textures(&root.textures_delta);raster.update_textures(&child.textures_delta);h.shapes=child.shapes;
    }
    for dark in [true,false] {
        let mut h=H::new();h.app.settings.settings_theme=if dark{crate::settings_theme::SettingsTheme::Dark}else{crate::settings_theme::SettingsTheme::Light};
        h.app.settings_tab=10;h.app.open_settings();h.ctx.set_embed_viewports(false);
        let mut raster=crate::preview_raster::Raster::new(780,1400);
        assert!(h.app.settings.show_clock,"已有配置默认保持时间常驻");
        for _ in 0..3{draw(&mut h,&mut raster,vec![]);}
        let title=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()=="时间"=>Some(t.visual_bounding_rect()),_=>None}).expect("状态组件应提供时间开关");
        let point=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Rect(r)if (r.rect.width()-34.0).abs()<0.01&&(r.rect.height()-20.0).abs()<0.01&&r.rect.center().y>title.top()&&r.rect.center().y<title.bottom()+24.0=>Some(r.rect.center()),_=>None}).unwrap();
        for expected in [false,true] {
            for pressed in [true,false]{draw(&mut h,&mut raster,vec![egui::Event::PointerMoved(point),egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);}
            draw(&mut h,&mut raster,vec![]);
            assert_eq!(h.app.settings.show_clock,expected);assert_eq!(crate::model::Settings::load().show_clock,expected,"常驻时间开关应自动保存");
            let preview=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text().contains(':')&&t.galley.job.sections.first().is_some_and(|s|(s.format.font_id.size-crate::theme::size::PILL_CLOCK).abs()<0.01)=>Some(t.fallback_color.a()),_=>None}).expect("常驻排列应保留时间的位置");
            assert_eq!(preview==255,expected,"关闭时间后排列预览应置灰");
            if let Some(dest)=std::env::var_os("DAOZAI_CLOCK_RESIDENCY_PREVIEW") {
                let dest=std::path::PathBuf::from(dest);std::fs::create_dir_all(&dest).unwrap();
                raster.buf.fill([0.08,0.08,0.09,1.0]);raster.draw(&h.ctx.tessellate(h.shapes.clone(),1.0),1.0);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                image::save_buffer(dest.join(format!("settings-{}-{}.png",if dark{"dark"}else{"light"},if expected{"on"}else{"off"})),&rgba,780,1400,image::ColorType::Rgba8).unwrap();
            }
        }
    }
}

#[test]
fn clock_seconds_keep_resident_spacing_and_digit_width(){
    use crate::bar_layout::{CLOCK,WEATHER};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for period in 0..=2 {for twelve in [false,true] {
        let mut h=H::new();h.app.settings.pet=false;h.app.settings.clock_seconds=true;
        h.app.settings.clock_period=period;h.app.settings.clock_12h=twelve;
        h.quota_accounts(3);h.power=(Some(76),true);
        h.app.weather.snapshot=Some(crate::weather::Snapshot{place:"杭州".into(),temp:19.0,..Default::default()});
        for reversed in [false,true] {
            h.app.settings.resident_order=crate::bar_layout::DEFAULT_ORDER.to_vec();
            if reversed {h.app.settings.resident_order.reverse();}
            for _ in 0..90 {h.frame(vec![]);}
            let digits=if twelve {"9:41:00"}else{"09:41:00"};
            let text=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==digits=>Some(t.visual_bounding_rect()),_=>None}).expect("收起时应显示秒数");
            let temp=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()=="19°"=>Some(t.visual_bounding_rect()),_=>None}).unwrap();
            assert!(!text.expand(4.0).intersects(temp),"时钟和天气应保持间距");
            let layout=h.app.resident.current();
            assert!((layout.x[CLOCK].unwrap()-layout.x[WEATHER].unwrap()).abs()>(layout.widths[CLOCK]+layout.widths[WEATHER])/2.0,"排列反转后仍需预留秒数宽度");
        }
        let _=h.ctx.run(Default::default(),|ctx|{egui::CentralPanel::default().show(ctx,|ui|{
            let c=crate::paint::Card::new(ui,Pos2::ZERO,ui.max_rect());
            let widths:Vec<_>=(0..60).map(|second|crate::paint::clock_width(&c,&h.app.settings.clock_text(&format!("09:41:{second:02}")),crate::theme::size::PILL_CLOCK)).collect();
            assert!(widths.iter().all(|w|(w-widths[0]).abs()<0.01),"秒数变化不应改变时钟占位");
        });});
        h.app.show_page(0);h.hover(H::local(200.0,40.0),1.0);
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==if twelve{"9:41:00"}else{"09:41:00"})),"展开后顶栏也应显示秒数");
        h.t+=5.0;
        for dock in [crate::model::Dock::Left,crate::model::Dock::Right] {
            h.app.settings.dock=dock;
            for _ in 0..90 {side_frame(&mut h,Some(Pos2::new(250.0,580.0)));}
            assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="00")),"侧边收起时应显示秒数");
        }
    }}
}

#[test]
fn clock_seconds_setting_toggles_and_saves(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.open_settings();h.ctx.set_embed_viewports(false);
    let mut raster=crate::preview_raster::Raster::new(780,1800);
    fn frame(h:&mut H,raster:&mut crate::preview_raster::Raster,events:Vec<egui::Event>){
        h.t+=1.0/60.0;let time=h.t;
        let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;
            *capture.lock().unwrap()=Some(ctx.run(egui::RawInput{time:Some(time),viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(780.0,1800.0))),events:events.clone(),viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.0),..Default::default()})).collect(),..Default::default()},|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        let root=h.ctx.run(egui::RawInput{time:Some(time),..Default::default()},|ctx|h.app.settings_ui(ctx,false));let child=output.lock().unwrap().take().unwrap();
        raster.update_textures(&root.textures_delta);raster.update_textures(&child.textures_delta);h.shapes=child.shapes;
    }
    for _ in 0..3{frame(&mut h,&mut raster,vec![]);}
    let label=|h:&H|h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()=="显示秒数"&&s.clip_rect.contains_rect(t.visual_bounding_rect())=>Some(t.visual_bounding_rect()),_=>None});
    assert!(!h.app.settings.clock_seconds);
    for enabled in [true,false] {
        let title=label(&h).expect("常规设置应提供显示秒数开关");
        let toggle=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Rect(r)if (r.rect.width()-34.0).abs()<0.1&&(r.rect.height()-20.0).abs()<0.1&&(r.rect.top()-title.top()).abs()<18.0=>Some(r.rect.center()),_=>None}).expect("秒数开关应与文字对齐");
        for pressed in [true,false]{frame(&mut h,&mut raster,vec![egui::Event::PointerMoved(toggle),egui::Event::PointerButton{pos:toggle,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);}
        frame(&mut h,&mut raster,vec![]);
        assert_eq!(h.app.settings.clock_seconds,enabled);assert_eq!(crate::model::Settings::load().clock_seconds,enabled);
        if let Some(dest)=std::env::var_os("DAOZAI_CLOCK_PREVIEW_DIR") {
            let dest=std::path::PathBuf::from(dest);std::fs::create_dir_all(&dest).unwrap();
            raster.buf.fill([0.08,0.08,0.09,1.0]);raster.draw(&h.ctx.tessellate(h.shapes.clone(),1.0),1.0);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            image::save_buffer(dest.join(if enabled{"settings-on.png"}else{"settings-off.png"}),&rgba,780,1800,image::ColorType::Rgba8).unwrap();
        }
    }
}

#[test]
fn clock_12h_formats_boundaries_and_persists() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    assert!(!h.app.settings.clock_12h);
    assert_eq!(h.app.settings.clock_text("23:59:00"),"23:59");
    assert_eq!(h.app.settings.clock_period,0);
    h.app.settings.clock_12h=true;
    assert_eq!(h.app.settings.clock_text("00:00"),"12:00");
    assert_eq!(h.app.settings.clock_text("23:59"),"11:59");
    h.app.settings.clock_period=1;
    for (input,expected) in [("00:00:00","上午 12:00"),("11:59:00","上午 11:59"),("12:00:00","下午 12:00"),("13:05:00","下午 1:05"),("23:59:00","下午 11:59")] {
        assert_eq!(h.app.settings.clock_text(input),expected);
    }
    assert_eq!(h.app.settings.clock_text("13:05"),"下午 1:05");
    h.app.settings.save();
    assert!(crate::model::Settings::load().clock_12h);
    h.app.settings.pet=false;
    h.quota_accounts(5);
    for _ in 0..90 { h.frame(vec![]); }
    let text_rect=|label:&str|h.shapes.iter().find_map(|s|match &s.shape {
        egui::Shape::Text(t) if t.galley.text()==label=>Some(t.visual_bounding_rect()),_=>None,
    }).unwrap();
    assert!(h.app.resident.current().x[crate::bar_layout::quota_slot(4)].unwrap()+WIN_W/2.0+13.5<text_rect("上午").left());
    h.hover(H::local(200.0,40.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="9:41")));
    h.app.settings.clock_period=0;
    h.app.settings.clock_12h=false;
    h.app.settings.save();
    assert!(!crate::model::Settings::load().clock_12h);
    h.frame(vec![]);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="09:41")));
}

#[test]
fn clock_period_boundaries_and_both_formats() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    for twelve in [false,true] {
        h.app.settings.clock_12h=twelve;
        h.app.settings.clock_period=2;
        for (time,label) in [("00:00","凌晨"),("05:59","凌晨"),("06:00","上午"),("10:59","上午"),("11:00","中午"),("12:59","中午"),("13:00","下午"),("16:59","下午"),("17:00","傍晚"),("18:59","傍晚"),("19:00","晚上"),("23:59","晚上")] {
            assert!(h.app.settings.clock_text(time).starts_with(&format!("{label} ")));
        }
        assert_eq!(h.app.settings.clock_text("19:30"),if twelve {"晚上 7:30"}else{"晚上 19:30"});
        for mode in 0..=2 {
            h.app.settings.clock_period=mode;
            h.app.settings.save();
            let loaded=crate::model::Settings::load();
            assert_eq!(loaded.clock_period,mode);
            assert_eq!(loaded.clock_12h,twelve);
        }
    }
}

#[test]
fn clock_period_visually_centers_with_digits() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let h=H::new();
    let mut raster=crate::preview_raster::Raster::new(420,230);
    for scale in [1.0,1.25,1.5,2.0] {
        let output=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(420.0,230.0))),..Default::default()},|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let mut c=crate::paint::Card::new(ui,Pos2::ZERO,ui.max_rect());c.s=scale;
                for (i,period) in ["凌晨","上午","中午","下午","傍晚","晚上"].iter().enumerate() {
                    for (j,size) in [12.5,15.5].iter().enumerate() {
                        crate::paint::clock(&c,10.0+j as f32*100.0,18.0+i as f32*18.0,&format!("{period} 12:59"),*size);
                    }
                }
            });
        });
        let texts:Vec<_>=output.shapes.iter().filter_map(|s|match &s.shape{egui::Shape::Text(t)=>Some(t),_=>None}).collect();
        for pair in texts.chunks_exact(2) {
            assert!((pair[0].visual_bounding_rect().center().y-pair[1].visual_bounding_rect().center().y).abs()<0.1,"时段文字与数字应按可见字形垂直居中，缩放 {scale}");
        }
        raster.update_textures(&output.textures_delta);
        if scale==2.0 {
            if let Some(dest)=std::env::var_os("DAOZAI_CLOCK_ALIGNMENT_PREVIEW") {
                raster.buf.fill([0.0,0.0,0.0,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                image::save_buffer(dest,&rgba,420,230,image::ColorType::Rgba8).unwrap();
            }
        }
    }
}

#[test]
fn avoidance_picker_add_remove_and_save() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.ctx.set_visuals(egui::Visuals::dark());
    assert!(h.app.settings.avoid_browsers);
    assert!(h.app.settings.avoid_apps.is_empty());
    h.app.settings.browser_minimal=true;
    h.app.avoidance.entries=vec![
        crate::avoidance::Candidate{path:r"D:\Quark\quark.exe".into(),name:"夸克浏览器".into(),title:"新标签页".into(),browser:true},
        crate::avoidance::Candidate{path:r"C:\Windows\System32\notepad.exe".into(),name:"记事本".into(),title:"未命名 - 记事本".into(),browser:false},
    ];
    fn draw(h:&mut H,events:Vec<egui::Event>)->egui::FullOutput {
        h.t+=1.0/60.0;
        h.ctx.run(egui::RawInput{time:Some(h.t),screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(600.0,560.0))),events,..Default::default()},|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.heading("常规");
                h.app.avoidance.settings(ui,&mut h.app.settings);
            });
            h.app.avoidance.dialog(ctx,&mut h.app.settings);
        })
    }
    fn click(h:&mut H,point:Pos2){
        draw(h,vec![egui::Event::PointerMoved(point),egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
        draw(h,vec![egui::Event::PointerMoved(point),egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
    }
    fn point(out:&egui::FullOutput,label:&str)->Pos2 {
        out.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()==label=>Some(t.visual_bounding_rect().center()),_=>None}).unwrap_or_else(||panic!("未找到 {label}"))
    }
    h.app.avoidance.open=true;
    let mut raster=crate::preview_raster::Raster::new(600,560);
    let mut out=draw(&mut h,vec![]);raster.update_textures(&out.textures_delta);
    for _ in 0..40 {std::thread::sleep(std::time::Duration::from_millis(10));out=draw(&mut h,vec![]);raster.update_textures(&out.textures_delta);}
    assert!(out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="浏览器规则已覆盖")));
    if let Some(dest)=std::env::var_os("DAOZAI_AVOID_PREVIEW_DIR") {
        std::fs::create_dir_all(&dest).unwrap();raster.buf.fill([0.0,0.0,0.0,1.0]);
        raster.draw(&h.ctx.tessellate(out.shapes.clone(),out.pixels_per_point),out.pixels_per_point);
        let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(std::path::Path::new(&dest).join("添加应用.png"),&rgba,600,560,image::ColorType::Rgba8).unwrap();
    }
    click(&mut h,point(&out,"记事本"));
    let out=draw(&mut h,vec![]);
    click(&mut h,point(&out,"添加"));
    assert!(!h.app.avoidance.open);
    assert_eq!(h.app.settings.avoid_apps.len(),1);
    h.app.settings.avoid_browsers=false;h.app.settings.save();
    let loaded=crate::model::Settings::load();
    assert!(!loaded.avoid_browsers);assert!(loaded.browser_minimal);
    assert_eq!(loaded.avoid_apps,h.app.settings.avoid_apps);
    let mut out=draw(&mut h,vec![]);raster.update_textures(&out.textures_delta);
    for _ in 0..40{std::thread::sleep(std::time::Duration::from_millis(20));out=draw(&mut h,vec![]);raster.update_textures(&out.textures_delta);}
    assert!(!out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="记事本")),"图标区不应常驻显示应用名称");
    if let Some(dest)=std::env::var_os("DAOZAI_AVOID_PREVIEW_DIR") {
        raster.buf.fill([0.0,0.0,0.0,1.0]);raster.draw(&h.ctx.tessellate(out.shapes.clone(),out.pixels_per_point),out.pixels_per_point);
        let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(std::path::Path::new(&dest).join("图标区域.png"),&rgba,600,560,image::ColorType::Rgba8).unwrap();
    }
    click(&mut h,point(&out,"×"));assert!(h.app.settings.avoid_apps.is_empty());
    h.app.settings.save();assert!(crate::model::Settings::load().avoid_apps.is_empty());
}

#[test]
fn six_minutes_without_interaction_do_not_restart_static_animation_or_grow_textures(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.pet=false;h.app.water.remind=false;
    for _ in 0..120{h.frame(vec![]);}
    let textures=h.ctx.tex_manager().read().num_allocated();
    for second in 3..=363 {
        h.t=second as f64;h.system_idle=Some(second as f64);h.frame(vec![]);
        assert!(!h.animating,"空闲到第 {second} 秒不应重新进入连续动画");
        assert_eq!(h.ctx.tex_manager().read().num_allocated(),textures,"空闲纹理数不能持续增长");
    }
}

#[test]
fn avoidance_icon_area_stays_one_row_and_scrolls() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.ctx.set_visuals(egui::Visuals::dark());h.app.settings.browser_minimal=true;
    fn draw(h:&mut H,events:Vec<egui::Event>)->egui::FullOutput {
        h.t+=1.0/60.0;
        h.ctx.run(egui::RawInput{time:Some(h.t),screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(420.0,560.0))),events,..Default::default()},|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{h.app.avoidance.settings(ui,&mut h.app.settings);});
        })
    }
    fn text(out:&egui::FullOutput,label:&str)->Rect {
        out.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()==label=>Some(t.visual_bounding_rect()),_=>None}).unwrap()
    }
    let footer="无需全屏或最大化；窗口移开或切换应用后恢复。";
    h.app.settings.avoid_apps=vec![("D:/sample/A.exe".into(),"A应用".into())];
    draw(&mut h,vec![]);let one=draw(&mut h,vec![]);let baseline=text(&one,footer).top();
    let cross=text(&one,"×").center();
    assert!(one.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Circle(c) if c.radius<=7.1 && c.center.distance(cross)<3.0)),"移除角标应为小圆点，不能被按钮最小尺寸撑大");
    h.app.settings.avoid_apps=(0..30).map(|i|(format!("D:/sample/{i}.exe"),format!("{}应用",char::from_u32(65+i).unwrap()))).collect();
    draw(&mut h,vec![]);let many=draw(&mut h,vec![]);
    assert!((text(&many,footer).top()-baseline).abs()<1.0,"应用增多时始终保持单行高度");
    let first=text(&many,"A");let pointer=first.center();
    draw(&mut h,vec![egui::Event::PointerMoved(pointer),egui::Event::MouseWheel{unit:egui::MouseWheelUnit::Point,delta:egui::vec2(0.0,-150.0),modifiers:Default::default()}]);
    let mut scrolled=draw(&mut h,vec![]);for _ in 0..30{scrolled=draw(&mut h,vec![]);}
    assert!((text(&scrolled,footer).top()-text(&many,footer).top()).abs()<1.0,"滚动图标不能移动区域下方内容");
    assert!(scrolled.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="A" && t.visual_bounding_rect().left()<first.left()-10.0)) || !scrolled.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="A")),"应能滚动查看后续应用");
}

#[test]
fn compact_canvas_preserves_panel_and_notice_pixels(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    fn draw(width:f32,page:Option<usize>,notice:bool)->Vec<u8>{
        let mut h=H::new();h.app.settings.island_lyrics=true;
        let media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:120.0,..Default::default()};
        h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]晚风吹过了山岗\n[01:00]下一句"));
        if let Some(page)=page{h.app.show_page(page);}
        let mut raster=crate::preview_raster::Raster::new(1280,270);
        let mut rgba=Vec::new();
        for frame in 0..180 {
            if notice && frame==60{h.app.alert("收到新邮件","会议通知及验证码 834921",5,true);}
            let click=(notice && (frame==90 || frame==91)).then(||Pos2::new(WIN_W/2.0+(h.app.current_width()-44.0)/2.0+26.0,15.0));
            let t=frame as f64/60.0;
            let env=Env{t,clock:"09:41:00".into(),today:"2026-10-28".into(),date:"10月28日 周三".into(),media:media.clone(),cursor:click.or_else(||page.map(|_|H::local(300.0,40.0))),..Default::default()};
            let output=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(width,WIN_H))),time:Some(t),events:click.map(|pos|vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:frame==90,modifiers:Default::default()}]).unwrap_or_default(),..Default::default()},|ctx|{
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H)),&env);});
            });
            raster.update_textures(&output.textures_delta);
            if [30,65,95,179].contains(&frame){
                raster.buf.fill([0.0;4]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut pixels=Vec::new();raster.to_rgba8(&mut pixels);
                if page.is_none() && notice && frame==95 {
                    if let Some(dir)=std::env::var_os("DAOZAI_GRAPHICS_PREVIEW") {let dir=std::path::PathBuf::from(dir);std::fs::create_dir_all(&dir).unwrap();image::save_buffer(dir.join(format!("canvas-{width}.png")),&pixels,1280,270,image::ColorType::Rgba8).unwrap();}
                }
                rgba.extend(pixels);
            }
        }
        rgba
    }
    for page in std::iter::once(None).chain((0..8).map(Some)) {
        for notice in [false,true] {
            assert_eq!(draw(1280.0,page,notice),draw(960.0,page,notice),"缩减透明余量不能改变页面 {page:?} 和通知 {notice} 的像素");
        }
    }
}

#[test]
fn external_notice_respects_permissions_masks_preview_and_never_reemits(){
 let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
 h.app.receive_notification("系统音量".into(),"external.test".into(),"验证码 123456".into());assert!(h.app.alerts.is_empty());
 h.app.settings.read_notifications=true;
 h.app.receive_notification("系统音量".into(),"external.test".into(),"验证码 123456".into());
 let a=h.app.alerts.back().unwrap();assert!(!a.is_hud());assert!(!a.body.contains("123456"));assert_eq!(a.page,usize::MAX);
 assert!(h.app.history.is_empty());assert!(!h.app.pending.iter().any(|c|matches!(c,crate::app::Cmd::Notify(..))));
 h.app.tick_alerts();assert!(h.app.notice.is_some());h.app.settings.read_notifications=false;h.app.tick_alerts();assert!(h.app.notice.is_none());
 h.app.settings.read_notifications=true;h.app.settings.notification_preview=true;
 h.app.receive_notification("测试".into(),"external.test".into(),"验证码 123456，请勿泄露".into());assert!(!h.app.alerts.back().unwrap().body.contains("123456"));
 h.app.tick_alerts();h.app.t+=4.6;h.app.tick_alerts();assert!(h.app.notice.is_none());
}

#[test]
fn qq_only_notifications_filter_by_app_id_before_display_and_respect_preview(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.settings.read_notifications=true;h.app.settings.qq_only=true;h.app.settings.save();
    assert!(crate::model::Settings::load().qq_only);
    h.app.receive_notification("QQ".into(),"QQMusic".into(),"不属于 QQ 聊天".into());assert!(h.app.alerts.is_empty());
    h.app.receive_notification("QQ".into(),"QQ".into(),"测试联系人 · 测试消息".into());
    assert!(!h.app.alerts.back().unwrap().body.contains("测试联系人"));
    h.app.settings.notification_preview=true;
    h.app.receive_notification("QQ".into(),"QQ".into(),"测试联系人 · 测试消息".into());
    assert_eq!(h.app.alerts.back().unwrap().body,"测试联系人 · 测试消息");assert!(h.app.history.is_empty());
}

#[test]
fn wechat_notifications_work_without_toast_permission_and_stop_when_disabled(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.receive_wechat_reminder();assert!(h.app.alerts.is_empty());
    h.app.settings.wechat_notifications=true;
    h.app.settings.save();assert!(crate::model::Settings::load().wechat_notifications);
    assert!(!h.app.settings.read_notifications);
    h.app.receive_wechat_reminder();h.app.tick_alerts();
    let notice=h.app.notice.as_ref().unwrap();
    assert_eq!(notice.title,"微信");assert_eq!(notice.body,"有新消息 · 请到微信查看");
    assert!(h.app.history.is_empty());
    assert!(!h.app.pending.iter().any(|c|matches!(c,crate::app::Cmd::Notify(..))));
    h.app.settings.wechat_notifications=false;h.app.tick_alerts();assert!(h.app.notice.is_none());
    h.app.settings.wechat_notifications=true;h.app.settings.notifications=false;
    h.app.receive_wechat_reminder();assert!(h.app.alerts.is_empty());
}

#[test]
fn hidden_mail_masks_every_display_field_and_reveal_is_temporary(){
 let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
 let i=crate::mail::Item{received_unix:None,account:"a@example.com".into(),from:"123456".into(),subject:"验证码 123456".into(),time:"09:41".into(),code:Some("123456".into()),html:"<h1>验证码</h1><p><strong>123456</strong></p>".into(), summary:"验证码 1 2 3-4 5 6".into(),links:vec![],meeting:None,uid:42};
 h.app.mail.privacy.observe(&i,true,0.0,100);h.app.t=61.0;
 let shown=h.app.display_mail(&i);assert!(shown.code.is_none());for text in [shown.from.as_str(),shown.subject.as_str(),shown.summary.as_ref()]{assert!(!text.contains('1'));}
 assert!(shown.html.is_empty(),"隐藏验证码时 HTML 原文也必须隐藏");
 assert!(!h.app.mail_body_lines(&shown).iter().any(|line|line.text.contains("123456")));
 h.app.reveal_mail_code(&i);let revealed=h.app.display_mail(&i);assert!(revealed.code.is_some());
 assert_eq!(revealed.html,i.html);assert!(h.app.mail_body_lines(&revealed).iter().any(|line|line.text.contains("123456")));
 h.app.t+=15.0;let hidden=h.app.display_mail(&i);assert!(hidden.code.is_none()&&hidden.html.is_empty());
}

#[test]
fn compact_font_atlas_preserves_panel_and_notice_pixels(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    fn draw(texture_side:usize,page:Option<usize>,notice:bool)->Vec<u8>{
        let mut h=H::new();h.app.settings.island_lyrics=true;
        let media=crate::smtc::NowPlaying{title:"Track".into(),artist:"Artist".into(),playing:true,duration:120.0,..Default::default()};
        h.app.set_lyrics("Track","Artist",120.0,crate::lyrics::parse_lrc("[00:00]晚风吹过了山岗\n[01:00]下一句"));
        if let Some(page)=page{h.app.show_page(page);}
        let mut raster=crate::preview_raster::Raster::new(1280,270);
        let mut rgba=Vec::new();
        for frame in 0..180 {
            if notice && frame==60{h.app.alert("收到新邮件","会议通知及验证码 834921",5,true);}
            let click=(notice && (frame==90 || frame==91)).then(||Pos2::new(WIN_W/2.0+(h.app.current_width()-44.0)/2.0+26.0,15.0));
            let t=frame as f64/60.0;
            let env=Env{t,clock:"09:41:00".into(),today:"2026-10-28".into(),date:"10月28日 周三".into(),media:media.clone(),cursor:click.or_else(||page.map(|_|H::local(300.0,40.0))),..Default::default()};
            let output=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(960.0,WIN_H))),max_texture_side:Some(texture_side),time:Some(t),events:click.map(|pos|vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:frame==90,modifiers:Default::default()}]).unwrap_or_default(),..Default::default()},|ctx|{
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H)),&env);});
            });
            raster.update_textures(&output.textures_delta);
            if [30,65,95,179].contains(&frame){
                raster.buf.fill([0.0;4]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut pixels=Vec::new();raster.to_rgba8(&mut pixels);
                if page.is_none() && notice && frame==95 {
                    if let Some(dir)=std::env::var_os("DAOZAI_GRAPHICS_PREVIEW") {let dir=std::path::PathBuf::from(dir);std::fs::create_dir_all(&dir).unwrap();image::save_buffer(dir.join(format!("atlas-{texture_side}.png")),&pixels,1280,270,image::ColorType::Rgba8).unwrap();}
                }
                rgba.extend(pixels);
            }
        }
        rgba
    }
    for page in std::iter::once(None).chain((0..8).map(Some)) {
        for notice in [false,true] {
            let reference=draw(16384,page,notice);
            for limit in [4096,2048] {let other=draw(limit,page,notice);
                let max=reference.iter().zip(&other).map(|(a,b)|a.abs_diff(*b)).max().unwrap_or(0);let count=reference.iter().zip(&other).filter(|(a,b)|a!=b).count();
                // 图集坐标改变只允许少量通道有 1/255 的浮点采样舍入差异（实测最多约 0.005% 的通道），肉眼不可见。
                assert!(max<=1 && count<reference.len()/1000,"图集上限 {limit} 改变了页面 {page:?}、通知 {notice} 的像素：{max}/{count}");}
        }
    }
}
#[test]
fn compact_font_atlas_preserves_settings_pixels_at_multiple_scales(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    fn draw(limit:usize,scale:f32)->Vec<Vec<u8>>{
        let mut h=H::new();h.app.open_settings();h.ctx.set_embed_viewports(false);
        let output=std::sync::Arc::new(std::sync::Mutex::new(None));let capture=output.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;
            let raw=egui::RawInput{viewport_id:id,max_texture_side:Some(limit),screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,560.0))),
                viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(scale),..Default::default()})).collect(),..Default::default()};
            *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        let mut raster=crate::preview_raster::Raster::new((690.0*scale)as usize,(560.0*scale)as usize);let mut result=Vec::new();
        for tab in 0..12 {h.app.settings_tab=tab;
            for frame in 0..3 {
                let root=h.ctx.run(egui::RawInput{max_texture_side:Some(limit),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(scale),..Default::default()})).collect(),..Default::default()},|ctx|h.app.settings_ui(ctx,false));
                let mut child=output.lock().unwrap().take().unwrap();
                raster.update_textures(&root.textures_delta);raster.update_textures(&child.textures_delta);
                if frame==2 {
                    // 常驻预览读取真实时钟，排除跨分钟变化；固定时钟已由主界面对照覆盖。
                    if tab==10{child.shapes.retain(|s|!matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().len()==5 && t.galley.text().as_bytes()[2]==b':'));}
                    raster.buf.fill([0.0,0.0,0.0,1.0]);raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);
                    let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);result.push(rgba);
                }
                raster.free_textures(&root.textures_delta);raster.free_textures(&child.textures_delta);
            }
        }result
    }
    for scale in [1.0,1.5,2.0] {
        let before=draw(16384,scale);
        for limit in [4096,2048] {let after=draw(limit,scale);
        for (tab,(before,after)) in before.iter().zip(&after).enumerate(){
            // 图集坐标改变只允许少量通道有 1/255 的浮点采样舍入差异。
            let max=before.iter().zip(after).map(|(a,b)|a.abs_diff(*b)).max().unwrap_or(0);
            let count=before.iter().zip(after).filter(|(a,b)|a!=b).count();assert!(max<=1 && count<before.len()/1000,"设置页 {tab} 在图集上限 {limit}、缩放 {scale} 下出现可见差异：{max}/{count}");
        }}
    }
}

#[test]
fn mailbox_wheel_stays_in_list_with_hidden_cards_and_short_lists() {
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for mode in 0..4 {for count in [0,1,3,8] {
        let mut h=H::new();
        h.app.mail.accounts=vec![crate::mail::Account{custom:None,email:"test@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
        h.app.mail.recent=(1..=count).map(|uid|crate::mail::Item{received_unix:None,account:"test@qq.com".into(),from:format!("来源 {uid}"),subject:format!("邮件 {uid}"),time:"11-04 09:41".into(),code:if mode==0{None}else{Some(format!("{uid:06}"))},html: String::new().into(), summary:String::new().into(),links:vec![],meeting:None,uid}).collect();
        if mode>=2 {for item in h.app.mail.recent.clone(){h.app.mail.privacy.observe(&item,true,h.app.t,chrono::Utc::now().timestamp());}}
        h.app.settings.mail_code=mode!=2;
        let cursor=H::local(if mode==3 && count>0{220.0}else{65.0},90.0);
        h.app.show_page(5);h.hover(cursor,1.0);
        for delta in [300.0,-900.0,-300.0,900.0,300.0] {
            h.wheel(delta);h.hover(cursor,0.3);
            assert_eq!(h.app.page(),5,"模式 {mode}、{count} 封邮件：列表范围内和滚动边界不能触发翻页");
        }
    }}
}

#[test]
fn mailbox_compact_third_row_stays_visible_below_the_toolbar(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.mail.accounts=vec![crate::mail::Account {custom:None,email:"test@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
    h.app.mail.recent=(1..=6).map(|uid|crate::mail::Item {received_unix:None,account:"test@qq.com".into(),from:format!("来源 {uid}"),subject:format!("邮件 {uid}"),time:"09:41".into(),code:None,html: String::new().into(), summary:String::new().into(),links:vec![],meeting:None,uid}).collect();
    h.app.show_page(5);h.hover(H::local(200.0,90.0),1.0);
    let third=h.ctx.read_response(egui::Id::new(("card",true,("mail-row",3u32,"test@qq.com")))).expect("第三封邮件应完整可见");
    assert!(third.rect.bottom()<=H::local(380.0,122.0).y+0.5);
    h.click(third.rect.center());
    assert_eq!(h.app.mail.open_uid,Some(("test@qq.com".into(),3)));
}

#[test]
fn bluetooth_switch_clears_notices_and_restarts_with_fresh_baseline(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    let device=crate::bt::Device{id:"keyboard".into(),name:"Keyboard".into(),battery:None,kind:crate::bt::DeviceKind::Keyboard};
    h.app.bluetooth_alert(&device,true);h.frame(vec![]);
    h.app.bluetooth_alert(&device,false);
    h.app.settings.bluetooth_notifications=false;h.frame(vec![]);
    assert!(h.app.notice.as_ref().is_none_or(|a|a.title!="蓝牙设备"));
    assert!(!h.app.alerts.iter().any(|a|a.title=="蓝牙设备"));
    h.app.bluetooth_alert(&device,true);
    assert!(!h.app.alerts.iter().any(|a|a.title=="蓝牙设备"));
    h.app.settings.save();assert!(!crate::model::Settings::load().bluetooth_notifications);
    h.bt=vec![device];h.app.settings.bluetooth_notifications=true;h.frame(vec![]);
    assert!(h.app.notice.as_ref().is_none_or(|a|a.title!="蓝牙设备"));
    h.bt.clear();h.frame(vec![]);
    assert!(h.app.notice.as_ref().is_some_and(|a|a.title=="蓝牙设备") || h.app.alerts.iter().any(|a|a.title=="蓝牙设备"));
}

#[test]
#[ignore = "按需导出五套皮肤的预览图：DAOZAI_SKIN_PREVIEW_DIR=目录 cargo test skin_gallery -- --ignored"]
fn skin_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_SKIN_PREVIEW_DIR").expect("需指定预览目录"));
    std::fs::create_dir_all(&dest).unwrap();
    let (w,hgt)=((crate::app::CANVAS_W*2.0) as usize,(WIN_H*2.0) as usize);
    // 日落渐变 + 几条城市剪影，用来观察半透明皮肤的通透感。
    let backdrop=|raster:&mut crate::preview_raster::Raster|{
        for y in 0..hgt {for x in 0..w {
            let t=y as f32/hgt as f32;let s=x as f32/w as f32;
            let mut c=[0.98-0.25*t+0.05*s,0.62+0.12*t-0.1*s,0.45+0.35*t,1.0];
            if (y>hgt*7/10) && ((x/37)%3!=0) && y>hgt*7/10+((x*7919)%97) {c=[0.30,0.33,0.45,1.0];}
            let cloud=((x as f32/90.0).sin()*(y as f32/40.0).cos()).max(0.0);if y<hgt/2 {for k in 0..3{c[k]=c[k]+(1.0-c[k])*cloud*0.6;}}
            if x%160<12 && y<hgt*6/10 {c=[0.18,0.20,0.30,1.0];}
            raster.buf[y*w+x]=c;
        }}
    };
    for (skin,shape) in crate::model::Skin::ALL.into_iter().flat_map(|s|crate::model::IslandShape::ALL.map(|p|(s,p))) {
        for open in [false,true] {
            let mut h=H::new();h.app.settings.skin=skin;h.app.settings.island_shape=shape;h.app.settings.pet=true;h.app.opts.pet_idle=0.0;
            h.media=crate::smtc::NowPlaying{title:"Paper Boats".into(),artist:"The Lanterns".into(),position:64.0,duration:201.0,playing:true,..Default::default()};
            let mut raster=crate::preview_raster::Raster::new(w,hgt);
            for frame in 0..120 {
                let t=frame as f64/60.0;
                let cursor=open.then_some(Pos2::new(WIN_W/2.0,14.0));
                let env=Env{t,clock:"11:13:00".into(),today:"2026-10-26".into(),date:"10月26日 周一".into(),media:h.media.clone(),cursor,backdrop:Some([0.85,0.62,0.6]),glass_blur:true,..Default::default()};
                let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(t),
                    viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
                let mut glass=None;
                let output=h.ctx.run(raw,|ctx|{
                    egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let win=Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,WIN_H));glass=h.app.frame(ui,win,&env).glass;});
                });
                raster.update_textures(&output.textures_delta);
                if open && frame==100 {h.app.show_page(1);}
                if open && frame==104 && shape==crate::model::IslandShape::Notch {
                    backdrop(&mut raster);
                    raster.draw(&h.ctx.tessellate(output.shapes.clone(),output.pixels_per_point),output.pixels_per_point);
                    let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                    let image=image::RgbaImage::from_raw(w as u32,hgt as u32,rgba).unwrap();
                    image::imageops::crop_imm(&image,120,0,1040,400).to_image().save(dest.join(format!("{}-switch.png",skin.key()))).unwrap();
                }
                if frame==119 {
                    backdrop(&mut raster);
                    // 模拟系统实时模糊：只有岛体轮廓内的背景被模糊（与实机的合成层一致）。
                    if let Some(shapes)=glass {
                        let pieces=crate::app::blur_pieces(&shapes,2.0);
                        let mut blurred=raster.buf.clone();
                        for _ in 0..3 {
                            let src=blurred.clone();let r=18i32;
                            for y in 0..hgt as i32 {for x in 0..w as i32 {let mut s=[0.0f32;4];let mut n=0.0;for dx in (-r..=r).step_by(3){let xx=(x+dx).clamp(0,w as i32-1);let c=src[(y as usize)*w+xx as usize];for k in 0..4{s[k]+=c[k];}n+=1.0;}blurred[(y as usize)*w+x as usize]=s.map(|v|v/n);}}
                            let src=blurred.clone();
                            for y in 0..hgt as i32 {for x in 0..w as i32 {let mut s=[0.0f32;4];let mut n=0.0;for dy in (-r..=r).step_by(3){let yy=(y+dy).clamp(0,hgt as i32-1);let c=src[(yy as usize)*w+x as usize];for k in 0..4{s[k]+=c[k];}n+=1.0;}blurred[(y as usize)*w+x as usize]=s.map(|v|v/n);}}
                        }
                        for y in 0..hgt {for x in 0..w {
                            let (px,py)=(x as f32+0.5,y as f32+0.5);
                            let inside=pieces.iter().any(|p|{px>=p.x&&px<p.x+p.w&&py>=p.y&&py<p.y+p.h&&{let cx=px.clamp(p.x+p.radius,p.x+p.w-p.radius);let cy=py.clamp(p.y+p.radius,p.y+p.h-p.radius);(px-cx).powi(2)+(py-cy).powi(2)<=p.radius*p.radius}});
                            if inside {raster.buf[y*w+x]=blurred[y*w+x];}
                        }}
                    }
                    raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                    let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                    let image=image::RgbaImage::from_raw(w as u32,hgt as u32,rgba).unwrap();
                    let crop=if open{image::imageops::crop_imm(&image,120,0,1040,400)}else{image::imageops::crop_imm(&image,300,0,680,110)};
                    crop.to_image().save(dest.join(format!("{}-{}-{}.png",skin.key(),shape.key(),if open{"open"}else{"closed"}))).unwrap();
                }
                raster.free_textures(&output.textures_delta);
            }
        }
    }
}

#[test]
#[ignore = "按需导出：带 AI 额度与吉祥物的收起态，以及设置页常驻排列预览"]
fn skin_gallery_resident(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_SKIN_PREVIEW_DIR").expect("需指定预览目录"));
    std::fs::create_dir_all(&dest).unwrap();
    for skin in crate::model::Skin::ALL {
        for shape in crate::model::IslandShape::ALL {
            let mut h=H::new();h.app.settings.skin=skin;h.app.settings.island_shape=shape;h.app.opts.pet_idle=0.0;
            h.quota_accounts(2);h.app.quota.config.colors.insert("preview-0".into(),crate::quota::Accent::Blue);
            // 岛体收起态
            let (w,hgt)=((crate::app::CANVAS_W*2.0) as usize,(WIN_H*2.0) as usize);
            let mut raster=crate::preview_raster::Raster::new(w,hgt);
            for frame in 0..90 {
                let t=frame as f64/60.0;
                let env=Env{t,clock:"21:54:00".into(),today:"2026-11-04".into(),date:"11月4日 周三".into(),backdrop:Some([0.55,0.68,0.75]),glass_blur:true,..Default::default()};
                let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(t),
                    viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
                let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let win=Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,WIN_H));h.app.frame(ui,win,&env);});});
                raster.update_textures(&output.textures_delta);
                if frame==89 {
                    raster.buf.fill([0.62,0.72,0.78,1.0]);
                    raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                    let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                    let image=image::RgbaImage::from_raw(w as u32,hgt as u32,rgba).unwrap();
                    image::imageops::crop_imm(&image,300,0,680,90).to_image().save(dest.join(format!("bar-{}-{}.png",skin.key(),shape.key()))).unwrap();
                }
                raster.free_textures(&output.textures_delta);
            }
            // 设置页常驻排列预览
            let mut raster=crate::preview_raster::Raster::new(1200,300);
            for frame in 0..3 {
                let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(600.0,150.0))),time:Some(frame as f64),
                    viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
                let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().show(ctx,|ui|{h.app.resident_order_settings(ui);});});
                raster.update_textures(&output.textures_delta);
                if frame==2 {
                    raster.buf.fill([0.13,0.13,0.15,1.0]);
                    raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                    let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                    image::RgbaImage::from_raw(1200,300,rgba).unwrap().save(dest.join(format!("resident-{}-{}.png",skin.key(),shape.key()))).unwrap();
                }
                raster.free_textures(&output.textures_delta);
            }
        }
    }
}

#[test]
#[ignore = "按需导出：浅色皮肤下每个页面的展开态，用于排查配色"]
fn skin_gallery_pages(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_SKIN_PREVIEW_DIR").expect("需指定预览目录"));
    std::fs::create_dir_all(&dest).unwrap();
    let skins=std::env::var("DAOZAI_SKINS").unwrap_or_else(|_|"glass_light,cream,classic".into());
    for skin in skins.split(',').map(crate::model::Skin::parse) {
        for page in 0..9usize {
            let mut h=H::new();h.app.settings.skin=skin;h.app.settings.island_shape=crate::model::IslandShape::Pill;
            h.quota_accounts(2);
            h.app.todos.items.push(crate::model::TodoItem{text:"整理岛仔皮肤反馈".into(),done_on:None,due:Some("2026-11-05 10:00".into()),reminded:false,..Default::default()});
            h.app.todos.items.push(crate::model::TodoItem{text:"已完成的事项".into(),done_on:Some("2026-11-04".into()),due:None,reminded:false,..Default::default()});
            h.media=crate::smtc::NowPlaying{title:"Paper Boats".into(),artist:"The Lanterns".into(),position:64.0,duration:201.0,playing:true,..Default::default()};
            let (w,hgt)=((crate::app::CANVAS_W*2.0) as usize,(WIN_H*2.0) as usize);
            let mut raster=crate::preview_raster::Raster::new(w,hgt);
            for frame in 0..140 {
                let t=frame as f64/60.0;
                if frame==40 { if page==8 {h.app.open_calendar_editor(None);} else {h.app.show_page(page);} }
                let env=Env{t,clock:"22:46:00".into(),today:"2026-11-04".into(),date:"11月4日 周三".into(),media:h.media.clone(),cursor:Some(Pos2::new(WIN_W/2.0,20.0)),backdrop:Some([0.8,0.84,0.88]),glass_blur:true,..Default::default()};
                let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(t),
                    viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
                let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let win=Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,WIN_H));h.app.frame(ui,win,&env);});});
                raster.update_textures(&output.textures_delta);
                if frame==139 {
                    raster.buf.fill([0.80,0.84,0.88,1.0]);
                    raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                    let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                    let image=image::RgbaImage::from_raw(w as u32,hgt as u32,rgba).unwrap();
                    image::imageops::crop_imm(&image,160,0,960,500).to_image().save(dest.join(format!("page-{}-{page}.png",skin.key()))).unwrap();
                }
                raster.free_textures(&output.textures_delta);
            }
        }
    }
}

#[test]
#[ignore = "按需导出 DeepSeek 余额界面"]
fn deepseek_balance_gallery(){
    use crate::quota::{Provider,Event};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_DEEPSEEK_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for skin in [crate::model::Skin::Classic,crate::model::Skin::Cream,crate::model::Skin::GlassLight,crate::model::Skin::GlassDark]{
    for (name,amount,stale,count) in [("normal","4.18",false,2),("large","12800.00",false,2),("overflow","128000.00",false,5),("stale","12800.00",true,2),("loading","4.18",false,2),("error","4.18",false,2)] {for open in [false,true]{
        let mut h=H::new();h.app.settings.pet=false;h.app.settings.skin=skin;h.app.settings.show_battery=true;h.quota_accounts(count);
        h.app.quota.config.accounts[1].provider=Provider::Claude;h.app.quota.config.accounts[1].alias="Claude".into();h.app.quota.config.accounts[0].alias="Codex".into();
        let id=h.app.quota.config.connect_deepseek("preview".into(),"DeepSeek".into());
        let snapshot=crate::quota::parse(Provider::DeepSeek,&serde_json::json!({"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":amount,"topped_up_balance":amount,"granted_balance":"0.00"}]})).unwrap();
        h.app.quota_event(Event{id:id.clone(),result:Ok(snapshot)});if stale{h.app.quota_event(Event{id:id.clone(),result:Err("无法连接".into())});}
        if name=="loading"{h.app.quota.rows.remove(&id);h.app.quota.busy.insert(id.clone());}
        if name=="error"{h.app.quota.rows.remove(&id);h.app.quota_event(Event{id,result:Err("暂时无法查询余额，请稍后重试".into())});}
        if open{h.app.show_page(6);}
        let mut raster=crate::preview_raster::Raster::new(1280,380);
        for frame in 0..90{
            let t=frame as f64/60.0;let env=Env{t,clock:"09:41:00".into(),battery:Some(80),charging:true,cursor:if open{Some(Pos2::new(320.0,60.0))}else{None},..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(640.0,190.0))),time:Some(t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
            let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{h.app.frame(ui,ui.max_rect(),&env);});});
            raster.update_textures(&output.textures_delta);
            if frame==89{
                raster.buf.fill([0.09,0.11,0.18,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                image::save_buffer(dest.join(format!("{}-{name}-{}.png",skin.key(),if open{"open"}else{"closed"})),&rgba,1280,380,image::ColorType::Rgba8).unwrap();
            }
            raster.free_textures(&output.textures_delta);
        }
    }}}
}

#[test]
fn deepseek_compact_amount_fits_text_and_opens_account(){
    use crate::quota::{Provider,Event};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.quota_accounts(5);h.app.settings.pet=false;h.app.settings.resident_order=vec![1,9,2,0,5,6,7];
    let id=h.app.quota.config.connect_deepseek("preview".into(),"DeepSeek".into());let mut width:Option<f32>=None;let mut amount_rect=Rect::NOTHING;
    for (amount,label) in [("4.18","¥4.1"),("12800.00","¥1.28万"),("128000.00","¥12.8万")]{
        let snapshot=crate::quota::parse(Provider::DeepSeek,&serde_json::json!({"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":amount,"topped_up_balance":amount,"granted_balance":"0.00"}]})).unwrap();h.app.quota_event(Event{id:id.clone(),result:Ok(snapshot)});
        for _ in 0..90{h.frame(vec![]);}
        let text_rect=|label:&str|h.shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Text(text) if text.galley.text()==label=>Some(text.visual_bounding_rect()),_=>None}).expect("常驻金额应可见");
        let fonts:Vec<_>=h.shapes.iter().filter_map(|shape|match &shape.shape{egui::Shape::Text(text) if text.galley.text()==label||text.galley.text()=="09:41"=>Some(text.galley.job.sections[0].format.font_id.clone()),_=>None}).collect();assert_eq!(fonts.len(),2);assert_eq!(fonts[0],fonts[1],"余额与时钟使用相同字体和字号");
        amount_rect=text_rect(label);assert!(amount_rect.right()<h.region.right());
        let gap=text_rect("09:41").left()-amount_rect.right();assert!((10.0..20.0).contains(&gap),"金额与时钟之间不应预留大片空白：{gap}");
        if let Some(short_width)=width{assert!(h.app.current_width()>short_width+10.0,"短金额必须释放多余占位");}else{width=Some(h.app.current_width());}
    }
    h.click(amount_rect.center());assert_eq!(h.app.page(),6);assert_eq!(h.app.quota.config.selected,id);
}

#[test]
#[ignore = "按需导出 DeepSeek 连接状态提示"]
fn deepseek_connection_preview(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.app.quota.provider=crate::quota::Provider::DeepSeek;h.app.quota.note="DeepSeek 已连接".into();
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_DEEPSEEK_PREVIEW_DIR").expect("需指定预览目录"));
    let mut raster=crate::preview_raster::Raster::new(1200,1400);
    for frame in 0..8{
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(600.0,700.0))),time:Some(frame as f64/60.0),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().show(ctx,|ui|{h.app.quota_settings(ui);});});raster.update_textures(&output.textures_delta);
        if frame==7{raster.buf.fill([0.1,0.1,0.1,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join("connection-status.png"),&rgba,1200,1400,image::ColorType::Rgba8).unwrap();}
        raster.free_textures(&output.textures_delta);
    }
}

#[test]
fn deepseek_resident_preview_does_not_overlap_icons(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.quota_accounts(2);h.app.settings.skin=crate::model::Skin::Classic;
    let id=h.app.quota.config.connect_deepseek("preview".into(),"DeepSeek".into());h.app.quota_event(crate::quota::Event{id,result:crate::quota::parse(crate::quota::Provider::DeepSeek,&serde_json::json!({"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"12800","topped_up_balance":"12800","granted_balance":"0"}]}))});
    let mut raster=crate::preview_raster::Raster::new(1200,380);
    for frame in 0..8{
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(600.0,190.0))),time:Some(frame as f64/60.0),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().show(ctx,|ui|{h.app.resident_order_settings(ui);});});raster.update_textures(&output.textures_delta);
        if frame==7{
            let circles:Vec<_>=output.shapes.iter().filter_map(|shape|if let egui::Shape::Circle(c)=&shape.shape{Some(c)}else{None}).collect();
            let whale=output.shapes.iter().find_map(|shape|if let egui::Shape::Mesh(mesh)=&shape.shape{let rect=mesh.calc_bounds();((rect.width()-24.0).abs()<0.1).then_some(rect)}else{None}).expect("DeepSeek 图标");
            let claude=circles.iter().find(|c|c.stroke.color==egui::Color32::from_rgb(255,116,26)&&c.radius>9.0).expect("Claude 圆环");
            assert!(whale.left()>claude.center.x+claude.radius+4.0,"DeepSeek 不应覆盖前一枚图标");
            if let Some(dest)=std::env::var_os("DAOZAI_DEEPSEEK_PREVIEW_DIR"){
                raster.buf.fill([0.1,0.1,0.1,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(std::path::PathBuf::from(dest).join("resident-preview.png"),&rgba,1200,380,image::ColorType::Rgba8).unwrap();
            }
        }
        raster.free_textures(&output.textures_delta);
    }
}

#[test]
fn quota_cards_expose_names_colors_settings_and_connection(){
    fn frame(h:&mut H,events:Vec<egui::Event>){
        h.t+=1.0/60.0;let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(600.0,800.0))),time:Some(h.t),events,..Default::default()};
        let out=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().show(ctx,|ui|{h.app.quota_settings(ui);});});h.shapes=out.shapes;
    }
    fn click(h:&mut H,label:&str){
        let pos=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()==label=>Some(t.visual_bounding_rect().center()),_=>None}).expect("操作入口应可见");
        frame(h,vec![egui::Event::PointerMoved(pos)]);
        frame(h,vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
        frame(h,vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
        frame(h,vec![]);
    }
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.quota_accounts(2);let id=h.app.quota.config.connect_deepseek("fixture".into(),"工作".into());h.app.quota_event(crate::quota::Event{id:id.clone(),result:Err("API Key 验证失败".into())});
    for _ in 0..8{frame(&mut h,vec![]);}
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="备注"||t.galley.text()=="服务")));
    for label in ["颜色","监控","工作","移除","当前"]{assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()==label)),"应直接显示 {label}");}
    click(&mut h,"工作");
    frame(&mut h,vec![egui::Event::Key{key:egui::Key::A,physical_key:None,pressed:true,repeat:false,modifiers:egui::Modifiers{ctrl:true,command:true,..Default::default()}},egui::Event::Text("备用账号".into())]);
    assert_eq!(h.app.quota.config.accounts.iter().find(|a|a.id==id).unwrap().alias,"备用账号","账号名称应可在行内修改");
    click(&mut h,"监控");assert!(!h.app.quota.config.accounts[0].enabled);click(&mut h,"监控");
    let handles:Vec<_>=h.shapes.iter().filter_map(|s|match &s.shape{egui::Shape::Text(t) if t.galley.text()=="::"=>Some(t.visual_bounding_rect().center()),_=>None}).collect();
    let from=handles[0];let to=handles[1];
    frame(&mut h,vec![egui::Event::PointerMoved(from),egui::Event::PointerButton{pos:from,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
    frame(&mut h,vec![egui::Event::PointerMoved(from+egui::vec2(12.0,0.0))]);
    frame(&mut h,vec![egui::Event::PointerMoved(to)]);
    frame(&mut h,vec![egui::Event::PointerButton{pos:to,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
    assert_eq!(h.app.quota.config.accounts[1].id,"preview-0","拖放应改变卡片顺序");
    h.app.quota.config.move_account("preview-0","preview-1");for _ in 0..3{frame(&mut h,vec![]);}
    click(&mut h,"常驻岛上");assert!(!h.app.quota.config.accounts[0].pinned);
    click(&mut h,"重试");assert!(h.app.pending.iter().any(|cmd|matches!(cmd,crate::app::Cmd::QuotaRefresh)));assert!(!h.app.quota.connect_open);
    click(&mut h,"重新连接");assert!(h.app.quota.connect_open);assert_eq!(h.app.quota.connect_target.as_deref(),Some("preview-0"));
    h.app.quota.connect_open=false;for _ in 0..8{frame(&mut h,vec![]);}click(&mut h,"＋ 添加账号");assert!(h.app.quota.connect_open);assert!(h.app.quota.connect_target.is_none());assert!(h.app.quota.alias.is_empty());
    h.app.quota.connect_open=false;for _ in 0..8{frame(&mut h,vec![]);}assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="···"||t.galley.text()=="设置")));
}

#[test]
#[ignore = "按需导出卡片式 AI 额度设置页"]
fn quota_cards_settings_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_DEEPSEEK_PREVIEW_DIR").expect("需指定预览目录"));
    for (name,width,light,dialog) in [("dark",690.0,false,false),("light",690.0,true,false),("wide",900.0,false,false),("connect",690.0,false,true),("narrow",600.0,false,false)]{
        let mut h=H::new();h.quota_accounts(2);h.app.quota.config.accounts[0].alias="本机账号".into();h.app.quota.config.accounts[1].alias="桌面版 1".into();
        for (i,remaining) in [54.0,72.0].into_iter().enumerate(){let account_id=h.app.quota.config.accounts[i].id.clone();h.app.quota_event(crate::quota::Event{id:account_id,result:Ok(crate::quota::Snapshot{windows:vec![crate::quota::Window{label:"5 小时".into(),seconds:Some(18000),remaining:remaining+20.0,reset:None,amount:None},crate::quota::Window{label:"本周".into(),seconds:Some(604800),remaining,reset:None,amount:None}],at:chrono::Utc::now().timestamp(),..Default::default()})});}
        let id=h.app.quota.config.connect_deepseek("preview".into(),"DeepSeek".into());h.app.quota_event(crate::quota::Event{id,result:Err("API Key 验证失败，请重新连接账号".into())});
        h.app.quota.connect_open=dialog;h.app.quota.provider=crate::quota::Provider::DeepSeek;
        h.app.settings.settings_theme=if light{crate::settings_theme::SettingsTheme::Light}else{crate::settings_theme::SettingsTheme::Dark};h.app.settings_tab=11;h.app.open_settings();h.ctx.set_embed_viewports(false);
        let size=egui::vec2(width,650.0);let ppp=1.5;let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{let id=viewport.ids.this;let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(ppp),focused:Some(true),..Default::default()})).collect(),focused:true,..Default::default()};*capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));});
        let (w,height)=((size.x*ppp)as u32,(size.y*ppp)as u32);let mut raster=crate::preview_raster::Raster::new(w as usize,height as usize);
        for frame in 0..8{let root=h.ctx.run(egui::RawInput{viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()},|ctx|h.app.settings_ui(ctx,false));raster.update_textures(&root.textures_delta);let child=output.lock().unwrap().take().unwrap();raster.update_textures(&child.textures_delta);
            if frame==7{raster.buf.fill([0.0,0.0,0.0,1.0]);raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join(format!("quota-cards-{name}.png")),&rgba,w,height,image::ColorType::Rgba8).unwrap();}
            raster.free_textures(&root.textures_delta);raster.free_textures(&child.textures_delta);
        }
    }
}

/// 新功能画面：按需导出，检查排版。
fn feature_raster()->crate::preview_raster::Raster{crate::preview_raster::Raster::new((crate::app::CANVAS_W*2.0) as usize,(WIN_H*2.0) as usize)}
fn shoot_feature(h:&mut H,raster:&mut crate::preview_raster::Raster,dest:&std::path::Path,name:&str,frames:usize,cursor:Option<Pos2>,today:&str){
    let ppp=2.0;let (w,hgt)=(crate::app::CANVAS_W,WIN_H);
    for f in 0..frames {
        h.t+=1.0/60.0;
        let env=Env{t:h.t,clock:"09:41:00".into(),today:today.into(),date:"11月4日 周三".into(),cursor,bt_ready:true,..Default::default()};
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt))),time:Some(h.t),focused:true,
            events:cursor.map(|p|vec![egui::Event::PointerMoved(p)]).unwrap_or_default(),
            viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
        let app=&mut h.app;
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let win=Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,WIN_H));app.frame(ui,win,&env);});});
        raster.update_textures(&output.textures_delta);
        let primitives=h.ctx.tessellate(output.shapes,output.pixels_per_point);
        if f+1==frames {
            raster.buf.fill([0.20,0.24,0.32,1.0]);
            raster.draw(&primitives,output.pixels_per_point);
            let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            let image=image::RgbaImage::from_raw((w*ppp) as u32,(hgt*ppp) as u32,rgba).unwrap();
            let crop_h=(h.app.expanded_height()*ppp+24.0).max(330.0).min(hgt*ppp) as u32;
            image::imageops::crop_imm(&image,200,0,880,crop_h).to_image().save(dest.join(format!("{name}.png"))).unwrap();
        }
        raster.free_textures(&output.textures_delta);
    }
}

#[test]
#[ignore="按需导出每周待办与剪贴板的实际画面"]
fn feature_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_FEATURE_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    let today="2026-11-04";// 周三
    let todos=||{
        let mut a=crate::model::TodoItem::new("写周报");a.repeat=0b001_0000;
        let mut b=crate::model::TodoItem::new("站会");b.repeat=crate::model::REPEAT_WORKDAYS;b.due=Some("2026-11-04 09:30".into());b.done_on=Some("2026-11-04".into());
        let mut c=crate::model::TodoItem::new("健身");c.repeat=0b000_0101;
        let d=crate::model::TodoItem::new("回复设计评审意见");
        vec![a,b,c,d]
    };
    let mut h=H::new();let mut r=feature_raster();h.app.todos.items=todos();h.app.show_page(4);
    shoot_feature(&mut h,&mut r,&dest,"todo-page",90,Some(H::local(120.0,60.0)),today);
    let mut h=H::new();let mut r=feature_raster();h.app.todos.items=todos();h.app.show_page(4);
    shoot_feature(&mut h,&mut r,&dest,"todo-editor-0",30,Some(H::local(120.0,60.0)),today);
    h.app.open_task_dialog(2);
    shoot_feature(&mut h,&mut r,&dest,"todo-editor",60,Some(H::local(120.0,60.0)),today);
    let mut h=H::new();let mut r=feature_raster();h.app.todos.items=todos();h.app.show_page(4);
    shoot_feature(&mut h,&mut r,&dest,"todo-picker-0",30,Some(H::local(120.0,60.0)),today);
    h.app.open_task_reminder(3);
    shoot_feature(&mut h,&mut r,&dest,"todo-picker",60,Some(H::local(120.0,60.0)),today);
    // 剪贴板
    let mut h=H::new();let mut r=feature_raster();h.app.show_page(3);h.app.shelf_tab=1;
    shoot_feature(&mut h,&mut r,&dest,"clip-empty",60,Some(H::local(120.0,60.0)),today);
    {let mut hist=h.app.clipboard.lock();hist.items.clear();
        for (i,t) in ["https://github.com/Renly1994/xiaotang-daozai/releases","会议纪要：\n1. 周三前确认岛仔竖版方案\n2. 剪贴板记录上线","13800138000","SELECT * FROM todo WHERE repeat > 0 ORDER BY due;","验证码 482913"].iter().enumerate(){hist.push(t,&format!("09:{:02}",30+i));}
        hist.items[3].pinned=true;}
    shoot_feature(&mut h,&mut r,&dest,"clip-list",30,Some(H::local(200.0,66.0)),today);
    h.app.shelf_tab=0;
    let file=std::env::temp_dir().join("岛仔周报.docx");std::fs::write(&file,b"x").unwrap();h.app.shelf.items=vec![file];
    shoot_feature(&mut h,&mut r,&dest,"shelf-files",30,Some(H::local(120.0,60.0)),today);
    h.app.confirm_clear=Some(h.app.t);
    shoot_feature(&mut h,&mut r,&dest,"shelf-confirm",5,Some(H::local(120.0,60.0)),today);
    h.app.settings.clipboard_history=false;h.app.shelf_tab=1;
    shoot_feature(&mut h,&mut r,&dest,"clip-off",30,Some(H::local(120.0,60.0)),today);
}

static SIDE_MUSIC:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
/// 侧边停靠的画面：画布为 SIDE_W × SIDE_H。
fn shoot_side(h:&mut H,raster:&mut crate::preview_raster::Raster,dest:&std::path::Path,name:&str,frames:usize,cursor:Option<Pos2>){
    let ppp=2.0;let (w,hgt)=(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H);
    for f in 0..frames {
        h.t+=1.0/60.0;
        let mut env=Env{t:h.t,clock:"09:41:00".into(),today:"2026-11-04".into(),date:"11月4日 周三".into(),cursor,bt_ready:true,battery:Some(76),..Default::default()};
        if SIDE_MUSIC.load(std::sync::atomic::Ordering::Relaxed) {env.media.title="晴天".into();env.media.artist="周杰伦".into();env.media.playing=true;env.media.source="cloudmusic.exe".into();env.media.duration=269.0;}
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt))),time:Some(h.t),focused:true,
            events:cursor.map(|p|vec![egui::Event::PointerMoved(p)]).unwrap_or_default(),
            viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
        let app=&mut h.app;let mut out=None;
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let win=Rect::from_min_size(ui.max_rect().min,Vec2::new(w,hgt));out=Some(app.frame(ui,win,&env));});});
        raster.update_textures(&output.textures_delta);
        if f+1==frames {
            raster.buf.fill([0.20,0.24,0.32,1.0]);
            raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
            let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            let mut image=image::RgbaImage::from_raw((w*ppp) as u32,(hgt*ppp) as u32,rgba).unwrap();
            // 画出窗口区域（红）与命中区域（绿）的边框，方便检查。
            if let Some(out)=out {
                for (r,color) in [(out.region,image::Rgba([255,80,80,255])),(out.hit,image::Rgba([80,255,120,255]))] {
                    let (x0,y0,x1,y1)=((r.left()*ppp).max(0.0) as u32,(r.top()*ppp).max(0.0) as u32,((r.right()*ppp) as u32).min(image.width()-1),((r.bottom()*ppp) as u32).min(image.height()-1));
                    for x in x0..=x1 {image.put_pixel(x,y0,color);image.put_pixel(x,y1,color);}
                    for y in y0..=y1 {image.put_pixel(x0,y,color);image.put_pixel(x1,y,color);}
                }
            }
            image.save(dest.join(format!("{name}.png"))).unwrap();
        }
        raster.free_textures(&output.textures_delta);
    }
}

#[test]
#[ignore="按需导出左右停靠的实际画面"]
fn side_dock_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_FEATURE_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    let (w,hgt)=(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H);
    for (dock,shape,skin) in [(crate::model::Dock::Left,crate::model::IslandShape::Notch,crate::model::Skin::Classic),(crate::model::Dock::Right,crate::model::IslandShape::Pill,crate::model::Skin::Cream)] {
        let tag=format!("{}-{}",dock.key(),shape.key());
        let mut h=H::new();let mut r=feature_raster_sized(w,hgt);
        h.app.settings.dock=dock;h.app.settings.island_shape=shape;h.app.settings.skin=skin;h.app.settings.pet=false;
        h.app.media.title="晴天".into();h.app.media.artist="周杰伦".into();h.app.media.playing=true;h.app.media.source="cloudmusic.exe".into();
        let away=Some(Pos2::new(w/2.0,hgt-10.0));
        shoot_side(&mut h,&mut r,&dest,&format!("side-{tag}-collapsed"),60,away);
        let edge_x=if dock==crate::model::Dock::Left{8.0}else{w-8.0};
        shoot_side(&mut h,&mut r,&dest,&format!("side-{tag}-hover"),8,Some(Pos2::new(edge_x,hgt/2.0)));
        shoot_side(&mut h,&mut r,&dest,&format!("side-{tag}-opening"),12,Some(Pos2::new(edge_x,hgt/2.0)));
        shoot_side(&mut h,&mut r,&dest,&format!("side-{tag}-open"),70,Some(Pos2::new(edge_x,hgt/2.0)));
        h.app.show_page(4);
        shoot_side(&mut h,&mut r,&dest,&format!("side-{tag}-todo"),50,Some(Pos2::new(edge_x,hgt/2.0)));
        shoot_side(&mut h,&mut r,&dest,&format!("side-{tag}-closed"),320,away);
        h.app.alert("待办到时间了","回复设计评审意见",4,false);
        shoot_side(&mut h,&mut r,&dest,&format!("side-{tag}-notice"),40,away);
    }
    // 播放音乐 + 专注中 + 毛玻璃；极简模式；侧边提示。
    SIDE_MUSIC.store(true,std::sync::atomic::Ordering::Relaxed);
    let mut h=H::new();let mut r=feature_raster_sized(w,hgt);
    h.app.settings.dock=crate::model::Dock::Left;h.app.settings.skin=crate::model::Skin::GlassDark;
    h.app.pomo.running=true;h.app.pomo.remaining=1430.0;
    side_quota_fixture(&mut h);
    let away=Some(Pos2::new(w/2.0,hgt-10.0));
    shoot_side(&mut h,&mut r,&dest,"side-left-glass-busy",60,away);
    shoot_side(&mut h,&mut r,&dest,"side-left-glass-open",70,Some(Pos2::new(8.0,hgt/2.0)));
    shoot_side(&mut h,&mut r,&dest,"side-left-glass-closed",120,away);
    h.app.code_copied("1");
    shoot_side(&mut h,&mut r,&dest,"side-left-toast",20,away);
    SIDE_MUSIC.store(false,std::sync::atomic::Ordering::Relaxed);
    let mut h=H::new();let mut r=feature_raster_sized(w,hgt);
    h.app.settings.dock=crate::model::Dock::Right;h.app.settings.minimal=true;
    shoot_side(&mut h,&mut r,&dest,"side-right-minimal",60,away);
}
fn feature_raster_sized(w:f32,h:f32)->crate::preview_raster::Raster{crate::preview_raster::Raster::new((w*2.0) as usize,(h*2.0) as usize)}

/// 三个环形额度账号，随后显示 WorkBuddy 与 DeepSeek 余额。
fn side_quota_fixture(h:&mut H)->(String,String){
    use crate::quota::{Provider,Event};
    h.quota_accounts(3);
    let wb=h.app.quota.config.connect_workbuddy("fixture".into(),"WorkBuddy".into());
    let ds=h.app.quota.config.connect_deepseek("fixture".into(),"DeepSeek".into());
    h.app.quota_event(Event{id:wb.clone(),result:crate::quota::parse(Provider::WorkBuddy,&serde_json::json!({"code":0,"data":{"Packages":[{"CycleRemainCapacity":"28560"}]}}))});
    h.app.quota_event(Event{id:ds.clone(),result:crate::quota::parse(Provider::DeepSeek,&serde_json::json!({"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"4.18","granted_balance":"0","topped_up_balance":"4.18"}]}))});
    (wb,ds)
}

#[test]
fn side_dock_stacks_ai_quota_and_opens_the_account(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let (w,hgt)=(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H);
    let mut h=H::new();h.app.settings.dock=crate::model::Dock::Left;
    let (wb,_)=side_quota_fixture(&mut h);
    let away=Some(Pos2::new(w/2.0,hgt-5.0));
    for _ in 0..60 {side_frame(&mut h,away);}
    let items=h.app.quota_side_items();
    assert_eq!(items.len(),h.app.quota.config.pinned_indices().len());
    let order:Vec<_>=items.iter().map(|(i,_)|h.app.quota.config.accounts[*i].provider).collect();
    assert!(items.iter().any(|(i,_)|h.app.quota.config.accounts[*i].id==wb),"WorkBuddy 应在竖向额度里");
    // 胶囊变长以容纳额度，仍完整留在画布内并竖直居中。
    let out=side_frame(&mut h,away);
    assert!(out.hit.height()>32.0+38.0*2.0,"{:?} {order:?}",out.hit);
    assert!(out.hit.top()>0.0 && out.hit.bottom()<hgt);
    // 点击 WorkBuddy 那一项（按竖排顺序算出它的中心）：选中该账号并打开 AI 额度页。
    let top=h.app.island_rect(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt)),h.app.w.pos,h.app.h.pos).top();
    let total=h.app.side_bar_len()-14.0;
    let mut y=top+(h.app.h.pos-total)/2.0;
    for item in h.app.side_items() {
        let height=match item {crate::side_dock::SideItem::Quota(i,hh) if h.app.quota.config.accounts[i].id==wb=>{y+=hh/2.0;break;},crate::side_dock::SideItem::Quota(_,hh)=>hh,crate::side_dock::SideItem::Clock=>32.0,crate::side_dock::SideItem::Weather=>40.0,crate::side_dock::SideItem::Battery=>20.0,crate::side_dock::SideItem::Music=>18.0,crate::side_dock::SideItem::Focus|crate::side_dock::SideItem::Bell=>22.0};
        y+=height;
    }
    let click=Pos2::new(18.0,y);
    side_frame(&mut h,Some(click));
    {
        let (w,hgt)=(w,hgt);
        h.t+=1.0/60.0;
        let env=Env{t:h.t,clock:"09:41:00".into(),cursor:Some(click),..Default::default()};
        for events in [vec![egui::Event::PointerMoved(click),egui::Event::PointerButton{pos:click,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}],vec![egui::Event::PointerButton{pos:click,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]] {
            h.t+=1.0/60.0;
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt))),time:Some(h.t),focused:true,events,..Default::default()};
            let app=&mut h.app;let env=Env{t:h.t,..env.clone()};
            let _=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(w,hgt)),&env);});});
        }
    }
    assert_eq!(h.app.quota.config.selected,wb);
    assert_eq!(h.app.page(),6);
}

#[test]
fn minimal_side_pill_returns_to_line_while_wechat_is_unread(){
    use crate::model::{Dock,IslandShape};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dock in [Dock::Right,Dock::Left] {
        let mut h=H::new();
        h.app.settings.dock=dock;
        h.app.settings.island_shape=IslandShape::Pill;
        h.app.settings.minimal=true;
        h.app.settings.wechat_notifications=true;
        for _ in 0..90 {side_frame_with(&mut h,None);}
        let idle=side_frame_with(&mut h,None);
        h.app.receive_wechat_reminder_from(4242,0x1234);
        for _ in 0..30 {side_frame_with(&mut h,None);}
        assert_eq!(h.app.notice.as_ref().and_then(|a|a.messenger()),Some(crate::app::Messenger::Wechat));
        for _ in 0..480 {side_frame_with(&mut h,None);}
        let collapsed=side_frame_with(&mut h,None);
        assert!(h.app.notice.is_none() && collapsed.notice_hit.is_none());
        assert_eq!(h.app.pending_messengers.len(),1);
        assert_eq!(collapsed.hit,idle.hit,"{dock:?} 有未读时也应收回细线");
        assert_eq!(collapsed.region,idle.region);
        let edge=Some(idle.hit.center());
        for _ in 0..90 {side_frame_with(&mut h,edge);}
        assert!(h.app.open && side_frame_with(&mut h,edge).notice_hit.is_some());
    }
}

#[test]
fn automatic_minimal_hides_unread_hit_area_until_reopened(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dock in [crate::model::Dock::Top,crate::model::Dock::Left,crate::model::Dock::Right] {
        let mut h=H::new();
        h.app.settings.dock=dock;
        h.app.settings.browser_minimal=true;
        h.app.settings.wechat_notifications=true;
        h.app.settings.read_notifications=true;
        h.app.receive_wechat_reminder();
        h.app.receive_notification("QQ".into(),"QQ".into(),"测试".into());
        h.app.alerts.clear();
        h.app.notice=None;
        h.app.notification_bell_until=0.0;
        h.browser_at_top=Some(true);
        for _ in 0..90 {side_frame_with(&mut h,None);}
        let out=side_frame_with(&mut h,None);
        assert!(out.notice_hit.is_none(),"{dock:?} 避让时未读仍占用鼠标区域");
        assert_eq!(h.app.pending_messengers.len(),2);
        h.app.show_page(0);
        for _ in 0..60 {side_frame_with(&mut h,None);}
        assert!(side_frame_with(&mut h,None).notice_hit.is_some());
        h.browser_at_top=Some(false);
        for _ in 0..360 {side_frame_with(&mut h,None);}
        assert!(!h.app.open && side_frame_with(&mut h,None).notice_hit.is_some());
    }
}

#[test]
fn side_dock_auto_minimal_follows_avoidance(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let (w,hgt)=(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H);
    let mut h=H::new();h.app.settings.dock=crate::model::Dock::Right;h.app.settings.browser_minimal=true;
    let away=Some(Pos2::new(w/2.0,hgt-5.0));
    for _ in 0..60 {side_frame(&mut h,away);}
    let normal=side_frame(&mut h,away).hit;
    h.browser_at_top=Some(true);
    let mut out=side_frame_with(&mut h,away);
    for _ in 0..60 {out=side_frame_with(&mut h,away);}
    assert!(out.hit.width()<normal.width() && out.hit.height()<normal.height(),"避让时收成贴边细线：{:?} → {:?}",normal,out.hit);
    h.browser_at_top=Some(false);
    for _ in 0..60 {out=side_frame_with(&mut h,away);}
    assert!((out.hit.height()-normal.height()).abs()<1.0,"离开后恢复：{:?}",out.hit);
}

fn side_frame_with(h:&mut H,cursor:Option<Pos2>)->crate::app::FrameOut{
    let at_top=h.browser_at_top;
    let (w,hgt)=(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H);
    h.t+=1.0/60.0;
    let env=Env{t:h.t,clock:"09:41:00".into(),cursor,battery:Some(76),browser_at_top:at_top,..Default::default()};
    let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt))),time:Some(h.t),focused:true,..Default::default()};
    let app=&mut h.app;let mut out=None;
    let _=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{out=Some(app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(w,hgt)),&env));});});
    out.unwrap()
}

fn side_frame(h:&mut H,cursor:Option<Pos2>)->crate::app::FrameOut{
    let (w,hgt)=(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H);
    h.t+=1.0/60.0;
    let env=Env{t:h.t,clock:"09:41:00".into(),today:"2026-11-04".into(),date:"11月4日 周三".into(),cursor,bt_ready:true,battery:Some(76),..Default::default()};
    let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt))),time:Some(h.t),focused:true,
        events:cursor.map(|p|vec![egui::Event::PointerMoved(p)]).unwrap_or_default(),..Default::default()};
    let app=&mut h.app;let mut out=None;
    let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{out=Some(app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(w,hgt)),&env));});});
    h.shapes=output.shapes;
    out.unwrap()
}

#[test]
fn side_dock_opens_from_the_edge_and_collapses_back(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let (w,hgt)=(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H);
    for (dock,shape) in [(crate::model::Dock::Left,crate::model::IslandShape::Notch),(crate::model::Dock::Right,crate::model::IslandShape::Pill)] {
        let mut h=H::new();h.app.settings.dock=dock;h.app.settings.island_shape=shape;
        let away=Some(Pos2::new(w/2.0,hgt-5.0));
        let mut out=side_frame(&mut h,away);
        for _ in 0..60 {out=side_frame(&mut h,away);}
        assert!(!h.app.open);
        // 收起：竖向胶囊贴边、竖直居中。
        assert!(out.hit.height()>out.hit.width(),"{dock:?} 收起时是竖向：{:?}",out.hit);
        assert!((out.hit.center().y-hgt/2.0).abs()<1.0);
        let edge=if dock==crate::model::Dock::Left{0.5}else{w-0.5};
        assert!(out.hit.contains(Pos2::new(edge,hgt/2.0)),"{dock:?} 光标推到屏幕边缘也能命中：{:?}",out.hit);
        assert!(out.region.contains_rect(out.hit.shrink(0.5)));
        // 推到边缘停留 → 展开，面板从边缘滑出且完整留在画布内。
        for _ in 0..60 {out=side_frame(&mut h,Some(Pos2::new(edge,hgt/2.0)));}
        assert!(h.app.open && h.app.progress()>0.95,"{dock:?} 应展开");
        assert!(out.hit.width()>crate::app::PW-1.0 && out.hit.height()<200.0,"{:?}",out.hit);
        assert!(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt)).contains_rect(out.region),"区域不能超出画布：{:?}",out.region);
        if dock==crate::model::Dock::Left {assert!(out.hit.left()<=0.5)} else {assert!(out.hit.right()>=w-0.5)}
        // 离开 → 收起。
        for _ in 0..90 {out=side_frame(&mut h,away);}
        assert!(!h.app.open && h.app.progress()<0.05);
        assert!(out.hit.width()<60.0);
    }
}

#[test]
fn workbuddy_and_deepseek_balances_stay_ordered_and_clickable(){
    use crate::quota::{Provider,Event};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.quota_accounts(5);h.app.settings.pet=false;
    let wb=h.app.quota.config.connect_workbuddy("fixture".into(),"WorkBuddy".into());
    let ds=h.app.quota.config.connect_deepseek("fixture".into(),"DeepSeek".into());
    h.app.quota_event(Event{id:wb.clone(),result:crate::quota::parse(Provider::WorkBuddy,&serde_json::json!({"code":0,"data":{"Packages":[{"CycleRemainCapacity":"28560"}]}}))});
    h.app.quota_event(Event{id:ds.clone(),result:crate::quota::parse(Provider::DeepSeek,&serde_json::json!({"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"4.18","granted_balance":"0","topped_up_balance":"4.18"}]}))});
    for _ in 0..90{h.frame(vec![]);}
    let rect=|label:&str|h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==label=>Some(t.visual_bounding_rect()),_=>None}).expect("两个余额应同时显示");
    let credits=rect("2.85万");let balance=rect("¥4.1");assert!(credits.right()+20.0<balance.left());
    assert_eq!(h.app.quota_bar_indices().iter().skip(5).map(|i|h.app.quota.config.accounts[*i].provider).collect::<Vec<_>>(),vec![Provider::WorkBuddy,Provider::DeepSeek]);
    h.click(credits.center());assert_eq!(h.app.quota.config.selected,wb);assert_eq!(h.app.page(),6);
}

#[test]
#[ignore = "按需导出 WorkBuddy 与 DeepSeek 并排界面"]
fn workbuddy_balance_gallery(){
    use crate::quota::{Provider,Event};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_DEEPSEEK_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for skin in [crate::model::Skin::Classic,crate::model::Skin::Cream,crate::model::Skin::GlassLight,crate::model::Skin::GlassDark]{
    for (name,amount,stale,count) in [("workbuddy","4.18",false,3)] {for open in [false,true]{
        let mut h=H::new();h.app.settings.pet=false;h.app.settings.skin=skin;h.app.settings.show_battery=true;h.quota_accounts(count);
        h.app.quota.config.accounts[1].provider=Provider::Claude;h.app.quota.config.accounts[1].alias="Claude".into();h.app.quota.config.accounts[0].alias="Codex".into();
        let id=h.app.quota.config.connect_deepseek("preview".into(),"DeepSeek".into());
        let snapshot=crate::quota::parse(Provider::DeepSeek,&serde_json::json!({"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":amount,"topped_up_balance":amount,"granted_balance":"0.00"}]})).unwrap();
        h.app.quota_event(Event{id:id.clone(),result:Ok(snapshot)});if stale{h.app.quota_event(Event{id:id.clone(),result:Err("无法连接".into())});}
        if name=="loading"{h.app.quota.rows.remove(&id);h.app.quota.busy.insert(id.clone());}
        if name=="error"{h.app.quota.rows.remove(&id);h.app.quota_event(Event{id,result:Err("暂时无法查询余额，请稍后重试".into())});}
        let wb=h.app.quota.config.connect_workbuddy("preview".into(),"WorkBuddy".into());
        h.app.quota_event(Event{id:wb,result:crate::quota::parse(Provider::WorkBuddy,&serde_json::json!({"code":0,"data":{"Packages":[{"CycleRemainCapacity":"2000"}],"CreditGroups":[{"Kind":"plan_base","CycleRemainCapacity":"500"},{"Kind":"benefit","CycleRemainCapacity":"1500"}]}}))});
        if open{h.app.show_page(6);}
        let mut raster=crate::preview_raster::Raster::new(1280,380);
        for frame in 0..90{
            let t=frame as f64/60.0;let env=Env{t,clock:"09:41:00".into(),battery:Some(80),charging:true,cursor:if open{Some(Pos2::new(320.0,60.0))}else{None},..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(640.0,190.0))),time:Some(t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
            let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{h.app.frame(ui,ui.max_rect(),&env);});});
            raster.update_textures(&output.textures_delta);
            if frame==89{
                raster.buf.fill([0.09,0.11,0.18,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                image::save_buffer(dest.join(format!("{}-{name}-{}.png",skin.key(),if open{"open"}else{"closed"})),&rgba,1280,380,image::ColorType::Rgba8).unwrap();
            }
            raster.free_textures(&output.textures_delta);
        }
    }}}
}
#[test]
fn new_mail_notification_masks_codes_in_sender_and_history(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.mail.accounts=vec![crate::mail::Account{email:"a@example.com".into(),provider:crate::mail::Provider::Qq,custom:None,enabled:true}];
    let item=crate::mail::Item{received_unix:None,account:"a@example.com".into(),from:"登录验证码 123456".into(),subject:"验证码 123456".into(),time:"09:41".into(),code:Some("123456".into()),html: String::new().into(), summary:"验证码 123456".into(),links:vec![],meeting:None,uid:42};
    h.app.mail_event(crate::mail::Event::Mail{item,new:true});
    assert!(!h.app.history.is_empty());
    for alert in h.app.alerts.iter().chain(h.app.history.iter()) {
        assert!(!alert.body.contains("123456"),"通知和历史记录不能留下验证码明文");
    }
}

#[test]
fn notification_preview_masks_fullwidth_and_zero_width_codes(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.read_notifications=true;h.app.settings.notification_preview=true;
    for body in ["验证码 １２３４５６，请勿泄露","验证码 12\u{200b}3456，请勿泄露"] {
        h.app.receive_notification("测试".into(),"external.test".into(),body.into());
        assert_eq!(h.app.alerts.back().unwrap().body,"验证码 ••••••，请勿泄露");
    }
}

/// 内存实测：模拟长时间听歌（不断出现新汉字）并浏览各页面，输出进程内存、字形图集和贴图占用。
/// 运行：DAOZAI_ATLAS=4096 DAOZAI_PPP=1.5 cargo test --lib memory_profile -- --ignored --nocapture --test-threads=1
#[test]
#[ignore="内存实测，按需运行"]
fn memory_profile_long_listening_session(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let side:usize=std::env::var("DAOZAI_ATLAS").ok().and_then(|s|s.parse().ok()).unwrap_or(4096);
    let ppp:f32=std::env::var("DAOZAI_PPP").ok().and_then(|s|s.parse().ok()).unwrap_or(1.5);
    let songs:usize=std::env::var("DAOZAI_SONGS").ok().and_then(|s|s.parse().ok()).unwrap_or(120);
    // 匿名内存≈Windows 的私有内存；文件映射（程序代码、系统字体）可在进程间共享，不计入私有内存。
    fn rss()->String{
        let status=std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        let kb=|key:&str|status.lines().find_map(|l|l.strip_prefix(key)?.trim().trim_end_matches(" kB").trim().parse::<f64>().ok()).unwrap_or(0.0)/1024.0;
        format!("匿名={:>6.1}MB 文件映射={:>6.1}MB",kb("RssAnon:"),kb("RssFile:"))
    }
    fn report(label:&str,h:&H){
        let [w,hgt]=h.ctx.fonts(|f|f.font_image_size());
        let tex:usize=h.ctx.tex_manager().read().allocated().map(|(_,m)|m.bytes_used()).sum();
        let galleys=h.ctx.fonts(|f|f.num_galleys_in_cache());
        // 图集在 CPU 端按 f32 存覆盖率，显卡端再按 RGBA 存一份。
        println!("{label:<16} {} 图集={w}x{hgt}（CPU≈{:.1}MB）贴图合计={:.1}MB 文字缓存={galleys}",rss(),(w*hgt*4) as f64/1048576.0,tex as f64/1048576.0);
    }
    println!("{:<16} {}","测试进程基线",rss());
    let mut h=H::new();h.app.settings.island_lyrics=true;h.app.show_lyrics=true;
    println!("{:<16} {}","创建界面后",rss());
    let mut seed=12345u64;let mut next=||{seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);(seed>>33) as u32};
    let run=|h:&mut H,media:&crate::smtc::NowPlaying|{
        h.t+=1.0/60.0;
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(h.t),max_texture_side:Some(side),
            viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
        let env=Env{t:h.t,clock:"09:41:00".into(),today:"2026-10-28".into(),date:"10月28日 周三".into(),media:media.clone(),cursor:h.cursor,bt_ready:true,..Default::default()};
        let app=&mut h.app;
        let out=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let _=app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,WIN_H)),&env);});});
        h.shapes=out.shapes;
    };
    let idle=crate::smtc::NowPlaying::default();
    for _ in 0..30 {run(&mut h,&idle);}
    report("启动后",&h);
    for page in 0..8 {h.app.show_page(page);for _ in 0..20 {run(&mut h,&idle);}}
    report("浏览8个页面后",&h);
    for song in 0..songs {
        // 常用字约 3000 个：每首歌 40 句、每句 8–12 字，从中随机取，接近真实歌词的用字分布。
        let lines:Vec<String>=(0..40).map(|_|(0..8+next()%5).map(|_|char::from_u32(0x4E00+next()%3000).unwrap()).collect()).collect();
        let lrc=lines.iter().enumerate().map(|(i,l)|format!("[{:02}:{:02}.00]{l}\n",i*3/60,i*3%60)).collect::<String>();
        let title=format!("测试歌曲{song}");
        h.app.set_lyrics(&title,"歌手",200.0,crate::lyrics::parse_lrc(&lrc));
        if song==songs/2 {h.app.show_page(0);}
        for i in 0..40 {
            let media=crate::smtc::NowPlaying{title:title.clone(),artist:"歌手".into(),duration:200.0,position:i as f64*3.0+0.5,playing:false,..Default::default()};
            for _ in 0..2 {run(&mut h,&media);}
        }
        if (song+1)%20==0 {report(&format!("听完{}首",song+1),&h);}
    }
}

#[test]
#[ignore = "按需导出设置页预览：DAOZAI_SETTINGS_PREVIEW_DIR=目录 cargo test settings_gallery -- --ignored"]
fn settings_gallery(){
    use crate::settings_theme::SettingsTheme;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_SETTINGS_PREVIEW_DIR").expect("需指定预览目录"));
    std::fs::create_dir_all(&dest).unwrap();
    let width:f32=std::env::var("DAOZAI_SETTINGS_WIDTH").ok().and_then(|v|v.parse().ok()).unwrap_or(690.0);
    let (w,hgt)=(width,2400.0f32);
    let scale=1.5f32;
    for (mode,dark) in [(SettingsTheme::Dark,true),(SettingsTheme::Light,false)] {
        for tab in [0usize,10,1,2,4,6,7,11,9] {
            let mut h=H::new();h.app.open_settings();h.ctx.set_embed_viewports(false);
            h.quota_accounts(2);
            h.app.settings.settings_theme=mode;h.app.settings_tab=tab;
            let output=std::sync::Arc::new(std::sync::Mutex::new(None));let capture=output.clone();
            egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
                let id=viewport.ids.this;
                let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(w,hgt))),
                    viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(scale),..Default::default()})).collect(),..Default::default()};
                *capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
            });
            let mut raster=crate::preview_raster::Raster::new((w*scale) as usize,(hgt*scale) as usize);
            for frame in 0..4 {
                let root=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));
                raster.update_textures(&root.textures_delta);
                let child=output.lock().unwrap().take().unwrap();
                raster.update_textures(&child.textures_delta);
                if frame==3 {
                    raster.buf.fill(if dark{[0.08,0.08,0.09,1.0]}else{[0.96,0.96,0.97,1.0]});
                    raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);
                    let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                    let image=image::RgbaImage::from_raw((w*scale) as u32,(hgt*scale) as u32,rgba).unwrap();
                    // 去掉页面下方的空白
                    let bg=image.get_pixel(((w-20.0)*scale) as u32,((hgt-5.0)*scale) as u32).0;
                    let mut bottom=image.height()-1;
                    'scan: while bottom>200 { for x in (270*scale as u32)..image.width()-10 { if image.get_pixel(x,bottom).0!=bg {break 'scan;} } bottom-=1; }
                    image::imageops::crop_imm(&image,0,0,image.width(),(bottom+20).min(image.height())).to_image()
                        .save(dest.join(format!("settings-{}-{tab:02}.png",if dark{"dark"}else{"light"}))).unwrap();
                }
                raster.free_textures(&child.textures_delta);
            }
        }
    }
}

#[test]
#[ignore="按需导出消息提醒与账号状态提示的实际样式"]
fn messenger_notice_status_gallery(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_MESSENGER_PREVIEW").expect("需指定预览目录"));
    std::fs::create_dir_all(&dest).unwrap();
    for case in 0..15{
        let mut h=H::new();h.app.settings.pet=true;h.app.settings.wechat_notifications=true;
        h.app.settings.read_notifications=true;h.app.settings.notification_preview=true;
        h.app.settings.bluetooth_notifications=true;
        let ppp=2.0;let (w,height)=(crate::app::CANVAS_W,WIN_H);
        let mut raster=crate::preview_raster::Raster::new((w*ppp) as usize,(height*ppp) as usize);
        for frame in 0..90{
            h.t=frame as f64/60.0;
            if frame==40{match case{
                0=>h.app.receive_wechat_reminder_from(100,0),
                1=>h.app.receive_notification("QQ".into(),"QQ".into(),"有新消息 · 请到 QQ 查看".into()),
                2=>{for pid in [100,200,300]{h.app.receive_wechat_reminder_from(pid,0);}h.app.alerts.clear();h.app.notice=None;h.app.notification_bell_until=0.0;},
                3=>h.app.messenger_unavailable("QQ 账号已退出"),
                4=>h.app.messenger_unavailable("微信 账号已退出"),
                5=>h.app.messenger_unavailable("暂时无法确认此 QQ 消息的账号"),
                6=>h.app.messenger_unavailable("QQ 窗口暂时无法打开"),
                7=>{h.app.show_page(4);h.app.messenger_unavailable("QQ 账号已退出");},
                8=>h.app.alert("大写锁定","已开启",5,false),
                9=>h.app.alert("系统音量","音量 60%",5,false),
                10=>h.app.bluetooth_alert(&crate::bt::Device{id:"preview".into(),name:"耳机".into(),battery:Some(80),kind:crate::bt::DeviceKind::Headphones},true),
                11=>h.app.alert("电量偏低","剩余 15% · 请连接电源",5,false),
                12=>{
                    let start=chrono::Local::now()+chrono::Duration::minutes(10);
                    h.app.meeting_alert(crate::mail::Meeting{all_day:false,title:"团队会议".into(),start:start.timestamp(),end:None,at:start.format("%m-%d %H:%M").to_string(),uid:"preview".into()},"本机日程".into(),7);
                },
                13=>h.app.alert("喝水时间到了","休息一下，喝 250 ml 水吧",2,false),
                14=>h.app.alert("待办到时间了","整理本周资料",4,false),
                _=>unreachable!(),
            }}
            let cursor=(case==7).then(||H::local(160.0,12.0));
            let env=Env{t:h.t,clock:"09:41:00".into(),today:"2026-11-08".into(),date:"11月8日 周日".into(),cursor,volume:(case==9).then_some((60,false)),media:h.media.clone(),..Default::default()};
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,height))),time:Some(h.t),focused:true,
                events:cursor.map(|p|vec![egui::Event::PointerMoved(p)]).unwrap_or_default(),
                viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
            let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{
                h.app.frame(ui,Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H)),&env);
            });});
            raster.update_textures(&output.textures_delta);
            if frame==89{
                raster.buf.fill([0.93,0.95,0.97,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                let image=image::RgbaImage::from_raw((w*ppp)as u32,(height*ppp)as u32,rgba).unwrap();
                image::imageops::crop_imm(&image,100,0,1560,if case==7||case==12{440}else{260}).to_image().save(dest.join(format!("{case:02}.png"))).unwrap();
            }
            raster.free_textures(&output.textures_delta);
        }
    }
}

#[test]
fn qq_unknown_reminders_survive_viewing_other_accounts(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.read_notifications=true;
    h.app.receive_notification("QQ".into(),"QQ".into(),"未确认账号的消息".into());
    h.app.viewed_client(crate::app::Messenger::Qq,200);
    assert_eq!(h.app.pending_messengers,[crate::app::Unread::of(crate::app::Messenger::Qq)]);
}

#[test]
fn messenger_window_replacement_removes_stale_banner_targets_without_clearing_other_accounts(){
    use crate::app::{Cmd,Messenger,Unread};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for source in Messenger::ALL{
        let mut h=H::new();h.app.settings.read_notifications=true;h.app.settings.wechat_notifications=true;h.app.settings.wecom_notifications=true;h.app.settings.feishu_notifications=true;h.app.settings.dingtalk_notifications=true;
        let old=Unread{source,pid:100,window:11};let other=Unread{source,pid:200,window:22};let current=Unread{window:33,..old};
        let receive=|app:&mut App,unread:Unread|match source{
            Messenger::Wechat|Messenger::WeCom|Messenger::Feishu|Messenger::DingTalk=>app.receive_messenger_reminder(unread),
            Messenger::Qq=>app.receive_notification_from("QQ".into(),"QQ".into(),"有新消息".into(),Some(unread)),
        };
        receive(&mut h.app,old);h.frame(vec![]);receive(&mut h.app,other);receive(&mut h.app,current);
        assert_eq!(h.app.pending_messengers,[current,other]);
        assert!(h.app.alerts.iter().chain(h.app.notice.iter()).all(|alert|alert.unread!=Some(old)),"窗口被新消息替换后，横幅不能继续引用已经失效的目标");
        assert!(h.app.alerts.iter().chain(h.app.notice.iter()).any(|alert|alert.unread==Some(other)),"其他账号的横幅必须保留");
        h.app.discard_unread(other);for _ in 0..45{h.frame(vec![]);}
        h.click_text("查看");assert!(h.cmds.contains(&Cmd::ViewMessenger(current)));
    }
}

#[test]
fn qq_bound_notices_open_their_original_account_and_clear_independently(){
    use crate::app::{Cmd,Messenger,Unread};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.read_notifications=true;
    let a=Unread{source:Messenger::Qq,pid:100,window:11};let b=Unread{source:Messenger::Qq,pid:200,window:22};
    h.app.receive_notification_from("QQ".into(),"QQ".into(),"有新消息".into(),Some(a));h.frame(vec![]);
    h.app.receive_notification_from("QQ".into(),"QQ".into(),"有新消息".into(),Some(b));
    assert_eq!(h.app.pending_messengers,[a,b]);
    for _ in 0..45{h.frame(vec![]);}
    h.click_text("查看");assert!(h.cmds.contains(&Cmd::ViewMessenger(a)),"横幅应打开它自己的账号，不能取最近新增的另一个账号");
    h.app.viewed_client(Messenger::Qq,200);h.frame(vec![]);
    assert_eq!(h.app.pending_messengers,[a]);
    h.cmds.clear();h.click_text("查看");assert!(h.cmds.contains(&Cmd::ViewMessenger(a)));
    h.app.discard_unread(a);h.frame(vec![]);assert!(h.app.pending_messengers.is_empty());assert!(h.app.notice.is_none());
}

#[test]
fn qq_unknown_notice_can_close_without_dismissing_a_bound_account(){
    use crate::app::{Messenger,Unread};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.read_notifications=true;
    h.app.receive_notification("QQ".into(),"QQ".into(),"未确认账号".into());h.frame(vec![]);
    let b=Unread{source:Messenger::Qq,pid:200,window:22};
    h.app.receive_notification_from("QQ".into(),"QQ".into(),"另一账号的消息".into(),Some(b));
    for _ in 0..45{h.frame(vec![]);}
    h.click_text("关闭");h.frame(vec![]);
    assert_eq!(h.app.pending_messengers,[b]);
}

#[test]
fn qq_expired_unidentified_notice_can_be_reopened_and_explicitly_cleared(){
    use crate::app::{Messenger,Unread};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.read_notifications=true;
    let unread=Unread::of(Messenger::Qq);
    h.app.receive_notification("QQ".into(),"QQ".into(),"测试消息".into());
    for _ in 0..360{h.frame(vec![]);}assert!(h.app.notice.is_none());
    h.app.reveal_messenger_notice(unread);for _ in 0..45{h.frame(vec![]);}
    h.click_text("关闭");h.frame(vec![]);assert!(h.app.pending_messengers.is_empty());
}

#[test]
fn qq_unknown_reminders_are_not_numbered_as_another_known_account(){
    use crate::app::{Messenger,Unread};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.read_notifications=true;
    for (pid,window) in [(100,11),(0,0),(200,22)]{
        h.app.receive_notification_from("QQ".into(),"QQ".into(),"有新消息".into(),(pid!=0).then_some(Unread{source:Messenger::Qq,pid,window}));
    }
    assert_eq!((h.app.unread_tag(0),h.app.unread_tag(1),h.app.unread_tag(2)),(Some(1),None,Some(2)));
    h.app.alerts.clear();h.app.notice=None;h.hover(Pos2::new(WIN_W/2.0,12.0),1.0);
    assert!(h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text)if text.galley.text()=="?")));
}

#[test]
fn unavailable_messenger_status_stays_visible_when_the_island_is_collapsed(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for message in ["QQ 账号已退出","微信 账号已退出","暂时无法确认此 QQ 消息的账号"]{
        let mut h=H::new();h.hover(Pos2::new(600.0,180.0),1.0);
        h.app.messenger_unavailable(message);h.hover(Pos2::new(600.0,180.0),0.3);
        let bounds=h.shapes.iter().find_map(|shape|match &shape.shape{
            egui::Shape::Text(text) if text.galley.text()==message=>Some(text.visual_bounding_rect()),_=>None,
        }).expect("岛收起时仍应完整显示账号状态提示");
        assert!(h.app.progress()<0.02,"提示不应展开岛体");
        assert!(h.region.contains_rect(bounds),"原生窗口区域应包含提示");
        assert!(!h.hit.intersects(bounds),"提示不应拦截鼠标");
    }
}

#[test]
fn exited_messenger_unreads_clear_their_badges_and_notifications(){
    use crate::app::{Messenger,Unread};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.wechat_notifications=true;h.app.settings.read_notifications=true;
    h.app.receive_wechat_reminder_from(100,11);h.frame(vec![]);
    assert_eq!(h.app.notice.as_ref().and_then(|notice|notice.messenger()),Some(Messenger::Wechat));
    h.app.receive_wechat_reminder_from(200,22);h.app.receive_wechat_reminder_from(300,33);
    h.app.receive_notification("QQ".into(),"QQ".into(),"测试".into());
    for (pid,window) in [(200,22),(100,11)]{
        h.app.discard_unread(Unread{source:Messenger::Wechat,pid,window});h.frame(vec![]);
        assert!(h.app.pending_messengers.contains(&Unread{source:Messenger::Wechat,pid:300,window:33}));
        assert_eq!(h.app.notice.as_ref().and_then(|notice|notice.messenger()),Some(Messenger::Wechat));
    }
    h.app.discard_unread(Unread{source:Messenger::Wechat,pid:300,window:33});h.frame(vec![]);
    assert_eq!(h.app.pending_messengers,[Unread::of(Messenger::Qq)]);
    assert!(h.app.alerts.iter().all(|notice|notice.messenger()!=Some(Messenger::Wechat)));
    assert_ne!(h.app.notice.as_ref().and_then(|notice|notice.messenger()),Some(Messenger::Wechat));
    h.app.discard_unread(Unread::of(Messenger::Qq));h.frame(vec![]);
    assert!(h.app.pending_messengers.is_empty());
    assert!(h.app.alerts.iter().all(|notice|notice.messenger()!=Some(Messenger::Qq)));
    assert_ne!(h.app.notice.as_ref().and_then(|notice|notice.messenger()),Some(Messenger::Qq));
}

#[test]
fn stale_exit_results_preserve_new_windows_and_unidentified_accounts(){
    use crate::app::{Messenger,Unread};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.wechat_notifications=true;
    let old=Unread{source:Messenger::Wechat,pid:100,window:11};
    h.app.receive_wechat_reminder_from(old.pid,old.window);h.app.receive_wechat_reminder_from(100,12);
    h.app.receive_wechat_reminder();h.app.discard_unread(old);
    assert_eq!(h.app.pending_messengers,[Unread{window:12,..old},Unread::of(Messenger::Wechat)]);
    h.app.discard_unread(Unread{window:12,..old});h.frame(vec![]);
    assert_eq!(h.app.pending_messengers,[Unread::of(Messenger::Wechat)]);
    h.app.discard_unread(Unread::of(Messenger::Wechat));h.frame(vec![]);
    assert!(h.app.pending_messengers.is_empty());
}

#[test]
fn messenger_notifications_keep_separate_badges_until_the_client_is_viewed(){
    use crate::app::{Cmd,Messenger,Unread};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.wechat_notifications=true;h.app.settings.read_notifications=true;
    h.frame(vec![]);h.app.receive_wechat_reminder();
    for _ in 0..60{h.frame(vec![]);}
    h.click_text("查看");assert!(h.cmds.contains(&Cmd::ViewMessenger(Unread::of(Messenger::Wechat))));
    h.click_text("关闭");assert!(h.app.notice.is_none());
    h.cursor=None;h.app.receive_notification("QQ".into(),"QQ".into(),"小林 · 测试".into());
    for _ in 0..360{h.frame(vec![]);}
    assert!(h.app.notice.is_none());
    let sources=|h:&H|h.app.pending_messengers.iter().map(|u|u.source).collect::<Vec<_>>();
    assert_eq!(sources(&h),[Messenger::Wechat,Messenger::Qq]);
    // 收起时两条未读叠成一个入口，点它展开面板，再在旁边的胶囊里逐个点开。
    h.frame(vec![]);
    let stack=h.ctx.read_response(egui::Id::new(("card",true,"messenger-stack"))).expect("叠放入口可点击").rect.center();
    h.click(stack);for _ in 0..60{h.frame(vec![]);}
    assert!(h.app.open);
    for source in [Messenger::Wechat,Messenger::Qq]{
        h.frame(vec![]);
        let pos=h.ctx.read_response(egui::Id::new(("card",true,source.name()))).expect("胶囊图标可点击").rect.center();
        h.cmds.clear(); h.click(pos);assert!(h.cmds.contains(&Cmd::ViewMessenger(Unread::of(source))),"{source:?} {pos:?} {:?}",h.cmds);
        h.app.viewed_messenger(source);h.frame(vec![]);
    }
    assert!(h.app.pending_messengers.is_empty());
    h.app.receive_wechat_reminder();h.app.receive_wechat_reminder();
    assert_eq!(sources(&h),[Messenger::Wechat]);
    h.app.settings.wechat_notifications=false;h.frame(vec![]);assert!(h.app.pending_messengers.is_empty());
}

#[test]
fn messenger_capsule_grows_during_opening(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.wechat_notifications=true;
    h.app.receive_wechat_reminder();
    h.app.alerts.clear();
    h.app.notice=None;
    for opening in [true,false,true,false] {
        h.cursor=opening.then_some(Pos2::new(WIN_W/2.0,12.0));
        let mut sampled=false;
        for _ in 0..90 {
            h.frame(vec![]);
            let appear=h.app.messenger_appear();
            if (0.02..0.95).contains(&appear) {
                let button=h.ctx.read_response(egui::Id::new(("card",true,"微信".to_string()))).unwrap();
                sampled|=(1.0..crate::app::UNREAD_W-1.0).contains(&button.rect.width());
            }
        }
        assert!(sampled && h.app.open==opening,"每次展开与收回都应经过中间尺寸");
    }
}

#[test]
fn messenger_accounts_get_number_tags_and_clear_one_by_one(){
    use crate::app::{Cmd,Messenger,Unread};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.wechat_notifications=true;h.app.settings.read_notifications=true;
    h.app.receive_wechat_reminder_from(100,11);h.app.receive_wechat_reminder_from(200,22);h.app.receive_wechat_reminder_from(100,12);
    h.app.receive_notification("QQ".into(),"QQ".into(),"测试".into());
    assert_eq!(h.app.pending_messengers,[Unread{source:Messenger::Wechat,pid:100,window:12},Unread{source:Messenger::Wechat,pid:200,window:22},Unread::of(Messenger::Qq)]);
    assert_eq!((h.app.unread_tag(0),h.app.unread_tag(1),h.app.unread_tag(2)),(Some(1),Some(2),None),"只有同一来源多个账号才编号");
    h.app.alerts.clear();h.app.notice=None;
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.5);
    let rect=|h:&H,name:&str|h.ctx.read_response(egui::Id::new(("card",true,name.to_string()))).unwrap_or_else(||panic!("{name}")).rect;
    let panel=h.app.island_rect(Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H)),crate::app::PW,h.app.expanded_height());
    let (a,b,q)=(rect(&h,"微信"),rect(&h,"微信2"),rect(&h,"QQ"));
    assert!(a.bottom()<=b.top()+0.5 && b.bottom()<=q.top()+0.5);
    let inset=h.app.unread_top_inset();assert!(inset>0.0,"顶部停靠时胶囊离开屏幕上沿");
    assert!((a.top()-panel.top()-inset-2.0).abs()<1.0 && (q.bottom()-panel.bottom()+2.0).abs()<1.0,"三个图标时胶囊下沿与面板对齐 {a:?} {q:?} {panel:?}");
    h.cmds.clear();h.click(b.center());
    assert!(h.cmds.contains(&Cmd::ViewMessenger(Unread{source:Messenger::Wechat,pid:200,window:22})));
    h.app.viewed_client(Messenger::Wechat,200);
    assert_eq!(h.app.pending_messengers,[Unread{source:Messenger::Wechat,pid:100,window:12},Unread::of(Messenger::Qq)]);
    assert_eq!(h.app.unread_tag(0),None,"剩一个账号就不再编号");
    h.app.viewed_client(Messenger::Qq,4321);
    assert!(h.app.pending_messengers.contains(&Unread::of(Messenger::Qq)),"查看其他 QQ 不能清除无法归属的提醒");
    h.app.discard_unread(Unread::of(Messenger::Qq));
    assert_eq!(h.app.pending_messengers,[Unread{source:Messenger::Wechat,pid:100,window:12}]);
    // 四个以上：胶囊仍与面板等高，滚轮翻到后面的图标。
    for pid in [300,400,500]{h.app.receive_wechat_reminder_from(pid,0);}
    h.hover(Pos2::new(WIN_W/2.0,12.0),0.2);
    let capsule=rect(&h,"微信");
    let last=|h:&H|rect(h,"微信4");
    let before=last(&h);
    assert!(before.height()<1.0 || before.top()>=panel.bottom()-3.0,"第四个先藏在下面");
    h.hover(Pos2::new(capsule.center().x,panel.top()+30.0),0.1);
    for _ in 0..10{h.wheel(-40.0);}
    h.frame(vec![]);
    assert!(last(&h).height()>20.0 && last(&h).bottom()<=panel.bottom(),"滚动后露出第四个 {:?}",last(&h));
}

#[test]
fn messenger_notifications_stay_right_and_move_outside_expanded_panel(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.wechat_notifications=true;
    h.frame(vec![]);h.app.receive_wechat_reminder();
    for _ in 0..360{h.frame(vec![]);}
    let badge=|h:&H|h.ctx.read_response(egui::Id::new(("card",true,"微信"))).unwrap().rect;
    assert!(badge(&h).left()>WIN_W/2.0);
    h.hover(Pos2::new(WIN_W/2.0,12.0),1.5);
    assert!(h.app.open && badge(&h).left()>WIN_W/2.0);
    assert!((badge(&h).left()-(WIN_W/2.0+crate::app::PW/2.0+crate::app::UNREAD_GAP)).abs()<1.0,"展开后与面板留一小段间距 {:?}",badge(&h));
    let center=badge(&h).center();
    assert!(h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Rect(r)
        if r.rect.contains(center) && (r.rect.width()-crate::app::UNREAD_W).abs()<0.1 && (r.rect.height()-crate::app::UNREAD_W).abs()<0.1)),"一个图标时是正圆");
    h.click(badge(&h).center());
    assert!(h.cmds.contains(&crate::app::Cmd::ViewMessenger(crate::app::Unread::of(crate::app::Messenger::Wechat))));
}

#[test]
fn messenger_notifications_align_with_system_banner(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.wechat_notifications=true;
    h.app.settings.read_notifications=true;
    h.frame(vec![]);h.app.receive_wechat_reminder();
    for _ in 0..360{h.frame(vec![]);}
    h.app.receive_notification("ChatGPT".into(),"ChatGPT".into(),"测试".into());
    for _ in 0..60{h.frame(vec![]);}
    let rect=|name|h.ctx.read_response(egui::Id::new(("card",true,name))).unwrap().rect;
    let badge=rect("微信");let close=rect("external-dismiss");
    assert!(badge.left()>close.right());
    assert!((badge.center().y-close.center().y).abs()<1.1);
}

#[test]
fn messenger_notifications_side_docks_keep_one_column(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dock in [crate::model::Dock::Left,crate::model::Dock::Right]{
        let mut h=H::new();h.app.settings.dock=dock;h.app.settings.wechat_notifications=true;h.app.settings.read_notifications=true;
        h.app.receive_wechat_reminder();h.app.receive_notification("QQ".into(),"QQ".into(),"测试".into());
        for _ in 0..720{h.frame(vec![]);}
        let win=Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H));
        let stack=h.ctx.read_response(egui::Id::new(("card",true,"messenger-stack"))).unwrap().rect;
        let bar=h.app.island_rect(win,h.app.w.pos,h.app.h.pos);
        assert!((stack.center().x-bar.center().x).abs()<1.0 && stack.top()>=bar.bottom()-0.5,"收起时叠放在竖胶囊下方");
        h.click(stack.center());for _ in 0..90{h.frame(vec![]);}
        let rect=|name|h.ctx.read_response(egui::Id::new(("card",true,name))).unwrap().rect;
        let a=rect("微信");let b=rect("QQ");
        assert_eq!(a.center().x,b.center().x);assert!(a.bottom()<=b.top()+0.01);
        let panel=h.app.island_rect(win,crate::app::PW,h.app.expanded_height());
        if dock==crate::model::Dock::Left {assert!(a.left()>=panel.right()+crate::app::UNREAD_GAP-0.5,"左停靠：胶囊在面板右侧");}
        else {assert!(a.right()<=panel.left()-crate::app::UNREAD_GAP+0.5,"右停靠：胶囊在面板左侧");}
        assert!((a.top()-panel.top()-2.0).abs()<1.0 && h.app.unread_top_inset()==0.0,"侧边停靠时顶端与面板对齐");
    }
}

#[test]
fn side_dock_file_drag_opens_the_shelf_and_drop_saves_it(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let (w,hgt)=(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H);
    let path=crate::model::config_dir().join("侧边拖入.txt");std::fs::create_dir_all(path.parent().unwrap()).unwrap();std::fs::write(&path,"x").unwrap();
    for dock in [crate::model::Dock::Left,crate::model::Dock::Right] {
        let mut h=H::new();h.app.settings.dock=dock;
        let edge=if dock==crate::model::Dock::Left{10.0}else{w-10.0};
        let at=Pos2::new(edge,hgt/2.0);
        let mut frame=|h:&mut H,hovered:bool,dropped:bool|{
            h.t+=1.0/60.0;
            let env=Env{t:h.t,clock:"09:41:00".into(),cursor:Some(at),..Default::default()};
            let mut raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt))),time:Some(h.t),focused:true,..Default::default()};
            if hovered {raw.hovered_files.push(egui::HoveredFile{path:Some(path.clone()),..Default::default()});}
            if dropped {raw.dropped_files.push(egui::DroppedFile{path:Some(path.clone()),..Default::default()});}
            let app=&mut h.app;let mut out=None;
            let _=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{out=Some(app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(w,hgt)),&env));});});
            out.unwrap()
        };
        let out=frame(&mut h,false,false);
        let zone=out.file_drop_zone.expect("置物架开启时侧边也有拖放感应区");
        assert!(zone.contains(at),"{dock:?} 感应区应覆盖贴边的胶囊：{zone:?}");
        frame(&mut h,true,false);
        assert!(h.app.drag_hot && h.app.page==3,"{dock:?} 文件拖到胶囊上应切到置物架");
        for _ in 0..40 {frame(&mut h,true,false);}
        assert!(h.app.open);
        frame(&mut h,false,true);
        assert!(h.app.shelf.items.contains(&path),"{dock:?} 松手后应放进置物架");
        h.app.shelf.items.clear();h.app.shelf.save();
    }
}

fn shoot_unread(h:&mut H,raster:&mut crate::preview_raster::Raster,dest:&std::path::Path,name:&str,frames:usize,cursor:Option<Pos2>){
    let ppp=2.0;
    let side=h.app.settings.dock.is_side();
    let (w,hgt)=if side {(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H)} else {(crate::app::CANVAS_W,WIN_H)};
    let (ww,wh)=if side {(w,hgt)} else {(WIN_W,WIN_H)};
    for f in 0..frames {
        h.t+=1.0/60.0;
        let env=Env{t:h.t,clock:"09:41:00".into(),today:"2026-11-05".into(),date:"11月5日 周四".into(),cursor,bt_ready:true,battery:Some(76),..Default::default()};
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(w,hgt))),time:Some(h.t),focused:true,
            events:cursor.map(|p|vec![egui::Event::PointerMoved(p)]).unwrap_or_default(),
            viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
        let app=&mut h.app;
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{let win=Rect::from_min_size(ui.max_rect().min,Vec2::new(ww,wh));app.frame(ui,win,&env);});});
        raster.update_textures(&output.textures_delta);
        if f+1==frames {
            raster.buf.fill([0.20,0.24,0.32,1.0]);
            raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
            let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            let image=image::RgbaImage::from_raw((w*ppp) as u32,(hgt*ppp) as u32,rgba).unwrap();
            let image=if side {image::imageops::crop_imm(&image,0,240,(w*ppp) as u32,720).to_image()} else {image::imageops::crop_imm(&image,0,0,(WIN_W*ppp) as u32,(WIN_H*ppp) as u32).to_image()};
            image.save(dest.join(format!("{name}.png"))).unwrap();
        }
        raster.free_textures(&output.textures_delta);
    }
}

#[test]
#[ignore="按需导出微信 / QQ 未读胶囊的实际画面"]
fn unread_gallery(){
    use crate::app::Messenger;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_FEATURE_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    let fill=|h:&mut H,n:usize|{
        h.app.settings.wechat_notifications=true;h.app.settings.read_notifications=true;h.app.settings.pet=false;
        let all=[(Messenger::Wechat,101),(Messenger::Qq,0),(Messenger::Wechat,202),(Messenger::Wechat,303),(Messenger::Qq,0)];
        for (source,pid) in all.into_iter().take(n){
            if source==Messenger::Wechat {h.app.receive_wechat_reminder_from(pid,0);} else {h.app.receive_notification("QQ".into(),"QQ".into(),"测试".into());}
        }
        h.app.alerts.clear();h.app.notice=None;h.app.notification_bell_until=0.0;
    };
    for dock in [crate::model::Dock::Top,crate::model::Dock::Left,crate::model::Dock::Right]{
        let key=dock.key();
        for n in [1,2,3,4]{
            let mut h=H::new();h.app.settings.dock=dock;fill(&mut h,n);
            let side=dock.is_side();
            let (w,hgt)=if side {(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H)} else {(crate::app::CANVAS_W,WIN_H)};
            let mut r=if side {feature_raster_sized(w,hgt)} else {feature_raster()};
            let away=Some(Pos2::new(if side {w/2.0} else {WIN_W/2.0},hgt-6.0));
            let near=if dock==crate::model::Dock::Left {Pos2::new(8.0,hgt/2.0)} else if dock==crate::model::Dock::Right {Pos2::new(w-8.0,hgt/2.0)} else {Pos2::new(WIN_W/2.0,12.0)};
            if n!=3 {shoot_unread(&mut h,&mut r,&dest,&format!("unread-{key}-{n}-collapsed"),60,away);}
            shoot_unread(&mut h,&mut r,&dest,&format!("unread-{key}-{n}-open"),80,Some(near));
        }
        // 微信多开：两个微信账号；两个微信账号 + QQ。
        for (tag,qq) in [("multi2",false),("multi3",true)]{
            let mut h=H::new();h.app.settings.dock=dock;fill(&mut h,0);
            h.app.receive_wechat_reminder_from(101,0);h.app.receive_wechat_reminder_from(202,0);
            if qq {h.app.receive_notification("QQ".into(),"QQ".into(),"测试".into());}
            h.app.alerts.clear();h.app.notice=None;h.app.notification_bell_until=0.0;
            let side=dock.is_side();
            let (w,hgt)=if side {(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H)} else {(crate::app::CANVAS_W,WIN_H)};
            let mut r=if side {feature_raster_sized(w,hgt)} else {feature_raster()};
            let away=Some(Pos2::new(if side {w/2.0} else {WIN_W/2.0},hgt-6.0));
            let near=if dock==crate::model::Dock::Left {Pos2::new(8.0,hgt/2.0)} else if dock==crate::model::Dock::Right {Pos2::new(w-8.0,hgt/2.0)} else {Pos2::new(WIN_W/2.0,12.0)};
            shoot_unread(&mut h,&mut r,&dest,&format!("unread-{key}-{tag}-collapsed"),60,away);
            shoot_unread(&mut h,&mut r,&dest,&format!("unread-{key}-{tag}-open"),80,Some(near));
        }
    }
}

#[test]
fn shelf_menu_last_action_fits_inside_window_and_works(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.shelf.items=vec![r"folder\menu-test.zip".into()];
    h.app.show_page(3);
    let point=H::local(209.5,101.0);
    h.hover(point,1.2);
    for pressed in [true,false]{
        h.frame(vec![egui::Event::PointerButton{pos:point,button:egui::PointerButton::Secondary,pressed,modifiers:Default::default()}]);
    }
    h.hover(point,1.5);
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()==r"folder\menu-test.zip")));
    let label=h.shapes.iter().find_map(|s|match &s.shape{
        egui::Shape::Text(t) if t.galley.text()=="从置物架移除"=>Some(Rect::from_min_size(t.pos,t.galley.size())),
        _=>None,
    }).expect("右键菜单应显示移除操作");
    assert!(h.region.contains_rect(h.app.shelf_menu_rect.unwrap() + h.ctx.style().visuals.popup_shadow.margin()));
    assert!(h.app.shelf_menu_rect.unwrap().contains_rect(label));
    h.hover(label.center(),1.0);
    assert!(h.app.open);
    h.click(label.center());
    assert!(h.app.shelf.items.is_empty());
}

#[test]
fn calendar_add_button_does_not_overlap_the_battery_ring(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for seconds in [false,true] {
    for (index,(day_number,date,period,twelve,battery,scale)) in [
        (6,"11月6日 周五",1,false,Some(76),2.0),
        (28,"11月28日 周六",1,true,Some(76),1.25),
        (28,"11月28日 周六",2,false,Some(76),1.5),
        (6,"11月6日 周五",0,false,Some(76),1.0),
        (28,"11月28日 周六",0,true,Some(76),2.0),
        (6,"11月6日 周五",1,true,None,1.25),
        (28,"11月28日 周六",2,true,None,1.5),
        (28,"11月28日 周六",0,false,None,1.0),
    ].into_iter().enumerate(){
    let mut h=H::new();
    h.app.settings.skin=crate::model::Skin::Cream;h.app.settings.island_shape=crate::model::IslandShape::Pill;
    h.app.settings.clock_period=period;h.app.settings.clock_12h=twelve;h.app.settings.pet=false;
    h.app.settings.clock_seconds=seconds;
    let day=chrono::NaiveDate::from_ymd_opt(2026,11,day_number).unwrap();
    h.app.calendar.view.day=day;h.app.calendar.view.month=crate::calendar::month_start(day);h.app.calendar.view.loaded=Some(h.app.calendar.view.month);
    h.app.show_page(7);
    let mut raster=feature_raster();let mut shapes=Vec::new();
    for _ in 0..90 {
        h.t+=1.0/60.0;
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(h.t),focused:true,
            viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(scale),..Default::default()})).collect(),..Default::default()};
        let env=Env{t:h.t,clock:"17:43:00".into(),date:date.into(),today:day.to_string(),cursor:Some(Pos2::new(WIN_W/2.0,12.0)),battery,charging:true,..Default::default()};
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(ui.max_rect().min,Vec2::new(WIN_W,WIN_H)),&env);});});
        raster.update_textures(&output.textures_delta);shapes=output.shapes;
        raster.free_textures(&output.textures_delta);
    }
    if let Some(path)=std::env::var_os("DAOZAI_CALENDAR_HEADER_PREVIEW").filter(|_|index==0&&!seconds) {
        raster.buf.fill([0.15,0.15,0.15,1.0]);raster.draw(&h.ctx.tessellate(shapes.clone(),2.0),2.0);
        let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
        let image=image::RgbaImage::from_raw((crate::app::CANVAS_W*2.0) as u32,(WIN_H*2.0) as u32,rgba).unwrap();
        image::imageops::crop_imm(&image,200,0,880,470).to_image().save(path).unwrap();
    }
    let text=shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Text(text) if text.galley.text()=="＋ 新增"=>Some(text.visual_bounding_rect()),_=>None}).unwrap();
    let button=shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Rect(rect) if rect.rect.contains(text.center()) && (rect.rect.width()-46.0).abs()<0.1 && (rect.rect.height()-18.0).abs()<0.1=>Some(rect.rect),_=>None}).unwrap();
    if battery.is_some(){
        let ring=shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Circle(circle) if (circle.radius-6.56).abs()<0.1 && (circle.center.y-button.center().y-0.6).abs()<0.1=>Some(Rect::from_center_size(circle.center,Vec2::splat(circle.radius*2.0+circle.stroke.width))),_=>None}).unwrap();
        assert!(!button.expand(3.0).intersects(ring),"日程新增按钮与电量圆环应留有间距：按钮 {button:?}，圆环 {ring:?}");
    }
    let clock=h.app.settings.clock_text("17:43:00");
    for text in shapes.iter().filter_map(|shape|match &shape.shape{egui::Shape::Text(text) if text.galley.text()==date || clock.split_whitespace().any(|part|part==text.galley.text())=>Some(text),_=>None}){
        let bounds=text.visual_bounding_rect();
        assert!(!button.expand(3.0).intersects(bounds),"新增按钮不能覆盖日期或时间：{}",text.galley.text());
        assert!(bounds.right()<h.app.content_origin(Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H))).x+355.0,"日期与时间应留出右侧播放和设置按钮的位置");
    }
    }
    }
}

#[test]
fn wheel_short_lists_turn_pages(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    for count in [0,1,5]{
        h.app.shelf.items=(0..count).map(|i|std::path::PathBuf::from(format!("scroll-test-{i}.txt"))).collect();
        for (delta,page) in [(-40.0,4),(40.0,2)]{
            h.app.show_page(3);h.hover(H::local(300.0,60.0),0.5);h.wheel(delta);
            assert_eq!(h.app.page,page,"置物架未溢出应翻页，条数 {count}");
        }
    }
    for count in [0,1,3]{
        h.app.todos.items=(0..count).map(|i|crate::model::TodoItem{text:format!("任务 {i}"),..Default::default()}).collect();
        for (delta,page) in [(-40.0,5),(40.0,3)]{
            h.app.show_page(4);h.hover(H::local(300.0,60.0),0.5);h.wheel(delta);
            assert_eq!(h.app.page,page,"待办未溢出应翻页，条数 {count}");
        }
    }
    for count in [0,1,crate::pages::CLIP_ROWS]{
        h.app.clipboard.lock().items.clear();
        for i in 0..count{h.app.clipboard.lock().push(&i.to_string(),"09:00");}
        for (delta,page) in [(-40.0,4),(40.0,2)]{
            h.app.show_page(3);h.app.shelf_tab=1;h.hover(H::local(300.0,60.0),0.5);h.wheel(delta);
            assert_eq!(h.app.page,page,"剪贴板未溢出应翻页，条数 {count}");
        }
    }
    let day=chrono::NaiveDate::from_ymd_opt(2026,10,28).unwrap();
    let start=day.and_hms_opt(10,0,0).unwrap().and_local_timezone(chrono::Local).earliest().unwrap().timestamp();
    h.app.calendar.view.day=day;h.app.calendar.view.month=crate::calendar::month_start(day);h.app.calendar.view.loaded=Some(h.app.calendar.view.month);
    for (count,ended,all_day) in [(0,false,false),(1,false,false),(5,false,false),(8,true,false),(4,false,true)]{
        h.app.calendar.view.events=(0..count).map(|i|crate::mail::Meeting{all_day,title:format!("日程 {i}"),start:if ended{start-7200}else{start+i*60},end:Some(if ended{start-3600}else{start+3600}),at:String::new(),uid:format!("wheel-{i}")}).collect();
        for (delta,page) in [(-40.0,8),(40.0,6)]{
            h.app.show_page(7);h.hover(H::local(300.0,160.0),0.5);h.wheel(delta);
            assert_eq!(h.app.page,page,"日历可见列表未溢出应翻页，条数 {count}");
        }
    }
    h.app.calendar.view.events=(0..6).map(|i|crate::mail::Meeting{all_day:false,title:format!("日程 {i}"),start:start+i*60,end:Some(start+3600),at:String::new(),uid:format!("wheel-long-{i}")}).collect();
    h.app.show_page(7);
    for (delta,first) in [(40.0,0),(-40.0,1),(-40.0,1),(40.0,0),(40.0,0)]{
        h.hover(H::local(300.0,160.0),0.5);h.wheel(delta);
        assert_eq!(h.app.page,7,"日历溢出后边界也应留在本页");assert_eq!(h.app.calendar.view.first,first);
    }
}

#[test]
fn mail_reading_wheel_over_headers_never_turns_pages(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.mail.accounts=vec![crate::mail::Account{custom:None,email:"work@qq.com".into(),provider:crate::mail::Provider::Qq,enabled:true}];
    h.app.mail.recent=vec![crate::mail::Item{html:String::new().into(),received_unix:None,account:"work@qq.com".into(),from:"产品团队".into(),subject:"阅读中的邮件".into(),time:"09:41".into(),code:None,summary:"正文内容。".repeat(200).into(),links:vec![],meeting:None,uid:42}];
    h.app.show_page(5);h.app.mail.open_uid=Some(("work@qq.com".into(),42));
    for (x,y) in [(210.0,18.0),(200.0,48.0),(200.0,73.0),(200.0,92.0),(270.0,214.0)]{
        for delta in [-120.0,120.0]{
            h.hover(H::local(x,y),0.5);h.wheel(delta);
            assert_eq!(h.app.page(),5,"阅读邮件时在 ({x},{y}) 滚轮不能翻页");
            assert_eq!(h.app.mail.open_uid,Some(("work@qq.com".into(),42)));
        }
    }
    h.click(H::local(350.0,48.0));assert!(h.app.mail.open_uid.is_none());
    h.hover(H::local(210.0,18.0),0.5);h.wheel(-120.0);
    assert_eq!(h.app.page(),6,"返回列表后顶栏滚轮应恢复翻页");
}

#[test]
fn reader_actions_remain_bound_to_the_opened_mailbox() {
    use crate::mail_manage::{Action,Folder,Request,Completed,Snapshot};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=managed_mail_fixture();
    let old=h.app.mail.recent[0].clone();
    h.app.mail_open_reader(&old);assert!(h.app.mail_reader_ready(&old,Folder::Inbox));
    let mut fresh=old.clone();fresh.subject="重建文件夹后的邮件".into();
    h.app.mail_event(crate::mail::Event::Status{account:old.account.clone(),status:crate::mail::Status::Ready});
    assert!(!h.app.mail_reader_ready(&old,Folder::Inbox));
    h.app.mail_managed(Completed{request:Request{account:h.app.mail.accounts[0].clone(),folder:Folder::Inbox,validity:None,known:vec![],extra:vec![],action:Action::Load},result:Ok(Snapshot{validity:8,entries:vec![(old.uid,false)],items:vec![fresh]})});
    assert!(h.app.mail.reading.is_none());assert!(!h.app.mail_reader_ready(&old,Folder::Inbox));
}

#[test]
fn local_reader_moves_and_restores_after_switching_the_main_list() {
    use crate::mail_manage::{Action,Folder};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=managed_mail_fixture();h.app.settings.mail_sync_operations=false;
    let item=h.app.mail.recent[0].clone();
    for folder in [Folder::Inbox,Folder::Trash]{
        h.app.mail_open_reader(&item);h.app.mail_toggle_trash();
        assert!(h.app.mail_reader_ready(&item,folder));
        h.app.mail_manage_request(Some(&item.account),folder,Action::Move{uid:item.uid});
        assert!(h.app.mail_rows().iter().any(|m|m.uid==item.uid));
    }
    h.app.mail_manage_request(Some(&item.account),Folder::Trash,Action::Move{uid:item.uid});
    assert!(h.app.mail.manage.message.contains("刷新"));
}

#[test]
fn new_mail_preserves_an_explicit_reveal_in_the_trash(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=managed_mail_fixture();
    let mut item=h.app.mail.recent[0].clone();item.uid=77;item.code=Some("123456".into());
    h.app.mail.manage.trash.push(item.clone());h.app.reveal_mail_code(&item);
    let incoming=h.app.mail.recent[1].clone();h.app.mail_event(crate::mail::Event::Mail{item:incoming,new:true});
    assert!(!h.app.mail_code_hidden(&item));
    h.app.t+=16.0;assert!(h.app.mail_code_hidden(&item));
}

fn managed_mail_fixture()->H{
    use crate::mail_manage::{Folder,Snapshot};
    let mut h=H::new();h.app.settings.mail_sync_operations=true;let account=crate::mail::Account{custom:None,email:"manage@example.com".into(),provider:crate::mail::Provider::Qq,enabled:true};
    h.app.mail.accounts=vec![account.clone()];
    h.app.mail.recent=(1..=3).map(|uid|crate::mail::Item{html:String::new().into(),received_unix:None,account:account.email.clone(),from:format!("发件人 {uid}"),subject:format!("测试邮件 {uid}"),time:format!("09:4{uid}"),code:None,summary:"完整的测试邮件正文。".repeat(10).into(),links:vec![],meeting:None,uid}).collect();
    h.app.mail.manage.identities.insert(account.email.clone(),account.clone());
    h.app.mail.manage.data.insert((account.email,Folder::Inbox),Snapshot{validity:7,entries:vec![(1,false),(2,true),(3,true)],items:vec![]});
    h.app.show_page(5);h.hover(H::local(220.0,80.0),1.0);h
}
#[test]
fn managed_mail_unread_filter_open_and_mark_read(){
    use crate::mail_manage::{Action,Folder,Completed};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=managed_mail_fixture();
    h.click(H::local(118.0,42.0));assert!(h.app.mail.manage.unread_only);
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="测试邮件 2")));
    h.cmds.clear();h.click(H::local(180.0,64.0));h.frame(vec![]);
    assert_eq!(h.app.mail.open_uid,Some(("manage@example.com".into(),1)));
    let request=h.cmds.iter().find_map(|c|match c{crate::app::Cmd::MailManage(r)if r.action==(Action::Read{uid:1,seen:true})=>Some(r.clone()),_=>None}).expect("打开未读邮件应提交服务器已读操作");
    let mut snapshot=h.app.mail.manage.data[&("manage@example.com".into(),Folder::Inbox)].clone();snapshot.entries[0].1=true;
    h.app.mail_managed(Completed{request,result:Ok(snapshot)});h.hover(H::local(270.0,150.0),0.5);
    h.click(H::local(350.0,48.0));assert!(h.app.mail.open_uid.is_none());h.hover(H::local(200.0,80.0),0.4);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="没有未读邮件")));
}
#[test]
fn managed_mail_trash_icon_and_restore_keep_inbox_uid_separate(){
    use crate::mail_manage::{Action,Folder,Snapshot,Completed};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=managed_mail_fixture();
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().starts_with("回收站"))),"回收站入口只显示图标");
    let item=h.app.mail.recent[0].clone();h.app.mail.manage.trash=vec![item];
    h.app.mail.manage.data.insert(("manage@example.com".into(),Folder::Trash),Snapshot{validity:9,entries:vec![(1,true)],items:vec![]});
    h.click(H::local(370.0,130.0));assert!(h.app.mail.manage.trash_open);
    h.hover(H::local(350.0,64.0),0.2);h.cmds.clear();h.click(H::local(350.0,64.0));
    let request=h.cmds.iter().find_map(|c|match c{crate::app::Cmd::MailManage(r)if r.action==(Action::Move{uid:1})=>Some(r.clone()),_=>None}).expect("回收站恢复按钮应提交移动操作");
    assert_eq!(request.folder,Folder::Trash);assert_eq!(request.validity,Some(9));
    h.app.mail_managed(Completed{request,result:Ok(Snapshot{validity:9,entries:vec![],items:vec![]})});
    assert!(h.app.mail.manage.trash.is_empty());assert_eq!(h.app.mail.recent.len(),3,"同 UID 的收件箱邮件不能被回收站恢复操作移除");
    assert!(h.app.mail.manage.data.contains_key(&("manage@example.com".into(),Folder::Inbox)),"恢复时应保留收件箱正文缓存的有效性，切回后只补读新增邮件");
    h.frame(vec![]);h.click(H::local(350.0,43.0));assert!(!h.app.mail.manage.trash_open);
}
#[test]
fn managed_mail_actions_wait_for_the_server_and_batch_read_is_explicit(){
    use crate::mail_manage::{Action,Completed};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=managed_mail_fixture();h.cmds.clear();
    h.hover(H::local(354.0,64.0),0.2);h.click(H::local(354.0,64.0));
    let request=h.cmds.iter().find_map(|c|match c{crate::app::Cmd::MailManage(r)if r.action==(Action::Move{uid:1})=>Some(r.clone()),_=>None}).expect("删除应提交移到回收站");
    assert_eq!(h.app.mail.recent.len(),3,"服务器确认前不能丢掉邮件");
    h.app.mail_managed(Completed{request,result:Err("服务器拒绝操作".into())});assert_eq!(h.app.mail.recent.len(),3);
    for snapshot in h.app.mail.manage.data.values_mut(){for (_,seen) in &mut snapshot.entries{*seen=true;}}
    h.frame(vec![]);h.cmds.clear();h.click(H::local(350.0,43.0));
    assert!(h.cmds.iter().any(|c|matches!(c,crate::app::Cmd::MailManage(r)if r.action==Action::ReadAll)),"即使最近邮件已读，也应能处理邮箱中更早的未读邮件");
}

#[test]
fn mail_local_mode_persists_without_writing_to_the_server(){
    use crate::mail_manage::{Action,Folder,Request,Completed};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=managed_mail_fixture();h.app.settings.mail_sync_operations=false;h.cmds.clear();
    let original=h.app.mail.recent.clone();let account=h.app.mail.accounts[0].clone();
    let snapshot=h.app.mail.manage.data[&("manage@example.com".into(),Folder::Inbox)].clone();
    h.app.mail_manage_request(Some("manage@example.com"),Folder::Inbox,Action::Read{uid:2,seen:false});
    assert_eq!(h.app.mail_seen(&original[1],Folder::Inbox),Some(false));
    assert!(h.app.mail.manage.data[&("manage@example.com".into(),Folder::Inbox)].entries[1].1,"服务器状态不能被本机已读操作改动");
    h.app.mail_manage_request(Some("manage@example.com"),Folder::Inbox,Action::Move{uid:1});
    assert_eq!(h.app.mail.recent.len(),2);assert_eq!(h.app.mail.manage.trash.len(),1);
    h.frame(vec![]);assert!(!h.cmds.iter().any(|c|matches!(c,crate::app::Cmd::MailManage(r)if r.action!=Action::Load)));
    let path=crate::model::config_dir().join("mail-local-state.json");let saved=std::fs::read_to_string(&path).unwrap();
    assert!(!saved.contains("manage@example.com")&&!saved.contains("测试邮件"),"本机状态仅保存标识和操作标记");
    h.app.mail.manage=Default::default();h.app.mail.recent=original.clone();h.app.mail_reload_local();
    h.app.mail_managed(Completed{request:Request{account,folder:Folder::Inbox,validity:Some(7),known:vec![],extra:vec![],action:Action::Load},result:Ok(snapshot)});
    assert_eq!(h.app.mail.recent.len(),2,"重新读取服务器后，本机移除的邮件不能重新出现");assert_eq!(h.app.mail.manage.trash.len(),1);
    assert_eq!(h.app.mail_seen(&original[1],Folder::Inbox),Some(false),"本机已读状态必须跨重启保留");
    h.app.mail.manage.trash_open=true;h.app.mail_manage_request(Some("manage@example.com"),Folder::Trash,Action::Move{uid:1});
    assert!(h.app.mail.manage.trash.is_empty());assert_eq!(h.app.mail.recent.len(),3);
    h.cmds.clear();h.app.settings.mail_sync_operations=true;h.app.mail_manage_mode_changed();h.frame(vec![]);
    assert!(!h.cmds.iter().any(|c|matches!(c,crate::app::Cmd::MailManage(r)if r.action!=Action::Load)),"开启同步不能补交本机操作");
    assert_eq!(h.app.mail_seen(&original[1],Folder::Inbox),Some(true),"开启同步后应显示服务器真实状态");
}
#[test]
fn mail_sync_switch_defaults_off_and_persists(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    assert!(!crate::model::Settings::default().mail_sync_operations);
    h.app.settings.mail_sync_operations=true;h.app.settings.save();assert!(crate::model::Settings::load().mail_sync_operations);
    h.app.settings.mail_sync_operations=false;h.app.settings.save();assert!(!crate::model::Settings::load().mail_sync_operations);
}

#[test]
fn claude_local_only_settings_show_notice_without_manual_reset_controls(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    fn frame(h:&mut H,raster:&mut crate::preview_raster::Raster){
        h.t+=1.0/60.0;
        let output=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(640.0,760.0))),time:Some(h.t),..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|h.app.quota_settings(ui));});
        raster.update_textures(&output.textures_delta);h.shapes=output.shapes;
    }
    fn capture(h:&H,raster:&mut crate::preview_raster::Raster,name:&str){
        if let Some(dir)=std::env::var_os("DAOZAI_CLAUDE_SETTINGS_PREVIEW"){
            let dir=std::path::PathBuf::from(dir);std::fs::create_dir_all(&dir).unwrap();
            raster.buf.fill([0.0,0.0,0.0,1.0]);raster.draw(&h.ctx.tessellate(h.shapes.clone(),1.0),1.0);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            image::save_buffer(dir.join(format!("{name}.png")),&rgba,640,760,image::ColorType::Rgba8).unwrap();
        }
    }
    for dark in [true,false]{
        let mut h=H::new();h.ctx.set_visuals(if dark{egui::Visuals::dark()}else{egui::Visuals::light()});
        let account=crate::quota::Account{id:"local-test".into(),provider:crate::quota::Provider::Claude,alias:"Claude 本地账号".into(),source:"fixture/config.json".into(),slot:Some("fixture".into()),enabled:true,pinned:true};
        h.app.quota.config.accounts.push(account.clone());
        let snapshot=crate::quota::Snapshot{at:chrono::Utc::now().timestamp(),windows:vec![crate::quota::Window{label:"5 小时".into(),seconds:Some(18000),remaining:37.5,reset:None,amount:None},crate::quota::Window{label:"本周".into(),seconds:Some(604800),remaining:62.0,reset:None,amount:None}],..Default::default()};
        h.app.quota.rows.insert(account.id.clone(),Ok(snapshot.clone()));let mut raster=crate::preview_raster::Raster::new(640,760);
        for connection in [false,true]{
            h.app.quota.provider=crate::quota::Provider::Claude;h.app.quota.connect_open=connection;
            for _ in 0..4{frame(&mut h,&mut raster);}
            for phrase in ["由于 Claude 对网络使用环境要求较高","只读取本机用量记录","重置时间未知","本地读取不构成账号安全保证"]{
                let text=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text().contains(phrase)=>Some((s.clip_rect,t.visual_bounding_rect())),_=>None}).unwrap_or_else(||panic!("缺少说明：{phrase}"));
                assert!(text.0.contains_rect(text.1),"说明不能被裁切：{phrase}");
            }
            for phrase in ["手动设置重置时间","保存时间","清除手动时间","默认预选","手动填写"]{
                assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains(phrase))),"已撤回的手动时间入口及提示不应显示：{phrase}");
            }
            assert_eq!(h.app.quota.rows[&account.id].as_ref().unwrap().windows,snapshot.windows,"撤回输入功能不能影响额度数据");
            let name=format!("{}-{}",if connection{"connection"}else{"account"},if dark{"dark"}else{"light"});capture(&h,&mut raster,&name);
        }
        assert!(h.app.quota.gemini_connecting.is_none()&&h.app.quota.workbuddy_connecting.is_none()&&h.app.quota.login.is_none());
    }

}

#[test]
fn weather_page_shows_the_local_snapshot_and_forecast(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.pet=false;
    h.app.weather.snapshot=Some(crate::weather::Snapshot{condition:None,attributions:vec![],place:"杭州".into(),position:None,auto:false,temp:9.0,feels:Some(5.6),humidity:Some(58),wind:Some(13.9),code:51,day:true,high:Some(15.2),low:Some(6.8),
        sunrise:Some("06:52".into()),sunset:Some("17:21".into()),sun_times:None,
        days:vec![
            crate::weather::Day{label:"今天".into(),date:"2026-11-09".into(),high:15.2,low:6.8,code:51,rain:60},
            crate::weather::Day{label:"明天".into(),date:"2026-11-10".into(),high:14.5,low:4.7,code:2,rain:0},
            crate::weather::Day{label:"周三".into(),date:"2026-11-11".into(),high:17.4,low:5.0,code:2,rain:0},
            crate::weather::Day{label:"周四".into(),date:"2026-11-12".into(),high:19.2,low:8.2,code:61,rain:25},
        ],at:chrono::Utc::now().timestamp()});
    h.app.show_page(8);h.hover(H::local(300.0,60.0),1.0);
    assert_eq!(h.app.page,8,"天气应是第九个页面");
    for phrase in ["9°","毛毛雨","杭州","今天","明天","周三","周四","60%","体感","6°","湿度","58%","日出 06:52","日落 17:21"]{
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains(phrase))),"天气页缺少：{phrase}");
    }
    for s in &h.shapes{if let egui::Shape::Text(t)=&s.shape{assert!(s.clip_rect.contains_rect(t.visual_bounding_rect()),"天气页文字被裁切：{}",t.galley.text());}}
    if let Some(dest)=std::env::var_os("DAOZAI_WEATHER_PREVIEW_DIR"){
        let dest=std::path::PathBuf::from(dest);std::fs::create_dir_all(&dest).unwrap();
        let mut raster=feature_raster();shoot_feature(&mut h,&mut raster,&dest,"weather",30,Some(H::local(300.0,60.0)),"2026-11-09");
    }
}

#[test]
fn weather_page_explains_when_data_is_missing(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.weather.error=Some("网络不可用".into());
    h.app.show_page(8);h.hover(H::local(300.0,60.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="天气暂时取不到")));
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains("网络不可用"))));
}

#[test]
fn weather_chip_sits_after_the_clock_on_the_resident_bar(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.pet=false;
    h.app.weather.snapshot=Some(crate::weather::Snapshot{condition:None,attributions:vec![],place:"杭州".into(),position:None,auto:false,temp:9.0,feels:Some(5.6),humidity:Some(58),wind:Some(13.9),code:51,day:true,high:Some(15.2),low:Some(6.8),sunrise:Some("06:52".into()),sunset:Some("17:21".into()),sun_times:None,days:vec![],at:chrono::Utc::now().timestamp()});
    for _ in 0..60{h.frame(vec![]);}
    let layout=h.app.resident.current();
    let clock=layout.x[crate::bar_layout::CLOCK].expect("时钟应常驻");
    let chip=layout.x[crate::bar_layout::WEATHER].expect("有快照时天气徽标应紧跟时钟占位");
    assert!(chip>clock+22.0+6.9,"天气应在时钟右侧：时钟 {clock}，天气 {chip}");
    let clock_text=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()=="09:41"=>Some(t.visual_bounding_rect()),_=>None}).expect("常驻栏应有时钟");
    let temp=h.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()=="9°"=>Some(t.visual_bounding_rect()),_=>None}).expect("常驻栏应在时间后显示当前温度");
    assert!(temp.left()>clock_text.right()+4.0,"温度应与时钟留出间距：时钟右边 {:?}，温度左边 {:?}",clock_text.right(),temp.left());
    assert!(temp.center().y<30.0,"温度应与图标同一行");
    h.click(temp.center());
    assert_eq!(h.app.page(),8,"点温度也能打开天气页");
}

#[test]
fn weather_chip_hides_without_data_or_when_the_page_is_hidden(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.pet=false;
    for _ in 0..60{h.frame(vec![]);}
    assert!(h.app.resident.current().x[crate::bar_layout::WEATHER].is_none(),"没有快照时不占位");
    h.app.weather.snapshot=Some(crate::weather::Snapshot{condition:None,attributions:vec![],place:"杭州".into(),position:None,auto:false,temp:9.0,feels:None,humidity:None,wind:None,code:0,day:true,high:None,low:None,sunrise:None,sunset:None,sun_times:None,days:vec![],at:chrono::Utc::now().timestamp()});
    for _ in 0..60{h.frame(vec![]);}
    assert!(h.app.resident.current().x[crate::bar_layout::WEATHER].is_some(),"有快照时占位");
    h.app.settings.visible_pages[8]=false;
    for _ in 0..60{h.frame(vec![]);}
    assert!(h.app.resident.current().x[crate::bar_layout::WEATHER].is_none(),"天气页被隐藏后不占位");
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="9°")),"隐藏后不再画温度");
}

#[test]
#[ignore="按需导出常驻天气徽标预览"]
fn weather_resident_bar_preview(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_WEATHER_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    let mut h=H::new();h.app.settings.pet=false;
    h.app.weather.snapshot=Some(crate::weather::Snapshot{condition:None,attributions:vec![],place:"杭州".into(),position:None,auto:false,temp:9.0,feels:Some(5.6),humidity:Some(58),wind:Some(13.9),code:51,day:true,high:Some(15.2),low:Some(6.8),sunrise:Some("06:52".into()),sunset:Some("17:21".into()),sun_times:None,days:vec![],at:chrono::Utc::now().timestamp()});
    let mut raster=crate::preview_raster::Raster::new(1280,380);
    for frame in 0..90{
        let t=frame as f64/60.0;let env=Env{t,clock:"18:07:00".into(),battery:Some(80),charging:false,..Default::default()};
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(640.0,190.0))),time:Some(t),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{h.app.frame(ui,ui.max_rect(),&env);});});
        raster.update_textures(&output.textures_delta);
        if frame==89{
            raster.buf.fill([0.09,0.11,0.18,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            image::save_buffer(dest.join("weather-bar.png"),&rgba,1280,380,image::ColorType::Rgba8).unwrap();
        }
    }
    // 设置页「常驻排列」预览：天气徽标有独立位置。换一个 H，避免复用纹理缓存。
    let mut h=H::new();h.app.settings.pet=false;h.quota_accounts(1);
    h.app.weather.snapshot=Some(crate::weather::Snapshot{condition:None,attributions:vec![],place:"杭州".into(),position:None,auto:false,temp:9.0,feels:Some(5.6),humidity:Some(58),wind:Some(13.9),code:51,day:true,high:Some(15.2),low:Some(6.8),sunrise:Some("06:52".into()),sunset:Some("17:21".into()),sun_times:None,days:vec![],at:chrono::Utc::now().timestamp()});
    let mut raster=crate::preview_raster::Raster::new(1200,300);
    for frame in 0..3{
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(600.0,150.0))),time:Some(frame as f64),
            viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
        let output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().show(ctx,|ui|{h.app.resident_order_settings(ui);});});
        raster.update_textures(&output.textures_delta);
        if frame==2{
            raster.buf.fill([0.13,0.13,0.15,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            image::save_buffer(dest.join("weather-bar-order.png"),&rgba,1200,300,image::ColorType::Rgba8).unwrap();
        }
        raster.free_textures(&output.textures_delta);
    }
}

#[test]
fn weather_icons_stay_bright_on_light_skins(){
    use crate::skin_paint::{map_color,CREAM,GLASS_LIGHT,GLASS_DARK};
    let luma=|c:egui::Color32|0.299*c.r() as f32+0.587*c.g() as f32+0.114*c.b() as f32;
    let cloud=egui::Color32::from_rgb(238,242,250);let sun=egui::Color32::from_rgb(255,196,72);
    for palette in [CREAM,GLASS_LIGHT]{
        // 通用换色会把白云反成深色、把太阳压成暗金；天气图标改用自己的浅色版配色。
        assert!(luma(map_color(cloud,&palette))<90.0);
        assert!(luma(crate::weather_ui::light_tone(cloud,&palette))>190.0,"浅色皮肤下的云应是淡蓝灰");
        let bright=crate::weather_ui::light_tone(sun,&palette);
        assert!(bright.r()>=250&&bright.g()>=160&&bright.b()<80,"太阳保持明亮暖黄：{bright:?}");
        let half=crate::weather_ui::light_tone(sun.gamma_multiply(0.5),&palette);assert_eq!(half.a(),sun.gamma_multiply(0.5).a(),"保留透明度");
    }
    assert_eq!(crate::weather_ui::light_tone(cloud,&GLASS_DARK),map_color(cloud,&GLASS_DARK),"深色皮肤不受影响");
}

#[test]
#[ignore="按需导出常驻天气的全部图标样式"]
fn weather_icon_atlas_preview(){
    use crate::{paint::{Card,H as Align},theme::{self,W},weather::{self,Snapshot}};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_WEATHER_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    let cases=[("晴 · 白天",0,true,"0"),("晴 · 夜间",0,false,"0"),("晴间多云 · 白天",2,true,"1 / 2"),
        ("晴间多云 · 夜间",2,false,"1 / 2"),("阴天",3,true,"3"),("雾 / 雾凇",45,true,"45 / 48"),
        ("细雨 / 冻毛毛雨",51,true,"51 / 53 / 55 / 56 / 57"),("雨 / 冻雨",63,true,"61 / 63 / 65 / 66 / 67"),("阵雨",80,true,"80 / 81 / 82"),
        ("雪 / 阵雪",73,true,"71 / 73 / 75 / 77 / 85 / 86"),("雷雨 / 冰雹",95,true,"95 / 96 / 99"),("未知代码 · 回退云图标",200,true,"其他代码")];
    for (name,palette,ppp) in [("weather-icons-dark",None,2.0f32),("weather-icons-cream",Some(crate::skin_paint::CREAM),2.0),("weather-icons-glass-light",Some(crate::skin_paint::GLASS_LIGHT),2.0),("weather-icons-dark-1x",None,1.0),("weather-icons-dark-125",None,1.25),("weather-icons-dark-150",None,1.5)] {
        let (pw,ph)=((924.0*ppp) as usize,(700.0*ppp) as usize);
        let mut h=H::new();h.app.settings.pet=false;
        let mut layout=crate::bar_layout::Layout::new(false,0,false,false,false,false,true);layout.widths[crate::bar_layout::WEATHER]=50.0;
        let mut raster=crate::preview_raster::Raster::new(pw,ph);
        for frame in 0..4 {
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(924.0,700.0))),time:Some(frame as f64),
                viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()};
            let mut output=h.ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{
                let c=Card::new(ui,Pos2::ZERO,ui.max_rect());c.fill(0.0,0.0,924.0,700.0,0.0,egui::Color32::from_rgb(14,15,19));
                c.text(20.0,38.0,"常驻天气图标",22.0,W::Bold,theme::WHITE,Align::Left);
                c.text(20.0,61.0,"11类图标 · 顶部横排与侧边竖排 · 当前实际绘制样式",11.5,W::Regular,theme::GRAY,Align::Left);
                for (index,&(label,code,day,codes)) in cases.iter().enumerate(){
                    let tile=c.shifted(20.0+(index%3)as f32*298.0,84.0+(index/3)as f32*150.0);
                    tile.fill(0.0,0.0,286.0,138.0,11.0,egui::Color32::from_rgb(25,25,29));
                    tile.text(14.0,23.0,label,12.5,W::Semibold,theme::WHITE,Align::Left);
                    crate::weather_ui::icon(&tile,32.0,63.0,33.0,weather::kind(code,day));
                    let snapshot=Snapshot{place:"杭州".into(),temp:if code==73{-6.0}else{19.0},code,day,at:chrono::Utc::now().timestamp(),..Default::default()};
                    tile.fill(65.0,46.0,148.0,30.0,15.0,egui::Color32::BLACK);
                    let bar=tile.shifted(65.0,46.0);bar.text(32.0,21.0,"10:18",15.5,W::Bold,theme::WHITE,Align::Center);
                    h.app.paint_pill_weather(&bar,105.0,&layout,&snapshot);
                    tile.fill(234.0,40.0,36.0,47.0,18.0,egui::Color32::BLACK);
                    h.app.paint_side_weather(&tile.shifted(234.0,40.0),18.0,23.0,&snapshot);
                    for (x,label) in [(32.0,"放大"),(139.0,"顶部 15px"),(252.0,"侧边")]{tile.text(x,104.0,label,9.5,W::Regular,theme::GRAY,Align::Center);}
                    tile.text(14.0,125.0,&format!("WMO  {codes}"),8.5,W::Regular,theme::DIM,Align::Left);
                }
                c.text(20.0,690.0,"无数据、关闭天气或极简模式时不显示；超过半小时未更新时，温度转灰。",10.5,W::Regular,theme::GRAY,Align::Left);
                // 和岛上一样按图层换色，天气图标登记的浅色配色才会生效。
                if let Some(palette)=palette {let layer=ui.layer_id();crate::skin_paint::recolor_layer(ctx,layer,&[(0,crate::skin_paint::shape_count(ctx,layer))],&palette);}
            });});
            raster.update_textures(&output.textures_delta);
            if frame==3 {raster.buf.fill([0.0,0.0,0.0,1.0]);raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join(format!("{name}.png")),&rgba,pw as u32,ph as u32,image::ColorType::Rgba8).unwrap();}
            raster.free_textures(&output.textures_delta);
        }
    }
}

#[test]
#[ignore="按需导出天气设置页，检查排版"]
fn weather_settings_preview(){
    use crate::settings_theme::SettingsTheme;
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_WEATHER_SETTINGS_PREVIEW").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for (tag,theme,dropdown) in [("dark",SettingsTheme::Dark,false),("light",SettingsTheme::Light,false),("dropdown-dark",SettingsTheme::Dark,true),("dropdown-light",SettingsTheme::Light,true)] {
        let mut h=H::new();h.app.settings.weather_city="杭州".into();h.app.weather.city_input="杭州".into();h.app.weather.manual_city=true;h.app.settings.settings_theme=theme;
        h.ctx.style_mut(|style|style.animation_time=0.0);
        let current=crate::weather::Position{lat:30.27,lon:120.15,label:"杭州".into(),region:"浙江 · 中国".into()};
        h.app.weather.selected_city=Some(current.clone());h.app.weather.active_position=Some(current.clone());
        h.app.settings.weather_position=Some(current.clone());h.app.settings.weather_saved=vec![current.clone(),crate::weather::Position{lat:31.23,lon:121.47,label:"上海".into(),region:"上海 · 中国".into()}];
        h.app.weather.snapshot=Some(crate::weather::Snapshot{place:"杭州".into(),temp:19.0,code:2,day:true,feels:Some(18.0),humidity:Some(58),wind:Some(12.0),high:Some(23.0),low:Some(15.0),
            sunrise:Some("06:52".into()),sunset:Some("17:21".into()),at:chrono::Utc::now().timestamp(),..Default::default()});
        h.app.weather.snapshot.as_mut().unwrap().position=Some(current);
        if dropdown {
            h.app.weather.city_input="佛山".into();
            h.app.weather.cities.seed("佛山",vec![
                crate::weather::Position{lat:23.02677,lon:113.13148,label:"佛山市".into(),region:"广东 · 中国".into()},
                crate::weather::Position{lat:28.73401,lon:98.80612,label:"佛山".into(),region:"迪庆藏族自治州 · 云南 · 中国".into()},
                crate::weather::Position{lat:30.04572,lon:106.41138,label:"佛山".into(),region:"重庆市 · 中国".into()},
            ]);
        }
        h.app.settings_tab=12;h.app.open_settings();h.ctx.set_embed_viewports(false);
        let children=std::sync::Arc::new(Mutex::new(Vec::new()));let capture=children.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
            let id=viewport.ids.this;
            let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(800.0,620.0))),
                viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
            capture.lock().unwrap().push(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));
        });
        let mut raster=crate::preview_raster::Raster::new(1600,1240);
        for frame in 0..4 {
            if dropdown && frame==1 {h.ctx.memory_mut(|m|m.open_popup(egui::Id::new("weather-city-options")));}
            let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(frame as f64),..Default::default()};
            let root=h.ctx.run(raw,|ctx|h.app.settings_ui(ctx,false));
            let child_frames:Vec<_>=children.lock().unwrap().drain(..).collect();
            for child in &child_frames {raster.update_textures(&child.textures_delta);}
            raster.update_textures(&root.textures_delta);
            for child in child_frames {
                if frame==3 {raster.buf.fill([0.1,0.1,0.1,1.0]);raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join(format!("weather-settings-{tag}.png")),&rgba,1600,1240,image::ColorType::Rgba8).unwrap();}
                raster.free_textures(&child.textures_delta);
            }
            raster.free_textures(&root.textures_delta);
        }
        assert!(dest.join(format!("weather-settings-{tag}.png")).exists(),"必须导出真实设置窗口");
    }
}

#[test]
#[cfg(windows)]
fn settings_window_memory_restores_only_the_child_viewport_and_keeps_live_resizing(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.ctx.set_embed_viewports(false);h.app.open_settings();
    let original=(h.app.settings.island_size,h.app.settings.dock,h.app.settings.display_id.clone());
    let size=std::sync::Arc::new(std::sync::Mutex::new(egui::vec2(900.0,600.0)));
    let resized=size.clone();
    egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
        let id=viewport.ids.this;let size=*resized.lock().unwrap();
        let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),
            viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.0),
                inner_rect:Some(Rect::from_min_size(egui::pos2(100.0,140.0),size)),
                outer_rect:Some(Rect::from_min_size(egui::pos2(92.0,100.0),size+egui::vec2(16.0,48.0))),..Default::default()})).collect(),..Default::default()};
        let _=ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx));
    });
    let frame=|h:&mut H|h.ctx.run(egui::RawInput{viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(1.75),..Default::default()})).collect(),..Default::default()},|ctx|h.app.settings_ui(ctx,false));
    let _=frame(&mut h);h.app.settings_open=false;let _=frame(&mut h);
    *size.lock().unwrap()=egui::vec2(690.0,560.0);h.app.open_settings();let root=frame(&mut h);
    assert!(root.viewport_output.iter().filter(|(id,_)|**id!=egui::ViewportId::ROOT).flat_map(|(_,v)|&v.commands).any(|c|matches!(c,egui::ViewportCommand::OuterPosition(_))),"重开时应恢复子窗口");
    assert!(!root.viewport_output.get(&egui::ViewportId::ROOT).unwrap().commands.iter().any(|c|matches!(c,egui::ViewportCommand::OuterPosition(_)|egui::ViewportCommand::InnerSize(_))));
    *size.lock().unwrap()=egui::vec2(1100.0,700.0);let live=frame(&mut h);
    assert!(!live.viewport_output.values().flat_map(|v|&v.commands).any(|c|matches!(c,egui::ViewportCommand::InnerSize(_))),"正常拖动不能每帧强制恢复旧尺寸");
    assert_eq!((h.app.settings.island_size,h.app.settings.dock,h.app.settings.display_id.clone()),original);
}

#[test]
fn quota_acceptance_monthly_limit_is_visible_on_the_card() {
    use crate::quota::{Provider,Account,plans};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let snapshot=plans::parse(plans::PlanProvider::OpenCodeGo,&serde_json::json!({"usage":{"rolling":{"percent":20},"weekly":{"percent":30},"monthly":{"percent":100}}})).unwrap();
    h.app.quota.config.accounts=vec![Account{id:"preview".into(),provider:Provider::OpenCodeGo,alias:"OpenCode Go".into(),source:Default::default(),slot:None,enabled:true,pinned:false}];
    h.app.quota.config.selected="preview".into();h.app.quota.rows.insert("preview".into(),Ok(snapshot));h.app.show_page(6);h.hover(H::local(230.0,65.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains("本月"))),"月额度已经用完，但卡片没有直接显示本月限制");
}

#[test]
fn quota_acceptance_closing_settings_cancels_the_key_dialog() {
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings_tab=11;h.app.open_settings();h.ctx.set_embed_viewports(false);
    h.app.quota.provider=crate::quota::Provider::OpenCodeGo;h.app.quota.connect_open=true;h.app.quota.api_key="synthetic".into();
    let (connection,cancelled)=crate::quota::api::Connect::pending_test();h.app.quota.api_connecting=Some(connection);
    egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{let id=viewport.ids.this;let _=ctx.run(egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,700.0))),viewports:std::iter::once((id,egui::ViewportInfo{events:vec![egui::ViewportEvent::Close],..Default::default()})).collect(),..Default::default()},|ctx|(viewport.viewport_ui_cb)(ctx));});
    let _=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));
    assert!(cancelled.load(std::sync::atomic::Ordering::Acquire));assert!(h.app.quota.api_connecting.is_none());
    assert!(!h.app.settings_open);assert!(!h.app.quota.connect_open&&h.app.quota.api_key.is_empty(),"关闭设置窗口后，密钥输入和连接状态仍未取消");
}

#[test]
fn quota_acceptance_relay_subscription_exhausted_week_is_visible() {
    use crate::quota::{Provider,Account,relay};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let snapshot=relay::parse_sub2api(&serde_json::json!({"mode":"unrestricted","isValid":true,"unit":"USD","remaining":0,"subscription":{"daily_limit_usd":10,"daily_usage_usd":2,"weekly_limit_usd":20,"weekly_usage_usd":20}})).unwrap();
    h.app.quota.config.accounts=vec![Account{id:"relay".into(),provider:Provider::Sub2Api,alias:"工作中转".into(),source:"https://relay.example".into(),slot:None,enabled:true,pinned:false}];h.app.quota.config.selected="relay".into();h.app.quota.rows.insert("relay".into(),Ok(snapshot));h.app.show_page(6);h.hover(H::local(230.0,65.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains("每周"))),"卡片必须显示已耗尽的套餐周期");
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="0%")),"套餐剩余额度应显示百分比");
}

#[test]
fn quota_acceptance_relay_key_only_response_shows_balance_unavailable(){
    use crate::quota::{Provider,Account,relay};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let snapshot=relay::parse_new_api(&serde_json::json!({"code":true,"data":{"object":"token_usage","unlimited_quota":true,"total_used":2}})).unwrap();
    h.app.quota.config.accounts=vec![Account{id:"relay".into(),provider:Provider::NewApi,alias:"测试站点".into(),source:"https://relay.example".into(),slot:None,enabled:true,pinned:true}];
    h.app.quota.config.selected="relay".into();h.app.quota.rows.insert("relay".into(),Ok(snapshot));h.app.show_page(6);h.hover(H::local(230.0,65.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="余额暂不可查询")));
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains("不限额")||t.galley.text().contains("未设置限额"))));
}



#[test]
fn quota_acceptance_new_api_user_balance_is_visible(){
    use crate::quota::{Provider,Account,relay};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let snapshot=relay::parse_new_api_user(&serde_json::json!({"success":true,"data":{"quota":550000,"used_quota":1000000}}),&serde_json::json!({"success":true,"data":{"quota_per_unit":500000,"quota_display_type":"USD"}})).unwrap();
    h.app.quota.config.accounts=vec![Account{id:"relay-user".into(),provider:Provider::NewApi,alias:"测试站点".into(),source:"https://relay.example".into(),slot:None,enabled:true,pinned:true}];
    h.app.quota.config.selected="relay-user".into();h.app.quota.rows.insert("relay-user".into(),Ok(snapshot));h.app.show_page(6);h.hover(H::local(230.0,65.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="1.1 USD")));
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="余额暂不可查询")));
}

#[test]
fn quota_new_api_user_connection_switches_without_reusing_api_key(){
    fn draw(h:&mut H,raster:&mut crate::preview_raster::Raster,events:Vec<egui::Event>)->egui::FullOutput{
        h.t+=0.2;let output=h.ctx.run(egui::RawInput{time:Some(h.t),screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(780.0,850.0))),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|h.app.quota_settings(ui));});
        raster.update_textures(&output.textures_delta);output
    }
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dark in [true,false]{
        let mut h=H::new();h.ctx.set_visuals(if dark{egui::Visuals::dark()}else{egui::Visuals::light()});
        h.app.settings_open=true;h.app.quota.provider=crate::quota::Provider::NewApi;h.app.quota.connect_open=true;h.app.quota.source="https://relay.example/tenant".into();h.app.quota.api_key="synthetic-api-key".into();
        let mut raster=crate::preview_raster::Raster::new(780,850);
        let _=draw(&mut h,&mut raster,vec![]);let _=draw(&mut h,&mut raster,vec![]);let output=draw(&mut h,&mut raster,vec![]);
        let pos=output.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()=="用户 Token（账户余额）"=>Some(t.visual_bounding_rect().center()),_=>None}).expect("应提供用户 Token 连接入口");
        let _=draw(&mut h,&mut raster,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
        let _=draw(&mut h,&mut raster,vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
        assert!(h.app.quota.relay_user_token);assert!(h.app.quota.api_key.is_empty());assert_eq!(h.app.quota.source,"https://relay.example/tenant");
        h.app.quota.relay_user_id="42".into();h.app.quota.api_key="synthetic-user-access-token".into();
        let _=draw(&mut h,&mut raster,vec![]);let output=draw(&mut h,&mut raster,vec![]);
        assert!(output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="用户 ID")));
        assert!(!output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains("synthetic-user-access-token"))));
        if let Some(dest)=std::env::var_os("DAOZAI_NEW_API_PREVIEW"){
            raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            let path=std::path::PathBuf::from(dest);std::fs::create_dir_all(&path).unwrap();image::save_buffer(path.join(if dark{"new-api-user-dark.png"}else{"new-api-user-light.png"}),&rgba,780,850,image::ColorType::Rgba8).unwrap();
        }
        h.app.quota_close_connection();assert!(h.app.quota.api_key.is_empty()&&h.app.quota.relay_user_id.is_empty());assert!(!h.app.quota.relay_user_token);
    }
}

#[test]
fn quota_cc_switch_general_and_custom_connections_render_in_both_themes(){
    use crate::quota::{Provider,relay};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for provider in [Provider::RelayGeneral,Provider::RelayCustom]{for dark in [true,false]{
        let mut h=H::new();h.ctx.set_visuals(if dark{egui::Visuals::dark()}else{egui::Visuals::light()});
        h.app.settings_open=true;h.app.quota.connect_open=true;h.app.quota.provider=provider;h.app.quota.relay_manual=true;h.app.quota.alias="我的中转站".into();h.app.quota.source="https://relay.example".into();h.app.quota.api_key="synthetic-private-key".into();h.app.quota.relay_script=relay::script::DEFAULT_SCRIPT.into();
        let mut raster=crate::preview_raster::Raster::new(780,850);
        for frame in 0..4{
            let output=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(780.0,850.0))),..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|h.app.quota_settings(ui));});raster.update_textures(&output.textures_delta);
            if frame==3{
                let texts:Vec<_>=output.shapes.iter().filter_map(|s|match &s.shape{egui::Shape::Text(t)=>Some(t.galley.text()),_=>None}).collect();let has_button=texts.contains(&"验证并连接");assert!(!texts.iter().any(|s|s.contains("synthetic-private-key")));
                if provider==Provider::RelayCustom{assert!(texts.contains(&"查询脚本"));}
                if let Some(dest)=std::env::var_os("DAOZAI_CC_SWITCH_PREVIEW"){
                    raster.draw(&h.ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);let path=std::path::PathBuf::from(dest);std::fs::create_dir_all(&path).unwrap();image::save_buffer(path.join(format!("{}-{}.png",if provider==Provider::RelayCustom{"custom"}else{"general"},if dark{"dark"}else{"light"})),&rgba,780,850,image::ColorType::Rgba8).unwrap();
                }
                assert!(has_button,"{provider:?} 连接按钮应可见");
            }
        }
        h.app.quota_close_connection();assert!(h.app.quota.relay_script.is_empty()&&h.app.quota.api_key.is_empty());
    }}
}

#[test]
fn quota_acceptance_grouped_connections_preserve_saved_sources() {
    use crate::quota::{Provider,Account};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for provider in [Provider::ZCode,Provider::Glm,Provider::NewApi,Provider::OneApi,Provider::Sub2Api] {
        let mut h=H::new();h.app.settings_open=true;h.app.quota.provider=provider;h.app.quota.connect_open=true;
        let source=if provider.relay(){"https://relay.example/path"}else if provider==Provider::Glm{"global"}else{"fixture/credentials.json"};
        let account=Account{id:"existing".into(),provider,alias:"原账号".into(),source:source.into(),slot:Some("existing-reference".into()),enabled:true,pinned:false};
        h.app.quota.config.accounts=vec![account.clone()];h.app.quota.config.selected=account.id.clone();h.app.quota.connect_target=Some(account.id.clone());h.app.quota.source=source.into();
        for _ in 0..3{
            let output=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(780.0,850.0))),..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|h.app.quota_settings(ui));});h.shapes=output.shapes;
        }
        let expected=if provider.relay(){"中转站"}else{"GLM / ZCode"};
        assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()==expected)),"应显示统一入口：{expected}");
        assert_eq!(h.app.quota.provider,provider);assert_eq!(h.app.quota.source,source);assert_eq!(h.app.quota.config.accounts,[account]);
        if provider.relay(){assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="高级设置")));}
    }
}

#[test]
fn quota_glm_get_key_link_follows_region_without_submitting_the_key(){
    fn draw(h:&mut H,events:Vec<egui::Event>)->egui::FullOutput{
        h.t+=0.2;
        h.ctx.run(egui::RawInput{time:Some(h.t),screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(780.0,850.0))),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|h.app.quota_settings(ui));})
    }
    fn click(h:&mut H,output:&egui::FullOutput,label:&str)->egui::FullOutput{
        let pos=output.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==label=>Some(t.visual_bounding_rect().center()),_=>None}).unwrap_or_else(||panic!("找不到控件：{label}"));
        let _=draw(h,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
        let _=draw(h,vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
        let _=draw(h,vec![]);draw(h,vec![])
    }
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dark in [true,false]{
        let mut h=H::new();h.ctx.set_visuals(if dark{egui::Visuals::dark()}else{egui::Visuals::light()});
        h.app.settings_open=true;h.app.quota.provider=crate::quota::Provider::Glm;h.app.quota.connect_open=true;h.app.quota.source="cn".into();h.app.quota.api_key="synthetic-glm-key".into();
        let _=draw(&mut h,vec![]);let _=draw(&mut h,vec![]);let mut output=draw(&mut h,vec![]);
        for (region,url) in [("中国区","https://bigmodel.cn/coding-plan/personal/overview"),("国际区","https://z.ai/manage-apikey/apikey-list")]{
            output=click(&mut h,&output,region);h.app.pending.clear();output=click(&mut h,&output,"获取密钥");
            assert!(h.app.pending.iter().any(|cmd|matches!(cmd,crate::app::Cmd::OpenUrl(target)if target==url)));
            assert_eq!(h.app.quota.api_key,"synthetic-glm-key");assert!(h.app.quota.api_connecting.is_none());assert!(h.app.quota.config.accounts.is_empty());
        }
    }
}

#[test]
fn quota_relay_automatic_is_the_default_and_manual_types_are_hidden() {
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings_open=true;h.app.quota.provider=crate::quota::Provider::NewApi;h.app.quota.connect_open=true;
    for _ in 0..3{
        let output=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(780.0,850.0))),..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|h.app.quota_settings(ui));});h.shapes=output.shapes;
    }
    let texts=h.shapes.iter().filter_map(|s|match &s.shape{egui::Shape::Text(t)=>Some(t.galley.text()),_=>None}).collect::<Vec<_>>();
    assert!(texts.contains(&"自动识别站点类型"));assert!(texts.contains(&"高级设置"));
    assert!(!texts.contains(&"站点类型")&&!texts.contains(&"One API")&&!texts.contains(&"Sub2API"));
}

#[test]
fn quota_relay_manual_types_require_advanced_settings_and_preserve_the_site(){
    fn draw(h:&mut H,events:Vec<egui::Event>)->egui::FullOutput{
        h.t+=0.2;
        h.ctx.run(egui::RawInput{time:Some(h.t),screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(780.0,850.0))),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|h.app.quota_settings(ui));})
    }
    fn click(h:&mut H,output:&egui::FullOutput,label:&str)->egui::FullOutput{
        let pos=output.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Text(t)if t.galley.text()==label=>Some(t.visual_bounding_rect().center()),_=>None}).unwrap_or_else(||panic!("找不到控件：{label}"));
        let _=draw(h,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
        let _=draw(h,vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
        let _=draw(h,vec![]);draw(h,vec![])
    }
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dark in [true,false]{
        let mut h=H::new();h.ctx.set_visuals(if dark{egui::Visuals::dark()}else{egui::Visuals::light()});
        h.app.settings_open=true;h.app.quota.provider=crate::quota::Provider::NewApi;h.app.quota.connect_open=true;h.app.quota.source="https://relay.example/tenant/v1".into();
        let mut raster=crate::preview_raster::Raster::new(780,850);
        let first=draw(&mut h,vec![]);raster.update_textures(&first.textures_delta);
        let second=draw(&mut h,vec![]);raster.update_textures(&second.textures_delta);
        let output=draw(&mut h,vec![]);raster.update_textures(&output.textures_delta);
        if let Some(dest)=std::env::var_os("DAOZAI_RELAY_PREVIEW"){
            let jobs=h.ctx.tessellate(output.shapes.clone(),output.pixels_per_point);raster.draw(&jobs,output.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
            let path=std::path::PathBuf::from(dest);std::fs::create_dir_all(&path).unwrap();image::save_buffer(path.join(if dark{"connect-auto-dark.png"}else{"connect-auto-light.png"}),&rgba,780,850,image::ColorType::Rgba8).unwrap();
        }
        let output=click(&mut h,&output,"高级设置");let output=click(&mut h,&output,"自动识别（推荐）");let output=click(&mut h,&output,"Sub2API");
        assert!(h.app.quota.relay_manual);assert_eq!(h.app.quota.provider,crate::quota::Provider::Sub2Api);assert_eq!(h.app.quota.source,"https://relay.example/tenant/v1");
        let output=click(&mut h,&output,"Sub2API");let _=click(&mut h,&output,"自动识别（推荐）");assert!(!h.app.quota.relay_manual);
    }
}

#[test]
fn quota_acceptance_details_open_the_correct_account() {
    use crate::quota::{Provider,Account,relay};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let s=relay::parse_sub2api(&serde_json::json!({"mode":"unrestricted","isValid":true,"unit":"USD","balance":0})).unwrap();
    let a=Account{id:"selected-relay".into(),provider:Provider::Sub2Api,alias:"原站点".into(),source:"https://relay.example".into(),slot:None,enabled:true,pinned:false};
    h.app.quota.config.accounts=vec![a.clone()];h.app.quota.config.selected=a.id.clone();h.app.quota.rows.insert(a.id.clone(),Ok(s));h.app.show_page(6);h.hover(H::local(230.0,65.0),1.0);
    h.click_text("明细 ›");assert!(h.app.settings_open);assert_eq!(h.app.settings_tab,11);assert_eq!(h.app.quota.config.selected,a.id);
}
#[test]
fn quota_acceptance_period_colors_match_the_drawn_rings() {
    use crate::quota::{Accent,Account,Provider,Snapshot,Window};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let id="period-colors";
    h.app.quota.config.accounts=vec![Account{id:id.into(),provider:Provider::MiniMax,alias:"MiniMax".into(),source:Default::default(),slot:None,enabled:true,pinned:false}];
    h.app.quota.config.selected=id.into();h.app.quota.config.colors.insert(id.into(),Accent::Pink);
    let outer=egui::Color32::from_rgb(255,43,132);let inner=crate::paint::mix(outer,egui::Color32::WHITE,0.16);
    for seconds in [[None,None],[Some(18000),Some(604800)]] {
        h.app.quota.rows.insert(id.into(),Ok(Snapshot{windows:vec![
            Window{label:"当前周期".into(),seconds:seconds[0],remaining:62.0,reset:None,amount:None},
            Window{label:"本周".into(),seconds:seconds[1],remaining:35.0,reset:None,amount:None},
        ],extra:vec![Window{label:"工具".into(),seconds:None,remaining:80.0,reset:None,amount:None}],..Default::default()}));
        let output=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(400.0,200.0))),..Default::default()},|ctx|{
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{
                let c=crate::paint::Card::new(ui,Pos2::ZERO,ui.max_rect());h.app.page_quota(&c);
            });
        });
        let expected=if seconds[0].is_none(){[inner,outer,crate::theme::GRAY]}else{[outer,inner,crate::theme::GRAY]};
        for (i,color) in expected.into_iter().enumerate(){
            let center=egui::pos2(159.0,79.0+i as f32*23.0);
            let dot=output.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Circle(c)if(c.radius-2.5).abs()<0.01&&c.center.distance(center)<0.01=>Some(c.fill),_=>None}).expect("应显示周期色点");
            assert_eq!(dot,color,"周期色点应对应实际圆环，未画入圆环的工具额度使用灰色");
        }
    }
}

#[test]
#[ignore="按需导出套餐、中转、深浅主题和连接窗口的实际界面"]
fn quota_acceptance_gallery() {
    use crate::quota::{Provider,Snapshot,Account,plans,relay};
    use serde_json::json;
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_QUOTA_PREVIEW").unwrap());std::fs::create_dir_all(&dest).unwrap();
    let providers=[Provider::OpenCodeGo,Provider::MiniMax,Provider::Glm,Provider::NewApi,Provider::OneApi,Provider::Sub2Api];
    let snapshots:Vec<Snapshot>=vec![
        plans::parse(plans::PlanProvider::OpenCodeGo,&json!({"usage":{"rolling":{"percent":25,"resetsAt":"2030-01-01T00:00:00Z"},"weekly":{"percent":50,"resetsAt":"2030-01-03T00:00:00Z"},"monthly":{"percent":100,"resetsAt":"2030-02-01T00:00:00Z"}}})).unwrap(),
        plans::parse(plans::PlanProvider::MiniMaxChina,&json!({"model_remains":[{"model_name":"general","current_interval_remaining_percent":62,"current_weekly_remaining_percent":35}]})).unwrap(),
        plans::parse(plans::PlanProvider::GlmChina,&json!({"data":{"limits":[{"type":"TOKENS_LIMIT","percentage":30,"unit":3,"number":5,"nextResetTime":1900000000000i64},{"type":"CREDIT_LIMIT","percentage":52,"unit":6,"number":1,"nextResetTime":1900604800000i64},{"type":"TIME_LIMIT","percentage":10}]}})).unwrap(),
        relay::parse_new_api(&json!({"code":true,"data":{"object":"token_usage","total_granted":500000,"total_available":250000,"total_used":250000,"unlimited_quota":false,"user_quota":{"total_available":100000,"total_used":400000}}})).unwrap(),
        relay::parse_one_api(&json!({"object":"billing_subscription","hard_limit_usd":100}),&json!({"object":"list","total_usage":2500})).unwrap(),
        relay::parse_sub2api(&json!({"mode":"unrestricted","isValid":true,"unit":"USD","remaining":0,"subscription":{"daily_limit_usd":10,"daily_usage_usd":5,"weekly_limit_usd":20,"weekly_usage_usd":20}})).unwrap(),
    ];
    for (index,provider) in providers.into_iter().enumerate() {
        let mut h=H::new();h.app.settings.pet=false;
        let a=Account{id:"preview".into(),provider,alias:provider.name().into(),source:if provider.relay(){"https://relay.example".into()}else{"cn".into()},slot:Some("synthetic-reference".into()),enabled:true,pinned:true};
        h.app.quota.config.accounts=vec![a.clone()];h.app.quota.config.selected=a.id.clone();h.app.quota.rows.insert(a.id,Ok(snapshots[index].clone()));
        h.app.show_page(6);let mut raster=feature_raster();
        shoot_feature(&mut h,&mut raster,&dest,&format!("island-{index}"),60,Some(H::local(240.0,65.0)),"2026-11-10");
    }
    for (group,indices) in [("plans",vec![0usize,1,2]),("relay",vec![3usize,4,5])] {
        for light in [false,true] {
            let mut h=H::new();h.app.settings.settings_theme=if light{crate::settings_theme::SettingsTheme::Light}else{crate::settings_theme::SettingsTheme::Dark};
            for &i in &indices {
                let id=format!("preview-{i}");h.app.quota.config.accounts.push(Account{id:id.clone(),provider:providers[i],alias:providers[i].name().into(),source:if providers[i].relay(){"https://relay.example".into()}else{"cn".into()},slot:Some("synthetic-reference".into()),enabled:true,pinned:true});
                h.app.quota.rows.insert(id.clone(),Ok(snapshots[i].clone()));if h.app.quota.config.selected.is_empty(){h.app.quota.config.selected=id;}
            }
            h.app.settings_tab=11;h.app.open_settings();h.ctx.set_embed_viewports(false);
            let size=egui::vec2(780.0,1080.0);let ppp=1.25;let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
            egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{let id=viewport.ids.this;let raw=egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(ppp),focused:Some(true),..Default::default()})).collect(),focused:true,..Default::default()};*capture.lock().unwrap()=Some(ctx.run(raw,|ctx|(viewport.viewport_ui_cb)(ctx)));});
            let mut raster=crate::preview_raster::Raster::new((size.x*ppp)as usize,(size.y*ppp)as usize);
            for f in 0..5{
                let root=h.ctx.run(egui::RawInput{viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(ppp),..Default::default()})).collect(),..Default::default()},|ctx|h.app.settings_ui(ctx,false));
                raster.update_textures(&root.textures_delta);let child=output.lock().unwrap().take().unwrap();raster.update_textures(&child.textures_delta);
                if f==4{raster.buf.fill([0.1,0.1,0.11,1.0]);raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join(format!("settings-{group}-{}.png",if light{"light"}else{"dark"})),&rgba,(size.x*ppp)as u32,(size.y*ppp)as u32,image::ColorType::Rgba8).unwrap();}
                raster.free_textures(&root.textures_delta);raster.free_textures(&child.textures_delta);
            }
        }
    }
    for (i,provider) in providers.into_iter().enumerate() {
        let mut h=H::new();h.app.quota.provider=provider;h.app.quota.connect_open=true;
        h.app.settings.settings_theme=crate::settings_theme::SettingsTheme::Dark;h.app.settings_tab=11;h.app.open_settings();h.ctx.set_embed_viewports(false);
        let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
        egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{let id=viewport.ids.this;*capture.lock().unwrap()=Some(ctx.run(egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,600.0))),viewports:std::iter::once((id,egui::ViewportInfo{native_pixels_per_point:Some(1.5),focused:Some(true),..Default::default()})).collect(),..Default::default()},|ctx|(viewport.viewport_ui_cb)(ctx)));});
        let mut raster=crate::preview_raster::Raster::new(1035,900);
        for f in 0..5{
            let root=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));raster.update_textures(&root.textures_delta);let child=output.lock().unwrap().take().unwrap();raster.update_textures(&child.textures_delta);
            if f==4{raster.buf.fill([0.1,0.1,0.11,1.0]);raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);image::save_buffer(dest.join(format!("connect-{i}.png")),&rgba,1035,900,image::ColorType::Rgba8).unwrap();}
            raster.free_textures(&root.textures_delta);raster.free_textures(&child.textures_delta);
        }
    }
    let images:Vec<_>=(0..6).map(|i|image::open(dest.join(format!("island-{i}.png"))).unwrap().to_rgba8()).collect();
    let width=images.iter().map(|i|i.width()).max().unwrap();let height=images.iter().map(|i|i.height()).max().unwrap();
    let mut sheet=image::RgbaImage::from_pixel(width*2,height*3,image::Rgba([18,20,26,255]));
    for (i,img) in images.iter().enumerate(){image::imageops::replace(&mut sheet,img,((i%2)as u32*width)as i64,((i/2)as u32*height)as i64);}sheet.save(dest.join("island-overview.png")).unwrap();
}

#[test]
fn media_sampling_is_fast_only_while_the_music_page_is_watched(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.media=crate::smtc::NowPlaying{source:"QQMusic.exe".into(),title:"歌曲".into(),artist:"歌手".into(),duration:120.0,playing:true,..Default::default()};
    h.frame(vec![]);
    h.app.open=false;
    assert_eq!(h.app.media_poll_interval(),crate::smtc::POLL_BACKGROUND,"收起且不显示歌词时只需跟上歌名");
    h.app.settings.island_lyrics=true;h.frame(vec![]);h.app.open=false;
    assert_eq!(h.app.media_poll_interval(),crate::smtc::POLL_LYRICS,"收起显示歌词时次快");
    h.app.open=true;h.app.page=0;
    assert_eq!(h.app.media_poll_interval(),crate::smtc::POLL_FAST,"展开音乐页保持原来的 250 毫秒");
    h.app.page=1;
    assert_eq!(h.app.media_poll_interval(),crate::smtc::POLL_BACKGROUND,"展开其他页面时不必按最快频率采样");
}

#[test]
fn calendar_signature_matches_without_copying(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let h=H::new();
    let known=h.app.calendar_configs();
    assert!(h.app.calendar_configs_match(&known));
    assert_eq!(h.app.calendar_configs_match(&[]),known.is_empty());
}

fn raised(h:&H,title:&str)->bool{
    h.app.alerts.iter().chain(h.app.notice.as_ref()).any(|alert|alert.title==title)
}

#[test]
fn closed_modules_stop_their_reminders_and_timers(){
    use crate::app::{FOCUS_PAGE,TODO_PAGE,WATER_PAGE};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for open in [true,false] {
        let mut h=H::new();h.frame(vec![]);
        h.app.pomo.running=true;h.app.pomo.remaining=0.5;
        for page in [FOCUS_PAGE,TODO_PAGE,WATER_PAGE] {h.app.set_page_visible(page,open);}
        h.app.water.remind=true;h.app.water.drunk_ml=0;h.app.last_reminder=h.t-1e6;
        h.app.todos.items.push(crate::model::TodoItem{text:"交周报".into(),due:Some("2026-10-28 09:40".into()),..Default::default()});
        if !open {assert!(!h.app.pomo.running,"关闭专注模块时立即停止计时");h.app.pomo.running=true;}
        for _ in 0..60 {h.frame(vec![]);}
        assert_eq!(raised(&h,"喝水时间到了"),open,"饮水模块开启={open}");
        assert_eq!(raised(&h,"待办到时间了"),open,"待办模块开启={open}");
        assert_eq!(raised(&h,"专注计时"),open,"专注模块开启={open}");
        if !open {assert!(!h.app.pomo.running && h.app.todos.items[0].due.is_some() && !h.app.todos.items[0].reminded,"关闭期间不消耗提醒，重新开启后仍会提醒");}
    }
}

#[test]
fn background_services_follow_the_module_switch(){
    use crate::app::{MAIL_PAGE,QUOTA_PAGE,SHELF_PAGE};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.mail_enabled=true;h.app.settings.clipboard_history=true;h.app.quota.config.enabled=true;
    assert!(h.app.mail_active() && h.app.clipboard_active() && h.app.quota_active());
    for page in [MAIL_PAGE,QUOTA_PAGE,SHELF_PAGE] {h.app.set_page_visible(page,false);}
    assert!(!h.app.mail_active() && !h.app.clipboard_active() && !h.app.quota_active(),"页面开关就是模块总开关");
    h.frame(vec![]);
    assert!(!h.app.clipboard.is_enabled(),"置物架关闭后不再记录剪贴板");
    for page in [MAIL_PAGE,QUOTA_PAGE,SHELF_PAGE] {h.app.set_page_visible(page,true);}
    h.app.settings.mail_enabled=false;h.app.settings.clipboard_history=false;h.app.quota.config.enabled=false;
    assert!(!h.app.mail_active() && !h.app.clipboard_active() && !h.app.quota_active(),"模块内的开关仍然有效");
}

#[test]
fn every_module_settings_tab_maps_to_its_page(){
    let titles=["常规","音乐与歌词","专注","健康提醒","待办","置物","通知","邮箱","日程","关于","页面管理","AI 额度","天气"];
    let mut pages:Vec<_>=(0..titles.len()).filter_map(crate::extras::settings_tab_module).collect();
    for tab in 0..titles.len() {
        if let Some(page)=crate::extras::settings_tab_module(tab) {assert!(crate::app::PAGE_NAMES[page].chars().next()==titles[tab].chars().next(),"{} ↔ {}",titles[tab],crate::app::PAGE_NAMES[page]);}
    }
    pages.sort();assert_eq!(pages,(0..crate::app::PAGE_NAMES.len()).collect::<Vec<_>>(),"每个功能页面各有一个设置分页");
}
