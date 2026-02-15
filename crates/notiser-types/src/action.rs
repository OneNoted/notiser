use serde::{Deserialize, Serialize};

use crate::notification::CloseReason;

/// Owned hint value for cross-thread transfer.
#[derive(Debug, Clone)]
pub enum OwnedHintValue {
    Byte(u8),
    Int32(i32),
    String(String),
    Bool(bool),
    ImageData {
        width: i32,
        height: i32,
        rowstride: i32,
        has_alpha: bool,
        bits_per_sample: i32,
        channels: i32,
        data: Vec<u8>,
    },
}

/// Signals sent from the main thread back to the D-Bus thread for emission.
#[derive(Debug, Clone)]
pub enum DbusSignal {
    NotificationClosed { id: u32, reason: CloseReason },
    ActionInvoked { id: u32, action_key: String },
}

#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub spec_version: String,
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self {
            name: "notiser".into(),
            vendor: "notiser".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            spec_version: "1.2".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSummary {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub urgency: String,
    pub timestamp: u64,
}
