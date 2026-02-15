use std::collections::HashMap;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::{debug, warn};

#[derive(Error, Debug)]
pub enum ImageError {
    #[error("failed to load image: {0}")]
    Load(#[from] image::ImageError),
    #[error("unsupported image format")]
    UnsupportedFormat,
    #[error("SVG render error")]
    SvgError,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("icon not found: {0}")]
    NotFound(String),
}

/// RGBA pixel data ready for GPU upload.
pub struct RgbaImage {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Loads and caches icon images.
pub struct ImageLoader {
    /// Cache: icon key → loaded RGBA data
    cache: HashMap<String, Option<RgbaImage>>,
    icon_size: u32,
}

impl ImageLoader {
    pub fn new(icon_size: u32) -> Self {
        Self {
            cache: HashMap::new(),
            icon_size,
        }
    }

    /// Load an icon by name or path. Returns None if not found.
    /// Results are cached.
    pub fn load_icon(&mut self, icon: &str) -> Option<&RgbaImage> {
        if icon.is_empty() {
            return None;
        }

        if !self.cache.contains_key(icon) {
            let result = self.load_icon_inner(icon);
            match result {
                Ok(img) => {
                    self.cache.insert(icon.to_string(), Some(img));
                }
                Err(e) => {
                    debug!(icon, error = %e, "icon load failed");
                    self.cache.insert(icon.to_string(), None);
                }
            }
        }

        self.cache.get(icon).and_then(|o| o.as_ref())
    }

    fn load_icon_inner(&self, icon: &str) -> Result<RgbaImage, ImageError> {
        // If it's already a file path, load directly
        let path = Path::new(icon);
        if path.is_absolute() && path.exists() {
            return self.load_file(path);
        }

        // Search freedesktop icon theme locations
        if let Some(path) = self.find_icon(icon) {
            return self.load_file(&path);
        }

        Err(ImageError::NotFound(icon.to_string()))
    }

    /// Search for an icon by name in standard locations.
    fn find_icon(&self, name: &str) -> Option<PathBuf> {
        let size = self.icon_size;
        let sizes = [size, 48, 64, 32, 128, 256, 24, 22, 16];
        let categories = ["apps", "status", "devices", "mimetypes"];

        // Check XDG_DATA_DIRS icon themes
        let data_dirs = std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/share:/usr/local/share".into());
        let home = std::env::var("HOME").unwrap_or_default();
        let local_share = format!("{home}/.local/share");

        let mut search_dirs: Vec<String> = vec![local_share];
        search_dirs.extend(data_dirs.split(':').map(String::from));

        // Search hicolor theme (fallback theme per spec)
        for dir in &search_dirs {
            for &sz in &sizes {
                for cat in &categories {
                    for ext in &["png", "svg"] {
                        let path = PathBuf::from(dir)
                            .join("icons/hicolor")
                            .join(format!("{sz}x{sz}"))
                            .join(cat)
                            .join(format!("{name}.{ext}"));
                        if path.exists() {
                            return Some(path);
                        }
                    }
                }
                // Also check scalable
                for cat in &categories {
                    let path = PathBuf::from(dir)
                        .join("icons/hicolor/scalable")
                        .join(cat)
                        .join(format!("{name}.svg"));
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
        }

        // Check pixmaps
        for ext in &["png", "svg", "xpm"] {
            let path = PathBuf::from("/usr/share/pixmaps").join(format!("{name}.{ext}"));
            if path.exists() {
                return Some(path);
            }
        }

        None
    }

    /// Load an image file and convert to RGBA.
    fn load_file(&self, path: &Path) -> Result<RgbaImage, ImageError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match ext {
            "svg" => self.load_svg(path),
            _ => self.load_raster(path),
        }
    }

    fn load_raster(&self, path: &Path) -> Result<RgbaImage, ImageError> {
        let img = image::open(path)?;
        let resized = img.resize_exact(
            self.icon_size,
            self.icon_size,
            image::imageops::FilterType::Lanczos3,
        );
        let rgba = resized.to_rgba8();
        Ok(RgbaImage {
            width: rgba.width(),
            height: rgba.height(),
            data: rgba.into_raw(),
        })
    }

    fn load_svg(&self, path: &Path) -> Result<RgbaImage, ImageError> {
        let data = std::fs::read(path)?;
        let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default())
            .map_err(|_| ImageError::SvgError)?;

        let size = self.icon_size;
        let mut pixmap =
            resvg::tiny_skia::Pixmap::new(size, size).ok_or(ImageError::SvgError)?;

        let tree_size = tree.size();
        let sx = size as f32 / tree_size.width();
        let sy = size as f32 / tree_size.height();
        let scale = sx.min(sy);

        let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        Ok(RgbaImage {
            width: size,
            height: size,
            data: pixmap.data().to_vec(),
        })
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

/// Manages GPU textures for icons.
pub struct TextureCache {
    textures: HashMap<String, wgpu::Texture>,
    bind_groups: HashMap<String, wgpu::BindGroup>,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl TextureCache {
    pub fn new(device: &wgpu::Device) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("icon_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("icon_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        Self {
            textures: HashMap::new(),
            bind_groups: HashMap::new(),
            sampler,
            bind_group_layout,
        }
    }

    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    /// Upload an image to GPU and cache the texture + bind group.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: &str,
        image: &RgbaImage,
    ) -> &wgpu::BindGroup {
        if !self.textures.contains_key(key) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("icon_texture"),
                size: wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &image.data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * image.width),
                    rows_per_image: Some(image.height),
                },
                wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                },
            );

            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("icon_bind_group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });

            self.textures.insert(key.to_string(), texture);
            self.bind_groups.insert(key.to_string(), bind_group);
        }

        &self.bind_groups[key]
    }

    /// Check if a texture is already cached.
    pub fn has(&self, key: &str) -> bool {
        self.bind_groups.contains_key(key)
    }

    /// Get a cached bind group.
    pub fn get(&self, key: &str) -> Option<&wgpu::BindGroup> {
        self.bind_groups.get(key)
    }
}

/// Pipeline for rendering textured quads (icons).
pub struct ImagePipeline {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    uniform_bind_group_layout: wgpu::BindGroupLayout,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ImageUniforms {
    rect: [f32; 4],       // x, y, width, height
    resolution: [f32; 2],
    rounding: f32,
    opacity: f32,
}

impl ImagePipeline {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        texture_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/image.wgsl").into()),
        });

        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("image_uniform_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image_pipeline_layout"),
            bind_group_layouts: &[&uniform_bind_group_layout, texture_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                    }],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        use wgpu::util::DeviceExt;
        let vertices: &[[f32; 2]] = &[[-1.0, -1.0], [3.0, -1.0], [-1.0, 3.0]];
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("image_vertices"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            pipeline,
            vertex_buffer,
            uniform_bind_group_layout,
        }
    }

    pub fn draw_icon(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        device: &wgpu::Device,
        rect: [f32; 4],
        resolution: [f32; 2],
        rounding: f32,
        opacity: f32,
        texture_bind_group: &wgpu::BindGroup,
    ) {
        use wgpu::util::DeviceExt;

        let uniforms = ImageUniforms {
            rect,
            resolution,
            rounding,
            opacity,
        };

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("image_uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image_uniform_bg"),
            layout: &self.uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &uniform_bind_group, &[]);
        pass.set_bind_group(1, texture_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..3, 0..1);
    }
}
