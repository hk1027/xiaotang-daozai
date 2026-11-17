//! 仅使用合成事件验证办公软件提醒，不启动或操控实际客户端。
use super::*;
use crate::app::{Cmd,Messenger,Unread};
fn enable(app:&mut App){
    app.settings.wechat_notifications=true;app.settings.read_notifications=true;
    app.settings.wecom_notifications=true;app.settings.feishu_notifications=true;app.settings.dingtalk_notifications=true;
}
#[test]
fn qq_open_regression_foreground_switch_keeps_unrelated_reminders(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();
    h.app.settings.read_notifications=true;
    let [a,b]=[100,200].map(|pid|Unread{source:Messenger::Qq,pid,window:pid as isize});
    let unknown=Unread::of(Messenger::Qq);
    for item in [a,b,unknown]{
        h.app.receive_notification_from("QQ".into(),"QQ".into(),"群消息".into(),Some(item));
    }
    assert_eq!(h.app.pending_messengers,[a,b,unknown]);
    assert!(!h.app.confirm_messenger_open(a,None));
    assert!(!h.app.confirm_messenger_open(a,Some((Messenger::Qq,b.pid))));
    assert!(!h.app.confirm_messenger_open(unknown,Some((Messenger::Qq,b.pid))));
    assert!(!h.app.confirm_messenger_open(a,Some((Messenger::Wechat,a.pid))));
    assert_eq!(h.app.pending_messengers,[a,b,unknown]);
    assert!(h.app.confirm_messenger_open(a,Some((Messenger::Qq,a.pid))));
    assert_eq!(h.app.pending_messengers,[b,unknown]);
    h.app.discard_unread(unknown);
    assert_eq!(h.app.pending_messengers,[b]);
}
#[test]
fn office_settings_are_independent_default_off_and_survive_restart(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    for client in Messenger::OFFICE{assert!(!client.enabled(&h.app.settings));}
    h.app.settings.feishu_notifications=true;h.app.settings.dingtalk_notifications=true;h.app.settings.save();
    let saved=crate::model::Settings::load();
    assert!(!saved.wecom_notifications && saved.feishu_notifications && saved.dingtalk_notifications);
    h.app.settings=saved;h.app.settings.notifications=false;
    assert_ne!(crate::messenger::toast_suppression(&h.app.settings)&(1<<(Messenger::Feishu as u8)),0,"关闭总开关后也不能退回读取该客户端的系统通知正文");
    for client in Messenger::OFFICE{h.app.receive_messenger_reminder(Unread::of(client));}
    assert!(h.app.pending_messengers.is_empty());
}
#[test]
fn office_reminders_keep_instances_separate_and_unknown_accounts_require_explicit_dismissal(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for source in Messenger::OFFICE{
        let mut h=H::new();enable(&mut h.app);
        let accounts=[100,200,300].map(|pid|Unread{source,pid,window:pid as isize});
        for account in accounts{h.app.receive_messenger_reminder(account);}
        h.app.receive_messenger_reminder(Unread::of(source));
        assert_eq!(h.app.pending_messengers.len(),4);
        h.app.viewed_client(source,200);
        assert_eq!(h.app.pending_messengers,[accounts[0],accounts[2],Unread::of(source)]);
        h.app.discard_unread(accounts[0]);assert_eq!(h.app.pending_messengers,[accounts[2],Unread::of(source)]);
        h.app.discard_unread(accounts[2]);assert_eq!(h.app.pending_messengers,[Unread::of(source)]);
        h.app.reveal_messenger_notice(Unread::of(source));for _ in 0..45{h.frame(vec![]);}
        h.click_text("关闭");h.frame(vec![]);assert!(h.app.pending_messengers.is_empty());
    }
}
#[test]
fn office_taskbar_mode_deduplicates_system_toasts_without_disabling_the_existing_windows_mode(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.app.settings.read_notifications=true;
    for source in Messenger::OFFICE{
        let id=source.adapter().notification_id;
        h.app.receive_notification(source.name().into(),id.into(),"旧有系统通知".into());
    }
    assert_eq!(h.app.alerts.len(),3,"办公提醒默认关闭时，保留原有 Windows 通知");
    h.app.alerts.clear();enable(&mut h.app);
    for source in Messenger::OFFICE{
        h.app.receive_messenger_reminder(Unread{source,pid:100,window:100});
        h.app.receive_notification(source.name().into(),source.adapter().notification_id.into(),"系统重复通知".into());
    }
    assert_eq!(h.app.pending_messengers.len(),3);assert_eq!(h.app.alerts.len(),3);
    let mask=crate::messenger::toast_suppression(&h.app.settings);
    for source in Messenger::OFFICE{assert_ne!(mask&(1<<(source as u8)),0);}
    h.app.settings.feishu_notifications=false;
    assert_eq!(crate::messenger::toast_suppression(&h.app.settings)&(1<<(Messenger::Feishu as u8)),0);
    h.app.viewed_client(Messenger::Feishu,100);
    assert_eq!(h.app.pending_messengers.len(),2,"不同客户端即使进程号相同也必须隔离");
}
#[test]
fn five_client_icons_fit_collapsed_and_expand_to_their_own_click_targets(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();enable(&mut h.app);
    for source in Messenger::ALL{
        let unread=Unread{source,pid:100+source as u32,window:11+source as isize};
        if source==Messenger::Qq{h.app.receive_notification_from("QQ".into(),"QQ".into(),"新消息".into(),Some(unread));}
        else{h.app.receive_messenger_reminder(unread);}
    }
    h.app.alerts.clear();h.app.notice=None;h.app.notification_bell_until=0.0;
    for _ in 0..90{h.frame(vec![]);}
    let compact=h.ctx.read_response(egui::Id::new(("card",true,"messenger-stack"))).unwrap();
    assert!(compact.rect.width()>90.0,"五个来源不能仍只给两个图标留宽度");
    h.app.reveal_unread();for _ in 0..90{h.frame(vec![]);}
    let first=h.ctx.read_response(egui::Id::new(("card",true,"微信".to_string()))).unwrap();
    h.hover(first.rect.center(),0.1);for _ in 0..20{h.wheel(-50.0);}h.frame(vec![]);
    let target=h.ctx.read_response(egui::Id::new(("card",true,"钉钉".to_string()))).unwrap();
    h.click(target.rect.center());
    assert!(h.cmds.iter().any(|cmd|matches!(cmd,Cmd::ViewMessenger(unread) if unread.source==Messenger::DingTalk)));
}
#[test]
#[ignore="按需导出办公软件提醒的合成预览，不操作真实客户端"]
fn office_notice_gallery(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_OFFICE_PREVIEW").expect("需指定预览目录"));
    std::fs::create_dir_all(&dest).unwrap();
    for source in Messenger::OFFICE{
        let mut h=H::new();enable(&mut h.app);h.app.settings.pet=false;
        h.app.receive_messenger_reminder(Unread{source,pid:100,window:11});
        let mut raster=feature_raster();
        shoot_feature(&mut h,&mut raster,&dest,source.name(),90,None,"2026-11-10");
    }
}

