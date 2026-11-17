//! 面板内绘制：局部坐标画布 `Card` + 全部矢量图标。
//!
//! `Card` 把「面板局部逻辑点」映射到屏幕，并自带裁剪、整体透明度与可交互开关：
//! 切页时旧页用 `live = false` 的画布绘制，只画不响应点击，也不会和新页抢 id。

use crate::theme::{self, font, W};
use eframe::egui::{self, Color32, FontId, Id, Pos2, Rect, Response, Sense, Stroke, Vec2};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum H {
    Left,
    Center,
    Right,
}

#[derive(Clone)]
pub struct Card<'a> {
    pub ui: &'a egui::Ui,
    pub painter: egui::Painter,
    pub origin: Pos2,
    pub s: f32,
    pub live: bool,
}

impl<'a> Card<'a> {
    pub fn new(ui: &'a egui::Ui, origin: Pos2, clip: Rect) -> Self {
        let painter = ui.painter().with_clip_rect(clip);
        Self { ui, painter, origin, s: 1.0, live: true }
    }

    pub fn pos(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.s
    }
    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.pos(x, y), Vec2::new(w, h) * self.s)
    }
    pub fn to_local(&self, p: Pos2) -> Pos2 {
        let v = (p - self.origin) / self.s;
        Pos2::new(v.x, v.y)
    }

    // ---------- 派生画布 ----------
    pub fn faded(&self, alpha: f32) -> Self {
        let mut c = self.clone();
        c.painter.multiply_opacity(alpha.clamp(0.0, 1.0));
        c
    }
    pub fn shifted(&self, dx: f32, dy: f32) -> Self {
        let mut c = self.clone();
        c.origin += Vec2::new(dx, dy) * self.s;
        c
    }
    /// 以局部点 (cx, cy) 为中心整体缩放（按钮按下的回弹）。
    pub fn scaled(&self, cx: f32, cy: f32, k: f32) -> Self {
        let mut c = self.clone();
        let center = self.pos(cx, cy);
        c.origin = center + (self.origin - center) * k;
        c.s = self.s * k;
        c
    }
    pub fn ghost(&self) -> Self {
        let mut c = self.clone();
        c.live = false;
        c
    }
    pub fn clipped(&self, x: f32, y: f32, w: f32, h: f32) -> Self {
        let mut c = self.clone();
        let r = self.rect(x, y, w, h).intersect(self.painter.clip_rect());
        c.painter.set_clip_rect(r);
        c
    }

    // ---------- 文字（y 一律是基线，跨字体对齐稳定） ----------
    pub fn font(&self, size: f32, w: W) -> FontId {
        font(size * self.s, w)
    }
    pub fn galley(&self, t: &str, size: f32, w: W, color: Color32) -> std::sync::Arc<egui::Galley> {
        self.painter.layout_no_wrap(t.to_owned(), self.font(size, w), color)
    }
    pub fn text_w(&self, t: &str, size: f32, w: W) -> f32 {
        self.galley(t, size, w, Color32::WHITE).size().x / self.s
    }
    /// 返回局部坐标下的文字宽度。
    pub fn text(&self, x: f32, base: f32, t: &str, size: f32, w: W, color: Color32, h: H) -> f32 {
        let g = self.galley(t, size, w, color);
        let gw = g.size().x;
        let asc = g.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.pos.y).unwrap_or(size * self.s);
        let mut p = self.pos(x, base) - Vec2::new(0.0, asc);
        match h {
            H::Left => {}
            H::Center => p.x -= gw / 2.0,
            H::Right => p.x -= gw,
        }
        self.painter.galley(p, g, color);
        gw / self.s
    }
    /// 以字形的实际可见范围对齐垂直中心，保持中英文状态文字居中。
    pub fn centered_text(&self, x:f32, cy:f32, t:&str, size:f32, w:W, color:Color32, h:H) {
        if t.is_empty(){return;}
        let g=self.galley(t,size,w,color);
        let dx=match h {H::Left=>0.0,H::Center=>g.size().x/2.0,H::Right=>g.size().x};
        let pos=self.pos(x,cy)-Vec2::new(dx,g.mesh_bounds.center().y);
        self.painter.galley(pos,g,color);
    }
    /// 超宽时末尾加省略号。
    pub fn fit(&self, t: &str, size: f32, w: W, max_w: f32) -> String {
        if self.text_w(t, size, w) <= max_w {
            return t.to_owned();
        }
        let chars: Vec<char> = t.chars().collect();
        let (mut low, mut high) = (0, chars.len().saturating_sub(1));
        while low < high {
            let n = low + (high - low + 1) / 2;
            let s: String = chars[..n].iter().collect::<String>() + "…";
            if self.text_w(&s, size, w) <= max_w {
                low = n;
            } else {
                high = n - 1;
            }
        }
        chars[..low].iter().collect::<String>() + "…"
    }

    /// 按宽度折行，最多 `max_lines` 行，还有剩余内容时最后一行加省略号。
    pub fn wrap(&self, t: &str, size: f32, w: W, max_w: f32, max_lines: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut used = 0usize;
        let mut total = 0usize;
        for ch in t.chars() {
            total += 1;
            if ch == '\n' {
                used = total;
                out.push(std::mem::take(&mut cur));
            } else {
                cur.push(ch);
                if self.text_w(&cur, size, w) > max_w && cur.chars().count() > 1 {
                    cur.pop();
                    used = total - 1;
                    out.push(std::mem::take(&mut cur));
                    cur.push(ch);
                }
            }
            if out.len() == max_lines {
                break;
            }
        }
        if out.len() < max_lines && !cur.is_empty() {
            used = total;
            out.push(cur);
        }
        if out.len() == max_lines && used < t.chars().count() {
            if let Some(last) = out.last_mut() {
                *last = self.fit(last, size, w, max_w);
            }
        }
        out
    }

    // ---------- 图形 ----------
    pub fn fill(&self, x: f32, y: f32, w: f32, h: f32, r: f32, color: Color32) {
        self.painter.rect_filled(self.rect(x, y, w, h), r * self.s, color);
    }
    pub fn stroke(&self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, color: Color32) {
        self.painter.rect_stroke(
            self.rect(x, y, w, h),
            r * self.s,
            Stroke::new(width * self.s, color),
            egui::StrokeKind::Middle,
        );
    }
    pub fn circle(&self, x: f32, y: f32, r: f32, color: Color32) {
        self.painter.circle_filled(self.pos(x, y), r * self.s, color);
    }
    pub fn ring(&self, x: f32, y: f32, r: f32, width: f32, color: Color32) {
        self.painter.circle_stroke(self.pos(x, y), r * self.s, Stroke::new(width * self.s, color));
    }
    pub fn seg(&self, a: (f32, f32), b: (f32, f32), width: f32, color: Color32) {
        self.painter.line_segment([self.pos(a.0, a.1), self.pos(b.0, b.1)], Stroke::new(width * self.s, color));
    }
    pub fn path(&self, pts: &[(f32, f32)], width: f32, color: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pos(x, y)).collect();
        self.painter.add(egui::Shape::line(v, Stroke::new(width * self.s, color)));
    }
    pub fn poly(&self, pts: &[(f32, f32)], color: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pos(x, y)).collect();
        self.painter.add(egui::Shape::convex_polygon(v, color, Stroke::NONE));
    }
    /// 圆弧描边（角度单位弧度，0 = 正右，顺时针为正）。
    pub fn arc(&self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, width: f32, color: Color32) {
        let n = (((a1 - a0).abs() / TAU) * 96.0).ceil().max(2.0) as usize;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        self.path(&pts, width, color);
    }
    /// 径向柔光（中心 color → 边缘透明）。
    pub fn glow(&self, cx: f32, cy: f32, r: f32, color: Color32) {
        let mut mesh = egui::Mesh::default();
        let c = self.pos(cx, cy);
        mesh.colored_vertex(c, color);
        let n = 48;
        for i in 0..=n {
            let a = i as f32 / n as f32 * TAU;
            // 中间加一圈 45% 处的半透明，近似高斯衰减
            mesh.colored_vertex(c + Vec2::angled(a) * r * 0.45 * self.s, color.gamma_multiply(0.45));
            mesh.colored_vertex(c + Vec2::angled(a) * r * self.s, Color32::TRANSPARENT);
        }
        for i in 0..n as u32 {
            let (m0, o0, m1, o1) = (1 + i * 2, 2 + i * 2, 3 + i * 2, 4 + i * 2);
            mesh.add_triangle(0, m0, m1);
            mesh.add_triangle(m0, o0, o1);
            mesh.add_triangle(m0, o1, m1);
        }
        self.painter.add(egui::Shape::mesh(mesh));
    }
    /// 圆角竖直渐变，沿用 egui 的边缘抗锯齿。
    pub fn vgrad(&self, x: f32, y: f32, w: f32, h: f32, radius:f32, top: Color32, bottom: Color32) {
        let r = self.rect(x, y, w, h);
        let shapes=vec![egui::epaint::ClippedShape{clip_rect:self.painter.clip_rect(),shape:egui::Shape::rect_filled(r,radius*self.s,Color32::WHITE)}];
        for primitive in self.ui.ctx().tessellate(shapes,self.ui.ctx().pixels_per_point()) {
            if let egui::epaint::Primitive::Mesh(mut mesh)=primitive.primitive {
                mesh.texture_id=egui::TextureId::default();
                for vertex in &mut mesh.vertices {
                    let t=((vertex.pos.y-r.top())/r.height()).clamp(0.0,1.0);
                    vertex.color=alpha(mix(top,bottom,t),vertex.color.a() as f32/255.0);
                }
                self.painter.add(egui::Shape::mesh(mesh));
            }
        }
    }
    /// 虚线圆角框；`phase` 让虚线沿边「流动」。
    pub fn dashed(&self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, color: Color32, phase: f32) {
        let mut pts = Vec::new();
        let corner = |cx: f32, cy: f32, a0: f32, pts: &mut Vec<Pos2>| {
            for i in 0..=8 {
                let a = a0 + FRAC_PI_2 * i as f32 / 8.0;
                pts.push(self.pos(cx + r * a.cos(), cy + r * a.sin()));
            }
        };
        corner(x + w - r, y + r, -FRAC_PI_2, &mut pts);
        corner(x + w - r, y + h - r, 0.0, &mut pts);
        corner(x + r, y + h - r, FRAC_PI_2, &mut pts);
        corner(x + r, y + r, PI, &mut pts);
        pts.push(pts[0]);
        let dash = 4.0 * self.s;
        let gap = 3.5 * self.s;
        let off = (phase * self.s).rem_euclid(dash + gap);
        let shapes = egui::Shape::dashed_line_with_offset(&pts, Stroke::new(width * self.s, color), &[dash], &[gap], off);
        self.painter.extend(shapes);
    }

    // ---------- 交互 ----------
    pub fn hit(&self, x: f32, y: f32, w: f32, h: f32, id: impl std::hash::Hash) -> Response {
        let rect = self.rect(x, y, w, h).intersect(self.painter.clip_rect());
        let sense = if self.live { Sense::click() } else { Sense::hover() };
        let id = Id::new(("card", self.live, id));
        self.ui.interact(rect, id, sense)
    }

    pub fn hit_drag(&self,x:f32,y:f32,w:f32,h:f32,id:impl std::hash::Hash)->Response {
        let rect=self.rect(x,y,w,h).intersect(self.painter.clip_rect());
        self.ui.interact(rect,Id::new(("card-drag",self.live,id)),if self.live{Sense::click_and_drag()}else{Sense::hover()})
    }
    /// 悬停 / 按下的平滑过渡量（0..1）。
    pub fn hover_t(&self, r: &Response) -> (f32, f32) {
        let ctx = self.ui.ctx();
        let h = ctx.animate_bool_with_time(r.id.with("h"), self.live && r.hovered(), 0.14);
        let p = ctx.animate_bool_with_time(r.id.with("p"), self.live && r.is_pointer_button_down_on(), 0.07);
        (h, p)
    }
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

