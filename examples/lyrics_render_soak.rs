//! 隔离数据目录下持续绘制七分钟：前六分钟反复伸缩，最后一分钟稳定歌词。
#[cfg(windows)]
fn main()->eframe::Result<()> {
    use daozai::app::{App,Env,Options,WIN_W,WIN_H};
    use eframe::egui;
    struct Probe {app:App,native:daozai::native::Window,start:std::time::Instant,frame_at:std::time::Instant,textures:Option<usize>,frames:u64,min_width:f32,max_width:f32}
    impl eframe::App for Probe {
        fn clear_color(&self,_:&egui::Visuals)->[f32;4]{[0.0;4]}
        fn update(&mut self,ctx:&egui::Context,_:&mut eframe::Frame){
            let delay=std::time::Duration::from_secs_f64(1.0/60.0).saturating_sub(self.frame_at.elapsed());
            if !delay.is_zero(){std::thread::sleep(delay);}self.frame_at=std::time::Instant::now();
            let t=self.start.elapsed().as_secs_f64();self.frames+=1;
            self.app.settings.island_lyrics=t>=360.0 || (t*2.0) as u64%2==0;
            let env=Env{t,clock:"00:49:00".into(),today:"2026-11-04".into(),battery:Some(80),charging:true,
                media:daozai::smtc::NowPlaying{source:"QQMusic.exe".into(),title:"性能检查".into(),artist:"测试".into(),duration:450.0,position:t,playing:true,playback_type:daozai::smtc::MediaKind::Music,..Default::default()},..Default::default()};
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{
                let out=self.app.frame(ui,egui::Rect::from_min_size(ui.max_rect().min,egui::vec2(WIN_W,WIN_H)),&env);
                self.min_width=self.min_width.min(out.hit.width());self.max_width=self.max_width.max(out.hit.width());
                let r=|r:egui::Rect|(r.min.x,r.min.y,r.width(),r.height());
                self.native.update(r(out.region),r(out.hit),out.notice_hit.map(r),out.file_drop_zone.map(r),out.glass.as_deref(),ctx.pixels_per_point(),false,out.expanded);
            });
            // 首帧建立媒体来源后再注入歌词，避免被当成上一播放器的结果清空。
            if self.frames==1 {self.app.set_lyrics("性能检查","测试",450.0,daozai::lyrics::parse_lrc("[00:00]灰色的夜\n[06:00]还在眼前"));}
            let textures=ctx.tex_manager().read().num_allocated();
            if t>=10.0 && self.textures.is_none(){self.textures=Some(textures);}
            if t>=420.0 {
                println!("seconds={t:.1} frames={} textures_before={} textures_after={textures}",self.frames,self.textures.unwrap());
                assert_eq!(textures,self.textures.unwrap(),"反复伸缩不能持续增加纹理");
                assert!(self.max_width-self.min_width>50.0,"检查期间必须实际发生岛体伸缩");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
    std::env::set_var("DAOZAI_DATA_DIR",std::env::temp_dir().join(format!("daozai-render-soak-{}",std::process::id())));
    let options=eframe::NativeOptions{vsync:false,viewport:egui::ViewportBuilder::default().with_title("岛仔性能检查").with_inner_size([daozai::app::CANVAS_W,WIN_H]).with_position([100.0,100.0]).with_decorations(false).with_transparent(true).with_active(false),..Default::default()};
    eframe::run_native("岛仔性能检查",options,Box::new(|cc|{
        cc.egui_ctx.set_pixels_per_point(1.0);
        if let Some(gl)=&cc.gl{use eframe::glow::HasContext;println!("renderer={}",unsafe{gl.get_parameter_string(eframe::glow::RENDERER)});}
        let app=App::new(&cc.egui_ctx,Options{fetch_lyrics:false,pet_idle:1e9});
        let native=daozai::native::Window::new(cc).unwrap();
        Ok(Box::new(Probe{app,native,start:std::time::Instant::now(),frame_at:std::time::Instant::now(),textures:None,frames:0,min_width:f32::INFINITY,max_width:0.0}))
    }))
}
#[cfg(not(windows))]
fn main(){}
