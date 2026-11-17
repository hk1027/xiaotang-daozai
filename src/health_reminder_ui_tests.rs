use super::*;

fn due() -> H {
    let mut h=H::new();h.frame(vec![]);
    h.app.settings.sit_remind=true;h.app.water.remind=true;
    h.app.settings.lunch_rest=false;h.app.settings.water_start=0;h.app.settings.water_end=0;
    h.app.settings.save();h.app.water.save();
    h.t+=61.0*60.0;h.frame(vec![]);
    h.t+=20.0;h.frame(vec![]);h.t+=20.0;h.frame(vec![]);
    for _ in 0..60{h.frame(vec![]);}
    h
}

fn badge(h:&H,kind:&str)->Rect {
    h.ctx.read_response(egui::Id::new(("card",true,format!("health-{kind}"))))
        .unwrap_or_else(||panic!("缺少 {kind} 常驻入口")).rect
}

fn click_badge(h:&mut H,kind:&str) {
    h.frame(vec![]);h.frame(vec![]);
    let p=badge(h,kind).center();
    h.hover(p,0.6);h.click(p);
    assert!(!h.app.health_pending[if kind=="water"{0}else{1}],"悬停提示不能挡住常驻入口点击");
}

#[test]
fn health_reminders_wait_without_repeating_or_advancing_timers(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=due();
    let (water_start,sit_due)=(h.app.last_reminder,h.app.sit_due());
    let alerts=h.app.history.len();
    h.screen_locked=true;h.system_idle=Some(500.0*60.0);h.t+=500.0*60.0;h.frame(vec![]);
    assert_eq!(h.app.history.len(),alerts,"未处理只常驻，不再重复提醒");
    assert_eq!(h.app.last_reminder,water_start,"未确认不能推进饮水计时");
    assert_eq!(h.app.sit_due(),sit_due,"锁屏和等待不能推进久坐计时");
    assert!(badge(&h,"water").width()>10.0 && badge(&h,"sit").width()>10.0);
}

#[test]
fn health_badges_confirm_independently_and_do_not_record_water(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=due();
    click_badge(&mut h,"water");
    assert_eq!(h.app.water.drunk_ml,0,"点击常驻只确认，不记录喝水量");
    assert!((h.app.last_reminder-h.t).abs()<0.1);
    h.hover(H::local(280.0,80.0),1.0);
    click_badge(&mut h,"sit");
    assert_eq!(h.app.sat_minutes(),0);
    assert!((h.app.sit_due()-h.t-3600.0).abs()<0.1);
    let alerts=h.app.history.len();
    h.cursor=None;h.t+=44.0*60.0;h.frame(vec![]);assert_eq!(h.app.history.len(),alerts);
    h.t+=61.0;h.frame(vec![]);assert!(h.app.history.len()>alerts,"确认后再完整计时到点提醒");
}

#[test]
fn health_pending_survives_restart_and_day_change(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=due();
    h.app=App::new(&h.ctx,Options{fetch_lyrics:false,pet_idle:1e9});h.frame(vec![]);
    h.t+=24.0*3600.0;
    let env=Env{t:h.t,clock:"09:41:00".into(),today:"2026-10-29".into(),..Default::default()};
    let _=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(crate::app::CANVAS_W,WIN_H))),time:Some(h.t),..Default::default()},|ctx|{
        egui::CentralPanel::default().show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H)),&env);});
    });
    assert_eq!(h.app.health_pending,[true,true]);h.frame(vec![]);
    assert!(badge(&h,"water").width()>10.0 && badge(&h,"sit").width()>10.0,"重启和跨天保留待处理提醒");
    h.app.show_page(crate::app::WATER_PAGE);h.app.sit_mode=true;
    h.hover(H::local(280.0,80.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="待活动")));
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text().contains("已坐"))),"待处理不再显示持续增长的久坐分钟数");
}

