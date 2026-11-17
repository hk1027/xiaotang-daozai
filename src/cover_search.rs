//! 封面兜底：播放器没给封面（系统缩略图缺失、电台 / FM 等模式）时，按歌名和歌手在线找一张。
//! 与播放器、播放模式无关；只在等过一会儿仍没有封面时才请求，每首歌最多请求两次。
use super::NowPlaying;
use eframe::egui::ColorImage;
use std::{sync::{mpsc,Arc},time::{Duration,Instant}};

/// 先给系统缩略图和播放器本地封面一点时间，避免每次换歌都去网上搜。
const WAIT:Duration=Duration::from_secs(4);
const RETRY:Duration=Duration::from_secs(30);

type Found=Option<Arc<ColorImage>>;

#[derive(Default)]
pub(crate) struct CoverSearch {
    key:String,missing_since:Option<Instant>,image:Found,attempts:usize,retry_at:Option<Instant>,
    pending:Option<mpsc::Receiver<Found>>,worker:Option<std::thread::JoinHandle<()>>,
}

impl CoverSearch {
    pub(crate) fn fill(&mut self,media:&NowPlaying,now:Instant)->Found {
        self.fill_with(media,now,|title,artist,duration|search(title,artist,duration))
    }

    pub(crate) fn fill_with(&mut self,media:&NowPlaying,now:Instant,load:impl FnOnce(String,String,f64)->Found+Send+'static)->Found {
        if media.artwork.is_some() || !media.is_music() {return None;}
        let key=format!("{}|{}",media.title,media.artist);
        // 换歌时丢弃旧请求，迟到的封面不能贴到新歌上。
        if key!=self.key {*self=Self{key,missing_since:Some(now),worker:self.worker.take(),..Default::default()};}
        if let Some(receiver)=&self.pending {
            match receiver.try_recv() {
                Ok(image)=>{self.pending=None;if image.is_some(){self.image=image;}else{self.retry_at=Some(now+RETRY);}}
                Err(mpsc::TryRecvError::Disconnected)=>self.pending=None,
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
        let waited=self.missing_since.is_some_and(|since|now.duration_since(since)>=WAIT);
        let due=self.attempts==0 || self.attempts<2 && self.retry_at.is_some_and(|at|now>=at);
        if self.image.is_none() && self.pending.is_none() && waited && due && self.worker.as_ref().is_none_or(|w|w.is_finished()) {
            let (sender,receiver)=mpsc::channel();
            let (title,artist,duration)=(media.title.clone(),media.artist.clone(),media.duration);
            // 与其他封面下载共用并发上限，网络卡住时不会越积越多。
            self.worker=crate::resources::COVER.spawn(move||{let _=sender.send(load(title,artist,duration));});
            if self.worker.is_some(){
                self.attempts+=1;self.retry_at=None;self.pending=Some(receiver);
            }
        }
        self.image.clone()
    }
}

/// 搜索结果里挑同一首歌：有歌手时歌名、歌手都要对上；没有歌手时歌名完全一致且时长相差 3 秒内才用，宁缺毋错。
pub(crate) fn pick(songs:&[serde_json::Value],title:&str,artist:&str,duration:f64)->Option<String> {
    songs.iter().find_map(|song|{
        let name=song.get("name")?.as_str()?;
        let singers=song.get("ar").or_else(||song.get("artists"))?.as_array()?.iter().filter_map(|a|a.get("name")?.as_str()).collect::<Vec<_>>().join("/");
        let length=song.get("dt").or_else(||song.get("duration")).and_then(|v|v.as_f64()).unwrap_or_default()/1000.0;
        let matched=if artist.trim().is_empty() {
            duration>0.0 && (length-duration).abs()<=3.0 && crate::lyrics::same_song(title,"",name,"")
        } else {
            crate::lyrics::same_song(title,artist,name,&singers)
                && (duration<=0.0 || length<=0.0 || (length-duration).abs()<=(duration*0.03).max(10.0))
        };
        if !matched {return None;}
        song.pointer("/al/picUrl").or_else(||song.pointer("/album/picUrl"))?.as_str().map(str::to_owned)
    })
}

fn search(title:String,artist:String,duration:f64)->Found {
    let query=format!("{title} {artist}");
    let mut url=url::Url::parse("https://music.163.com/api/cloudsearch/pc").ok()?;
    url.query_pairs_mut().append_pair("s",query.trim()).append_pair("type","1").append_pair("limit","10").append_pair("offset","0");
    let response=ureq::AgentBuilder::new().timeout(Duration::from_secs(5)).redirects(0).build().get(url.as_str())
        .set("User-Agent",concat!("Daozai/",env!("CARGO_PKG_VERSION")," (Windows)"))
        .set("Referer","https://music.163.com").set("Accept","application/json").call().ok()?;
    use std::io::Read;
    let limit=512*1024;let mut bytes=Vec::new();
    response.into_reader().take(limit+1).read_to_end(&mut bytes).ok()?;
    if bytes.len()>limit as usize{crate::resources::REJECTED_DATA.fetch_add(1,std::sync::atomic::Ordering::Relaxed);return None;}
    let value:serde_json::Value=serde_json::from_slice(&bytes).ok()?;
    if value.get("code").and_then(|v|v.as_i64())!=Some(200){return None;}
    let cover=pick(value.pointer("/result/songs")?.as_array()?,&title,&artist,duration)?;
    // 只接受网易云图片域名，按 256 缩略图下载，复用现有的大小与尺寸上限。
    super::netease_playback::fetch_cover(&cover)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smtc::MediaKind;
    fn songs()->Vec<serde_json::Value>{serde_json::from_value(serde_json::json!([
        {"name":"遥远的她 (Live)","ar":[{"name":"张学友"}],"dt":300000,"al":{"picUrl":"https://p2.music.126.net/live.jpg"}},
        {"name":"遥远的她","ar":[{"name":"张学友"}],"dt":253000,"al":{"picUrl":"https://p2.music.126.net/studio.jpg"}},
        {"name":"遥远的她","ar":[{"name":"翻唱歌手"}],"dt":240000,"al":{"picUrl":"https://p2.music.126.net/cover.jpg"}}
    ])).unwrap()}
    #[test]
    fn only_the_same_song_supplies_a_cover(){
        assert_eq!(pick(&songs(),"遥远的她","张学友",253.0).as_deref(),Some("https://p2.music.126.net/studio.jpg"),"Live 版、翻唱都不能当原曲封面");
        assert_eq!(pick(&songs(),"遥远的她","翻唱歌手",0.0).as_deref(),Some("https://p2.music.126.net/cover.jpg"));
        assert!(pick(&songs(),"遥远的她","别的歌手",253.0).is_none());
        assert_eq!(pick(&songs(),"遥远的她","",240.5).as_deref(),Some("https://p2.music.126.net/cover.jpg"),"没有歌手时靠时长确认");
        assert!(pick(&songs(),"遥远的她","",0.0).is_none(),"没有歌手也没有时长时不猜");
    }
    #[test]
    fn cover_waits_for_a_free_worker_without_spending_an_attempt(){
        let media=NowPlaying{title:"晚风".into(),artist:"示例歌手".into(),playback_type:MediaKind::Music,..Default::default()};
        let now=Instant::now();
        let mut cache=CoverSearch::default();
        cache.fill_with(&media,now,|_,_,_|None);
        let busy=[crate::resources::COVER.acquire().unwrap(),crate::resources::COVER.acquire().unwrap()];
        cache.fill_with(&media,now+WAIT,|_,_,_|panic!("工作线程已占满"));
        drop(busy);
        let image=Arc::new(ColorImage::new([1,1],eframe::egui::Color32::RED));
        let expected=image.clone();
        cache.fill_with(&media,now+WAIT+RETRY,move|_,_,_|Some(image));
        cache.worker.take().expect("并发空位释放后仍应补封面").join().unwrap();
        assert!(Arc::ptr_eq(&cache.fill_with(&media,now+WAIT+RETRY,|_,_,_|None).unwrap(),&expected));
    }
    #[test]
    fn search_waits_then_runs_once_in_background_and_follows_the_song(){
        let start=Instant::now();let mut cache=CoverSearch::default();
        let media=NowPlaying{title:"晴天".into(),artist:"周杰伦".into(),playback_type:MediaKind::Music,..Default::default()};
        let calls=Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let load=|calls:&Arc<std::sync::atomic::AtomicUsize>,image:Found|{let calls=calls.clone();move|_:String,_:String,_:f64|{calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst);image}};
        assert!(cache.fill_with(&media,start,load(&calls,None)).is_none());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst),0,"先等系统缩略图");
        cache.fill_with(&media,start+WAIT,load(&calls,None));cache.worker.take().unwrap().join().unwrap();
        cache.fill_with(&media,start+WAIT,load(&calls,None));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst),1,"失败后不立即重试");
        let image=Arc::new(ColorImage::new([1,1],eframe::egui::Color32::RED));
        cache.fill_with(&media,start+WAIT+RETRY,load(&calls,Some(image.clone())));cache.worker.take().unwrap().join().unwrap();
        assert!(Arc::ptr_eq(&cache.fill_with(&media,start+WAIT+RETRY,load(&calls,None)).unwrap(),&image));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst),2);
        let mut with_cover=media.clone();with_cover.artwork=Some(image);
        assert!(cache.fill_with(&with_cover,start,load(&calls,None)).is_none(),"播放器自己有封面时不搜");
        let next=NowPlaying{title:"七里香".into(),..media};
        assert!(cache.fill_with(&next,start+WAIT*20,load(&calls,None)).is_none(),"换歌后重新等待，旧封面不沿用");
    }
}
