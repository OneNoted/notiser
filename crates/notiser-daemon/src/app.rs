use std::time::Instant;

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::dbus;
use crate::dbus::bridge::DbusCommand;
use crate::notification::manager::NotificationManager;
use notiser_types::action::{DbusSignal, ServerInfo};
use notiser_types::notification::{
    CloseReason, Notification, NotificationAction, NotificationHints, Urgency,
};

pub fn run() -> Result<()> {
    let (command_tx, mut command_rx) = tokio::sync::mpsc::channel::<DbusCommand>(256);

    let (_dbus_handle, signal_tx) = dbus::spawn_dbus_thread(command_tx)?;

    info!("main loop starting, waiting for notifications...");

    let mut manager = NotificationManager::new();

    // For Phase 1a we use a simple blocking loop on the tokio channel.
    // Phase 1d will replace this with calloop.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        loop {
            let Some(cmd) = command_rx.recv().await else {
                info!("D-Bus channel closed, shutting down");
                break;
            };

            match cmd {
                DbusCommand::Notify {
                    app_name,
                    replaces_id,
                    app_icon,
                    summary,
                    body,
                    actions,
                    hints,
                    expire_timeout,
                    reply,
                } => {
                    let id = NotificationManager::allocate_id();
                    let parsed_hints = parse_hints(&hints);
                    let parsed_actions = parse_actions(&actions);

                    let notification = Notification {
                        id,
                        app_name: app_name.clone(),
                        app_icon,
                        summary: summary.clone(),
                        body: body.clone(),
                        actions: parsed_actions,
                        hints: parsed_hints,
                        expire_timeout,
                        created_at: Instant::now(),
                        replaces_id: if replaces_id > 0 {
                            Some(replaces_id)
                        } else {
                            None
                        },
                    };

                    info!(
                        id,
                        app = app_name,
                        summary,
                        body,
                        urgency = ?notification.urgency(),
                        "notification #{id}"
                    );

                    manager.add(notification);
                    let _ = reply.send(id);
                }

                DbusCommand::CloseNotification { id } => {
                    if manager.remove(id).is_some() {
                        info!(id, "notification closed");
                        let _ = signal_tx
                            .send(DbusSignal::NotificationClosed {
                                id,
                                reason: CloseReason::Closed,
                            })
                            .await;
                    }
                }

                DbusCommand::GetCapabilities { reply } => {
                    let _ = reply.send(vec![
                        "body".into(),
                        "body-markup".into(),
                        "body-hyperlinks".into(),
                        "actions".into(),
                        "icon-static".into(),
                        "persistence".into(),
                    ]);
                }

                DbusCommand::GetServerInformation { reply } => {
                    let _ = reply.send(ServerInfo::default());
                }

                DbusCommand::ListNotifications { reply } => {
                    let summaries = manager
                        .iter()
                        .map(|n| notiser_types::action::NotificationSummary {
                            id: n.id,
                            app_name: n.app_name.clone(),
                            summary: n.summary.clone(),
                            body: n.body.clone(),
                            urgency: format!("{:?}", n.urgency()),
                            timestamp: n.created_at.elapsed().as_secs(),
                        })
                        .collect();
                    let _ = reply.send(summaries);
                }

                DbusCommand::ToggleDnd => {
                    info!("DND toggled (not yet implemented)");
                }

                DbusCommand::Reload { hard } => {
                    info!(hard, "reload requested (not yet implemented)");
                }

                DbusCommand::GetHistory { limit, reply } => {
                    let _ = reply.send(Vec::new());
                }
            }
        }
    });

    Ok(())
}

fn parse_hints(
    hints: &[(String, notiser_types::action::OwnedHintValue)],
) -> NotificationHints {
    use notiser_types::action::OwnedHintValue;

    let mut result = NotificationHints::default();

    for (key, value) in hints {
        match (key.as_str(), value) {
            ("urgency", OwnedHintValue::Byte(b)) => {
                result.urgency = Some(Urgency::from_byte(*b));
            }
            ("category", OwnedHintValue::String(s)) => {
                result.category = Some(s.clone());
            }
            ("desktop-entry", OwnedHintValue::String(s)) => {
                result.desktop_entry = Some(s.clone());
            }
            ("image-path", OwnedHintValue::String(s)) => {
                result.image_path = Some(s.clone());
            }
            ("sound-file", OwnedHintValue::String(s)) => {
                result.sound_file = Some(s.clone());
            }
            ("sound-name", OwnedHintValue::String(s)) => {
                result.sound_name = Some(s.clone());
            }
            ("suppress-sound", OwnedHintValue::Bool(b)) => {
                result.suppress_sound = *b;
            }
            ("transient", OwnedHintValue::Bool(b)) => {
                result.transient = *b;
            }
            ("resident", OwnedHintValue::Bool(b)) => {
                result.resident = *b;
            }
            ("action-icons", OwnedHintValue::Bool(b)) => {
                result.action_icons = *b;
            }
            ("value", OwnedHintValue::Int32(v)) => {
                result.value = Some(*v);
            }
            (k, OwnedHintValue::String(s)) => {
                result.extra.insert(k.to_string(), s.clone());
            }
            _ => {}
        }
    }

    result
}

fn parse_actions(actions: &[String]) -> Vec<NotificationAction> {
    actions
        .chunks(2)
        .filter_map(|chunk| {
            if chunk.len() == 2 {
                Some(NotificationAction {
                    key: chunk[0].clone(),
                    label: chunk[1].clone(),
                })
            } else {
                None
            }
        })
        .collect()
}
