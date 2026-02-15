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

pub struct WaylandState {
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

impl WaylandState {
    pub fn new(
        globals: &wayland_client::globals::GlobalList,
        qh: &QueueHandle<Self>,
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

    pub fn create_layer_surface(&mut self, qh: &QueueHandle<Self>) {
        let wl_surface = self.compositor.create_surface(qh);

        let layer = self.layer_shell.create_layer_surface(
            qh,
            wl_surface,
            Layer::Overlay,
            Some("notiser"),
            None,
        );

        layer.set_anchor(Anchor::TOP | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(self.width, self.height);
        layer.set_margin(10, 10, 0, 0);
        layer.set_exclusive_zone(-1);
        layer.commit();

        info!(
            width = self.width,
            height = self.height,
            "layer surface created"
        );

        self.surface = Some(ManagedSurface::new_pending(layer));
    }

    pub fn render(&mut self) {
        if let Some(ref mut surface) = self.surface {
            if surface.has_gpu_surface() {
                // Catppuccin Mocha surface color
                if let Err(e) = surface.render_solid(
                    0.118, 0.118, 0.180, 1.0, // #1e1e2e
                ) {
                    warn!("render error: {e}");
                }
            }
        }
    }
}

impl CompositorHandler for WaylandState {
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
        self.dirty = true;
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

impl LayerShellHandler for WaylandState {
    fn closed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
    ) {
        info!("layer surface closed");
        self.running = false;
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
        let width = if w > 0 { w } else { self.width };
        let height = if h > 0 { h } else { self.height };

        info!(width, height, "layer surface configured");

        self.width = width;
        self.height = height;
        self.configured = true;
        self.dirty = true;

        if let Some(ref mut managed) = self.surface {
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

impl OutputHandler for WaylandState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
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

impl SeatHandler for WaylandState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
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

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(WaylandState);
delegate_layer!(WaylandState);
delegate_output!(WaylandState);
delegate_seat!(WaylandState);
delegate_registry!(WaylandState);

/// Initialize the Wayland connection and create the event queue.
pub fn init_wayland() -> Result<(Connection, EventQueue<WaylandState>, wayland_client::globals::GlobalList)> {
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init::<WaylandState>(&conn)?;
    Ok((conn, event_queue, globals))
}
