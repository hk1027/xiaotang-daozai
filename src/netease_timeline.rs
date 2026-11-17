//! 网易云、酷狗未上报系统时间轴时，读取已验证版本的只读播放时间。
use std::ffi::c_void;
type Handle = *mut c_void;
#[link(name="kernel32")]
extern "system" {
    fn OpenProcess(access:u32,inherit:i32,pid:u32)->Handle;
    fn CloseHandle(handle:Handle)->i32;
    fn ReadProcessMemory(handle:Handle,address:usize,buffer:*mut c_void,size:usize,read:*mut usize)->i32;
    fn K32EnumProcessModules(handle:Handle,modules:*mut usize,size:u32,needed:*mut u32)->i32;
    fn K32GetModuleBaseNameW(handle:Handle,module:usize,name:*mut u16,size:u32)->u32;
    fn K32GetModuleFileNameExW(handle:Handle,module:usize,name:*mut u16,size:u32)->u32;
}
struct Process(Handle);
impl Drop for Process {fn drop(&mut self){unsafe{CloseHandle(self.0);}}}
fn read<const N:usize>(p:Handle,address:usize)->Option<[u8;N]> {
    let mut bytes=[0;N];let mut count=0;
    (unsafe{ReadProcessMemory(p,address,bytes.as_mut_ptr().cast(),N,&mut count)}!=0 && count==N).then_some(bytes)
}
fn valid(position:f64,duration:f64)->bool {
    position.is_finite() && duration.is_finite() && duration>0.0 && duration<86400.0 && position>=0.0 && position<=duration
}
#[test]
fn rejects_invalid_timeline(){
    assert!(!valid(f64::NAN,240.0));assert!(!valid(0.0,0.0));assert!(!valid(241.0,240.0));assert!(!valid(-1.0,240.0));assert!(valid(12.01,275.5));
}
fn name(p:Handle,module:usize)->String {
    let mut s=[0u16;260];let n=unsafe{K32GetModuleBaseNameW(p,module,s.as_mut_ptr(),260)} as usize;
    String::from_utf16_lossy(&s[..n])
}
pub fn timeline()->Option<(f64,f64)> {query(false)}
pub fn kugou_position()->Option<f64> {query(true).map(|(position,_)|position)}

fn valid_kugou_position(position:f64)->bool {position.is_finite() && (0.0..86400.0).contains(&position)}
#[test]
fn kugou_rejects_invalid_positions(){
    assert!(valid_kugou_position(0.0));assert!(valid_kugou_position(214.358));
    for invalid in [f64::NAN,f64::INFINITY,-1.0,86400.0]{assert!(!valid_kugou_position(invalid));}
}

/// 已找到的进程与模块基址；每轮只需读几个字节，不再枚举全部进程和模块。
#[derive(Clone,Copy)]
struct Found {pid:u32,base:usize}
/// `rejected`：组件版本不匹配的进程号。版本在进程存活期间不会变，同一进程不再反复做进程快照。
struct Slot {found:Option<Found>,missed:Option<std::time::Instant>,rejected:Option<u32>}
static SLOTS:std::sync::Mutex<[Slot;2]>=std::sync::Mutex::new([Slot{found:None,missed:None,rejected:None},Slot{found:None,missed:None,rejected:None}]);
/// 没找到（未运行或版本不匹配）时，重新枚举的最短间隔。
const REDISCOVER:std::time::Duration=std::time::Duration::from_secs(3);

fn target(kugou:bool)->(&'static str,&'static str,u32,u32,usize){
    if kugou {("KuGou.exe","kgplayer.dll",0x6ac9ec86,4587520,0x3949f8)}else{("cloudmusic.exe","cloudmusic.dll",0x6ad712e1,41054208,0x21a74d8)}
}

