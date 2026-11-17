//! 皮肤调色板：把按深色主题画好的岛体内容，整体映射到浅色皮肤的配色。
//!
//! 岛上的页面、图标与控件都按「黑底白字」设计，颜色散落在几十处绘制代码里。
//! 这里不逐处改写，而是在一帧画完之后对岛体内容的图形统一换色：
//! - 中性色按亮度对调：白字 → 墨色，黑底卡片 → 纸色，灰阶按比例落在两者之间；
//! - 强调色（蓝）换成皮肤的主题色，其余彩色略加深，保证在浅底上可读；
//! - 带纹理的图形（专辑封面、应用图标）原样保留。

use eframe::egui::{self, Color32};
use eframe::epaint::{shape_transform::adjust_colors, ColorMode, Shape};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// 原来的「黑」映射到的底色（sRGB 0..255）。
    pub paper: [u8; 3],
    /// 原来的「白」映射到的文字色。
    pub ink: [u8; 3],
    /// 蓝色强调色替换成的主题色；None 表示保留色相，只加深。
    pub accent: Option<[u8; 3]>,
    /// 深色块（原卡片、按钮底）的不透明度系数：毛玻璃让卡片也带一点通透。
    pub dark_alpha: f32,
    /// 文字类中性色的亮度曲线（1 = 线性；小于 1 让灰字更亮，用于深色玻璃）。
    pub lift: f32,
    /// 非强调彩色的加深系数（浅底上需要加深；深色玻璃保持 1）。
    pub darken: f32,
    /// 深色块（卡片、按钮底）映射时的亮度系数：越小越接近纸色，毛玻璃浅色用它做白色磨砂按钮。
    pub chip: f32,
}

/// 奶油（参考 Oyen）：暖白底、深棕字、橙色强调。
pub const CREAM: Palette = Palette { paper: [255, 243, 228], ink: [46, 32, 22], accent: Some([240, 138, 36]), dark_alpha: 1.0, lift: 1.0, darken: 0.78, chip: 1.0 };
/// 毛玻璃·浅：白色磨砂、近黑字、系统蓝。
pub const GLASS_LIGHT: Palette = Palette { paper: [255, 255, 255], ink: [22, 22, 26], accent: Some([10, 112, 230]), dark_alpha: 0.8, lift: 1.0, darken: 0.78, chip: 0.6 };
/// 毛玻璃·深：保持黑底白字，只把灰色文字提亮、卡片底调透，适配更通透的深色玻璃。
pub const GLASS_DARK: Palette = Palette { paper: [0, 0, 0], ink: [255, 255, 255], accent: None, dark_alpha: 0.5, lift: 0.6, darken: 1.0, chip: 1.0 };

/// 单色图标纹理（白色图形 + 透明度，如 AI 厂商标识）：和文字一样参与换色。
/// 其余纹理（专辑封面、应用图标、文件预览）保持原样。
fn monochrome() -> &'static std::sync::Mutex<std::collections::HashSet<egui::TextureId>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<egui::TextureId>>> = std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
}

/// 登记一张单色图标纹理。
pub fn mark_monochrome(id: egui::TextureId) {
    monochrome().lock().unwrap_or_else(|e| e.into_inner()).insert(id);
}

/// 彩色标识不参与皮肤换色，同时清除纹理编号复用时的旧登记。
pub fn mark_colored(id:egui::TextureId){
    monochrome().lock().unwrap_or_else(|e|e.into_inner()).remove(&id);
}

fn is_monochrome(id: egui::TextureId) -> bool {
    monochrome().lock().unwrap_or_else(|e| e.into_inner()).contains(&id)
}

