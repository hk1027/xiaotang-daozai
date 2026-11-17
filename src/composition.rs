//! 毛玻璃皮肤的实时背景模糊：Windows 合成引擎（Windows.UI.Composition）。
//!
//! 早先用的 `SetWindowCompositionAttribute` 模糊总是铺满整个窗口矩形、不认窗口区域，
//! 而岛仔窗口是一大块透明画布，于是桌面顶部出现一整块深色方块。
//!
//! 现在的做法：单独建一个只承载合成视觉的「模糊底板」窗口（无重定向位图、不接收鼠标、
//! 不出现在任务栏），与岛仔窗口同位置同大小，并始终紧贴在它**正下方**。底板上的每块视觉
//! 都用宿主背景画刷（Windows 11 亚克力用的系统实时模糊），主体按圆角裁剪，顶部外翻的
//! 肩部按物理像素逐行拼出，模糊因此严格贴合岛体轮廓；岛体的着色、高光与文字仍由 egui
//! 画在上层的岛仔窗口里。（直接挂在岛仔窗口上时，部分显卡会把视觉层叠到内容之上。）

use crate::app::BlurPiece;
use windows::core::Interface;
use windows::Foundation::Numerics::{Vector2, Vector3};
use windows::System::DispatcherQueueController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{CompositionBrush, CompositionRoundedRectangleGeometry, Compositor, ContainerVisual, SpriteVisual, VisualCollection};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_HOSTBACKDROPBRUSH};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{CreateDispatcherQueueController, DispatcherQueueOptions, RoInitialize, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, RO_INIT_SINGLETHREADED};

/// 底板窗口：鼠标一律穿透、不激活。
unsafe extern "system" fn backdrop_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn create_backdrop_window() -> windows::core::Result<HWND> {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    static CLASS: std::sync::Once = std::sync::Once::new();
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = windows::core::w!("DaozaiGlassBackdrop");
    CLASS.call_once(|| unsafe {
        let wc = WNDCLASSW { lpfnWndProc: Some(backdrop_proc), hInstance: instance.into(), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
    });
    const WS_EX_NOREDIRECTIONBITMAP: WINDOW_EX_STYLE = WINDOW_EX_STYLE(0x0020_0000);
    unsafe {
        CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            class,
            windows::core::w!(""),
            WS_POPUP,
            0, 0, 1, 1,
            None, None, instance, None,
        )
    }
}

fn enable_host_backdrop(hwnd: HWND) -> windows::core::Result<()> {
    unsafe {
        let on = BOOL::from(true);
        DwmSetWindowAttribute(hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, (&on as *const BOOL).cast(), std::mem::size_of::<BOOL>() as u32)
    }
}

/// 把底板窗口裁成模糊块的并集。底板和岛仔画布一样大，而 `HTTRANSPARENT`
/// 只把点击让给同一线程的窗口：不裁剪时，画布范围内（含岛右侧和下方大片空白）
/// 其他程序的标题栏按钮等都会点不动，直到退出岛仔。
///
/// 裁剪只负责让出点击，可见轮廓由各模糊块自己的圆角裁剪决定，所以按 `CLIP_GRID` 像素向外取整：
/// 动画和歌词宽度变化时不必每帧重设窗口区域。每次重设都会让合成器重建这扇窗口，
/// 在部分核显旧驱动上频繁重设会拖垮合成器（整机闪屏后卡死）。
const CLIP_GRID: i32 = 8;

fn clip_rects(pieces: &[BlurPiece]) -> Vec<(i32, i32, i32, i32)> {
    let down = |v: f32| (v.floor() as i32).div_euclid(CLIP_GRID) * CLIP_GRID;
    let up = |v: f32| -((-(v.ceil() as i32)).div_euclid(CLIP_GRID) * CLIP_GRID);
    let mut rects: Vec<_> = pieces.iter().filter(|p| p.w > 0.0 && p.h > 0.0)
        .map(|p| (down(p.x), down(p.y), up(p.x + p.w), up(p.y + p.h))).collect();
    rects.sort_unstable();
    rects.dedup();
    rects
}

