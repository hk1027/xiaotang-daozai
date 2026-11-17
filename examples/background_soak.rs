//! 真实后台采样持续检查，不打开窗口或读取账号配置。
#[cfg(windows)]
fn main(){
    let ctx=eframe::egui::Context::default();
    let observations=daozai::system::observe(ctx.clone(),Default::default(),None);
    let bluetooth=daozai::bt::spawn();
    let media=daozai::smtc::shared();
    let _enabled=daozai::smtc::spawn_poller(media.clone(),ctx);
    let start=std::time::Instant::now();let mut samples=0;
    while start.elapsed().as_secs()<360 {
        while observations.try_recv().is_ok(){samples+=1;}
        while bluetooth.rx.try_recv().is_ok(){}
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    println!("持续运行 {} 秒，接收 {} 次系统状态变化",start.elapsed().as_secs(),samples);
}
#[cfg(not(windows))]
fn main(){}
