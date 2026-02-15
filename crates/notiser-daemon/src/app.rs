use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use calloop::channel::{Channel, Sender};
use calloop::{EventLoop, LoopSignal};
use calloop_wayland_source::WaylandSource;
use tracing::{info, warn};

use crate::config::lua::load_config;
use crate::dbus;
use crate::dbus::bridge::DbusCommand;
use crate::notification::manager::NotificationManager;
use crate::wayland::compositor::{WaylandFields, init_wayland};
use crate::wayland::surface::CardRenderData;
use smithay_client_toolkit::shell::WaylandSurface;
use notiser_types::action::{DbusSignal, ServerInfo};
use notiser_types::config::Config;
use notiser_types::notification::{
    CloseReason, Notification, NotificationAction, NotificationHints, Urgency,
};
use notiser_render::layout::{LayoutRect, ResolvedElement, default_layout, resolve_layout};
use notiser_render::text::PreparedTextArea;

pub struct AppState {
    pub wayland: WaylandFields,
    pub manager: NotificationManager,
    pub config: Config,
    pub signal_tx: tokio::sync::mpsc::Sender<DbusSignal>,
    pub loop_signal: LoopSignal,
}

pub fn run() -> Result<()> {
    // Load Lua configuration
    let config = load_config().context("failed to load config")?;
    info!(
        width = config.appearance.width,
        timeout = config.general.default_timeout,
        "config loaded"
    );

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
    wayland_state.create_layer_surface(&qh, &config);

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
        config,
        signal_tx,
        loop_signal,
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
    let default_timeout = Duration::from_millis(state.config.general.default_timeout as u64);
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
                default_timeout
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
    let appearance = &state.config.appearance;
    let gap = state.config.display.gap;
    let card_height: u32 = appearance.padding.top + appearance.padding.bottom + 56;
    let surface_padding: u32 = 8;

    let width = appearance.width;
    let total_height = if count == 0 {
        0
    } else {
        surface_padding * 2 + count as u32 * card_height + (count as u32 - 1) * gap
    };

    if let Some(ref mut surface) = state.wayland.surface {
        if count == 0 {
            surface.layer().set_size(0, 0);
            surface.layer().commit();
        } else {
            surface.layer().set_size(width, total_height);
            surface.layer().commit();
            surface.resize(width, total_height);
        }
    }
}

fn render_notifications(state: &mut AppState) {
    let notifications: Vec<_> = state.manager.iter().collect();
    if notifications.is_empty() {
        return;
    }

    let config = &state.config;
    let appearance = &config.appearance;
    let gap = config.display.gap as f32;
    let surface_padding: f32 = 8.0;
    let card_pad = &appearance.padding;
    let card_height = card_pad.top as f32 + card_pad.bottom as f32 + 56.0;
    let card_width = appearance.width as f32;

    let bg_color = appearance.background.to_array();
    let border_color = appearance.border.color.to_array();
    let border_radius = appearance.border.radius;
    let border_width = appearance.border.width;

    let layout = config.layout.as_ref().cloned().unwrap_or_else(default_layout);

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

    let mut cards = Vec::new();
    let mut text_buffers = Vec::new();

    for (i, notification) in notifications.iter().enumerate() {
        let y = surface_padding + i as f32 * (card_height + gap);

        // Per-urgency background/border overrides
        let urgency = notification.urgency();
        let (card_bg, card_border) = if let Some(ov) = config.urgency.get(&urgency) {
            (
                ov.background.as_ref().map_or(bg_color, |c| c.to_array()),
                ov.border_color.as_ref().map_or(border_color, |c| c.to_array()),
            )
        } else {
            (bg_color, border_color)
        };

        cards.push(CardRenderData {
            rect: [0.0, y, card_width, card_height],
            background: card_bg,
            border_color: card_border,
            border_radius,
            border_width,
        });

        // Resolve layout for this notification
        let content_rect = LayoutRect {
            x: card_pad.left as f32,
            y: y + card_pad.top as f32,
            width: card_width - card_pad.left as f32 - card_pad.right as f32,
            height: card_height - card_pad.top as f32 - card_pad.bottom as f32,
        };

        let elements = resolve_layout(&layout, notification, appearance, content_rect);

        for element in elements {
            if let ResolvedElement::Text {
                rect,
                content,
                font_size,
                color,
                ..
            } = element
            {
                let glyphon_color = color
                    .as_ref()
                    .map(color_to_glyphon)
                    .unwrap_or_else(|| color_to_glyphon(&appearance.colors.body));

                let buf = gpu.text_engine.create_buffer(
                    &content,
                    font_size,
                    rect.width,
                );
                text_buffers.push((buf, rect, glyphon_color));
            }
        }
    }

    // Build text areas from buffers
    let text_areas: Vec<PreparedTextArea<'_>> = text_buffers
        .iter()
        .map(|(buf, rect, color)| PreparedTextArea {
            buffer: buf,
            left: rect.x,
            top: rect.y,
            scale: 1.0,
            color: *color,
        })
        .collect();

    if let Err(e) = surface.render_cards(&cards, &text_areas) {
        warn!("render error: {e}");
    }
}

fn color_to_glyphon(c: &notiser_types::config::Color) -> glyphon::Color {
    glyphon::Color::rgba(
        (c.r * 255.0) as u8,
        (c.g * 255.0) as u8,
        (c.b * 255.0) as u8,
        (c.a * 255.0) as u8,
    )
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
