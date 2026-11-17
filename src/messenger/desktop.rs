//! 办公客户端只监听任务栏事件；唤起时仅处理已确认的原窗口。
use super::{Messenger,OpenError,Target,Unread};
use super::windows::*;
use windows::{core::Owned,Win32::{Foundation::{BOOL,HWND,LPARAM},System::Threading::{OpenProcess,PROCESS_QUERY_LIMITED_INFORMATION},UI::WindowsAndMessaging::*}};

pub(crate) const WECOM:NativeAdapter=NativeAdapter{
    paths:||paths(Messenger::WeCom),main_frame:|class,style,extended|main_frame(Messenger::WeCom,class,style,extended),
    accounts:None,ready,strict_window:true,open,taskbar:Some(|ctx|start(Messenger::WeCom,ctx)),
};
pub(crate) const FEISHU:NativeAdapter=NativeAdapter{
    paths:||paths(Messenger::Feishu),main_frame:|class,style,extended|main_frame(Messenger::Feishu,class,style,extended),
    accounts:None,ready,strict_window:true,open,taskbar:Some(|ctx|start(Messenger::Feishu,ctx)),
};
pub(crate) const DINGTALK:NativeAdapter=NativeAdapter{
    paths:||paths(Messenger::DingTalk),main_frame:|class,style,extended|main_frame(Messenger::DingTalk,class,style,extended),
    accounts:None,ready,strict_window:true,open,taskbar:Some(|ctx|start(Messenger::DingTalk,ctx)),
};
fn paths(client:Messenger)->Vec<String>{client.adapter().process_names.iter().map(|name|format!("*{}",name.to_ascii_lowercase())).collect()}
fn start(client:Messenger,ctx:eframe::egui::Context)->Result<Box<dyn TaskbarSource>,String>{
    Ok(Box::new(crate::wechat::Listener::for_client(client,ctx)?))
}
fn main_frame(client:Messenger,class:&str,style:u32,extended:u32)->bool{
    if style&WS_CHILD.0!=0 || extended&(WS_EX_TOOLWINDOW|WS_EX_NOACTIVATE).0!=0{return false;}
    match client{
        Messenger::WeCom=>class=="WeWorkWindow",
        Messenger::Feishu=>class=="Chrome_WidgetWin_1" && frame_style(class,style,extended),
        Messenger::DingTalk=>(class=="StandardFrame_DingTalk" || class.starts_with("Qt") && class.ends_with("QWindowIcon")) && frame_style(class,style,extended),
        _=>false,
    }
}
pub(crate) unsafe fn main_window(window:HWND,client:Messenger)->bool{
    use windows::Win32::Foundation::RECT;
    if !IsWindow(window).as_bool() || GetWindow(window,GW_OWNER).is_ok()
        || !client_main_frame(client,&class_name(window),GetWindowLongW(window,GWL_STYLE)as u32,GetWindowLongW(window,GWL_EXSTYLE)as u32){return false;}
    if client==Messenger::DingTalk && class_name(window)!=DINGTALK_FRAME && has_dingtalk_frame(window_pid(window)){return false;}
    if client==Messenger::Feishu && !feishu_title(&window_title(window)) && feishu_main_frame(window_pid(window)).is_some(){return false;}
    if IsIconic(window).as_bool(){return true;}
    let mut rect=RECT::default();
    GetWindowRect(window,&mut rect).is_ok() && rect.right-rect.left>=300 && rect.bottom-rect.top>=300
}
const DINGTALK_FRAME:&str="StandardFrame_DingTalk";
/// 新版钉钉的主窗口是 StandardFrame_DingTalk；同进程里的 Qt 窗口只是对话框（常为隐藏状态），
/// 不能算作第二个主窗口。主窗口本身隐藏到托盘时仍然认它。只有旧版没有该窗口时才按 Qt 主窗口识别。
unsafe fn has_dingtalk_frame(pid:u32)->bool{
    struct Search{pid:u32,found:bool}
    unsafe extern "system" fn visit(window:HWND,data:LPARAM)->BOOL{
        let search=&mut *(data.0 as *mut Search);
        if window_pid(window)==search.pid && class_name(window)==DINGTALK_FRAME && GetWindow(window,GW_OWNER).is_err()
            && main_frame(Messenger::DingTalk,DINGTALK_FRAME,GetWindowLongW(window,GWL_STYLE)as u32,GetWindowLongW(window,GWL_EXSTYLE)as u32){
            search.found=true;return false.into();
        }
        true.into()
    }
    let mut search=Search{pid,found:false};
    let _=EnumWindows(Some(visit),LPARAM((&mut search as *mut Search)as isize));
    search.found
}
/// 飞书主窗口的标题固定为应用名（切换消息、云文档等页面不变，关到托盘后仍保留）；
/// 「在独立窗口打开」的会话、文档窗口与主窗口同进程、同类名、同样式，只有标题是会话或文档名。
const FEISHU_TITLES:[&str;3]=["飞书","Feishu","Lark"];
const FEISHU_FRAME:&str="Chrome_WidgetWin_1";
fn feishu_title(title:&str)->bool{FEISHU_TITLES.iter().any(|name|name.eq_ignore_ascii_case(title.trim()))}
fn window_title(window:HWND)->String{
    let mut text=[0u16;256];
    let n=unsafe{GetWindowTextW(window,&mut text)}.max(0) as usize;
    String::from_utf16_lossy(&text[..n])
}
unsafe fn feishu_frame(window:HWND)->bool{
    class_name(window)==FEISHU_FRAME && GetWindow(window,GW_OWNER).is_err()
        && main_frame(Messenger::Feishu,FEISHU_FRAME,GetWindowLongW(window,GWL_STYLE)as u32,GetWindowLongW(window,GWL_EXSTYLE)as u32)
}
/// 同进程里唯一一个以应用名为标题的飞书主窗口；没有或不止一个（比如有会话恰好叫「飞书」）时不猜。
unsafe fn feishu_main_frame(pid:u32)->Option<HWND>{
    struct Search{pid:u32,found:Vec<HWND>}
    unsafe extern "system" fn visit(window:HWND,data:LPARAM)->BOOL{
        let search=&mut *(data.0 as *mut Search);
        if window_pid(window)==search.pid && feishu_frame(window) && feishu_title(&window_title(window)){search.found.push(window);}
        true.into()
    }
    let mut search=Search{pid,found:Vec::new()};
    let _=EnumWindows(Some(visit),LPARAM((&mut search as *mut Search)as isize));
    match search.found.as_slice(){[one]=>Some(*one),_=>None}
}
/// 飞书独立会话 / 文档窗口闪动时，归到同进程的主窗口：同一进程就是同一个飞书实例，不会串到其他账号。
unsafe fn feishu_owner(window:HWND)->Option<HWND>{
    if !feishu_frame(window) || feishu_title(&window_title(window)){return None;}
    feishu_main_frame(window_pid(window))
}
fn candidates(client:Messenger,paths:&[String])->Option<Vec<Target>>{unsafe{
    struct Search<'a>{client:Messenger,paths:&'a [String],windows:Vec<Unread>}
    unsafe extern "system" fn visit(window:HWND,data:LPARAM)->BOOL{
        let search=&mut *(data.0 as *mut Search);
        if main_window(window,search.client) && matches(window,search.paths){
            search.windows.push(Unread{source:search.client,pid:window_pid(window),window:window.0 as isize});
        }
        true.into()
    }
    let mut search=Search{client,paths,windows:Vec::new()};
    EnumWindows(Some(visit),LPARAM((&mut search as *mut Search)as isize)).ok()?;
    search.windows.into_iter().map(|unread|{
        let process=Owned::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,unread.pid).ok()?);
        Some(Target{unread,started:process_started(*process)?})
    }).collect()
}}
/// 只接受原事件窗口或其明确的所属主窗口，同进程的其他窗口不能借用。
fn select_target(client:Messenger,window:isize,accounts:&[Target])->Unread{
    let Some(target)=accounts.iter().find(|target|target.unread.source==client && target.unread.window==window)else{return Unread::of(client);};
    if accounts.iter().filter(|other|other.unread.source==client && other.unread.pid==target.unread.pid).count()!=1{return Unread::of(client);}
    target.unread
}
pub(crate) fn flash_target(client:Messenger,window:HWND,paths:&[String])->Option<Unread>{unsafe{
    if !matches(window,paths){return None;}
    let Some(accounts)=candidates(client,paths)else{return Some(Unread::of(client));};
    let mut root=GetAncestor(window,GA_ROOTOWNER);
    if client==Messenger::Feishu {if let Some(main)=feishu_owner(root){root=main;}}
    Some(select_target(client,root.0 as isize,&accounts))
}}
fn ready(target:Target)->bool{
    ready_with_paths(target,&client_paths(target.unread.source))
}
fn ready_with_paths(target:Target,paths:&[String])->bool{
    candidates(target.unread.source,paths).is_some_and(|accounts|{
        let mine=accounts.iter().filter(|current|current.unread.pid==target.unread.pid).collect::<Vec<_>>();
        matches!(mine.as_slice(),[one] if **one==target)
    })
}
fn open(unread:Unread,paths:&[String],_:bool)->Result<(),OpenError>{unsafe{
    let window=HWND(unread.window as *mut _);
    if unread.pid==0 || window_pid(window)!=unread.pid {wake_step("提醒窗口已不属于该账号");return Err(OpenError::Unavailable);}
    if !main_window(window,unread.source){wake_step(format!("提醒窗口不是主窗口（{}）",describe_window(window)));return Err(OpenError::Unavailable);}
    if !matches(window,paths){wake_step("提醒窗口所属程序不在已知安装路径中");return Err(OpenError::Unavailable);}
    // 不执行协议、托盘回调或鼠标键盘模拟，只恢复该消息保存的主窗口。
    wake_step(format!("切前台 {}",describe_window(window)));
    match wake_route(unread.source,IsWindowVisible(window).as_bool(),IsIconic(window).as_bool()) {
        WakeRoute::Restore=>{let _=ShowWindowAsync(window,SW_RESTORE);}
        WakeRoute::Show=>{let _=ShowWindowAsync(window,SW_SHOW);}
        WakeRoute::Relaunch=>return relaunch(window,unread.pid,paths),
        WakeRoute::Focus=>{}
    }
    if SetForegroundWindow(window).as_bool(){wake_step("系统已接受");Ok(())}else{wake_step("系统拒绝切前台");Err(OpenError::Unavailable)}
}}

