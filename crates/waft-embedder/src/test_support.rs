//! Images, composition layers and drawn pixels for tests, without a host.
//!
//! An [`Image`](crate::Image) only exists once pixels have been uploaded to a GPU, so a test of
//! anything above the host — a cache, a layout, a widget — would otherwise have no image to work
//! with. This makes one directly, on a device of its own that no window is attached to, and
//! draws a [`Picture`](crate::Picture) there to read back what it shows. A
//! [`CompositionLayer`](crate::CompositionLayer) only exists where a host composites, so this
//! makes one nobody feeds.
//!
//! Behind the `test-support` feature, so an application never builds it. A test crate asks for
//! it as a dev-dependency:
//!
//! ```toml
//! [dev-dependencies]
//! waft-embedder = { workspace = true, features = ["test-support"] }
//! ```

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use valo::{Context, ImageDesc};

use crate::{CompositionLayer, HardwareBuffer, Image, Picture, Placement};

/// One device for the whole test process, created on first use.
///
/// Shared rather than made per test: a device costs driver set-up that dwarfs anything a test
/// does with it. It also never runs a destructor, which matters — a graphics device torn down
/// while the thread it belongs to is already unwinding its own thread-locals aborts the process.
static GPU: OnceLock<Mutex<Context>> = OnceLock::new();

/// A solid image of `size`, every pixel `color` as straight red, green, blue and alpha.
///
/// # Panics
///
/// Panics if no GPU adapter is available, which on a machine that can run the renderer at all
/// means the test environment has no graphics device.
pub fn solid_image(size: [u32; 2], color: [u8; 4]) -> Image {
    let [width, height] = size;
    let pixels: Vec<u8> = color
        .iter()
        .copied()
        .cycle()
        .take(width as usize * height as usize * 4)
        .collect();
    image_from_pixels(size, &pixels)
}

/// An image of `size` from straight-alpha RGBA samples, four bytes a pixel.
///
/// # Panics
///
/// Panics if `pixels` is not exactly four bytes per pixel, or if no GPU adapter is available.
pub fn image_from_pixels(size: [u32; 2], pixels: &[u8]) -> Image {
    let [width, height] = size;
    assert_eq!(
        pixels.len(),
        width as usize * height as usize * 4,
        "an image needs four bytes a pixel"
    );
    gpu().upload_image(
        ImageDesc {
            size,
            premultiplied: false,
            // No mips: a test never draws these small enough to need them, and building a chain
            // would only slow every image a test makes.
            mips: false,
        },
        pixels,
    )
}

/// What `picture` shows drawn over a transparent area of `size`, as straight-alpha RGBA
/// samples, four bytes a pixel and row after row.
///
/// # Panics
///
/// Panics if no GPU adapter is available.
pub fn drawn_pixels(picture: Picture, size: [u32; 2]) -> Vec<u8> {
    let mut gpu = gpu();
    let request = gpu.request_pixels(&Arc::new(picture), size, Some(valo::Color::TRANSPARENT));
    gpu.device()
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the test GPU device answers");
    pollster::block_on(request)
        .expect("the picture reads back")
        .into_bytes()
}

/// `image`'s pixels read back from the test device, as a host's `Platform::read_image_pixels`
/// reads them, through the same read-back, the device waited on at once.
///
/// # Panics
///
/// Panics if no GPU adapter is available.
pub fn read_image_pixels(image: &Image) -> crate::image_codecs::ImageFuture<valo::PixelBuffer> {
    crate::snapshot_delegate::read_image_pixels(&mut gpu(), image, |device| {
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the test GPU device answers");
    })
}

/// An image of `pixels` on the test device, which [`drawn_pixels`] draws: what a host's
/// `Platform::import_pixels` makes; none where the pixels do not upload.
///
/// # Panics
///
/// Panics if no GPU adapter is available.
pub fn imported_pixels(pixels: valo::PixelBuffer) -> Option<Image> {
    gpu().image_context().upload_pixels(pixels, false).ok()
}

/// `bytes` opened as a picture on the test device, which [`drawn_pixels`] draws: what a host's
/// `Platform::open_image_codec` answers, decoded by valo's software decoders, the same on every
/// machine.
///
/// # Panics
///
/// Panics if no GPU adapter is available.
pub fn open_image_codec(bytes: Arc<[u8]>) -> crate::ImageCodecFuture {
    let decoders: Vec<Box<dyn valo_codec::Decoder>> =
        vec![Box::new(valo_codec_software::SoftwareDecoder)];
    let loader = valo_codec::ImageLoader::new(gpu().image_context().clone(), decoders);
    crate::valo_images::open_image_codec(Some(loader), bytes)
}

/// `picture` drawn into an image of `size` on the test device, as a host's
/// `Platform::picture_to_image_sync` draws it.
///
/// # Panics
///
/// Panics if no GPU adapter is available.
pub fn picture_to_image(picture: &Picture, size: [u32; 2]) -> Image {
    gpu().display_list_to_image(picture, size)
}

fn gpu() -> MutexGuard<'static, Context> {
    GPU.get_or_init(|| Mutex::new(headless_context()))
        .lock()
        .expect("the test GPU device")
}

fn headless_context() -> Context {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("a GPU adapter for tests");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("a GPU device for tests");
    let mut context = Context::new(device, queue);
    context.set_hide_missing_glyphs(true);
    context
}

/// A composition layer no producer feeds, as the handle a compositor hands out. Enough to place
/// one in a scene and compare the placement.
pub fn idle_layer() -> Arc<dyn CompositionLayer> {
    Arc::new(IdleLayer)
}

/// What [`idle_layer`] hands out: presents go nowhere, so it shows nothing.
struct IdleLayer;

impl CompositionLayer for IdleLayer {
    fn present(
        &self,
        _buffer: Arc<dyn HardwareBuffer>,
        _children: Vec<(Arc<dyn CompositionLayer>, Placement)>,
        _released: Box<dyn FnOnce() + Send + Sync>,
    ) {
    }

    fn current_picture(&self) -> Option<crate::LayerPicture> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dispatcher, Instant, SnapshotDelegate};

    /// A host's dispatcher: work runs on a thread of its own, as a system queue runs it.
    struct ThreadDispatcher;

    impl Dispatcher for ThreadDispatcher {
        fn wake_at(&self, _timer_wakeup: Option<Instant>) {}

        fn wake_now(&self) {}

        #[allow(
            clippy::disallowed_methods,
            reason = "the test plays a host, which runs dispatched work on threads of its own"
        )]
        fn dispatch(&self, work: Box<dyn FnOnce() + Send>) {
            std::thread::spawn(work);
        }
    }

    #[test]
    fn a_host_reads_an_image_back_with_the_gpu_waited_on_off_the_calling_thread() {
        let image = solid_image([3, 2], [255, 0, 0, 255]);
        let snapshots = SnapshotDelegate::new(gpu().image_context().clone());
        let pixels = pollster::block_on(snapshots.read_pixels(&image, |device| {
            let device = device.clone();
            ThreadDispatcher.dispatch(Box::new(move || {
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
            }));
        }))
        .expect("the image reads back");
        assert_eq!(pixels.layout().size, [3, 2]);
        assert!(
            pixels
                .bytes()
                .chunks(4)
                .all(|pixel| pixel == [255, 0, 0, 255]),
            "red throughout"
        );
    }
}