/// 区域没变时不调用系统；返回是否真的重设了区域。
fn clip_to_pieces(hwnd: HWND, pieces: &[BlurPiece], applied: &mut Option<Vec<(i32, i32, i32, i32)>>) -> bool {
    use windows::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, RGN_OR};
    let rects = clip_rects(pieces);
    if applied.as_ref() == Some(&rects) { return false; }
    unsafe {
        let all = CreateRectRgn(0, 0, 0, 0);
        if all.0.is_null() { return false; }
        for &(l, t, r, b) in &rects {
            let part = CreateRectRgn(l, t, r, b);
            if part.0.is_null() { continue; }
            let _ = CombineRgn(all, all, part, RGN_OR);
            let _ = DeleteObject(part);
        }
        // 底板没有重定向位图，内容全由合成视觉提供，不需要系统重画。成功后区域归系统所有，不能再删除。
        if SetWindowRgn(hwnd, all, false) == 0 { let _ = DeleteObject(all); return false; }
    }
    *applied = Some(rects);
    true
}

pub struct Backdrop {
    hwnd: HWND,
    placed: (RECT, bool),
    // 合成器依赖当前线程的 DispatcherQueue；控制器必须与合成器同生命周期。
    _queue: Option<DispatcherQueueController>,
    compositor: Compositor,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    children: VisualCollection,
    brush: CompositionBrush,
    pool: Vec<(SpriteVisual, CompositionRoundedRectangleGeometry)>,
    shown: Vec<BlurPiece>,
    clip: Option<Vec<(i32, i32, i32, i32)>>,
    visible: bool,
}

