//! 在屏幕外测量调节大小前后的翻页渲染，覆盖设置同时打开与关闭的情况。
use daozai::app::{App,Env,Options,CANVAS_W,WIN_W,WIN_H};
use eframe::egui;
use std::time::Instant;

struct Probe {app:App,frame:usize,last:Instant,intervals:Vec<f64>,textures:usize,bytes:usize}

impl eframe::App for Probe {
    fn update(&mut self,ctx:&egui::Context,_:&mut eframe::Frame){
        const FRAMES:usize=180;
        let cases=[("original",1.0,false),("enlarged-settings",1.4,true),("reset-settings",1.0,true),("enlarged-closed",1.4,false)];
        let stage=self.frame/FRAMES;let offset=self.frame%FRAMES;
        if stage>=cases.len(){ctx.send_viewport_cmd(egui::ViewportCommand::Close);return;}
        let (name,scale,settings)=cases[stage];
        let now=Instant::now();let interval=now.duration_since(self.last).as_secs_f64()*1000.0;self.last=now;
        if offset==0{
            self.intervals.clear();self.textures=0;self.bytes=0;
            eframe::set_root_viewport_scale(scale);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(CANVAS_W*scale,WIN_H*scale)));
            self.app.settings_open=settings;
        }
        if offset>=30{self.intervals.push(interval);}
        if offset%45==0{self.app.show_page((offset/45)%4);}
        let env=Env{t:self.frame as f64/60.0,clock:"09:41:00".into(),today:"2026-11-06".into(),date:"11月6日 周五".into(),cursor:Some(egui::pos2(WIN_W/2.0,60.0)),..Default::default()};
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{
            let win=egui::Rect::from_min_size(ui.max_rect().min,egui::vec2(WIN_W,WIN_H));
            self.app.frame(ui,win,&env);
        });
        self.app.settings_ui(ctx,false);
        if offset>=30{
            let textures=ctx.tex_manager();let textures=textures.read();
            let fonts:Vec<_>=textures.allocated().filter(|(_,meta)|meta.name.starts_with("egui_font_texture")).collect();
            self.textures=self.textures.max(fonts.len());self.bytes=self.bytes.max(fonts.iter().map(|(_,meta)|meta.bytes_used()).sum());
        }
        if offset==FRAMES-1{
            self.intervals.sort_by(f64::total_cmp);
            let mean=self.intervals.iter().sum::<f64>()/self.intervals.len() as f64;
            let p95=self.intervals[self.intervals.len()*95/100];
            println!("case={name} mean_ms={mean:.3} p95_ms={p95:.3} font_textures={} font_bytes={}",self.textures,self.bytes);
        }
        self.frame+=1;ctx.request_repaint();
    }
}

fn main()->eframe::Result<()> {
    let wgpu=std::env::args().any(|arg|arg=="--wgpu");
    let mut options=eframe::NativeOptions{vsync:false,viewport:egui::ViewportBuilder::default().with_title("岛仔缩放渲染检查").with_inner_size([CANVAS_W,WIN_H]).with_position([-10000.0,-10000.0]).with_active(false).with_taskbar(false),..Default::default()};
    if wgpu{
        options.renderer=eframe::Renderer::Wgpu;
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup)=&mut options.wgpu_options.wgpu_setup{
            setup.instance_descriptor.backends=eframe::wgpu::Backends::DX12;
        }
    }
    eframe::run_native("缩放渲染检查",options,Box::new(|cc|{
        cc.egui_ctx.on_begin_pass("检查窗口保持屏幕外",std::sync::Arc::new(|ctx|{
            if ctx.viewport_id()!=egui::ViewportId::ROOT{ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(-10000.0,-10000.0)));}
        }));
        let app=App::new(&cc.egui_ctx,Options{fetch_lyrics:false,pet_idle:1e9});
        Ok(Box::new(Probe{app,frame:0,last:Instant::now(),intervals:vec![],textures:0,bytes:0}))
    }))
}
