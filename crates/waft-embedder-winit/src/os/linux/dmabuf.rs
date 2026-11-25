//! Hardware buffers on Linux: a dmabuf of one plane in ARGB8888 (BGRA in memory), what a Wayland
//! compositor shows as a `wl_buffer` and a Vulkan device imports. Waft's allocator makes one with
//! GBM on the first render node, linear, and exports it read-write; a dmabuf another host made,
//! such as a program's frame the WaOS engine allocated, is wrapped as it is ([`DmabufBuffer::new`]).
//!
//! The invariants a later editor must keep: a buffer is exported with `DRM_RDWR`, since
//! `gbm_bo_get_fd` exports read-only and a driver that maps the memory, as lavapipe does, refuses
//! such an fd (`experiments/2026-11-13-linux-dmabuf-subsurfaces`, row 1); and the CPU reads the
//! memory only between `DMA_BUF_IOCTL_SYNC`'s start and end.
//!
//! GBM is loaded at run time, so a build needs no development package, and its calls, the DRM
//! ioctls and the mapping are C's, which is why this module allows `unsafe`.
#![allow(unsafe_code)]

use std::ffi::{c_int, c_void};
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{Arc, Mutex, OnceLock};

use waft_embedder::image_codecs::{
    AlphaType, ImageError, ImageErrorKind, ImageFuture, PixelBuffer, PixelFormat, PixelLayout,
};
use waft_embedder::{HardwareBuffer, HardwareBufferAllocator, HardwareBufferUsage};
use wayland_client::Proxy;
use wayland_client::protocol::wl_buffer::WlBuffer;

use super::wayland::{BufferData, Wayland};

/// `DRM_FORMAT_ARGB8888`, BGRA in memory.
pub(crate) const ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
pub(crate) const MOD_LINEAR: u64 = 0;
/// The driver's implicit layout, which a compositor may offer alone.
pub(crate) const MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

/// The largest side Waft allocates, as Vulkan devices make a texture of at least.
const LARGEST_SIDE: u32 = 8192;

/// One dmabuf of one plane in ARGB8888, and what Waft has made of it: its `wl_buffer`.
pub struct DmabufBuffer {
    fd: OwnedFd,
    width: u32,
    height: u32,
    stride: u32,
    offset: u32,
    modifier: u64,
    usage: HardwareBufferUsage,
    /// The `wl_buffer` the compositor made of it, made at its first present.
    wl_buffer: Mutex<Option<WlBuffer>>,
    /// The GBM buffer object behind it, where Waft allocated it.
    _allocation: Option<GbmBufferObject>,
}

impl DmabufBuffer {
    /// A dmabuf another host made: `fd`, `width` by `height` ARGB8888 pixels, `stride` bytes a
    /// row from `offset`, in the layout `modifier` names, for `usage`.
    pub fn new(
        fd: OwnedFd,
        [width, height]: [u32; 2],
        stride: u32,
        offset: u32,
        modifier: u64,
        usage: HardwareBufferUsage,
    ) -> DmabufBuffer {
        DmabufBuffer {
            fd,
            width,
            height,
            stride,
            offset,
            modifier,
            usage,
            wl_buffer: Mutex::new(None),
            _allocation: None,
        }
    }

    pub fn fd(&self) -> &OwnedFd {
        &self.fd
    }

    pub fn stride(&self) -> u32 {
        self.stride
    }

    pub fn offset(&self) -> u32 {
        self.offset
    }

    pub fn modifier(&self) -> u64 {
        self.modifier
    }

    /// The `wl_buffer` the compositor made of this buffer, made at the first call; `None` where it
    /// refused it.
    pub(crate) fn wl_buffer(&self, wayland: &Wayland) -> Option<(WlBuffer, Arc<BufferData>)> {
        let mut made = self.wl_buffer.lock().unwrap();
        if made.is_none() {
            *made = wayland.wl_buffer(self);
        }
        let buffer = made.clone()?;
        let data = buffer.data::<Arc<BufferData>>()?.clone();
        Some((buffer, data))
    }