#[test]
fn health_actions_clear_pending_and_keep_messenger_unread(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=due();
    h.app.settings.wechat_notifications=true;h.app.receive_wechat_reminder();h.app.alerts.clear();h.app.notice=None;
    h.app.show_page(crate::app::WATER_PAGE);h.hover(H::local(280.0,80.0),1.0);
    let water=badge(&h,"water");let sit=badge(&h,"sit");
    assert!((water.center().x-sit.center().x).abs()<0.1 && water.bottom()<=sit.top()+0.1,"展开时健康提醒沿右侧竖排");
    h.click_text("喝一口");assert_eq!(h.app.water.drunk_ml,250);
    click_badge(&mut h,"sit");
    assert_eq!(h.app.pending_messengers.len(),1,"处理健康提醒不影响微信未读");
    h.app=App::new(&h.ctx,Options{fetch_lyrics:false,pet_idle:1e9});h.frame(vec![]);
    assert_eq!(h.app.health_pending,[false,false],"处理结果已保存");
}

#[test]
fn health_notice_body_and_lyrics_badge_confirm_without_recording_water(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for lyrics in [false,true]{
        let mut h=H::new();h.frame(vec![]);
        if lyrics {
            h.app.settings.island_lyrics=true;
            h.media=crate::smtc::NowPlaying{source:"cloudmusic.exe".into(),title:"测试歌曲".into(),artist:"歌手".into(),playing:true,duration:200.0,..Default::default()};
        }
        h.app.alert("喝水时间到了","喝点水吧",2,false);for _ in 0..60{h.frame(vec![]);}
        if lyrics{h.click_notice_badge();}else{h.click_text("喝水时间到了");}
        assert!(!h.app.health_pending[0] && h.app.notice.is_none());
        assert_eq!(h.app.water.drunk_ml,0);
        assert!((h.app.last_reminder-h.t).abs()<0.1);
    }
}

#[test]
fn health_side_docks_keep_confirmation_independent(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for dock in [crate::model::Dock::Left,crate::model::Dock::Right]{
        let mut h=due();h.app.settings.dock=dock;h.app.show_page(crate::app::WATER_PAGE);
        h.hover(H::local(280.0,80.0),1.0);
        click_badge(&mut h,"water");assert!(h.app.health_pending[1]);
        click_badge(&mut h,"sit");assert_eq!(h.app.health_pending,[false,false]);
    }
}

#[test]
fn health_fullscreen_hidden_keeps_pending_silent_and_returns_as_badges(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.hide_fullscreen=true;h.app.settings.sound=true;h.fullscreen=true;h.frame(vec![]);
    h.app.settings.lunch_rest=false;h.app.settings.water_start=0;h.app.settings.water_end=0;
    let timers=(h.app.last_reminder,h.app.sit_start);
    h.t+=61.0*60.0;h.cmds.clear();h.frame(vec![]);
    assert_eq!(h.app.health_pending,[true,true],"全屏期间仍记住两项待处理提醒");
    assert!(h.app.notice.is_none() && h.app.alerts.is_empty(),"全屏期间不弹出健康提醒");
    assert!(h.app.health_badges().is_empty(),"全屏隐藏期间不显示健康常驻图标");
    assert!(!h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Notify(..))),"开启声音也不能发送系统通知或响铃");
    assert_eq!(crate::model::load_health_pending(),[true,true]);
    let count=h.app.history.len();h.t+=500.0*60.0;h.frame(vec![]);
    h.fullscreen=false;h.frame(vec![]);
    assert!(h.app.notice.is_none() && h.app.alerts.is_empty(),"退出全屏不补弹横幅");
    assert_eq!(h.app.history.len(),count,"全屏期间不重复提醒");
    assert_eq!((h.app.last_reminder,h.app.sit_start),timers,"全屏切换不能重启计时");
    assert!(badge(&h,"water").width()>10.0 && badge(&h,"sit").width()>10.0,"退出全屏显示常驻入口");
    click_badge(&mut h,"water");assert!(h.app.health_pending[1]);
}

