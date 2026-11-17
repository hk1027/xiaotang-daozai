//! 兼容渲染的逐像素透明：独立的分层显示窗口（`UpdateLayeredWindow`）。
//!
//! 兼容后端（Direct3D 12 交换链、部分 AMD OpenGL 驱动）交给系统的画面没有可用的 alpha，
//! 透明画布会显示成黑块，过去只能按岛体轮廓裁剪窗口区域；窗口区域逐像素「要么有要么没有」，
//! 圆角和肩部必然出现锯齿。
//!
//! 现在的做法：
//! - 画面变化时把渲染结果读回（已预乘 alpha），交给一个只负责显示的分层窗口，
//!   它紧贴在岛仔窗口正上方、鼠标一律穿透，边缘保留抗锯齿；
//! - 岛仔窗口本身设成整体不透明度 1/255 的分层窗口：交换链的画面几乎完全看不见，
//!   但仍按原来的窗口区域与命中区接收点击、悬停和拖放（不透明度为 0 时系统不再命中）。
//!   直接对岛仔窗口用 `UpdateLayeredWindow` 不行：翻转模型交换链的画面会盖过分层内容。

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// 岛仔窗口的整体不透明度：1/255 仍可命中，交换链画面肉眼不可见。
const OWNER_ALPHA: u8 = 1;

