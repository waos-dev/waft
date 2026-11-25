//! Pictures in shared memory on Wayland: a `memfd` of ARGB8888 rows (BGRA in memory) that its
//! producer writes with the CPU and the compositor maps as a `wl_shm` pool, the one buffer every
//! Wayland compositor shows, as Chromium's software output does. A producer without a GPU the
//! compositor imports from presents these where it would present dmabufs
//! (`AcceptedBuffers::shared_memory`).
//!
//! The invariant a later editor must keep: a buffer is checked against its memory's size as it is
//! made, since a `wl_shm` pool or buffer that does not fit is a protocol error, which ends the
//! whole connection.
//!
//! Reading the memory maps it, which is C's, which is why this module allows `unsafe`.
#![allow(unsafe_code)]

use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::{Arc, Mutex};

use waft_embedder::image_codecs::{
    AlphaType, ImageError, ImageErrorKind, ImageFuture, PixelBuffer, PixelFormat, PixelLayout,
};
use waft_embedder::{HardwareBuffer, HardwareBufferUsage};
use wayland_client::Proxy;
use wayland_client::protocol::wl_buffer::WlBuffer;

use super::wayland::{BufferData, Wayland};

/// `height` rows of `stride` bytes of ARGB8888 in shared memory, and the `wl_buffer` Waft made of
/// it. The compositor reads it from a present until the `wl_buffer.release` owed to it.
pub struct ShmBuffer {
    fd: OwnedFd,
    width: u32,
    height: u32,
    stride: u32,
    /// The `wl_buffer` the compositor made of it, made at its first present.
    wl_buffer: Mutex<Option<WlBuffer>>,
}

impl ShmBuffer {
    /// A buffer over `fd`, shared memory another host made and writes, `width` by `height` pixels
    /// of `stride` bytes a row from its start; `None` where the memory is smaller than that, a
    /// side has no pixels, a row is shorter than its pixels, or the whole is past what `wl_shm`
    /// takes.
    pub fn new(fd: OwnedFd, [width, height]: [u32; 2], stride: u32) -> Option<ShmBuffer> {
        let length = u64::from(stride) * u64::from(height);
        let fits = width > 0
            && height > 0
            && u64::from(stride) >= u64::from(width) * 4
            && length <= i32::MAX as u64
            && length <= memory_size(&fd)?;
        fits.then(|| ShmBuffer {
            fd,
            width,
            height,
            stride,
            wl_buffer: Mutex::new(None),
        })
    }

    pub fn fd(&self) -> &OwnedFd {
        &self.fd
    }

    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// The bytes the buffer covers, which fit in an `i32`.
    pub(crate) fn length(&self) -> i32 {
        (self.stride * self.height) as i32
    }

    /// The `wl_buffer` the compositor made of this buffer, made at the first call; `None` where
    /// the compositor offers no shared memory.
    pub(crate) fn wl_buffer(&self, wayland: &Wayland) -> Option<(WlBuffer, Arc<BufferData>)> {
        let mut made = self.wl_buffer.lock().unwrap();
        if made.is_none() {
            *made = wayland.shm_wl_buffer(self);
        }
        let buffer = made.clone()?;
        let data = buffer.data::<Arc<BufferData>>()?.clone();
        Some((buffer, data))
    }

    fn read_mapped(&self) -> Result<PixelBuffer, ImageError> {
        let row_bytes = self.width * 4;
        let length = self.length() as usize;
        let failed = |what: &str| {
            ImageError::new(
                ImageErrorKind::Other,
                format!("{what}: {}", std::io::Error::last_os_error()),
            )
        };
        let mut rows = Vec::with_capacity(row_bytes as usize * self.height as usize);
        // SAFETY: the memory is at least `length` bytes (`new` checked), mapped shared for reading
        // and unmapped before this returns.
        unsafe {
            let mapped = libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ,
                libc::MAP_SHARED,
                self.fd.as_raw_fd(),
                0,
            );
            if mapped == libc::MAP_FAILED {
                return Err(failed("mapping the shared memory"));
            }
            let bytes = std::slice::from_raw_parts(mapped.cast::<u8>(), length);
            for row in 0..self.height as usize {
                let start = row * self.stride as usize;
                rows.extend_from_slice(&bytes[start..start + row_bytes as usize]);
            }
            libc::munmap(mapped, length);
        }
        let layout = PixelLayout {
            size: [self.width, self.height],
            row_bytes,
            format: PixelFormat::Bgra8,
            alpha: AlphaType::Premultiplied,
        };
        PixelBuffer::new(layout, rows)
            .map_err(|error| ImageError::new(ImageErrorKind::Other, format!("{error:?}")))
    }
}

/// The size of the memory behind `fd`; `None` where it cannot be read.
fn memory_size(fd: &OwnedFd) -> Option<u64> {
    std::fs::File::from(fd.try_clone().ok()?)
        .metadata()
        .ok()
        .map(|metadata| metadata.len())
}

impl Drop for ShmBuffer {
    /// The compositor keeps showing what it has of a `wl_buffer` destroyed while shown.
    fn drop(&mut self) {
        if let Some(buffer) = self.wl_buffer.get_mut().unwrap().take() {
            buffer.destroy();
        }
    }
}

impl HardwareBuffer for ShmBuffer {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    /// Read by the CPU, and by the compositor; no GPU imports it.
    fn usage(&self) -> HardwareBufferUsage {
        HardwareBufferUsage {
            cpu_read: true,
            ..HardwareBufferUsage::default()
        }
    }

    fn read_pixels(&self) -> ImageFuture<PixelBuffer> {
        Box::pin(std::future::ready(self.read_mapped()))
    }
}
