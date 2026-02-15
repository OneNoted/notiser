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
            .map(|area| TextArea {
                buffer: area.buffer,
                left: area.left,
                top: area.top,
                scale: area.scale,
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                },
                default_color: area.color,
                custom_glyphs: &[],
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
pub fn measure_text_height(buffer: &Buffer) -> f32 {
    buffer
        .layout_runs()
        .last()
        .map(|run| run.line_top + run.line_height)
        .unwrap_or(0.0)
}

pub struct PreparedTextArea<'a> {
    pub buffer: &'a Buffer,
    pub left: f32,
    pub top: f32,
    pub scale: f32,
    pub color: glyphon::Color,
}
