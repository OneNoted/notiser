use std::collections::HashMap;

use tracing::{debug, info};
use zbus::{fdo, interface};

use super::bridge::DbusCommand;
use notiser_types::action::OwnedHintValue;

pub struct NotificationService {
    command_tx: tokio::sync::mpsc::Sender<DbusCommand>,
}

impl NotificationService {
    pub fn new(command_tx: tokio::sync::mpsc::Sender<DbusCommand>) -> Self {
        Self { command_tx }
    }
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationService {
    async fn get_capabilities(&self) -> fdo::Result<Vec<String>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(DbusCommand::GetCapabilities { reply: tx })
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel send error: {e}")))?;
        rx.await
            .map_err(|e| fdo::Error::Failed(format!("channel recv error: {e}")))
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<String>,
        hints: HashMap<String, zbus::zvariant::OwnedValue>,
        expire_timeout: i32,
    ) -> fdo::Result<u32> {
        info!(app = app_name, summary, "notification received");

        let converted_hints = convert_hints(hints);

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(DbusCommand::Notify {
                app_name: app_name.to_string(),
                replaces_id,
                app_icon: app_icon.to_string(),
                summary: summary.to_string(),
                body: body.to_string(),
                actions,
                hints: converted_hints,
                expire_timeout,
                reply: tx,
            })
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel send error: {e}")))?;

        rx.await
            .map_err(|e| fdo::Error::Failed(format!("channel recv error: {e}")))
    }

    async fn close_notification(&self, id: u32) -> fdo::Result<()> {
        debug!(id, "close notification requested");
        self.command_tx
            .send(DbusCommand::CloseNotification { id })
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel send error: {e}")))?;
        Ok(())
    }

    async fn get_server_information(&self) -> fdo::Result<(String, String, String, String)> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(DbusCommand::GetServerInformation { reply: tx })
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel send error: {e}")))?;

        let info = rx
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel recv error: {e}")))?;

        Ok((info.name, info.vendor, info.version, info.spec_version))
    }

    // Signals are emitted via conn.emit_signal() in the dbus mod, not here
}

fn convert_hints(
    hints: HashMap<String, zbus::zvariant::OwnedValue>,
) -> Vec<(String, OwnedHintValue)> {
    let mut result = Vec::new();
    for (key, value) in hints {
        if let Some(hint) = convert_hint_value(&key, &value) {
            result.push((key, hint));
        }
    }
    result
}

fn convert_hint_value(key: &str, value: &zbus::zvariant::OwnedValue) -> Option<OwnedHintValue> {
    // Try specific types first
    if let Ok(b) = value.downcast_ref::<u8>() {
        return Some(OwnedHintValue::Byte(b));
    }
    if let Ok(i) = value.downcast_ref::<i32>() {
        return Some(OwnedHintValue::Int32(i));
    }
    if let Ok(b) = value.downcast_ref::<bool>() {
        return Some(OwnedHintValue::Bool(b));
    }
    if let Ok(s) = value.downcast_ref::<zbus::zvariant::Str<'_>>() {
        return Some(OwnedHintValue::String(s.to_string()));
    }
    if let Ok(s) = value.downcast_ref::<String>() {
        return Some(OwnedHintValue::String(s));
    }

    debug!(key, "unhandled hint type");
    None
}