pub fn alpha(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

#[cfg(test)]
mod gradient_tests {
    use super::*;
    #[test]
    fn regression_play_icon_has_no_internal_seam() {
        for ppp in [1.0,1.25,1.5,2.0] {
            for scale in [0.9,1.0,1.4] {
                for offset in [0.0,0.25,0.5,0.75] {
                    for (size,color) in [11.0,15.0].into_iter().flat_map(|size|[Color32::WHITE,Color32::from_gray(22),Color32::from_white_alpha(128)].map(|color|(size,color))) {
                        let ctx=egui::Context::default();ctx.set_zoom_factor(ppp);
                        let center=Pos2::new(30.0+offset,26.0+offset);
                        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::splat(64.0))),..Default::default()};
                        let output=ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{
                            let card=Card::new(ui,Pos2::ZERO,ui.max_rect()).scaled(center.x,center.y,scale);
                            play_pause(&card,center.x,center.y,size,0.0,color);
                        });});
                        let mut raster=crate::preview_raster::Raster::new((64.0*ppp) as usize,(64.0*ppp) as usize);
                        raster.update_textures(&output.textures_delta);
                        raster.draw(&ctx.tessellate(output.shapes,ppp),ppp);
                        let y=(center.y*ppp).floor() as usize;
                        for dx in [-0.5,0.0,0.5,1.0,1.5] {
                            let x=((center.x+dx*scale)*ppp).floor() as usize;
                            let expected=color.a() as f32/255.0;
                            assert!((raster.buf[y*raster.w+x][3]-expected).abs()<0.02,"播放三角形内部应连续：ppp={ppp}, scale={scale}, offset={offset}, dx={dx}, alpha={}",raster.buf[y*raster.w+x][3]);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn long_email_links_do_not_create_hundreds_of_layouts() {
        let ctx=egui::Context::default();
        let text=format!("https://example.com/{}","newsletter-link-".repeat(40));
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::splat(500.0))),..Default::default()};
        let _=ctx.run(raw,|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let c=Card::new(ui,Pos2::ZERO,ui.max_rect());
                let before=ctx.fonts(|f|f.num_galleys_in_cache());
                let fitted=c.fit(&text,10.5,W::Regular,180.0);
                assert!(fitted.ends_with('…'));
                assert!(c.text_w(&fitted,10.5,W::Regular)<=180.0);
                let layouts=ctx.fonts(|f|f.num_galleys_in_cache())-before;
                assert!(layouts<=20,"长邮件链接只需少量排版，不应逐字重排：{layouts}");
                println!("长链接排版次数：{layouts}");
            });
        });
    }
    #[test]
    fn text_fitting_preserves_short_text_and_unicode_results() {
        let ctx=egui::Context::default();
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::splat(500.0))),..Default::default()};
        let _=ctx.run(raw,|ctx|{egui::CentralPanel::default().show(ctx,|ui|{
            let c=Card::new(ui,Pos2::ZERO,ui.max_rect());
            for text in ["", "a", "收到新的邮件与验证码", "会议提醒 🌴 完整标题", "https://example.com/short"] {
                for width in [0.0,10.0,40.0,80.0,300.0] {
                    let expected=if c.text_w(text,10.5,W::Regular)<=width{text.into()}else{
                        let chars:Vec<_>=text.chars().collect();(1..chars.len()).rev().map(|n|chars[..n].iter().collect::<String>()+"…").find(|s|c.text_w(s,10.5,W::Regular)<=width).unwrap_or_else(||"…".into())
                    };
                    assert_eq!(c.fit(text,10.5,W::Regular,width),expected,"宽度 {width} 的文字裁剪应保持原来的显示结果");
                }
            }
        });});
    }
    #[test]
    fn cover_gradient_keeps_all_four_corners_clear() {
        let ctx=egui::Context::default();
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::splat(100.0))),..Default::default()};
        let output=ctx.run(raw,|ctx|{
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{
                let c=Card::new(ui,Pos2::ZERO,ui.max_rect());
                c.vgrad(20.0,20.0,62.0,62.0,13.0,Color32::RED,Color32::BLUE);
            });
        });
        let meshes:Vec<_>=output.shapes.into_iter().filter_map(|s|if let egui::Shape::Mesh(m)=s.shape{Some(m)}else{None}).collect();
        let covers=|point:Pos2|meshes.iter().any(|m|m.indices.chunks_exact(3).any(|tri|{
            let [a,b,c]=[m.vertices[tri[0] as usize].pos,m.vertices[tri[1] as usize].pos,m.vertices[tri[2] as usize].pos];
            let cross=|a:Pos2,b:Pos2|(b.x-a.x)*(point.y-a.y)-(b.y-a.y)*(point.x-a.x);
            let signs=[cross(a,b),cross(b,c),cross(c,a)];
            signs.iter().all(|s|*s>=0.0)||signs.iter().all(|s|*s<=0.0)
        }));
        assert!(covers(Pos2::new(51.0,51.0)),"封面中心需要被绘制");
        for (x,y) in [(20.5,20.5),(81.5,20.5),(20.5,81.5),(81.5,81.5)] {
            assert!(!covers(Pos2::new(x,y)),"渐变不能覆盖圆角之外的区域");
        }
    }
}