    fn read_mapped(&self) -> Result<PixelBuffer, ImageError> {
        if !self.usage.cpu_read {
            return Err(ImageError::new(
                ImageErrorKind::Unsupported,
                "the buffer was not allocated for the CPU to read",
            ));
        }
        if self.modifier != MOD_LINEAR {
            return Err(ImageError::new(
                ImageErrorKind::Unsupported,
                format!(
                    "the CPU reads only a linear buffer, not modifier {:#x}",
                    self.modifier
                ),
            ));
        }
        let row_bytes = self.width * 4;
        let mut rows = Vec::with_capacity(row_bytes as usize * self.height as usize);
        let mapping = Mapping::new(
            &self.fd,
            self.offset,
            self.stride,
            self.height,
            Access::Read,
        )?;
        let bytes = mapping.bytes();
        for row in 0..self.height as usize {
            let start = row * self.stride as usize;
            rows.extend_from_slice(&bytes[start..start + row_bytes as usize]);
        }
        drop(mapping);
        let layout = PixelLayout {
            size: [self.width, self.height],
            row_bytes,
            format: PixelFormat::Bgra8,
            alpha: AlphaType::Premultiplied,
        };
        PixelBuffer::new(layout, rows)
            .map_err(|error| ImageError::new(ImageErrorKind::Other, format!("{error:?}")))
    }

    /// Writes `height` rows of `row_bytes` each from `rows`, one after another `source_stride`
    /// bytes apart, over the buffer's own rows from the top: what the CPU puts in a linear buffer
    /// Waft allocated, as a picture decoded on the processor.
    pub(crate) fn write_rows(
        &self,
        rows: &[u8],
        source_stride: usize,
        row_bytes: usize,
        height: u32,
    ) -> Result<(), ImageError> {
        if self.modifier != MOD_LINEAR {
            return Err(ImageError::new(
                ImageErrorKind::Unsupported,
                format!(
                    "the CPU writes only a linear buffer, not modifier {:#x}",
                    self.modifier
                ),
            ));
        }
        let height = height.min(self.height) as usize;
        let row_bytes = row_bytes.min(self.width as usize * 4);
        if rows.len() < source_stride * height.saturating_sub(1) + row_bytes {
            return Err(ImageError::new(
                ImageErrorKind::Other,
                "fewer bytes than the rows written",
            ));
        }
        let mut mapping = Mapping::new(
            &self.fd,
            self.offset,
            self.stride,
            self.height,
            Access::Write,
        )?;
        let bytes = mapping.bytes_mut();
        for row in 0..height {
            let source = &rows[row * source_stride..row * source_stride + row_bytes];
            let target = row * self.stride as usize;
            bytes[target..target + row_bytes].copy_from_slice(source);
        }
        Ok(())
    }
}

impl Drop for DmabufBuffer {
    /// The compositor keeps showing what it has of a `wl_buffer` destroyed while shown.
    fn drop(&mut self) {
        if let Some(buffer) = self.wl_buffer.get_mut().unwrap().take() {
            buffer.destroy();
        }
    }
}

impl HardwareBuffer for DmabufBuffer {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn usage(&self) -> HardwareBufferUsage {
        self.usage
    }

    fn read_pixels(&self) -> ImageFuture<PixelBuffer> {
        Box::pin(std::future::ready(self.read_mapped()))
    }
}

/// Whether the CPU reads a mapped buffer or writes it.
#[derive(Clone, Copy)]
enum Access {
    Read,
    Write,
}

/// A dmabuf's bytes from `offset`, mapped shared for the CPU between `DMA_BUF_IOCTL_SYNC`'s
/// start and end, which the mapping's end ends and unmaps.
struct Mapping<'a> {
    fd: &'a OwnedFd,
    address: *mut libc::c_void,
    length: usize,
    offset: usize,
    access: Access,
}

impl<'a> Mapping<'a> {
    fn new(
        fd: &'a OwnedFd,
        offset: u32,
        stride: u32,
        height: u32,
        access: Access,
    ) -> Result<Mapping<'a>, ImageError> {
        let length = offset as usize + stride as usize * height as usize;
        let protection = match access {
            Access::Read => libc::PROT_READ,
            Access::Write => libc::PROT_READ | libc::PROT_WRITE,
        };
        // SAFETY: a live dmabuf fd at least `length` bytes long, mapped shared; the mapping is
        // unmapped when it ends.
        let address = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                protection,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if address == libc::MAP_FAILED {
            return Err(ImageError::new(
                ImageErrorKind::Other,
                format!("mapping the dmabuf: {}", std::io::Error::last_os_error()),
            ));
        }
        let mapping = Mapping {
            fd,
            address,
            length,
            offset: offset as usize,
            access,
        };
        mapping.sync(SYNC_START);
        Ok(mapping)
    }

    /// The buffer's bytes from its offset.
    fn bytes(&self) -> &[u8] {
        // SAFETY: `length` bytes mapped at `address`, of which `offset` lies within; the
        // mapping lives as long as `self`.
        unsafe {
            &std::slice::from_raw_parts(self.address.cast::<u8>(), self.length)[self.offset..]
        }
    }

    /// The buffer's bytes from its offset, for writing: a mapping for writing only.
    fn bytes_mut(&mut self) -> &mut [u8] {
        assert!(
            matches!(self.access, Access::Write),
            "a mapping for reading is not written"
        );
        // SAFETY: as `bytes`, mapped writable, and borrowed once through `&mut self`.
        unsafe {
            &mut std::slice::from_raw_parts_mut(self.address.cast::<u8>(), self.length)
                [self.offset..]
        }
    }

    fn sync(&self, step: u64) {
        let access = match self.access {
            Access::Read => SYNC_READ,
            Access::Write => SYNC_READ | SYNC_WRITE,
        };
        let mut sync = DmaBufSync {
            flags: step | access,
        };
        // SAFETY: `DMA_BUF_IOCTL_SYNC` on a live dmabuf fd, with the struct it takes.
        unsafe { libc::ioctl(self.fd.as_raw_fd(), DMA_BUF_IOCTL_SYNC as _, &mut sync) };
    }
}

