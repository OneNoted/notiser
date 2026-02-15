pub mod bridge;
pub mod freedesktop;
pub mod notiser;

use anyhow::Result;
use tracing::{info, warn};

use self::bridge::DbusCommand;
use self::freedesktop::NotificationService;
use notiser_types::action::DbusSignal;

pub fn spawn_dbus_thread(
    command_tx: tokio::sync::mpsc::Sender<DbusCommand>,
) -> Result<(std::thread::JoinHandle<()>, tokio::sync::mpsc::Sender<DbusSignal>)> {
    let (signal_tx, mut signal_rx) = tokio::sync::mpsc::channel::<DbusSignal>(64);

    let handle = std::thread::Builder::new()
        .name("dbus".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for dbus thread");

            rt.block_on(async move {
                let service = NotificationService::new(command_tx);

                let conn = zbus::connection::Builder::session()
                    .expect("failed to create session bus builder")
                    .name("org.freedesktop.Notifications")
                    .expect("failed to request bus name")
                    .serve_at("/org/freedesktop/Notifications", service)
                    .expect("failed to serve notification interface")
                    .build()
                    .await
                    .expect("failed to build D-Bus connection");

                info!("D-Bus service registered on org.freedesktop.Notifications");

                // For signal emission, use the connection directly with low-level API
                loop {
                    match signal_rx.recv().await {
                        Some(DbusSignal::NotificationClosed { id, reason }) => {
                            if let Err(e) = conn
                                .emit_signal(
                                    None::<&str>,
                                    "/org/freedesktop/Notifications",
                                    "org.freedesktop.Notifications",
                                    "NotificationClosed",
                                    &(id, reason as u32),
                                )
                                .await
                            {
                                warn!("failed to emit NotificationClosed signal: {e}");
                            }
                        }
                        Some(DbusSignal::ActionInvoked { id, action_key }) => {
                            if let Err(e) = conn
                                .emit_signal(
                                    None::<&str>,
                                    "/org/freedesktop/Notifications",
                                    "org.freedesktop.Notifications",
                                    "ActionInvoked",
                                    &(id, action_key.as_str()),
                                )
                                .await
                            {
                                warn!("failed to emit ActionInvoked signal: {e}");
                            }
                        }
                        None => break,
                    }
                }
            });
        })?;

    Ok((handle, signal_tx))
}
