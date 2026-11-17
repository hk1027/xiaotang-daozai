//! 本机日程：独立保存，不需要连接工作账号。
use chrono::{Duration, Local, NaiveDate, NaiveTime, Timelike};
use crate::mail::Meeting;

#[derive(Clone)]
pub(crate) struct Entry { pub event: Meeting, pub reminder: Option<u32> }

pub(crate) struct Draft {
    pub uid: Option<String>, pub title: String, pub start_date: String, pub start_time: String,
    pub end_date: String, pub end_time: String, pub all_day: bool, pub reminder: Option<u32>,
    pub error: String, pub focused: bool, pub confirm_delete: bool,
}
impl Draft {
    pub fn new(day: NaiveDate) -> Self {
        let now=Local::now()+Duration::minutes(30);
        let start=if day==Local::now().date_naive(){now.naive_local().with_second(0).unwrap()}
            else{day.and_hms_opt(9,0,0).unwrap()};
        let end=start+Duration::hours(1);
        Self{uid:None,title:String::new(),start_date:start.format("%Y-%m-%d").to_string(),start_time:start.format("%H:%M").to_string(),
            end_date:end.format("%Y-%m-%d").to_string(),end_time:end.format("%H:%M").to_string(),all_day:false,reminder:Some(15),
            error:String::new(),focused:false,confirm_delete:false}
    }
    pub fn edit(entry:&Entry)->Self {
        let start=chrono::DateTime::from_timestamp(entry.event.start,0).unwrap().with_timezone(&Local);
        let end=chrono::DateTime::from_timestamp(entry.event.end.unwrap_or(entry.event.start+3600)-if entry.event.all_day{1}else{0},0).unwrap().with_timezone(&Local);
        Self{uid:Some(entry.event.uid.clone()),title:entry.event.title.clone(),start_date:start.format("%Y-%m-%d").to_string(),start_time:start.format("%H:%M").to_string(),
            end_date:end.format("%Y-%m-%d").to_string(),end_time:end.format("%H:%M").to_string(),all_day:entry.event.all_day,reminder:entry.reminder,
            error:String::new(),focused:false,confirm_delete:false}
    }
    pub fn entry(&self)->Result<Entry,String> {
        if self.title.trim().is_empty(){return Err("请填写日程名称".into());}
        let date=|text:&str|NaiveDate::parse_from_str(text.trim(),"%Y-%m-%d").map_err(|_|"日期格式：2026-11-03".to_string());
        let time=|text:&str|NaiveTime::parse_from_str(text.trim(),"%H:%M").map_err(|_|"时间格式：18:30".to_string());
        let start_day=date(&self.start_date)?;let end_day=date(&self.end_date)?;
        let (start,end)=if self.all_day {
            (start_day.and_hms_opt(0,0,0).unwrap(),end_day.succ_opt().ok_or("结束日期无效")?.and_hms_opt(0,0,0).unwrap())
        }else{(start_day.and_time(time(&self.start_time)?),end_day.and_time(time(&self.end_time)?))};
        let timestamp=|dt:chrono::NaiveDateTime|dt.and_local_timezone(Local).single().map(|dt|dt.timestamp()).ok_or("此时间不存在或有歧义，请调整时间".to_string());
        let (start,end)=(timestamp(start)?,timestamp(end)?);
        if end<=start{return Err("结束时间必须晚于开始时间".into());}
        let uid=self.uid.clone().unwrap_or_else(||format!("local-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        Ok(Entry{event:Meeting{uid,title:self.title.trim().into(),start,end:Some(end),all_day:self.all_day,
            at:chrono::DateTime::from_timestamp(start,0).unwrap().with_timezone(&Local).format("%m-%d %H:%M").to_string()},reminder:self.reminder})
    }
}
impl Entry {
    pub fn reminder_at(&self)->Option<i64> {
        let minutes=self.reminder?;
        if self.event.all_day {
            let date=chrono::DateTime::from_timestamp(self.event.start,0)?.with_timezone(&Local).date_naive();
            date.and_hms_opt(9,0,0)?.and_local_timezone(Local).earliest().map(|at|at.timestamp())
        }else{Some(self.event.start-minutes as i64*60)}
    }
}

pub(crate) fn decode(raw:&[u8])->Result<Vec<Entry>,String> {
    let rows:Vec<serde_json::Value>=serde_json::from_slice(raw).map_err(|_|"本机日程文件无法读取".to_string())?;
    rows.into_iter().map(|row|{
        let text=|key:&str|row.get(key).and_then(|v|v.as_str()).map(str::to_owned).ok_or("本机日程字段缺失".to_string());
        let start=row["start"].as_i64().ok_or("本机日程时间无效")?;
        let end=row["end"].as_i64().ok_or("本机日程时间无效")?;
        let dt=chrono::DateTime::from_timestamp(start,0).ok_or("本机日程时间无效")?;
        if end<=start || chrono::DateTime::from_timestamp(end,0).is_none(){return Err("本机日程结束时间无效".into());}
        Ok(Entry{event:Meeting{uid:text("uid")?,title:text("title")?,start,end:Some(end),all_day:row["all_day"].as_bool().unwrap_or(false),
            at:dt.with_timezone(&Local).format("%m-%d %H:%M").to_string()},reminder:row["reminder"].as_u64().and_then(|n|u32::try_from(n).ok())})
    }).collect()
}
pub(crate) fn load()->Result<Vec<Entry>,String> {
    match std::fs::read(crate::model::config_dir().join("local-calendar.json")) {
        Ok(raw)=>decode(&raw),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(vec![]),Err(_)=>Err("无法读取本机日程文件".into()),
    }
}
pub(crate) fn save(entries:&[Entry])->Result<(),String> {
    let rows:Vec<_>=entries.iter().map(|entry|serde_json::json!({"uid":entry.event.uid,"title":entry.event.title,"start":entry.event.start,
        "end":entry.event.end,"all_day":entry.event.all_day,"reminder":entry.reminder})).collect();
    let path=crate::model::config_dir().join("local-calendar.json");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|_|"无法创建日程目录")?;
    crate::model::atomic_write(&path,&serde_json::to_vec(&rows).map_err(|_|"日程序列化失败")?).map_err(|_|"日程保存失败，请检查磁盘和文件权限".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_calendar_rejects_invalid_dates_and_reversed_ranges(){
        let mut draft=Draft::new(NaiveDate::from_ymd_opt(2026,11,3).unwrap());
        draft.end_date="2026-11-03".into();
        assert!(draft.entry().is_err());draft.title="散步".into();
        draft.start_date="2026-02-29".into();assert!(draft.entry().is_err());
        draft.start_date="2026-11-03".into();draft.start_time="25:00".into();assert!(draft.entry().is_err());
        draft.start_time="18:00".into();draft.end_time="17:00".into();assert!(draft.entry().is_err());
        draft.end_date="2026-11-04".into();assert!(draft.entry().is_ok());
    }
    #[test]
    fn local_calendar_all_day_end_is_exclusive_and_reminds_at_nine(){
        let mut draft=Draft::new(NaiveDate::from_ymd_opt(2026,11,3).unwrap());draft.title="休假".into();draft.all_day=true;
        draft.start_date="2026-11-03".into();draft.end_date=draft.start_date.clone();
        let entry=draft.entry().unwrap();assert_eq!(entry.event.end.unwrap()-entry.event.start,86400);
        assert_eq!(entry.reminder_at().unwrap()-entry.event.start,9*3600);
        let edited=Draft::edit(&entry).entry().unwrap();assert_eq!(entry.event.end,edited.event.end);
    }
}