/// 胶囊按钮：悬停提亮、按下轻微缩小，返回 (响应, 内容画布)。
pub fn capsule<'a>(
    c: &Card<'a>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    id: &str,
    base: Color32,
    hover: Color32,
) -> (Response, Card<'a>) {
    let r = c.hit(x, y, w, h, id);
    let (ht, pt) = c.hover_t(&r);
    let k = 1.0 - 0.05 * pt;
    let inner = c.scaled(x + w / 2.0, y + h / 2.0, k);
    inner.fill(x, y, w, h, h / 2.0, mix(base, hover, ht));
    (r, inner)
}

// ======================= 图标 =======================

pub fn close(c:&Card,cx:f32,cy:f32,r:f32,color:Color32) {
    c.seg((cx-r,cy-r),(cx+r,cy+r),1.2,color);
    c.seg((cx-r,cy+r),(cx+r,cy-r),1.2,color);
}

pub fn chevron(c: &Card, x: f32, y: f32, dir: f32, hover: f32, press: f32) {
    let k = 1.0 - 0.08 * press;
    let c = c.scaled(x, y, k);
    c.circle(x, y, 9.0, mix(theme::ARROW_BG, Color32::from_rgb(78, 78, 82), hover));
    let nudge = dir * 0.8 * hover;
    let (cx, s) = (x + nudge + dir * 0.6, 2.7);
    c.path(&[(cx - dir * s * 0.75, y - s), (cx + dir * s * 0.45, y), (cx - dir * s * 0.75, y + s)], 1.6, Color32::WHITE);
}

/// 电池环：充电绿色 + 闪电；低电量红色；否则灰色并显示百分比弧。
pub fn battery(c: &Card, cx: f32, cy: f32, pct: Option<u8>, charging: bool, k: f32) {
    let r = 8.2 * k;
    let w = 2.6 * k;
    let level = pct.unwrap_or(100) as f32 / 100.0;
    let col = if charging {
        theme::CHARGE
    } else if level < 0.2 {
        theme::RED
    } else {
        Color32::from_rgb(220, 220, 224)
    };
    c.ring(cx, cy, r, w, alpha(col, 0.22));
    let a0 = -FRAC_PI_2;
    if level >= 0.999 {
        c.ring(cx, cy, r, w, col);
    } else {
        c.arc(cx, cy, r, a0, a0 + TAU * level.max(0.02), w, col);
    }
    if charging || pct.is_none() {
        let s = k;
        c.poly(&[(cx + 1.6 * s, cy - 5.0 * s), (cx - 3.2 * s, cy + 0.9 * s), (cx + 0.3 * s, cy + 0.9 * s)], col);
        c.poly(&[(cx - 0.3 * s, cy - 0.9 * s), (cx + 3.2 * s, cy - 0.9 * s), (cx - 1.6 * s, cy + 5.0 * s)], col);
    } else {
        c.text(cx, cy + 2.4 * k, &format!("{}", pct.unwrap_or(0)), 6.6 * k, W::Bold, col, H::Center);
    }
}