impl Drop for Mapping<'_> {
    fn drop(&mut self) {
        self.sync(SYNC_END);
        // SAFETY: the mapping `new` made, unmapped once.
        unsafe { libc::munmap(self.address, self.length) };
    }
}

#[repr(C)]
struct DmaBufSync {
    flags: u64,
}
const SYNC_READ: u64 = 1;
const SYNC_WRITE: u64 = 2;
const SYNC_START: u64 = 0;
const SYNC_END: u64 = 4;
const DMA_BUF_IOCTL_SYNC: u64 = ioctl_code(1, b'b', 0, size_of::<DmaBufSync>());

/// Linux's `_IOC`: `direction` 1 writes, 3 reads and writes.
const fn ioctl_code(direction: u64, kind: u8, number: u8, size: usize) -> u64 {
    (direction << 30) | ((size as u64) << 16) | ((kind as u64) << 8) | number as u64
}

/// Waft's allocator on Linux: GBM on the first render node, linear ARGB8888, exported read-write.
pub(crate) struct GbmAllocator {
    gbm: &'static Gbm,
}

impl GbmAllocator {
    /// The allocator, where a render node opens and GBM makes a device on it.
    pub(crate) fn new() -> Option<GbmAllocator> {
        Gbm::get().map(|gbm| GbmAllocator { gbm })
    }

    /// A linear ARGB8888 buffer of `width` by `height`, for `usage`: what the CPU writes into,
    /// as [`DmabufBuffer::write_rows`] does.
    pub(crate) fn allocate_linear(
        &self,
        width: u32,
        height: u32,
        usage: HardwareBufferUsage,
    ) -> Result<DmabufBuffer, ImageError> {
        if width == 0 || height == 0 || width > LARGEST_SIDE || height > LARGEST_SIDE {
            return Err(ImageError::new(
                ImageErrorKind::TooLarge,
                format!("a side of no pixels or past {LARGEST_SIDE}"),
            ));
        }
        self.gbm.allocate(width, height, usage)
    }
}

impl HardwareBufferAllocator for GbmAllocator {
    fn allocate(
        &self,
        width: u32,
        height: u32,
        format: PixelFormat,
        usage: HardwareBufferUsage,
    ) -> Result<Arc<dyn HardwareBuffer>, ImageError> {
        if format != PixelFormat::Bgra8 {
            return Err(ImageError::new(
                ImageErrorKind::Unsupported,
                "Linux allocates hardware buffers in BGRA",
            ));
        }
        if width == 0 || height == 0 {
            return Err(ImageError::new(
                ImageErrorKind::Other,
                "a side of no pixels",
            ));
        }
        if width > LARGEST_SIDE || height > LARGEST_SIDE {
            return Err(ImageError::new(
                ImageErrorKind::TooLarge,
                format!("a side past {LARGEST_SIDE} pixels"),
            ));
        }
        let buffer = self.gbm.allocate(width, height, usage)?;
        Ok(Arc::new(buffer))
    }
}

/// libgbm's calls, loaded once, with a device on the first render node.
struct Gbm {
    node: File,
    device: usize,
    create: unsafe extern "C" fn(*mut c_void, u32, u32, u32, u32) -> *mut c_void,
    destroy: unsafe extern "C" fn(*mut c_void),
    stride: unsafe extern "C" fn(*mut c_void) -> u32,
    offset: unsafe extern "C" fn(*mut c_void, c_int) -> u32,
    handle: unsafe extern "C" fn(*mut c_void) -> u64,
}

const GBM_BO_USE_RENDERING: u32 = 1 << 2;
const GBM_BO_USE_LINEAR: u32 = 1 << 4;

/// A GBM buffer object, destroyed when the buffer goes.
struct GbmBufferObject(usize);

