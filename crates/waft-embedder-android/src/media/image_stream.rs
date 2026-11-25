//! Pictures as an `AImageReader` takes them: a stream of one size and format whose window a camera
//! or a decoder draws into, each picture an `AImage` lent with its slot in the reader's pool and
//! handed to what the stream was made with, on the reader's own thread: a camera's newest at each
//! delivery, a decoder's every one. A `PRIVATE` picture's memory is its `AHardwareBuffer`, which
//! the compositor shows and the GPU samples; a JPEG one's is the encoded photo.
//!
//! The invariant: a picture holds its slot until it is dropped, and holds the reader until then
//! too, so the reader is deleted only once its last picture is, never under one still shown.

// The image reader's pictures cross threads, which `ndk` cannot vouch for; this module does.
#![allow(unsafe_code)]

use std::sync::{Arc, Mutex, PoisonError, Weak};

use ndk::hardware_buffer::{HardwareBufferRef, HardwareBufferUsage};
use ndk::media::image_reader::{AcquireResult, Image, ImageFormat, ImageReader};
use ndk::media_error::MediaError;
use ndk::native_window::NativeWindow;
use waft_embedder::PixelRect;

/// A stream of pictures, its reader deleted once the stream and its last picture are dropped.
pub(crate) struct ImageStream(Arc<Reader>);

/// Which of the pictures that arrived since the last delivery a stream hands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// The newest alone, the others dropped: a camera's, whose reader shows the present.
    Newest,
    /// Each, in the order they arrived: a decoder's, every one of which is a frame.
    Each,
}

/// The reader, behind a lock only so that its listener can be set once it is shared.
struct Reader(Mutex<ImageReader>);

// SAFETY: an `AImageReader` guards its own state, and is acquired from on its listener's thread
// and deleted on whichever thread lets go of it last.
unsafe impl Send for Reader {}
unsafe impl Sync for Reader {}

impl ImageStream {
    /// A stream of `size` pictures in `format` for `usage`, at most `max_images` of them lent at
    /// once, each one `delivery` hands on given to `delivered`.
    pub(crate) fn new(
        size: [u32; 2],
        format: ImageFormat,
        usage: HardwareBufferUsage,
        max_images: i32,
        delivery: Delivery,
        delivered: impl Fn(StreamImage) + Send + 'static,
    ) -> Result<ImageStream, MediaError> {
        let reader =
            ImageReader::new_with_usage(size[0] as i32, size[1] as i32, format, usage, max_images)?;
        let reader = Arc::new(Reader(Mutex::new(reader)));
        let listened: Weak<Reader> = Arc::downgrade(&reader);
        reader
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .set_image_listener(Box::new(move |reader: &ImageReader| {
                let Some(held) = listened.upgrade() else {
                    return;
                };
                let lend = || match delivery {
                    Delivery::Newest => reader.acquire_latest_image(),
                    Delivery::Each => reader.acquire_next_image(),
                };
                while let Ok(AcquireResult::Image(image)) = lend() {
                    delivered(StreamImage {
                        image,
                        _reader: Arc::clone(&held),
                    });
                    if delivery == Delivery::Newest {
                        break;
                    }
                }
            }))?;
        Ok(ImageStream(reader))
    }

    /// The window the camera or decoder draws into the stream through.
    pub(crate) fn window(&self) -> Result<NativeWindow, MediaError> {
        self.0
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .window()
    }
}

/// One picture, lent with its slot in its reader's pool until dropped.
pub(crate) struct StreamImage {
    image: Image,
    _reader: Arc<Reader>,
}

// SAFETY: an `AImage` is read and deleted from any thread, one at a time, which owning it here
// makes sure of; its reader is kept while it lives.
unsafe impl Send for StreamImage {}
unsafe impl Sync for StreamImage {}

impl StreamImage {
    /// The picture's memory, a reference of its own on it.
    pub(crate) fn hardware_buffer(&self) -> Result<HardwareBufferRef, MediaError> {
        Ok(self.image.hardware_buffer()?.acquire())
    }

    /// The bytes of its one plane: a JPEG picture's encoded photo.
    pub(crate) fn encoded(&self) -> Result<Vec<u8>, MediaError> {
        Ok(self.image.plane_data(0)?.to_vec())
    }

    /// When it was drawn, in nanoseconds, as whatever drew it said: a decoder's, its chunk's time.
    pub(crate) fn timestamp(&self) -> Result<i64, MediaError> {
        self.image.timestamp()
    }

    /// The part of its memory that is picture; none where the reader says of no such part.
    pub(crate) fn visible_rect(&self) -> Option<PixelRect> {
        let crop = self.image.crop_rect().ok()?;
        Some(PixelRect {
            x: u32::try_from(crop.left).ok()?,
            y: u32::try_from(crop.top).ok()?,
            width: u32::try_from(crop.right - crop.left).ok()?,
            height: u32::try_from(crop.bottom - crop.top).ok()?,
        })
    }
}
