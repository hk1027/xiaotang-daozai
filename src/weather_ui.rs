//! 天气页与天气设置：实况、七日预报与日照时段，图标全部用面板矢量绘制。
use crate::app::{App, Cmd, Env, WEATHER_PAGE, WEATHER_H};
use crate::paint::{self, Card, H};
use crate::theme::{self, W};
use crate::weather::{self, Kind};
use eframe::egui::{self, Color32};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const SUN: Color32 = Color32::from_rgb(255, 196, 72);
const SUN_CORE: Color32 = Color32::from_rgb(255, 168, 44);
const SUN_LIGHT: Color32 = Color32::from_rgb(255, 224, 120);
const MOON: Color32 = Color32::from_rgb(246, 236, 196);
const CLOUD: Color32 = Color32::from_rgb(238, 242, 250);
const CLOUD_SHADE: Color32 = Color32::from_rgb(160, 174, 200);
/// 雾带：深色皮肤下与云几乎同色，单列出来是为了浅色皮肤能换成更实的灰蓝。
const FOG: Color32 = Color32::from_rgb(228, 234, 244);
const STORM: Color32 = Color32::from_rgb(184, 192, 214);
const STORM_SHADE: Color32 = Color32::from_rgb(110, 118, 146);
const RAIN: Color32 = Color32::from_rgb(96, 170, 255);
const SNOW: Color32 = Color32::from_rgb(222, 238, 255);
const BOLT: Color32 = Color32::from_rgb(255, 208, 64);
const NIGHT: Color32 = Color32::from_rgb(150, 160, 255);
const WARN: Color32 = Color32::from_rgb(255, 166, 77);

/// 浅色皮肤（奶油、毛玻璃·浅）下的图标配色：(深色版, 浅色版)。
/// 通用换色会把彩色一律压暗到 3:1 对比度，太阳、月亮就成了暗金、橄榄色；
/// 天气图标是插画，浅底上要保持明朗：暖黄的太阳、琥珀色月亮、淡蓝灰的云、冰蓝的雪。
const LIGHT_TONES: [(Color32, Color32); 14] = [
    (SUN, Color32::from_rgb(255, 178, 36)),
    (SUN_CORE, Color32::from_rgb(255, 146, 28)),
    (SUN_LIGHT, Color32::from_rgb(255, 202, 64)),
    (MOON, Color32::from_rgb(244, 176, 52)),
    (CLOUD, Color32::from_rgb(200, 212, 230)),
    (CLOUD_SHADE, Color32::from_rgb(136, 154, 188)),
    (FOG, Color32::from_rgb(132, 148, 176)),
    (STORM, Color32::from_rgb(164, 176, 200)),
    (STORM_SHADE, Color32::from_rgb(110, 122, 152)),
    (RAIN, Color32::from_rgb(46, 144, 250)),
    (SNOW, Color32::from_rgb(92, 170, 238)),
    (BOLT, Color32::from_rgb(255, 176, 18)),
    (NIGHT, Color32::from_rgb(244, 176, 52)),
    (Color32::WHITE, Color32::WHITE),
];

/// 天气图标的换肤函数：浅色皮肤按 `LIGHT_TONES` 换成浅色版，保留原透明度；
/// 表外颜色与深色皮肤仍走通用换色。
pub(crate) fn light_tone(color: Color32, p: &crate::skin_paint::Palette) -> Color32 {
    let a = color.a();
    let light = p.paper.iter().map(|&v| v as u32).sum::<u32>() > 3 * 128;
    if a == 0 || !light { return crate::skin_paint::map_color(color, p); }
    let af = a as f32 / 255.0;
    let c = [color.r(), color.g(), color.b()].map(|v| (v as f32 / af).min(255.0));
    let near = |t: Color32| (c[0] - t.r() as f32).abs() <= 6.0 && (c[1] - t.g() as f32).abs() <= 6.0 && (c[2] - t.b() as f32).abs() <= 6.0;
    match LIGHT_TONES.iter().find(|(dark, _)| near(*dark)) {
        Some(&(_, to)) => Color32::from_rgba_premultiplied(
            (to.r() as f32 * af).round() as u8, (to.g() as f32 * af).round() as u8, (to.b() as f32 * af).round() as u8, a),
        None => crate::skin_paint::map_color(color, p),
    }
}

/// 把 `paint` 画出的图形登记为天气图标配色，浅色皮肤下不被通用换色压暗。
fn toned(c: &Card, paint: impl FnOnce()) {
    let (ctx, layer) = (c.ui.ctx(), c.painter.layer_id());
    let from = crate::skin_paint::shape_count(ctx, layer);
    paint();
    crate::skin_paint::tone_since(ctx, layer, from, light_tone);
}

/// 常驻胶囊上迷你天气图标的边长。
const PILL_ICON: f32 = 15.0;
/// 日照视图里太阳沿轨迹滑入的时长（秒）。
const SUN_SLIDE: f32 = 1.4;
/// 点击反馈的时长（秒）。
const TAP_FX: f32 = 0.38;
/// 进入天气页后，图标环境动效持续的秒数；之后静止，指针停在页面上时继续。
const AMBIENT_SECS: f64 = 6.0;

/// 胶囊上的温度文字：取整并带度符，负零归零。
/// 温度文字：取整并带度符，负零归零。零下的减号在绘制时另行处理，见 `paint_temp`。
fn pill_temp_text(snapshot: &crate::weather::Snapshot) -> String {
    temp_text(snapshot.temp)
}

fn temp_text(value: f32) -> String {
    let value = value.round() as i32;
    format!("{value}°")
}

/// 温度的墨迹度量（局部单位）：(墨迹宽度, 数字墨迹中心相对基线的偏移)。
/// 数字中线统一按「0」量取，所有温度共享同一条中线，零下与否都不会上下跳动。
fn temp_ink(c: &Card, text: &str, size: f32, weight: W) -> (f32, f32) {
    let zero = c.galley("0", size, weight, theme::WHITE);
    let asc = zero.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.pos.y).unwrap_or(size * c.s);
    let mid = (zero.mesh_bounds.center().y - asc) / c.s;
    let ink = |t: &str| { let b = c.galley(t, size, weight, theme::WHITE).mesh_bounds; (b.width() / c.s, b.min.x / c.s) };
    match text.strip_prefix('-') {
        Some(rest) => { let (dash, _) = ink("-"); (dash + size * 0.07 + ink(rest).0, mid) }
        None => (ink(text).0, mid),
    }
}

/// 按墨迹对齐画温度：`h` 决定 `x` 是墨迹的左缘、中心还是右缘。零下时减号用连字符的长短粗细手绘，
/// 但垂直放在数字中线上：普通连字符偏低，紧挨图标时显得下沉；排版减号「−」又太长。返回墨迹宽度。
fn paint_temp(c: &Card, x: f32, base: f32, text: &str, size: f32, weight: W, color: Color32, h: H) -> f32 {
    let (width, mid) = temp_ink(c, text, size, weight);
    let left = match h { H::Left => x, H::Center => x - width / 2.0, H::Right => x - width };
    let (digits, start) = match text.strip_prefix('-') {
        Some(rest) => {
            let dash = c.galley("-", size, weight, color).mesh_bounds;
            // 字形网格四周各带半个像素的抗锯齿留边，减掉才是连字符真实的长短与粗细。
            let pad = 1.0 / c.ui.ctx().pixels_per_point();
            let (len, thick) = ((dash.width() - pad) / c.s, ((dash.height() - pad) / c.s).max(1.2));
            c.fill(left, base + mid - thick / 2.0, len, thick, thick * 0.3, color);
            (rest, left + len + size * 0.07)
        }
        None => (text, left),
    };
    let bearing = c.galley(digits, size, weight, color).mesh_bounds.min.x / c.s;
    c.text(start - bearing, base, digits, size, weight, color, H::Left);
    width
}

/// 实况区地点文字的最大宽度。
const PLACE_MAX: f32 = 84.0;

fn place_width(c: &Card, place: &str) -> f32 { c.text_w(&c.fit(place, 10.0, W::Regular, PLACE_MAX), 10.0, W::Regular) }

/// 小下拉箭头：`open` 为 1 时翻转朝上。
fn chevron_down(c: &Card, x: f32, y: f32, open: f32) {
    let dy = 2.0 * (1.0 - 2.0 * open);
    c.path(&[(x - 3.0, y - dy), (x, y + dy), (x + 3.0, y - dy)], 1.3, paint::mix(theme::GRAY, theme::WHITE, open));
}

/// 图标与温度墨迹之间的留白。
const PILL_GAP: f32 = 3.0;

fn ease_out(t: f32) -> f32 { 1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3) }

/// 天气类型对应的氛围色：页面柔光、点击涟漪与设置页卡片共用。
fn tint(kind: Kind) -> Color32 {
    match kind {
        Kind::Sun | Kind::Partly => SUN,
        Kind::Moon | Kind::PartlyNight => NIGHT,
        Kind::Cloud | Kind::Fog => CLOUD_SHADE,
        Kind::Drizzle | Kind::Rain | Kind::Shower => RAIN,
        Kind::Snow => SNOW,
        Kind::Thunder => Color32::from_rgb(176, 146, 255),
    }
}

// ---------- 图标部件 ----------

/// 太阳：外层柔光 + 长短交替的光芒 + 双层圆心（暖橙底、亮黄面）。
fn sun_at(c: &Card, cx: f32, cy: f32, s: f32, spin: f32, detail: bool) {
    if detail { c.glow(cx, cy, s * 1.35, paint::alpha(SUN, 0.24)); }
    let width = (s * 0.15).clamp(1.1, 3.0);
    for i in 0..8 {
        let a = spin + i as f32 * TAU / 8.0;
        let (r0, r1) = if i % 2 == 0 { (0.64, 1.0) } else { (0.64, 0.86) };
        let (a0, a1) = ((cx + a.cos() * s * r0, cy + a.sin() * s * r0), (cx + a.cos() * s * r1, cy + a.sin() * s * r1));
        c.seg(a0, a1, width, SUN);
        if detail { c.circle(a0.0, a0.1, width / 2.0, SUN); c.circle(a1.0, a1.1, width / 2.0, SUN); }
    }
    c.circle(cx, cy, s * 0.48, SUN_CORE);
    c.circle(cx - s * 0.04, cy - s * 0.04, s * 0.39, SUN_LIGHT);
    if detail { c.glow(cx - s * 0.14, cy - s * 0.14, s * 0.26, Color32::from_white_alpha(110)); }
}

/// 弯月：外圆与偏右内圆之间的三角带，保留透明月牙缺口。
fn crescent(c: &Card, cx: f32, cy: f32, s: f32, color: Color32) {
    let (outer, inner, offset) = (0.62 * s, 0.55 * s, 0.30 * s);
    let x = (offset * offset + outer * outer - inner * inner) / (2.0 * offset);
    let y = (outer * outer - x * x).max(0.0).sqrt();
    let (outer_top, inner_top) = (y.atan2(x), y.atan2(x - offset));
    let mut mesh = egui::Mesh::default();
    let steps = 24;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a = outer_top + (TAU - 2.0 * outer_top) * t;
        let b = inner_top + (TAU - 2.0 * inner_top) * t;
        mesh.colored_vertex(c.pos(cx + a.cos() * outer, cy + a.sin() * outer), color);
        mesh.colored_vertex(c.pos(cx + offset + b.cos() * inner, cy + b.sin() * inner), color);
        if i > 0 { let n = i * 2; mesh.add_triangle(n - 2, n, n + 1); mesh.add_triangle(n - 2, n + 1, n - 1); }
    }
    // 网格本身没有抗锯齿，小尺寸时边缘发毛；沿内外两条弧各描一道约一像素的同色细线补出平滑边缘。
    // 两条弧分开画、不闭合：月牙尖角极锐，闭合折线在尖角处的斜接会拉出长尖刺。
    let outer_arc: Vec<_> = mesh.vertices.iter().step_by(2).map(|v| v.pos).collect();
    let inner_arc: Vec<_> = mesh.vertices.iter().skip(1).step_by(2).map(|v| v.pos).collect();
    c.painter.add(egui::Shape::mesh(mesh));
    let stroke = egui::Stroke::new(1.0 / c.ui.ctx().pixels_per_point(), color);
    c.painter.add(egui::Shape::line(outer_arc, stroke));
    c.painter.add(egui::Shape::line(inner_arc, stroke));
}

