//! 悬浮云：微灰玻璃底 + 封面主色淡彩 + 光标光晕 + 进度条拖动光效。
//!
//! 这里只负责「算颜色」——给定岛体局部坐标返回一个顶点颜色，由
//! [`crate::shape::fill_shaded`] 在填充轮廓网格时逐顶点采样，因此所有光都天然
//! 被药丸／面板的圆角轮廓裁住，不会溢出到桌面，也不需要额外裁剪层。
//!
//! 设计取向（对齐参考效果）：
//! - **底色是中性微灰**，不是自带的霓虹渐变；颜色只从封面主色和桌面背景里来；
//! - **换曲只换色，不闪**：底色约 0.4 s 交叉淡入到新封面主色，全程没有额外提亮，
//!   所以切歌不会突兀；音乐关掉时同样淡回中性灰；
//! - **光标光晕**跟随鼠标在岛上的位置，很淡很散，移开即衰减消失；
//! - **追光**：进度条头部常驻一团柔光，随播放前进，按住拖动时略微加强；
//! - **静止即静止**：没有播放、没有光标移动、没有换曲与拖动时，
//!   [`Fluid::animating`] 返回 false，界面可以降到低频刷新，这是控制功耗的关键。

use eframe::egui::{Color32, ColorImage};

/// 换曲时底色交叉淡入的时长（秒）。
const TINT_FADE: f64 = 0.45;
/// 光标光晕的横向 / 纵向半径（逻辑点）。
const GLOW_RX: f32 = 152.0;
const GLOW_RY: f32 = 100.0;
/// 进度条光斑：头部往左的拖尾长度、头前收边宽度与竖向尺度。
const SEEK_TAIL: f32 = 96.0;
const SEEK_LEAD: f32 = 16.0;
const SEEK_RY: f32 = 30.0;

/// 没有封面时用的中性色：接近白，乘上低强度后就是一层灰雾。
pub const NEUTRAL: [f32; 3] = [0.62, 0.66, 0.74];

pub struct Fluid {
    t: f64,
    /// 交叉淡入的两端：`tint_from → tint_to`，`tint_at` 为起点。
    tint_from: [f32; 3],
    tint_to: [f32; 3],
    tint_at: f64,
    /// 光标目标位置（岛体局部坐标）与平滑后的实际位置。
    cursor: Option<(f32, f32)>,
    glow: (f32, f32),
    glow_a: f32,
    /// 光晕是否还在追随（用于判断要不要继续刷新）。
    glow_moving: bool,
    /// 拖动进度条时的光斑位置与强度。
    seek: Option<(f32, f32)>,
    seek_xy: (f32, f32),
    seek_a: f32,
    /// 追光的目标亮度：暂停 0.3、播放 0.5、按住拖动 1.0。
    seek_hot: f32,
}

impl Default for Fluid {
    fn default() -> Self {
        Self {
            t: 0.0,
            tint_from: NEUTRAL,
            tint_to: NEUTRAL,
            tint_at: f64::NEG_INFINITY,
            cursor: None,
            glow: (0.0, 0.0),
            glow_a: 0.0,
            glow_moving: false,
            seek: None,
            seek_xy: (0.0, 0.0),
            seek_a: 0.0,
            seek_hot: 0.0,
        }
    }
}

impl Fluid {
    pub fn new() -> Self {
        Self::default()
    }

    /// 中性色（音乐停止时淡回的颜色）。
    pub fn neutral() -> [f32; 3] {
        NEUTRAL
    }

    // ---------- 每帧推进 ----------

