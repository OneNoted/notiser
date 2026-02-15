use thiserror::Error;

#[derive(Error, Debug)]
pub enum ImageError {
    #[error("failed to load image: {0}")]
    Load(#[from] image::ImageError),
    #[error("unsupported image format")]
    UnsupportedFormat,
    #[error("SVG render error")]
    SvgError,
}

pub struct ImageLoader;

impl ImageLoader {
    pub fn new() -> Self {
        Self
    }
}