/// 四角星：夜空点缀。
fn sparkle(c: &Card, x: f32, y: f32, r: f32, color: Color32) {
    c.poly(&[(x, y - r), (x + r * 0.28, y), (x, y + r), (x - r * 0.28, y)], color);
    c.poly(&[(x - r, y), (x, y - r * 0.28), (x + r, y), (x, y + r * 0.28)], color);
}

/// 月亮：淡月光 + 月牙；大尺寸时旁边点两颗会闪的星。
fn moon_at(c: &Card, cx: f32, cy: f32, s: f32, t: f32, detail: bool) {
    if detail { c.glow(cx, cy, s * 1.25, paint::alpha(NIGHT, 0.20)); }
    crescent(c, cx - s * 0.06, cy, s, MOON);
    if detail {
        let twinkle = |k: f32| 0.55 + 0.45 * (t * 2.2 + k).sin();
        sparkle(c, cx + s * 0.52, cy - s * 0.46, s * 0.17, paint::alpha(MOON, twinkle(0.0)));
        sparkle(c, cx + s * 0.74, cy + s * 0.06, s * 0.10, paint::alpha(MOON, twinkle(2.1)));
    }
}

/// 云朵轮廓：三团云 + 平底，四个角都被圆覆盖，没有方角外露。
fn cloud_shape(c: &Card, cx: f32, cy: f32, s: f32, color: Color32) {
    c.circle(cx - s * 0.40, cy + s * 0.08, s * 0.30, color);
    c.circle(cx - s * 0.06, cy - s * 0.16, s * 0.42, color);
    c.circle(cx + s * 0.36, cy + s * 0.06, s * 0.32, color);
    c.fill(cx - s * 0.40, cy - s * 0.02, s * 0.76, s * 0.40, 0.0, color);
}

/// 立体云：底部错开一层阴影色，形成厚度。
fn cloud(c: &Card, cx: f32, cy: f32, s: f32, top: Color32, shade: Color32) {
    cloud_shape(c, cx + s * 0.03, cy + s * 0.09, s, shade);
    cloud_shape(c, cx, cy, s, top);
}

/// 雨滴：大尺寸画水滴形，小尺寸退化为斜线保证清晰。
fn drops(c: &Card, cx: f32, top: f32, s: f32, count: usize, len: f32, color: Color32, t: Option<f32>, detail: bool) {
    let gap = s * if count == 2 { 0.40 } else { 0.32 };
    for i in 0..count {
        let x = cx - gap * (count as f32 - 1.0) / 2.0 + i as f32 * gap;
        let (fall, fade) = match t {
            Some(t) => { let k = (t * 1.6 + i as f32 * 0.37).fract(); (k * s * 0.22, 1.0 - k * k) }
            None => (if i % 2 == 1 { s * 0.08 } else { 0.0 }, 1.0),
        };
        let y = top + fall;
        let color = paint::alpha(color, fade);
        if detail { paint::droplet(c, x - s * 0.04, y + s * len * 0.5, s * len, color); }
        else { c.seg((x, y), (x - s * 0.10, y + s * len), (s * 0.20).clamp(1.2, 2.2), color); }
    }
}

/// 雪花：六根主枝，每根在外侧带一对 V 形侧枝，中心一点；整体缓慢自转。
/// 常驻 15px 时侧枝更靠外、更短更粗，主枝末端不加点，避免糊成星号。
fn snowflake(c: &Card, cx: f32, cy: f32, r: f32, spin: f32, detail: bool) {
    let (width, branch_w, at, len, open) = if detail {
        ((r * 0.13).clamp(1.1, 2.4), (r * 0.11).clamp(1.0, 2.0), 0.55, 0.32, 0.75)
    } else {
        (1.35, 1.15, 0.62, 0.40, 0.85)
    };
    for k in 0..6 {
        let a = spin + k as f32 * PI / 3.0 - FRAC_PI_2;
        let (dx, dy) = (a.cos(), a.sin());
        c.seg((cx, cy), (cx + dx * r, cy + dy * r), width, SNOW);
        let (bx, by) = (cx + dx * r * at, cy + dy * r * at);
        for side in [-1.0_f32, 1.0] {
            let b = a + side * open;
            c.seg((bx, by), (bx + b.cos() * r * len, by + b.sin() * r * len), branch_w, SNOW);
        }
        if detail { c.circle(cx + dx * r, cy + dy * r, width * 0.55, SNOW); }
    }
    c.circle(cx, cy, width * if detail { 1.3 } else { 0.9 }, SNOW);
}

/// 闪电：平行四边形 + 三角拼成折线，均为凸多边形，避免凹多边形填充错位。
/// 比例偏矮胖，和其他实心图标的体量一致；不加光晕，与整套图标风格统一。
fn bolt(c: &Card, cx: f32, cy: f32, s: f32, flash: f32) {
    let p = |x: f32, y: f32| (cx + x * s, cy + y * s);
    let color = paint::alpha(BOLT, 0.6 + 0.4 * flash);
    c.poly(&[p(-0.08, -0.84), p(0.56, -0.84), p(0.18, -0.06), p(-0.52, 0.06)], color);
    c.poly(&[p(-0.24, -0.16), p(0.52, -0.16), p(-0.26, 0.86)], color);
}

/// 雾：四条错落的圆头横带，透明度自上而下递减，动态时左右缓慢漂移。
fn haze(c: &Card, cx: f32, cy: f32, s: f32, t: Option<f32>, detail: bool) {
    let bands: &[(f32, f32)] = if detail { &[(-0.10, 1.25), (0.16, 1.55), (-0.18, 1.30), (0.10, 0.95)] } else { &[(-0.10, 1.40), (0.14, 1.60), (-0.12, 1.20)] };
    let h = (s * 0.16).max(1.6);
    let gap = s * if detail { 0.44 } else { 0.58 };
    let top = cy - gap * (bands.len() as f32 - 1.0) / 2.0;
    for (i, &(dx, w)) in bands.iter().enumerate() {
        let shift = t.map(|t| (t * 0.7 + i as f32 * 1.4).sin() * s * 0.08).unwrap_or(0.0);
        let y = top + i as f32 * gap;
        c.fill(cx + s * (dx - w / 2.0) + shift, y - h / 2.0, s * w, h, h / 2.0, paint::alpha(FOG, 0.95 - i as f32 * 0.16));
    }
}

/// 细雨：一大两小三颗水滴错落排布，动态时轻轻上下浮动。
fn droplets(c: &Card, cx: f32, cy: f32, s: f32, t: Option<f32>) {
    let bob = |k: f32| t.map(|t| (t * 2.0 + k).sin() * s * 0.05).unwrap_or(0.0);
    paint::droplet(c, cx - s * 0.20, cy + s * 0.12 + bob(0.0), s * 1.05, RAIN);
    paint::droplet(c, cx + s * 0.46, cy - s * 0.42 + bob(1.6), s * 0.56, paint::alpha(RAIN, 0.85));
    paint::droplet(c, cx + s * 0.50, cy + s * 0.46 + bob(3.1), s * 0.44, paint::alpha(RAIN, 0.7));
    // 大水滴左上的高光，让它像水而不是色块。
    if s >= 11.0 { c.circle(cx - s * 0.34, cy + s * 0.22, s * 0.08, Color32::from_white_alpha(150)); }
}

/// 静态图标：常驻胶囊、侧栏、七日预报与设置页共用。
pub(crate) fn icon(c: &Card, cx: f32, cy: f32, size: f32, kind: Kind) { icon_at(c, cx, cy, size, kind, None) }

/// 带动效的图标：`t` 为秒，雨滴下落、雪花自转、雾带漂移、光芒缓转、星星闪烁。
/// 只有晴间多云、阴、雨、阵雨用云；雪、雾、细雨、雷各用独立的主体形状，常驻栏一眼能分辨。
pub(crate) fn icon_at(c: &Card, cx: f32, cy: f32, size: f32, kind: Kind, t: Option<f32>) {
    toned(c, || icon_shapes(c, cx, cy, size, kind, t));
}

fn icon_shapes(c: &Card, cx: f32, cy: f32, size: f32, kind: Kind, t: Option<f32>) {
    let s = size / 2.0;
    let detail = size >= 22.0;
    let time = t.unwrap_or(0.0);
    let float = t.map(|t| (t * 0.9).sin() * s * 0.03).unwrap_or(0.0);
    match kind {
        Kind::Sun => sun_at(c, cx, cy, s * 0.92, time * 0.25, detail),
        Kind::Moon => moon_at(c, cx - s * 0.06, cy, s * 1.08, time, detail),
        Kind::Partly => {
            sun_at(c, cx + s * 0.28, cy - s * 0.26, s * 0.60, time * 0.25, detail);
            cloud(c, cx - s * 0.10 + float, cy + s * 0.22, s * 0.88, CLOUD, CLOUD_SHADE);
        }
        Kind::PartlyNight => {
            moon_at(c, cx + s * 0.26, cy - s * 0.28, s * 0.66, time, false);
            cloud(c, cx - s * 0.10 + float, cy + s * 0.22, s * 0.88, CLOUD, CLOUD_SHADE);
        }
        Kind::Cloud => {
            cloud_shape(c, cx + s * 0.30 - float, cy - s * 0.22, s * 0.58, CLOUD_SHADE);
            cloud(c, cx - s * 0.06 + float, cy + s * 0.12, s * 0.90, CLOUD, CLOUD_SHADE);
        }
        Kind::Fog => haze(c, cx, cy, s, t, detail),
        Kind::Drizzle => droplets(c, cx, cy, s, t),
        Kind::Rain => {
            cloud(c, cx, cy - s * 0.20, s * 0.88, STORM, STORM_SHADE);
            drops(c, cx, cy + s * 0.32, s, 3, 0.36, RAIN, t, detail);
        }
        Kind::Shower => {
            sun_at(c, cx + s * 0.34, cy - s * 0.40, s * 0.42, time * 0.25, false);
            cloud(c, cx - s * 0.06 + float, cy - s * 0.14, s * 0.84, STORM, STORM_SHADE);
            drops(c, cx - s * 0.06, cy + s * 0.36, s, 2, 0.34, RAIN, t, detail);
        }
        Kind::Snow => snowflake(c, cx, cy, s * 0.92, time * 0.35, detail),
        Kind::Thunder => {
            let flash = t.map(|t| { let k = (t * 0.45).fract(); if k < 0.06 || (0.10..0.13).contains(&k) { 1.0 } else { 0.6 } }).unwrap_or(1.0);
            bolt(c, cx + s * 0.02, cy, s, flash);
        }
    }
}

// ---------- 小图形 ----------

fn thermo_glyph(c: &Card, x: f32, y: f32, color: Color32) {
    c.stroke(x - 2.0, y - 6.5, 4.0, 9.0, 2.0, 1.1, color);
    c.circle(x, y + 3.6, 2.9, color);
    c.seg((x, y - 2.5), (x, y + 2.0), 1.4, color);
}

