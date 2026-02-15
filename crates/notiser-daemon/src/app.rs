use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use calloop::channel::{Channel, Sender};
use calloop::{EventLoop, LoopSignal};
use calloop_wayland_source::WaylandSource;
use tracing::{info, warn};

use crate::animation::AnimationController;
use crate::config::lua::load_config;
use crate::config::watcher::{ConfigReloadEvent, watch_config};
use crate::dbus;
use crate::dbus::bridge::DbusCommand;
use crate::notification::history::NotificationHistory;
use crate::notification::manager::NotificationManager;
use crate::wayland::compositor::{WaylandFields, init_wayland};
use crate::wayland::surface::{CardRenderData, IconRenderData};
use smithay_client_toolkit::shell::WaylandSurface;
use notiser_types::action::{DbusSignal, ServerInfo};
use notiser_types::config::Config;
use notiser_types::config::{AppRule, SortOrder};
use notiser_types::notification::{
    CloseReason, Notification, NotificationAction, NotificationHints, Urgency,
};
use notiser_render::layout::{LayoutRect, ResolvedElement, default_layout, measure_layout_height, resolve_layout};
use notiser_render::text::PreparedTextArea;

/// Minimum card height to prevent tiny/empty cards.
const MIN_CARD_HEIGHT: f32 = 48.0;

pub struct AppState {
    pub wayland: WaylandFields,
    pub manager: NotificationManager,
    pub history: NotificationHistory,
    pub animations: AnimationController,
    pub config: Config,
    pub dnd_active: bool,
    #[cfg(feature = "audio")]
    pub audio: Option<crate::audio::AudioPlayer>,
    pub signal_tx: tokio::sync::mpsc::Sender<DbusSignal>,
    pub loop_signal: LoopSignal,
    /// Cached measured heights per notification ID.
    pub card_heights: HashMap<u32, f32>,
    last_frame: Instant,
}

impl AppState {
    pub fn handle_input_action(&mut self, action: crate::wayland::input::InputAction) {
        use crate::wayland::input::InputAction;
        match action {
            InputAction::Dismiss(id) => {
                self.dismiss_notification(id, CloseReason::Dismissed);
            }
            InputAction::DismissAll => {
                let ids: Vec<u32> = self.manager.iter().map(|n| n.id).collect();
                for id in ids {
                    self.dismiss_notification(id, CloseReason::Dismissed);
                }
            }
            InputAction::InvokeDefault(id) => {
                // Fire ActionInvoked signal for "default" action, then dismiss
                let _ = self.signal_tx.try_send(DbusSignal::ActionInvoked {
                    id,
                    action_key: "default".into(),
                });
                self.dismiss_notification(id, CloseReason::Dismissed);
            }
            InputAction::InvokeAction(id, key) => {
                let _ = self.signal_tx.try_send(DbusSignal::ActionInvoked {
                    id,
                    action_key: key,
                });
                self.dismiss_notification(id, CloseReason::Dismissed);
            }
        }
    }

    fn dismiss_notification(&mut self, id: u32, reason: CloseReason) {
        let notification = match self.manager.get(id) {
            Some(n) => n,
            None => return,
        };

        // Push to history before removal
        if self.config.history.enabled {
            let dominated_by_transient =
                notification.hints.transient && !self.config.history.store_transient;
            if !dominated_by_transient {
                self.history.push(notification);
            }
        }

        if self.animations.on_exit(id) {
            self.wayland.dirty = true;
        } else {
            self.manager.remove(id);
            self.card_heights.remove(&id);
            update_surface_size(self);
            self.wayland.dirty = true;
        }
        let _ = self.signal_tx.try_send(DbusSignal::NotificationClosed { id, reason });
    }
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

    // Set up config file watcher
    let (reload_tx, reload_rx): (
        calloop::channel::Sender<ConfigReloadEvent>,
        calloop::channel::Channel<ConfigReloadEvent>,
    ) = calloop::channel::channel();
    let _config_watcher = watch_config(reload_tx).context("failed to start config watcher")?;