    pub fn advance(&mut self, t: f64, dt: f32) {
        self.t = t;
        let k = (dt * 15.0).clamp(0.0, 1.0);
        match self.cursor {
            Some((tx, ty)) => {
                let (dx, dy) = (tx - self.glow.0, ty - self.glow.1);
                self.glow.0 += dx * k;
                self.glow.1 += dy * k;
                self.glow_a += (1.0 - self.glow_a) * (dt * 5.5).clamp(0.0, 1.0);
                self.glow_moving = dx.abs() + dy.abs() > 0.35;
            }
            None => {
                self.glow_a += (0.0 - self.glow_a) * (dt * 6.5).clamp(0.0, 1.0);
                self.glow_moving = self.glow_a > 0.004;
            }
        }
        if self.glow_a < 0.003 {
            self.glow_a = 0.0;
        }

        // 进度条光斑：拖动时紧跟滑块，松手后快速收掉。
        match self.seek {
            Some((sx, sy)) => {
                let k = (dt * 18.0).clamp(0.0, 1.0);
                self.seek_xy.0 += (sx - self.seek_xy.0) * k;
                self.seek_xy.1 += (sy - self.seek_xy.1) * k;
                self.seek_a += (self.seek_hot - self.seek_a) * (dt * 6.0).clamp(0.0, 1.0);
            }
            None => {
                self.seek_a += (0.0 - self.seek_a) * (dt * 6.0).clamp(0.0, 1.0);
            }
        }
        if self.seek_a < 0.003 {
            self.seek_a = 0.0;
        }
    }

    /// 光标位置（岛体局部坐标）；移出岛体传 None。
    pub fn set_cursor(&mut self, at: Option<(f32, f32)>) {
        if let (Some(p), None) = (at, self.cursor) {
            // 首次进入：光晕直接落在光标处，不要从左上角飞过来。
            self.glow = p;
        }
        self.cursor = at;
    }

    /// 拖动进度条时光斑所在的位置（岛体局部坐标）；松手传 None。
    /// 追光位置（岛体局部坐标，进度条头部）与目标亮度；没有音源传 None。
    pub fn set_seek(&mut self, at: Option<(f32, f32)>, hot: f32) {
        if let (Some(p), None) = (at, self.seek) {
            self.seek_xy = p;
            self.seek_a = 0.0;
        }
        self.seek = at;
        self.seek_hot = hot.clamp(0.0, 1.0);
    }

    // ---------- 变色 ----------

    /// 目标主色变化：底色交叉淡入过去。**只换色，不提亮**，所以切歌不会突兀。
    pub fn set_tint(&mut self, rgb: [f32; 3]) {
        if rgb == self.tint_to {
            return;
        }
        self.tint_from = self.tint_now();
        self.tint_to = rgb;
        self.tint_at = self.t;
    }

    /// 当前封面主色（向白色提亮 `to_white`），供进度条追光等元素共用，
    /// 这样换歌时界面上的光会跟着一起换色，整体才协调。
    pub fn tint_color(&self, to_white: f32) -> Color32 {
        let c = self.tint_now();
        let k = to_white.clamp(0.0, 1.0);
        let q = |v: f32| ((v + (1.0 - v) * k) * 255.0).clamp(0.0, 255.0) as u8;
        Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
    }

    /// 当前应当显示的封面主色。
    pub fn tint_now(&self) -> [f32; 3] {
        let u = ((self.t - self.tint_at) / TINT_FADE).clamp(0.0, 1.0) as f32;
        let e = 1.0 - (1.0 - u).powi(3);
        let mut out = [0.0; 3];
        for i in 0..3 {
            out[i] = self.tint_from[i] + (self.tint_to[i] - self.tint_from[i]) * e;
        }
        out
    }

    // ---------- 底色 ----------

    /// 玻璃底色透明度：(顶部, 底部)。合上整体半透，展开顶部实、下半渐透。
    pub fn glass_alpha(pc: f32) -> (f32, f32) {
        let pc = pc.clamp(0.0, 1.0);
        let collapsed = 0.52;
        (collapsed + (0.88 - collapsed) * pc, collapsed + (0.34 - collapsed) * pc)
    }

    /// 中性微灰的明度（0..255）：收起时偏亮（像磨砂玻璃），展开时偏深（压得住文字）。
    pub fn gray_at(expand: f32, v: f32) -> f32 {
        let expand = expand.clamp(0.0, 1.0);
        let base = 54.0 + (36.0 - 54.0) * expand;
        let drop = (4.0 + 10.0 * expand) * v.clamp(0.0, 1.0);
        (base - drop).clamp(18.0, 80.0)
    }

