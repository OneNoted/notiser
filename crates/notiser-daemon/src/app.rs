use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use calloop::channel::{Channel, Sender};
use calloop::{EventLoop, LoopSignal};
use calloop_wayland_source::WaylandSource;
use tracing::{info, warn};

use crate::dbus;
use crate::dbus::bridge::DbusCommand;
use crate::notification::manager::NotificationManager;
use crate::wayland::compositor::{WaylandFields, init_wayland};
use smithay_client_toolkit::shell::WaylandSurface;
use notiser_types::action::{DbusSignal, ServerInfo};
use notiser_types::notification::{
    CloseReason, Notification, NotificationAction, NotificationHints, Urgency,
};
use notiser_render::text::PreparedTextArea;

pub struct AppState {
    pub wayland: WaylandFields,
    pub manager: NotificationManager,
    pub signal_tx: tokio::sync::mpsc::Sender<DbusSignal>,
    pub loop_signal: LoopSignal,
    pub default_timeout: Duration,
}

pub fn run() -> Result<()> {
    // Initialize Wayland
    let (conn, event_queue, globals) = init_wayland()
        .context("failed to connect to Wayland")?;

    // Create calloop event loop
    let mut event_loop: EventLoop<AppState> = EventLoop::try_new()
        .context("failed to create event loop")?;

    let loop_handle = event_loop.handle();
    let loop_signal = event_loop.get_signal();
    let qh = event_queue.handle();

    // Initialize Wayland state and create the layer surface
    let mut wayland_state = WaylandFields::new(&globals, &qh)
        .context("failed to initialize Wayland state")?;
    wayland_state.create_layer_surface(&qh);

    // Insert Wayland event source
    WaylandSource::new(conn, event_queue)
        .insert(loop_handle.clone())
        .map_err(|e| anyhow::anyhow!("failed to insert Wayland source: {e}"))?;

    // Set up D-Bus channel
    let (dbus_cmd_tx, dbus_cmd_rx) = tokio::sync::mpsc::channel::<DbusCommand>(256);
    let (_dbus_handle, signal_tx) = dbus::spawn_dbus_thread(dbus_cmd_tx)?;

    // Bridge tokio channel to calloop
    let (calloop_tx, calloop_rx): (Sender<DbusCommand>, Channel<DbusCommand>) =
        calloop::channel::channel();

    // Spawn a bridge thread that reads from tokio mpsc and writes to calloop channel
    let dbus_bridge_rx = dbus_cmd_rx;
    std::thread::Builder::new()
        .name("dbus-bridge".into())
        .spawn({
            let calloop_tx = calloop_tx.clone();
            move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build bridge runtime");
                rt.block_on(async move {
                    let mut rx = dbus_bridge_rx;
                    while let Some(cmd) = rx.recv().await {
                        if calloop_tx.send(cmd).is_err() {
                            break;
                        }
                    }
                });
            }
        })?;

    // Insert D-Bus command channel into calloop
    loop_handle
        .insert_source(calloop_rx, |event, _metadata, state: &mut AppState| {
            if let calloop::channel::Event::Msg(cmd) = event {
                handle_dbus_command(cmd, state);
            }
        })
        .map_err(|e| anyhow::anyhow!("failed to insert dbus channel source: {e}"))?;

    // Add a timer for checking notification timeouts (every 100ms)
    let timer = calloop::timer::Timer::from_duration(Duration::from_millis(100));
    loop_handle
        .insert_source(timer, |_deadline, _metadata, state: &mut AppState| {
            check_timeouts(state);
            // Re-arm: check every 100ms
            calloop::timer::TimeoutAction::ToDuration(Duration::from_millis(100))
        })
        .map_err(|e| anyhow::anyhow!("failed to insert timer source: {e}"))?;

    let mut state = AppState {
        wayland: wayland_state,
        manager: NotificationManager::new(),
        signal_tx,
        loop_signal,
        default_timeout: Duration::from_secs(5),
    };

    info!("main loop starting, waiting for notifications...");

    // Main event loop
    loop {
        if !state.wayland.running {
            break;
        }

        // Render if dirty
        if state.wayland.dirty && state.wayland.configured {
            state.wayland.dirty = false;
            render_notifications(&mut state);
        }

        event_loop
            .dispatch(Duration::from_millis(16), &mut state)
            .context("event loop dispatch failed")?;
    }

    Ok(())
}

