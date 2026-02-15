use anyhow::{Context, Result};
use zbus::Connection;

use crate::output::OutputFormat;

/// Connect to the notiser daemon via D-Bus.
async fn connect() -> Result<Connection> {
    Connection::session()
        .await
        .context("failed to connect to session bus")
}

/// D-Bus proxy for org.notiser.Daemon interface.
#[zbus::proxy(
    interface = "org.notiser.Daemon",
    default_service = "org.notiser.Daemon",
    default_path = "/org/notiser/Daemon"
)]
trait NotiserDaemon {
    async fn list_notifications(
        &self,
    ) -> zbus::Result<Vec<(u32, String, String, String, String, u64)>>;

    async fn toggle_dnd(&self) -> zbus::Result<()>;

    async fn reload(&self, hard: bool) -> zbus::Result<()>;

    async fn get_history(
        &self,
        limit: u32,
    ) -> zbus::Result<Vec<(u32, String, String, String, String, u64)>>;

    async fn close_all(&self) -> zbus::Result<()>;
}

/// D-Bus proxy for org.freedesktop.Notifications (for close).
#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    async fn close_notification(&self, id: u32) -> zbus::Result<()>;
}

pub async fn list(format: &OutputFormat) -> Result<()> {
    let conn = connect().await?;
    let proxy = NotiserDaemonProxy::new(&conn).await?;
    let notifications = proxy.list_notifications().await?;

    if notifications.is_empty() {
        println!("No active notifications.");
        return Ok(());
    }

    match format {
        OutputFormat::Json => {
            let items: Vec<serde_json::Value> = notifications
                .iter()
                .map(|(id, app, summary, body, urgency, age)| {
                    serde_json::json!({
                        "id": id,
                        "app_name": app,
                        "summary": summary,
                        "body": body,
                        "urgency": urgency,
                        "age_seconds": age,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&items)?);
        }
        OutputFormat::Text => {
            for (id, app, summary, _body, urgency, age) in &notifications {
                println!("#{id} [{urgency}] {app}: {summary} ({age}s ago)");
            }
        }
    }

    Ok(())
}

pub async fn close(id: u32) -> Result<()> {
    let conn = connect().await?;
    let proxy = NotificationsProxy::new(&conn).await?;
    proxy.close_notification(id).await?;
    println!("Closed notification #{id}");
    Ok(())
}

pub async fn close_all() -> Result<()> {
    let conn = connect().await?;
    let proxy = NotiserDaemonProxy::new(&conn).await?;
    proxy.close_all().await?;
    println!("Closed all notifications");
    Ok(())
}

pub async fn dnd() -> Result<()> {
    let conn = connect().await?;
    let proxy = NotiserDaemonProxy::new(&conn).await?;
    proxy.toggle_dnd().await?;
    println!("Toggled Do Not Disturb");
    Ok(())
}

pub async fn reload(hard: bool) -> Result<()> {
    let conn = connect().await?;
    let proxy = NotiserDaemonProxy::new(&conn).await?;
    proxy.reload(hard).await?;
    println!("Configuration reloaded{}", if hard { " (hard)" } else { "" });
    Ok(())
}

pub async fn history(limit: u32, format: &OutputFormat) -> Result<()> {
    let conn = connect().await?;
    let proxy = NotiserDaemonProxy::new(&conn).await?;
    let entries = proxy.get_history(limit).await?;

    if entries.is_empty() {
        println!("No history entries.");
        return Ok(());
    }

    match format {
        OutputFormat::Json => {
            let items: Vec<serde_json::Value> = entries
                .iter()
                .map(|(id, app, summary, body, urgency, age)| {
                    serde_json::json!({
                        "id": id,
                        "app_name": app,
                        "summary": summary,
                        "body": body,
                        "urgency": urgency,
                        "age_seconds": age,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&items)?);
        }
        OutputFormat::Text => {
            for (id, app, summary, _body, urgency, age) in &entries {
                println!("#{id} [{urgency}] {app}: {summary} ({age}s ago)");
            }
        }
    }

    Ok(())
}

pub async fn inspect() -> Result<()> {
    let conn = connect().await?;
    let proxy = NotiserDaemonProxy::new(&conn).await?;
    let notifications = proxy.list_notifications().await?;
    println!("Active notifications: {}", notifications.len());
    for (id, app, summary, body, urgency, age) in &notifications {
        println!("  #{id} [{urgency}] {app}: {summary}");
        if !body.is_empty() {
            println!("    {body}");
        }
        println!("    Age: {age}s");
    }
    Ok(())
}
