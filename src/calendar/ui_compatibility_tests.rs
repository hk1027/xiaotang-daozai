use super::*;
use crate::calendar::{Config,Event,Provider};

#[test]
fn ics_import_retains_good_records_and_distinguishes_duplicates_and_bad_records(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let at=(chrono::Utc::now()+chrono::Duration::days(1)).format("%Y%m%dT%H%M%SZ").to_string();
    let ics=format!("BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:{at}\nSUMMARY:同一时间的面试甲\nEND:VEVENT\nBEGIN:VEVENT\nDTSTART:{at}\nSUMMARY:同一时间的面试乙\nEND:VEVENT\nBEGIN:VEVENT\nUID:bad\nDTSTART;TZID=Unsupported/Zone:20261112T100000\nEND:VEVENT\nBEGIN:VEVENT\nUID:old\nDTSTART:20000101T000000Z\nEND:VEVENT\nBEGIN:VEVENT\nUID:future\nDTSTART:21000101T000000Z\nEND:VEVENT\nBEGIN:VTODO\nSUMMARY:待办\nEND:VTODO\nEND:VCALENDAR");
    let files=vec![("mock.ics".into(),ics)];let note=h.app.import_ics(&files);
    assert_eq!(h.app.calendar.local.len(),2,"缺少 UID 的同一时刻不同日程不能互相覆盖");
    for phrase in ["已导入 2 个","1 条日程无法解析","2 个超出导入范围","1 条待办"]{assert!(note.contains(phrase),"{note}");}
    let ids=h.app.calendar.local.iter().map(|entry|entry.event.uid.clone()).collect::<Vec<_>>();assert_ne!(ids[0],ids[1]);
    let note=h.app.import_ics(&files);assert_eq!(h.app.calendar.local.len(),2);assert!(note.contains("2 个已存在已跳过"));
    let legacy=format!("ics--{}",h.app.calendar.local[0].event.start);h.app.calendar.local[0].event.uid=legacy.clone();
    h.app.import_ics(&files);assert_eq!(h.app.calendar.local.len(),2);assert_eq!(h.app.calendar.local[0].event.uid,legacy,"已有本机记录应原样保留");
}

#[test]
fn ics_import_reports_the_capacity_limit_without_calling_new_records_duplicates(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let at=(chrono::Utc::now()+chrono::Duration::days(1)).format("%Y%m%dT%H%M%SZ").to_string();
    let first=format!("BEGIN:VEVENT\nUID:existing\nDTSTART:{at}\nEND:VEVENT");h.app.import_ics(&[("first.ics".into(),first)]);
    let entry=h.app.calendar.local[0].clone();h.app.calendar.local=(0..5000).map(|i|{let mut entry=entry.clone();entry.event.uid=format!("existing-{i}");entry}).collect();
    let new=format!("BEGIN:VEVENT\nUID:new\nDTSTART:{at}\nEND:VEVENT");let note=h.app.import_ics(&[("new.ics".into(),new)]);
    assert_eq!(h.app.calendar.local.len(),5000);assert!(note.contains("5000"));assert!(note.contains("1 个未导入"));assert!(!note.contains("已存在"));
}

#[test]
fn calendar_partial_warnings_are_scoped_and_clear_after_a_complete_refresh(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();
    let config=Config{provider:Provider::DingTalk,username:"mock".into(),href:None,server:None};
    h.app.calendar.config=Some(config.clone());let month=h.app.calendar.view.month;
    h.app.calendar_event(Event::MonthLoaded{config:config.clone(),month,result:Ok(vec![])});
    let message="1 处日历读取失败；读取日程失败（403）".to_string();
    h.app.calendar_event(Event::Warning{config:config.clone(),month:Some(month),message:message.clone()});
    assert_eq!(h.app.calendar.view.warning,message);assert!(h.app.calendar.view.error.is_empty());
    let other=Config{provider:Provider::Feishu,..config.clone()};
    h.app.calendar_event(Event::Warning{config:other,month:Some(month),message:"其他账号".into()});
    h.app.calendar_event(Event::Warning{config:config.clone(),month:Some(month+chrono::Duration::days(32)),message:"旧月份".into()});
    assert_eq!(h.app.calendar.view.warning,message);
    h.app.show_page(crate::app::CALENDAR_PAGE);h.hover(H::local(200.0,12.0),1.0);
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="部分同步")));
    assert!(h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.text()=="部分日程未同步")));
    for shape in &h.shapes{if let egui::Shape::Text(text)=&shape.shape{if ["部分同步","部分日程未同步"].contains(&text.galley.text()){assert!(shape.clip_rect.contains_rect(text.visual_bounding_rect()));}}}
    h.app.calendar_event(Event::MonthLoaded{config:config.clone(),month,result:Ok(vec![])});
    h.app.calendar_event(Event::Warning{config,month:Some(month),message:String::new()});assert!(h.app.calendar.view.warning.is_empty());
}

#[test]
#[ignore="按需导出日历部分同步提示的实际样式"]
fn calendar_partial_sync_preview(){
    let _guard=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let destination=std::path::PathBuf::from(std::env::var_os("DAOZAI_CALENDAR_PREVIEW").expect("需指定预览目录"));std::fs::create_dir_all(&destination).unwrap();
    for empty in [false,true]{
        let mut h=H::new();let config=Config{provider:Provider::DingTalk,username:"mock".into(),href:None,server:None};
        h.app.calendar.config=Some(config.clone());let month=h.app.calendar.view.month;
        let start=(chrono::Local::now()+chrono::Duration::minutes(30)).timestamp();
        let events=if empty{vec![]}else{vec![crate::mail::Meeting{uid:"preview".into(),title:"面试安排".into(),start,end:Some(start+1800),all_day:false,at:"面试时间".into()}]};
        h.app.calendar_event(Event::MonthLoaded{config:config.clone(),month,result:Ok(events)});
        h.app.calendar_event(Event::Warning{config,month:Some(month),message:"1 处日历读取失败；日历服务器拒绝访问（403）".into()});h.app.show_page(crate::app::CALENDAR_PAGE);
        let mut raster=feature_raster();let day=chrono::Local::now().date_naive().to_string();
        shoot_feature(&mut h,&mut raster,&destination,if empty{"calendar-partial-empty"}else{"calendar-partial-events"},100,Some(H::local(200.0,12.0)),&day);
    }
}