/// 岛仔自己的主题蓝（按钮、进度、饮水等）。只有这几种蓝会换成皮肤的主题色；
/// 用户为 AI 账号等选的品牌色一律保留色相。
fn theme_blue(c: [f32; 3]) -> Option<bool> {
    let near = |t: eframe::egui::Color32| {
        let d = |a: f32, b: u8| (a * 255.0 - b as f32).abs();
        d(c[0], t.r()) <= 10.0 && d(c[1], t.g()) <= 10.0 && d(c[2], t.b()) <= 10.0
    };
    use eframe::egui::Color32 as C;
    // 界面里实际用到的蓝：按钮、选中态、链接、进度、饮水；不含 AI 账号可选的品牌蓝。
    const BRIGHT: [C; 8] = [crate::theme::BLUE, crate::theme::CYAN, C::from_rgb(82, 162, 255), C::from_rgb(10, 132, 255), C::from_rgb(70, 150, 255), C::from_rgb(64, 156, 255), C::from_rgb(86, 204, 242), C::from_rgb(77, 192, 242)];
    const DEEP: [C; 4] = [crate::theme::BLUE_DEEP, crate::theme::WATER_TRACK, C::from_rgb(45, 105, 165), C::from_rgb(28, 65, 100)];
    if BRIGHT.iter().any(|t| near(*t)) {
        Some(true)
    } else if DEEP.iter().any(|t| near(*t)) {
        Some(false)
    } else {
        None
    }
}

fn light_paper(p: &Palette) -> bool {
    luma([p.paper[0] as f32 / 255.0, p.paper[1] as f32 / 255.0, p.paper[2] as f32 / 255.0]) > 0.5
}

fn contrast(a: [f32; 3], b: [f32; 3]) -> f32 {
    crate::glass::contrast(a, b)
}

