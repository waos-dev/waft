//! A picture GStreamer decoded, as the app holds it: a dmabuf in ARGB8888, Waft's hardware buffer
//! on Linux, with what its chunk and its stream say of it.
//!
//! A picture decoded on the video hardware is the decoder's own dmabuf, converted to ARGB8888 on
//! the GPU (`vapostproc`), and its sample is kept with it so the decoder does not reuse the memory
//! while the app holds the frame. A picture decoded on the processor is converted there
//! (`videoconvert`) and written into a dmabuf of Waft's from a small pool (`FramePool`): the one
//! copy a machine without video hardware makes, which a GPU then samples as any other frame.
//!
//! The invariant: a pooled dmabuf is written only while no frame holds it, so a frame's picture
//! never changes under the app.

use std::os::fd::BorrowedFd;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use waft_embedder::image_codecs::{
    AlphaType, ImageError, ImageErrorKind, ImageFuture, PixelBuffer, PixelFormat, PixelLayout,
};
use waft_embedder::{HardwareBuffer, HardwareBufferUsage, PixelAspect, VideoFrame};

use super::super::dmabuf::{DmabufBuffer, GbmAllocator, MOD_INVALID, MOD_LINEAR};
use super::super::gstreamer::{PictureFacts, Sample};

/// What a chunk said of its picture, carried to its frame.
#[derive(Clone, Copy)]
pub(super) struct ChunkFacts {
    pub(super) timestamp: i64,
    pub(super) duration: Option<u64>,
}

/// A decoded picture.
pub(super) struct DecodedFrame {
    buffer: Arc<DmabufBuffer>,
    /// The decoder's sample the buffer is, kept while the frame is; none for a pooled copy.
    sample: Option<Sample>,
    chunk: ChunkFacts,
    pixel_aspect: PixelAspect,
}

impl DecodedFrame {
    /// The frame of `sample`, the decoder's own dmabuf; `None` for a sample in other memory or a
    /// DRM format other than ARGB8888.
    pub(super) fn of_dmabuf(
        sample: Sample,
        facts: &PictureFacts,
        chunk: ChunkFacts,
    ) -> Option<DecodedFrame> {
        let modifier = modifier_of_argb8888(facts.drm_format.as_deref()?)?;
        let plane = sample.dmabuf_plane()?;
        let fd = duplicate(plane.fd)?;
        let buffer = DmabufBuffer::new(
            fd,
            [facts.width, facts.height],
            plane.stride,
            plane.offset,
            modifier,
            HardwareBufferUsage::GPU_SAMPLED,
        );
        Some(DecodedFrame {
            buffer: Arc::new(buffer),
            sample: Some(sample),
            chunk,
            pixel_aspect: aspect_of(facts),
        })
    }

    /// The frame of `sample`, BGRA in the processor's memory, written into a dmabuf of `pool`.
    pub(super) fn copied(
        sample: &Sample,
        facts: &PictureFacts,
        chunk: ChunkFacts,
        pool: &FramePool,
    ) -> Result<DecodedFrame, ImageError> {
        let buffer = pool.free_buffer(facts.width, facts.height)?;
        let row_bytes = facts.width as usize * 4;
        let written = sample.with_bytes(|bytes, stride| {
            let stride = if stride == 0 { row_bytes } else { stride };
            buffer.write_rows(bytes, stride, row_bytes, facts.height)
        });
        written.unwrap_or_else(|| {
            Err(ImageError::new(
                ImageErrorKind::Other,
                "the decoded picture did not map",
            ))
        })?;
        Ok(DecodedFrame {
            buffer,
            sample: None,
            chunk,
            pixel_aspect: aspect_of(facts),
        })
    }
}

impl VideoFrame for DecodedFrame {
    fn width(&self) -> u32 {
        self.buffer.width()
    }

    fn height(&self) -> u32 {
        self.buffer.height()
    }