impl Drop for GbmBufferObject {
    fn drop(&mut self) {
        if let Some(gbm) = Gbm::get() {
            // SAFETY: a buffer object this device made, destroyed once.
            unsafe { (gbm.destroy)(self.0 as *mut c_void) };
        }
    }
}

#[repr(C)]
struct PrimeHandle {
    handle: u32,
    flags: u32,
    fd: i32,
}
const DRM_IOCTL_PRIME_HANDLE_TO_FD: u64 = ioctl_code(3, b'd', 0x2d, size_of::<PrimeHandle>());

impl Gbm {
    fn get() -> Option<&'static Gbm> {
        static GBM: OnceLock<Option<Gbm>> = OnceLock::new();
        GBM.get_or_init(|| match Gbm::load() {
            Ok(gbm) => Some(gbm),
            Err(why) => {
                eprintln!("waft: no hardware buffers on this machine: {why}");
                None
            }
        })
        .as_ref()
    }

    fn load() -> Result<Gbm, String> {
        let node = first_render_node()?;
        // SAFETY: libgbm's documented entry points, with their C signatures; the library is kept
        // loaded for the process's life.
        unsafe {
            let library = Box::leak(Box::new(
                libloading::Library::new("libgbm.so.1").map_err(|error| error.to_string())?,
            ));
            let create_device: unsafe extern "C" fn(c_int) -> *mut c_void =
                symbol(library, b"gbm_create_device")?;
            let device = create_device(node.as_raw_fd());
            if device.is_null() {
                return Err("GBM made no device on the render node".into());
            }
            Ok(Gbm {
                device: device as usize,
                create: symbol(library, b"gbm_bo_create")?,
                destroy: symbol(library, b"gbm_bo_destroy")?,
                stride: symbol(library, b"gbm_bo_get_stride")?,
                offset: symbol(library, b"gbm_bo_get_offset")?,
                handle: symbol(library, b"gbm_bo_get_handle")?,
                node,
            })
        }
    }

    /// A linear ARGB8888 buffer object, exported read-write.
    fn allocate(
        &self,
        width: u32,
        height: u32,
        usage: HardwareBufferUsage,
    ) -> Result<DmabufBuffer, ImageError> {
        // SAFETY: the device made in `load`; the buffer object is owned by the `DmabufBuffer`
        // from here, and its handle is exported through the render node it was made on.
        unsafe {
            let object = (self.create)(
                self.device as *mut c_void,
                width,
                height,
                ARGB8888,
                GBM_BO_USE_RENDERING | GBM_BO_USE_LINEAR,
            );
            if object.is_null() {
                return Err(ImageError::new(
                    ImageErrorKind::OutOfMemory,
                    format!(
                        "GBM allocated no buffer: {}",
                        std::io::Error::last_os_error()
                    ),
                ));
            }
            let allocation = GbmBufferObject(object as usize);
            let mut prime = PrimeHandle {
                handle: (self.handle)(object) as u32,
                flags: (libc::O_CLOEXEC | libc::O_RDWR) as u32,
                fd: -1,
            };
            if libc::ioctl(
                self.node.as_raw_fd(),
                DRM_IOCTL_PRIME_HANDLE_TO_FD as _,
                &mut prime,
            ) != 0
            {
                return Err(ImageError::new(
                    ImageErrorKind::Other,
                    format!("exporting the buffer: {}", std::io::Error::last_os_error()),
                ));
            }
            let mut buffer = DmabufBuffer::new(
                OwnedFd::from_raw_fd(prime.fd),
                [width, height],
                (self.stride)(object),
                (self.offset)(object, 0),
                MOD_LINEAR,
                usage,
            );
            buffer._allocation = Some(allocation);
            Ok(buffer)
        }
    }
}

// SAFETY: GBM's device and buffer objects are used from any thread only through calls libgbm
// makes thread-safe for distinct buffer objects; the device is only read after `load`.
unsafe impl Send for Gbm {}
unsafe impl Sync for Gbm {}

/// The function `name` of `library`, of the type the caller names.
///
/// # Safety
///
/// `T` must be the C signature `name` has.
unsafe fn symbol<T: Copy>(library: &libloading::Library, name: &[u8]) -> Result<T, String> {
    // SAFETY: the caller names the symbol's type.
    unsafe { library.get::<T>(name) }
        .map(|symbol| *symbol)
        .map_err(|error| error.to_string())
}

/// The first DRM render node that opens for reading and writing.
fn first_render_node() -> Result<File, String> {
    (128..192)
        .map(|minor| format!("/dev/dri/renderD{minor}"))
        .find_map(|path| OpenOptions::new().read(true).write(true).open(path).ok())
        .ok_or_else(|| "no DRM render node opens".into())
}