fn luma(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

/// 映射单个颜色（Color32 为预乘 alpha）。
pub fn map_color(color: Color32, p: &Palette) -> Color32 {
    let a = color.a();
    if a == 0 {
        return color;
    }
    // egui 的淡入淡出（gamma_multiply）直接在 sRGB 数值上乘透明度，
    // 所以这里也在 sRGB 空间里还原/预乘，半透明的白字才会被识别为「白」。
    let af = a as f32 / 255.0;
    let c = [color.r() as f32 / 255.0 / af, color.g() as f32 / 255.0 / af, color.b() as f32 / 255.0 / af].map(|v| v.min(1.0));
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let f = |v: [u8; 3]| [v[0] as f32 / 255.0, v[1] as f32 / 255.0, v[2] as f32 / 255.0];
    let (out, alpha) = if max - min < 0.16 || max < 0.14 {
        // 中性色：亮度对调
        let t = luma(c).clamp(0.0, 1.0);
        let t = if t > 0.3 { t.powf(p.lift) } else { t * p.chip };
        let (paper, ink) = (f(p.paper), f(p.ink));
        let mixed = [paper[0] + (ink[0] - paper[0]) * t, paper[1] + (ink[1] - paper[1]) * t, paper[2] + (ink[2] - paper[2]) * t];
        let alpha = if t < 0.3 { a as f32 * p.dark_alpha } else { a as f32 };
        (mixed, alpha)
    } else {
        match (p.accent, theme_blue(c)) {
            (Some(accent), Some(bright)) => {
                // 主题蓝换成皮肤主题色；深蓝底色映射成浅色的主题色背景
                let acc = f(accent);
                if !bright {
                    let paper = f(p.paper);
                    ([paper[0] + (acc[0] - paper[0]) * 0.28, paper[1] + (acc[1] - paper[1]) * 0.28, paper[2] + (acc[2] - paper[2]) * 0.28], a as f32)
                } else {
                    (acc, a as f32)
                }
            }
            _ if light_paper(p) && max < 0.5 => {
                // 深色的彩色底块（选中态按钮、强调卡片）：在浅底上改为「纸色 + 淡淡的色相」，
                // 上面换成墨色的文字才看得清。
                let hue = [c[0] / max, c[1] / max, c[2] / max];
                let paper = f(p.paper);
                let k = 0.24;
                ([paper[0] + (hue[0] - paper[0]) * k, paper[1] + (hue[1] - paper[1]) * k, paper[2] + (hue[2] - paper[2]) * k], a as f32)
            }
            _ => {
                // 其它彩色（文字、图标、进度）：浅底上逐步加深，直到对比度至少 3:1。
                let mut out = c;
                let mut alpha = a as f32;
                if light_paper(p) {
                    // 按「叠在纸色上的实际效果」计算对比度，半透明的彩色字也要可读。
                    let paper = f(p.paper);
                    let seen = |c: [f32; 3], alpha: f32| {
                        let t = alpha / 255.0;
                        [paper[0] + (c[0] - paper[0]) * t, paper[1] + (c[1] - paper[1]) * t, paper[2] + (c[2] - paper[2]) * t]
                    };
                    let mut k = 1.0;
                    while k > 0.45 && contrast(seen(out, alpha), paper) < 3.0 {
                        k -= 0.05;
                        out = [c[0] * k, c[1] * k, c[2] * k];
                    }
                    // 只有原本就比较实（不是淡淡的装饰色）的半透明彩色才补足不透明度。
                    if a >= 110 {
                        while alpha < 255.0 && contrast(seen(out, alpha), paper) < 3.0 {
                            alpha = (alpha * 1.15).min(255.0);
                        }
                    }
                } else if max > 0.6 {
                    out = [c[0] * p.darken, c[1] * p.darken, c[2] * p.darken];
                }
                (out, alpha)
            }
        }
    };
    let alpha = alpha.round().clamp(0.0, 255.0);
    let to = |v: f32| (v.clamp(0.0, 1.0) * alpha).round() as u8;
    Color32::from_rgba_premultiplied(to(out[0]), to(out[1]), to(out[2]), alpha as u8)
}

/// 递归地给一个图形换色。
///
/// 直接改写图形里**已经生效**的颜色（包括文字每个字形的顶点色），所以淡入淡出
/// 乘进去的透明度、一段文字里的多种颜色都会原样保留。
pub fn recolor(shape: &mut Shape, p: &Palette) {
    recolor_with(shape, p, map_color)
}

/// 同 `recolor`，但用指定的换色函数。
fn recolor_with(shape: &mut Shape, p: &Palette, tone: Tone) {
    let palette = *p;
    let adjust = move |c: &mut Color32| {
        if *c != Color32::PLACEHOLDER {
            *c = tone(*c, &palette);
        }
    };
    match shape {
        Shape::Vec(shapes) => shapes.iter_mut().for_each(|s| recolor_with(s, p, tone)),
        // 带纹理的网格是图片（专辑封面、图标），保持原样；单色图标照常换色。
        Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() && !is_monochrome(mesh.texture_id) => {}
        // 带纹理的矩形同理，只换描边。
        Shape::Rect(s) if s.brush.as_ref().is_some_and(|b| !is_monochrome(b.fill_texture_id)) => adjust(&mut s.stroke.color),
        Shape::Callback(_) => {}
        _ => adjust_colors(shape, adjust),
    }
}

/// 实心彩色底块（按钮、开关、日期圆点）的范围；落在上面的白字与白色图标保持白色。
fn colored_fill(shape: &Shape) -> Option<egui::Rect> {
    let (color, rect) = match shape {
        Shape::Rect(r) if r.brush.is_none() => (r.fill, r.rect),
        Shape::Circle(c) => (c.fill, egui::Rect::from_center_size(c.center, egui::Vec2::splat(c.radius * 2.0))),
        Shape::Path(p) if p.closed => (p.fill, shape.visual_bounding_rect()),
        Shape::Mesh(m) if m.texture_id == egui::TextureId::default() && !m.vertices.is_empty() => (m.vertices[0].color, m.calc_bounds()),
        _ => return None,
    };
    let a = color.a();
    if a < 180 { return None; }
    let af = a as f32 / 255.0;
    let c = [color.r() as f32 / 255.0 / af, color.g() as f32 / 255.0 / af, color.b() as f32 / 255.0 / af];
    let (max, min) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    (max - min > 0.3 && max > 0.45 && rect.is_positive()).then_some(rect)
}

/// 图形的主要颜色是否为浅色中性色（白字、白色图标）。
fn light_neutral(shape: &Shape) -> bool {
    let color = match shape {
        Shape::Text(t) => t.galley.rows.iter().flat_map(|r| r.visuals.mesh.vertices.first()).next().map(|v| v.color).unwrap_or(t.fallback_color),
        Shape::Circle(c) => if c.fill.a() > 0 { c.fill } else { c.stroke.color },
        Shape::Path(p) => if p.fill.a() > 0 { p.fill } else if let ColorMode::Solid(c) = p.stroke.color { c } else { return false },
        Shape::Rect(r) => r.fill,
        Shape::LineSegment { stroke, .. } => stroke.color,
        Shape::Mesh(m) if m.texture_id == egui::TextureId::default() => match m.vertices.first() { Some(v) => v.color, None => return false },
        _ => return false,
    };
    let a = color.a();
    if a < 90 { return false; }
    let af = a as f32 / 255.0;
    let c = [color.r() as f32 / 255.0 / af, color.g() as f32 / 255.0 / af, color.b() as f32 / 255.0 / af];
    let (max, min) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    max - min < 0.12 && luma(c) > 0.8
}

/// 换色函数：输入按深色主题画的颜色，输出该皮肤下的颜色。
pub type Tone = fn(Color32, &Palette) -> Color32;

/// 本帧登记过自带配色的图形区段：(图层, 起, 止, 换色函数)，附带登记时的帧号，跨帧自动作废。
#[derive(Clone, Default)]
struct Toned { pass: u64, spans: Vec<(egui::LayerId, usize, usize, Tone)> }

fn toned_id() -> egui::Id { egui::Id::new("skin-paint-toned") }

/// 把 `from` 起到当前为止画在 `layer` 上的图形登记为自带配色：换肤时改用 `tone`，
/// 不走通用的「彩色逐步加深」。用于天气图标这类本身就有浅色版配色的插画式图形。
pub fn tone_since(ctx: &egui::Context, layer: egui::LayerId, from: usize, tone: Tone) {
    let to = shape_count(ctx, layer);
    if to <= from { return; }
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let list = d.get_temp_mut_or_default::<Toned>(toned_id());
        if list.pass != pass { list.pass = pass; list.spans.clear(); }
        list.spans.push((layer, from, to, tone));
    });
}