/// 读回画面的哈希：画面没变时不必重设分层窗口，也不再请求下一次读回。
pub fn image_hash(size: [usize; 2], rgba: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ ((size[0] as u64) << 32 | size[1] as u64);
    for p in rgba.chunks_exact(8) {
        h = (h ^ u64::from_le_bytes(p.try_into().unwrap())).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 画面自己在动（歌词滚动、动效）而没有用户输入时，两次读回的最小间隔：约 30 帧/秒。
/// 每次读回都要 GPU 回传整帧、换通道再交给系统合成，2 倍缩放下每帧约 2ms，60 帧/秒会持续占掉十几个百分点的单核。
pub const AMBIENT_READBACK: std::time::Duration = std::time::Duration::from_millis(33);

/// 这一帧要不要请求下一次读回：`None` 立即请求；`Some(d)` 先等 d 再重绘、到时再请求（保证最后一帧不会漏掉）。
/// 拖动、悬停、点击等有输入的帧保持满帧，跟手不打折扣。
pub fn readback_delay(last: Option<std::time::Instant>, now: std::time::Instant, interactive: bool) -> Option<std::time::Duration> {
    if interactive { return None; }
    let elapsed = now.saturating_duration_since(last?);
    (elapsed < AMBIENT_READBACK).then(|| AMBIENT_READBACK - elapsed)
}

/// 读回的画面里必须真的有透明像素（画布四角一定是透明的），否则说明这条路径读不到 alpha，
/// 不能切到分层窗口（整块画布会变成黑底）。
pub fn has_real_alpha(size: [usize; 2], rgba: &[u8]) -> bool {
    let [w, h] = size;
    if w == 0 || h == 0 || rgba.len() != w * h * 4 { return false; }
    let corner = |x: usize, y: usize| rgba[(y * w + x) * 4 + 3];
    [corner(0, 0), corner(w - 1, 0), corner(0, h - 1), corner(w - 1, h - 1)].iter().any(|&a| a < 255)
}

/// 读回画面没有 alpha 时（部分 AMD 显卡的 OpenGL 默认帧缓冲不带透明通道），按岛体轮廓
/// 算出每个像素被覆盖的比例当作 alpha：轮廓是物理像素坐标的闭合多边形，多块之间取并集。
/// 每行取 4 条子扫描线、横向按精确的线段长度累计，边缘过渡与 egui 自身的抗锯齿相当。
pub fn coverage_mask(size: [usize; 2], polygons: &[Vec<(f32, f32)>]) -> Vec<u8> {
    const SUB: usize = 4;
    let [w, h] = size;
    let mut mask = vec![0u8; w * h];
    let edges: Vec<Vec<((f32, f32), (f32, f32))>> = polygons.iter().filter(|p| p.len() >= 3)
        .map(|p| p.iter().zip(p.iter().cycle().skip(1)).map(|(a, b)| (*a, *b)).collect()).collect();
    let (top, bottom) = edges.iter().flatten().fold((f32::MAX, f32::MIN), |(t, b), (p, q)| (t.min(p.1).min(q.1), b.max(p.1).max(q.1)));
    if w == 0 || h == 0 || edges.is_empty() || !(top < bottom) { return mask; }
    let (y0, y1) = (top.floor().max(0.0) as usize, (bottom.ceil().max(0.0) as usize).min(h));
    let (mut row, mut part) = (vec![0f32; w + 1], vec![0f32; w + 1]);
    let mut xs: Vec<(f32, i32)> = Vec::new();
    let weight = 1.0 / SUB as f32;
    for y in y0..y1 {
        row.fill(0.0);
        for poly in &edges {
            part.fill(0.0);
            for s in 0..SUB {
                let sy = y as f32 + (s as f32 + 0.5) * weight;
                xs.clear();
                for &((ax, ay), (bx, by)) in poly {
                    if (ay <= sy) != (by <= sy) {
                        xs.push((ax + (sy - ay) / (by - ay) * (bx - ax), if by > ay { 1 } else { -1 }));
                    }
                }
                xs.sort_by(|a, b| a.0.total_cmp(&b.0));
                // 非零环绕：环绕数从 0 变非 0 处开始、回到 0 处结束，与 GDI 的 WINDING 区域一致。
                let (mut wind, mut start) = (0, 0.0f32);
                for &(x, dir) in &xs {
                    if wind == 0 { start = x; }
                    wind += dir;
                    if wind == 0 {
                        let (a, b) = (start.clamp(0.0, w as f32), x.clamp(0.0, w as f32));
                        if b <= a { continue; }
                        let (ia, ib) = (a as usize, b as usize);
                        if ia == ib { part[ia] += (b - a) * weight; continue; }
                        part[ia] += ((ia + 1) as f32 - a) * weight;
                        for v in &mut part[ia + 1..ib] { *v += weight; }
                        part[ib] += (b - ib as f32) * weight;
                    }
                }
            }
            for (r, p) in row.iter_mut().zip(&part) { *r = r.max(*p); }
        }
        for (m, r) in mask[y * w..(y + 1) * w].iter_mut().zip(&row) { *m = (r.min(1.0) * 255.0).round() as u8; }
    }
    mask
}

/// 把轮廓算出的 alpha 套到读回的画面上：画布清屏色是透明黑，读回的 RGB 本来就是「颜色 × 覆盖率」，
/// 再按预乘要求把每个通道压到不超过 alpha，轮廓外的像素全部清空。
pub fn apply_mask(rgba: &[u8], mask: &[u8], out: &mut Vec<u8>) {
    out.clear();
    out.extend(rgba.chunks_exact(4).zip(mask).flat_map(|(p, &a)| [p[0].min(a), p[1].min(a), p[2].min(a), a]));
}

/// 显示窗口：鼠标一律穿透、不激活。
unsafe extern "system" fn display_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn create_display_window() -> windows::core::Result<HWND> {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    static CLASS: std::sync::Once = std::sync::Once::new();
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = windows::core::w!("DaozaiLayeredDisplay");
    CLASS.call_once(|| unsafe {
        let wc = WNDCLASSW { lpfnWndProc: Some(display_proc), hInstance: instance.into(), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
    });
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            class,
            windows::core::w!(""),
            WS_POPUP,
            0, 0, 1, 1,
            None, None, instance, None,
        )
    }
}

/// 让岛仔窗口自身几乎不可见但仍可命中；winit 改样式时可能抹掉分层位，每帧兜底。
unsafe fn hide_owner(owner: HWND) -> windows::core::Result<()> {
    let ex = GetWindowLongPtrW(owner, GWL_EXSTYLE);
    if ex & WS_EX_LAYERED.0 as isize == 0 {
        SetWindowLongPtrW(owner, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
        SetLayeredWindowAttributes(owner, COLORREF(0), OWNER_ALPHA, LWA_ALPHA)?;
    }
    Ok(())
}

/// 层级上紧挨在下面的可见窗口（跳过输入法等隐藏的辅助窗口）。
unsafe fn next_visible(hwnd: HWND) -> Option<HWND> {
    let mut current = hwnd;
    while let Ok(next) = GetWindow(current, GW_HWNDNEXT) {
        if IsWindowVisible(next).as_bool() { return Some(next); }
        current = next;
    }
    None
}

pub struct Layered {
    display: HWND,
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    size: (i32, i32),
    last: Option<u64>,
}

impl Layered {
    pub fn new() -> windows::core::Result<Self> {
        let display = create_display_window()?;
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.is_invalid() {
            unsafe { let _ = DestroyWindow(display); }
            return Err(windows::core::Error::from_win32());
        }
        Ok(Self { display, dc, bitmap: HBITMAP::default(), old: HGDIOBJ::default(), bits: std::ptr::null_mut(), size: (0, 0), last: None })
    }

    pub(crate) fn display(&self) -> HWND { self.display }

    fn ensure_bitmap(&mut self, w: i32, h: i32) -> bool {
        if self.size == (w, h) && !self.bits.is_null() { return true; }
        unsafe {
            self.release_bitmap();
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // 自上而下
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(self.dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) else { return false };
            if bits.is_null() { let _ = DeleteObject(bitmap); return false; }
            self.old = SelectObject(self.dc, bitmap);
            self.bitmap = bitmap;
            self.bits = bits.cast();
            self.size = (w, h);
        }
        true
    }

    unsafe fn release_bitmap(&mut self) {
        if !self.bitmap.is_invalid() {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap);
        }
        self.bitmap = HBITMAP::default();
        self.bits = std::ptr::null_mut();
        self.size = (0, 0);
    }

    /// 把一帧（RGBA，已预乘）交给显示窗口。画面没变返回 Ok(false)；
    /// 尺寸与岛仔窗口当前大小不符（读回之后窗口又改了大小）时跳过，等下一帧。
    pub fn present(&mut self, owner: HWND, size: [usize; 2], rgba: &[u8]) -> windows::core::Result<bool> {
        let (w, h) = (size[0] as i32, size[1] as i32);
        let mut rect = RECT::default();
        unsafe { GetWindowRect(owner, &mut rect)?; }
        if (rect.right - rect.left, rect.bottom - rect.top) != (w, h) || rgba.len() != size[0] * size[1] * 4 { return Ok(false); }
        let hash = image_hash(size, rgba);
        if self.last == Some(hash) { return Ok(false); }
        if !self.ensure_bitmap(w, h) { return Err(windows::core::Error::from_win32()); }
        unsafe {
            let dst = std::slice::from_raw_parts_mut(self.bits, rgba.len());
            for (out, p) in dst.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
                out[0] = p[2]; out[1] = p[1]; out[2] = p[0]; out[3] = p[3];
            }
            let _ = GdiFlush();
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let (src, dst_pos, sz) = (POINT::default(), POINT { x: rect.left, y: rect.top }, SIZE { cx: w, cy: h });
            UpdateLayeredWindow(self.display, None, Some(&dst_pos), Some(&sz), self.dc, Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA)?;
            // 显示窗口有了内容之后才把岛仔窗口自身藏起来，避免切换瞬间什么都看不到。
            hide_owner(owner)?;
        }
        self.last = Some(hash);
        self.sync(owner, false);
        Ok(true)
    }

    /// 每帧跟随岛仔窗口：可见性、位置，以及紧贴在它正上方的层级。
    pub fn sync(&mut self, owner: HWND, raise: bool) {
        if self.last.is_none() { return; }
        unsafe {
            let _ = hide_owner(owner);
            let owner_visible = IsWindowVisible(owner).as_bool();
            let display_visible = IsWindowVisible(self.display).as_bool();
            if !owner_visible {
                if display_visible { let _ = ShowWindow(self.display, SW_HIDE); }
                return;
            }
            let (mut o, mut d) = (RECT::default(), RECT::default());
            if GetWindowRect(owner, &mut o).is_err() || GetWindowRect(self.display, &mut d).is_err() { return; }
            let flags = SWP_NOACTIVATE | SWP_NOSIZE;
            if (o.left, o.top) != (d.left, d.top) || !display_visible || raise {
                let _ = SetWindowPos(self.display, HWND_TOPMOST, o.left, o.top, 0, 0, flags | SWP_SHOWWINDOW);
            }
            // 岛仔窗口紧贴在显示窗口下面：别的置顶窗口不会夹在两者之间。
            if next_visible(self.display) != Some(owner) {
                let _ = SetWindowPos(owner, self.display, 0, 0, 0, 0, flags | SWP_NOMOVE);
            }
        }
    }

    /// 放弃分层显示，岛仔窗口恢复为普通窗口（回到轮廓裁剪）。
    pub fn detach(&mut self, owner: HWND) {
        unsafe {
            let ex = GetWindowLongPtrW(owner, GWL_EXSTYLE);
            if ex & WS_EX_LAYERED.0 as isize != 0 { SetWindowLongPtrW(owner, GWL_EXSTYLE, ex & !(WS_EX_LAYERED.0 as isize)); }
            let _ = ShowWindow(self.display, SW_HIDE);
        }
        self.last = None;
    }
}

