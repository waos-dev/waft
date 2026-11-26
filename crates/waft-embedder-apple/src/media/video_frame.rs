//! A video's frame on macOS and iOS as a frame reader hands it out (`frames`): the `CVPixelBuffer`
//! AVFoundation handed out, backed by an `IOSurface`, in 32-bit BGRA, whose memory is its surface,
//! a hardware buffer that holds the frame (`hardware_buffers`), which the app's GPU samples with no
//! copy through a texture imported over it (`gpu_interop`), and whose pixels are copied into the
//! app's memory while the surface is locked (`locked_pixels`). A picture the system drew into such
//! a buffer is one too (`drawn_frames`).

use std::sync::Arc;
use std::time::Duration;

use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    CVImageBufferGetCleanRect, CVPixelBuffer, CVPixelBufferGetHeight, CVPixelBufferGetWidth,
};
use waft_embedder::image_codecs::{ImageFuture, PixelBuffer};
use waft_embedder::{ColorRange, HardwareBuffer, PixelAspect, PixelRect, VideoColor, VideoFrame};

use crate::hardware_buffers::SurfaceBuffer;

/// One picture of a player, a camera or a decoder on macOS and iOS: the `CVPixelBuffer` the
/// system handed out, backed by an `IOSurface`: in 32-bit BGRA from a reader, and in the
/// decoder's own layout, 4:2:0 in two planes (NV12, or P010 for a deeper stream), from a decoder. Platform code reaches it from the
/// [`VideoFrame`] a reader answers by downcasting through `Any`:
///
/// ```
/// # fn platform_code(frame: std::sync::Arc<dyn waft_embedder::VideoFrame>) {
/// use waft_embedder_apple::media::CoreVideoFrame;
///
/// let frame: &dyn std::any::Any = &*frame;
/// if let Some(frame) = frame.downcast_ref::<CoreVideoFrame>() {
///     let _buffer = frame.pixel_buffer();
/// }
/// # }
/// ```
#[derive(Clone)]
pub struct CoreVideoFrame {
    buffer: CFRetained<CVPixelBuffer>,
    /// The turn it is shown at, in degrees clockwise, before `flip`.
    rotation: u32,
    /// Whether it is shown mirrored, after `rotation`.
    flip: bool,
    /// What its decoder said of it; none for a reader's frame, which says only what it is.
    decoded: Option<DecodedFacts>,
}

/// What a decoder says of a picture it put out.
#[derive(Clone, Copy)]
pub(crate) struct DecodedFacts {
    /// When it is shown, in nanoseconds on its stream's own timeline.
    pub(crate) timestamp: i64,
    pub(crate) duration: Option<Duration>,
    pub(crate) pixel_aspect: PixelAspect,
    pub(crate) color: VideoColor,
}

impl CoreVideoFrame {
    /// The frame of `buffer`, a pixel buffer backed by an `IOSurface`, in 32-bit BGRA, as a
    /// reader's output hands it out, shown as it is: what platform code that holds the system's
    /// own buffer, as WaOS does for a program, hands the app's codecs back.
    pub fn new(buffer: CFRetained<CVPixelBuffer>) -> CoreVideoFrame {
        CoreVideoFrame {
            buffer,
            rotation: 0,
            flip: false,
            decoded: None,
        }
    }

    /// The frame of `buffer`, a pixel buffer backed by an `IOSurface` in a decoder's own layout,
    /// 4:2:0 in two planes, NV12 or P010, as VideoToolbox hands it out, with what its decoder says of
    /// it.
    pub(crate) fn decoded(
        buffer: CFRetained<CVPixelBuffer>,
        facts: DecodedFacts,
    ) -> CoreVideoFrame {
        CoreVideoFrame {
            buffer,
            rotation: 0,
            flip: false,
            decoded: Some(facts),
        }
    }

    /// The frame shown turned `rotation` degrees clockwise, then mirrored where `flip` says, as
    /// a camera's frame is whose pixels its connection did not turn.
    pub fn shown_turned(self, rotation: u32, flip: bool) -> CoreVideoFrame {
        CoreVideoFrame {
            rotation: rotation % 360,
            flip,
            ..self
        }
    }

