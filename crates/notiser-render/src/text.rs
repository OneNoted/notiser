use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache};
use glyphon::{
    Cache as GlyphonCache, TextAtlas, TextRenderer as GlyphonTextRenderer,
};

pub struct TextEngine {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub atlas: TextAtlas,
    pub text_renderer: GlyphonTextRenderer,
    pub cache: GlyphonCache,
}

impl TextEngine {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = GlyphonCache::new(device);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let text_renderer =
            GlyphonTextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);

        Self {
            font_system,
            swash_cache,
            atlas,
            text_renderer,
            cache,
        }
    }

    pub fn create_buffer(&mut self, text: &str, font_size: f32, width: f32) -> Buffer {
        let metrics = Metrics::new(font_size, font_size * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, Some(width), None);
        buffer.set_text(&mut self.font_system, text, Attrs::new(), Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
    }
}
