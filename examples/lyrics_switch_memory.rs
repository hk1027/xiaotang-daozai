//! 隔离连续切歌压测：真实渲染、真实缓存歌词，同时记录进程与字体资源。
#[cfg(windows)]
fn main()->eframe::Result<()> {
    use daozai::app::{App,Env,Options,WIN_W,WIN_H};
    use eframe::egui;
    use std::{sync::Arc,time::{Duration,Instant},ffi::c_void};
    #[repr(C)]
    #[derive(Default)]
    struct Memory {size:u32,faults:u32,peak:usize,working:usize,paged_peak:usize,paged:usize,nonpaged_peak:usize,nonpaged:usize,pagefile:usize,pagefile_peak:usize,private:usize}
    #[link(name="kernel32")]
    extern "system" {
        fn GetCurrentProcess()->*mut c_void;
        fn K32GetProcessMemoryInfo(process:*mut c_void,memory:*mut Memory,size:u32)->i32;
        fn GetProcessHandleCount(process:*mut c_void,count:*mut u32)->i32;
    }
    struct Probe {
        app:App,native:daozai::native::Window,start:Instant,frame_at:Instant,
        lyrics:Vec<daozai::lyrics::Lyrics>,song:usize,artwork:Arc<egui::ColorImage>,
        mode:String,seconds:f64,frames:u64,next_sample:f64,baseline:Option<usize>,peak:usize,active_frames:u64,
    }
    impl eframe::App for Probe {
        fn clear_color(&self,_:&egui::Visuals)->[f32;4]{[0.0;4]}
        fn update(&mut self,ctx:&egui::Context,_:&mut eframe::Frame){
            let delay=Duration::from_secs_f64(1.0/60.0).saturating_sub(self.frame_at.elapsed());
            if !delay.is_zero(){std::thread::sleep(delay);}self.frame_at=Instant::now();
            let t=self.start.elapsed().as_secs_f64();self.frames+=1;
            let song=(t/2.0)as usize;let changed=self.song!=song;
            if changed {
                self.song=song;
                self.artwork=Arc::new(egui::ColorImage::new([256,256],egui::Color32::from_rgb((song*37)as u8,(song*73)as u8,150)));
            }
            let title=format!("连续切歌检查 {song}");
            let env=Env{t,clock:"16:30:00".into(),today:"2026-11-04".into(),battery:Some(80),charging:true,
                cursor:(self.mode=="expanded").then_some(egui::pos2(WIN_W/2.0,12.0)),
                media:daozai::smtc::NowPlaying{source:"cloudmusic.exe".into(),title:title.clone(),artist:"测试".into(),song_id:Some(song as u64+1),duration:240.0,position:(t%2.0)*120.0,playing:true,artwork:Some(self.artwork.clone()),playback_type:daozai::smtc::MediaKind::Music,..Default::default()},..Default::default()};
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{
                let out=self.app.frame(ui,egui::Rect::from_min_size(ui.max_rect().min,egui::vec2(WIN_W,WIN_H)),&env);
                if out.hit.width()>280.0{self.active_frames+=1;}
                let r=|r:egui::Rect|(r.min.x,r.min.y,r.width(),r.height());
                self.native.update(r(out.region),r(out.hit),out.notice_hit.map(r),out.file_drop_zone.map(r),out.glass.as_deref(),ctx.pixels_per_point(),false,out.expanded);
            });
            // 媒体更新先清空旧歌状态，再为当前歌曲注入缓存，下一帧绘制。
            if changed{self.app.set_lyrics(&title,"测试",240.0,self.lyrics[song%self.lyrics.len()].clone());}
            if t>=self.next_sample {
                self.next_sample=t+5.0;
                let mut memory=Memory{size:std::mem::size_of::<Memory>()as u32,..Default::default()};let mut handles=0;
                unsafe{assert_ne!(K32GetProcessMemoryInfo(GetCurrentProcess(),&mut memory,memory.size),0);GetProcessHandleCount(GetCurrentProcess(),&mut handles);}
                if t>=20.0{self.baseline.get_or_insert(memory.private);self.peak=self.peak.max(memory.private);}
                let (atlas,galleys)=ctx.fonts(|f|(f.font_image_size(),f.num_galleys_in_cache()));
                let textures=ctx.tex_manager().read().num_allocated();
                println!("mode={} seconds={t:.1} tracks={} frames={} private_mib={:.2} working_mib={:.2} handles={handles} textures={textures} atlas={}x{} galleys={galleys}",self.mode,song+1,self.frames,memory.private as f64/1048576.0,memory.working as f64/1048576.0,atlas[0],atlas[1]);
                if memory.private>1536*1024*1024{panic!("私有内存超过 1.5 GiB，停止压测");}
            }
            if t>=self.seconds {
                if self.mode!="disabled"{assert!(self.active_frames>self.frames/2,"压测必须实际显示歌词区域");}
                println!("completed mode={} tracks={} baseline_mib={:.2} peak_mib={:.2} growth_mib={:.2}",self.mode,song+1,self.baseline.unwrap_or(0)as f64/1048576.0,self.peak as f64/1048576.0,self.peak.saturating_sub(self.baseline.unwrap_or(0))as f64/1048576.0);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
    let args=std::env::args().collect::<Vec<_>>();let mode=args.get(1).cloned().unwrap_or_else(||"island".into());
    assert!(["island","expanded","disabled"].contains(&mode.as_str()));
    let seconds=args.get(2).map(|s|s.parse::<f64>().unwrap()).unwrap_or(120.0);
    let cache=std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap()).join("Daozai/lyrics");
    let lyrics=std::fs::read_dir(cache).unwrap().filter_map(|e|{
        let path=e.ok()?.path();if path.extension()?.to_str()? != "lrc"{return None;}
        let lyrics=daozai::lyrics::parse_lrc(&std::fs::read_to_string(path).ok()?);
        (lyrics.synced&&lyrics.lines.len()>10).then_some(lyrics)
    }).take(100).collect::<Vec<_>>();assert!(!lyrics.is_empty(),"需要实际缓存歌词作为测试数据");
    println!("cached_songs={}",lyrics.len());
    let root=std::env::temp_dir().join(format!("daozai-switch-memory-{}",std::process::id()));std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("settings.txt"),format!("lyrics_enabled={}\nisland_lyrics={}\npet=1\n",u8::from(mode!="disabled"),u8::from(mode=="island"))).unwrap();
    std::env::set_var("DAOZAI_DATA_DIR",&root);
    let _monitor=daozai::resources::Monitor::start(root.join("diagnostics")).unwrap();
    let options=eframe::NativeOptions{vsync:false,viewport:egui::ViewportBuilder::default().with_title("岛仔连续切歌检查").with_inner_size([daozai::app::CANVAS_W,WIN_H]).with_position([100.0,100.0]).with_decorations(false).with_transparent(true).with_active(false),..Default::default()};
    eframe::run_native("岛仔连续切歌检查",options,Box::new(move|cc|{
        cc.egui_ctx.set_pixels_per_point(1.0);
        if let Some(gl)=&cc.gl{use eframe::glow::HasContext;println!("renderer={}",unsafe{gl.get_parameter_string(eframe::glow::RENDERER)});}
        let app=App::new(&cc.egui_ctx,Options{fetch_lyrics:false,pet_idle:1e9});let native=daozai::native::Window::new(cc).unwrap();
        Ok(Box::new(Probe{app,native,start:Instant::now(),frame_at:Instant::now(),lyrics,song:usize::MAX,artwork:Arc::new(egui::ColorImage::new([1,1],egui::Color32::WHITE)),mode,seconds,frames:0,next_sample:0.0,baseline:None,peak:0,active_frames:0}))
    }))
}
#[cfg(not(windows))]
fn main(){}