/// 播放 ↔ 暂停形变：`t` = 0 播放三角，1 暂停双竖条。
pub fn play_pause(c: &Card, cx: f32, cy: f32, size: f32, t: f32, color: Color32) {
    let (w, h) = (size * 0.86, size);
    let bw = w * 0.34;
    let l = |a: (f32, f32), b: (f32, f32)| (cx + a.0 + (b.0 - a.0) * t, cy + a.1 + (b.1 - a.1) * t);
    // 三角拆成左梯形 + 右三角，与两根竖条一一对应插值
    let dx = w * 0.08 * (1.0 - t); // 三角视觉重心右移
    if t==0.0 {
        // 静止播放态画完整三角形，公共边不能被两块图形分别羽化。
        c.poly(&[(cx-w/2.0+dx,cy-h/2.0),(cx+w/2.0+dx,cy),(cx-w/2.0+dx,cy+h/2.0)],color);
        return;
    }
    let left = [
        l((-w / 2.0 + dx, -h / 2.0), (-w / 2.0, -h / 2.0)),
        l((dx, -h / 4.0), (-w / 2.0 + bw, -h / 2.0)),
        l((dx, h / 4.0), (-w / 2.0 + bw, h / 2.0)),
        l((-w / 2.0 + dx, h / 2.0), (-w / 2.0, h / 2.0)),
    ];
    let right = [
        l((dx, -h / 4.0), (w / 2.0 - bw, -h / 2.0)),
        l((w / 2.0 + dx, -0.01), (w / 2.0, -h / 2.0)),
        l((w / 2.0 + dx, 0.01), (w / 2.0, h / 2.0)),
        l((dx, h / 4.0), (w / 2.0 - bw, h / 2.0)),
    ];
    c.poly(&left, color);
    c.poly(&right, color);
}

pub fn skip(c: &Card, cx: f32, cy: f32, dir: f32, color: Color32) {
    for k in [-1.0f32, 1.0] {
        let x = cx + k * 4.3;
        c.poly(&[(x - dir * 4.6, cy - 6.0), (x + dir * 4.6, cy), (x - dir * 4.6, cy + 6.0)], color);
    }
}

pub fn playback_bars(c:&Card,cx:f32,cy:f32,time:f64,playing:bool,color:Color32) {
    for i in 0..5 {
        let phase=if playing {time as f32*(5.0+i as f32*0.7)}else{0.0}+i as f32*1.6;
        let height=if playing {3.5+(phase.sin()*0.5+0.5)*8.0}else{3.5};
        c.fill(cx-7.8+i as f32*3.9,cy-height/2.0,1.9,height,0.95,color);
    }
}

pub fn speaker(c:&Card,cx:f32,cy:f32,muted:bool,color:Color32) {
    c.fill(cx-7.0,cy-2.8,4.0,5.6,0.8,color);
    c.poly(&[(cx-4.0,cy-2.8),(cx+0.5,cy-6.0),(cx+0.5,cy+6.0),(cx-4.0,cy+2.8)],color);
    if muted {
        c.seg((cx+4.0,cy-2.7),(cx+9.0,cy+2.7),1.4,color);
        c.seg((cx+4.0,cy+2.7),(cx+9.0,cy-2.7),1.4,color);
    } else {
        c.arc(cx-0.8,cy,5.4,-0.7,0.7,1.4,color);
        c.arc(cx-0.8,cy,8.5,-0.7,0.7,1.4,alpha(color,0.6));
    }
}

pub fn note(c: &Card, cx: f32, cy: f32, size: f32, color: Color32) {
    let s = size;
    c.circle(cx - s * 0.24, cy + s * 0.28, s * 0.14, color);
    c.circle(cx + s * 0.26, cy + s * 0.18, s * 0.14, color);
    c.seg((cx - s * 0.11, cy + s * 0.28), (cx - s * 0.11, cy - s * 0.30), 1.6, color);
    c.seg((cx + s * 0.39, cy + s * 0.18), (cx + s * 0.39, cy - s * 0.42), 1.6, color);
    c.poly(&[(cx - s * 0.11 - 0.8, cy - s * 0.30), (cx + s * 0.39 + 0.8, cy - s * 0.42), (cx + s * 0.39 + 0.8, cy - s * 0.30), (cx - s * 0.11 - 0.8, cy - s * 0.18)], color);
}

pub fn stopwatch(c: &Card, cx: f32, cy: f32, r: f32, hand: f32, color: Color32) {
    c.ring(cx, cy + 2.0, r, 3.0, color);
    c.fill(cx - 6.8, cy - r - 6.0, 13.6, 3.6, 1.6, color);
    c.fill(cx - 1.8, cy - r - 3.0, 3.6, 4.0, 1.0, color);
    let a = -FRAC_PI_2 + hand;
    let k = (r + 2.5, r + 6.0);
    let b = (-FRAC_PI_2 + 0.78f32).sin_cos();
    c.seg((cx + b.1 * k.0, cy + 2.0 + b.0 * k.0), (cx + b.1 * k.1, cy + 2.0 + b.0 * k.1), 3.4, color);
    c.seg((cx, cy + 2.0), (cx + a.cos() * r * 0.62, cy + 2.0 + a.sin() * r * 0.62), 3.0, color);
    c.circle(cx, cy + 2.0, 2.0, color);
}

/// 重置图标：开口圆弧 + 箭头，`spin` 为整体旋转（弧度）。
pub fn reset(c: &Card, cx: f32, cy: f32, spin: f32, color: Color32) {
    let r = 5.6;
    let a0 = -1.1 + spin;
    let a1 = a0 + 5.0;
    c.arc(cx, cy, r, a0, a1, 1.8, color);
    let (s, co) = a0.sin_cos();
    let tip = (cx + co * r, cy + s * r);
    // 箭头沿切线方向（逆时针）
    let t = (s, -co);
    let n = (co, s);
    c.poly(
        &[
            (tip.0 + t.0 * 3.4, tip.1 + t.1 * 3.4),
            (tip.0 + n.0 * 2.8 - t.0 * 0.6, tip.1 + n.1 * 2.8 - t.1 * 0.6),
            (tip.0 - n.0 * 2.8 - t.0 * 0.6, tip.1 - n.1 * 2.8 - t.1 * 0.6),
        ],
        color,
    );
}