    /// The pixel buffer AVFoundation handed out. Retained, it holds the frame's slot in its pool,
    /// as the frame does.
    pub fn pixel_buffer(&self) -> &CVPixelBuffer {
        &self.buffer
    }
}

impl VideoFrame for CoreVideoFrame {
    fn width(&self) -> u32 {
        CVPixelBufferGetWidth(&self.buffer) as u32
    }

    fn height(&self) -> u32 {
        CVPixelBufferGetHeight(&self.buffer) as u32
    }

    /// Its surface's rows as they lie, BGRA and premultiplied, copied while it is locked for
    /// reading.
    fn read_pixels(&self) -> ImageFuture<PixelBuffer> {
        self.surface_buffer().read_pixels()
    }

    fn hardware_buffer(&self) -> Arc<dyn HardwareBuffer> {
        Arc::new(self.surface_buffer())
    }

    fn rotation(&self) -> u32 {
        self.rotation
    }

    fn flip(&self) -> bool {
        self.flip
    }

    fn timestamp(&self) -> Option<i64> {
        self.decoded.map(|decoded| decoded.timestamp)
    }

    fn duration(&self) -> Option<Duration> {
        self.decoded.and_then(|decoded| decoded.duration)
    }

    /// The buffer's clean aperture, which a decoder sets from its stream's cropping; all of a
    /// reader's frame.
    fn visible_rect(&self) -> PixelRect {
        let clean = CVImageBufferGetCleanRect(&self.buffer);
        PixelRect {
            x: clean.origin.x.round() as u32,
            y: clean.origin.y.round() as u32,
            width: clean.size.width.round() as u32,
            height: clean.size.height.round() as u32,
        }
    }

    fn pixel_aspect(&self) -> PixelAspect {
        match self.decoded {
            Some(decoded) => decoded.pixel_aspect,
            None => PixelAspect {
                width: 1,
                height: 1,
            },
        }
    }

    fn color(&self) -> VideoColor {
        match self.decoded {
            Some(decoded) => decoded.color,
            None => VideoColor {
                primaries: None,
                transfer: None,
                matrix: Some(VideoColor::RGB_MATRIX),
                range: Some(ColorRange::Full),
                chroma_location: None,
            },
        }
    }
}

impl CoreVideoFrame {
    /// The frame's surface, which holds the frame.
    fn surface_buffer(&self) -> SurfaceBuffer {
        SurfaceBuffer::of_frame(self.buffer.clone())
            .expect("a frame's pixel buffer is on an IOSurface in BGRA or a decoder's planes, as `CoreVideoFrame` takes it")
    }
}

// SAFETY: a pixel buffer is made to be shared between threads: its retain count is atomic, and
// nothing here writes its pixels. The binding cannot say so.
unsafe impl Send for CoreVideoFrame {}
unsafe impl Sync for CoreVideoFrame {}

/// A 64 by 48 frame as a frame reader hands one out: BGRA, on an `IOSurface`.
#[cfg(test)]
pub(crate) fn test_frame() -> CoreVideoFrame {
    test_frame_in(objc2_core_video::kCVPixelFormatType_32BGRA)
}

/// A 64 by 48 frame on an `IOSurface` in CoreVideo's `pixel_format`, as a decoder hands one out
/// in its own layout.
#[cfg(test)]
pub(crate) fn test_frame_in(pixel_format: u32) -> CoreVideoFrame {
    use std::ptr::NonNull;

    use objc2::rc::Retained;
    use objc2_core_foundation::CFDictionary;
    use objc2_core_video::{CVPixelBufferCreate, kCVReturnSuccess};

    let attributes = super::frames::pixel_buffer_attributes();
    let attributes: *const CFDictionary = Retained::as_ptr(&attributes).cast();
    let mut buffer = std::ptr::null_mut();
    // SAFETY: the readers' own attributes, an NSDictionary bridged to the CFDictionary it is, and
    // `buffer` a valid place for the answer.
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            64,
            48,
            pixel_format,
            Some(&*attributes),
            NonNull::from(&mut buffer),
        )
    };
    assert_eq!(status, kCVReturnSuccess);
    // SAFETY: a buffer made just now, handed over with its one retain.
    let buffer = unsafe { CFRetained::from_raw(NonNull::new(buffer).expect("a pixel buffer")) };
    CoreVideoFrame::new(buffer)
}