    loop_handle
        .insert_source(reload_rx, |event, _metadata, state: &mut AppState| {
            if let calloop::channel::Event::Msg(_) = event {
                handle_config_reload(state);
            }
        })
        .map_err(|e| anyhow::anyhow!("failed to insert config reload source: {e}"))?;

    // Set up signal handling for graceful shutdown
    loop_handle
        .insert_source(
            calloop::signals::Signals::new(&[calloop::signals::Signal::SIGINT, calloop::signals::Signal::SIGTERM])
                .context("failed to create signal source")?,
            |event, _metadata, state: &mut AppState| {
                info!(signal = ?event.signal(), "received signal, shutting down");
                state.wayland.running = false;
            },
        )
        .map_err(|e| anyhow::anyhow!("failed to insert signal source: {e}"))?;

    // Add a timer for checking notification timeouts (every 100ms)
    let timer = calloop::timer::Timer::from_duration(Duration::from_millis(100));
    loop_handle
        .insert_source(timer, |_deadline, _metadata, state: &mut AppState| {
            check_timeouts(state);
            // Re-arm: check every 100ms
            calloop::timer::TimeoutAction::ToDuration(Duration::from_millis(100))
        })
        .map_err(|e| anyhow::anyhow!("failed to insert timer source: {e}"))?;

    let animations = AnimationController::from_preset(config.animations.preset);
    let history = NotificationHistory::new(config.history.max_entries as usize);

    let mut state = AppState {
        wayland: wayland_state,
        manager: NotificationManager::new(),
        history,
        animations,
        dnd_active: config.dnd.enabled,
        #[cfg(feature = "audio")]
        audio: crate::audio::AudioPlayer::new(&config.audio),
        config,
        signal_tx,
        loop_signal,
        card_heights: HashMap::new(),
        last_frame: Instant::now(),
    };

    info!("main loop starting, waiting for notifications...");