fn query(kugou:bool)->Option<(f64,f64)> {
    let mut slots=SLOTS.lock().unwrap_or_else(|e|e.into_inner());
    let slot=&mut slots[kugou as usize];
    if let Some(found)=slot.found {
        if let Some(value)=sample(kugou,found){return value;}
        slot.found=None;
    }
    if slot.missed.is_some_and(|at|at.elapsed()<REDISCOVER){return None;}
    if let Some(pid)=slot.rejected {
        // 播放器窗口仍属于那个版本不匹配的进程：只看一次窗口（微秒级），不做十几毫秒的进程快照。
        if window_pid(kugou)==Some(pid) && super::player_process::image_is(pid,target(kugou).0) {slot.missed=Some(std::time::Instant::now());return None;}
        slot.rejected=None;
    }
    let topic=if kugou{"酷狗进度"}else{"网易云进度"};
    match discover(kugou) {
        Ok(found)=>{
            slot.found=Some(found);slot.missed=None;
            let value=sample(kugou,found).flatten();
            super::music_log::state(topic,"已适配，读取内存进度");
            value
        }
        Err(Miss{reason,mismatched})=>{
            slot.missed=Some(std::time::Instant::now());slot.rejected=mismatched;
            super::music_log::state(topic,&reason);None
        }
    }
}

/// 外层 None 表示进程或模块已失效，需要重新查找；内层 None 只是本轮读数无效。
fn sample(kugou:bool,found:Found)->Option<Option<(f64,f64)>> {
    let (process,_,stamp,image_size,position_offset)=target(kugou);
    // 进程号可能已被其他软件复用：先用有限查询权限核对身份，再申请读取权限。
    if !super::player_process::image_is(found.pid,process){return None;}
    let p=Process(unsafe{OpenProcess(0x410,0,found.pid)});if p.0.is_null(){return None;}
    // 进程号可能被复用：每轮都核对进程名与 PE 标识（只读几个字节）。
    if !name(p.0,0).eq_ignore_ascii_case(process){return None;}
    let pe=u32::from_le_bytes(read::<4>(p.0,found.base+60)?) as usize;
    if pe>4096{return None;}
    if u32::from_le_bytes(read::<4>(p.0,found.base+pe+8)?)!=stamp || u32::from_le_bytes(read::<4>(p.0,found.base+pe+80)?)!=image_size{return None;}
    let position=f64::from_le_bytes(read::<8>(p.0,found.base+position_offset)?);
    if kugou {return Some(valid_kugou_position(position).then_some((position,0.0)));}
    let duration=f64::from_le_bytes(read::<8>(p.0,found.base+0x2214778)?);
    Some(valid(position,duration).then_some((position,duration)))
}

/// 找不到时返回原因（写进诊断日志）：进程没开、组件没加载，或组件版本与已验证的不一致。
/// 没找到的原因；`mismatched` 是组件版本不匹配的进程号（其他原因为空，之后照常重试）。
struct Miss {reason:String,mismatched:Option<u32>}
impl From<String> for Miss {fn from(reason:String)->Self{Self{reason,mismatched:None}}}

/// 播放器主窗口所属的进程号：查一次窗口，不需要进程快照。
fn window_pid(kugou:bool)->Option<u32>{
    use windows::{core::{w,PCWSTR},Win32::UI::WindowsAndMessaging::{FindWindowW,GetWindowThreadProcessId}};
    let class=if kugou{w!("kugou_ui")}else{w!("OrpheusBrowserHost")};
    let window=unsafe{FindWindowW(class,PCWSTR::null())}.ok()?;
    let mut pid=0;unsafe{GetWindowThreadProcessId(window,Some(&mut pid));}
    (pid!=0).then_some(pid)
}

fn discover(kugou:bool)->Result<Found,Miss> {
    let (process,module,stamp,image_size,_)=target(kugou);
    discover_in(process,module,stamp,image_size)
}