fn wind_glyph(c: &Card, x: f32, y: f32, color: Color32) {
    c.seg((x - 6.0, y - 2.5), (x + 2.0, y - 2.5), 1.3, color);
    c.arc(x + 2.0, y - 4.5, 2.0, FRAC_PI_2, -PI, 1.3, color);
    c.seg((x - 6.0, y + 1.5), (x + 4.0, y + 1.5), 1.3, color);
    c.arc(x + 4.0, y + 3.5, 2.0, -FRAC_PI_2, PI, 1.3, color);
}

/// 日出图形：`rise` 让太阳从地平线后升起（0..1），只画地平线以上的部分。
fn sunrise_glyph(c: &Card, x: f32, y: f32, rise: f32, color: Color32) {
    c.clipped(x - 8.0, y - 12.0, 16.0, 14.5).ring(x, y + 2.5 - 2.4 * rise, 4.2, 1.4, color);
    c.seg((x - 6.5, y + 2.5), (x + 6.5, y + 2.5), 1.3, color);
    for a in [-2.36_f32, -1.57, -0.79] { c.seg((x + a.cos() * 5.6, y + 2.5 + a.sin() * 5.6), (x + a.cos() * 7.2, y + 2.5 + a.sin() * 7.2), 1.1, color); }
}

/// 定位标记：圆头 + 尖底，中间留孔。
fn location_glyph(c: &Card, x: f32, y: f32, color: Color32, hole: Color32) {
    c.circle(x, y - 1.6, 3.4, color);
    c.poly(&[(x - 2.9, y - 0.2), (x + 2.9, y - 0.2), (x, y + 4.2)], color);
    c.circle(x, y - 1.6, 1.3, hole);
}

/// 实况小卡：图形 + 标签 + 数值。
fn stat_tile(c: &Card, x: f32, y: f32, w: f32, label: &str, value: &str, glyph: impl Fn(&Card, f32, f32)) {
    c.fill(x, y, w, 34.0, 9.0, Color32::from_white_alpha(9));
    glyph(c, x + 14.0, y + 17.0);
    c.text(x + 27.0, y + 14.0, label, 9.0, W::Regular, theme::GRAY, H::Left);
    c.text(x + 27.0, y + 28.5, &c.fit(value, 11.5, W::Semibold, w - 31.0), 11.5, W::Semibold, theme::WHITE, H::Left);
}

fn weather_now(env: &Env) -> i64 {
    use chrono::TimeZone;
    chrono::NaiveDateTime::parse_from_str(&format!("{} {}", env.today, env.clock), "%Y-%m-%d %H:%M:%S").ok()
        .and_then(|time| chrono::Local.from_local_datetime(&time).earliest()).map(|time| time.timestamp())
        .unwrap_or_else(|| chrono::Utc::now().timestamp())
}

// ---------- 日照视图 ----------

/// 日照视图：左侧倒计时，右侧太阳沿弧线滑到当前位置，底部 24 小时昼夜条。
/// `slide` 为入场进度（0..1），太阳、亮轨与「现在」指针都随它从日出点出发。
fn paint_sun_times(c: &Card, snapshot: &weather::Snapshot, now: i64, t: f32, slide: f32) {
    let times = snapshot.sun_times.as_ref();
    let progress = times.and_then(|times| times.progress(now));
    let today = times.filter(|times| (times.day_start..times.day_end).contains(&now));
    let rise = snapshot.sunrise.as_deref().unwrap_or("--:--");
    let set = snapshot.sunset.as_deref().unwrap_or("--:--");

    // 顶部：返回胶囊 + 地点。
    c.fill(34.0, 37.0, 76.0, 19.0, 9.5, Color32::from_white_alpha(14));
    c.path(&[(46.0, 42.5), (42.5, 46.5), (46.0, 50.5)], 1.4, theme::GRAY);
    c.text(51.0, 50.5, "返回天气", 10.0, W::Regular, theme::GRAY, H::Left);
    let place = c.fit(&snapshot.place, 10.0, W::Regular, 150.0);
    let place_w = c.text(384.0, 50.5, &place, 10.0, W::Regular, theme::GRAY, H::Right);
    location_glyph(c, 384.0 - place_w - 7.0, 46.0, theme::GRAY, Color32::BLACK);

    // 左侧：倒计时与下一事件。
    let mut target = None;
    if let Some(times) = today {
        if now < times.sunset {
            let (title, event, label, index) = if now < times.sunrise { ("距离日出", times.sunrise, rise, 0) } else { ("距离日落", times.sunset, set, 2) };
            let minutes = ((event - now).max(0) + 59) / 60;
            c.text(40.0, 78.0, title, 10.5, W::Regular, theme::GRAY, H::Left);
            c.text(39.0, 111.0, &format!("{:02}:{:02}", minutes / 60, minutes % 60), 30.0, W::Bold, theme::WHITE, H::Left);
            c.text(40.0, 124.0, "小时 : 分钟", 9.5, W::Regular, theme::DIM, H::Left);
            let text = format!("{} {label}", if index == 0 { "日出" } else { "日落" });
            let w = c.text_w(&text, 10.5, W::Semibold);
            c.fill(36.0, 133.0, w + 16.0, 19.0, 9.5, paint::alpha(SUN, 0.16));
            c.text(44.0, 147.0, &text, 10.5, W::Semibold, SUN, H::Left);
            target = Some(index);
        } else {
            c.text(40.0, 78.0, "今日已日落", 10.5, W::Regular, theme::GRAY, H::Left);
            c.text(39.0, 111.0, set, 30.0, W::Bold, theme::WHITE, H::Left);
            c.text(40.0, 128.0, "白昼已经结束", 10.0, W::Regular, theme::DIM, H::Left);
        }
        if let Some(progress) = progress {
            c.text(40.0, 168.0, &format!("日照已过 {:.0}%", progress * 100.0), 9.5, W::Regular, theme::DIM, H::Left);
        }
    } else {
        c.text(40.0, 78.0, "日出时间", 10.5, W::Regular, theme::GRAY, H::Left);
        c.text(39.0, 111.0, rise, 30.0, W::Bold, theme::WHITE, H::Left);
        c.text(40.0, 132.0, "日照进度待更新", 9.5, W::Regular, WARN, H::Left);
    }

    // 右侧：天空弧线。白昼按时间线性映射到半个正弦拱。
    let (x0, x1, horizon, height) = (190.0, 380.0, 128.0, 60.0);
    let point = |pos: f32| (x0 + pos * (x1 - x0), horizon - height * (PI * pos).sin());
    let shown = progress.map(|p| p * ease_out(slide));
    let samples = 60;
    let track: Vec<_> = (0..=samples).map(|i| point(i as f32 / samples as f32)).collect();
    // 弧下天空：已走过的部分是暖色，其余淡白，都向地平线渐隐。
    let mut sky = egui::Mesh::default();
    for (i, &(x, y)) in track.iter().enumerate() {
        let lit = shown.is_some_and(|p| i as f32 / samples as f32 <= p);
        let top = if lit { paint::alpha(SUN, 0.26) } else { Color32::from_white_alpha(4) };
        sky.colored_vertex(c.pos(x, y), top);
        sky.colored_vertex(c.pos(x, horizon), Color32::TRANSPARENT);
        if i > 0 { let n = i as u32 * 2; sky.add_triangle(n - 2, n - 1, n); sky.add_triangle(n - 1, n + 1, n); }
    }
    c.painter.add(egui::Shape::mesh(sky));
    let screen: Vec<_> = track.iter().map(|&(x, y)| c.pos(x, y)).collect();
    c.painter.extend(egui::Shape::dashed_line(&screen, egui::Stroke::new(1.2 * c.s, Color32::from_white_alpha(70)), 3.0 * c.s, 3.0 * c.s));
    if let Some(shown) = shown.filter(|p| *p > 0.0) {
        let mut lit: Vec<_> = track.iter().take_while(|&&(x, _)| x < point(shown).0).copied().collect();
        lit.push(point(shown));
        c.path(&lit, 2.0, SUN);
    }
    c.seg((x0 - 10.0, horizon), (x1 + 10.0, horizon), 0.8, Color32::from_white_alpha(36));

    let midpoint = times.map(|times| times.clock(times.midpoint())).unwrap_or_else(|| "--:--".into());
    for (index, (pos, label, time)) in [(0.0, "日出", rise), (0.5, "白昼中点", midpoint.as_str()), (1.0, "日落", set)].into_iter().enumerate() {
        let (x, y) = point(pos);
        let next = target == Some(index);
        if index != 1 { c.circle(x, y, 2.6, if next { SUN } else { theme::GRAY }); }
        else { c.seg((x, y - 4.0), (x, y + 2.0), 1.0, Color32::from_white_alpha(60)); }
        c.text(x, 144.0, label, 9.5, W::Regular, if next { SUN } else { theme::GRAY }, H::Center);
        c.text(x, 158.0, time, 11.0, W::Semibold, theme::WHITE, H::Center);
    }

    if let Some(shown) = shown {
        let (x, y) = point(shown);
        // 滑动中拖一串渐隐的光点，停稳后收起。
        let moving = 1.0 - ease_out(slide);
        // 小巧思一：入场瞬间日出点「被点亮」，一圈细环从日出标记处漾开，太阳由此出发。
        if slide < 0.4 {
            let k = slide / 0.4;
            let (rx, ry) = point(0.0);
            c.ring(rx, ry, 3.0 + 9.0 * ease_out(k), 1.2 * (1.0 - k) + 0.2, paint::alpha(SUN, 0.8 * (1.0 - k)));
        }
        for k in 1..7 {
            let back = shown - k as f32 * 0.028;
            if back <= 0.0 { break; }
            let (px, py) = point(back);
            let fade = 1.0 - k as f32 / 7.0;
            toned(c, || c.circle(px, py, 2.2 * fade + 0.4, paint::alpha(SUN_LIGHT, 0.75 * fade * moving)));
        }
        let pulse = 1.0 + 0.08 * (t * 2.2).sin();
        // 小巧思二：太阳到位时轻轻「弹」一下再停稳。
        let settle = ((slide - 0.82) / 0.18).clamp(0.0, 1.0);
        let pop = 1.0 + 0.2 * (PI * settle).sin();
        toned(c, || { c.glow(x, y, 26.0 * pulse, paint::alpha(SUN, 0.22)); sun_at(c, x, y, 8.0 * pop, t * 0.5, true); });
    } else if today.is_some() {
        // 夜间：月亮从弧心升起，周围星星闪烁。
        let rise = ease_out(slide);
        let night = c.faded(rise);
        toned(c, || {
            for (i, &(sx, sy, r)) in [(214.0, 82.0, 2.4), (246.0, 64.0, 1.6), (322.0, 60.0, 2.0), (352.0, 86.0, 1.5), (300.0, 104.0, 1.3), (230.0, 112.0, 1.2)].iter().enumerate() {
                sparkle(&night, sx, sy, r, paint::alpha(MOON, 0.35 + 0.35 * (t * 1.8 + i as f32 * 1.3).sin().abs()));
            }
            night.glow(285.0, 92.0 + 10.0 * (1.0 - rise), 22.0, paint::alpha(NIGHT, 0.20));
            moon_at(&night, 285.0, 92.0 + 10.0 * (1.0 - rise), 10.0, t, false);
        });
    }

    // 底部：24 小时昼夜条，白昼段暖色，夜晚段深蓝，白点指向现在。
    if let Some(times) = times {
        let day = (times.sunset - times.sunrise).max(0) / 60;
        let span = (times.day_end - times.day_start).max(1);
        let night = (span / 60 - day).max(0);
        c.text(40.0, 186.0, &format!("白昼 {}小时{:02}分", day / 60, day % 60), 10.0, W::Regular, theme::GRAY, H::Left);
        c.text(384.0, 186.0, &format!("夜晚 {}小时{:02}分", night / 60, night % 60), 10.0, W::Regular, theme::DIM, H::Right);
        let (bx, bw, by, bh) = (40.0, 344.0, 192.0, 7.0);
        let frac = |at: i64| ((at - times.day_start) as f32 / span as f32).clamp(0.0, 1.0);
        c.fill(bx, by, bw, bh, bh / 2.0, Color32::from_rgb(38, 50, 92));
        let (a, b) = (bx + bw * frac(times.sunrise), bx + bw * frac(times.sunset));
        let mut light = egui::Mesh::default();
        let r = c.rect(a, by, (b - a).max(0.0), bh);
        light.colored_vertex(r.left_top(), Color32::from_rgb(255, 140, 84));
        light.colored_vertex(egui::pos2(r.center().x, r.top()), SUN_LIGHT);
        light.colored_vertex(r.right_top(), Color32::from_rgb(255, 128, 92));
        light.colored_vertex(r.left_bottom(), Color32::from_rgb(255, 140, 84));
        light.colored_vertex(egui::pos2(r.center().x, r.bottom()), SUN_LIGHT);
        light.colored_vertex(r.right_bottom(), Color32::from_rgb(255, 128, 92));
        for (p, q) in [(0, 1), (1, 2)] { light.add_triangle(p, q, p + 3); light.add_triangle(q, q + 3, p + 3); }
        c.painter.add(egui::Shape::mesh(light));
        c.text(bx, 211.0, "0:00", 8.5, W::Regular, theme::DIM, H::Left);
        c.text(bx + bw, 211.0, "24:00", 8.5, W::Regular, theme::DIM, H::Right);
        if today.is_some() {
            let x = bx + bw * frac(now) * ease_out(slide);
            c.circle(x, by + bh / 2.0, 5.2, Color32::BLACK);
            c.circle(x, by + bh / 2.0, 3.6, theme::WHITE);
            c.text(x.clamp(bx + 44.0, bx + bw - 48.0), 211.0, &format!("现在 {}", times.clock(now)), 8.5, W::Semibold, theme::WHITE, H::Center);
        }
    }
}