impl Drop for Layered {
    fn drop(&mut self) {
        unsafe {
            self.release_bitmap();
            let _ = DeleteDC(self.dc);
            let _ = DestroyWindow(self.display);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_animation_readback_is_paced_but_input_stays_full_rate() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        assert_eq!(readback_delay(None, now, false), None, "第一次读回不等待");
        assert_eq!(readback_delay(Some(now), now + Duration::from_millis(10), false), Some(Duration::from_millis(23)), "动画期间未到间隔时延后");
        assert_eq!(readback_delay(Some(now), now + AMBIENT_READBACK, false), None, "到间隔立即读回");
        assert_eq!(readback_delay(Some(now), now + Duration::from_millis(1), true), None, "有输入时保持满帧");
        // 60 帧/秒的动画持续 1 秒：读回次数减半左右。
        let (mut last, mut count) = (None, 0);
        for frame in 0..60u64 {
            let t = now + Duration::from_micros(frame * 16_667);
            if readback_delay(last, t, false).is_none() { last = Some(t); count += 1; }
        }
        assert!((28..=31).contains(&count), "约 30 次/秒，实际 {count}");
    }

    #[test]
    fn opaque_readback_is_not_trusted_for_layering() {
        let opaque = [0u8, 0, 0, 255].repeat(16);
        assert!(!has_real_alpha([4, 4], &opaque), "读不到 alpha 时不能切到分层窗口");
        let mut clear = opaque.clone();
        clear[3] = 0;
        assert!(has_real_alpha([4, 4], &clear));
        assert!(!has_real_alpha([4, 4], &clear[..32]), "尺寸不符视为无效");
    }

    #[test]
    fn coverage_mask_gives_fractional_edges() {
        // x 1.5..3.5、y 0..2 的矩形：左右边缘各盖住半个像素，中间整格。
        let rect = vec![(1.5, 0.0), (3.5, 0.0), (3.5, 2.0), (1.5, 2.0)];
        let mask = coverage_mask([5, 2], &[rect]);
        assert_eq!(&mask[..5], &[0, 128, 255, 128, 0]);
        assert_eq!(&mask[..5], &mask[5..]);
        // 上下边缘落在像素中间时同样只盖一半。
        let half = coverage_mask([1, 2], &[vec![(0.0, 0.5), (1.0, 0.5), (1.0, 2.0), (0.0, 2.0)]]);
        assert_eq!(half, vec![128, 255]);
    }

    #[test]
    fn coverage_mask_smooths_curves_and_unions_shapes() {
        // 圆：内部全满、外部全空，边缘一圈是过渡值（这正是轮廓裁剪做不到的）。
        let circle: Vec<(f32, f32)> = (0..64).map(|i| { let a = i as f32 / 64.0 * std::f32::consts::TAU; (10.0 + 7.0 * a.cos(), 10.0 + 7.0 * a.sin()) }).collect();
        let mask = coverage_mask([20, 20], &[circle]);
        assert_eq!(mask[10 * 20 + 10], 255);
        assert_eq!(mask[0], 0);
        let soft = mask.iter().filter(|&&a| a > 0 && a < 255).count();
        assert!(soft >= 30, "圆周应有一圈抗锯齿过渡像素，实际 {soft}");
        // 两块方向相反、互相重叠的轮廓取并集，重叠处不能被抵消成空洞。
        let a = vec![(0.0, 0.0), (6.0, 0.0), (6.0, 4.0), (0.0, 4.0)];
        let b = vec![(4.0, 0.0), (4.0, 4.0), (10.0, 4.0), (10.0, 0.0)];
        let both = coverage_mask([10, 4], &[a, b]);
        assert!(both.iter().all(|&v| v == 255), "并集应整块覆盖：{both:?}");
        // 轮廓超出画布时安全截断。
        let out = coverage_mask([4, 4], &[vec![(-5.0, -5.0), (9.0, -5.0), (9.0, 9.0), (-5.0, 9.0)]]);
        assert!(out.iter().all(|&v| v == 255));
        assert!(coverage_mask([4, 4], &[]).iter().all(|&v| v == 0));
    }

    #[test]
    fn mask_keeps_premultiplied_colors_valid() {
        let rgba = [200u8, 100, 50, 255, 255, 255, 255, 255, 30, 30, 30, 255];
        let mut out = Vec::new();
        apply_mask(&rgba, &[255, 128, 0], &mut out);
        assert_eq!(out, vec![200, 100, 50, 255, 128, 128, 128, 128, 0, 0, 0, 0]);
    }

    #[test]
    fn hash_changes_with_content_and_size() {
        let a = [1u8, 2, 3, 4].repeat(6);
        let mut b = a.clone();
        b[23] = 5;
        assert_ne!(image_hash([3, 2], &a), image_hash([3, 2], &b));
        assert_ne!(image_hash([3, 2], &a), image_hash([2, 3], &a));
        assert_eq!(image_hash([3, 2], &a), image_hash([3, 2], &a));
    }

    /// 兼容渲染每帧都会走这里：连续提交几千帧不同画面，进程内存和 GDI 对象都不能随帧数增长。
    #[test]
    fn presenting_many_frames_does_not_leak() {
        unsafe {
            use windows::Win32::System::Threading::{GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS, GR_USEROBJECTS};
            let (w, h) = (960usize, 285usize);
            let owner = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, windows::core::w!("BUTTON"), windows::core::w!(""), WS_POPUP | WS_VISIBLE, -10000, -10000, w as i32, h as i32, None, None, None, None).unwrap();
            let mut layered = Layered::new().unwrap();
            let mut frame = vec![0u8; w * h * 4];
            let mut present = |n: u32, layered: &mut Layered| for i in 0..n {
                frame[(i as usize * 4) % (w * h * 4)] = (i % 251) as u8;
                frame[3] = (i % 200) as u8;
                assert!(layered.present(owner, [w, h], &frame).unwrap(), "每帧画面不同，必须真正提交");
            };
            present(300, &mut layered);
            let gdi = |kind| GetGuiResources(GetCurrentProcess(), kind);
            let (mem0, gdi0, user0) = (crate::resources::snapshot().unwrap().private_bytes, gdi(GR_GDIOBJECTS), gdi(GR_USEROBJECTS));
            present(3000, &mut layered);
            let (mem1, gdi1, user1) = (crate::resources::snapshot().unwrap().private_bytes, gdi(GR_GDIOBJECTS), gdi(GR_USEROBJECTS));
            eprintln!("3000 帧：私有内存 {:+.1} MB，GDI {gdi0}→{gdi1}，USER {user0}→{user1}", (mem1 as f64 - mem0 as f64) / 1048576.0);
            assert!(mem1 < mem0 + 8 * 1024 * 1024, "3000 帧后私有内存增长超过 8MB");
            assert!(gdi1 <= gdi0 + 2 && user1 <= user0 + 2, "GDI/USER 对象随帧数增长");
            drop(layered);
            let _ = DestroyWindow(owner);
        }
    }