pub fn coffee(c: &Card, x: f32, y: f32, color: Color32) {
    c.stroke(x - 4.5, y - 2.0, 7.6, 6.0, 1.8, 1.1, color);
    c.arc(x + 3.4, y + 1.0, 1.8, -FRAC_PI_2, FRAC_PI_2, 1.1, color);
    c.seg((x - 2.0, y - 5.8), (x - 2.0, y - 3.8), 1.0, color);
    c.seg((x + 0.6, y - 6.4), (x + 0.6, y - 3.8), 1.0, color);
}

pub fn target(c: &Card, x: f32, y: f32, color: Color32) {
    c.ring(x, y, 5.0, 1.1, color);
    c.ring(x, y, 2.4, 1.1, color);
    c.seg((x, y), (x + 5.2, y - 5.2), 1.2, color);
    c.poly(&[(x + 5.6, y - 7.4), (x + 7.4, y - 5.6), (x + 4.4, y - 4.4)], color);
}

/// 办公椅（久坐提醒的图标）：侧面的实心转椅——微微后仰的靠背、扶手、坐垫、气压杆、带滚轮的底座。
/// 和水滴一样是物体图标，宽约 24k、高约 30k，(x,y) 为中心。
pub fn chair(c: &Card, x: f32, y: f32, k: f32, color: Color32) {
    let p = |dx: f32, dy: f32| (x + dx * k, y + dy * k);
    let bar = |a: (f32, f32), b: (f32, f32), w: f32| {
        c.seg(p(a.0, a.1), p(b.0, b.1), w * k, color);
        c.circle(p(a.0, a.1).0, p(a.0, a.1).1, w * k * 0.5, color);
        c.circle(p(b.0, b.1).0, p(b.0, b.1).1, w * k * 0.5, color);
    };
    bar((-9.0, -13.0), (-7.0, 0.5), 5.0);        // 靠背
    bar((-7.0, 2.0), (8.5, 2.0), 4.6);           // 坐垫
    bar((-4.0, -4.5), (5.0, -4.5), 2.2);         // 扶手
    bar((3.5, -4.5), (3.5, 0.0), 2.2);           // 扶手支撑
    bar((0.5, 4.0), (0.5, 10.0), 2.6);           // 气压杆
    bar((-9.0, 11.0), (10.0, 11.0), 2.4);        // 底座
    for dx in [-9.0f32, 10.0] {                  // 滚轮
        c.circle(x + dx * k, y + 14.0 * k, 2.0 * k, color);
    }
}

pub fn droplet(c: &Card, cx: f32, cy: f32, size: f32, color: Color32) {
    // 水滴 = 尖顶 + 与两条切线相接的圆底
    let r = size * 0.34;
    let center = (cx, cy + size * 0.5 - r);
    let tip = (cx, cy - size * 0.5);
    let d = center.1 - tip.1;
    let phi = (r / d).clamp(-1.0, 1.0).acos();
    let mut pts = vec![tip];
    let n = 20;
    for i in 0..=n {
        let b = phi + (TAU - 2.0 * phi) * i as f32 / n as f32;
        pts.push((center.0 + r * b.sin(), center.1 - r * b.cos()));
    }
    c.poly(&pts, color);
}

pub(crate) fn bell_swing(elapsed:f64)->f32 {
    if !(0.0..0.95).contains(&elapsed) {return 0.0;}
    let t=elapsed as f32/0.95;
    (t*TAU*2.0).sin()*0.48*(1.0-t).sqrt()
}

pub fn bell(c: &Card, x: f32, y: f32, color: Color32, angle:f32) {
    let mut pts = Vec::new();
    for i in 0..=12 {
        let a = PI + PI * i as f32 / 12.0;
        pts.push((x + 3.4 * a.cos(), y - 0.6 + 3.4 * a.sin()));
    }
    pts.push((x + 3.6, y + 2.4));
    pts.push((x + 4.6, y + 3.2));
    pts.push((x - 4.6, y + 3.2));
    pts.push((x - 3.6, y + 2.4));
    let rotate=|(px,py):(f32,f32)| {
        let (sin,cos)=angle.sin_cos();let dx=px-x;let dy=py-(y-5.0);
        (x+dx*cos-dy*sin,y-5.0+dx*sin+dy*cos)
    };
    c.poly(&pts.into_iter().map(rotate).collect::<Vec<_>>(), color);
    let clapper=rotate((x,y+4.6));
    c.circle(clapper.0,clapper.1,1.2,color);
}

pub fn gear(c: &Card, x: f32, y: f32, color: Color32) {
    let points=(0..64).map(|i|{
        let angle=i as f32*TAU/64.0;
        let radius=if i%8<2 || i%8>=6 {7.0}else{5.6};
        c.pos(x+radius*angle.cos(),y+radius*angle.sin())
    }).collect();
    c.painter.add(egui::Shape::closed_line(points,Stroke::new(1.2*c.s,color)));
    c.painter.circle_stroke(c.pos(x,y),2.3*c.s,Stroke::new(1.2*c.s,color));
}

pub fn sliders(c: &Card, x: f32, y: f32, color: Color32) {
    for (dy, dx) in [(-2.4f32, -2.0f32), (2.4, 2.2)] {
        c.seg((x - 5.0, y + dy), (x + 5.0, y + dy), 1.1, color);
        c.circle(x + dx, y + dy, 1.9, color);
    }
}

pub fn plus(c: &Card, cx: f32, cy: f32, r: f32, color: Color32, ink: Color32) {
    c.circle(cx, cy, r, color);
    c.seg((cx - r * 0.5, cy), (cx + r * 0.5, cy), 1.3, ink);
    c.seg((cx, cy - r * 0.5), (cx, cy + r * 0.5), 1.3, ink);
}

pub fn pencil(c: &Card, cx: f32, cy: f32, size: f32, color: Color32) {
    let s = size;
    c.poly(&[(cx - s * 0.30, cy + s * 0.14), (cx + s * 0.22, cy - s * 0.38), (cx + s * 0.40, cy - s * 0.20), (cx - s * 0.12, cy + s * 0.32)], color);
    c.poly(&[(cx - s * 0.40, cy + s * 0.42), (cx - s * 0.30, cy + s * 0.14), (cx - s * 0.12, cy + s * 0.32)], color);
}

