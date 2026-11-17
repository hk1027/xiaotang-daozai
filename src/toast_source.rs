//! 仅在通知缺少应用标识时，按编号和创建时间核对 Windows 记录的发送方；不查询通知正文。
use std::{ffi::{c_char,c_int,c_void,CStr,CString},path::Path,ptr};
type RowCallback=unsafe extern "C" fn(*mut c_void,c_int,*mut *mut c_char,*mut *mut c_char)->c_int;
#[link(name="winsqlite3")]
extern "C" {
    fn sqlite3_open_v2(name:*const c_char,db:*mut *mut c_void,flags:c_int,vfs:*const c_char)->c_int;
    fn sqlite3_exec(db:*mut c_void,sql:*const c_char,callback:Option<RowCallback>,data:*mut c_void,error:*mut *mut c_char)->c_int;
    fn sqlite3_close(db:*mut c_void)->c_int;
}
pub(super) fn sender(id:u32,created:i64)->Option<String>{
    let local=std::env::var_os("LOCALAPPDATA")?;
    sender_in(&Path::new(&local).join("Microsoft/Windows/Notifications/wpndatabase.db"),id,created)
}
fn sender_in(path:&Path,id:u32,created:i64)->Option<String>{
    let name=CString::new(path.to_string_lossy().as_bytes()).ok()?;
    // 参数均为数值，创建时间必须同时匹配，避免通知编号复用时混淆应用。
    let sql=CString::new(format!("SELECT h.PrimaryId FROM Notification n JOIN NotificationHandler h ON h.RecordId=n.HandlerId WHERE n.Id={id} AND n.ArrivalTime={created} AND n.Type='toast' LIMIT 1")).ok()?;
    unsafe extern "C" fn found(data:*mut c_void,count:c_int,values:*mut *mut c_char,_:*mut *mut c_char)->c_int{
        if count>0 && !values.is_null(){unsafe{if !(*values).is_null(){*data.cast::<Option<String>>()=Some(CStr::from_ptr(*values).to_string_lossy().into_owned());}}}0
    }
    let mut db=ptr::null_mut();let mut sender=None;
    unsafe{
        let opened=sqlite3_open_v2(name.as_ptr(),&mut db,1,ptr::null())==0;
        let ok=opened && sqlite3_exec(db,sql.as_ptr(),Some(found),(&mut sender as *mut Option<String>).cast(),ptr::null_mut())==0;
        if !db.is_null(){sqlite3_close(db);}
        if ok{sender}else{None}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sender_lookup_matches_creation_time_and_never_modifies_the_database(){
        let path=std::env::temp_dir().join(format!("daozai-toast-source-{}.db",std::process::id()));
        let name=CString::new(path.to_string_lossy().as_bytes()).unwrap();let mut db=ptr::null_mut();
        let sql=CString::new("CREATE TABLE NotificationHandler (RecordId INTEGER, PrimaryId TEXT); CREATE TABLE Notification (Id INTEGER, ArrivalTime INTEGER, HandlerId INTEGER, Type TEXT); INSERT INTO NotificationHandler VALUES (1,'QQ'),(2,'OtherApp'); INSERT INTO Notification VALUES (10,100,1,'toast'),(11,101,2,'toast'),(12,102,1,'badge');").unwrap();
        unsafe{
            assert_eq!(sqlite3_open_v2(name.as_ptr(),&mut db,6,ptr::null()),0);
            let result=sqlite3_exec(db,sql.as_ptr(),None,ptr::null_mut(),ptr::null_mut());
            sqlite3_close(db);assert_eq!(result,0);
        }
        let before=std::fs::read(&path).unwrap();
        let found=[sender_in(&path,10,100),sender_in(&path,11,101),sender_in(&path,10,101),sender_in(&path,12,102)];
        let after=std::fs::read(&path).unwrap();std::fs::remove_file(&path).unwrap();
        assert_eq!(found,[Some("QQ".into()),Some("OtherApp".into()),None,None]);assert_eq!(before,after);
        assert_eq!(sender_in(&path,10,100),None);assert!(!path.exists(),"只读查询不能新建数据库");
    }
    #[test]
    #[ignore="需要本机通知中心留有指定测试通知"]
    fn qq_notification_local_identity(){
        let id=std::env::var("DAOZAI_QQ_TEST_ID").unwrap().parse().unwrap();
        use windows::UI::Notifications::{Management::UserNotificationListener,NotificationKinds};
        use windows::Win32::System::WinRT::{RoInitialize,RoUninitialize,RO_INIT_MULTITHREADED};
        unsafe{RoInitialize(RO_INIT_MULTITHREADED).unwrap();}
        let items=UserNotificationListener::Current().unwrap().GetNotificationsAsync(NotificationKinds::Toast).unwrap().get().unwrap();
        let n=(0..items.Size().unwrap()).filter_map(|i|items.GetAt(i).ok()).find(|n|n.Id().ok()==Some(id)).unwrap();
        let created=n.CreationTime().unwrap().UniversalTime;
        assert_eq!(super::super::notification_identity(&n,id,created,None),("QQ".into(),"QQ".into()));
        assert_eq!(sender(id,created).as_deref(),Some("QQ"));assert!(sender(id,created+1).is_none());
        drop(n);drop(items);unsafe{RoUninitialize();}
    }
}
