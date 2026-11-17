//! QQ 的安装位置、主窗口和托盘规则。
use super::{Messenger,OpenError,Target,Unread};
use super::windows::*;
use windows::{core::{w,PCWSTR},Win32::{Foundation::{HWND,LPARAM,WPARAM},System::{Registry::{HKEY_CLASSES_ROOT,HKEY_CURRENT_USER},Threading::{OpenProcess,PROCESS_QUERY_LIMITED_INFORMATION}},UI::WindowsAndMessaging::*}};
use std::path::PathBuf;

pub(crate) const ADAPTER:NativeAdapter=NativeAdapter{paths,main_frame,accounts:Some(accounts),ready,strict_window:true,open,taskbar:None};
fn main_frame(class:&str,style:u32,extended:u32)->bool{matches!(class,"Chrome_WidgetWin_1"|"TXGuiFoundation") && frame_style(class,style,extended)}
fn open(unread:Unread,paths:&[String],_:bool)->Result<(),OpenError>{unsafe{
    let window=HWND(unread.window as *mut _);
    if unread.window==0 || !IsWindow(window).as_bool() || window_pid(window)!=unread.pid{wake_step("提醒窗口已不属于该账号");return Err(OpenError::Unavailable);}
    let target=if usable_main_frame(window,unread.source) && matches(window,paths){Some(window)}else{visible_main_window_of(unread.pid,unread.source,paths)};
    if let Some(target)=target{
        wake_step(format!("切前台 {}",describe_window(target)));
        if IsIconic(target).as_bool(){let _=ShowWindowAsync(target,SW_RESTORE);}
        if SetForegroundWindow(target).as_bool(){wake_step("系统已接受");return Ok(());}
        wake_step("系统拒绝切前台");
    }else{wake_step("没有可见的 QQ 主窗口");}
    if class_name(window)=="Electron_NotifyIconHostWindow" && matches(window,paths){
        let icons=host_icons(&[(unread.pid,window)],unread.pid);
        if let [(_,id)]=icons.as_slice(){
            wake_step(format!("点击 QQ 托盘图标 {id}"));
            for event in [WM_LBUTTONDOWN,WM_LBUTTONUP,WM_LBUTTONDBLCLK,WM_LBUTTONUP]{
                PostMessageW(window,WM_APP+1,WPARAM(*id as usize),LPARAM(event as isize)).map_err(|_|{wake_step("托盘消息发送失败");OpenError::Failed("QQ 托盘暂时无法响应".into())})?;
            }
            return Ok(());
        }
        wake_step(format!("托盘宿主窗口里有 {} 个该账号的图标，无法确定点哪个",icons.len()));
    }else{wake_step(format!("提醒窗口不是 QQ 托盘宿主（{}）",class_name(window)));}
    Err(OpenError::Unavailable)
}}

fn paths()->Vec<String>{
    use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    let mut candidates=Vec::new();
    for root in [HKEY_CURRENT_USER,HKEY_LOCAL_MACHINE]{
        for key in [w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\QQ"),w!("Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\QQ")]{
            if let Some(icon)=registry_string(root,key,w!("DisplayIcon")){
                let exe=icon.rsplit_once(',').filter(|(_,index)|index.trim().parse::<i32>().is_ok()).map_or(icon.as_str(),|(path,_)|path);
                candidates.push(PathBuf::from(exe.trim().trim_matches('"')));
            }
        }
    }
    // QQ NT 的协议处理器在版本子目录中，主程序在安装根目录。
    if let Some(command)=registry_string(HKEY_CLASSES_ROOT,w!("tencent\\shell\\open\\command"),PCWSTR::null()){
        let helper=command.trim().trim_start_matches('"').split('"').next().unwrap_or("");
        let helper=std::path::Path::new(helper);
        if helper.is_absolute() && helper.file_name().is_some_and(|n|n.eq_ignore_ascii_case("timwp.exe")){
            if let Some(versions)=helper.ancestors().find(|p|p.file_name().is_some_and(|n|n.eq_ignore_ascii_case("versions"))){
                if let Some(root)=versions.parent(){candidates.push(root.join("QQ.exe"));}
            }
        }
    }
    let mut paths:Vec<String>=candidates.into_iter().filter(|p|p.is_absolute() && p.file_name().is_some_and(|n|n.eq_ignore_ascii_case("QQ.exe")) && p.is_file())
        .filter_map(|p|p.canonicalize().ok()).map(|p|normalized(&p.to_string_lossy())).collect();
    paths.push("*qq.exe".into());
    paths
}

fn ready(target:Target)->bool{ready_with_paths(target,&client_paths(Messenger::Qq))}
fn ready_with_paths(target:Target,paths:&[String])->bool{
    accounts(paths).is_some_and(|accounts|{
        let mine=accounts.iter().filter(|account|account.unread.pid==target.unread.pid).collect::<Vec<_>>();
        matches!(mine.as_slice(),[one] if **one==target)
    })
}