    // ---------- 合成 ----------

    /// 把一个顶点合成成颜色（未预乘）。`x/y` 是岛体局部坐标。
    pub fn compose(&self, x: f32, y: f32, w: f32, h: f32, expand: f32) -> Color32 {
        let w = w.max(1.0);
        let h = h.max(1.0);
        let (u, v) = ((x / w).clamp(0.0, 1.0), (y / h).clamp(0.0, 1.0));
        let expand = expand.clamp(0.0, 1.0);

        // 1) 中性微灰底
        let (at, ab) = Self::glass_alpha(expand);
        let a0 = at + (ab - at) * v.powf(1.3);
        let g = Self::gray_at(expand, v) / 255.0;
        let mut a = a0;
        let mut c = [g * a0; 3];

        // 2) 封面主色淡彩：贴着封面那一侧最浓，向右迅速衰减
        let tint = self.tint_now();
        let edge = 0.22 + 0.78 * (-(u / 0.32).powi(2)).exp();
        over(&mut c, &mut a, tint, 0.22 * edge * (0.55 + 0.45 * expand));

        // 3) 光标光晕：跟着鼠标的一团散光
        if self.glow_a > 0.003 {
            let dx = (x - self.glow.0) / GLOW_RX;
            let dy = (y - self.glow.1) / GLOW_RY;
            let e = (-(dx * dx + dy * dy) * 1.5).exp();
            over(&mut c, &mut a, [1.0, 1.0, 1.0], 0.12 * e * self.glow_a);
        }

        // 4) 追光：头部往**后**拖一条很淡的尾光，头前几乎立刻收掉。
        //    和进度条上那道彗尾同方向，面板上才不会出现「一团圆灯」。
        if self.seek_a > 0.003 {
            let dx = x - self.seek_xy.0;
            let along = if dx >= 0.0 {
                (-(dx / SEEK_LEAD).powi(2) * 1.6).exp()
            } else {
                (-(-dx / SEEK_TAIL).powf(1.3)).exp()
            };
            let dy = (y - self.seek_xy.1) / SEEK_RY;
            let e = along * (-(dy * dy) * 1.6).exp();
            over(&mut c, &mut a, tint, 0.065 * e * self.seek_a);
        }

        unmultiply(c, a)
    }

    /// 是否还需要连续刷新（功耗闸门）。
    pub fn animating(&self) -> bool {
        (self.t - self.tint_at) < TINT_FADE
            || self.glow_moving
            || (self.glow_a > 0.0 && self.glow_a < 0.996)
            || (self.seek_a - if self.seek.is_some() { self.seek_hot } else { 0.0 }).abs() > 0.004
    }

    // ---------- 取色 ----------

    /// 从封面图取一个「不太脏」的主色：暗部和低饱和像素权重更低，
    /// 取完再把明度归一，保证淡彩看得见但不会变成霓虹。
    pub fn tint_from_image(image: &ColorImage) -> [f32; 3] {
        let [w, h] = image.size;
        if w == 0 || h == 0 {
            return NEUTRAL;
        }
        let step = (w.min(h) / 16).max(1);
        let (mut r, mut g, mut b, mut n) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut y = 0;
        while y < h {
            let mut x = 0;
            while x < w {
                let p = image.pixels[y * w + x];
                let (rr, gg, bb) = (p.r() as f32 / 255.0, p.g() as f32 / 255.0, p.b() as f32 / 255.0);
                let mx = rr.max(gg).max(bb);
                let mn = rr.min(gg).min(bb);
                let sat = if mx > 0.001 { (mx - mn) / mx } else { 0.0 };
                let wgt = (0.25 + sat) * (0.30 + mx);
                r += rr * wgt;
                g += gg * wgt;
                b += bb * wgt;
                n += wgt;
                x += step;
            }
            y += step;
        }
        if n <= 0.0 {
            return NEUTRAL;
        }
        normalize([r / n, g / n, b / n])
    }

