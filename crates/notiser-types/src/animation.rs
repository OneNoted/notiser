use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AnimationPreset {
    None,
    Standard,
    Dynamic,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BezierCurve {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl BezierCurve {
    pub const EASE_OUT: Self = Self {
        x1: 0.0,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    };

    pub const EASE_IN: Self = Self {
        x1: 0.42,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    };

    pub const EASE_IN_OUT: Self = Self {
        x1: 0.42,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    };

    pub const SPRING: Self = Self {
        x1: 0.175,
        y1: 0.885,
        x2: 0.32,
        y2: 1.275,
    };

    pub const SPRING_HEAVY: Self = Self {
        x1: 0.2,
        y1: 0.9,
        x2: 0.25,
        y2: 1.35,
    };

    pub fn compute_lut(&self) -> BezierLut {
        let mut samples = [0.0f32; LUT_SIZE];
        for i in 0..LUT_SIZE {
            let t = i as f64 / (LUT_SIZE - 1) as f64;
            samples[i] = self.sample_y_at_x(t) as f32;
        }
        BezierLut { samples }
    }

    fn sample_y_at_x(&self, x: f64) -> f64 {
        // Newton's method to find t for given x
        let mut t = x;
        for _ in 0..8 {
            let x_t = self.bezier_x(t);
            let dx = x_t - x;
            if dx.abs() < 1e-7 {
                break;
            }
            let dx_dt = self.bezier_dx(t);
            if dx_dt.abs() < 1e-7 {
                break;
            }
            t -= dx / dx_dt;
        }
        t = t.clamp(0.0, 1.0);
        self.bezier_y(t)
    }

    fn bezier_x(&self, t: f64) -> f64 {
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        3.0 * mt2 * t * self.x1 + 3.0 * mt * t2 * self.x2 + t3
    }

    fn bezier_y(&self, t: f64) -> f64 {
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        3.0 * mt2 * t * self.y1 + 3.0 * mt * t2 * self.y2 + t3
    }

    fn bezier_dx(&self, t: f64) -> f64 {
        let mt = 1.0 - t;
        3.0 * mt * mt * self.x1 + 6.0 * mt * t * (self.x2 - self.x1) + 3.0 * t * t * (1.0 - self.x2)
    }
}

const LUT_SIZE: usize = 256;

#[derive(Debug, Clone)]
pub struct BezierLut {
    pub samples: [f32; LUT_SIZE],
}

impl BezierLut {
    pub fn sample(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let idx = t * (LUT_SIZE - 1) as f32;
        let lo = idx.floor() as usize;
        let hi = (lo + 1).min(LUT_SIZE - 1);
        let frac = idx - lo as f32;
        self.samples[lo] * (1.0 - frac) + self.samples[hi] * frac
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimationState {
    Entering,
    Visible,
    Exiting,
    Dismissed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransitionKind {
    SlideDown,
    SlideUp,
    SlideLeft,
    SlideRight,
    FadeIn,
    FadeOut,
    Scale,
    Grow,
    Shrink,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionConfig {
    pub kind: TransitionKind,
    pub duration_ms: u32,
    pub curve: BezierCurve,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimatableProperties {
    pub opacity: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub width: f32,
    pub height: f32,
    pub border_radius: f32,
}

impl Default for AnimatableProperties {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            width: 0.0,
            height: 0.0,
            border_radius: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_lut_endpoints() {
        let curve = BezierCurve::EASE_OUT;
        let lut = curve.compute_lut();
        assert!((lut.sample(0.0)).abs() < 0.01);
        assert!((lut.sample(1.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn bezier_lut_monotonic_for_standard_curves() {
        let curve = BezierCurve::EASE_IN_OUT;
        let lut = curve.compute_lut();
        for i in 1..LUT_SIZE {
            assert!(lut.samples[i] >= lut.samples[i - 1] - 0.001);
        }
    }
}