pub(crate) fn accounts(paths:&[String])->Option<Vec<Target>>{unsafe{
    use windows::Win32::Foundation::BOOL;
    struct Search<'a>{paths:&'a [String],hosts:Vec<HWND>,mains:Vec<HWND>}
    unsafe extern "system" fn visit(hwnd:HWND,data:LPARAM)->BOOL{
        let search=&mut *(data.0 as *mut Search);
        let class=class_name(hwnd);
        let tray=class=="Electron_NotifyIconHostWindow";
        let main=client_main_frame(Messenger::Qq,&class,GetWindowLongW(hwnd,GWL_STYLE)as u32,GetWindowLongW(hwnd,GWL_EXSTYLE)as u32)
            && GetWindow(hwnd,GW_OWNER).is_err();
        if (tray || main) && matches(hwnd,search.paths){if tray{search.hosts.push(hwnd);}else{search.mains.push(hwnd);}}
        true.into()
    }
    let mut search=Search{paths,hosts:Vec::new(),mains:Vec::new()};
    EnumWindows(Some(visit),LPARAM((&mut search as *mut Search)as isize)).ok()?;
    if search.hosts.is_empty() && search.mains.is_empty(){return Some(Vec::new());}
    for &host in &search.hosts{if host_icons(&[(window_pid(host),host)],window_pid(host)).len()!=1{return None;}}
    let owners:Vec<_>=search.hosts.iter().map(|&hwnd|window_pid(hwnd)).collect();
    let parents=parent_processes();let mut windows=search.hosts;let mut main_pids=std::collections::HashSet::new();
    // 已由唯一托盘图标确认的账号，其主窗口和隐藏辅助窗口不再重复计数。
    for main in search.mains{
        let pid=window_pid(main);
        if account_tray_pid(pid,&owners,&parents).is_none(){
            // 有相关宿主却无法唯一归属时，不把公共主进程再猜成一个账号。
            if owners.iter().any(|owner|ancestors(*owner,&parents).contains(&pid)){return None;}
            if !main_pids.insert(pid){return None;}
            if !windows.iter().any(|&hwnd|window_pid(hwnd)==pid){windows.push(main);}
        }
    }
    windows.into_iter().map(|hwnd|{
        let pid=window_pid(hwnd);
        let process=windows::core::Owned::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid).ok()?);
        Some(Target{unread:Unread{source:Messenger::Qq,pid,window:hwnd.0 as isize},started:process_started(*process)?})
    }).collect()
}}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn qq_wake_records_why_neither_window_nor_tray_could_be_used(){unsafe{
        let window=CreateWindowExW(WINDOW_EX_STYLE(0),w!("STATIC"),w!(""),WS_POPUP,0,0,10,10,None,None,None,None).unwrap();
        let paths=[normalized(&std::env::current_exe().unwrap().to_string_lossy())];
        take_wake_steps();
        let result=open(Unread{source:Messenger::Qq,pid:std::process::id(),window:window.0 as isize},&paths,true);
        let steps=take_wake_steps();
        let _=DestroyWindow(window);
        assert_eq!(result,Err(OpenError::Unavailable));
        assert_eq!(steps,["没有可见的 QQ 主窗口".to_string(),"提醒窗口不是 QQ 托盘宿主（Static）".to_string()],"依次记录切前台和托盘两条路为什么都走不通");
        take_wake_steps();
        let gone=open(Unread{source:Messenger::Qq,pid:std::process::id(),window:0},&paths,true);
        assert_eq!((gone,take_wake_steps()),(Err(OpenError::Unavailable),vec!["提醒窗口已不属于该账号".to_string()]));
    }}
    #[test]
    fn qq_single_tray_account_ignores_its_auxiliary_main_windows(){unsafe{
        use windows::Win32::{Foundation::LRESULT,System::LibraryLoader::GetModuleHandleW,UI::Shell::*};
        unsafe extern "system" fn proc(h:HWND,m:u32,w:WPARAM,l:LPARAM)->LRESULT{DefWindowProcW(h,m,w,l)}
        struct Fixture{windows:Vec<HWND>,icon:NOTIFYICONDATAW}
        impl Drop for Fixture{fn drop(&mut self){unsafe{let _=Shell_NotifyIconW(NIM_DELETE,&self.icon);for &window in &self.windows{let _=DestroyWindow(window);}}}}
        let instance=GetModuleHandleW(None).unwrap().into();
        for class in [w!("Chrome_WidgetWin_1"),w!("Electron_NotifyIconHostWindow")]{RegisterClassW(&WNDCLASSW{lpfnWndProc:Some(proc),hInstance:instance,lpszClassName:class,..Default::default()});}
        let host=CreateWindowExW(WINDOW_EX_STYLE(0),w!("Electron_NotifyIconHostWindow"),w!(""),WS_POPUP,-10000,-10000,0,0,None,None,instance,None).unwrap();
        let mut fixture=Fixture{windows:vec![host],icon:NOTIFYICONDATAW{cbSize:std::mem::size_of::<NOTIFYICONDATAW>() as u32,hWnd:host,uID:3,uFlags:NIF_ICON|NIF_MESSAGE,hIcon:LoadIconW(None,IDI_APPLICATION).unwrap(),uCallbackMessage:WM_APP+1,..Default::default()}};
        assert!(Shell_NotifyIconW(NIM_ADD,&fixture.icon).as_bool());
        for (style,width,height) in [(0x04c70000,800,600),(0x04c20000,280,44),(WS_OVERLAPPEDWINDOW.0,400,500)]{
            fixture.windows.push(CreateWindowExW(WINDOW_EX_STYLE(0),w!("Chrome_WidgetWin_1"),w!(""),WINDOW_STYLE(style),-10000,-10000,width,height,None,None,instance,None).unwrap());
        }
        let pid=std::process::id();let paths=[normalized(&std::env::current_exe().unwrap().to_string_lossy())];
        assert_eq!(host_icons(&[(pid,host)],pid),vec![(host,3)]);
        let found=accounts(&paths).expect("单一托盘账号不能因多个隐藏辅助窗口而识别失败");
        assert_eq!(found.len(),1);assert_eq!(found[0].unread.window,host.0 as isize);
        assert!(ready_with_paths(found[0],&paths));
        let mut history=super::super::AccountHistory::new(Messenger::Qq);
        history.observe(10,Some(found.clone()));history.observe(20,Some(found));
        assert_eq!(history.target(1,15,|target|ready_with_paths(target,&paths)).unwrap().unread.window,host.0 as isize);
        let mut second=fixture.icon;second.uID=4;
        assert!(Shell_NotifyIconW(NIM_ADD,&second).as_bool());
        let ambiguous=accounts(&paths);let _=Shell_NotifyIconW(NIM_DELETE,&second);
        assert!(ambiguous.is_none(),"同宿主多个账号图标仍不能假装只有一个账号");
    }}
    #[test]
    fn qq_multiple_main_windows_in_one_process_are_not_a_unique_account(){unsafe{
        use windows::{core::w,Win32::{Foundation::LRESULT,System::LibraryLoader::GetModuleHandleW}};
        unsafe extern "system" fn proc(h:HWND,m:u32,w:WPARAM,l:LPARAM)->LRESULT{DefWindowProcW(h,m,w,l)}
        let instance=GetModuleHandleW(None).unwrap().into();let class=w!("TXGuiFoundation");
        RegisterClassW(&WNDCLASSW{lpfnWndProc:Some(proc),hInstance:instance,lpszClassName:class,..Default::default()});
        let create=||CreateWindowExW(WINDOW_EX_STYLE(0),class,w!("Daozai synthetic QQ"),WS_OVERLAPPEDWINDOW,-10000,-10000,400,500,None,None,instance,None).unwrap();
        let paths=[normalized(&std::env::current_exe().unwrap().to_string_lossy())];
        let first=create();let one=accounts(&paths);let original=one.as_ref().unwrap()[0];let before=ready_with_paths(original,&paths);let second=create();let ambiguous=accounts(&paths);let after=ready_with_paths(original,&paths);
        DestroyWindow(first).unwrap();DestroyWindow(second).unwrap();
        assert_eq!(one.unwrap().len(),1);assert!(before);assert!(!after,"已经绑定的 QQ 目标在出现同进程多主窗口后也不能继续猜测唤起");
        assert!(ambiguous.is_none(),"同进程多个 QQ 主窗口不能随便拿第一个当成唯一账号");
    }}
    #[test]
    #[ignore="需要本机正在运行 QQ，仅用于现场诊断"]
    fn live_probe(){
        unsafe{
            use windows::Win32::Foundation::BOOL;
            let paths=paths();println!("paths={paths:?}");
            struct S<'a>{paths:&'a [String],hosts:Vec<HWND>,mains:Vec<HWND>}
            unsafe extern "system" fn visit(hwnd:HWND,data:LPARAM)->BOOL{
                let search=&mut *(data.0 as *mut S);
                let class=class_name(hwnd);
                let tray=class=="Electron_NotifyIconHostWindow";
                let main=client_main_frame(Messenger::Qq,&class,GetWindowLongW(hwnd,GWL_STYLE)as u32,GetWindowLongW(hwnd,GWL_EXSTYLE)as u32) && GetWindow(hwnd,GW_OWNER).is_err();
                if (tray||main) && matches(hwnd,search.paths){
                    println!("候选 {} {:#x} pid {} 样式 0x{:08X}/0x{:08X}",if tray{"托盘宿主"}else{"主窗口"},hwnd.0 as usize,window_pid(hwnd),GetWindowLongW(hwnd,GWL_STYLE)as u32,GetWindowLongW(hwnd,GWL_EXSTYLE)as u32);
                    if tray{search.hosts.push(hwnd);}else{search.mains.push(hwnd);}
                }
                true.into()
            }
            let mut search=S{paths:&paths,hosts:Vec::new(),mains:Vec::new()};
            let _=EnumWindows(Some(visit),LPARAM((&mut search as *mut S)as isize));
            for &host in &search.hosts{
                let icons=host_icons(&[(window_pid(host),host)],window_pid(host));
                println!("宿主 {:#x} pid {} 图标 {:?}",host.0 as usize,window_pid(host),icons);
            }
            println!("accounts = {:?}",accounts(&paths));
            println!("parent_processes 数量 {}",parent_processes().len());
        }
    }

}