fn toned_spans(ctx: &egui::Context, layer: egui::LayerId) -> Vec<(usize, usize, Tone)> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<Toned>(toned_id()))
        .filter(|list| list.pass == pass)
        .map(|list| list.spans.iter().filter(|s| s.0 == layer).map(|s| (s.1, s.2, s.3)).collect())
        .unwrap_or_default()
}

/// 给某个图层中 `ranges` 范围内的图形换色。
///
/// 先找出这些范围里的实心彩色底块；之后画在它们上面的白字、白色图标保持白色
/// （「保存」按钮、播放键里的三角、开关的滑块），其余照常换色。
pub fn recolor_layer(ctx: &egui::Context, layer: egui::LayerId, ranges: &[(usize, usize)], p: &Palette) {
    let toned = toned_spans(ctx, layer);
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        let end = list.next_idx().0;
        let mut fills: Vec<(usize, egui::Rect)> = Vec::new();
        if light_paper(p) {
            for (i, entry) in list.all_entries().enumerate() {
                if ranges.iter().any(|&(from, to)| i >= from && i < to.min(end)) {
                    if let Some(rect) = colored_fill(&entry.shape) { fills.push((i, rect)); }
                }
            }
        }
        for &(from, to) in ranges {
            for i in from..to.min(end) {
                if let Some(&(_, _, tone)) = toned.iter().rev().find(|&&(a, b, _)| i >= a && i < b) {
                    list.mutate_shape(egui::layers::ShapeIdx(i), |s| recolor_with(&mut s.shape, p, tone));
                    continue;
                }
                list.mutate_shape(egui::layers::ShapeIdx(i), |s| {
                    if !fills.is_empty() && light_neutral(&s.shape) {
                        let center = s.shape.visual_bounding_rect().center();
                        if fills.iter().any(|&(at, rect)| at < i && rect.contains(center)) { return; }
                    }
                    recolor(&mut s.shape, p)
                });
            }
        }
    });
}