#[test]
fn health_entering_fullscreen_clears_active_and_queued_popups_only(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.alert("喝水时间到了","喝点水吧",2,true);h.app.tick_alerts();
    h.app.hud_notice=h.app.notice.clone();
    h.app.alert(crate::app::SIT_TITLE,"起来走走",2,true);
    h.app.alert("待办到时间了","检查周报",4,true);
    h.fullscreen=true;h.cmds.clear();h.frame(vec![]);
    let health=|alert:&crate::extras::Alert|alert.health().is_some();
    assert!(!h.app.notice.as_ref().is_some_and(health) && !h.app.hud_notice.as_ref().is_some_and(health),"全屏隐藏时清除正在显示的健康横幅");
    assert!(!h.app.alerts.iter().any(health),"健康提醒不能留在弹窗队列里等退出后重播");
    assert!(!h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Notify(title,_)if title=="喝水时间到了" || title==crate::app::SIT_TITLE)),"尚未发出的健康系统通知也要抑制");
    assert!(h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Notify(title,_)if title=="待办到时间了")),"其他类型提醒沿用原有逻辑");
    assert_eq!(h.app.health_pending,[true,true]);
    h.fullscreen=false;h.frame(vec![]);
    assert!(!h.app.notice.as_ref().is_some_and(health));
    assert!(badge(&h,"water").width()>10.0 && badge(&h,"sit").width()>10.0);
}

#[test]
fn health_fullscreen_with_hide_disabled_keeps_normal_reminders(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.hide_fullscreen=false;h.fullscreen=true;h.frame(vec![]);
    h.app.settings.lunch_rest=false;h.app.settings.water_start=0;h.app.settings.water_end=0;
    h.t+=61.0*60.0;h.cmds.clear();h.frame(vec![]);
    assert!(h.app.notice.as_ref().is_some_and(|a|a.health().is_some()));
    assert!(h.cmds.iter().any(|cmd|matches!(cmd,crate::app::Cmd::Notify(..))),"关闭全屏隐藏时仍正常提醒");
}

