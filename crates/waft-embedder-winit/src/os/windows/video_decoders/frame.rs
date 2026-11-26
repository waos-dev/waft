//! A picture the system's decoder put out, as the app holds it: its texture, shared in NV12 or P010, and
//! what its stream and its chunk say of it.

use std::sync::Arc;
use std::time::Duration;

use waft_embedder::image_codecs::{ImageError, ImageErrorKind, ImageFuture, PixelBuffer};
use waft_embedder::{HardwareBuffer, PixelAspect, PixelRect, VideoColor, VideoFrame};

use super::super::shared_textures::SharedTexture;

pub(super) struct DecodedFrame {
    pub(super) texture: Arc<SharedTexture>,
    pub(super) timestamp: i64,
    pub(super) duration: Option<Duration>,
    pub(super) visible: PixelRect,
    pub(super) pixel_aspect: PixelAspect,
    pub(super) color: VideoColor,
}

impl VideoFrame for DecodedFrame {
    fn width(&self) -> u32 {
        self.texture.width()
    }

    fn height(&self) -> u32 {
        self.texture.height()
    }

    /// The picture is in NV12 or P010, which no [`PixelFormat`](waft_embedder::image_codecs::PixelFormat)
    /// names, and in GPU memory the CPU does not map.
    fn read_pixels(&self) -> ImageFuture<PixelBuffer> {
        Box::pin(std::future::ready(Err(ImageError::new(
            ImageErrorKind::Unsupported,
            "a decoded picture is NV12 or P010 on the GPU, read through a device".to_owned(),
        ))))
    }

    fn hardware_buffer(&self) -> Arc<dyn HardwareBuffer> {
        self.texture.clone()
    }

    fn rotation(&self) -> u32 {
        0
    }

    fn flip(&self) -> bool {
        false
    }

    fn timestamp(&self) -> Option<i64> {
        Some(self.timestamp)
    }

    fn duration(&self) -> Option<Duration> {
        self.duration
    }

    fn visible_rect(&self) -> PixelRect {
        self.visible
    }

    fn pixel_aspect(&self) -> PixelAspect {
        self.pixel_aspect
    }

    fn color(&self) -> VideoColor {
        self.color
    }
}
