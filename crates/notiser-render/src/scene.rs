use notiser_types::config::Color;

use crate::layout::LayoutRect;

pub struct Scene {
    pub width: u32,
    pub height: u32,
    pub elements: Vec<SceneElement>,
}

pub enum SceneElement {
    RoundedRect {
        rect: LayoutRect,
        color: Color,
        border_color: Color,
        border_width: f32,
        border_radius: f32,
        opacity: f32,
    },
    Text {
        rect: LayoutRect,
        text: String,
        font_size: f32,
        color: Color,
    },
    Image {
        rect: LayoutRect,
        texture_id: u32,
    },
}