#[test]
fn health_settings_support_direct_input_and_preserve_pending_reminders(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=due();
    h.app.open_settings_category(1);h.hover(H::local(300.0,60.0),1.0);
    let timers=(h.app.last_reminder,h.app.sit_start);
    for (label,input,expected) in [("45 分钟","37",37),("37 分钟","999",180),("180 分钟","0",5),("5 分钟","abc",5)]{
        h.cmds.clear();h.click_text(label);h.frame(vec![]);
        assert!(h.app.is_editing(),"健康时长应支持单击输入");
        assert!(h.cmds.contains(&crate::app::Cmd::Focus),"输入时应取得原生键盘焦点");
        h.frame(vec![egui::Event::Text(input.into())]);
        h.frame(vec![egui::Event::Key{key:egui::Key::Enter,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);h.frame(vec![]);
        assert_eq!(crate::model::Settings::load().water_minutes,expected);
        assert!(!h.app.is_editing());
    }
    h.click_text("5 分钟");h.frame(vec![]);h.frame(vec![egui::Event::Text("50".into())]);
    h.click(H::local(357.0,89.0));h.frame(vec![]);
    assert_eq!(crate::model::Settings::load().water_minutes,55,"加号应先保存手输值，再增加五分钟");
    h.click_text("55 分钟");h.frame(vec![]);h.frame(vec![egui::Event::Text("40".into())]);
    h.frame(vec![egui::Event::Key{key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);h.frame(vec![]);
    assert_eq!(h.app.settings.water_minutes,55,"Esc 取消输入");
    h.app.sit_mode=true;h.hover(H::local(300.0,60.0),0.8);
    h.click_text("60 分钟");h.frame(vec![]);h.frame(vec![egui::Event::Text("42".into())]);
    h.click(H::local(50.0,50.0));h.frame(vec![]);
    assert_eq!(crate::model::Settings::load().sit_minutes,42,"点击别处保存久坐时长");
    assert_eq!((h.app.last_reminder,h.app.sit_start),timers,"调整设置不能重启计时");
    assert_eq!(h.app.health_pending,[true,true],"调整设置不能清除待处理提醒");
}

#[test]
fn health_amounts_and_time_window_support_direct_input(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.frame(vec![]);
    h.app.open_settings_category(1);h.hover(H::local(300.0,60.0),1.0);
    for (label,input) in [("2.0 L","2.75"),("250 ml","300")]{
        h.click_text(label);h.frame(vec![]);assert!(h.app.is_editing());
        h.frame(vec![egui::Event::Text(input.into())]);
        h.frame(vec![egui::Event::Key{key:egui::Key::Enter,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);h.frame(vec![]);
    }
    assert_eq!(crate::model::Water::load("2026-10-28").goal_ml,2750);
    assert_eq!(crate::model::Settings::load().serving_ml,300);
    h.app.show_health_time();h.hover(H::local(300.0,55.0),0.5);
    for (label,input) in [("09:00","8"),("22:00","24:00"),("12:00","12:35")]{
        h.click_text(label);h.frame(vec![]);assert!(h.app.is_editing());
        h.frame(vec![egui::Event::Text(input.into())]);
        h.frame(vec![egui::Event::Key{key:egui::Key::Enter,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}]);h.frame(vec![]);
    }
    let saved=crate::model::Settings::load();
    assert_eq!((saved.water_start,saved.water_end,saved.lunch_start),(8,24,12*60+35));
}

#[test]
#[ignore="按需导出健康提醒常驻的实际界面"]
fn health_reminder_gallery(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_FEATURE_PREVIEW_DIR").expect("需指定预览目录"));std::fs::create_dir_all(&dest).unwrap();
    for dock in [crate::model::Dock::Top,crate::model::Dock::Left,crate::model::Dock::Right]{
        let mut h=due();h.app.settings.dock=dock;h.app.settings.pet=false;
        let side=dock.is_side();
        let (w,height)=if side{(crate::side_dock::SIDE_W,crate::side_dock::SIDE_H)}else{(crate::app::CANVAS_W,WIN_H)};
        let mut raster=feature_raster_sized(w,height);
        let away=Some(Pos2::new(if side{w/2.0}else{WIN_W/2.0},height-6.0));
        let near=if dock==crate::model::Dock::Left{Pos2::new(8.0,height/2.0)}else if dock==crate::model::Dock::Right{Pos2::new(w-8.0,height/2.0)}else{Pos2::new(WIN_W/2.0,12.0)};
        shoot_unread(&mut h,&mut raster,&dest,&format!("health-{}-collapsed",dock.key()),80,away);
        h.app.show_page(crate::app::WATER_PAGE);h.app.sit_mode=true;
        shoot_unread(&mut h,&mut raster,&dest,&format!("health-{}-sit",dock.key()),90,Some(near));
        h.app.sit_mode=false;
        shoot_unread(&mut h,&mut raster,&dest,&format!("health-{}-water",dock.key()),60,Some(near));
        h.app.settings.wechat_notifications=true;h.app.receive_wechat_reminder();h.app.notice=None;h.app.alerts.clear();
        shoot_unread(&mut h,&mut raster,&dest,&format!("health-{}-wechat",dock.key()),60,Some(near));
        if dock==crate::model::Dock::Top {
            h.app.water.goal_ml=2750;h.app.open_settings_category(1);
            shoot_unread(&mut h,&mut raster,&dest,"health-settings-water",60,Some(H::local(100.0,60.0)));
            h.app.sit_mode=true;
            shoot_unread(&mut h,&mut raster,&dest,"health-settings-sit",60,Some(H::local(100.0,60.0)));
            h.app.show_health_time();
            shoot_unread(&mut h,&mut raster,&dest,"health-settings-time",60,Some(H::local(100.0,60.0)));
        }
    }
}
