//! 极简 egui 软件光栅器：把 `ClippedPrimitive` 三角形网格画进 RGBA 帧缓冲。
//!
//! 与 egui_glow 的混合方式一致：顶点色与纹理都是 gamma 空间预乘 alpha，
//! `dst = src + dst·(1 - src.a)`。抗锯齿由 egui 的羽化网格本身提供。

use eframe::egui::{epaint, ImageData, TextureId, TexturesDelta};
use std::collections::HashMap;

struct Tex {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

pub struct Raster {
    pub w: usize,
    pub h: usize,
    pub buf: Vec<[f32; 4]>,
    textures: HashMap<TextureId, Tex>,
}

impl Raster {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, buf: vec![[0.0; 4]; w * h], textures: HashMap::new() }
    }

    pub fn update_textures(&mut self, delta: &TexturesDelta) {
        for (id, d) in &delta.set {
            let (w, h, px): (usize, usize, Vec<[f32; 4]>) = match &d.image {
                ImageData::Color(img) => (
                    img.width(),
                    img.height(),
                    img.pixels.iter().map(|c| [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0]).collect(),
                ),
                ImageData::Font(img) => (
                    img.width(),
                    img.height(),
                    img.srgba_pixels(None)
                        .map(|c| [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0])
                        .collect(),
                ),
            };
            match d.pos {
                Some([x0, y0]) => {
                    let t = self.textures.get_mut(id).expect("partial update of unknown texture");
                    for y in 0..h {
                        for x in 0..w {
                            t.px[(y0 + y) * t.w + x0 + x] = px[y * w + x];
                        }
                    }
                }
                None => {
                    self.textures.insert(*id, Tex { w, h, px });
                }
            }
        }
    }

    pub fn free_textures(&mut self, delta: &TexturesDelta) {
        for id in &delta.free {
            self.textures.remove(id);
        }
    }

    pub fn draw(&mut self, prims: &[epaint::ClippedPrimitive], ppp: f32) {
        for p in prims {
            let epaint::Primitive::Mesh(mesh) = &p.primitive else { continue };
            let clip = p.clip_rect;
            let cx0 = ((clip.min.x * ppp).round().max(0.0) as i32).min(self.w as i32);
            let cy0 = ((clip.min.y * ppp).round().max(0.0) as i32).min(self.h as i32);
            let cx1 = ((clip.max.x * ppp).round().max(0.0) as i32).min(self.w as i32);
            let cy1 = ((clip.max.y * ppp).round().max(0.0) as i32).min(self.h as i32);
            if cx0 >= cx1 || cy0 >= cy1 {
                continue;
            }
            let tex = self.textures.get(&mesh.texture_id).map(|t| t as *const Tex);
            for tri in mesh.indices.chunks_exact(3) {
                let v = [&mesh.vertices[tri[0] as usize], &mesh.vertices[tri[1] as usize], &mesh.vertices[tri[2] as usize]];
                // SAFETY: 纹理表在绘制期间不会被修改
                let t = tex.map(|p| unsafe { &*p });
                self.tri(v, ppp, (cx0, cy0, cx1, cy1), t);
            }
        }
    }

    fn tri(&mut self, v: [&epaint::Vertex; 3], ppp: f32, clip: (i32, i32, i32, i32), tex: Option<&Tex>) {
        let p: [(f32, f32); 3] = [(v[0].pos.x * ppp, v[0].pos.y * ppp), (v[1].pos.x * ppp, v[1].pos.y * ppp), (v[2].pos.x * ppp, v[2].pos.y * ppp)];
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        if area.abs() < 1e-8 {
            return;
        }
        let x0 = (p.iter().map(|q| q.0).fold(f32::MAX, f32::min).floor() as i32).max(clip.0);
        let x1 = (p.iter().map(|q| q.0).fold(f32::MIN, f32::max).ceil() as i32).min(clip.2);
        let y0 = (p.iter().map(|q| q.1).fold(f32::MAX, f32::min).floor() as i32).max(clip.1);
        let y1 = (p.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil() as i32).min(clip.3);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let inv = 1.0 / area;
        let col: [[f32; 4]; 3] = std::array::from_fn(|i| {
            let c = v[i].color;
            [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0]
        });
        let uv: [(f32, f32); 3] = std::array::from_fn(|i| (v[i].uv.x, v[i].uv.y));
        let edge = |a: (f32, f32), b: (f32, f32), x: f32, y: f32| (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
        for y in y0..y1 {
            let py = y as f32 + 0.5 + 0.000291; // 采样点微偏：共享边上的像素只归一个三角形
            for x in x0..x1 {
                let px = x as f32 + 0.5 + 0.000137;
                let w0 = edge(p[1], p[2], px, py) * inv;
                let w1 = edge(p[2], p[0], px, py) * inv;
                let w2 = edge(p[0], p[1], px, py) * inv;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let mut c = [0.0f32; 4];
                for k in 0..4 {
                    c[k] = col[0][k] * w0 + col[1][k] * w1 + col[2][k] * w2;
                }
                if let Some(t) = tex {
                    let u = uv[0].0 * w0 + uv[1].0 * w1 + uv[2].0 * w2;
                    let vv = uv[0].1 * w0 + uv[1].1 * w1 + uv[2].1 * w2;
                    let s = sample(t, u, vv);
                    for k in 0..4 {
                        c[k] *= s[k];
                    }
                }
                if c[3] <= 0.0 && c[0] + c[1] + c[2] <= 0.0 {
                    continue;
                }
                let d = &mut self.buf[y as usize * self.w + x as usize];
                let ia = 1.0 - c[3];
                for k in 0..4 {
                    d[k] = c[k] + d[k] * ia;
                }
            }
        }
    }

    pub fn to_rgba8(&self, out: &mut Vec<u8>) {
        out.clear();
        for p in &self.buf {
            for k in 0..3 {
                out.push((p[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            }
            out.push(255);
        }
    }
}

fn sample(t: &Tex, u: f32, v: f32) -> [f32; 4] {
    let x = (u * t.w as f32 - 0.5).clamp(0.0, (t.w - 1) as f32);
    let y = (v * t.h as f32 - 0.5).clamp(0.0, (t.h - 1) as f32);
    let (xi, yi) = (x.floor() as usize, y.floor() as usize);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let x1 = (xi + 1).min(t.w - 1);
    let y1 = (yi + 1).min(t.h - 1);
    let a = t.px[yi * t.w + xi];
    let b = t.px[yi * t.w + x1];
    let c = t.px[y1 * t.w + xi];
    let d = t.px[y1 * t.w + x1];
    let mut o = [0.0; 4];
    for k in 0..4 {
        o[k] = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy;
    }
    o
}