fn handle_dbus_command(cmd: DbusCommand, state: &mut AppState) {
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
                urgency = ?notification.urgency(),
                "notification #{id}"
            );

            state.manager.add(notification);
            state.wayland.dirty = true;

            // Update surface size based on notification count
            update_surface_size(state);

            let _ = reply.send(id);
        }

        DbusCommand::CloseNotification { id } => {
            if state.manager.remove(id).is_some() {
                info!(id, "notification closed");
                update_surface_size(state);
                state.wayland.dirty = true;

                // Fire and forget the signal
                let _ = state.signal_tx.try_send(DbusSignal::NotificationClosed {
                    id,
                    reason: CloseReason::Closed,
                });
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
            let summaries = state
                .manager
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

        DbusCommand::Reload { .. } => {
            info!("reload requested (not yet implemented)");
        }

        DbusCommand::GetHistory { reply, .. } => {
            let _ = reply.send(Vec::new());
        }
    }
}

fn check_timeouts(state: &mut AppState) {
    let expired: Vec<u32> = state
        .manager
        .iter()
        .filter(|n| {
            let timeout = if n.expire_timeout > 0 {
                Duration::from_millis(n.expire_timeout as u64)
            } else if n.expire_timeout == 0 {
                // 0 means never expire
                return false;
            } else {
                state.default_timeout
            };
            n.created_at.elapsed() >= timeout
        })
        .map(|n| n.id)
        .collect();

    let had_expired = !expired.is_empty();
    for id in expired {
        if state.manager.remove(id).is_some() {
            info!(id, "notification expired");
            let _ = state.signal_tx.try_send(DbusSignal::NotificationClosed {
                id,
                reason: CloseReason::Expired,
            });
        }
    }

    if had_expired {
        update_surface_size(state);
        state.wayland.dirty = true;
    }
}

fn update_surface_size(state: &mut AppState) {
    let count = state.manager.active_count();
    let card_height: u32 = 80;
    let gap: u32 = 8;
    let padding: u32 = 8;

    let total_height = if count == 0 {
        0
    } else {
        padding * 2 + count as u32 * card_height + (count as u32 - 1) * gap
    };

    if let Some(ref mut surface) = state.wayland.surface {
        if count == 0 {
            // Hide surface
            surface.layer().set_size(0, 0);
            surface.layer().commit();
        } else {
            surface.layer().set_size(400, total_height);
            surface.layer().commit();
            surface.resize(400, total_height);
        }
    }
}

fn render_notifications(state: &mut AppState) {
    let notifications: Vec<_> = state.manager.iter().collect();
    if notifications.is_empty() {
        return;
    }

    let surface = match state.wayland.surface.as_mut() {
        Some(s) => s,
        None => return,
    };

    let gpu = match surface.gpu_mut() {
        Some(g) => g,
        None => return,
    };

    let width = gpu.config.width;
    let height = gpu.config.height;
    if width == 0 || height == 0 {
        return;
    }

    // Create text buffers for all notifications
    let card_width = width as f32 - 32.0; // 16px padding each side
    let mut buffers = Vec::new();

    for notification in &notifications {
        let summary_buf = gpu.text_engine.create_buffer(
            &notification.summary,
            15.0,
            card_width,
        );
        let body_buf = gpu.text_engine.create_buffer(
            &notification.body,
            13.0,
            card_width,
        );
        buffers.push((summary_buf, body_buf));
    }

    // Create text areas
    let mut text_areas = Vec::new();
    let white = glyphon::Color::rgb(205, 214, 244); // #cdd6f4
    let light = glyphon::Color::rgb(186, 194, 222); // #bac2de

    let card_height: f32 = 80.0;
    let gap: f32 = 8.0;
    let padding: f32 = 8.0;

    for (i, (summary_buf, body_buf)) in buffers.iter().enumerate() {
        let y_offset = padding + i as f32 * (card_height + gap);

        text_areas.push(PreparedTextArea {
            buffer: summary_buf,
            left: 16.0,
            top: y_offset + 12.0,
            scale: 1.0,
            color: white,
        });

        text_areas.push(PreparedTextArea {
            buffer: body_buf,
            left: 16.0,
            top: y_offset + 34.0,
            scale: 1.0,
            color: light,
        });
    }

    // Render
    if let Err(e) = surface.render_with_text(
        [0.118, 0.118, 0.180, 1.0], // #1e1e2e
        &text_areas,
    ) {
        warn!("render error: {e}");
    }
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
