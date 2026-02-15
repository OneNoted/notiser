use std::collections::HashMap;
use std::time::Duration;

use notiser_types::animation::{
    AnimatableProperties, AnimationPreset, AnimationState, BezierCurve, BezierLut, TransitionKind,
};

/// Per-notification animation tracking.
pub struct NotificationAnim {
    pub state: AnimationState,
    elapsed: Duration,
    duration: Duration,
    lut: BezierLut,
    transition: TransitionKind,
    /// Offset distance for slide transitions (pixels).
    slide_distance: f32,
}

impl NotificationAnim {
    fn new(transition: TransitionKind, duration: Duration, curve: &BezierCurve) -> Self {
        Self {
            state: AnimationState::Entering,
            elapsed: Duration::ZERO,
            duration,
            lut: curve.compute_lut(),
            transition,
            slide_distance: 40.0,
        }
    }

    /// Raw 0→1 progress through the easing curve.
    fn progress(&self) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        let t = self.elapsed.as_secs_f32() / self.duration.as_secs_f32();
        self.lut.sample(t.clamp(0.0, 1.0))
    }

    /// Compute current animatable properties.
    pub fn current(&self) -> AnimatableProperties {
        let p = self.progress();
        match self.state {
            AnimationState::Entering => self.enter_props(p),
            AnimationState::Exiting => self.exit_props(p),
            AnimationState::Visible | AnimationState::Dismissed => AnimatableProperties::default(),
        }
    }

    fn enter_props(&self, p: f32) -> AnimatableProperties {
        match self.transition {
            TransitionKind::SlideDown => AnimatableProperties {
                opacity: p,
                offset_y: -self.slide_distance * (1.0 - p),
                ..Default::default()
            },
            TransitionKind::SlideUp => AnimatableProperties {
                opacity: p,
                offset_y: self.slide_distance * (1.0 - p),
                ..Default::default()
            },
            TransitionKind::SlideLeft => AnimatableProperties {
                opacity: p,
                offset_x: self.slide_distance * (1.0 - p),
                ..Default::default()
            },
            TransitionKind::SlideRight => AnimatableProperties {
                opacity: p,
                offset_x: -self.slide_distance * (1.0 - p),
                ..Default::default()
            },
            TransitionKind::FadeIn => AnimatableProperties {
                opacity: p,
                ..Default::default()
            },
            TransitionKind::Scale => AnimatableProperties {
                opacity: p,
                scale_x: 0.8 + 0.2 * p,
                scale_y: 0.8 + 0.2 * p,
                ..Default::default()
            },
            _ => AnimatableProperties {
                opacity: p,
                ..Default::default()
            },
        }
    }

    fn exit_props(&self, p: f32) -> AnimatableProperties {
        // p goes 0→1 as exit progresses; invert for "fading out"
        let inv = 1.0 - p;
        match self.transition {
            TransitionKind::SlideDown | TransitionKind::SlideUp => AnimatableProperties {
                opacity: inv,
                offset_y: -self.slide_distance * p,
                ..Default::default()
            },
            TransitionKind::SlideLeft | TransitionKind::SlideRight => AnimatableProperties {
                opacity: inv,
                offset_x: -self.slide_distance * p,
                ..Default::default()
            },
            TransitionKind::FadeOut | TransitionKind::FadeIn => AnimatableProperties {
                opacity: inv,
                ..Default::default()
            },
            TransitionKind::Shrink | TransitionKind::Scale => AnimatableProperties {
                opacity: inv,
                scale_x: 1.0 - 0.2 * p,
                scale_y: 1.0 - 0.2 * p,
                ..Default::default()
            },
            _ => AnimatableProperties {
                opacity: inv,
                ..Default::default()
            },
        }
    }

    fn is_finished(&self) -> bool {
        self.elapsed >= self.duration
    }
}

/// Manages animation state for all active notifications.
pub struct AnimationController {
    anims: HashMap<u32, NotificationAnim>,
    enter_transition: TransitionKind,
    exit_transition: TransitionKind,
    enter_duration: Duration,
    exit_duration: Duration,
    enter_curve: BezierCurve,
    exit_curve: BezierCurve,
    enabled: bool,
}