// ---------- 天气视图 ----------

/// `lift` / `press`：指针停在、按在日照入口卡上的过渡量。
fn paint_forecast(c: &Card, snapshot: &weather::Snapshot, t: f32, lift: f32, press: f32) {
    // 上方实况：动态大图标 + 温度 + 描述，右侧四张小卡。
    let kind = weather::kind(snapshot.code, snapshot.day);
    c.glow(66.0, 72.0, 56.0, paint::alpha(tint(kind), 0.16));
    icon_at(c, 66.0, 72.0, 46.0, kind, Some(t));
    paint_temp(c, 99.0, 82.0, &pill_temp_text(snapshot), 34.0, W::Bold, theme::WHITE, H::Left);
    let desc = c.fit(snapshot.description(), 12.0, W::Semibold, 104.0);
    let desc_w = c.text(99.0, 100.0, &desc, 12.0, W::Semibold, theme::WHITE, H::Left);
    if let (Some(high), Some(low)) = (snapshot.high, snapshot.low) {
        let range = format!("{high:.0}° / {low:.0}°");
        if desc_w + 6.0 + c.text_w(&range, 10.5, W::Regular) <= 106.0 { c.text(99.0 + desc_w + 6.0, 100.0, &range, 10.5, W::Regular, theme::GRAY, H::Left); }
    }
    location_glyph(c, 103.0, 112.0, theme::DIM, Color32::BLACK);
    c.text(110.0, 116.0, &c.fit(&snapshot.place, 10.0, W::Regular, PLACE_MAX), 10.0, W::Regular, theme::GRAY, H::Left);

    let (tx, tw) = ([212.0, 300.0], 84.0);
    let dash = "—".to_string();
    stat_tile(c, tx[0], 38.0, tw, "体感", &snapshot.feels.map(|v| format!("{v:.0}°")).unwrap_or(dash.clone()), |c, x, y| thermo_glyph(c, x, y, WARN));
    stat_tile(c, tx[1], 38.0, tw, "湿度", &snapshot.humidity.map(|v| format!("{v}%")).unwrap_or(dash.clone()), |c, x, y| paint::droplet(c, x, y, 11.0, RAIN));
    stat_tile(c, tx[0], 77.0, tw, "风速", &snapshot.wind.map(|v| format!("{v:.0} km/h")).unwrap_or(dash), |c, x, y| wind_glyph(c, x, y, theme::CYAN));
    // 第四张是日照入口：暖色底，箭头轻轻摆动提示可点；悬停时小太阳从地平线升起，按下时卡片微缩。
    let entry = c.scaled(tx[1] + tw / 2.0, 94.0, 1.0 - 0.05 * press);
    let c = &entry;
    c.fill(tx[1], 77.0, tw, 34.0, 9.0, paint::alpha(SUN, 0.13 + 0.07 * lift));
    sunrise_glyph(c, tx[1] + 14.0, 77.0 + 16.0, lift, SUN);
    c.text(tx[1] + 27.0, 77.0 + 14.5, &format!("日出 {}", snapshot.sunrise.as_deref().unwrap_or("--:--")), 9.5, W::Regular, theme::GRAY, H::Left);
    c.text(tx[1] + 27.0, 77.0 + 28.0, &format!("日落 {}", snapshot.sunset.as_deref().unwrap_or("--:--")), 9.5, W::Semibold, theme::WHITE, H::Left);
    let nudge = (t * 3.0).sin() * 1.2;
    c.path(&[(tx[1] + tw - 9.0 + nudge, 90.0), (tx[1] + tw - 6.0 + nudge, 94.0), (tx[1] + tw - 9.0 + nudge, 98.0)], 1.3, paint::alpha(SUN, 0.9));

    // 下方七日预报：图标、降水概率，以及贯穿七天的高低温曲线。
    c.seg((24.0, 122.0), (395.0, 122.0), 0.8, Color32::from_white_alpha(16));
    let span = 371.0 / weather::FORECAST_DAYS as f32;
    let col = |index: usize| 24.0 + span * (index as f32 + 0.5);
    let days: Vec<_> = snapshot.days.iter().take(weather::FORECAST_DAYS).collect();
    let (lo, hi) = days.iter().fold((f32::MAX, f32::MIN), |(lo, hi), day| (lo.min(day.low), hi.max(day.high)));
    let curve_y = |temp: f32| 202.0 - (temp - lo) / (hi - lo).max(1.0) * 12.0;
    for index in 0..weather::FORECAST_DAYS {
        let x = col(index);
        if index == 0 { c.fill(x - 24.0, 125.0, 48.0, 94.0, 9.0, Color32::from_white_alpha(9)); }
        let Some(day) = days.get(index) else { c.text(x, 172.0, "—", 12.0, W::Regular, theme::DIM, H::Center); continue; };
        c.text(x, 135.0, &day.label, 9.5, if index == 0 { W::Semibold } else { W::Regular }, if index == 0 { theme::WHITE } else { theme::GRAY }, H::Center);
        let date = day.date.get(5..10).map(|date| date.replace('-', "/")).unwrap_or_else(|| "--/--".into());
        c.text(x, 146.0, &date, 8.5, W::Regular, theme::DIM, H::Center);
        icon(c, x, 159.0, 17.0, weather::kind(day.code, true));
        if day.rain >= 20 { c.text(x, 177.0, &format!("{}%", day.rain), 8.5, W::Semibold, RAIN, H::Center); }
        paint_temp(c, x, 187.0, &temp_text(day.high), 11.0, W::Semibold, theme::WHITE, H::Center);
        paint_temp(c, x, 216.0, &temp_text(day.low), 10.0, W::Regular, theme::GRAY, H::Center);
    }
    if days.len() >= 2 {
        let highs: Vec<_> = days.iter().enumerate().map(|(i, d)| (col(i), curve_y(d.high))).collect();
        let lows: Vec<_> = days.iter().enumerate().map(|(i, d)| (col(i), curve_y(d.low))).collect();
        let mut band = egui::Mesh::default();
        for i in 0..highs.len() {
            band.colored_vertex(c.pos(highs[i].0, highs[i].1), paint::alpha(SUN, 0.26));
            band.colored_vertex(c.pos(lows[i].0, lows[i].1), paint::alpha(RAIN, 0.22));
            if i > 0 { let n = i as u32 * 2; band.add_triangle(n - 2, n - 1, n); band.add_triangle(n - 1, n + 1, n); }
        }
        c.painter.add(egui::Shape::mesh(band));
        c.path(&highs, 1.5, paint::alpha(SUN, 0.95));
        c.path(&lows, 1.5, paint::alpha(RAIN, 0.95));
        for (i, (&(hx, hy), &(lx, ly))) in highs.iter().zip(&lows).enumerate() {
            let r = if i == 0 { 2.6 } else { 2.0 };
            c.circle(hx, hy, r, SUN);
            c.circle(lx, ly, r, RAIN);
        }
    }
}

/// 点击反馈：指尖处一个实心小点收拢，同时一圈细环轻轻扩开，克制、不抢内容。
fn paint_tap(c: &Card, x: f32, y: f32, k: f32, color: Color32) {
    let e = ease_out(k);
    let fade = 1.0 - k;
    c.ring(x, y, 4.0 + 13.0 * e, 0.3 + 1.1 * fade, paint::alpha(color, 0.7 * fade));
    c.circle(x, y, 3.2 * (1.0 - e), paint::alpha(color, 0.85 * fade));
}

