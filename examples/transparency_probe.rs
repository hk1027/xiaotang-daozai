//! 本机合成回归探针：模拟创建时未启用透明，检查原生初始化能否恢复。
#[cfg(windows)]
fn main()->eframe::Result<()> {
    use eframe::egui;
    use raw_window_handle::{HasWindowHandle,RawWindowHandle};
    use windows::Win32::{Foundation::*,Graphics::Gdi::*,UI::WindowsAndMessaging::*};
    struct Probe {native:daozai::native::Window,back:HWND,hwnd:HWND,start:std::time::Instant,checked:bool,reset:usize}
    impl Drop for Probe {fn drop(&mut self){unsafe{let _=DestroyWindow(self.back);}}}
    impl eframe::App for Probe {
        fn clear_color(&self,_:&egui::Visuals)->[f32;4]{[0.0;4]}
        fn update(&mut self,ctx:&egui::Context,_:&mut eframe::Frame){
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{
                ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(60.0,30.0),egui::pos2(260.0,130.0)),25.0,egui::Color32::BLACK);
            });
            self.native.update((0.0,0.0,320.0,160.0),(60.0,30.0,200.0,100.0),None,None,None,ctx.pixels_per_point(),false,false);
            // 隐藏创建的窗口首次显示时可能被系统重新定位，白色测试背景需跟随实际位置和层级。
            unsafe {
                let mut rect=RECT::default();GetWindowRect(self.hwnd,&mut rect).unwrap();
                SetWindowPos(self.back,self.hwnd,rect.left,rect.top,rect.right-rect.left,rect.bottom-rect.top,SWP_NOACTIVATE).unwrap();
            }
            if !self.checked && self.start.elapsed().as_secs()>=2 {
                unsafe {
                    let mut origin=POINT::default();let _=ClientToScreen(self.hwnd,&mut origin);
                    let dc=GetDC(None);
                    let outside=GetPixel(dc,origin.x+10,origin.y+10).0;
                    let inside=GetPixel(dc,origin.x+160,origin.y+80).0;
                    ReleaseDC(None,dc);
                    println!("outside={outside:06x} inside={inside:06x}");
                    assert_eq!(outside,0x00ffffff,"透明区域应透出背后的白色窗口");
                    assert_eq!(inside,0,"黑色岛体不能被当作透明色挖掉");
                }
                self.checked=true;
            }
            if self.checked && self.reset<4 && self.start.elapsed().as_secs()>=3+self.reset as u64*2 {
                unsafe {
                    use windows::Win32::Graphics::Dwm::*;
                    DwmEnableBlurBehindWindow(self.hwnd,&DWM_BLURBEHIND{dwFlags:DWM_BB_ENABLE,..Default::default()}).unwrap();
                    if self.reset==3 {
                        let _=ShowWindow(self.hwnd,SW_HIDE);
                        let _=ShowWindow(self.hwnd,SW_SHOWNOACTIVATE);
                    }else{
                        SendMessageW(self.hwnd,[WM_DWMCOMPOSITIONCHANGED,WM_DISPLAYCHANGE,WM_DWMCOLORIZATIONCOLORCHANGED][self.reset],WPARAM(0),LPARAM(0));
                    }
                }
                self.reset+=1;self.checked=false;
            }
            if self.start.elapsed().as_secs()>=12 {assert!(self.checked && self.reset==4);ctx.send_viewport_cmd(egui::ViewportCommand::Close);}
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
    let hidden_start=std::env::args().any(|arg|arg=="--hidden-start");
    let opts=eframe::NativeOptions{viewport:egui::ViewportBuilder::default().with_title("岛仔透明合成回归检查").with_inner_size([320.0,160.0]).with_position([100.0,100.0]).with_decorations(false).with_transparent(std::env::args().any(|arg|arg=="--transparent")).with_visible(!hidden_start).with_active(false),..Default::default()};
    eframe::run_native("透明合成回归检查",opts,Box::new(move |cc|{
        cc.egui_ctx.set_pixels_per_point(1.0);
        let RawWindowHandle::Win32(raw)=cc.window_handle().unwrap().as_raw() else {unreachable!()};
        let hwnd=HWND(raw.hwnd.get() as *mut _);
        let mut rect=RECT::default();
        let back=unsafe {
            GetWindowRect(hwnd,&mut rect).unwrap();
            CreateWindowExW(WS_EX_NOACTIVATE|WS_EX_TOOLWINDOW,windows::core::w!("STATIC"),windows::core::w!(""),WS_POPUP|WS_VISIBLE|WINDOW_STYLE(6),rect.left,rect.top,rect.right-rect.left,rect.bottom-rect.top,None,None,None,None).unwrap()
        };
        let native=daozai::native::Window::new(cc).unwrap();
        if hidden_start {unsafe{
            use windows::Win32::Graphics::Dwm::*;
            DwmEnableBlurBehindWindow(hwnd,&DWM_BLURBEHIND{dwFlags:DWM_BB_ENABLE,..Default::default()}).unwrap();
            let _=ShowWindow(hwnd,SW_SHOWNOACTIVATE);
        }}
        Ok(Box::new(Probe{native,back,hwnd,start:std::time::Instant::now(),checked:false,reset:0}))
    }))
}
#[cfg(not(windows))]
fn main(){}
