//! Flutter counterpart: the engine's `SnapshotDelegate::MakeRasterSnapshotSync`
//! (`lib/ui/snapshot_delegate.h`), which the rasterizer implements and `Picture.toImageSync`
//! draws through: what a host answers [`Platform::picture_to_image_sync`](crate::Platform) with;
//! and the read-back behind `Image.toByteData`, what it answers
//! [`Platform::read_image_pixels`](crate::Platform) with.
//!
//! The invariant a later editor must keep: the picture is drawn with a renderer of the delegate's
//! own, on the device the host's images belong to, so an image it makes draws in every view and
//! no view's renderer is borrowed while the framework records a frame.

use std::cell::{RefCell, RefMut};
use std::sync::Arc;

use crate::Rect;
use crate::image_codecs::{ImageError, ImageErrorKind, ImageFuture};
use crate::painting::{Canvas, Image, Paint, Picture, valo};

/// Draws pictures into images on the device of a host's image store, with a renderer made at
/// the first picture.
pub struct SnapshotDelegate {
    images: valo::ImageContext,
    context: RefCell<Option<valo::Context>>,
}

impl SnapshotDelegate {
    /// A delegate drawing on the device `images` belongs to.
    pub fn new(images: valo::ImageContext) -> SnapshotDelegate {
        SnapshotDelegate {
            images,
            context: RefCell::new(None),
        }
    }

    /// `picture` drawn into a new image `width` by `height` pixels large over transparent black,
    /// what lies past those bounds clipped: valo's `display_list_to_image`, Impeller's
    /// `DisplayListToTexture`. The drawing is submitted now, and anything drawn with the image
    /// waits for it on the GPU.
    ///
    /// # Panics
    ///
    /// Where `width` or `height` is zero, as Dart throws "Invalid image dimensions.".
    pub fn make_raster_snapshot_sync(&self, picture: &Picture, width: u32, height: u32) -> Image {
        assert!(width > 0 && height > 0, "Invalid image dimensions.");
        self.context()
            .display_list_to_image(picture, [width, height])
    }

    /// `image`'s pixels, read back from the device it lives on, `wait` waiting for the GPU's
    /// answer as the host can: see [`read_image_pixels`].
    pub fn read_pixels(
        &self,
        image: &Image,
        wait: impl FnOnce(&wgpu::Device),
    ) -> ImageFuture<valo::PixelBuffer> {
        read_image_pixels(&mut self.context(), image, wait)
    }

    /// The delegate's renderer, made at its first use.
    fn context(&self) -> RefMut<'_, valo::Context> {
        RefMut::map(self.context.borrow_mut(), |context| {
            context.get_or_insert_with(|| {
                let mut context =
                    valo::Context::new(self.images.device().clone(), self.images.queue().clone());
                context.set_hide_missing_glyphs(true);
                context
            })
        })
    }
}

/// `image`'s pixels as straight-alpha RGBA, read back with `context`, a renderer on the device the
/// image lives on: Dart's `Image.toByteData(format: ImageByteFormat.rawStraightRgba)`. valo reads
/// back what a picture draws, so the image is drawn into one of its size and that is read.
///
/// The drawing and its copy go to the GPU now, and the answer comes once the device is polled.
/// How to wait for it is the host's, which `wait` is given the device for: a native host polls it
/// off the main thread, on its `Dispatcher`; a browser answers on its own, so its host waits on
/// nothing; a test polls it at once.
pub(crate) fn read_image_pixels(
    context: &mut valo::Context,
    image: &Image,
    wait: impl FnOnce(&wgpu::Device),
) -> ImageFuture<valo::PixelBuffer> {
    let [width, height] = image.size();
    let mut canvas = Canvas::new();
    let whole = Rect::from_ltwh(0.0, 0.0, f64::from(width), f64::from(height));
    canvas.draw_image(image, whole.into(), &Paint::default());
    let request = context.request_pixels(
        &Arc::new(canvas.build()),
        [width, height],
        Some(valo::Color::TRANSPARENT),
    );
    wait(context.device());
    Box::pin(async move {
        request
            .await
            .map_err(|error| ImageError::new(ImageErrorKind::Other, error.to_string()))
    })
}

impl std::fmt::Debug for SnapshotDelegate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotDelegate").finish_non_exhaustive()
    }
}