#[derive(Debug,PartialEq,Eq)]
enum WakeRoute {Focus,Restore,Show,Relaunch}
/// 怎样把主窗口带到前台。飞书关到托盘后窗口被隐藏，若由岛仔直接 `ShowWindow`，飞书不会恢复渲染，
/// 页面停在加载骨架、点击无响应（#181，本机飞书 8.6 复现）；改为再启动一次飞书主程序，由飞书自己显示原窗口。
/// 最小化的窗口照常还原（实测正常）；其他客户端未验证，保持原做法。
fn wake_route(source:Messenger,visible:bool,iconic:bool)->WakeRoute {
    if iconic {WakeRoute::Restore}
    else if visible {WakeRoute::Focus}
    else if source==Messenger::Feishu {WakeRoute::Relaunch}
    else {WakeRoute::Show}
}

/// 启动提醒窗口所属进程自己的程序（单实例：不会多开，飞书把已有主窗口显示出来），再等窗口出现。
unsafe fn relaunch(window:HWND,pid:u32,paths:&[String])->Result<(),OpenError>{
    let Some(path)=crate::smtc::player_process::image_path(pid) else{wake_step("无法读取客户端程序路径");return Err(OpenError::Unavailable);};
    // 只启动已核实属于该客户端（安装路径或文件名加发行商签名）的程序。
    if !path_allowed(&normalized(&path),paths) {wake_step("客户端程序不在已知安装路径中");return Err(OpenError::Unavailable);}
    let directory=std::path::Path::new(&path).parent().map(std::path::Path::to_path_buf);
    let mut command=std::process::Command::new(&path);
    if let Some(directory)=directory {command.current_dir(directory);}
    wake_step("窗口在托盘中，由客户端自己显示主窗口");
    if command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().is_err(){
        wake_step("启动客户端程序失败");return Err(OpenError::Unavailable);
    }
    let until=std::time::Instant::now()+std::time::Duration::from_secs(3);
    while std::time::Instant::now()<until {
        if IsWindowVisible(window).as_bool(){let _=SetForegroundWindow(window);wake_step("客户端已显示主窗口");return Ok(());}
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    wake_step("客户端 3 秒内没有显示主窗口");Err(OpenError::Unavailable)
}

#[cfg(test)]
mod tests{
    use super::*;
    use windows::{core::w,Win32::{Foundation::{LRESULT,WPARAM},System::{LibraryLoader::GetModuleHandleW,Threading::GetCurrentProcess}}};
    fn target(client:Messenger,pid:u32,window:isize)->Target{Target{unread:Unread{source:client,pid,window},started:1}}
    #[test]
    fn three_or_more_instances_never_rebind_to_another_or_the_last_instance(){
        for client in Messenger::OFFICE{
            let (a,b,c)=(target(client,100,11),target(client,200,22),target(client,300,33));
            assert_eq!(select_target(client,11,&[a,b,c]),a.unread);
            assert_eq!(select_target(client,22,&[a,b,c]),b.unread);
            assert_eq!(select_target(client,11,&[b,c]),Unread::of(client));
            assert_eq!(select_target(client,11,&[c]),Unread::of(client));
            assert_eq!(select_target(client,11,&[a,Target{unread:Unread{window:44,..a.unread},..a}]),Unread::of(client),"同进程多窗口不能当作唯一账号");
        }
    }
    #[test]
    fn feishu_in_tray_is_shown_by_feishu_itself(){
        assert_eq!(wake_route(Messenger::Feishu,false,false),WakeRoute::Relaunch,"关到托盘的飞书不能由岛仔直接显示（#181 卡死）");
        assert_eq!(wake_route(Messenger::Feishu,true,true),WakeRoute::Restore,"最小化的飞书照常还原");
        assert_eq!(wake_route(Messenger::Feishu,true,false),WakeRoute::Focus);
        for client in [Messenger::DingTalk,Messenger::WeCom] {assert_eq!(wake_route(client,false,false),WakeRoute::Show,"其他客户端未验证，保持原做法");}
    }
    /// 本机实测：先把飞书关到托盘，再运行 `cargo test live_feishu -- --ignored`，走岛仔点提醒时的同一条唤起路径。
    #[test]
    #[ignore="需要本机飞书已登录并关到托盘"]
    fn live_feishu_tray_window_is_shown_by_feishu(){unsafe{
        let paths=client_paths(Messenger::Feishu);
        let accounts=candidates(Messenger::Feishu,&paths).expect("应找到飞书主窗口");
        let [target]=accounts.as_slice() else{panic!("应只有一个飞书主窗口：{}",accounts.len())};
        let window=HWND(target.unread.window as *mut _);
        assert!(!IsWindowVisible(window).as_bool(),"请先把飞书关到托盘");
        crate::messenger::windows::take_wake_steps();
        let result=open(target.unread,&paths,true);
        let steps=crate::messenger::windows::take_wake_steps();
        eprintln!("{steps:?}");
        assert_eq!(result,Ok(()));assert!(IsWindowVisible(window).as_bool());
        assert!(steps.iter().any(|s|s.contains("由客户端自己显示")));
    }}
    #[test]
    fn main_window_rules_exclude_login_popups_and_cross_client_windows(){
        let frame=WS_OVERLAPPEDWINDOW.0;
        assert!(main_frame(Messenger::WeCom,"WeWorkWindow",WS_POPUP.0,0));
        assert!(main_frame(Messenger::Feishu,"Chrome_WidgetWin_1",frame,0));
        assert!(main_frame(Messenger::DingTalk,"Qt51511QWindowIcon",frame,0));
        assert!(main_frame(Messenger::DingTalk,"StandardFrame_DingTalk",0x360f0000,0x100),"支持实际日志中的已登录钉钉主窗口和最小化状态");
        for client in Messenger::OFFICE{
            assert!(!main_frame(client,"WeChatLogin",frame,0));
            assert!(!main_frame(client,"Chrome_WidgetWin_0",frame,0));
            assert!(!main_frame(client,"PerryShadowWnd",frame,0));
        }
        assert!(!main_frame(Messenger::Feishu,"Chrome_WidgetWin_1",WS_POPUP.0,0));
        assert!(!main_frame(Messenger::DingTalk,"Qt51511QWindowIcon",0x96020000,0));
        assert!(!main_frame(Messenger::DingTalk,"StandardFrame_DingTalk",WS_POPUP.0,0));
        assert!(!main_frame(Messenger::DingTalk,"StandardFrame_DingTalk",frame,WS_EX_TOOLWINDOW.0));
        assert!(!main_frame(Messenger::DingTalk,"StandardFrame_DingTalk",frame|WS_CHILD.0,0));
        assert!(!main_frame(Messenger::Feishu,"StandardFrame_DingTalk",frame,0));
        assert!(!main_frame(Messenger::WeCom,"WeWorkWindow",WS_POPUP.0,WS_EX_TOOLWINDOW.0));
    }
    #[test]
    fn synthetic_native_windows_keep_original_handles_and_reject_ambiguity(){unsafe{
        unsafe extern "system" fn proc(h:HWND,m:u32,w:WPARAM,l:LPARAM)->LRESULT{DefWindowProcW(h,m,w,l)}
        let instance=GetModuleHandleW(None).unwrap().into();let pid=std::process::id();
        let paths=[normalized(&std::env::current_exe().unwrap().to_string_lossy())];
        let started=process_started(GetCurrentProcess()).unwrap();
        for (client,class) in [(Messenger::WeCom,w!("WeWorkWindow")),(Messenger::Feishu,w!("Chrome_WidgetWin_1")),(Messenger::DingTalk,w!("Qt51511QWindowIcon")),(Messenger::DingTalk,w!("StandardFrame_DingTalk"))]{
            RegisterClassW(&WNDCLASSW{lpfnWndProc:Some(proc),hInstance:instance,lpszClassName:class,..Default::default()});
            let create=||CreateWindowExW(WINDOW_EX_STYLE(0),class,w!("Daozai synthetic client"),WS_OVERLAPPEDWINDOW,-10000,-10000,400,500,None,None,instance,None).unwrap();
            let first=create();let original=Target{unread:Unread{source:client,pid,window:first.0 as isize},started};
            assert!(ready_with_paths(original,&paths),"唯一原窗口必须可识别");
            assert_eq!(flash_target(client,first,&paths),Some(original.unread));
            let helper=CreateWindowExW(WS_EX_TOOLWINDOW,w!("STATIC"),w!(""),WS_POPUP,-10000,-10000,400,500,None,None,instance,None).unwrap();
            let ambiguous=flash_target(client,helper,&paths);DestroyWindow(helper).unwrap();
            assert_eq!(ambiguous,Some(Unread::of(client)),"同进程但没有窗口归属关系的辅助窗口不能借用仅剩的主窗口");
            let owned=CreateWindowExW(WS_EX_TOOLWINDOW,w!("STATIC"),w!(""),WS_POPUP,-10000,-10000,400,500,first,None,instance,None).unwrap();
            let bound=flash_target(client,owned,&paths);DestroyWindow(owned).unwrap();
            assert_eq!(bound,Some(original.unread),"窗口明确属于原主窗口时可以识别");
            let second=create();
            assert!(!ready_with_paths(original,&paths),"同进程新增主窗口后不得继续按进程猜账号");
            assert_eq!(flash_target(client,first,&paths),Some(Unread::of(client)));
            DestroyWindow(first).unwrap();
            assert!(!ready_with_paths(original,&paths),"不能换成仍存活的其他窗口");
            assert_eq!(open(original.unread,&paths,true),Err(OpenError::Unavailable));
            assert!(!IsWindowVisible(second).as_bool(),"不能打开其他窗口");
            DestroyWindow(second).unwrap();
        }
        // 用户诊断包里钉钉 9.0.5 的窗口组合：一个 StandardFrame_DingTalk 主窗口和三个隐藏的 Qt 对话框。
        // 主窗口最小化到任务栏和隐藏到托盘时，都必须对应到原进程。
        for (state,frame_style) in [("最小化",0x260f0000u32),("隐藏到托盘",0x060f0000)]{
            let frame=CreateWindowExW(WINDOW_EX_STYLE(0x100),w!("StandardFrame_DingTalk"),w!("Daozai synthetic client"),WINDOW_STYLE(frame_style),-10000,-10000,1200,800,None,None,instance,None).unwrap();
            let dialogs=[0;3].map(|_|CreateWindowExW(WINDOW_EX_STYLE(0x100),w!("Qt51511QWindowIcon"),w!(""),WINDOW_STYLE(0x86cf0000),-10000,-10000,655,517,None,None,instance,None).unwrap());
            let original=Target{unread:Unread{source:Messenger::DingTalk,pid,window:frame.0 as isize},started};
            assert!(dialogs.iter().all(|dialog|!main_window(*dialog,Messenger::DingTalk)),"{state}：隐藏的 Qt 对话框不能算主窗口");
            assert!(ready_with_paths(original,&paths),"{state}：主窗口必须可识别");
            assert_eq!(flash_target(Messenger::DingTalk,frame,&paths),Some(original.unread),"{state}：闪动必须对应到原进程");
            DestroyWindow(frame).unwrap();
            assert!(dialogs.iter().all(|dialog|main_window(*dialog,Messenger::DingTalk)),"没有新版主窗口时仍按旧版 Qt 主窗口识别");
            assert_eq!(flash_target(Messenger::DingTalk,dialogs[0],&paths),Some(Unread::of(Messenger::DingTalk)),"旧版多个 Qt 主窗口仍不能猜账号");
            for dialog in dialogs{DestroyWindow(dialog).unwrap();}
        }
        // 本机飞书 8.6 实测：主窗口标题「飞书」，「在独立窗口打开」的会话窗口与它同进程、同类名、同样式，标题是会话名。
        // 主窗口正常显示和关到托盘（隐藏）两种状态都要能对应到原进程，独立窗口闪动归到主窗口。
        for (state,main_style) in [("显示",0x16cf0000u32),("关到托盘",0x06cf0000)]{
            let frame=|title,style|CreateWindowExW(WINDOW_EX_STYLE(0x00200100),w!("Chrome_WidgetWin_1"),title,WINDOW_STYLE(style),-10000,-10000,1200,800,None,None,instance,None).unwrap();
            let main=frame(w!("飞书"),main_style);let chat=frame(w!("少爷"),0x16cf0000);
            let original=Target{unread:Unread{source:Messenger::Feishu,pid,window:main.0 as isize},started};
            assert!(!main_window(chat,Messenger::Feishu),"{state}：独立会话窗口不能算第二个主窗口");
            assert!(ready_with_paths(original,&paths),"{state}：有独立会话窗口时主窗口仍可识别");
            assert_eq!(flash_target(Messenger::Feishu,main,&paths),Some(original.unread),"{state}：主窗口闪动对应原进程");
            assert_eq!(flash_target(Messenger::Feishu,chat,&paths),Some(original.unread),"{state}：独立会话窗口闪动归到同进程主窗口");
            let lookalike=frame(w!("Feishu"),0x16cf0000);
            assert_eq!(flash_target(Messenger::Feishu,chat,&paths),Some(Unread::of(Messenger::Feishu)),"{state}：两个窗口都像主窗口时不猜");
            DestroyWindow(lookalike).unwrap();DestroyWindow(main).unwrap();
            assert!(main_window(chat,Messenger::Feishu),"{state}：找不到主窗口时沿用原规则");
            DestroyWindow(chat).unwrap();
        }
    }}
    #[test]
    fn hidden_helpers_are_never_shown(){unsafe{
        let paths=[normalized(&std::env::current_exe().unwrap().to_string_lossy())];
        let window=CreateWindowExW(WS_EX_TOOLWINDOW,w!("STATIC"),w!(""),WS_POPUP,-10000,-10000,400,500,None,None,None,None).unwrap();
        for source in Messenger::OFFICE{
            assert_eq!(open(Unread{source,pid:std::process::id(),window:window.0 as isize},&paths,true),Err(OpenError::Unavailable));
            assert!(!IsWindowVisible(window).as_bool());
        }
        DestroyWindow(window).unwrap();
    }}
}
