use std::collections::HashMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

impl Urgency {
    pub fn from_byte(value: u8) -> Self {
        match value {
            0 => Self::Low,
            2 => Self::Critical,
            _ => Self::Normal,
        }
    }
}

impl Default for Urgency {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    Closed = 3,
    Unknown = 4,
}

#[derive(Debug, Clone)]
pub struct ImageData {
    pub width: i32,
    pub height: i32,
    pub rowstride: i32,
    pub has_alpha: bool,
    pub bits_per_sample: i32,
    pub channels: i32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct NotificationHints {
    pub urgency: Option<Urgency>,
    pub category: Option<String>,
    pub desktop_entry: Option<String>,
    pub image_data: Option<ImageData>,
    pub image_path: Option<String>,
    pub icon_data: Option<ImageData>,
    pub sound_file: Option<String>,
    pub sound_name: Option<String>,
    pub suppress_sound: bool,
    pub transient: bool,
    pub resident: bool,
    pub action_icons: bool,
    pub value: Option<i32>,
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<NotificationAction>,
    pub hints: NotificationHints,
    pub expire_timeout: i32,
    pub created_at: Instant,
    pub replaces_id: Option<u32>,
}

impl Notification {
    pub fn urgency(&self) -> Urgency {
        self.hints.urgency.unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct NotificationAction {
    pub key: String,
    pub label: String,
}