impl Backdrop {
    pub fn new() -> windows::core::Result<Self> {
        let hwnd = create_backdrop_window()?;
        // Win10 不支持这个 Win32 宿主背景属性。失败时不能继续绘制纯色画刷，
        // 交给上层使用半透明底色，并释放尚未显示的底板窗口。
        if let Err(error) = enable_host_backdrop(hwnd) {
            unsafe { let _ = DestroyWindow(hwnd); }
            return Err(error);
        }
        unsafe {
            // 主线程已由窗口框架初始化为单线程套间时返回 S_FALSE，忽略即可。
            let _ = RoInitialize(RO_INIT_SINGLETHREADED);
        }
        let queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            })
        }
        .ok();
        let compositor = Compositor::new()?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        // 底板窗口没有自己的内容，视觉树就是它的全部画面。
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, BOOL::from(true)) }?;
        let root = compositor.CreateContainerVisual()?;
        target.SetRoot(&root)?;
        let children = root.Children()?;
        let brush: CompositionBrush = compositor.CreateHostBackdropBrush()?.cast()?;
        root.SetIsVisible(false)?;
        Ok(Self { hwnd, placed: (RECT::default(), false), _queue: queue, compositor, _target: target, root, children, brush, pool: Vec::new(), shown: Vec::new(), clip: None, visible: false })
    }

    /// 让底板与岛仔窗口重合，并紧贴在它正下方。
    fn follow(&mut self, island: HWND) {
        unsafe {
            let mut rect = RECT::default();
            if GetWindowRect(island, &mut rect).is_err() { return; }
            let below = GetWindow(island, GW_HWNDNEXT).ok() == Some(self.hwnd);
            if self.placed.0 != rect || !below || !self.placed.1 {
                let _ = SetWindowPos(self.hwnd, island, rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, SWP_NOACTIVATE | SWP_SHOWWINDOW);
                self.placed = (rect, true);
            }
        }
    }

    /// 显示并更新模糊块（物理像素，岛仔窗口坐标）。形状不变时不提交任何改动。
    pub fn show(&mut self, island: HWND, pieces: &[BlurPiece]) -> windows::core::Result<()> {
        self.follow(island);
        if pieces != self.shown.as_slice() {
            while self.pool.len() < pieces.len() {
                let visual = self.compositor.CreateSpriteVisual()?;
                visual.SetBrush(&self.brush)?;
                let geometry = self.compositor.CreateRoundedRectangleGeometry()?;
                let clip = self.compositor.CreateGeometricClipWithGeometry(&geometry)?;
                visual.SetClip(&clip)?;
                self.children.InsertAtTop(&visual)?;
                self.pool.push((visual, geometry));
            }
            for (i, (visual, geometry)) in self.pool.iter().enumerate() {
                match pieces.get(i) {
                    Some(p) => {
                        let size = Vector2 { X: p.w, Y: p.h };
                        visual.SetOffset(Vector3 { X: p.x, Y: p.y, Z: 0.0 })?;
                        visual.SetSize(size)?;
                        geometry.SetSize(size)?;
                        geometry.SetCornerRadius(Vector2 { X: p.radius, Y: p.radius })?;
                        visual.SetIsVisible(true)?;
                    }
                    None => visual.SetIsVisible(false)?,
                }
            }
            clip_to_pieces(self.hwnd, pieces, &mut self.clip);
            self.shown = pieces.to_vec();
        }
        if !self.visible {
            self.root.SetIsVisible(true)?;
            self.visible = true;
        }
        Ok(())
    }

    pub fn hide(&mut self) {
        if self.visible {
            let _ = self.root.SetIsVisible(false);
            self.visible = false;
        }
        if self.placed.1 {
            unsafe { let _ = ShowWindow(self.hwnd, SW_HIDE); }
            self.placed.1 = false;
        }
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        unsafe { let _ = DestroyWindow(self.hwnd); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, GetWindowRgn, PtInRegion};

    #[test]
    fn regression_rejected_host_backdrop_is_not_reported_as_working_blur() {
        assert!(enable_host_backdrop(HWND::default()).is_err(),"DWM 拒绝宿主背景画刷时必须启用透明回退，不能把纯色底板当成模糊成功");
    }

    #[test]
    fn backdrop_initialization_respects_system_support_and_starts_hidden() {
        let hwnd=create_backdrop_window().unwrap();
        let supported=enable_host_backdrop(hwnd).is_ok();
        unsafe { let _=DestroyWindow(hwnd); }
        match Backdrop::new() {
            Ok(backdrop)=>{
                assert!(supported);
                assert!(!unsafe{IsWindowVisible(backdrop.hwnd)}.as_bool(),"模糊块准备好之前不能显示矩形底板");
            }
            Err(error)=>assert!(!supported,"支持宿主背景画刷时不应初始化失败：{error}"),
        }
    }

    #[test]
    fn backdrop_only_covers_the_blur_pieces_so_other_apps_stay_clickable() {
        let hwnd = create_backdrop_window().unwrap();
        unsafe {
            let _ = SetWindowPos(hwnd, None, 0, 0, 960, 270, SWP_NOACTIVATE | SWP_NOZORDER);
            let mut applied = None;
            assert!(clip_to_pieces(hwnd, &[BlurPiece { x: 230.0, y: 0.0, w: 180.0, h: 34.0, radius: 12.0 }], &mut applied));
            let region = CreateRectRgn(0, 0, 0, 0);
            assert_ne!(GetWindowRgn(hwnd, region).0, 0, "底板必须带窗口区域");
            assert!(PtInRegion(region, 320, 17).as_bool(), "岛体下方仍要显示模糊");
            assert!(!PtInRegion(region, 800, 150).as_bool(), "画布右侧空白处必须让给下层程序");
            assert!(!PtInRegion(region, 320, 200).as_bool(), "岛下方空白处必须让给下层程序");
            assert!(clip_to_pieces(hwnd, &[], &mut applied));
            GetWindowRgn(hwnd, region);
            assert!(!PtInRegion(region, 320, 17).as_bool(), "没有模糊块时不能占任何区域");
            let _ = DeleteObject(region);
            let _ = DestroyWindow(hwnd);
        }
    }

    #[test]
    fn animation_frames_do_not_reset_the_backdrop_region_every_frame() {
        // 展开 / 歌词宽度变化时轮廓每帧只差零点几到一两个像素：大多数帧不应重设窗口区域。
        let hwnd = create_backdrop_window().unwrap();
        let mut applied = None;
        let mut resets = 0;
        for i in 0..60 {
            let w = 180.0 + i as f32 * 0.5;
            if clip_to_pieces(hwnd, &[BlurPiece { x: 320.0 - w / 2.0, y: 0.0, w, h: 34.0, radius: 12.0 }], &mut applied) { resets += 1; }
        }
        assert!(resets <= 10, "60 帧细微变化重设了 {resets} 次窗口区域");
        let rects = applied.unwrap();
        assert!(rects.iter().all(|r| r.0 % CLIP_GRID == 0 && r.2 % CLIP_GRID == 0), "区域应按网格取整");
        unsafe { let _ = DestroyWindow(hwnd); }
    }
}
