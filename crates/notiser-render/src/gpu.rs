use thiserror::Error;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum GpuError {
    #[error("failed to create wgpu instance")]
    InstanceCreation,
    #[error("no suitable GPU adapter found")]
    NoAdapter,
    #[error("failed to request device: {0}")]
    DeviceRequest(#[from] wgpu::RequestDeviceError),
    #[error("surface error: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
}

pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl GpuContext {
    pub async fn new() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(GpuError::NoAdapter)?;

        info!("GPU adapter: {}", adapter.get_info().name);

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("notiser"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                    ..Default::default()
                },
                None,
            )
            .await?;

        device.on_uncaptured_error(Box::new(|error| {
            warn!("wgpu error: {error}");
        }));

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }
}
