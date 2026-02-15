use std::time::Duration;

use notiser_types::animation::{AnimatableProperties, AnimationState, BezierLut};

pub struct AnimationInstance {
    pub state: AnimationState,
    pub elapsed: Duration,
    pub duration: Duration,
    pub lut: BezierLut,
    pub from: AnimatableProperties,
    pub to: AnimatableProperties,
}

impl AnimationInstance {
    pub fn progress(&self) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        let t = self.elapsed.as_secs_f32() / self.duration.as_secs_f32();
        self.lut.sample(t.clamp(0.0, 1.0))
    }

    pub fn tick(&mut self, dt: Duration) -> bool {
        self.elapsed += dt;
        self.elapsed < self.duration
    }

    pub fn current(&self) -> AnimatableProperties {
        let p = self.progress();
        AnimatableProperties {
            opacity: lerp(self.from.opacity, self.to.opacity, p),
            offset_x: lerp(self.from.offset_x, self.to.offset_x, p),
            offset_y: lerp(self.from.offset_y, self.to.offset_y, p),
            scale_x: lerp(self.from.scale_x, self.to.scale_x, p),
            scale_y: lerp(self.from.scale_y, self.to.scale_y, p),
            width: lerp(self.from.width, self.to.width, p),
            height: lerp(self.from.height, self.to.height, p),
            border_radius: lerp(self.from.border_radius, self.to.border_radius, p),
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