/// 环境动效时钟：只在 `active` 时前进并以约 30 帧请求重绘；静止时不再请求，
/// 页面常开也不会持续占用 CPU / GPU。恢复时从停下的位置接着动，不会跳帧。
fn ambient_clock(ctx: &egui::Context, id: egui::Id, active: bool, now: f64) -> f32 {
    let (mut clock, last) = ctx.data(|d| d.get_temp::<(f64, f64)>(id)).unwrap_or((0.0, now));
    if active {
        clock += (now - last).clamp(0.0, 0.1);
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
    ctx.data_mut(|d| d.insert_temp(id, (clock, now)));
    clock as f32
}

/// 设置页顶部卡片的背景渐变：按天气类型与昼夜换色。
fn hero_colors(kind: Kind) -> (Color32, Color32) {
    match kind {
        Kind::Sun | Kind::Partly => (Color32::from_rgb(70, 146, 236), Color32::from_rgb(38, 94, 186)),
        Kind::Moon | Kind::PartlyNight => (Color32::from_rgb(44, 52, 104), Color32::from_rgb(20, 24, 54)),
        Kind::Cloud | Kind::Fog => (Color32::from_rgb(104, 122, 152), Color32::from_rgb(60, 72, 98)),
        Kind::Drizzle | Kind::Rain | Kind::Shower => (Color32::from_rgb(70, 104, 152), Color32::from_rgb(36, 54, 90)),
        Kind::Snow => (Color32::from_rgb(112, 146, 196), Color32::from_rgb(66, 94, 140)),
        Kind::Thunder => (Color32::from_rgb(76, 66, 120), Color32::from_rgb(32, 28, 60)),
    }
}

impl App {
    fn weather_is_stale(&self,snapshot:&weather::Snapshot)->bool {
        let (interval,failed)=if self.weather_source.displayed==crate::weather_source_ui::Provider::Qweather {(crate::qweather::CURRENT_INTERVAL,self.weather_source.runtime.current_refresh_failed())}else{(weather::INTERVAL,self.weather.failures>=2)};
        failed&&chrono::Utc::now().timestamp()-snapshot.at>interval*2
    }
    fn weather_can_refresh(&self)->bool {
        self.module_on(WEATHER_PAGE)&&if self.weather_source.displayed==crate::weather_source_ui::Provider::Qweather {self.weather_source.client.is_some()&&self.weather_source.runtime.can_refresh(chrono::Utc::now().timestamp())}else{self.weather.can_refresh(&self.settings.weather_city,self.t)}
    }
    fn weather_refresh(&mut self){
        if !self.module_on(WEATHER_PAGE){return;}
        if self.weather_source.displayed==crate::weather_source_ui::Provider::Qweather {if !cfg!(test){if let Some(client)=&self.weather_source.client {self.weather_source.runtime.refresh(client,chrono::Utc::now().timestamp(),true);}}}
        else{self.weather.refresh_manual(&self.settings.weather_city,self.t);}
    }
    /// 后台取数的轮询与自动刷新；数据过期就重新拉一次。
    pub(crate) fn weather_tick(&mut self) {
        let enabled=self.module_on(WEATHER_PAGE);
        if self.page!=WEATHER_PAGE {self.weather.picker_open=false;}
        self.weather_source.poll(enabled);
        if self.weather_source.displayed==crate::weather_source_ui::Provider::Qweather {
            let now=chrono::Utc::now().timestamp();
            if self.weather_source.runtime.poll(enabled,now) {
                self.weather.snapshot=self.weather_source.runtime.snapshot();
            }
            if enabled&&!cfg!(test){if let Some(client)=&self.weather_source.client {self.weather_source.runtime.refresh(client,now,false);}}
            if let Some(snapshot)=&mut self.weather.snapshot {
                if let Some(sun)=&snapshot.sun_times {snapshot.day=now>=sun.sunrise&&now<sun.sunset;}
            }
            self.weather.loading=self.weather_source.runtime.loading();
            self.weather.error=self.weather_source.runtime.error.map(|e|e.text().to_string()).or_else(||if self.weather_source.client.is_none(){Some("尚未配置和风凭据，请到天气设置填写并测试。".into())}else if self.weather_source.config.location.is_none(){Some("请先选择和风城市，和风暂不支持自动定位。".into())}else{None});
            return;
        }
        self.weather.poll(&self.settings.weather_city,enabled);
        if self.weather.should_refresh(&self.settings.weather_city,self.t,chrono::Utc::now().timestamp(),enabled) {
            // 单元测试仍执行模块开关与刷新判定，仅跳过实际联网。
            if !cfg!(test) {self.weather.refresh(&self.settings.weather_city,self.t);}
        }
    }

    /// 加入已保存城市列表（去重，超出上限时不加）。返回是否在列表里。
    fn remember_weather_city(&mut self, position: weather::Position) -> bool {
        if self.settings.weather_saved.contains(&position) {return true;}
        if self.settings.weather_saved.len()>=weather::MAX_CITIES {return false;}
        self.settings.weather_saved.push(position);true
    }

    /// 切换当前城市；`None` 为自动定位。立即换上缓存并保存设置，是否联网由刷新调度决定。
    pub(crate) fn switch_weather_city(&mut self, position: Option<weather::Position>) {
        if !self.module_on(WEATHER_PAGE) {return;}
        if self.weather_source.displayed==crate::weather_source_ui::Provider::Qweather {
            if let Some(location)=self.weather_source.config.saved.iter().find(|l|Some(&l.position)==position.as_ref()).cloned(){self.qweather_select_location(location);}
            return;
        }
        let city=position.as_ref().map(|p|p.label.clone()).unwrap_or_default();
        if self.settings.weather_city==city && self.settings.weather_position==position {self.weather.picker_open=false;return;}
        self.settings.weather_city=city.clone();
        self.settings.weather_position=position.clone();
        self.settings.save();
        self.weather.switch(&city,position,chrono::Utc::now().timestamp());
    }

    /// 设置页「添加城市」：把搜索框里确认的地区加入列表，不改变当前城市。
    pub(crate) fn add_weather_city(&mut self) -> bool {
        if !self.module_on(WEATHER_PAGE) {return false;}
        let Some(position)=self.weather.picked_city().cloned() else {return false;};
        if self.settings.weather_saved.contains(&position) || !self.remember_weather_city(position) {return false;}
        // 只加入列表，不切换当前城市；想看它的天气时在列表或岛上下拉框里点它。
        self.settings.save();
        self.weather.city_input.clear();self.weather.selected_city=None;
        true
    }

    /// 删除已保存城市；删掉的正是当前城市时回到自动定位。
    pub(crate) fn remove_weather_city(&mut self, position: &weather::Position) {
        self.settings.weather_saved.retain(|p|p!=position);
        self.weather.forget_city(position);
        if self.settings.weather_position.as_ref()==Some(position) {self.switch_weather_city(None);}
        self.settings.save();
    }

    pub(crate) fn weather_city_picker(&mut self, ui: &mut egui::Ui) -> bool {
        use crate::extras::{settings_control_row,settings_hint};
        let popup=egui::Id::new("weather-city-options");
        let mut enter=false;
        let response=settings_control_row(ui,"城市名称",80.0,|ui|{
            ui.horizontal(|ui|{
                let input=ui.add_sized([ui.available_width()-38.0,32.0],egui::TextEdit::singleline(&mut self.weather.city_input)
                    .id_salt("weather-city-input").vertical_align(egui::Align::Center).margin(egui::vec2(10.0,7.0)).hint_text("输入城市名称搜索"));
                let arrow=ui.add_sized([30.0,32.0],egui::Button::new("").corner_radius(7.0)).on_hover_text("选择具体地区");
                let center=arrow.rect.center();ui.painter().add(egui::Shape::line(vec![center+egui::vec2(-4.0,-2.0),center+egui::vec2(0.0,2.0),center+egui::vec2(4.0,-2.0)],egui::Stroke::new(1.5,ui.visuals().text_color())));
                if input.changed() || input.clicked() {ui.memory_mut(|m|m.open_popup(popup));}
                if arrow.clicked() {ui.memory_mut(|m|m.toggle_popup(popup));}
                enter=input.lost_focus() && ui.input(|i|i.key_pressed(egui::Key::Enter));
                if enter && self.weather.draft_city().is_none() {ui.memory_mut(|m|m.open_popup(popup));}
                input.union(arrow)
            }).inner
        });
        self.weather.cities.update(&self.weather.city_input,self.t);
        if self.weather.cities.pending() && self.weather.city_input.trim().chars().count()>=2 {ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));}
        let mut selected=None;
        let saved=self.settings.weather_saved.clone();
        egui::popup::popup_below_widget(ui,popup,&response,egui::popup::PopupCloseBehavior::CloseOnClickOutside,|ui|{
            ui.spacing_mut().item_spacing.y=2.0;
            let weak=ui.visuals().weak_text_color();
            let note=|ui:&mut egui::Ui,text:&str|{ui.add_space(6.0);ui.label(egui::RichText::new(text).size(12.0).color(weak));ui.add_space(6.0);};
            if self.weather.city_input.trim().chars().count()<2 {note(ui,"输入至少两个字，搜索城市或地区");}
            else if let Some(error)=self.weather.cities.error.clone() {
                note(ui,&error);
                if ui.button("重新搜索").clicked(){self.weather.cities.retry();}
            }else if self.weather.cities.pending(){ui.add_space(6.0);ui.horizontal(|ui|{ui.spinner();ui.label(egui::RichText::new("正在搜索城市…").size(12.0).color(weak));});ui.add_space(6.0);}
            else if self.weather.cities.results.is_empty(){note(ui,"未找到城市，试试去掉「市」「县」或用英文名");}
            else {
                egui::ScrollArea::vertical().max_height(232.0).show(ui,|ui|{
                    for position in &self.weather.cities.results {
                        let chosen=self.weather.selected_city.as_ref()==Some(position);
                        if city_option(ui,position,chosen,saved.contains(position)).clicked(){selected=Some(position.clone());}
                    }
                });
            }
        });
        if let Some(position)=selected {
            self.weather.city_input=position.label.clone();self.weather.selected_city=Some(position);
            ui.memory_mut(|m|m.close_popup());
        }
        if let Some(position)=self.weather.picked_city(){settings_hint(ui,&format!("已选择：{} · {}",position.label,position.region));}
        else {settings_hint(ui,"输入中英文城市名，从下拉列表选择具体地区，再点「添加」。");}
        enter && self.weather.picked_city().is_some()
    }

    /// 城市切换入口：「📍 地名 ▾」。有天气时就是实况区的地点行，没有数据时画在页面中下方。
    /// 返回入口的命中区域（局部坐标 x, y, w, h）。
    fn weather_place_trigger(&self, c: &Card, snapshot: Option<&weather::Snapshot>) -> (f32, f32, f32, f32) {
        let open=c.ui.ctx().animate_bool_with_time(egui::Id::new(("weather-picker",c.live)),self.weather.picker_open,0.16);
        match snapshot {
            // 地点文字由 paint_forecast 绘制，这里只补 ▾ 并给出命中区域。
            Some(snapshot)=>{
                let w=place_width(c,&snapshot.place);
                chevron_down(c,110.0+w+7.0,112.5,open);
                (97.0,103.0,w+27.0,18.0)
            }
            None=>{
                let label=if self.weather_source.displayed==crate::weather_source_ui::Provider::Qweather {self.weather_source.config.location.as_ref().map(|l|l.position.label.clone()).unwrap_or_else(||"选择和风城市".into())}else if self.settings.weather_city.trim().is_empty() {"自动定位".to_string()} else {self.settings.weather_city.clone()};
                let label=c.fit(&label,10.5,W::Regular,140.0);
                let w=c.text_w(&label,10.5,W::Regular)+24.0;
                let x=209.5-w/2.0;
                location_glyph(c,x+5.0,146.0,theme::GRAY,Color32::BLACK);
                c.text(x+12.0,150.0,&label,10.5,W::Regular,theme::GRAY,H::Left);
                chevron_down(c,x+w-4.0,146.5,open);
                (x-6.0,137.0,w+12.0,19.0)
            }
        }
    }

    /// 岛内的城市下拉面板：自动定位 + 已保存城市 + 管理入口。画在页面内部，不用系统弹窗，
    /// 不会被岛窗口裁掉。每行带该城市缓存里的天气，切换前就能看到大概。
    fn paint_weather_picker(&mut self, c: &Card, open: f32) {
        enum Pick {Close,City(Option<weather::Position>),Manage}
        let mut pick=None;
        // 背景层：点面板外任何地方都收起。
        if c.live && self.weather.picker_open && c.hit(0.0,0.0,419.0,WEATHER_H,"weather-picker-backdrop").clicked() {pick=Some(Pick::Close);}
        let mut rows:Vec<(Option<weather::Position>,String,String)>=vec![(None,"自动定位".into(),
            self.weather.cached_for("",None).map(|s|s.place.clone()).unwrap_or_else(||"按网络出口".into()))];
        let qweather=self.weather_source.displayed==crate::weather_source_ui::Provider::Qweather;
        if qweather {
            rows.clear();for l in &self.weather_source.config.saved {let p=&l.position;rows.push((Some(p.clone()),p.label.clone(),p.region.clone()));}
        }else{for p in &self.settings.weather_saved {rows.push((Some(p.clone()),p.label.clone(),p.region.split(" · ").next().unwrap_or_default().to_string()));}}
        let row=27.0;
        let h=rows.len() as f32*row+row+10.0;
        // 从地点行下方展开；城市多时整体上移，保证不超出页面底部。
        let (x,w)=(92.0,200.0);
        let y=(124.0_f32).min(WEATHER_H-10.0-h).max(34.0);
        let p=c.faded(open).scaled(x+20.0,y,0.94+0.06*open);
        let p=if open<1.0 {p.ghost()} else {p};
        // 柔和投影 + 面板。
        p.fill(x-2.0,y+2.0,w+4.0,h+4.0,12.0,Color32::from_black_alpha(90));
        p.fill(x,y,w,h,11.0,Color32::from_rgb(30,30,34));
        p.stroke(x,y,w,h,11.0,0.8,Color32::from_white_alpha(22));
        let current=if qweather{self.weather_source.config.location.as_ref().map(|l|l.position.clone())}else{self.settings.weather_position.clone()};
        for (i,(position,label,detail)) in rows.iter().enumerate() {
            let ry=y+5.0+i as f32*row;
            let hit=p.hit(x+4.0,ry,w-8.0,row-1.0,("weather-picker-row",i)).on_hover_cursor(egui::CursorIcon::PointingHand);
            let selected=*position==current;
            let (hover,_)=p.hover_t(&hit);
            if selected || hover>0.0 {p.fill(x+4.0,ry,w-8.0,row-1.0,7.0,Color32::from_white_alpha(if selected{14}else{(hover*10.0) as u8}));}
            let cached=if qweather{self.weather.snapshot.as_ref().filter(|s|s.position==*position)}else{self.weather.cached_for(position.as_ref().map(|p|p.label.as_str()).unwrap_or(""),position.as_ref())};
            match (position,cached) {
                (_,Some(s)) => icon(&p,x+18.0,ry+13.0,14.0,weather::kind(s.code,s.day)),
                (None,None) => location_glyph(&p,x+18.0,ry+13.5,theme::GRAY,Color32::from_rgb(30,30,34)),
                (Some(_),None) => p.circle(x+18.0,ry+13.0,2.0,theme::DIM),
            }
            let lw=p.text(x+32.0,ry+17.5,&p.fit(label,11.5,W::Semibold,82.0),11.5,W::Semibold,if selected{theme::CYAN}else{theme::WHITE},H::Left);
            let right=x+w-14.0;
            let temp=cached.map(pill_temp_text);
            let tw=temp.as_ref().map(|t|paint_temp(&p,right,ry+17.5,t,11.0,W::Semibold,theme::GRAY,H::Right)).unwrap_or(0.0);
            let room=right-tw-8.0-(x+32.0+lw+6.0);
            if room>18.0 {p.text(x+32.0+lw+6.0,ry+17.0,&p.fit(detail,9.5,W::Regular,room),9.5,W::Regular,theme::DIM,H::Left);}
            if hit.clicked() {pick=Some(Pick::City(position.clone()));}
        }
        let my=y+5.0+rows.len() as f32*row;
        p.seg((x+12.0,my+0.5),(x+w-12.0,my+0.5),0.7,Color32::from_white_alpha(18));
        let manage=p.hit(x+4.0,my+2.0,w-8.0,row-2.0,"weather-picker-manage").on_hover_cursor(egui::CursorIcon::PointingHand);
        let (hover,_)=p.hover_t(&manage);
        if hover>0.0 {p.fill(x+4.0,my+2.0,w-8.0,row-2.0,7.0,Color32::from_white_alpha((hover*10.0) as u8));}
        let full=if qweather{self.weather_source.config.saved.len()}else{self.settings.weather_saved.len()}>=weather::MAX_CITIES;
        p.text(x+18.0,my+19.0,"＋",11.0,W::Semibold,theme::GRAY,H::Center);
        p.text(x+32.0,my+19.0,if full{"管理城市"}else{"添加或管理城市"},11.0,W::Regular,theme::GRAY,H::Left);
        if manage.clicked() {pick=Some(Pick::Manage);}
        if c.live && self.weather.picker_open && c.ui.input(|i|i.key_pressed(egui::Key::Escape)) {pick=Some(Pick::Close);}
        match pick {
            Some(Pick::Close) => self.weather.picker_open=false,
            Some(Pick::City(position)) => {self.switch_weather_city(position);self.weather.picker_open=false;}
            Some(Pick::Manage) => {self.weather.picker_open=false;self.settings_tab=12;self.open_settings();}
            None => {}
        }
    }

    /// 常驻天气开关开启、有快照、天气页可见且非极简时才占位。
    pub(crate) fn weather_pill(&self) -> Option<&crate::weather::Snapshot> {
        self.weather.snapshot.as_ref()
            .filter(|_| self.settings.show_weather && self.module_on(WEATHER_PAGE) && !self.effective_minimal())
    }

    /// 温度文字比基础槽宽时需要补的宽度（负数会被调用方忽略）。
    pub(crate) fn weather_pill_extra(&self, ctx: &egui::Context) -> Option<f32> {
        let snapshot = self.weather_pill()?;
        let text = pill_temp_text(snapshot);
        // 与绘制一致：按墨迹宽度计算（含手绘减号），不会额外撑出左侧空白。
        let size = theme::size::PILL_CLOCK;
        let ink_w = |t: &str| ctx.fonts(|fonts| fonts.layout_no_wrap(t.to_string(), theme::font(size, W::Bold), theme::WHITE).mesh_bounds.width());
        let ink = match text.strip_prefix('-') { Some(rest) => ink_w("-") + size * 0.07 + ink_w(rest), None => ink_w(&text) }.ceil();
        let chip = PILL_ICON + PILL_GAP + ink + 2.0;
        Some(chip - crate::bar_layout::WEATHER_WIDTH)
    }

    /// 常驻胶囊的天气徽标：小图标 + 温度；点击进入天气页。
    pub(crate) fn draw_pill_weather(&mut self, c: &Card, x: f32, layout: &crate::bar_layout::Layout) {
        let Some(snapshot) = self.weather_pill().cloned() else { return };
        self.paint_pill_weather(c, x, layout, &snapshot);
        let width = layout.widths[crate::bar_layout::WEATHER];
        let tip = format!("{} · {}\n更新于 {} · {}", snapshot.place, snapshot.description(), weather::clock(snapshot.at),self.weather_source.displayed.name());
        // 用 bar_tooltip：常驻胶囊上的提示不能用 on_hover_text，悬浮层会盖住指针、抢走点击。
        if crate::quota_ui::bar_tooltip(c.hit(x - width / 2.0, 1.0, width, 28.0, "weather-pill"), tip).clicked() {
            self.weather.solar_open=false;
            self.show_page(WEATHER_PAGE);
        }
    }

    pub(crate) fn draw_side_weather(&mut self,c:&Card,cx:f32,cy:f32) {
        let Some(snapshot)=self.weather_pill() else{return;};
        self.paint_side_weather(c,cx,cy,snapshot);
        let tip=format!("{} · {}\n更新于 {} · {}",snapshot.place,snapshot.description(),weather::clock(snapshot.at),self.weather_source.displayed.name());
        let response=crate::quota_ui::bar_tooltip(c.hit(cx-17.0,cy-19.0,34.0,38.0,"weather-side"),tip);
        if response.clicked(){self.weather.solar_open=false;self.show_page(WEATHER_PAGE);}
    }

    pub(crate) fn paint_side_weather(&self,c:&Card,cx:f32,cy:f32,snapshot:&weather::Snapshot) {
        icon(c,cx,cy-8.0,PILL_ICON,weather::kind(snapshot.code,snapshot.day));
        let stale=self.weather_is_stale(snapshot);
        // 按墨迹水平居中，「°」与减号不会把数字挤偏。
        paint_temp(c,cx,cy+14.0,&pill_temp_text(snapshot),10.5,W::Bold,if stale{theme::GRAY}else{theme::WHITE},H::Center);
    }

    /// 只画不响应命中，设置页的常驻预览复用。
    pub(crate) fn paint_pill_weather(&self, c: &Card, x: f32, layout: &crate::bar_layout::Layout, snapshot: &crate::weather::Snapshot) {
        // 吉祥物游到徽标附近时先淡出，避免遮挡。
        let alpha = layout.pet_clearance(crate::bar_layout::WEATHER);
        let faded = c.faded(alpha);
        let faded = if alpha < 0.8 { faded.ghost() } else { faded };
        let c = &faded;
        let width = layout.widths[crate::bar_layout::WEATHER];
        let left = x - width / 2.0;
        // 按墨迹对齐：图标中心对准数字中线，温度墨迹左缘紧跟图标右缘留固定间距。
        let text = pill_temp_text(snapshot);
        let base = 21.0;
        let (_, mid) = temp_ink(c, &text, theme::size::PILL_CLOCK, W::Bold);
        icon(c, left + PILL_ICON / 2.0, base + mid, PILL_ICON, weather::kind(snapshot.code, snapshot.day));
        // 持续刷新失败且数据超过两个刷新周期才降为灰色。
        let stale = self.weather_is_stale(snapshot);
        paint_temp(c, left + PILL_ICON + PILL_GAP, base, &text, theme::size::PILL_CLOCK, W::Bold, if stale { theme::GRAY } else { theme::WHITE }, H::Left);
    }

    pub(crate) fn page_weather(&mut self, c: &Card, env: &Env, cmds: &mut Vec<Cmd>) {
        self.page_weather_body(c,env,cmds);
        if self.weather.solar_open {self.weather.picker_open=false;return;}
        // 地点行「📍 地名 ▾」是城市切换入口：在内容点击区之后注册，优先于「轻点切换日照」；
        // 面板最后注册，命中优先级最高。
        let snapshot=self.weather.snapshot.clone();
        let (x,y,w,h)=self.weather_place_trigger(c,snapshot.as_ref());
        let place=c.hit(x,y,w,h,"weather-place").on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("切换城市");
        let (hover,_)=c.hover_t(&place);
        if hover>0.0 {c.fill(x,y,w,h,h/2.0,Color32::from_white_alpha((hover*14.0) as u8));}
        if place.clicked() {self.weather.picker_open=!self.weather.picker_open;}
        let ctx=c.ui.ctx().clone();
        let open=ctx.animate_bool_with_time(egui::Id::new(("weather-picker",c.live)),self.weather.picker_open,0.16);
        if open>0.0 {self.paint_weather_picker(c,open);}
        if c.live && open>0.0 && open<1.0 {ctx.request_repaint();}
    }

    fn page_weather_body(&mut self, c: &Card, env: &Env, cmds: &mut Vec<Cmd>) {
        let loading = self.weather.loading;
        let ready=self.weather_can_refresh();
        c.text(326.0, 23.0, if loading { "更新中" } else if ready { "刷新" } else { "稍后刷新" }, 10.5, W::Regular, if ready { theme::GRAY } else { theme::DIM }, H::Center);
        if c.hit(304.0, 8.0, 44.0, 21.0, "weather-refresh").on_hover_text("重新获取天气；自动模式会重新定位。请求间隔至少 30 秒，失败后至少 2 分钟").clicked() && ready {
            self.weather_refresh();
        }
        let footer=WEATHER_H-4.0;
        let source=self.weather_source.displayed;
        let source_label=if source==crate::weather_source_ui::Provider::Qweather{"QWeather"}else{source.name()};
        let source_w=c.text(404.0,footer,source_label,10.0,W::Regular,theme::GRAY,H::Right);
        if source==crate::weather_source_ui::Provider::Qweather {
            let forecast_at=self.weather_source.runtime.stamps().1;
            let label=if forecast_at>0{format!("预报 {}",weather::clock(forecast_at))}else{"预报未取得".into()};
            c.text(120.0,footer,&label,8.5,W::Regular,if forecast_at==0||chrono::Utc::now().timestamp()-forecast_at>crate::qweather::DAILY_INTERVAL{WARN}else{theme::DIM},H::Left);
            c.hit(116.0,footer-11.0,100.0,15.0,"qweather-forecast-stamp").on_hover_text(format!("预报上次获取：{}",crate::weather_source_ui::updated_at(forecast_at)));
            let sources=self.weather.snapshot.as_ref().map(|s|s.attributions.join("\n")).unwrap_or_default();
            c.text(24.0,footer,"数据归因 ⓘ",9.5,W::Regular,theme::GRAY,H::Left);
            if c.hit(20.0,footer-12.0,86.0,16.0,"qweather-attributions").on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("天气服务由和风天气驱动\n{sources}\n降水概率：日夜较高值")).clicked(){
                let link=self.weather.snapshot.as_ref().and_then(|s|s.attributions.iter().find(|s|url::Url::parse(s).is_ok_and(|u|u.scheme()=="https"))).map(String::as_str).unwrap_or("https://dev.qweather.com/docs/terms/attribution/");
                cmds.push(Cmd::OpenUrl(link.into()));
            }
        }
        if c.hit(404.0-source_w,footer-11.0,source_w,15.0,"weather-source").on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("天气数据来自{}，点击打开官网",source_label)).clicked() {
            cmds.push(Cmd::OpenUrl(source.url().into()));
        }

        let Some(snapshot) = self.weather.snapshot.clone() else {
            let (title, detail) = if loading {
                ("正在获取当地天气…", "首次定位需要几秒钟".to_string())
            } else if let Some(error) = &self.weather.error {
                ("天气暂时取不到", error.clone())
            } else {
                ("还没有天气数据", "点右上「刷新」重试".to_string())
            };
            c.text(209.5, 104.0, title, 13.0, W::Semibold, theme::WHITE, H::Center);
            c.text(209.5, 124.0, &c.fit(&detail, 10.5, W::Regular, 320.0), 10.5, W::Regular, theme::GRAY, H::Center);
            return;
        };

        let age = chrono::Utc::now().timestamp() - snapshot.at;
        let offline = self.weather.error.is_some();
        let stamp = weather::clock(snapshot.at);
        let stale = offline || age > 3600;
        let stamp_x=394.0-source_w;
        let stamp_label=if source==crate::weather_source_ui::Provider::Qweather{format!("实况 {stamp}")}else{stamp.clone()};
        c.text(stamp_x, footer, &stamp_label, 8.5, W::Regular, if stale { WARN } else { theme::DIM }, H::Right);
        c.hit(stamp_x-32.0, footer-9.0, 32.0, 13.0, "weather-stamp").on_hover_text(format!(
            "上次更新 {}{}",if source==crate::weather_source_ui::Provider::Qweather{crate::weather_source_ui::updated_at(snapshot.at)}else{stamp.clone()},
            if offline { "，当前显示的是缓存" } else { "" }
        ));

        let response=c.hit(36.0,34.0,347.0,WEATHER_H-48.0,"weather-view").on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked(){
            self.weather.solar_open=!self.weather.solar_open;
            self.weather.solar_at=env.t;
            let at=response.interact_pointer_pos().map(|p|c.to_local(p)).unwrap_or(egui::pos2(209.5,120.0));
            self.weather.tap=Some((at.x,at.y,env.t));
        }
        let ctx=c.ui.ctx().clone();
        // 进页面后先动几秒再静止；指针停在页面上时继续动。离开再回来重新计时。
        let seen_id=egui::Id::new(("weather-page-seen",c.live));
        let (mut seen,last)=ctx.data(|d|d.get_temp::<(f64,f64)>(seen_id)).unwrap_or((env.t,env.t));
        if env.t-last>0.5 {seen=env.t;}
        ctx.data_mut(|d|d.insert_temp(seen_id,(seen,env.t)));
        let ambient=c.live && (response.hovered() || env.t-seen<AMBIENT_SECS);
        let t=ambient_clock(&ctx,egui::Id::new(("weather-ambient",c.live)),ambient,env.t);
        // 日照入口卡的悬停与按下：只在指针落在卡片范围内时生效。
        let on_entry=response.hover_pos().map(|p|c.to_local(p)).is_some_and(|p|(300.0..384.0).contains(&p.x)&&(77.0..111.0).contains(&p.y)) && !self.weather.solar_open;
        let lift=ctx.animate_bool_with_time(egui::Id::new(("weather-entry-lift",c.live)),on_entry,0.25);
        let press=ctx.animate_bool_with_time(egui::Id::new(("weather-entry-press",c.live)),on_entry&&response.is_pointer_button_down_on(),0.08);
        let mix=ctx.animate_bool_with_time(egui::Id::new(("weather-view-blend",c.live)),self.weather.solar_open,0.24);
        let slide=((env.t-self.weather.solar_at) as f32/SUN_SLIDE).clamp(0.0,1.0);
        // 两个视图交叉淡入，同时轻微缩放，切换有景深感。
        if mix<1.0 {
            let forecast=c.faded((1.0-mix).powi(2)).scaled(209.5,128.0,1.0-0.04*mix).ghost();
            paint_forecast(&forecast,&snapshot,t,lift,press);
        }
        if mix>0.0 {
            let solar=c.faded(mix.powi(2)).scaled(209.5,128.0,0.96+0.04*mix).ghost();
            paint_sun_times(&solar,&snapshot,weather_now(env),t,if self.weather.solar_open{slide}else{1.0});
        }
        if let Some((x,y,at))=self.weather.tap {
            let k=(env.t-at) as f32/TAP_FX;
            if k<1.0 {paint_tap(c,x,y,k.max(0.0),if self.weather.solar_open{SUN}else{theme::CYAN});}
            else {self.weather.tap=None;}
        }
        if c.live {
            // 只有过渡动画进行中才逐帧重绘；常态由 ambient_clock 决定，静止时不重绘。
            if self.weather.tap.is_some() || (self.weather.solar_open && slide<1.0) || (mix>0.0&&mix<1.0) || (lift>0.0&&lift<1.0) || (press>0.0&&press<1.0) {ctx.request_repaint();}
        }
    }

    /// 设置页顶部的实况卡：按天气换渐变底色，右侧放动态大图标，底部一行指标与刷新按钮。
    fn weather_settings_hero(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let width = ui.available_width();
        let height = 150.0;
        let (rect, hero) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let c = Card::new(ui, rect.min, rect);
        // 设置页常开时不持续重绘：只在指针停在卡片上时让图标动起来。
        let t = ambient_clock(&ctx, hero.id.with("ambient"), hero.hovered(), ctx.input(|i| i.time));
        let snapshot = self.weather.snapshot.clone();
        let kind = snapshot.as_ref().map(|s| weather::kind(s.code, s.day));
        let (top, bottom) = kind.map(hero_colors).unwrap_or((Color32::from_rgb(86, 94, 112), Color32::from_rgb(52, 58, 72)));
        c.vgrad(0.0, 0.0, width, height, 14.0, top, bottom);
        // 背景装饰：右上一团柔光，半径不越过圆角，避免溢出卡片轮廓。
        c.glow(width - 80.0, 64.0, 62.0, Color32::from_white_alpha(30));
        let soft = Color32::from_white_alpha(205);
        let faint = Color32::from_white_alpha(160);
        let Some(snapshot) = snapshot else {
            icon_at(&c, width - 74.0, 68.0, 70.0, Kind::Cloud, Some(t));
            c.text(22.0, 40.0, "天气", 15.0, W::Semibold, theme::WHITE, H::Left);
            c.text(22.0, 84.0, if self.weather.loading { "正在获取天气…" } else { "尚无天气数据" }, 22.0, W::Bold, theme::WHITE, H::Left);
            c.text(22.0, 108.0, "选择定位方式并应用位置后，这里会显示当前天气。", 11.5, W::Regular, soft, H::Left);
            return;
        };
        let kind = kind.unwrap_or(Kind::Cloud);
        c.glow(width - 74.0, 66.0, 64.0, paint::alpha(tint(kind), 0.30));
        icon_at(&c, width - 74.0, 66.0, 76.0, kind, Some(t));

        location_glyph(&c, 26.0, 30.0, theme::WHITE, top);
        let place_w = c.text(35.0, 35.0, &c.fit(&snapshot.place, 15.0, W::Semibold, width - 200.0), 15.0, W::Semibold, theme::WHITE, H::Left);
        if let Some(position) = snapshot.position.as_ref().filter(|p| !p.region.is_empty()) {
            c.text(35.0 + place_w + 8.0, 35.0, &c.fit(&position.region, 11.5, W::Regular, (width - 220.0 - place_w).max(40.0)), 11.5, W::Regular, faint, H::Left);
        }
        let temp_w = paint_temp(&c, 22.0, 88.0, &pill_temp_text(&snapshot), 46.0, W::Bold, theme::WHITE, H::Left) + 4.0;
        c.text(28.0 + temp_w, 70.0, snapshot.description(), 14.0, W::Semibold, theme::WHITE, H::Left);
        if let (Some(high), Some(low)) = (snapshot.high, snapshot.low) {
            c.text(28.0 + temp_w, 89.0, &format!("最高 {high:.0}°  最低 {low:.0}°"), 11.5, W::Regular, soft, H::Left);
        }

        // 底部一行：体感 / 湿度 / 风速 / 日出日落，以半透明分隔条隔开。
        let mut x = 24.0;
        let stats = [
            snapshot.feels.map(|v| format!("体感 {v:.0}°")),
            snapshot.humidity.map(|v| format!("湿度 {v}%")),
            snapshot.wind.map(|v| format!("风速 {v:.0} km/h")),
            snapshot.sunrise.as_ref().zip(snapshot.sunset.as_ref()).map(|(a, b)| format!("日出 {a} · 日落 {b}")),
        ];
        let limit = width - 150.0;
        c.fill(12.0, 108.0, width - 24.0, 32.0, 10.0, Color32::from_black_alpha(46));
        for text in stats.into_iter().flatten() {
            let w = c.text_w(&text, 11.5, W::Regular);
            if x + w > limit { break; }
            if x > 24.0 { c.fill(x - 9.0, 117.0, 1.0, 14.0, 0.5, Color32::from_white_alpha(50)); }
            c.text(x, 128.5, &text, 11.5, W::Regular, soft, H::Left);
            x += w + 18.0;
        }
        // 右下：更新时间 + 圆形刷新按钮（加载中转圈）。
        let ready = self.weather_can_refresh();
        let (bx, by) = (width - 32.0, 124.0);
        let button = ui.interact(c.rect(bx - 13.0, by - 13.0, 26.0, 26.0), ui.id().with("weather-hero-refresh"), if ready { egui::Sense::click() } else { egui::Sense::hover() })
            .on_hover_text(if ready { "刷新天气；自动模式会重新定位" } else { "刚刚刷新过，请稍后再试" });
        let hover = ctx.animate_bool_with_time(button.id, ready && button.hovered(), 0.12);
        c.circle(bx, by, 13.0, Color32::from_white_alpha((34.0 + 40.0 * hover) as u8));
        let spin = if self.weather.loading { ctx.request_repaint(); ctx.input(|i| i.time) as f32 * 6.0 } else { 0.0 };
        paint::reset(&c.scaled(bx, by, 0.95), bx, by, spin, if ready || self.weather.loading { theme::WHITE } else { faint });
        c.text(bx - 20.0, 128.5, &format!("更新于 {}", weather::clock(snapshot.at)), 11.0, W::Regular, faint, H::Right);
        if button.clicked() { self.weather_refresh(); }
    }

    pub(crate) fn weather_settings(&mut self, ui: &mut egui::Ui) {
        use crate::extras::{settings_card, settings_hint, settings_label};
        use crate::settings_theme::color;
        let ctx=ui.ctx().clone();
        let accent=color(&ctx,Color32::from_rgb(96,164,255),Color32::from_rgb(34,108,220));
        ui.add_space(4.0);
        self.weather_source_settings(ui);
        ui.add_space(12.0);
        if self.weather_source.selected==crate::weather_source_ui::Provider::Qweather {
            if self.weather_source.displayed==self.weather_source.selected {self.weather_settings_hero(ui);ui.add_space(8.0);}
            self.qweather_city_settings(ui);return;
        }
        if self.weather_source.displayed!=self.weather_source.selected {return;}
        self.weather_settings_hero(ui);
        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui|{
            ui.label(egui::RichText::new("天气数据来自").size(11.5).color(ui.visuals().weak_text_color()));
            let source=self.weather_source.displayed;ui.hyperlink_to(source.name(),source.url());
        });
        if let Some(error)=self.weather.error.clone() {
            ui.add_space(6.0);
            egui::Frame::new().fill(color(&ctx,Color32::from_rgb(64,44,24),Color32::from_rgb(255,240,224))).corner_radius(10.0)
                .inner_margin(egui::Margin::symmetric(14,9)).show(ui,|ui|{
                    ui.set_min_width(ui.available_width());
                    ui.add(egui::Label::new(egui::RichText::new(format!("⚠  {error}")).size(12.0).color(color(&ctx,WARN,Color32::from_rgb(155,84,24)))).wrap());
                });
        }

        settings_label(ui,"城市");
        settings_card(ui,|ui|{
            ui.spacing_mut().item_spacing.y=4.0;
            let current=self.settings.weather_position.clone();
            let mut rows:Vec<Option<weather::Position>>=vec![None];
            rows.extend(self.settings.weather_saved.iter().cloned().map(Some));
            let (mut chosen,mut removed)=(None,None);
            for position in rows {
                let city=position.as_ref().map(|p|p.label.as_str()).unwrap_or("");
                let cached=self.weather.cached_for(city,position.as_ref()).cloned();
                let (title,detail)=match &position {
                    None=>("自动定位".to_string(),cached.as_ref().map(|s|format!("按网络出口识别 · 当前为 {}",s.place)).unwrap_or_else(||"按网络出口识别所在城市；使用代理时可能是代理所在地".into())),
                    Some(p)=>(p.label.clone(),p.region.clone()),
                };
                let (click,remove)=city_row(ui,position==current,position.is_some(),&title,&detail,cached.as_ref(),accent);
                if click {chosen=Some(position.clone());}
                if remove {removed=position.clone();}
            }
            if let Some(position)=removed {self.remove_weather_city(&position);}
            else if let Some(position)=chosen {self.switch_weather_city(position);}
            ui.add_space(4.0);
            settings_hint(ui,&format!("最多保存 {} 个城市。点上方列表或岛上天气页的地点就能切换；只有当前城市会在后台定时更新。",weather::MAX_CITIES));
        });

        settings_label(ui,"添加城市");
        settings_card(ui,|ui|{
            let full=self.settings.weather_saved.len()>=weather::MAX_CITIES;
            if full {
                settings_hint(ui,&format!("已保存 {} 个城市，删除一个后再添加。",weather::MAX_CITIES));
                return;
            }
            let mut add=self.weather_city_picker(ui);
            let picked=self.weather.picked_city().cloned();
            let exists=picked.as_ref().is_some_and(|p|self.settings.weather_saved.contains(p));
            let ready=picked.is_some() && !exists;
            ui.add_space(2.0);
            ui.horizontal(|ui|{
                let text=egui::RichText::new("添加").size(13.0)
                    .color(if ready{Color32::WHITE}else{color(&ctx,Color32::from_gray(150),Color32::from_gray(120))});
                let button=egui::Button::new(text).fill(if ready{accent}else{color(&ctx,Color32::from_rgb(52,52,56),Color32::from_rgb(232,235,240))})
                    .stroke(egui::Stroke::NONE).corner_radius(8.0);
                add|=ui.add_enabled_ui(ready,|ui|ui.add_sized([88.0,32.0],button)).inner.clicked();
                let status=if exists {"这个城市已经在列表里"} else if picked.is_none() {"先从下拉列表确认具体地区"} else {""};
                if !status.is_empty(){ui.label(egui::RichText::new(status).size(11.5).color(ui.visuals().weak_text_color()));}
            });
            if add && ready {self.add_weather_city();}
        });

        settings_label(ui,"更新与数据");
        settings_card(ui,|ui|{
            info_row(ui,"自动更新",|ui|{ui.label(egui::RichText::new(if self.weather_source.displayed==crate::weather_source_ui::Provider::Qweather{"实况约 60 分钟 · 预报约 6 小时"}else{"每 15 分钟"}).size(12.5).color(ui.visuals().weak_text_color()));});
            ui.separator();
            info_row(ui,"离线时",|ui|{ui.label(egui::RichText::new("保留上次天气，温度转灰提示").size(12.5).color(ui.visuals().weak_text_color()));});
            ui.separator();
            info_row(ui,"数据来源",|ui|{
                let source=self.weather_source.displayed;
                if source==crate::weather_source_ui::Provider::OpenMeteo{ui.hyperlink_to("CC BY 4.0","https://creativecommons.org/licenses/by/4.0/");}
                else{ui.hyperlink_to("来源说明","https://dev.qweather.com/docs/terms/attribution/");}
                ui.label(egui::RichText::new("·").color(ui.visuals().weak_text_color()));
                ui.hyperlink_to(source.name(),source.url());
            });
        });
    }
}

