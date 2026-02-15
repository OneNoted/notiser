use tracing::info;
use zbus::{fdo, interface};

use super::bridge::DbusCommand;

/// Custom notiser daemon interface for control operations.
pub struct NotiserService {
    command_tx: tokio::sync::mpsc::Sender<DbusCommand>,
}

impl NotiserService {
    pub fn new(command_tx: tokio::sync::mpsc::Sender<DbusCommand>) -> Self {
        Self { command_tx }
    }
}

#[interface(name = "org.notiser.Daemon")]
impl NotiserService {
    /// List active notifications.
    async fn list_notifications(
        &self,
    ) -> fdo::Result<Vec<(u32, String, String, String, String, u64)>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(DbusCommand::ListNotifications { reply: tx })
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel send error: {e}")))?;

        let summaries = rx
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel recv error: {e}")))?;

        Ok(summaries
            .into_iter()
            .map(|s| (s.id, s.app_name, s.summary, s.body, s.urgency, s.timestamp))
            .collect())
    }

    /// Toggle Do Not Disturb mode. Returns the new DND state.
    async fn toggle_dnd(&self) -> fdo::Result<bool> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(DbusCommand::ToggleDnd { reply: tx })
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel send error: {e}")))?;

        rx.await
            .map_err(|e| fdo::Error::Failed(format!("channel recv error: {e}")))
    }

    /// Reload configuration.
    async fn reload(&self, hard: bool) -> fdo::Result<()> {
        info!(hard, "reload requested via D-Bus");
        self.command_tx
            .send(DbusCommand::Reload { hard })
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel send error: {e}")))?;
        Ok(())
    }

    /// Get notification history.
    async fn get_history(
        &self,
        limit: u32,
    ) -> fdo::Result<Vec<(u32, String, String, String, String, u64)>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(DbusCommand::GetHistory { limit, reply: tx })
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel send error: {e}")))?;

        let summaries = rx
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel recv error: {e}")))?;

        Ok(summaries
            .into_iter()
            .map(|s| (s.id, s.app_name, s.summary, s.body, s.urgency, s.timestamp))
            .collect())
    }

    /// Get daemon status: (dnd_active, active_count, history_count).
    async fn get_status(&self) -> fdo::Result<(bool, u32, u32)> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(DbusCommand::GetStatus { reply: tx })
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel send error: {e}")))?;

        rx.await
            .map_err(|e| fdo::Error::Failed(format!("channel recv error: {e}")))
    }

    /// Close all notifications.
    async fn close_all(&self) -> fdo::Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(DbusCommand::ListNotifications { reply: tx })
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel send error: {e}")))?;

        let summaries = rx
            .await
            .map_err(|e| fdo::Error::Failed(format!("channel recv error: {e}")))?;

        for s in summaries {
            let _ = self
                .command_tx
                .send(DbusCommand::CloseNotification { id: s.id })
                .await;
        }

        Ok(())
    }
}
