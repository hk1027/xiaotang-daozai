//! 系统文件缩略图 / 图标。
//!
//! 文件夹用系统图标，其余文件优先用 `IShellItemImageFactory` 取 96 px 内容缩略图，
//! 失败再用 `SHGetFileInfoW` 取 32 px 大图标；都失败返回 `None`，界面改画矢量文件图标。

use eframe::egui::{self,ColorImage};
use std::path::{Path,PathBuf};
use std::collections::{HashMap,HashSet};
use std::sync::mpsc::{self,Receiver,SyncSender};

#[derive(Clone)]
pub(crate) struct Preview {pub image:Option<egui::TextureHandle>,pub metadata:String}
struct Loaded {image:Option<ColorImage>,metadata:String}
type ResultItem=(PathBuf,Loaded);

#[derive(Default)]
pub(crate) struct Cache {
    entries:HashMap<PathBuf,(Preview,u64)>,pending:HashSet<PathBuf>,
    worker:Option<(SyncSender<PathBuf>,Receiver<ResultItem>)>,generation:u64,
}

impl Cache {
    fn start(&mut self,ctx:egui::Context,load:impl Fn(&Path)->Loaded+Send+'static){
        let(tx,requests)=mpsc::sync_channel::<PathBuf>(8);let(results,rx)=mpsc::sync_channel(8);
        std::thread::Builder::new().name("daozai-thumbnails".into()).spawn(move||{
            #[cfg(windows)] let initialized=unsafe{windows::Win32::System::Com::CoInitializeEx(None,windows::Win32::System::Com::COINIT_APARTMENTTHREADED).is_ok()};
            while let Ok(path)=requests.recv(){
                let preview=load(&path);
                if results.send((path,preview)).is_err(){break;}
                ctx.request_repaint();
            }
            #[cfg(windows)] if initialized{unsafe{windows::Win32::System::Com::CoUninitialize();}}
        }).ok();
        self.worker=Some((tx,rx));
    }
    pub fn get(&mut self,ctx:&egui::Context,path:&Path)->Option<Preview>{
        if self.worker.is_none(){self.start(ctx.clone(),load_preview);}
        let(tx,rx)=self.worker.as_ref().unwrap();
        while let Ok((path,loaded))=rx.try_recv(){
            if !self.pending.remove(&path){continue;}
            if self.entries.len()>=32{
                if let Some(oldest)=self.entries.iter().min_by_key(|(_,(_,used))|used).map(|(path,_)|path.clone()){self.entries.remove(&oldest);}
            }
            let image=loaded.image.map(|image|ctx.load_texture(format!("shelf:{}",path.display()),image,egui::TextureOptions::LINEAR));
            self.entries.insert(path,(Preview{image,metadata:loaded.metadata},self.generation));
        }
        self.generation=self.generation.wrapping_add(1);
        if let Some((preview,used))=self.entries.get_mut(path){*used=self.generation;return Some(preview.clone());}
        if !self.pending.contains(path) && tx.try_send(path.to_path_buf()).is_ok(){self.pending.insert(path.to_path_buf());}
        None
    }
    pub fn retain(&mut self,paths:&[PathBuf]){
        self.entries.retain(|path,_|paths.contains(path));self.pending.retain(|path|paths.contains(path));
    }
    pub fn remove(&mut self,path:&Path){self.entries.remove(path);self.pending.remove(path);}
    pub fn clear(&mut self){self.entries.clear();self.pending.clear();}
}

fn load_preview(path:&Path)->Loaded {
    let info=std::fs::metadata(path);
    let is_dir=info.as_ref().is_ok_and(|metadata|metadata.is_dir());
    let metadata=info.map(|metadata|if is_dir{"文件夹".into()}else{crate::model::human_size(metadata.len())}).unwrap_or_else(|_|"已移动".into());
    Loaded{image:file_icon(path,is_dir),metadata}
}

fn preview_image(width:usize,height:usize,pixels:Vec<u8>)->Option<ColorImage>{
    let image=image::RgbaImage::from_raw(width as u32,height as u32,pixels)?;
    let image=image::DynamicImage::ImageRgba8(image).thumbnail(96,96).to_rgba8();
    Some(ColorImage::from_rgba_premultiplied([image.width()as usize,image.height()as usize],image.as_raw()))
}

#[cfg(windows)]
fn file_icon(path: &Path, is_dir: bool) -> Option<ColorImage> {
    use windows::Win32::UI::Shell::{SIIGBF_ICONONLY,SIIGBF_RESIZETOFIT,SIIGBF_BIGGERSIZEOK};
    if !path.exists() {
        return None;
    }
    let flags=if is_dir{SIIGBF_ICONONLY}else{SIIGBF_RESIZETOFIT|SIIGBF_BIGGERSIZEOK};
    if let Some((w, h, px)) = thumbnail(path, 96, flags) {
        return preview_image(w,h,px);
    }
    shell_icon(path)
}

#[cfg(windows)]
fn thumbnail(path: &Path, size: i32, flags: windows::Win32::UI::Shell::SIIGBF) -> Option<(usize, usize, Vec<u8>)> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName,
    };

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
        let hbmp = factory.GetImage(SIZE { cx: size, cy: size }, flags).ok()?;
        let mut bm = BITMAP::default();
        let got = GetObjectW(
            HGDIOBJ(hbmp.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut BITMAP as *mut core::ffi::c_void),
        );
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        if got == 0 || w <= 0 || h <= 0 || w > 1024 || h > 1024 {
            let _ = DeleteObject(HGDIOBJ(hbmp.0));
            return None;
        }
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let lines = GetDIBits(dc, hbmp, 0, h as u32, Some(px.as_mut_ptr() as *mut core::ffi::c_void), &mut bmi, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        if lines == 0 {
            return None;
        }
        // BGRA（预乘）→ RGBA；JPEG 之类的缩略图 alpha 全 0，当作不透明
        let opaque = px.chunks_exact(4).all(|p| p[3] == 0);
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
            if opaque {
                p[3] = 255;
            }
        }
        Some((w as usize, h as usize, px))
    }
}