/// 勾：`t` 0..1 逐笔画出。
pub fn check(c: &Card, cx: f32, cy: f32, s: f32, t: f32, width: f32, color: Color32) {
    let a = (cx - s * 0.45, cy + s * 0.02);
    let b = (cx - s * 0.12, cy + s * 0.34);
    let d = (cx + s * 0.48, cy - s * 0.32);
    let t1 = (t / 0.4).clamp(0.0, 1.0);
    let t2 = ((t - 0.4) / 0.6).clamp(0.0, 1.0);
    if t1 <= 0.0 {
        return;
    }
    let p1 = (a.0 + (b.0 - a.0) * t1, a.1 + (b.1 - a.1) * t1);
    if t2 <= 0.0 {
        c.path(&[a, p1], width, color);
    } else {
        let p2 = (b.0 + (d.0 - b.0) * t2, b.1 + (d.1 - b.1) * t2);
        c.path(&[a, b, p2], width, color);
    }
}

pub fn checklist(c: &Card, cx: f32, cy: f32, size: f32, ink: Color32, plate: Color32) {
    c.fill(cx - size / 2.0, cy - size / 2.0, size, size, size * 0.26, plate);
    for dy in [-0.24f32, 0.0, 0.24] {
        let y = cy + dy * size;
        check(c, cx - size * 0.17, y, size * 0.2, 1.0, 1.6, ink);
        c.seg((cx + size * 0.04, y), (cx + size * 0.28, y), 1.6, ink);
    }
}

/// 下载托盘：向下箭头落进收纳盒。`drop` 为箭头下坠位移（拖入时轻微弹跳）。
pub fn tray(c: &Card, cx: f32, cy: f32, size: f32, drop: f32, color: Color32, shade: Color32) {
    let u = size / 30.0;
    let ay = cy - 13.0 * u + drop * u;
    c.fill(cx - 1.8 * u, ay, 3.6 * u, 8.0 * u, 1.0 * u, color);
    c.poly(&[(cx - 6.0 * u, ay + 6.5 * u), (cx + 6.0 * u, ay + 6.5 * u), (cx, ay + 12.5 * u)], color);
    c.poly(
        &[
            (cx - 10.0 * u, cy + 1.0 * u),
            (cx + 10.0 * u, cy + 1.0 * u),
            (cx + 13.5 * u, cy + 8.5 * u),
            (cx - 13.5 * u, cy + 8.5 * u),
        ],
        shade,
    );
    c.fill(cx - 14.0 * u, cy + 8.0 * u, 28.0 * u, 8.5 * u, 1.8 * u, color);
    let mut notch = Vec::new();
    for i in 0..=16 {
        let a = PI * i as f32 / 16.0;
        notch.push((cx + 4.6 * u * a.cos(), cy + 8.0 * u + 3.6 * u * a.sin()));
    }
    c.poly(&notch, shade);
}

// ---------- 文件图标（取不到系统图标时的矢量版） ----------
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    Folder,
    Image,
    Pdf,
    Doc,
    Sheet,
    Archive,
    Audio,
    Other,
}

pub fn file_kind(p: &std::path::Path) -> FileKind {
    if p.is_dir() {
        return FileKind::Folder;
    }
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "svg" => FileKind::Image,
        "pdf" => FileKind::Pdf,
        "doc" | "docx" | "txt" | "md" | "rtf" => FileKind::Doc,
        "xls" | "xlsx" | "csv" => FileKind::Sheet,
        "zip" | "7z" | "rar" | "tar" | "gz" => FileKind::Archive,
        "mp3" | "flac" | "wav" | "m4a" | "ogg" => FileKind::Audio,
        _ => FileKind::Other,
    }
}

pub fn file_glyph(c: &Card, kind: FileKind, cx: f32, cy: f32, size: f32) {
    let s = size / 36.0;
    if kind == FileKind::Folder {
        let back = Color32::from_rgb(230, 167, 38);
        let front = Color32::from_rgb(255, 202, 74);
        c.fill(cx - 17.0 * s, cy - 12.0 * s, 13.0 * s, 6.0 * s, 2.0 * s, back);
        c.fill(cx - 17.0 * s, cy - 9.5 * s, 34.0 * s, 22.0 * s, 3.0 * s, back);
        c.fill(cx - 17.0 * s, cy - 5.5 * s, 34.0 * s, 19.5 * s, 3.0 * s, front);
        c.fill(cx - 17.0 * s, cy - 5.5 * s, 34.0 * s, 1.2 * s, 0.6 * s, Color32::from_rgba_unmultiplied(255, 240, 200, 120));
        return;
    }
    // 纸张 + 折角
    let (w, h) = (26.0 * s, 33.0 * s);
    let (x0, y0) = (cx - w / 2.0, cy - h / 2.0);
    let fold = 7.5 * s;
    let paper = Color32::from_rgb(246, 246, 248);
    c.poly(&[(x0, y0 + 2.0 * s), (x0 + w - fold, y0), (x0 + w, y0 + fold), (x0 + w, y0 + h), (x0, y0 + h)], paper);
    c.fill(x0, y0, w - fold, h, 2.4 * s, paper);
    c.fill(x0, y0 + fold, w, h - fold, 2.4 * s, paper);
    c.poly(&[(x0 + w - fold, y0), (x0 + w, y0 + fold), (x0 + w - fold, y0 + fold)], Color32::from_rgb(205, 205, 212));
    let accent = match kind {
        FileKind::Image => Color32::from_rgb(64, 156, 255),
        FileKind::Pdf => Color32::from_rgb(235, 64, 52),
        FileKind::Doc => Color32::from_rgb(44, 110, 230),
        FileKind::Sheet => Color32::from_rgb(33, 160, 90),
        FileKind::Archive => Color32::from_rgb(140, 110, 230),
        FileKind::Audio => Color32::from_rgb(255, 69, 120),
        _ => Color32::from_rgb(150, 150, 160),
    };
    match kind {
        FileKind::Image => {
            c.fill(x0 + 4.0 * s, y0 + 11.0 * s, w - 8.0 * s, 15.0 * s, 2.0 * s, Color32::from_rgb(214, 234, 255));
            c.poly(&[(x0 + 5.0 * s, y0 + 25.0 * s), (x0 + 11.0 * s, y0 + 16.5 * s), (x0 + 16.0 * s, y0 + 25.0 * s)], accent);
            c.poly(&[(x0 + 11.0 * s, y0 + 25.0 * s), (x0 + 16.5 * s, y0 + 19.0 * s), (x0 + 21.0 * s, y0 + 25.0 * s)], Color32::from_rgb(40, 120, 220));
            c.circle(x0 + 18.0 * s, y0 + 14.5 * s, 1.8 * s, YELLOW_SOFT);
        }
        FileKind::Pdf | FileKind::Doc | FileKind::Sheet => {
            c.fill(x0 + 3.0 * s, y0 + h - 12.5 * s, 15.0 * s, 8.0 * s, 1.6 * s, accent);
            let tag = match kind {
                FileKind::Pdf => "PDF",
                FileKind::Doc => "DOC",
                _ => "XLS",
            };
            c.text(x0 + 10.5 * s, y0 + h - 6.3 * s, tag, 5.4 * s, W::Bold, Color32::WHITE, H::Center);
            for i in 0..3 {
                let y = y0 + 9.0 * s + i as f32 * 3.6 * s;
                c.fill(x0 + 4.0 * s, y, if i == 2 { 10.0 * s } else { 16.0 * s }, 1.4 * s, 0.7 * s, Color32::from_rgb(200, 200, 208));
            }
        }
        FileKind::Archive => {
            for i in 0..5 {
                let y = y0 + 3.0 * s + i as f32 * 3.2 * s;
                c.fill(cx - 2.2 * s + if i % 2 == 0 { 0.0 } else { 2.2 * s }, y, 2.2 * s, 2.0 * s, 0.4 * s, accent);
            }
            c.fill(cx - 3.0 * s, y0 + 19.0 * s, 6.0 * s, 7.0 * s, 1.6 * s, accent);
        }
        FileKind::Audio => note(c, cx, cy + 2.0 * s, 16.0 * s, accent),
        _ => {
            for i in 0..4 {
                let y = y0 + 10.0 * s + i as f32 * 4.0 * s;
                c.fill(x0 + 4.0 * s, y, 18.0 * s, 1.4 * s, 0.7 * s, Color32::from_rgb(200, 200, 208));
            }
        }
    }
}