/// 当前图层已有的图形数量，用来标记换色范围。
pub fn shape_count(ctx: &egui::Context, layer: egui::LayerId) -> usize {
    ctx.graphics_mut(|g| g.entry(layer).next_idx().0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutrals_swap_lightness_and_keep_alpha() {
        assert_eq!(map_color(Color32::WHITE, &CREAM), Color32::from_rgb(46, 32, 22));
        assert_eq!(map_color(Color32::BLACK, &CREAM), Color32::from_rgb(255, 243, 228));
        let faint = map_color(Color32::from_white_alpha(40), &CREAM);
        assert_eq!(faint.a(), 40, "半透明高光保留透明度");
        assert!(faint.r() <= 8, "半透明的白仍按白处理：{faint:?}");
        let gray = map_color(Color32::from_rgb(142, 142, 153), &CREAM).to_srgba_unmultiplied();
        assert!(gray[0] > 46 && gray[0] < 255, "灰阶落在纸色与墨色之间");
        assert_eq!(map_color(Color32::TRANSPARENT, &CREAM), Color32::TRANSPARENT);
        // 毛玻璃浅色：深色卡片带上通透感，文字仍不透明
        assert!(map_color(Color32::from_rgb(22, 22, 24), &GLASS_LIGHT).a() < 230);
        assert_eq!(map_color(Color32::WHITE, &GLASS_LIGHT).a(), 255);
    }

    #[test]
    fn blue_accents_take_the_skin_color_and_other_hues_darken() {
        let blue = map_color(Color32::from_rgb(62, 172, 248), &CREAM);
        assert_eq!(blue, Color32::from_rgb(240, 138, 36), "强调蓝换成奶油皮肤的橙色");
        let deep = map_color(Color32::from_rgb(15, 38, 64), &CREAM).to_srgba_unmultiplied();
        assert!(deep[0] > 200, "深蓝底色变成浅橙底：{deep:?}");
        let brand = map_color(Color32::from_rgb(40, 126, 255), &CREAM).to_srgba_unmultiplied();
        assert!(brand[2] > brand[0], "用户选的品牌蓝不能被换成橙色：{brand:?}");
        let red = map_color(Color32::from_rgb(242, 48, 70), &CREAM).to_srgba_unmultiplied();
        assert!(red[0] > red[1] && red[0] > 150, "红色保留色相：{red:?}");
        let f = |c: [u8; 4]| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0];
        let paper = [255.0 / 255.0, 243.0 / 255.0, 228.0 / 255.0];
        for bright in [Color32::from_rgb(253, 104, 92), Color32::from_rgb(255, 196, 61), Color32::from_rgb(118, 196, 151), Color32::from_rgb(48, 209, 88)] {
            let m = map_color(bright, &CREAM).to_srgba_unmultiplied();
            assert!(crate::glass::contrast(f(m), paper) >= 2.95, "浅亮彩色在浅底上要加深到可读：{bright:?} -> {m:?}");
        }
        let save = map_color(Color32::from_rgb(82, 162, 255), &CREAM);
        assert_eq!(save, Color32::from_rgb(240, 138, 36), "界面按钮蓝跟随奶油主题色");
        let faded_red = map_color(Color32::from_rgb(253, 104, 92).gamma_multiply(0.6), &CREAM).to_srgba_unmultiplied();
        let fr = [faded_red[0] as f32 / 255.0, faded_red[1] as f32 / 255.0, faded_red[2] as f32 / 255.0];
        let seen = [paper[0] + (fr[0] - paper[0]) * faded_red[3] as f32 / 255.0, paper[1] + (fr[1] - paper[1]) * faded_red[3] as f32 / 255.0, paper[2] + (fr[2] - paper[2]) * faded_red[3] as f32 / 255.0];
        assert!(crate::glass::contrast(seen, paper) >= 2.9, "半透明的彩色字也要可读：{faded_red:?}");
        // 深色彩色底块（选中态）变成浅色淡彩，墨色文字压在上面清晰可读
        let chip = map_color(Color32::from_rgb(30, 58, 110), &GLASS_LIGHT).to_srgba_unmultiplied();
        assert!(chip[0] > 180 && chip[2] > chip[0], "深色选中底块变为浅淡蓝：{chip:?}");
        let ink = [22.0 / 255.0, 22.0 / 255.0, 26.0 / 255.0];
        assert!(crate::glass::contrast(f(chip), ink) >= 7.0);
    }

    #[test]
    fn dark_glass_brightens_gray_text_and_keeps_white_and_accents() {
        assert_eq!(map_color(Color32::WHITE, &GLASS_DARK), Color32::WHITE);
        let gray = map_color(Color32::from_rgb(142, 142, 153), &GLASS_DARK).to_srgba_unmultiplied();
        assert!(gray[0] > 165, "灰色文字提亮：{gray:?}");
        assert_eq!(map_color(Color32::from_rgb(62, 172, 248), &GLASS_DARK), Color32::from_rgb(62, 172, 248), "强调色不变");
        assert!(map_color(Color32::from_rgb(22, 22, 24), &GLASS_DARK).a() < 160, "卡片底带通透感");
    }

    #[test]
    fn fading_text_keeps_its_fade_after_recoloring() {
        // 切页时旧页面的文字按透明度淡出；换色后必须仍是半透明，不能和新页面叠在一起。
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let mut faded = painter.clone();
            faded.multiply_opacity(0.25);
            faded.text(egui::Pos2::ZERO, egui::Align2::LEFT_TOP, "淡出", egui::FontId::proportional(14.0), Color32::WHITE);
            recolor_layer(ctx, egui::LayerId::background(), &[(0, 1)], &CREAM);
            let shapes: Vec<_> = ctx.graphics_mut(|g| g.entry(egui::LayerId::background()).all_entries().cloned().collect());
            let Shape::Text(t) = &shapes[0].shape else { panic!() };
            let color = t.galley.rows.iter().flat_map(|r| r.visuals.mesh.vertices.iter()).next().unwrap().color;
            assert_eq!(color.a(), 64, "淡出透明度要保留：{color:?}");
            assert!(color.r() <= 13, "淡出的白字换成同样淡出的墨色：{color:?}");
            assert!(t.override_text_color.is_none(), "不能用统一颜色覆盖整段文字");
        });
    }

    #[test]
    fn monochrome_icons_follow_the_text_color() {
        let id = egui::TextureId::User(4242);
        mark_monochrome(id);
        let mut shape = Shape::image(id, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(10.0)), egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        recolor(&mut shape, &CREAM);
        let tint = match &shape { Shape::Rect(r) => r.fill, Shape::Mesh(m) => m.vertices[0].color, other => panic!("{other:?}") };
        assert_eq!(tint, Color32::from_rgb(46, 32, 22), "白色标识在浅底上变成墨色");
    }

    #[test]
    fn white_marks_on_colored_buttons_stay_white() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let layer = egui::LayerId::background();
            let painter = ctx.layer_painter(layer);
            painter.rect_filled(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(60.0, 24.0)), 12.0, Color32::from_rgb(82, 162, 255));
            painter.text(egui::pos2(30.0, 12.0), egui::Align2::CENTER_CENTER, "保存", egui::FontId::proportional(12.0), Color32::WHITE);
            painter.text(egui::pos2(120.0, 12.0), egui::Align2::CENTER_CENTER, "取消", egui::FontId::proportional(12.0), Color32::WHITE);
            recolor_layer(ctx, layer, &[(0, 3)], &CREAM);
            let shapes: Vec<_> = ctx.graphics_mut(|g| g.entry(layer).all_entries().cloned().collect());
            let first = |s: &Shape| match s { Shape::Text(t) => t.galley.rows.iter().flat_map(|r| r.visuals.mesh.vertices.first()).next().unwrap().color, _ => panic!() };
            assert_eq!(first(&shapes[1].shape), Color32::WHITE, "彩色按钮上的白字保持白色");
            assert_eq!(first(&shapes[2].shape), Color32::from_rgb(46, 32, 22), "普通白字换成墨色");
            let Shape::Rect(r) = &shapes[0].shape else { panic!() };
            assert_eq!(r.fill, Color32::from_rgb(240, 138, 36), "按钮本身换成主题色");
        });
    }

    #[test]
    fn textured_meshes_are_left_alone() {
        let mut image = egui::Mesh::with_texture(egui::TextureId::User(7));
        image.vertices.push(eframe::epaint::Vertex { pos: egui::Pos2::ZERO, uv: egui::Pos2::ZERO, color: Color32::WHITE });
        let mut shape = Shape::mesh(image);
        recolor(&mut shape, &CREAM);
        let Shape::Mesh(mesh) = shape else { unreachable!() };
        assert_eq!(mesh.vertices[0].color, Color32::WHITE);
    }
}
