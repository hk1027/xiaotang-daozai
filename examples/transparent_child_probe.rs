//! 验证 Windows 驱动报告不支持透明时，原生子窗口仍保留逐像素透明。
#[cfg(windows)]
fn main()->eframe::Result<()> {
    use eframe::egui;
    use windows::Win32::{Foundation::*,Graphics::Gdi::*,UI::WindowsAndMessaging::*};
    struct Probe {back:HWND,start:std::time::Instant,checked:bool}
    impl Drop for Probe {fn drop(&mut self){unsafe{if !self.back.0.is_null(){let _=DestroyWindow(self.back);}}}}
    impl eframe::App for Probe {
        fn clear_color(&self,_:&egui::Visuals)->[f32;4]{[0.0;4]}
        fn update(&mut self,ctx:&egui::Context,_:&mut eframe::Frame){
            ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("alpha-child"),egui::ViewportBuilder::default().with_title("透明子窗口检查").with_inner_size([320.0,160.0]).with_position([450.0,100.0]).with_decorations(false).with_transparent(true).with_active(false).with_always_on_top(),|child,_|{
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(child,|ui|{
                    ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(60.0,30.0),egui::pos2(260.0,130.0)),25.0,egui::Color32::BLACK);
                });
                unsafe {
                    if let Ok(hwnd)=FindWindowW(windows::core::PCWSTR::null(),windows::core::w!("透明子窗口检查")) {
                        let mut r=RECT::default();GetWindowRect(hwnd,&mut r).unwrap();
                        if self.back.0.is_null(){
                            self.back=CreateWindowExW(WS_EX_NOACTIVATE|WS_EX_TOOLWINDOW,windows::core::w!("STATIC"),windows::core::w!(""),WS_POPUP|WS_VISIBLE|WINDOW_STYLE(6),r.left,r.top,r.right-r.left,r.bottom-r.top,None,None,None,None).unwrap();
                            SetWindowPos(self.back,hwnd,r.left,r.top,r.right-r.left,r.bottom-r.top,SWP_NOACTIVATE).unwrap();
                            self.start=std::time::Instant::now();
                        }
                        if !self.checked && self.start.elapsed().as_secs()>=2 {
                            let mut p=POINT::default();let _=ClientToScreen(hwnd,&mut p);
                            let dc=GetDC(None);let outside=GetPixel(dc,p.x+10,p.y+10).0;let inside=GetPixel(dc,p.x+160,p.y+80).0;ReleaseDC(None,dc);
                            println!("child outside={outside:06x} inside={inside:06x}");
                            assert_eq!(outside,0xffffff,"子窗口透明区应透出白色背景");assert_eq!(inside,0,"子窗口黑色主体应保留");self.checked=true;
                        }
                    }
                }
            });
            if self.start.elapsed().as_secs()>=5 {assert!(self.checked,"子窗口检查未执行");ctx.send_viewport_cmd(egui::ViewportCommand::Close);}
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
    eframe::run_native("子窗口合成检查",eframe::NativeOptions{viewport:egui::ViewportBuilder::default().with_inner_size([1.0,1.0]).with_position([-1000.0,-1000.0]).with_active(false),..Default::default()},Box::new(|_|Ok(Box::new(Probe{back:HWND::default(),start:std::time::Instant::now(),checked:false}))))
}
#[cfg(not(windows))]
fn main(){}