const YELLOW_SOFT: Color32 = Color32::from_rgb(255, 214, 92);


// 时段文字弱化，数字保持原有字号与字重。
pub fn clock_width(c:&Card,text:&str,size:f32)->f32 {
    match text.split_once(' ') {
        Some((period,digits))=>c.text_w(period,size*0.72,W::Semibold)+4.0+c.text_w(digits,size,W::Bold),
        None=>c.text_w(text,size,W::Bold),
    }
}
pub fn clock(c:&Card,x:f32,y:f32,text:&str,size:f32) {
    let (digits,x)=if let Some((period,digits))=text.split_once(' ') {
        let numbers=c.galley(digits,size,W::Bold,theme::WHITE);
        let ascent=numbers.rows.first().and_then(|r|r.glyphs.first()).map(|g|g.pos.y).unwrap_or(size*c.s);
        let center=y+(numbers.mesh_bounds.center().y-ascent)/c.s;
        c.centered_text(x,center,period,size*0.72,W::Semibold,theme::WHITE,H::Left);
        (digits,x+c.text_w(period,size*0.72,W::Semibold)+4.0)
    }else{(text,x)};
    c.text(x,y,digits,size,W::Bold,theme::WHITE,H::Left);
}

/// 图钉：固定的剪贴板条目。
pub fn pin(c: &Card, cx: f32, cy: f32, size: f32, color: Color32) {
    let s = size / 10.0;
    c.fill(cx - 2.4 * s, cy - 5.0 * s, 4.8 * s, 4.2 * s, 1.0 * s, color);
    c.poly(&[(cx - 4.2 * s, cy - 1.2 * s), (cx + 4.2 * s, cy - 1.2 * s), (cx + 2.6 * s, cy + 1.0 * s), (cx - 2.6 * s, cy + 1.0 * s)], color);
    c.seg((cx, cy + 1.0 * s), (cx, cy + 5.2 * s), 1.1, color);
}

/// 剪贴板：空状态插图。
pub fn clipboard_glyph(c: &Card, cx: f32, cy: f32, size: f32, color: Color32, shade: Color32) {
    let u = size / 22.0;
    c.fill(cx - 9.0 * u, cy - 9.0 * u, 18.0 * u, 22.0 * u, 3.0 * u, shade);
    c.stroke(cx - 9.0 * u, cy - 9.0 * u, 18.0 * u, 22.0 * u, 3.0 * u, 1.3, color);
    c.fill(cx - 4.5 * u, cy - 11.5 * u, 9.0 * u, 5.0 * u, 1.6 * u, color);
    for (i, w) in [11.0, 8.0, 10.0].into_iter().enumerate() {
        c.fill(cx - 5.5 * u, cy - 1.5 * u + i as f32 * 4.5 * u, w * u, 1.4 * u, 0.7 * u, color);
    }
}

const CHASE_ROWS: [f32; 23] = [
    -16.0, -11.0, -8.2, -6.2, -4.6, -3.4, -2.4, -1.5, -1.0, -0.65, -0.32, 0.0, 0.32, 0.65, 1.0, 1.5, 2.4, 3.4, 4.6, 6.2,
    8.2, 11.0, 16.0,
];

