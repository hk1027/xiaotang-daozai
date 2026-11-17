//! 动效基础件：弹簧、缓动与插值。
//!
//! 所有形变都用「带速度的弹簧」驱动：目标随时可改，速度保留，
//! 所以展开途中移开、收起途中回来都不会跳变，这是「丝滑」的关键。

/// 二阶阻尼弹簧 `x'' = -k(x - target) - c·x'`，半隐式欧拉 + 子步长积分。
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub pos: f32,
    pub vel: f32,
    pub target: f32,
    k: f32,
    c: f32,
}

impl Spring {
    /// `stiffness` = k，`zeta` = 阻尼比（<1 有过冲，=1 临界阻尼）。
    pub fn new(value: f32, stiffness: f32, zeta: f32) -> Self {
        let mut s = Self { pos: value, vel: 0.0, target: value, k: 0.0, c: 0.0 };
        s.tune(stiffness, zeta);
        s
    }

    pub fn tune(&mut self, stiffness: f32, zeta: f32) {
        self.k = stiffness;
        self.c = 2.0 * zeta * stiffness.sqrt();
    }

    pub fn snap(&mut self, value: f32) {
        self.pos = value;
        self.target = value;
        self.vel = 0.0;
    }

    pub fn step(&mut self, dt: f32) {
        // 掉帧时限幅，避免一帧跨太远；子步 ≤ 1/480 s，保证在 30–240 Hz 下轨迹一致。
        let dt = dt.clamp(0.0, 0.1);
        let n = (dt * 480.0).ceil().max(1.0) as usize;
        let h = dt / n as f32;
        for _ in 0..n {
            let a = -self.k * (self.pos - self.target) - self.c * self.vel;
            self.vel += a * h;
            self.pos += self.vel * h;
        }
    }

    /// 相对 `scale` 足够接近目标且几乎静止。
    pub fn settled(&self, scale: f32) -> bool {
        (self.pos - self.target).abs() < 0.002 * scale && self.vel.abs() < 0.02 * scale
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn clamp01(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

/// 把 `x` 从 [a, b] 线性映射到 [0, 1] 并钳制。
pub fn remap(x: f32, a: f32, b: f32) -> f32 {
    // 允许 a > b（反向区间，用于“越靠近边缘越淡”这类渐隐）
    let d = b - a;
    if d.abs() < 1e-6 {
        return if x >= b { 1.0 } else { 0.0 };
    }
    clamp01((x - a) / d)
}

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = remap(x, a, b);
    t * t * (3.0 - 2.0 * t)
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - clamp01(t);
    1.0 - u * u * u
}

pub fn ease_in_out(t: f32) -> f32 {
    let t = clamp01(t);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// 帧率无关的指数逼近：`rate` 越大越快（约 1/rate 秒到 63%）。
pub fn approach(cur: f32, target: f32, rate: f32, dt: f32) -> f32 {
    cur + (target - cur) * (1.0 - (-rate * dt).exp())
}

/// 确定性的伪随机（0..1），给吉祥物眨眼/张望用，录屏可复现。
pub fn hash01(n: u64) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_supports_reversed_ranges() {
        assert_eq!(remap(200.0, 392.0, 358.0), 1.0);
        assert_eq!(remap(392.0, 392.0, 358.0), 0.0);
        assert!((smoothstep(392.0, 358.0, 375.0) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn underdamped_spring_overshoots_then_settles() {
        let mut s = Spring::new(0.0, 320.0, 0.7);
        s.target = 1.0;
        let mut peak: f32 = 0.0;
        for _ in 0..120 {
            s.step(1.0 / 60.0);
            peak = peak.max(s.pos);
        }
        assert!(peak > 1.02 && peak < 1.08, "peak {peak}");
        assert!(s.settled(1.0));
    }

    #[test]
    fn critically_damped_never_overshoots() {
        let mut s = Spring::new(1.0, 560.0, 1.0);
        s.target = 0.0;
        for _ in 0..60 {
            s.step(1.0 / 60.0);
            assert!(s.pos > -1e-4);
        }
    }

    #[test]
    fn frame_rate_independent() {
        let mut a = Spring::new(0.0, 300.0, 0.75);
        let mut b = a;
        a.target = 1.0;
        b.target = 1.0;
        for _ in 0..30 {
            a.step(1.0 / 30.0);
        }
        for _ in 0..144 {
            b.step(1.0 / 144.0);
        }
        assert!((a.pos - b.pos).abs() < 0.01);
    }
}