    /// 没有封面时的兜底色：去饱和的标题哈希色，只作很淡的暗示。
    pub fn tint_from_title(title: &str) -> [f32; 3] {
        if title.trim().is_empty() {
            return NEUTRAL;
        }
        let mut h: u32 = 2166136261;
        for b in title.bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(16777619);
        }
        normalize(hsv((h % 360) as f32, 0.30, 0.92))
    }
}

/// 把颜色归一：最大分量抬到 0.9，避免深色封面把淡彩压没。
fn normalize(c: [f32; 3]) -> [f32; 3] {
    let mx = c[0].max(c[1]).max(c[2]).max(1e-3);
    let k = 0.90 / mx;
    [(c[0] * k).min(1.0), (c[1] * k).min(1.0), (c[2] * k).min(1.0)]
}

/// 预乘空间里的 source-over 叠加。
fn over(c: &mut [f32; 3], a: &mut f32, src: [f32; 3], sa: f32) {
    let sa = sa.clamp(0.0, 1.0);
    if sa <= 0.0005 {
        return;
    }
    for i in 0..3 {
        c[i] = c[i] * (1.0 - sa) + src[i] * sa;
    }
    *a += sa * (1.0 - *a);
}

fn unmultiply(c: [f32; 3], a: f32) -> Color32 {
    let inv = if a > 1e-4 { 1.0 / a } else { 0.0 };
    let q = |v: f32| (v * inv * 255.0).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(q(c[0]), q(c[1]), q(c[2]), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_of(color: Color32) -> ColorImage {
        ColorImage { size: [8, 8], pixels: vec![color; 64] }
    }

    fn lum(c: Color32) -> u32 {
        c.r() as u32 + c.g() as u32 + c.b() as u32
    }

    #[test]
    fn collapsed_is_translucent_and_expanded_fades_toward_the_bottom() {
        let (top, bottom) = Fluid::glass_alpha(0.0);
        assert!((top - bottom).abs() < 1e-6, "收起时是一条均匀的半透药丸");
        assert!(top > 0.45 && top < 0.75, "半透但不能糊掉文字：{top}");

        let (top, bottom) = Fluid::glass_alpha(1.0);
        assert!(top > 0.85, "展开后顶部要压得住文字：{top}");
        assert!(bottom < 0.45, "展开后下半段要透出桌面：{bottom}");
        assert!(top > bottom);
    }

    #[test]
    fn base_is_neutral_gray_not_a_neon_wash() {
        let f = Fluid::new();
        let a = f.compose(40.0, 12.0, 163.0, 30.0, 0.0);
        let b = f.compose(120.0, 26.0, 163.0, 30.0, 0.0);
        for c in [a, b] {
            let mx = c.r().max(c.g()).max(c.b()) as i32;
            let mn = c.r().min(c.g()).min(c.b()) as i32;
            assert!(mx - mn <= 8, "底色应接近中性灰，色差只有 {}", mx - mn);
            assert!(c.r() > 18, "微灰不能退化成纯黑");
        }
    }

    #[test]
    fn track_change_fades_color_without_any_brightness_step() {
        let mut f = Fluid::new();
        f.advance(1.0, 0.016);
        f.set_tint([0.9, 0.2, 0.2]);
        f.advance(1.0 + TINT_FADE, 0.016);
        let red = f.compose(60.0, 10.0, 419.0, 140.0, 1.0);

        // 换曲：逐帧看亮度，任何一帧都不许跳（这就是「突兀」的来源）。
        f.set_tint([0.2, 0.8, 0.4]);
        let mut prev = lum(f.compose(60.0, 10.0, 419.0, 140.0, 1.0));
        for i in 0..40 {
            f.advance(f.t + 0.016, 0.016);
            let cur = lum(f.compose(60.0, 10.0, 419.0, 140.0, 1.0));
            let step = (cur as i32 - prev as i32).abs();
            assert!(step <= 8, "第 {i} 帧亮度跳了 {step}，切歌会显得突兀");
            prev = cur;
        }
        let green = f.compose(60.0, 10.0, 419.0, 140.0, 1.0);
        assert!(green.g() > red.g(), "淡入结束应当换成新主色");
    }

    #[test]
    fn color_returns_to_neutral_when_music_stops() {
        let mut f = Fluid::new();
        f.advance(1.0, 0.016);
        f.set_tint([0.95, 0.15, 0.15]);
        f.advance(1.0 + TINT_FADE, 0.016);
        let colored = f.compose(60.0, 70.0, 419.0, 140.0, 1.0);
        f.set_tint(Fluid::neutral());
        f.advance(20.0, 0.016);
        f.advance(20.0 + TINT_FADE + 0.05, 0.016);
        let after = f.compose(60.0, 70.0, 419.0, 140.0, 1.0);
        assert!(lum(colored) != lum(after) || colored.r() != after.r(), "必须淡回中性");
        // 回到中性后不应再带明显色相
        let mx = after.r().max(after.g()).max(after.b()) as i32;
        let mn = after.r().min(after.g()).min(after.b()) as i32;
        assert!(mx - mn <= 10, "音乐停止后底色应当回到中性灰：{}", mx - mn);
    }

    #[test]
    fn bloom_is_gone_so_animating_stops_after_the_fade() {
        let mut f = Fluid::new();
        f.advance(1.0, 0.016);
        f.set_tint([0.9, 0.3, 0.2]);
        assert!(f.animating(), "淡入期间要刷新");
        f.advance(1.0 + TINT_FADE + 0.1, 0.016);
        assert!(!f.animating(), "淡入结束就该停手");
    }

    #[test]
    fn cursor_glow_fades_in_and_out() {
        let mut f = Fluid::new();
        f.advance(1.0, 0.016);
        f.set_cursor(Some((80.0, 15.0)));
        for _ in 0..40 {
            f.advance(f.t + 0.016, 0.016);
        }
        assert!(f.glow_a > 0.9, "光标在岛上时光晕要亮起来：{}", f.glow_a);
        f.set_cursor(None);
        for _ in 0..60 {
            f.advance(f.t + 0.016, 0.016);
        }
        assert_eq!(f.glow_a, 0.0, "移开后光晕必须完全消失");
    }

    #[test]
    fn glow_reaches_middle_and_top_regions() {
        let mut f = Fluid::new();
        f.advance(1.0, 0.016);
        // 面板正中偏上：这正是旧版网格被抹平、完全看不到光的位置。
        let point = (209.5f32, 46.0f32);
        let before = f.compose(point.0, point.1, 419.0, 140.0, 1.0);
        f.set_cursor(Some(point));
        for _ in 0..40 {
            f.advance(f.t + 0.016, 0.016);
        }
        let after = f.compose(point.0, point.1, 419.0, 140.0, 1.0);
        assert!(lum(after) > lum(before) + 15, "中间偏上区域必须有明显光晕");
    }

    #[test]
    fn cursor_glow_is_smooth_across_neighbouring_samples() {
        let mut f = Fluid::new();
        f.advance(1.0, 0.016);
        f.set_cursor(Some((209.5, 70.0)));
        for _ in 0..40 {
            f.advance(f.t + 0.016, 0.016);
        }
        // 沿竖线取样：相邻采样点的亮度不应出现台阶（网格抹平会表现为突变或平段）。
        let mut prev = lum(f.compose(209.5, 2.0, 419.0, 140.0, 1.0));
        for i in 1..28 {
            let y = 2.0 + i as f32 * 5.0;
            let cur = lum(f.compose(209.5, y, 419.0, 140.0, 1.0));
            let step = (cur as i32 - prev as i32).abs();
            assert!(step <= 60, "第 {i} 个采样点亮度跳了 {step}，光晕不连续");
            prev = cur;
        }
    }

    #[test]
    fn chase_light_follows_the_head_and_is_soft() {
        let mut f = Fluid::new();
        f.advance(1.0, 0.016);
        let before = f.compose(120.0, 92.0, 419.0, 140.0, 1.0);
        // 头前 / 头后各取一个基准点（此时还没有追光），后面用来算各自的抬升
        let base_behind = f.compose(260.0, 92.0, 419.0, 140.0, 1.0);
        let base_ahead = f.compose(340.0, 92.0, 419.0, 140.0, 1.0);

        // 播放中：追光常驻，亮度只有拖动时的一半左右。
        f.set_seek(Some((120.0, 92.0)), 0.5);
        for _ in 0..40 {
            f.advance(f.t + 0.016, 0.016);
        }
        let playing = f.compose(120.0, 92.0, 419.0, 140.0, 1.0);
        let spread = f.compose(120.0, 62.0, 419.0, 140.0, 1.0);
        assert!(lum(playing) > lum(before), "进度头要亮于底色");
        assert!(lum(spread) > lum(before), "光要散得开，不能只是一颗硬点");
        // 柔和：单通道抬升不超过 30 级（旧版是 40 级以上的硬白核）
        let lift = (lum(playing) - lum(before)) as f32 / 3.0;
        assert!(lift < 30.0, "追光太亮了：+{lift:.0}/255");

        // 拖动时更亮，但不能亮到突兀
        f.set_seek(Some((120.0, 92.0)), 1.0);
        for _ in 0..40 {
            f.advance(f.t + 0.016, 0.016);
        }
        let dragging = f.compose(120.0, 92.0, 419.0, 140.0, 1.0);
        assert!(lum(dragging) > lum(playing), "拖动应当比播放时更亮");
        let lift = (lum(dragging) - lum(before)) as f32 / 3.0;
        assert!(lift < 55.0, "拖动光也太亮了：+{lift:.0}/255");

        // 追光随位置移动：同一个采样点，头压过来时才亮
        // （底色本身左边偏彩、右边偏灰，跨点比亮度会被底色差淹没，所以固点比）。
        let at300 = f.compose(300.0, 92.0, 419.0, 140.0, 1.0);
        f.set_seek(Some((300.0, 92.0)), 1.0);
        for _ in 0..40 {
            f.advance(f.t + 0.016, 0.016);
        }
        let moved = f.compose(300.0, 92.0, 419.0, 140.0, 1.0);
        assert!(lum(moved) > lum(at300), "追光要跟着走");

        // 并且是**朝后**拖：同样离开头部 40 DIP，头后的抬升要远大于头前。
        let behind = f.compose(260.0, 92.0, 419.0, 140.0, 1.0);
        let ahead = f.compose(340.0, 92.0, 419.0, 140.0, 1.0);
        let lift_behind = lum(behind) as i32 - lum(base_behind) as i32;
        let lift_ahead = lum(ahead) as i32 - lum(base_ahead) as i32;
        assert!(lift_behind > lift_ahead + 6, "光应当朝后拖：头后 +{lift_behind}，头前 +{lift_ahead}");

        // 没有音源后收干净
        f.set_seek(None, 0.0);
        for _ in 0..80 {
            f.advance(f.t + 0.016, 0.016);
        }
        assert_eq!(f.seek_a, 0.0, "没有音源后必须收干净");
        assert!(!f.animating(), "收干净后不应继续刷新");
    }

    #[test]
    fn tint_from_image_ignores_dark_borders() {
        let mut img = image_of(Color32::from_rgb(0, 0, 0));
        for y in 2..6 {
            for x in 2..6 {
                img.pixels[y * 8 + x] = Color32::from_rgb(220, 60, 90);
            }
        }
        let t = Fluid::tint_from_image(&img);
        assert!(t[0] > t[1] && t[0] > t[2], "主色应当偏红：{t:?}");
    }

    #[test]
    fn compose_alpha_stays_within_bounds() {
        let mut f = Fluid::new();
        f.advance(1.0, 0.016);
        f.set_tint([0.9, 0.3, 0.2]);
        f.set_cursor(Some((40.0, 10.0)));
        for _ in 0..30 {
            f.advance(f.t + 0.016, 0.016);
        }
        for expand in [0.0, 0.5, 1.0] {
            for y in [0.0, 14.0, 139.0] {
                let c = f.compose(80.0, y, 419.0, 140.0, expand);
                let (at, ab) = Fluid::glass_alpha(expand);
                let floor = at.min(ab) * 255.0 - 1.0;
                assert!(c.a() as f32 >= floor, "alpha 不能被压到低于底色：{}", c.a());
            }
        }
    }
}