#[cfg(windows)]
fn shell_icon(path: &Path) -> Option<ColorImage> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL};

    const SIZE: i32 = 32;
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let mut info = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if ok == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let icon = info.hIcon;
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let rgba = unsafe {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            let _ = DestroyIcon(icon);
            return None;
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE,
                biHeight: -SIZE, // 自上而下
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        let mut out = None;
        if let Ok(bmp) = bmp {
            if !bits.is_null() {
                let old = SelectObject(dc, HGDIOBJ(bmp.0));
                let _ = DrawIconEx(dc, 0, 0, icon, SIZE, SIZE, 0, None, DI_NORMAL);
                SelectObject(dc, old);
                // 图标是 32 位 BGRA（直通 alpha）
                let n = (SIZE * SIZE) as usize;
                let src = std::slice::from_raw_parts(bits as *const u8, n * 4);
                out = Some(src.to_vec());
            }
            let _ = DeleteObject(HGDIOBJ(bmp.0));
        }
        let _ = DeleteDC(dc);
        let _ = DestroyIcon(icon);
        out
    };
    let mut src = rgba?;
    // BGRA -> RGBA
    for px in src.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let img = ColorImage::from_rgba_unmultiplied([SIZE as usize, SIZE as usize], &src);
    Some(img)
}

#[cfg(not(windows))]
fn file_icon(_path: &Path, _is_dir: bool) -> Option<ColorImage> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration,Instant};
    #[cfg(windows)]
    #[test]
    fn folders_use_system_icons_and_images_keep_content_thumbnails(){
        use windows::Win32::System::Com::{CoInitializeEx,CoUninitialize,COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::Shell::{SIIGBF_ICONONLY,SIIGBF_RESIZETOFIT,SIIGBF_BIGGERSIZEOK};
        let initialized=unsafe{CoInitializeEx(None,COINIT_APARTMENTTHREADED).is_ok()};
        let dir=std::env::temp_dir().join(format!("daozai-folder-icons-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&dir).unwrap();
        let picture=dir.join("preview.png");
        image::RgbaImage::from_pixel(80,40,image::Rgba([230,70,40,255])).save(&picture).unwrap();
        let cases=[(&dir,SIIGBF_ICONONLY),(&picture,SIIGBF_RESIZETOFIT|SIIGBF_BIGGERSIZEOK)];
        let previews:Vec<_>=cases.into_iter().map(|(path,flags)|{
            let (w,h,pixels)=thumbnail(path,96,flags).unwrap();
            (load_preview(path).image.unwrap(),preview_image(w,h,pixels).unwrap())
        }).collect();
        std::fs::remove_file(&picture).unwrap();std::fs::remove_dir(&dir).unwrap();
        if initialized{unsafe{CoUninitialize();}}
        for ((actual,expected),kind) in previews.into_iter().zip(["文件夹系统图标","图片内容缩略图"]){
            assert!(actual.size[0]<=96&&actual.size[1]<=96,"{kind}不能上传大图");
            assert_eq!(actual.size,expected.size,"{kind}尺寸不符");
            assert!(actual.pixels==expected.pixels,"{kind}应使用对应的系统图像");
        }
    }

    #[test]
    fn oversized_shell_thumbnails_are_reduced_before_upload(){
        let preview=preview_image(1024,512,vec![255;1024*512*4]).unwrap();
        assert_eq!(preview.size,[96,48]);
        assert!(preview.pixels.len()<=96*96,"缩略图插件返回大图时也必须限制单项内存");
    }
    #[test]
    fn slow_preview_keeps_ui_nonblocking_and_requests_bounded(){
        let ctx=egui::Context::default();let mut cache=Cache::default();
        let(started,ready)=mpsc::channel();let(release,blocked)=mpsc::channel();
        cache.start(ctx.clone(),move|_|{
            let _=started.send(());let _=blocked.recv();
            Loaded{image:None,metadata:"文件夹".into()}
        });
        let path=Path::new("blocked-preview");
        assert!(cache.get(&ctx,path).is_none());
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let at=Instant::now();
        for _ in 0..100 {assert!(cache.get(&ctx,path).is_none());}
        assert!(at.elapsed()<Duration::from_millis(200),"慢磁盘或缩略图插件不能阻塞界面");
        assert_eq!(cache.pending.len(),1,"同一文件不能反复排队");
        for i in 0..100{cache.get(&ctx,Path::new(&format!("queued-{i}")));}
        assert!(cache.pending.len()<=9,"在途请求和排队请求必须有上限");
        release.send(()).unwrap();
    }

    #[test]
    fn preview_cache_bounds_textures_and_releases_them_when_cleared(){
        let ctx=egui::Context::default();let mut cache=Cache::default();
        cache.start(ctx.clone(),|_|Loaded{image:Some(ColorImage::new([8,8],egui::Color32::WHITE)),metadata:"1 KB".into()});
        for i in 0..40{
            let path=PathBuf::from(format!("preview-{i}"));let deadline=Instant::now()+Duration::from_secs(2);
            while cache.get(&ctx,&path).is_none(){assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(1));}
        }
        assert_eq!(cache.entries.len(),32);
        assert!(ctx.tex_manager().read().num_allocated()<=33,"滚动大量文件不能无限保留 GPU 缩略图");
        cache.clear();
        assert!(ctx.tex_manager().read().num_allocated()<=1,"清空置物架后应释放缩略图纹理");
    }
}