fn discover_in(process:&str,module:&str,stamp:u32,image_size:u32)->Result<Found,Miss> {
    let mut reason=format!("未找到 {process}");
    // 先从进程快照按名称筛出播放器，只向核实后的播放器申请内存读取权限。
    for id in super::player_process::pids_named(process) {
        if !super::player_process::image_is(id,process){continue;}
        let p=Process(unsafe{OpenProcess(0x410,0,id)});
        if p.0.is_null(){reason=format!("无法读取 {process}（权限不足）");continue;}
        if !name(p.0,0).eq_ignore_ascii_case(process){continue;}
        let mut modules=[0usize;1024];let mut size=0;
        if unsafe{K32EnumProcessModules(p.0,modules.as_mut_ptr(),std::mem::size_of_val(&modules) as u32,&mut size)}==0{continue;}
        reason=format!("{process} 未加载 {module}");
        for &base in modules.iter().take(size as usize/std::mem::size_of::<usize>()) {
            if !name(p.0,base).eq_ignore_ascii_case(module){continue;}
            let header=read::<4>(p.0,base+60).map(|pe|u32::from_le_bytes(pe) as usize).filter(|pe|*pe<=4096)
                .and_then(|pe|Some((u32::from_le_bytes(read::<4>(p.0,base+pe+8)?),u32::from_le_bytes(read::<4>(p.0,base+pe+80)?))));
            let Some((actual_stamp,actual_size))=header else{return Err(format!("无法读取 {module} 文件头").into());};
            // 仅使用本机验证过的 PE 时间戳与映像大小，其他版本安全回退。
            if actual_stamp!=stamp || actual_size!=image_size {
                let version=module_path(p.0,base).map_or_else(||"未知".into(),|path|super::music_log::file_version(&path));
                return Err(Miss{reason:format!("{module} 版本不匹配：{version} 时间戳{actual_stamp:08x} 大小{actual_size}（已适配 时间戳{stamp:08x} 大小{image_size}）"),mismatched:Some(id)});
            }
            return Ok(Found{pid:id,base});
        }
    }
    Err(reason.into())
}

fn module_path(p:Handle,module:usize)->Option<String> {
    let mut s=[0u16;1024];let n=unsafe{K32GetModuleFileNameExW(p,module,s.as_mut_ptr(),1024)} as usize;
    (n>0).then(||String::from_utf16_lossy(&s[..n]))
}

#[test]
fn discovery_reports_why_a_player_is_not_adapted(){
    let me=std::env::current_exe().unwrap();
    let exe=me.file_name().unwrap().to_str().unwrap().to_owned();
    let miss=|module:&str,stamp|discover_in(&exe,module,stamp,1).err().unwrap();
    let none=discover_in("不存在的播放器.exe","x.dll",0,0).err().unwrap();
    assert_eq!((none.reason.as_str(),none.mismatched),("未找到 不存在的播放器.exe",None));
    let missing=miss("不存在的组件.dll",0);
    assert_eq!((missing.reason,missing.mismatched),(format!("{exe} 未加载 不存在的组件.dll"),None),"组件可能稍后才加载，之后要照常重试");
    let mismatch=miss("kernel32.dll",1);
    // 同名测试程序可能同时运行多个，只要求记下的是这个程序的某个进程。
    assert!(mismatch.mismatched.is_some_and(|pid|super::player_process::image_is(pid,&exe)),"版本不匹配时记下进程，同一进程不再反复快照");
    let reason=mismatch.reason;
    assert!(reason.starts_with("kernel32.dll 版本不匹配：") && reason.contains("已适配 时间戳00000001 大小1") && !reason.contains("未知"),"{reason}");
}

#[test]
#[ignore="需要本机酷狗正在运行：只读核对播放组件是否为已适配版本，并打印原因"]
fn live_kugou_discovery_reason(){
    let result=discover(true);
    println!("酷狗进度：{}，主窗口进程 {:?}",result.as_ref().map_or_else(|miss|miss.reason.clone(),|found|format!("已适配 pid{}",found.pid)),window_pid(true));
    assert!(result.is_ok() || result.as_ref().err().is_some_and(|miss|!miss.reason.is_empty()));
}