impl AnimationController {
    pub fn from_preset(preset: AnimationPreset) -> Self {
        match preset {
            AnimationPreset::None => Self {
                anims: HashMap::new(),
                enter_transition: TransitionKind::FadeIn,
                exit_transition: TransitionKind::FadeOut,
                enter_duration: Duration::ZERO,
                exit_duration: Duration::ZERO,
                enter_curve: BezierCurve::EASE_OUT,
                exit_curve: BezierCurve::EASE_IN,
                enabled: false,
            },
            AnimationPreset::Standard => Self {
                anims: HashMap::new(),
                enter_transition: TransitionKind::SlideDown,
                exit_transition: TransitionKind::SlideUp,
                enter_duration: Duration::from_millis(250),
                exit_duration: Duration::from_millis(200),
                enter_curve: BezierCurve::EASE_OUT,
                exit_curve: BezierCurve::EASE_IN,
                enabled: true,
            },
            AnimationPreset::Dynamic => Self {
                anims: HashMap::new(),
                enter_transition: TransitionKind::Scale,
                exit_transition: TransitionKind::Shrink,
                enter_duration: Duration::from_millis(350),
                exit_duration: Duration::from_millis(250),
                enter_curve: BezierCurve::SPRING,
                exit_curve: BezierCurve::EASE_IN_OUT,
                enabled: true,
            },
            AnimationPreset::Custom => Self {
                anims: HashMap::new(),
                enter_transition: TransitionKind::SlideDown,
                exit_transition: TransitionKind::FadeOut,
                enter_duration: Duration::from_millis(300),
                exit_duration: Duration::from_millis(200),
                enter_curve: BezierCurve::EASE_OUT,
                exit_curve: BezierCurve::EASE_IN,
                enabled: true,
            },
        }
    }

    /// Start enter animation for a notification.
    pub fn on_enter(&mut self, id: u32) {
        if !self.enabled {
            return;
        }
        let anim =
            NotificationAnim::new(self.enter_transition, self.enter_duration, &self.enter_curve);
        self.anims.insert(id, anim);
    }

    /// Start exit animation for a notification. Returns true if an exit animation
    /// was started (meaning the caller should NOT remove the notification yet).
    pub fn on_exit(&mut self, id: u32) -> bool {
        if !self.enabled {
            self.anims.remove(&id);
            return false;
        }
        let mut anim =
            NotificationAnim::new(self.exit_transition, self.exit_duration, &self.exit_curve);
        anim.state = AnimationState::Exiting;
        self.anims.insert(id, anim);
        true
    }

    /// Tick all animations by `dt`. Returns IDs of notifications whose exit
    /// animation has completed (ready for removal).
    pub fn tick(&mut self, dt: Duration) -> Vec<u32> {
        let mut finished = Vec::new();
        for (id, anim) in &mut self.anims {
            anim.elapsed += dt;
            if anim.is_finished() {
                match anim.state {
                    AnimationState::Entering => {
                        anim.state = AnimationState::Visible;
                    }
                    AnimationState::Exiting => {
                        anim.state = AnimationState::Dismissed;
                        finished.push(*id);
                    }
                    _ => {}
                }
            }
        }
        for id in &finished {
            self.anims.remove(id);
        }
        finished
    }

    /// Get current animatable properties for a notification.
    pub fn props(&self, id: u32) -> AnimatableProperties {
        self.anims
            .get(&id)
            .map(|a| a.current())
            .unwrap_or_default()
    }

    /// Whether any animations are currently running.
    pub fn has_active(&self) -> bool {
        self.anims
            .values()
            .any(|a| matches!(a.state, AnimationState::Entering | AnimationState::Exiting))
    }

    /// Whether a notification is currently in its exit animation.
    pub fn is_exiting(&self, id: u32) -> bool {
        self.anims
            .get(&id)
            .is_some_and(|a| a.state == AnimationState::Exiting)
    }

    /// Remove tracking for a notification (no animation).
    pub fn remove(&mut self, id: u32) {
        self.anims.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_preset_enter_exit() {
        let mut ctrl = AnimationController::from_preset(AnimationPreset::Standard);

        ctrl.on_enter(1);
        assert!(ctrl.has_active());

        // Partially tick
        let finished = ctrl.tick(Duration::from_millis(100));
        assert!(finished.is_empty());
        let props = ctrl.props(1);
        assert!(props.opacity > 0.0 && props.opacity < 1.0);

        // Complete enter
        let finished = ctrl.tick(Duration::from_millis(200));
        assert!(finished.is_empty());
        let props = ctrl.props(1);
        assert!((props.opacity - 1.0).abs() < f32::EPSILON);

        // Start exit
        assert!(ctrl.on_exit(1));
        assert!(ctrl.has_active());

        // Complete exit
        let finished = ctrl.tick(Duration::from_millis(250));
        assert_eq!(finished, vec![1]);
        assert!(!ctrl.has_active());
    }

    #[test]
    fn no_animation_preset() {
        let mut ctrl = AnimationController::from_preset(AnimationPreset::None);

        ctrl.on_enter(1);
        assert!(!ctrl.has_active());

        // on_exit returns false (no animation, remove immediately)
        assert!(!ctrl.on_exit(1));
    }

    #[test]
    fn props_default_when_no_anim() {
        let ctrl = AnimationController::from_preset(AnimationPreset::Standard);
        let props = ctrl.props(999);
        assert!((props.opacity - 1.0).abs() < f32::EPSILON);
        assert!(props.offset_y.abs() < f32::EPSILON);
    }
}
