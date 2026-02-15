use cosmic_text::{Attrs, Buffer, Color as CosmicColor, FontSystem, Metrics, Shaping, SwashCache};
use glyphon::{
    Cache as GlyphonCache, ColorMode, Resolution, TextArea, TextAtlas, TextBounds,
    TextRenderer as GlyphonTextRenderer, Viewport,
};

pub struct TextEngine {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub atlas: TextAtlas,
    pub text_renderer: GlyphonTextRenderer,
    pub viewport: Viewport,
    cache: GlyphonCache,
}

impl TextEngine {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = GlyphonCache::new(device);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let text_renderer =
            GlyphonTextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let viewport = Viewport::new(device, &cache);

        Self {
            font_system,
            swash_cache,
            atlas,
            text_renderer,
            viewport,
            cache,
        }
    }

    pub fn create_buffer(&mut self, text: &str, font_size: f32, width: f32) -> Buffer {
        let metrics = Metrics::new(font_size, font_size * 1.3);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, Some(width), None);
        buffer.set_text(&mut self.font_system, text, Attrs::new(), Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
    }

    pub fn prepare_text(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        text_areas: &[PreparedTextArea<'_>],
    ) -> Result<(), glyphon::PrepareError> {
        self.viewport.update(
            queue,
            Resolution { width, height },
        );

        let areas: Vec<TextArea<'_>> = text_areas
            .iter()
            .map(|area| {
                let bounds = if let Some(c) = area.clip {
                    TextBounds {
                        left: c[0],
                        top: c[1],
                        right: c[2],
                        bottom: c[3],
                    }
                } else {
                    TextBounds {
                        left: 0,
                        top: 0,
                        right: width as i32,
                        bottom: height as i32,
                    }
                };
                TextArea {
                    buffer: area.buffer,
                    left: area.left,
                    top: area.top,
                    scale: area.scale,
                    bounds,
                    default_color: area.color,
                    custom_glyphs: &[],
                }
            })
            .collect();

        self.text_renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            areas,
            &mut self.swash_cache,
        )
    }

    pub fn render_text<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
    ) -> Result<(), glyphon::RenderError> {
        self.text_renderer.render(&self.atlas, &self.viewport, pass)
    }

    pub fn trim(&mut self) {
        self.atlas.trim();
    }
}

/// Measure the actual rendered height of a buffer using layout runs.
/// When `max_lines` is set, only counts up to that many lines.
pub fn measure_text_height(buffer: &Buffer, max_lines: Option<u32>) -> f32 {
    let limit = max_lines.unwrap_or(u32::MAX);
    let mut count = 0u32;
    let mut height = 0.0f32;
    for run in buffer.layout_runs() {
        count += 1;
        height = run.line_top + run.line_height;
        if count >= limit {
            break;
        }
    }
    height
}

pub struct PreparedTextArea<'a> {
    pub buffer: &'a Buffer,
    pub left: f32,
    pub top: f32,
    pub scale: f32,
    pub color: glyphon::Color,
    /// Clip bounds [left, top, right, bottom] in surface coordinates.
    /// If None, uses full surface bounds.
    pub clip: Option<[i32; 4]>,
}
