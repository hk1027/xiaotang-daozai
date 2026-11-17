//! 手动性能采样：各后台服务在独立测试进程中运行，避免相互干扰。
#![cfg(windows)]
use std::{ffi::c_void,time::{Duration,Instant}};

#[repr(C)]
#[derive(Default)]
struct Memory {size:u32,faults:u32,peak:usize,working:usize,paged_peak:usize,paged:usize,nonpaged_peak:usize,nonpaged:usize,pagefile:usize,pagefile_peak:usize,private:usize}
#[link(name="kernel32")]
extern "system" {
    fn GetCurrentProcess()->*mut c_void;
    fn GetProcessTimes(process:*mut c_void,created:*mut u64,exited:*mut u64,kernel:*mut u64,user:*mut u64)->i32;
    fn K32GetProcessMemoryInfo(process:*mut c_void,memory:*mut Memory,size:u32)->i32;
}
fn sample()->(f64,usize,usize){unsafe{
    let(mut created,mut exited,mut kernel,mut user)=(0,0,0,0);
    assert_ne!(GetProcessTimes(GetCurrentProcess(),&mut created,&mut exited,&mut kernel,&mut user),0);
    let mut memory=Memory{size:std::mem::size_of::<Memory>()as u32,..Default::default()};
    assert_ne!(K32GetProcessMemoryInfo(GetCurrentProcess(),&mut memory,memory.size),0);
    ((kernel+user)as f64/10_000_000.0,memory.working,memory.private)
}}

#[test]
#[ignore="读取本机系统服务及已配置账号；通过 DAOZAI_PERF_SERVICE 选择采样模块"]
fn background_service_sample(){
    let service=std::env::var("DAOZAI_PERF_SERVICE").unwrap_or_else(|_|"idle".into());
    let context=eframe::egui::Context::default();
    let media=daozai::smtc::shared();
    let(mut bluetooth,mut system,mut notifications)=(None,None,None);
    let mut mail=None;
    let mut mail_events=Vec::new();
    let (mut calendar,mut quota)=(None,None);
    let (mut quota_ok,mut quota_failed)=(0,0);
    let (mut calendar_events,mut quota_events)=(Vec::new(),Vec::new());
    match service.as_str(){
        "media"=>{daozai::smtc::spawn_poller(media.clone(),context);},
        "bluetooth"=>bluetooth=Some(daozai::bt::spawn()),
        "system"=>system=Some(daozai::system::observe(context,Default::default(),None)),
        "notifications"=>notifications=Some(daozai::system::connect(context,daozai::messenger::NotificationFilter::All).expect("已有通知权限")),
        "mail"=>{
            let accounts=daozai::mail::load_accounts().into_iter().filter(|a|a.enabled).take(1).collect::<Vec<_>>();
            assert_eq!(accounts.len(),1,"需要一个已配置邮箱");
            let worker=daozai::mail::Service::start();worker.sync(&accounts,120);mail=Some(worker);
        },
        "calendar"=>{let configs=daozai::calendar::load_configs();println!("configured_accounts={}",configs.len());let worker=daozai::calendar::Service::start();worker.configure(configs,true);calendar=Some(worker);},
        "quota"=>{let config=daozai::quota::Config::load();let accounts=config.accounts.iter().filter(|a|config.enabled && a.enabled).cloned().collect::<Vec<_>>();println!("configured_accounts={}",accounts.len());let mut worker=daozai::quota::Service::start();worker.refresh(accounts);quota=Some((worker,config));},
        "idle"=>{},_=>panic!("未知采样服务"),
    }
    let before=sample();let started=Instant::now();
    while started.elapsed()<Duration::from_secs(20){
        if let Some(service)=&bluetooth{while service.rx.try_recv().is_ok(){}}
        if let Some(rx)=&system{while rx.try_recv().is_ok(){}}
        if let Some(rx)=&notifications{while rx.try_recv().is_ok(){}}
        if let Some(worker)=&mail{while let Some(event)=worker.try_recv(){mail_events.push(event);}}
        if let Some(worker)=&calendar{while let Some(event)=worker.try_recv(){calendar_events.push(event);}}
        if let Some((worker,config))=&quota{while let Some(event)=worker.try_recv(config){if event.result.is_ok(){quota_ok+=1;}else{quota_failed+=1;}quota_events.push(event);}}
        std::thread::sleep(Duration::from_millis(100));
    }
    let after=sample();
    if quota.is_some(){println!("successful_accounts={quota_ok} failed_accounts={quota_failed}");}
    if calendar.is_some(){println!("successful_syncs={} failed_syncs={}",calendar_events.iter().filter(|e|matches!(e,daozai::calendar::Event::Synced{..})).count(),calendar_events.iter().filter(|e|matches!(e,daozai::calendar::Event::Failed{..})).count());}
    if mail.is_some(){let count=mail_events.iter().filter(|event|matches!(event,daozai::mail::Event::Mail{..})).count();assert!(count>0,"必须实际读到邮件才能比较占用");println!("loaded_messages={count}");}
    println!("service={service} elapsed={:.2}s cpu={:.3}s working={:.2}MiB private={:.2}MiB",started.elapsed().as_secs_f64(),after.0-before.0,after.1 as f64/1048576.0,after.2 as f64/1048576.0);
}