    /// BGRA, as the frame holds it: read from the decoder's memory, mapped by GStreamer, for a
    /// picture of the video hardware, and from the pooled dmabuf otherwise.
    fn read_pixels(&self) -> ImageFuture<PixelBuffer> {
        let Some(sample) = &self.sample else {
            return self.buffer.read_pixels();
        };
        let (width, height) = (self.buffer.width(), self.buffer.height());
        let row_bytes = width as usize * 4;
        let read = sample.with_bytes(|bytes, stride| {
            let stride = if stride == 0 { row_bytes } else { stride };
            let mut rows = Vec::with_capacity(row_bytes * height as usize);
            for row in 0..height as usize {
                let start = row * stride;
                rows.extend_from_slice(bytes.get(start..start + row_bytes)?);
            }
            Some(rows)
        });
        let answer = match read.flatten() {
            Some(rows) => PixelBuffer::new(
                PixelLayout {
                    size: [width, height],
                    row_bytes: row_bytes as u32,
                    format: PixelFormat::Bgra8,
                    alpha: AlphaType::Premultiplied,
                },
                rows,
            )
            .map_err(|error| ImageError::new(ImageErrorKind::Other, format!("{error:?}"))),
            None => Err(ImageError::new(
                ImageErrorKind::Other,
                "the decoded picture did not map for reading",
            )),
        };
        Box::pin(std::future::ready(answer))
    }

    fn hardware_buffer(&self) -> Arc<dyn HardwareBuffer> {
        Arc::clone(&self.buffer) as Arc<dyn HardwareBuffer>
    }

    fn rotation(&self) -> u32 {
        0
    }

    fn flip(&self) -> bool {
        false
    }

    fn timestamp(&self) -> Option<i64> {
        Some(self.chunk.timestamp)
    }

    fn duration(&self) -> Option<Duration> {
        self.chunk.duration.map(Duration::from_nanos)
    }

    fn pixel_aspect(&self) -> PixelAspect {
        self.pixel_aspect
    }
}

/// The dmabufs pictures decoded on the processor are written into, each reused once no frame
/// holds it.
pub(super) struct FramePool {
    allocator: GbmAllocator,
    buffers: Mutex<Vec<Arc<DmabufBuffer>>>,
}

impl FramePool {
    /// A pool on Waft's allocator; `None` where no render node opens.
    pub(super) fn new() -> Option<FramePool> {
        Some(FramePool {
            allocator: GbmAllocator::new()?,
            buffers: Mutex::new(Vec::new()),
        })
    }

    /// A buffer of `width` by `height` no frame holds: one of the pool's, or a new one.
    fn free_buffer(&self, width: u32, height: u32) -> Result<Arc<DmabufBuffer>, ImageError> {
        let mut buffers = self.buffers.lock().unwrap_or_else(PoisonError::into_inner);
        buffers.retain(|buffer| buffer.width() == width && buffer.height() == height);
        if let Some(free) = buffers.iter().find(|buffer| Arc::strong_count(buffer) == 1) {
            return Ok(Arc::clone(free));
        }
        let usage = HardwareBufferUsage {
            cpu_read: true,
            ..HardwareBufferUsage::GPU_SAMPLED
        };
        let buffer = Arc::new(self.allocator.allocate_linear(width, height, usage)?);
        buffers.push(Arc::clone(&buffer));
        Ok(buffer)
    }
}

/// The modifier of `drm_format`, as GStreamer's `drm-format` writes one: `AR24` for a linear
/// ARGB8888 buffer, `AR24:0x…` with its modifier; `None` for another format.
fn modifier_of_argb8888(drm_format: &str) -> Option<u64> {
    let (fourcc, modifier) = match drm_format.split_once(':') {
        Some((fourcc, modifier)) => (fourcc, Some(modifier)),
        None => (drm_format, None),
    };
    if fourcc != "AR24" {
        return None;
    }
    match modifier {
        None => Some(MOD_LINEAR),
        Some(hex) => {
            let modifier = u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok()?;
            (modifier != MOD_INVALID).then_some(modifier)
        }
    }
}

fn aspect_of(facts: &PictureFacts) -> PixelAspect {
    let (width, height) = facts.pixel_aspect;
    PixelAspect { width, height }
}

/// A descriptor of the file `fd` names, for a buffer that outlives the sample's borrow.
fn duplicate(fd: BorrowedFd<'_>) -> Option<std::os::fd::OwnedFd> {
    fd.try_clone_to_owned().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argb8888_reads_its_modifier_and_other_formats_are_none() {
        assert_eq!(modifier_of_argb8888("AR24"), Some(MOD_LINEAR));
        assert_eq!(
            modifier_of_argb8888("AR24:0x0100000000000002"),
            Some(0x0100_0000_0000_0002)
        );
        assert_eq!(modifier_of_argb8888("NV12:0x0"), None);
        assert_eq!(modifier_of_argb8888("AR24:0x00ffffffffffffff"), None);
    }
}