    #[test]
    fn display_window_follows_owner_and_owner_stays_clickable() {
        unsafe {
            // 用 BUTTON：STATIC 自己就返回 HTTRANSPARENT，测不出命中。
            let owner = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST, windows::core::w!("BUTTON"), windows::core::w!(""), WS_POPUP | WS_VISIBLE, -10000, -10000, 4, 2, None, None, None, None).unwrap();
            let mut layered = Layered::new().unwrap();
            let mut px = vec![0u8; 32];
            px[4..8].copy_from_slice(&[64, 64, 64, 128]);
            px[8..12].copy_from_slice(&[255, 255, 255, 255]);
            assert!(layered.present(owner, [4, 2], &px).unwrap());
            let mut alpha = 0u8;
            GetLayeredWindowAttributes(owner, None, Some(&mut alpha), None).unwrap();
            assert_eq!(alpha, OWNER_ALPHA, "岛仔窗口几乎不可见，但不能是 0（否则点不到）");
            assert!(IsWindowVisible(layered.display()).as_bool());
            assert_eq!(next_visible(layered.display()), Some(owner), "岛仔窗口紧贴在显示窗口下面");
            assert!(!layered.present(owner, [4, 2], &px).unwrap(), "同一画面不重复提交");
            assert!(!layered.present(owner, [8, 1], &[0u8; 32]).unwrap(), "尺寸与窗口不符时跳过");
            SetWindowPos(owner, None, -9000, -9000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
            layered.sync(owner, false);
            let mut d = RECT::default();
            GetWindowRect(layered.display(), &mut d).unwrap();
            assert_eq!((d.left, d.top), (-9000, -9000), "窗口移动后显示窗口跟随");
            let _ = ShowWindow(owner, SW_HIDE);
            layered.sync(owner, false);
            assert!(!IsWindowVisible(layered.display()).as_bool(), "岛仔窗口隐藏时显示窗口一起隐藏");
            layered.detach(owner);
            assert!(GetWindowLongPtrW(owner, GWL_EXSTYLE) & WS_EX_LAYERED.0 as isize == 0);
            let _ = DestroyWindow(owner);
        }
    }
}