/// 设置行：左标题，右侧内容靠右。
fn info_row(ui: &mut egui::Ui, title: &str, right: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        ui.label(egui::RichText::new(title).size(13.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
}

/// 城市列表的一行：单选圆点、缓存天气图标、名称与地区、缓存温度；可删除的行右侧带 ×。
/// 返回 (点击整行, 点击删除)。
pub(crate) fn city_row(ui: &mut egui::Ui, selected: bool, removable: bool, title: &str, detail: &str, cached: Option<&weather::Snapshot>, accent: Color32) -> (bool, bool) {
    use crate::settings_theme::color;
    let ctx = ui.ctx().clone();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 48.0), egui::Sense::click());
    let remove_rect = egui::Rect::from_center_size(egui::pos2(rect.right() - 18.0, rect.center().y), egui::vec2(26.0, 26.0));
    let remove = if removable {
        Some(ui.interact(remove_rect, response.id.with("remove"), egui::Sense::click()).on_hover_text("删除这个城市"))
    } else { None };
    let row_hover = response.hovered() && !remove.as_ref().is_some_and(|r| r.hovered());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover = ctx.animate_bool_with_time(response.id, row_hover, 0.12);
    let pick = ctx.animate_bool_with_time(response.id.with("selected"), selected, 0.18);
    let base = color(&ctx, Color32::from_rgb(35, 35, 38), Color32::WHITE);
    let fill = paint::mix(paint::mix(base, color(&ctx, Color32::from_rgb(46, 46, 52), Color32::from_rgb(242, 244, 248)), hover), paint::mix(base, accent, 0.14), pick);
    ui.painter().rect(rect, 10.0, fill, egui::Stroke::new(1.0, paint::mix(Color32::TRANSPARENT, accent, pick)), egui::StrokeKind::Inside);
    let text = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    // 单选圆点。
    let dot = egui::pos2(rect.left() + 18.0, rect.center().y);
    ui.painter().circle_stroke(dot, 7.0, egui::Stroke::new(1.4, paint::mix(weak, accent, pick)));
    if pick > 0.0 { ui.painter().circle_filled(dot, 3.8 * pick, accent); }
    // 缓存天气图标：浅色主题下沿用设置页的浅色配色。
    let canvas = Card::new(ui, rect.min, rect);
    let first = canvas.painter.add(egui::Shape::Noop).0;
    match cached {
        Some(s) => icon(&canvas, 48.0, 24.0, 22.0, weather::kind(s.code, s.day)),
        None if !removable => location_glyph(&canvas, 48.0, 25.0, weak, fill),
        None => canvas.circle(48.0, 24.0, 2.5, weak),
    }
    let end = canvas.painter.add(egui::Shape::Noop).0;
    if !ui.visuals().dark_mode { crate::skin_paint::recolor_layer(&ctx, canvas.painter.layer_id(), &[(first, end)], &crate::skin_paint::GLASS_LIGHT); }
    let right = rect.right() - if removable { 40.0 } else { 16.0 };
    let painter = ui.painter();
    let temp_w = cached.map(|s| {
        let g = painter.layout_no_wrap(pill_temp_text(s), egui::FontId::proportional(15.0), text);
        let w = g.size().x;
        painter.galley(egui::pos2(right - w, rect.center().y - g.size().y / 2.0), g, text);
        w
    }).unwrap_or(0.0);
    let max = (right - temp_w - 12.0 - (rect.left() + 68.0)).max(40.0);
    let clip = egui::Rect::from_min_max(egui::pos2(rect.left() + 68.0, rect.top()), egui::pos2(rect.left() + 68.0 + max, rect.bottom()));
    let text_painter = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
    text_painter.text(rect.min + egui::vec2(68.0, 17.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(13.5), if selected { accent } else { text });
    text_painter.text(rect.min + egui::vec2(68.0, 34.0), egui::Align2::LEFT_CENTER, detail, egui::FontId::proportional(11.0), weak);
    let mut removed = false;
    if let Some(remove) = remove {
        let h = ctx.animate_bool_with_time(remove.id, remove.hovered(), 0.12);
        let danger = color(&ctx, Color32::from_rgb(255, 110, 110), Color32::from_rgb(205, 55, 55));
        if h > 0.0 { painter.circle_filled(remove_rect.center(), 12.0, paint::alpha(danger, 0.18 * h)); }
        let ink = paint::mix(weak, danger, h);
        let c = remove_rect.center();
        for (a, b) in [(egui::vec2(-4.0, -4.0), egui::vec2(4.0, 4.0)), (egui::vec2(-4.0, 4.0), egui::vec2(4.0, -4.0))] { painter.line_segment([c + a, c + b], egui::Stroke::new(1.5, ink)); }
        removed = remove.clicked();
    }
    (response.clicked() && !removed, removed)
}

/// 城市搜索下拉的一项：定位标记 + 名称（粗）+ 行政区（灰），左对齐；
/// 已选中的右侧打勾，已在列表里的标「已添加」。
pub(crate) fn city_option(ui: &mut egui::Ui, position: &weather::Position, chosen: bool, saved: bool) -> egui::Response {
    use crate::settings_theme::color;
    let ctx = ui.ctx().clone();
    let width = ui.available_width().max(220.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 44.0), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover = ctx.animate_bool_with_time(response.id, response.hovered(), 0.1);
    let accent = color(&ctx, Color32::from_rgb(96, 164, 255), Color32::from_rgb(34, 108, 220));
    let fill = if chosen { paint::alpha(accent, 0.16) } else { color(&ctx, Color32::from_white_alpha((hover * 14.0) as u8), Color32::from_black_alpha((hover * 10.0) as u8)) };
    let painter = ui.painter();
    painter.rect_filled(rect.shrink2(egui::vec2(2.0, 1.0)), 8.0, fill);
    let text = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let canvas = Card::new(ui, rect.min, rect);
    location_glyph(&canvas, 18.0, 22.5, if chosen { accent } else { weak }, color(&ctx, Color32::from_rgb(40, 40, 44), Color32::WHITE));
    let right = rect.right() - 12.0;
    let tag = if chosen { Some(("✓", accent)) } else if saved { Some(("已添加", weak)) } else { None };
    let tag_w = tag.map(|(t, c)| painter.text(egui::pos2(right, rect.center().y), egui::Align2::RIGHT_CENTER, t, egui::FontId::proportional(12.0), c).width()).unwrap_or(0.0);
    let clip = egui::Rect::from_min_max(rect.min + egui::vec2(32.0, 0.0), egui::pos2(right - tag_w - 8.0, rect.bottom()));
    let text_painter = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
    text_painter.text(rect.min + egui::vec2(32.0, 15.0), egui::Align2::LEFT_CENTER, &position.label, egui::FontId::proportional(13.5), if chosen { accent } else { text });
    text_painter.text(rect.min + egui::vec2(32.0, 31.0), egui::Align2::LEFT_CENTER, &position.region, egui::FontId::proportional(11.0), weak);
    response
}
