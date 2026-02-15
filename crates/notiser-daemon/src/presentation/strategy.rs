use std::time::Duration;

use notiser_types::config::Config;
use notiser_types::notification::Notification;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutputId(pub String);

#[derive(Debug, Clone)]
pub struct LayoutSlot {
    pub notification_id: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub opacity: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub border_radius: f32,
}

#[derive(Debug)]
pub struct SurfaceRequirements {
    pub output: OutputId,
    pub width: u32,
    pub height: u32,
    pub visible: bool,
}

#[derive(Debug)]
pub enum HitTestResult {
    Notification(u32),
    Action { notification_id: u32, action_key: String },
    DismissButton(u32),
}

pub trait PresentationStrategy {
    fn notification_added(
        &mut self,
        id: u32,
        notification: &Notification,
        config: &Config,
    ) -> Vec<SurfaceRequirements>;

    fn notification_removing(&mut self, id: u32) -> Vec<SurfaceRequirements>;

    fn notification_replaced(
        &mut self,
        old_id: u32,
        new_id: u32,
        notification: &Notification,
        config: &Config,
    ) -> Vec<SurfaceRequirements>;

    fn notification_removed(&mut self, id: u32) -> Vec<SurfaceRequirements>;

    fn tick(&mut self, dt: Duration) -> bool;

    fn layout_slots(&self, output: &OutputId) -> Vec<LayoutSlot>;

    fn active_outputs(&self) -> Vec<OutputId>;

    fn hit_test(&self, output: &OutputId, x: f32, y: f32) -> Option<HitTestResult>;

    fn clear_all(&mut self) -> Vec<SurfaceRequirements>;
}
