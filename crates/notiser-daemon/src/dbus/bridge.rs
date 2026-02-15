use notiser_types::action::{NotificationSummary, OwnedHintValue, ServerInfo};

/// Commands sent from D-Bus thread to the main calloop thread.
#[derive(Debug)]
pub enum DbusCommand {
    Notify {
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: Vec<(String, OwnedHintValue)>,
        expire_timeout: i32,
        reply: tokio::sync::oneshot::Sender<u32>,
    },
    CloseNotification {
        id: u32,
    },
    GetCapabilities {
        reply: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    GetServerInformation {
        reply: tokio::sync::oneshot::Sender<ServerInfo>,
    },
    ListNotifications {
        reply: tokio::sync::oneshot::Sender<Vec<NotificationSummary>>,
    },
    ToggleDnd {
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    Reload {
        hard: bool,
    },
    GetHistory {
        limit: u32,
        reply: tokio::sync::oneshot::Sender<Vec<NotificationSummary>>,
    },
    GetStatus {
        reply: tokio::sync::oneshot::Sender<(bool, u32, u32)>, // (dnd_active, active_count, history_count)
    },
}
