//! 用真实 Windows 媒体会话验证连续切歌、反复开关和诊断记录。
#[cfg(windows)]
fn main(){
    use std::{time::{Duration,Instant},collections::HashSet,sync::atomic::Ordering};
    use daozai::{resources,smtc};
    let seconds=std::env::args().nth(1).map(|s|s.parse().unwrap()).unwrap_or(180);
    let directory=std::env::temp_dir().join(format!("daozai-media-resources-{}",std::process::id()));
    let monitor=resources::Monitor::start(directory.clone()).unwrap();
    let state=smtc::shared();let poller=smtc::spawn_poller(state.clone(),Default::default());
    let start=Instant::now();let mut next=0;let mut tracks=HashSet::new();let mut covers=0;
    while start.elapsed()<Duration::from_secs(seconds){
        let elapsed=start.elapsed().as_secs();poller.store(elapsed%20<18,Ordering::Relaxed);
        let media=state.lock().unwrap().clone();
        if media.title.starts_with("Daozai memory fixture"){tracks.insert(media.title);if media.artwork.is_some(){covers+=1;}}
        if elapsed>=next {
            next=elapsed+5;let sample=resources::snapshot().unwrap();
            println!("seconds={elapsed} tracks={} covers={covers} private_mib={:.2} threads={} handles={} pollers={} artwork={} winrt={}",tracks.len(),sample.private_bytes as f64/1048576.0,sample.threads,sample.handles,resources::POLLER.active(),resources::ARTWORK.active(),resources::ASYNC.active());
            assert!(sample.private_bytes<512*1024*1024 && sample.threads<128);
            assert!(resources::POLLER.active()<=1 && resources::ARTWORK.active()<=2 && resources::ASYNC.active()<=4);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(poller);let until=Instant::now()+Duration::from_secs(5);
    while resources::POLLER.active()>0 && Instant::now()<until{std::thread::sleep(Duration::from_millis(10));}
    assert_eq!(resources::POLLER.active(),0);assert!(tracks.len()>10 && covers>20,"必须经过真实的媒体与封面路径");
    assert!(std::fs::metadata(directory.join("resources.log")).unwrap().len()>100);
    drop(monitor);println!("completed tracks={} covers={covers} diagnostics={}",tracks.len(),directory.display());
}
#[cfg(not(windows))]
fn main(){}