    // Main event loop
    loop {
        if !state.wayland.running {
            break;
        }

        // Tick animations at frame rate (not the 100ms timer rate)
        {
            let now = Instant::now();
            let dt = now.duration_since(state.last_frame);
            state.last_frame = now;

            let completed = state.animations.tick(dt);
            if !completed.is_empty() {
                for id in completed {
                    if state.manager.remove(id).is_some() {
                        state.card_heights.remove(&id);
                        info!(id, "notification exit animation completed");
                    }
                }
                update_surface_size(&mut state);
                state.wayland.dirty = true;
            }

            if state.animations.has_active() {
                state.wayland.dirty = true;
            }
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

            let mut notification = Notification {
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

            // Apply app rules
            if let Some(rule) = matching_app_rule(&notification, &state.config.apps) {
                if let Some(urgency_override) = rule.urgency {
                    notification.hints.urgency = Some(urgency_override);
                }
                if let Some(timeout_override) = rule.timeout {
                    notification.expire_timeout = timeout_override as i32;
                }
            }

            info!(
                id,
                app = app_name,
                summary,
                urgency = ?notification.urgency(),
                "notification #{id}"
            );

            // DND: suppress non-critical notifications
            if state.dnd_active {
                let dominated_by_critical =
                    notification.urgency() == Urgency::Critical && state.config.dnd.allow_critical;
                if !dominated_by_critical {
                    info!(id, "notification suppressed by DND");
                    if state.config.history.enabled {
                        state.history.push(&notification);
                    }
                    let _ = reply.send(id);
                    return;
                }
            }

            // Handle replaces_id: remove the old notification silently
            if let Some(old_id) = notification.replaces_id {
                if state.manager.remove(old_id).is_some() {
                    state.animations.remove(old_id);
                    state.card_heights.remove(&old_id);
                    info!(old_id, id, "notification replaced");
                }
            }

            state.manager.add(notification);
            state.animations.on_enter(id);
            state.wayland.dirty = true;

            // Play notification sound
            #[cfg(feature = "audio")]
            if let Some(ref mut audio) = state.audio {
                if let Some(n) = state.manager.get(id) {
                    audio.play_for_notification(n, &state.config);
                }
            }

            // Update surface size based on notification count
            update_surface_size(state);

            let _ = reply.send(id);
        }

        DbusCommand::CloseNotification { id } => {
            info!(id, "notification closed via D-Bus");
            state.dismiss_notification(id, CloseReason::Closed);
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

        DbusCommand::ToggleDnd { reply } => {
            state.dnd_active = !state.dnd_active;
            info!(dnd = state.dnd_active, "DND toggled");
            let _ = reply.send(state.dnd_active);
        }

        DbusCommand::Reload { .. } => {
            handle_config_reload(state);
        }

        DbusCommand::GetHistory { limit, reply } => {
            let _ = reply.send(state.history.recent(limit as usize));
        }

        DbusCommand::GetStatus { reply } => {
            let _ = reply.send((
                state.dnd_active,
                state.manager.active_count() as u32,
                state.history.len() as u32,
            ));
        }
    }
}

fn check_timeouts(state: &mut AppState) {
    let default_timeout = Duration::from_millis(state.config.general.default_timeout as u64);

    // Find notifications that have timed out (skip those already exiting)
    let expired: Vec<u32> = state
        .manager
        .iter()
        .filter(|n| {
            if state.animations.is_exiting(n.id) {
                return false;
            }
            let timeout = if n.expire_timeout > 0 {
                Duration::from_millis(n.expire_timeout as u64)
            } else if n.expire_timeout == 0 {
                return false; // 0 means never expire
            } else {
                // Check per-urgency timeout override
                let urgency = n.urgency();
                if let Some(ov) = state.config.urgency.get(&urgency) {
                    if let Some(t) = ov.timeout {
                        Duration::from_millis(t as u64)
                    } else {
                        default_timeout
                    }
                } else {
                    default_timeout
                }
            };
            n.created_at.elapsed() >= timeout
        })
        .map(|n| n.id)
        .collect();

    let changed = !expired.is_empty();
    for id in expired {
        if state.animations.on_exit(id) {
            // Exit animation started; deferred removal
            let _ = state.signal_tx.try_send(DbusSignal::NotificationClosed {
                id,
                reason: CloseReason::Expired,
            });
        } else {
            // No animation
            if state.manager.remove(id).is_some() {
                state.card_heights.remove(&id);
                info!(id, "notification expired");
                let _ = state.signal_tx.try_send(DbusSignal::NotificationClosed {
                    id,
                    reason: CloseReason::Expired,
                });
            }
        }
    }

    if changed {
        update_surface_size(state);
        state.wayland.dirty = true;
    }
}

fn handle_config_reload(state: &mut AppState) {
    info!("reloading configuration");
    match load_config() {
        Ok(new_config) => {
            info!(
                width = new_config.appearance.width,
                timeout = new_config.general.default_timeout,
                "config reloaded successfully"
            );
            state.animations = AnimationController::from_preset(new_config.animations.preset);
            #[cfg(feature = "audio")]
            if let Some(ref mut audio) = state.audio {
                audio.update_config(&new_config.audio);
            }
            state.config = new_config;
            update_surface_size(state);
            state.wayland.dirty = true;
        }
        Err(e) => {
            warn!("config reload failed: {e}");
        }
    }
}

/// Extra padding (horizontal per-side, vertical per-side) to prevent animation clipping.
/// Covers spring overshoot (~27.5% on Grow) and slide offsets (40px).
const ANIM_H_PAD: u32 = 32;
const ANIM_V_PAD: u32 = 48;

fn animation_padding(state: &AppState) -> (u32, u32) {
    if state.animations.is_enabled() {
        (ANIM_H_PAD, ANIM_V_PAD)
    } else {
        (0, 0)
    }
}

fn update_surface_size(state: &mut AppState) {
    let count = state
        .manager
        .active_count()
        .min(state.config.general.max_visible as usize);

    if count == 0 {
        // Destroy the surface — a 0x0 layer surface is a protocol error
        if state.wayland.surface.take().is_some() {
            state.wayland.configured = false;
            info!("surface destroyed (no active notifications)");
        }
        return;
    }

    // Ensure surface exists when we have notifications
    if state.wayland.surface.is_none() {
        state.wayland.create_layer_surface(&state.wayland.qh.clone(), &state.config);
    }

    let appearance = &state.config.appearance;
    let gap = state.config.display.gap;
    let surface_padding: u32 = 8;
    let fallback_height = (appearance.padding.top + appearance.padding.bottom + 56) as f32;

    let ids = sorted_notification_ids(state);
    let total_cards_height: f32 = ids
        .iter()
        .map(|id| state.card_heights.get(id).copied().unwrap_or(fallback_height))
        .sum();

    let (h_pad, v_pad) = animation_padding(state);

    let width = appearance.width + h_pad * 2;
    let base_height = surface_padding * 2 + total_cards_height as u32 + (count as u32 - 1) * gap;
    let total_height = base_height + v_pad * 2;

    if let Some(ref mut surface) = state.wayland.surface {
        surface.layer().set_size(width, total_height);

        // Adjust margins to compensate for animation padding so visual card
        // position stays the same regardless of the extra surface area.
        let m = &state.config.display.margin;
        let (mt, mr, mb, ml) = adjusted_margins(m, state.config.display.anchor, h_pad, v_pad);
        surface.layer().set_margin(mt, mr, mb, ml);

        surface.layer().commit();
        surface.resize(width, total_height);
    }
}

/// Reduce margin on the anchored edge(s) to compensate for animation padding.
fn adjusted_margins(
    m: &notiser_types::config::Margins,
    anchor: notiser_types::config::Anchor,
    h_pad: u32,
    v_pad: u32,
) -> (i32, i32, i32, i32) {
    use notiser_types::config::Anchor::*;
    let mut mt = m.top as i32;
    let mut mr = m.right as i32;
    let mut mb = m.bottom as i32;
    let mut ml = m.left as i32;

    // Vertical: reduce margin on the anchor edge
    match anchor {
        TopLeft | TopCenter | TopRight => mt -= v_pad as i32,
        BottomLeft | BottomCenter | BottomRight => mb -= v_pad as i32,
        CenterLeft | CenterRight => {} // vertically centered, surface expands symmetrically
    }

    // Horizontal: reduce margin on the anchor edge
    match anchor {
        TopRight | BottomRight | CenterRight => mr -= h_pad as i32,
        TopLeft | BottomLeft | CenterLeft => ml -= h_pad as i32,
        TopCenter | BottomCenter => {} // horizontally centered, surface expands symmetrically
    }

    (mt, mr, mb, ml)
}

/// Get sorted notification IDs capped at max_visible.
pub fn sorted_notification_ids(state: &AppState) -> Vec<u32> {
    let mut notifications: Vec<_> = state.manager.iter().collect();
    match state.config.general.sort_order {
        SortOrder::TimeAscending => notifications.sort_by_key(|n| n.created_at),
        SortOrder::TimeDescending => notifications.sort_by_key(|n| std::cmp::Reverse(n.created_at)),
        SortOrder::UrgencyDescending => notifications.sort_by(|a, b| {
            b.urgency().cmp(&a.urgency()).then_with(|| a.created_at.cmp(&b.created_at))
        }),
    }
    let max = state.config.general.max_visible as usize;
    notifications.truncate(max);
    notifications.into_iter().map(|n| n.id).collect()
}

fn render_notifications(state: &mut AppState) {
    let ids = sorted_notification_ids(state);
    if ids.is_empty() {
        return;
    }

    // Snapshot config values before any mutable borrows
    let layout = state.config.layout.as_ref().cloned().unwrap_or_else(default_layout);
    let appearance = state.config.appearance.clone();
    let card_pad_top = appearance.padding.top as f32;
    let card_pad_bottom = appearance.padding.bottom as f32;
    let card_pad_left = appearance.padding.left as f32;
    let card_pad_right = appearance.padding.right as f32;
    let card_width = appearance.width as f32;

    // Collect notification data before borrowing gpu
    let notification_data: Vec<_> = ids
        .iter()
        .filter_map(|id| state.manager.get(*id).cloned())
        .collect();

    // --- Pass 1: Measure content heights (scoped borrow) ---
    let content_width = card_width - card_pad_left - card_pad_right;
    let measured_heights = {
        let surface = match state.wayland.surface.as_mut() {
            Some(s) => s,
            None => return,
        };
        let gpu = match surface.gpu_mut() {
            Some(g) => g,
            None => return,
        };
        if gpu.config.width == 0 || gpu.config.height == 0 {
            return;
        }

        let mut measured = Vec::with_capacity(notification_data.len());
        for notification in &notification_data {
            let h = measure_layout_height(&layout, notification, &appearance, content_width, &mut gpu.text_engine);
            let card_h = (card_pad_top + h + card_pad_bottom).max(MIN_CARD_HEIGHT);
            measured.push(card_h);
        }
        measured
    }; // surface borrow dropped

    // Update card_heights cache and resize surface to match
    for (i, notification) in notification_data.iter().enumerate() {
        state.card_heights.insert(notification.id, measured_heights[i]);
    }
    update_surface_size(state);

    // --- Pass 2: Render (re-acquire surface) ---
    let gap = state.config.display.gap as f32;
    let surface_padding: f32 = 8.0;

    let (h_pad, v_pad) = animation_padding(state);
    let h_pad = h_pad as f32;
    let v_pad = v_pad as f32;

    let bg_color = appearance.background.to_linear_array();
    let border_color = appearance.border.color.to_linear_array();
    let border_radius = appearance.border.radius;
    let border_width = appearance.border.width;

    let surface = match state.wayland.surface.as_mut() {
        Some(s) => s,
        None => return,
    };

    let gpu = match surface.gpu_mut() {
        Some(g) => g,
        None => return,
    };

    // --- Pass 2: Position and render cards ---
    let mut cards = Vec::new();
    let mut icon_renders = Vec::new();
    let mut text_buffers = Vec::new();

    let mut y_cursor = v_pad + surface_padding;

    for (i, notification) in notification_data.iter().enumerate() {
        let card_height = measured_heights[i];
        let anim_props = state.animations.props(notification.id);
        let opacity = anim_props.opacity;

        // Apply scale around card center
        let scale_x = anim_props.scale_x;
        let scale_y = anim_props.scale_y;
        let scaled_width = card_width * scale_x;
        let scaled_height = card_height * scale_y;

        let x = h_pad + anim_props.offset_x + (card_width - scaled_width) / 2.0;
        let y = y_cursor + anim_props.offset_y + (card_height - scaled_height) / 2.0;

        y_cursor += card_height + gap;

        // Clip bounds for text within this card
        let card_clip = [
            x as i32,
            y as i32,
            (x + scaled_width) as i32,
            (y + scaled_height) as i32,
        ];

        // Animate border radius: use anim value if set, but never less than config default
        let animated_radius = if anim_props.border_radius > 0.0 {
            anim_props.border_radius.max(border_radius)
        } else {
            border_radius
        };

        // Per-urgency background/border overrides
        let urgency = notification.urgency();
        let (mut card_bg, card_border) = if let Some(ov) = state.config.urgency.get(&urgency) {
            (
                ov.background.as_ref().map_or(bg_color, |c| c.to_linear_array()),
                ov.border_color.as_ref().map_or(border_color, |c| c.to_linear_array()),
            )
        } else {
            (bg_color, border_color)
        };

        // Per-app background override
        if let Some(rule) = matching_app_rule(notification, &state.config.apps) {
            if let Some(ref c) = rule.background {
                card_bg = c.to_linear_array();
            }
        }

        // Apply opacity to colors
        let card_bg = [card_bg[0], card_bg[1], card_bg[2], card_bg[3] * opacity];
        let card_border = [card_border[0], card_border[1], card_border[2], card_border[3] * opacity];

        cards.push(CardRenderData {
            rect: [x, y, scaled_width, scaled_height],
            background: card_bg,
            border_color: card_border,
            border_radius: animated_radius,
            border_width,
        });

        // Resolve layout for this notification (aligned with scaled card)
        let content_rect = LayoutRect {
            x: x + card_pad_left * scale_x,
            y: y + card_pad_top * scale_y,
            width: scaled_width - (card_pad_left + card_pad_right) * scale_x,
            height: scaled_height - (card_pad_top + card_pad_bottom) * scale_y,
        };

        let elements = resolve_layout(&layout, notification, &appearance, content_rect, &mut gpu.text_engine);

        for element in elements {
            match element {
                ResolvedElement::Text {
                    rect,
                    content,
                    font_size,
                    color,
                    ..
                } => {
                    let base_color = color
                        .as_ref()
                        .map(color_to_glyphon)
                        .unwrap_or_else(|| color_to_glyphon(&appearance.colors.body));

                    let text_color = glyphon::Color::rgba(
                        base_color.r(),
                        base_color.g(),
                        base_color.b(),
                        (base_color.a() as f32 * opacity) as u8,
                    );

                    let buf = gpu.text_engine.create_buffer(
                        &content,
                        font_size,
                        rect.width,
                    );
                    text_buffers.push((buf, rect, text_color, card_clip));
                }
                ResolvedElement::Image { rect, kind } => {
                    use notiser_types::layout::ImageKind;
                    let icon_name = match kind {
                        ImageKind::AppIcon => &notification.app_icon,
                        ImageKind::ImageData => continue, // TODO: inline image data
                    };
                    if icon_name.is_empty() {
                        continue;
                    }
                    // Load icon and upload to GPU
                    if let Some(rgba) = gpu.image_loader.load_icon(icon_name) {
                        if !gpu.texture_cache.has(icon_name) {
                            gpu.texture_cache.upload(
                                &gpu.ctx.device,
                                &gpu.ctx.queue,
                                icon_name,
                                rgba,
                            );
                        }
                        icon_renders.push(IconRenderData {
                            rect: [rect.x, rect.y, rect.width, rect.height],
                            icon_key: icon_name.to_string(),
                            rounding: 6.0,
                            opacity,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // Build text areas from buffers
    let text_areas: Vec<PreparedTextArea<'_>> = text_buffers
        .iter()
        .map(|(buf, rect, color, clip)| PreparedTextArea {
            buffer: buf,
            left: rect.x,
            top: rect.y,
            scale: 1.0,
            color: *color,
            clip: Some(*clip),
        })
        .collect();

    if let Err(e) = surface.render_cards(&cards, &icon_renders, &text_areas) {
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

/// Find the first matching app rule for a notification.
fn matching_app_rule<'a>(notification: &Notification, rules: &'a [AppRule]) -> Option<&'a AppRule> {
    rules.iter().find(|rule| {
        if let Some(ref pattern) = rule.match_app_name {
            if notification.app_name == *pattern {
                return true;
            }
        }
        if let Some(ref pattern) = rule.match_app_id {
            if let Some(ref entry) = notification.hints.desktop_entry {
                if entry == pattern {
                    return true;
                }
            }
        }
        false
    })
}