#[test]
#[ignore="按需检查消息图标在深浅皮肤下的实际画面，不操作真实客户端"]
fn messenger_brand_icon_gallery(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_OFFICE_PREVIEW").expect("需指定预览目录"));
    std::fs::create_dir_all(&dest).unwrap();
    for skin in [crate::model::Skin::Classic,crate::model::Skin::GlassLight,crate::model::Skin::GlassDark]{
        let mut h=H::new();enable(&mut h.app);h.app.settings.pet=false;h.app.settings.skin=skin;
        h.app.receive_messenger_reminder(Unread{source:Messenger::DingTalk,pid:100,window:11});
        h.app.receive_notification_from("QQ".into(),"QQ".into(),"新消息".into(),Some(Unread{source:Messenger::Qq,pid:200,window:22}));
        h.app.receive_messenger_reminder(Unread{source:Messenger::Feishu,pid:300,window:33});
        h.app.alerts.clear();h.app.notice=None;h.app.notification_bell_until=0.0;
        let mut raster=feature_raster();
        shoot_unread(&mut h,&mut raster,&dest,skin.key(),90,Some(Pos2::new(WIN_W/2.0,12.0)));
        shoot_unread(&mut h,&mut raster,&dest,&format!("{}-collapsed",skin.key()),90,Some(Pos2::new(0.0,WIN_H-10.0)));
    }
}

#[test]
fn taskbar_duplicate_flashes_do_not_restart_the_unread_alert(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for source in Messenger::ALL.into_iter().filter(|source|source.adapter().capture==crate::messenger::Capture::Taskbar){
        let mut h=H::new();enable(&mut h.app);
        let unread=Unread{source,pid:100,window:11};
        h.app.receive_messenger_reminder(unread);
        let at=h.app.alerts.back().unwrap().at;
        h.app.t+=10.0;
        h.app.receive_messenger_reminder(unread);
        assert_eq!(h.app.alerts.back().unwrap().at,at,"{} 未查看期间，重复闪动不能重新弹出或延长横幅",source.name());
        h.app.discard_unread(unread);h.app.t+=0.1;
        h.app.receive_messenger_reminder(unread);
        assert_eq!(h.app.alerts.back().unwrap().at,h.app.t,"明确关闭后，下一条提醒立即生效");
    }
}

#[test]
fn qq_text_matching_an_island_reminder_is_not_a_self_notification_echo(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.read_notifications=true;
    h.app.alert("喝水时间到了","喝点水吧",2,true);
    h.app.receive_notification("QQ".into(),"QQ".into(),"喝水时间到了 · 喝点水吧".into());
    assert_eq!(h.app.pending_messengers,[Unread::of(Messenger::Qq)],"QQ 的真实消息不能只因文字相同就被当成岛仔通知回传");
}

#[test]
fn qq_old_message_banners_never_rebind_to_a_new_window(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    h.app.settings.read_notifications=true;h.app.settings.notification_preview=true;
    let old=Unread{source:Messenger::Qq,pid:100,window:11};let new=Unread{window:23,..old};
    h.app.receive_notification_from("QQ".into(),"QQ".into(),"旧窗口消息".into(),Some(old));h.frame(vec![]);
    h.app.receive_notification_from("QQ".into(),"QQ".into(),"新窗口消息".into(),Some(new));
    assert!(h.app.alerts.iter().chain(h.app.notice.iter()).all(|alert|alert.body!="旧窗口消息"),"QQ 旧消息横幅不能指向后来的新窗口");
    assert!(h.app.alerts.iter().chain(h.app.notice.iter()).any(|alert|alert.body=="新窗口消息" && alert.unread==Some(new)));
}