/// 进度条追光：**一片**解析式彗尾，不是「光带 + 亮珠 + 几层光晕」的叠罗汉。
///
/// 每个顶点直接采样五个连续函数并就地做 source-over 合成：
/// 1. `halo`：沿光带铺开的宽高斯外晕，把光"洇"到轨道上下；
/// 2. `under`：已播放段的底色光，横贯整段、很淡；
/// 3. `band`：贴着中轴的细光带，**在头部收窄**，尾巴那头再张开；
/// 4. `core`：播放头附近的中等亮核；
/// 5. `needle`：压在播放头正上方的一颗极小的近白高光——头部之所以"尖"全靠它。
/// 五者共用同一条横向拖尾曲线，所以从头到尾是一次连续过渡，
/// 没有矩形拼接、没有台阶色带、没有可见的圆环接缝。
///
/// 光**朝后**：亮核在 `head`，尾巴一路向左按 `exp(-(d/tail)^1.35)` 衰减，
/// 头前只有约 3 DIP 的收边（两头一阶导都为 0，接得上，不会顿一下）。
///
/// `hot`：暂停 0.45 / 播放 0.62 / 按住拖动 1.0，只影响亮度与尾巴长度。
pub fn chase(c: &Card, x0: f32, head: f32, y: f32, thick: f32, tint: Color32, hot: f32) {
    let th = thick.max(1.0);
    let hot = hot.clamp(0.0, 1.0);
    let span = head - x0;
    if span < 0.5 {
        return;
    }
    // 拖尾特征长度：播放约 46 DIP，拖动时拉长到 56 DIP（越用力尾巴越长）。
    let tail = 30.0 + 26.0 * hot;
    let sy = th * 0.46; // 细光带半高
    let by = th * 1.15 + 1.4 * hot; // 外晕半径（贴着轨道，不要糊成一片雾）
    let cx = th * 1.1; // 亮核横向尺度
    let cy = th * 0.9; // 亮核纵向尺度
    let nx = th * 0.85; // 尖端高光的横向尺度
    let ny = th * 0.32; // 竖向尺度（压扁 → 收成横向的楔形尖头）
    let ramp = 6.0; // 轨道左端收边（够宽才不会在起点切出一条垂直硬边）
    let lead = 6.0f32; // 头部前方收边（越宽过渡越软）

    // 按屏幕像素宽度过滤细光带，移动时亮度不会集中到单行像素。
    let pixel = 1.0 / (c.s * c.ui.ctx().pixels_per_point()).max(0.1);
    let filter_variance = pixel * pixel / 6.0;
    let filtered = |offset: f32, width: f32| {
        let variance = width * width + filter_variance;
        width / variance.sqrt() * (-offset * offset / variance).exp()
    };

    let a_halo = 0.045 + 0.045 * hot;
    let a_under = 0.26 + 0.10 * hot;
    let a_band = 0.16 + 0.16 * hot;
    let a_core = 0.24 + 0.22 * hot;
    let a_needle = 0.50 + 0.30 * hot;

    let c_halo = mix(tint, Color32::WHITE, 0.34);
    let c_under = mix(tint, Color32::WHITE, 0.46);
    let c_band = mix(tint, Color32::WHITE, 0.66);
    let c_core = mix(tint, Color32::WHITE, 0.90);
    let c_needle = mix(tint, Color32::WHITE, 0.97);

    // 列：尾巴 1 DIP 一列（平滑曲线够用），头部 6 DIP 单独加密到 0.6 DIP，
    // 并且**保证有一列正好落在 head 上**——尖点不被插值削平，头才立得住。
    let n_back = (span / 1.0).ceil().max(2.0) as usize;
    let n_fwd = (lead / 0.6).ceil().max(2.0) as usize;
    let mut xs = Vec::with_capacity(n_back + n_fwd + 1);
    for i in 0..=n_back {
        xs.push(x0 + span * i as f32 / n_back as f32);
    }
    for i in 1..=n_fwd {
        xs.push(head + lead * i as f32 / n_fwd as f32);
    }
    let cols = xs.len();

    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve(cols * CHASE_ROWS.len());
    for off in CHASE_ROWS {
        for &x in &xs {
            let d = head - x; // >0 在头后（左），<0 在头前
            // 横向：向后指数拖尾，向前 6 DIP 内平滑收边
            let back = (-(d.max(0.0) / tail).powf(1.35)).exp();
            // 前端收边随 |off| 提前：越靠上下越早收，前端自然成一个尖角，
            // 而不是一条垂直切平的钝口。
            let d_front = d - off.abs();  // 收角 45°：每往后 1 DIP，半宽张 1 DIP
            let fwd = if d_front >= 0.0 {
                1.0
            } else {
                let q = (-d_front / lead).min(1.0);
                // 两端导数为 0 的平滑收边（smoothstep 的补），斜坡均匀，不会出现台阶
                (1.0 - q).powi(2) * (1.0 + 2.0 * q)
            };
            let s = ((x - x0) / ramp).clamp(0.0, 1.0);
            let start = s * s * (3.0 - 2.0 * s);
            let prof = back * fwd * start;
            // 纵向：所有层都在头部收敛成一点（锥形收尖），往尾巴那头再张开
            let t_along = (d.max(0.0) / tail).min(1.0);
            let taper = 0.48 + 0.52 * t_along;
            let taper_halo = 0.40 + 0.60 * t_along;
            let halo = (-(off / (by * taper_halo)).powi(2)).exp();
            let band = filtered(off, sy * taper / 1.2_f32.sqrt());
            let core = (-((d / (cx * (0.6 + 0.4 * t_along))).powi(2) + (off / (cy * (0.5 + 0.5 * t_along))).powi(2)) * 1.4).exp();
            // 尖端：很扁很亮的一道，自己按高斯在头前几 DIP 内收掉，不被 6 DIP 的
            // 前收边切齐——否则会看见一条垂直的刀口
            let needle = (-(d / nx).powi(2) * 1.2).exp() * filtered(off, ny / 1.2_f32.sqrt());

            let mut acc = [0.0f32; 3];
            let mut inv = 1.0f32;
            let mut put = |col: Color32, sa: f32| {
                let sa = sa.clamp(0.0, 1.0);
                if sa <= 0.002 {
                    return;
                }
                acc[0] += col.r() as f32 / 255.0 * sa * inv;
                acc[1] += col.g() as f32 / 255.0 * sa * inv;
                acc[2] += col.b() as f32 / 255.0 * sa * inv;
                inv *= 1.0 - sa;
            };
            put(c_halo, a_halo * prof * halo);
            // 已播放段的底光不跟着尾巴衰减：整段都要看得见「已播放」，
            // 只是越靠近头部越亮——衰减交给上面那几层彗尾。
            put(c_under, a_under * fwd * start * band);
            put(c_band, a_band * prof * band);
            put(c_core, a_core * core * fwd);
            put(c_needle, a_needle * needle);
            let a = 1.0 - inv;
            let inv_a = if a > 1e-4 { 1.0 / a } else { 0.0 };
            let q = |v: f32| (v * inv_a * 255.0).clamp(0.0, 255.0) as u8;
            let col = Color32::from_rgba_unmultiplied(q(acc[0]), q(acc[1]), q(acc[2]), (a * 255.0) as u8);
            mesh.colored_vertex(c.pos(x, y + off), col);
        }
    }
    let stride = cols as u32;
    for j in 0..CHASE_ROWS.len() as u32 - 1 {
        for i in 0..stride - 1 {
            let a = j * stride + i;
            let b = a + 1;
            let d = a + stride;
            let e = d + 1;
            mesh.add_triangle(a, b, e);
            mesh.add_triangle(a, e, d);
        }
    }
    c.painter.add(egui::Shape::mesh(mesh));
}
