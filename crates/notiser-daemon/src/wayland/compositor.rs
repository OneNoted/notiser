use anyhow::Result;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_seat,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{SeatHandler, SeatState},
    shell::WaylandSurface,
    shell::wlr_layer::{
        Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
        LayerSurfaceConfigure,
    },
};
use tracing::{debug, info, warn};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_seat, wl_surface},
    Connection, EventQueue, QueueHandle,
};

use super::surface::ManagedSurface;
use crate::app::AppState;

/// Initialize the Wayland connection and create the event queue.
pub fn init_wayland() -> Result<(Connection, EventQueue<AppState>, wayland_client::globals::GlobalList)> {
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init::<AppState>(&conn)?;
    Ok((conn, event_queue, globals))
}

/// Wayland-related fields embedded in AppState.
pub struct WaylandFields {
    pub registry: RegistryState,
    pub compositor: CompositorState,
    pub layer_shell: LayerShell,
    pub seat: SeatState,
    pub output: OutputState,
    pub surface: Option<ManagedSurface>,
    pub configured: bool,
    pub width: u32,
    pub height: u32,
    pub dirty: bool,
    pub running: bool,
}

impl WaylandFields {
    pub fn new(
        globals: &wayland_client::globals::GlobalList,
        qh: &QueueHandle<AppState>,
    ) -> Result<Self> {
        let registry = RegistryState::new(globals);
        let compositor = CompositorState::bind(globals, qh)?;
        let layer_shell = LayerShell::bind(globals, qh)?;
        let seat = SeatState::new(globals, qh);
        let output = OutputState::new(globals, qh);

        Ok(Self {
            registry,
            compositor,
            layer_shell,
            seat,
            output,
            surface: None,
            configured: false,
            width: 400,
            height: 100,
            dirty: false,
            running: true,
        })
    }

    pub fn create_layer_surface(
        &mut self,
        qh: &QueueHandle<AppState>,
        config: &notiser_types::config::Config,
    ) {
        let wl_surface = self.compositor.create_surface(qh);

        let sctk_layer = match config.display.layer {
            notiser_types::config::Layer::Background => Layer::Background,
            notiser_types::config::Layer::Bottom => Layer::Bottom,
            notiser_types::config::Layer::Top => Layer::Top,
            notiser_types::config::Layer::Overlay => Layer::Overlay,
        };

        let layer = self.layer_shell.create_layer_surface(
            qh,
            wl_surface,
            sctk_layer,
            Some("notiser"),
            None,
        );

        let anchor = config_anchor_to_sctk(config.display.anchor);
        layer.set_anchor(anchor);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);

        self.width = config.appearance.width;
        layer.set_size(self.width, self.height);

        let m = &config.display.margin;
        layer.set_margin(m.top as i32, m.right as i32, m.bottom as i32, m.left as i32);
        layer.set_exclusive_zone(-1);
        layer.commit();

        info!(
            width = self.width,
            height = self.height,
            "layer surface created"
        );

        self.surface = Some(ManagedSurface::new_pending(layer));
    }
}

// All SCTK handlers implemented on AppState

impl CompositorHandler for AppState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        debug!(factor = new_factor, "scale factor changed");
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.wayland.dirty = true;
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for AppState {
    fn closed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
    ) {
        info!("layer surface closed");
        self.wayland.running = false;
    }

    fn configure(
        &mut self,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (w, h) = configure.new_size;
        let width = if w > 0 { w } else { self.wayland.width };
        let height = if h > 0 { h } else { self.wayland.height };

        info!(width, height, "layer surface configured");

        self.wayland.width = width;
        self.wayland.height = height;
        self.wayland.configured = true;
        self.wayland.dirty = true;

        if let Some(ref mut managed) = self.wayland.surface {
            if !managed.has_gpu_surface() {
                if let Err(e) = managed.init_gpu_surface(conn, width, height) {
                    warn!("failed to initialize GPU surface: {e}");
                }
            } else {
                managed.resize(width, height);
            }
        }
    }
}

impl OutputHandler for AppState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.wayland.output
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
        debug!("new output");
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl SeatHandler for AppState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.wayland.seat
    }

    fn new_seat(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
    ) {
    }

    fn new_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: smithay_client_toolkit::seat::Capability,
    ) {
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: smithay_client_toolkit::seat::Capability,
    ) {
    }

    fn remove_seat(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
    ) {
    }
}

impl ProvidesRegistryState for AppState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.wayland.registry
    }

    registry_handlers![OutputState, SeatState];
}

fn config_anchor_to_sctk(anchor: notiser_types::config::Anchor) -> Anchor {
    use notiser_types::config::Anchor as CfgAnchor;
    match anchor {
        CfgAnchor::TopLeft => Anchor::TOP | Anchor::LEFT,
        CfgAnchor::TopCenter => Anchor::TOP,
        CfgAnchor::TopRight => Anchor::TOP | Anchor::RIGHT,
        CfgAnchor::BottomLeft => Anchor::BOTTOM | Anchor::LEFT,
        CfgAnchor::BottomCenter => Anchor::BOTTOM,
        CfgAnchor::BottomRight => Anchor::BOTTOM | Anchor::RIGHT,
        CfgAnchor::CenterLeft => Anchor::LEFT,
        CfgAnchor::CenterRight => Anchor::RIGHT,
    }
}

delegate_compositor!(AppState);
delegate_layer!(AppState);
delegate_output!(AppState);
delegate_seat!(AppState);
delegate_registry!(AppState);
