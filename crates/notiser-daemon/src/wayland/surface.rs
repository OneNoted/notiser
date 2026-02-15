use std::ptr::NonNull;

use anyhow::{Context, Result};
use raw_window_handle::{
    HasDisplayHandle, RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle,
    WaylandWindowHandle,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use tracing::{debug, info};
use wayland_client::{Connection, Proxy};

use notiser_render::gpu::GpuContext;
use notiser_render::text::{PreparedTextArea, TextEngine};

/// Owns both the Wayland layer surface and the wgpu surface.
/// Drop order: gpu_surface first, then layer_surface.
pub struct ManagedSurface {
    gpu_surface: Option<GpuSurface>,
    layer_surface: LayerSurface,
}

pub struct GpuSurface {
    pub ctx: GpuContext,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub text_engine: TextEngine,
}

impl ManagedSurface {
    pub fn new_pending(layer_surface: LayerSurface) -> Self {
        Self {
            gpu_surface: None,
            layer_surface,
        }
    }

    pub fn has_gpu_surface(&self) -> bool {
        self.gpu_surface.is_some()
    }

    pub fn layer(&self) -> &LayerSurface {
        &self.layer_surface
    }

    pub fn gpu(&self) -> Option<&GpuSurface> {
        self.gpu_surface.as_ref()
    }

    pub fn gpu_mut(&mut self) -> Option<&mut GpuSurface> {
        self.gpu_surface.as_mut()
    }

    pub fn init_gpu_surface(
        &mut self,
        connection: &Connection,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let ctx = pollster::block_on(GpuContext::new())
            .context("failed to initialize GPU context")?;

        let backend = connection.backend();
        let display_handle = backend
            .display_handle()
            .context("failed to get display handle")?;
        let raw_display_handle = display_handle.as_raw();

        let wl_surface = self.layer_surface.wl_surface();
        let surface_id = wl_surface.id();
        let surface_ptr = surface_id.as_ptr();
        let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(surface_ptr.cast()).context("null surface pointer")?,
        ));

        // SAFETY: We ensure the Wayland connection and surface outlive the wgpu surface
        // by dropping gpu_surface before layer_surface in the Drop impl.
        #[allow(unsafe_code)]
        let surface = unsafe {
            ctx.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle,
                    raw_window_handle,
                })
                .context("failed to create wgpu surface from raw handles")?
        };

        let capabilities = surface.get_capabilities(&ctx.adapter);
        let format = capabilities
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(capabilities.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Mailbox,
            alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&ctx.device, &config);

        let text_engine = TextEngine::new(&ctx.device, &ctx.queue, format);

        info!(format = ?format, width, height, "wgpu surface initialized");

        self.gpu_surface = Some(GpuSurface {
            ctx,
            surface,
            config,
            text_engine,
        });

        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(ref mut gpu) = self.gpu_surface {
            if gpu.config.width != width || gpu.config.height != height {
                gpu.config.width = width;
                gpu.config.height = height;
                gpu.surface.configure(&gpu.ctx.device, &gpu.config);
                debug!(width, height, "surface resized");
            }
        }
    }

    pub fn render_solid(&mut self, r: f64, g: f64, b: f64, a: f64) -> Result<()> {
        let gpu = self
            .gpu_surface
            .as_ref()
            .context("GPU surface not initialized")?;

        let output = gpu.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = gpu
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render"),
            });

        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        gpu.ctx.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Render background with text overlay.
    pub fn render_with_text(
        &mut self,
        bg: [f64; 4],
        text_areas: &[PreparedTextArea<'_>],
    ) -> Result<()> {
        let gpu = self
            .gpu_surface
            .as_mut()
            .context("GPU surface not initialized")?;

        let width = gpu.config.width;
        let height = gpu.config.height;

        // Prepare text
        gpu.text_engine
            .prepare_text(&gpu.ctx.device, &gpu.ctx.queue, width, height, text_areas)
            .context("failed to prepare text")?;

        let output = gpu.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = gpu
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0],
                            g: bg[1],
                            b: bg[2],
                            a: bg[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            gpu.text_engine
                .render_text(&mut pass)
                .context("failed to render text")?;
        }

        gpu.ctx.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        gpu.text_engine.trim();

        Ok(())
    }
}

impl Drop for ManagedSurface {
    fn drop(&mut self) {
        self.gpu_surface.take();
        debug!("managed surface dropped");
    }
}
