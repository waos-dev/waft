//! GStreamer, the system's media framework on Linux, as much of it as the video decoders use: a
//! pipeline made from a description, its source the app pushes buffers into (`appsrc`) and its
//! sink the app pulls samples from (`appsink`), buffers with a reference timestamp to carry a
//! chunk's number through the decoder, and a sample's memory, mapped or as a dmabuf.
//!
//! GStreamer is loaded at run time, as GBM is (`dmabuf`), so a build needs no development package
//! and an app starts where GStreamer is missing: [`Gst::get`] answers `None` there. Its calls are
//! C's, and its structs are read where its headers expose them as fields, which is why this module
//! allows `unsafe`.
//!
//! The invariants a later editor must keep: every object or mini-object this module gets with
//! "transfer full" is wrapped at once in a type that unrefs it once when dropped; and a struct read
//! as a field (`MapInfo`, `VideoMeta`, `ReferenceTimestampMeta`, `GError`) is laid out exactly as
//! GStreamer 1.x's headers declare it, which its ABI keeps across 1.x.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::os::fd::{BorrowedFd, RawFd};
use std::ptr::{NonNull, null_mut};
use std::sync::OnceLock;
use std::time::Duration;

/// `GST_CLOCK_TIME_NONE`.
const CLOCK_TIME_NONE: u64 = u64::MAX;
/// `GST_STATE_NULL` and `GST_STATE_PLAYING`.
const STATE_NULL: c_int = 1;
const STATE_PLAYING: c_int = 4;
/// `GST_STATE_CHANGE_FAILURE`.
const STATE_CHANGE_FAILURE: c_int = 0;
/// `GST_MESSAGE_ERROR`.
const MESSAGE_ERROR: c_uint = 1 << 1;
/// `GST_MAP_READ`.
const MAP_READ: c_uint = 1;
/// `GST_FLOW_OK`.
const FLOW_OK: c_int = 0;

/// GStreamer's calls, loaded once.
pub(crate) struct Gst {
    parse_launch: unsafe extern "C" fn(*const c_char, *mut *mut GError) -> *mut c_void,
    object_ref_sink: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    object_unref: unsafe extern "C" fn(*mut c_void),
    mini_object_unref: unsafe extern "C" fn(*mut c_void),
    bin_get_by_name: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    element_set_state: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    element_get_bus: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    bus_timed_pop_filtered: unsafe extern "C" fn(*mut c_void, u64, c_uint) -> *mut c_void,
    message_parse_error: unsafe extern "C" fn(*mut c_void, *mut *mut GError, *mut *mut c_char),
    element_factory_find: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    caps_from_string: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    caps_get_structure: unsafe extern "C" fn(*mut c_void, c_uint) -> *mut c_void,
    structure_get_int: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_int) -> c_int,
    structure_get_fraction:
        unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_int, *mut c_int) -> c_int,
    structure_get_string: unsafe extern "C" fn(*mut c_void, *const c_char) -> *const c_char,
    buffer_new_memdup: unsafe extern "C" fn(*const c_void, usize) -> *mut c_void,
    buffer_add_reference_timestamp_meta:
        unsafe extern "C" fn(*mut c_void, *mut c_void, u64, u64) -> *mut c_void,
    buffer_get_reference_timestamp_meta:
        unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut ReferenceTimestampMeta,
    buffer_map: unsafe extern "C" fn(*mut c_void, *mut MapInfo, c_uint) -> c_int,
    buffer_unmap: unsafe extern "C" fn(*mut c_void, *mut MapInfo),
    buffer_n_memory: unsafe extern "C" fn(*mut c_void) -> c_uint,
    buffer_peek_memory: unsafe extern "C" fn(*mut c_void, c_uint) -> *mut c_void,
    sample_get_buffer: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    sample_get_caps: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    app_src_push_buffer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    app_src_end_of_stream: unsafe extern "C" fn(*mut c_void) -> c_int,
    app_src_set_caps: unsafe extern "C" fn(*mut c_void, *mut c_void),
    app_sink_try_pull_sample: unsafe extern "C" fn(*mut c_void, u64) -> *mut c_void,
    app_sink_is_eos: unsafe extern "C" fn(*mut c_void) -> c_int,
    buffer_get_video_meta: unsafe extern "C" fn(*mut c_void) -> *mut VideoMeta,
    is_dmabuf_memory: unsafe extern "C" fn(*mut c_void) -> c_int,
    dmabuf_memory_get_fd: unsafe extern "C" fn(*mut c_void) -> c_int,
    error_free: unsafe extern "C" fn(*mut GError),
    free: unsafe extern "C" fn(*mut c_void),
}

/// `GError`, as glib declares it.
#[repr(C)]
struct GError {
    domain: u32,
    code: c_int,
    message: *mut c_char,
}

/// `GstMapInfo`, as GStreamer 1.x declares it.
#[repr(C)]
struct MapInfo {
    memory: *mut c_void,
    flags: c_uint,
    data: *mut u8,
    size: usize,
    maxsize: usize,
    user_data: [*mut c_void; 4],
    reserved: [*mut c_void; 4],
}

/// `GstMeta`, the head of every meta.
#[repr(C)]
struct Meta {
    flags: c_uint,
    info: *const c_void,
}

/// `GstReferenceTimestampMeta`.
#[repr(C)]
struct ReferenceTimestampMeta {
    parent: Meta,
    reference: *mut c_void,
    timestamp: u64,
    duration: u64,
}

/// `GstVideoMeta`, as far as its strides.
#[repr(C)]
struct VideoMeta {
    meta: Meta,
    buffer: *mut c_void,
    flags: c_uint,
    format: c_uint,
    id: c_uint,
    width: c_uint,
    height: c_uint,
    n_planes: c_uint,
    offset: [usize; 4],
    stride: [c_int; 4],
}

impl Gst {
    /// GStreamer, loaded and initialized once; `None` where it is missing or does not start.
    pub(crate) fn get() -> Option<&'static Gst> {
        static GST: OnceLock<Option<Gst>> = OnceLock::new();
        GST.get_or_init(|| match Gst::load() {
            Ok(gst) => Some(gst),
            Err(why) => {
                eprintln!("waft: no video decoders on this machine: {why}");
                None
            }
        })
        .as_ref()
    }

    fn load() -> Result<Gst, String> {
        // SAFETY: GStreamer's and glib's documented entry points, with their C signatures; the
        // libraries are kept loaded for the process's life, and GStreamer is initialized once,
        // before any other call.
        unsafe {
            let core = library("libgstreamer-1.0.so.0")?;
            let app = library("libgstapp-1.0.so.0")?;
            let video = library("libgstvideo-1.0.so.0")?;
            let allocators = library("libgstallocators-1.0.so.0")?;
            let glib = library("libglib-2.0.so.0")?;
            let init_check: unsafe extern "C" fn(
                *mut c_int,
                *mut *mut *mut c_char,
                *mut *mut GError,
            ) -> c_int = symbol(core, b"gst_init_check")?;
            let gst = Gst {
                parse_launch: symbol(core, b"gst_parse_launch")?,
                object_ref_sink: symbol(core, b"gst_object_ref_sink")?,
                object_unref: symbol(core, b"gst_object_unref")?,
                mini_object_unref: symbol(core, b"gst_mini_object_unref")?,
                bin_get_by_name: symbol(core, b"gst_bin_get_by_name")?,
                element_set_state: symbol(core, b"gst_element_set_state")?,
                element_get_bus: symbol(core, b"gst_element_get_bus")?,
                bus_timed_pop_filtered: symbol(core, b"gst_bus_timed_pop_filtered")?,
                message_parse_error: symbol(core, b"gst_message_parse_error")?,
                element_factory_find: symbol(core, b"gst_element_factory_find")?,
                caps_from_string: symbol(core, b"gst_caps_from_string")?,
                caps_get_structure: symbol(core, b"gst_caps_get_structure")?,
                structure_get_int: symbol(core, b"gst_structure_get_int")?,
                structure_get_fraction: symbol(core, b"gst_structure_get_fraction")?,
                structure_get_string: symbol(core, b"gst_structure_get_string")?,
                buffer_new_memdup: symbol(core, b"gst_buffer_new_memdup")?,
                buffer_add_reference_timestamp_meta: symbol(
                    core,
                    b"gst_buffer_add_reference_timestamp_meta",
                )?,
                buffer_get_reference_timestamp_meta: symbol(
                    core,
                    b"gst_buffer_get_reference_timestamp_meta",
                )?,
                buffer_map: symbol(core, b"gst_buffer_map")?,
                buffer_unmap: symbol(core, b"gst_buffer_unmap")?,
                buffer_n_memory: symbol(core, b"gst_buffer_n_memory")?,
                buffer_peek_memory: symbol(core, b"gst_buffer_peek_memory")?,
                sample_get_buffer: symbol(core, b"gst_sample_get_buffer")?,
                sample_get_caps: symbol(core, b"gst_sample_get_caps")?,
                app_src_push_buffer: symbol(app, b"gst_app_src_push_buffer")?,
                app_src_end_of_stream: symbol(app, b"gst_app_src_end_of_stream")?,
                app_src_set_caps: symbol(app, b"gst_app_src_set_caps")?,
                app_sink_try_pull_sample: symbol(app, b"gst_app_sink_try_pull_sample")?,
                app_sink_is_eos: symbol(app, b"gst_app_sink_is_eos")?,
                buffer_get_video_meta: symbol(video, b"gst_buffer_get_video_meta")?,
                is_dmabuf_memory: symbol(allocators, b"gst_is_dmabuf_memory")?,
                dmabuf_memory_get_fd: symbol(allocators, b"gst_dmabuf_memory_get_fd")?,
                error_free: symbol(glib, b"g_error_free")?,
                free: symbol(glib, b"g_free")?,
            };
            let mut error = null_mut();
            if init_check(null_mut(), null_mut(), &mut error) == 0 {
                return Err(gst.message_of(error));
            }
            Ok(gst)
        }
    }

    /// Whether the registry has an element named `name`.
    pub(crate) fn has_element(&self, name: &str) -> bool {
        let Ok(name) = CString::new(name) else {
            return false;
        };
        // SAFETY: a factory looked up by a valid name, unreffed at once.
        unsafe {
            let factory = (self.element_factory_find)(name.as_ptr());
            if factory.is_null() {
                return false;
            }
            (self.object_unref)(factory);
            true
        }
    }

    /// A pipeline made from `description`, in GStreamer's launch syntax; why not otherwise.
    pub(crate) fn pipeline(&'static self, description: &str) -> Result<Element, String> {
        let description = CString::new(description).map_err(|error| error.to_string())?;
        let mut error = null_mut();
        // SAFETY: a valid description; the floating element answered is sunk into a reference
        // owned by the `Element`.
        unsafe {
            let element = (self.parse_launch)(description.as_ptr(), &mut error);
            if element.is_null() {
                return Err(self.message_of(error));
            }
            if !error.is_null() {
                // A pipeline made with a recoverable error, such as a missing property.
                let message = self.message_of(error);
                (self.object_unref)((self.object_ref_sink)(element));
                return Err(message);
            }
            Ok(Element::own(self, (self.object_ref_sink)(element)))
        }
    }

    /// Caps from `description`; `None` for one GStreamer does not read.
    pub(crate) fn caps(&'static self, description: &str) -> Option<Caps> {
        let description = CString::new(description).ok()?;
        // SAFETY: a valid description; the caps are owned by the `Caps`.
        let caps = unsafe { (self.caps_from_string)(description.as_ptr()) };
        NonNull::new(caps).map(|caps| Caps { gst: self, caps })
    }

    /// The message of `error`, freeing it.
    ///
    /// # Safety
    ///
    /// `error` is null or a `GError` this caller owns.
    unsafe fn message_of(&self, error: *mut GError) -> String {
        if error.is_null() {
            return "GStreamer gave no reason".to_owned();
        }
        // SAFETY: an owned GError, read, then freed once.
        unsafe {
            let message = CStr::from_ptr((*error).message)
                .to_string_lossy()
                .into_owned();
            (self.error_free)(error);
            message
        }
    }
}

/// A GStreamer element this side holds a reference to: a pipeline, or one of its elements.
pub(crate) struct Element {
    gst: &'static Gst,
    element: NonNull<c_void>,
}

impl Element {
    /// # Safety
    ///
    /// `element` is a non-null element this caller owns a reference to.
    unsafe fn own(gst: &'static Gst, element: *mut c_void) -> Element {
        Element {
            gst,
            element: NonNull::new(element).expect("an element"),
        }
    }

    /// The element of this pipeline named `name`.
    pub(crate) fn by_name(&self, name: &str) -> Option<Element> {
        let name = CString::new(name).ok()?;
        // SAFETY: a bin looked up by a valid name; the element answered is a new reference.
        let element = unsafe { (self.gst.bin_get_by_name)(self.element.as_ptr(), name.as_ptr()) };
        // SAFETY: a new reference, owned from here.
        (!element.is_null()).then(|| unsafe { Element::own(self.gst, element) })
    }

    /// Runs the pipeline; why not, where an element does not start. A pipeline that is not live
    /// finishes starting once its first buffer reaches its sink, which is not waited for.
    pub(crate) fn play(&self) -> Result<(), String> {
        self.set_state(STATE_PLAYING)
    }

    /// Stops the pipeline to its beginning: every element lets go of all it holds, its buffers,
    /// its stream's end and its resources. Only this clears a sink's end of stream; a pipeline
    /// stopped only as far as `READY` keeps it, and takes no more buffers.
    pub(crate) fn stop(&self) -> Result<(), String> {
        self.set_state(STATE_NULL)
    }

    fn set_state(&self, state: c_int) -> Result<(), String> {
        // SAFETY: a live element; the state is one of GStreamer's.
        let changed = unsafe { (self.gst.element_set_state)(self.element.as_ptr(), state) };
        if changed == STATE_CHANGE_FAILURE {
            return Err(self
                .error()
                .unwrap_or_else(|| "the pipeline did not start".into()));
        }
        Ok(())
    }

    /// The first error the pipeline posted and nobody read yet.
    pub(crate) fn error(&self) -> Option<String> {
        // SAFETY: a live element's bus, a new reference, unreffed below; a message taken off it is
        // an error, parsed, then unreffed.
        unsafe {
            let bus = (self.gst.element_get_bus)(self.element.as_ptr());
            if bus.is_null() {
                return None;
            }
            let message = (self.gst.bus_timed_pop_filtered)(bus, 0, MESSAGE_ERROR);
            (self.gst.object_unref)(bus);
            if message.is_null() {
                return None;
            }
            let (mut error, mut debug) = (null_mut(), null_mut());
            (self.gst.message_parse_error)(message, &mut error, &mut debug);
            (self.gst.free)(debug.cast());
            (self.gst.mini_object_unref)(message);
            Some(self.gst.message_of(error))
        }
    }

    /// Pushes `data` into this source (`appsrc`), carrying `number` as its reference timestamp
    /// under `reference`, which reaches the sample its picture becomes.
    pub(crate) fn push(&self, data: &[u8], reference: &Caps, number: u64) -> Result<(), String> {
        // SAFETY: a buffer made of a copy of `data`, given a meta, and handed to the source, which
        // takes it.
        unsafe {
            let buffer = (self.gst.buffer_new_memdup)(data.as_ptr().cast(), data.len());
            if buffer.is_null() {
                return Err("GStreamer made no buffer".into());
            }
            (self.gst.buffer_add_reference_timestamp_meta)(
                buffer,
                reference.caps.as_ptr(),
                number,
                CLOCK_TIME_NONE,
            );
            match (self.gst.app_src_push_buffer)(self.element.as_ptr(), buffer) {
                FLOW_OK => Ok(()),
                flow => Err(format!("the source took no more chunks ({flow})")),
            }
        }
    }

    /// Ends this source's stream (`appsrc`): the elements after it put out all they hold.
    pub(crate) fn end_stream(&self) {
        // SAFETY: a live source.
        unsafe { (self.gst.app_src_end_of_stream)(self.element.as_ptr()) };
    }

    /// What this source (`appsrc`) says its buffers are.
    pub(crate) fn set_caps(&self, caps: &Caps) {
        // SAFETY: a live source and live caps, which it refs itself.
        unsafe { (self.gst.app_src_set_caps)(self.element.as_ptr(), caps.caps.as_ptr()) };
    }

    /// The next sample of this sink (`appsink`), waiting at most `timeout`.
    pub(crate) fn pull(&self, timeout: Duration) -> Pulled {
        let nanoseconds = timeout.as_nanos().try_into().unwrap_or(u64::MAX - 1);
        // SAFETY: a live sink; the sample answered is a new reference, owned by the `Sample`.
        unsafe {
            let sample = (self.gst.app_sink_try_pull_sample)(self.element.as_ptr(), nanoseconds);
            match NonNull::new(sample) {
                Some(sample) => Pulled::Sample(Sample {
                    gst: self.gst,
                    sample,
                }),
                None if (self.gst.app_sink_is_eos)(self.element.as_ptr()) != 0 => {
                    Pulled::EndOfStream
                }
                None => Pulled::Nothing,
            }
        }
    }
}

impl Drop for Element {
    fn drop(&mut self) {
        // SAFETY: the reference this element owns, released once.
        unsafe { (self.gst.object_unref)(self.element.as_ptr()) };
    }
}

/// The pipeline stops before it goes, so its threads end.
pub(crate) struct Pipeline(pub(crate) Element);

impl Drop for Pipeline {
    fn drop(&mut self) {
        // SAFETY: a live pipeline, taken down before its last reference goes.
        unsafe { (self.0.gst.element_set_state)(self.0.element.as_ptr(), STATE_NULL) };
    }
}

/// What a sink answered.
pub(crate) enum Pulled {
    Sample(Sample),
    /// The stream ended, and every sample was pulled.
    EndOfStream,
    /// Nothing came in time.
    Nothing,
}

/// Caps this side owns.
pub(crate) struct Caps {
    gst: &'static Gst,
    caps: NonNull<c_void>,
}

impl Drop for Caps {
    fn drop(&mut self) {
        // SAFETY: the reference these caps own, released once.
        unsafe { (self.gst.mini_object_unref)(self.caps.as_ptr()) };
    }
}

/// A sample an `appsink` handed out: a buffer and the caps it is in, kept while this lives.
pub(crate) struct Sample {
    gst: &'static Gst,
    sample: NonNull<c_void>,
}

/// What a sample's caps say of its picture.
pub(crate) struct PictureFacts {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixel_aspect: (u32, u32),
    /// The DRM format and modifier of a dmabuf picture, as `drm-format` says them.
    pub(crate) drm_format: Option<String>,
}

/// Where a plane of a dmabuf picture lies.
pub(crate) struct DmabufPlane<'a> {
    pub(crate) fd: BorrowedFd<'a>,
    pub(crate) offset: u32,
    pub(crate) stride: u32,
}

impl Sample {
    fn buffer(&self) -> *mut c_void {
        // SAFETY: a live sample; its buffer lives as long as it does.
        unsafe { (self.gst.sample_get_buffer)(self.sample.as_ptr()) }
    }

    /// The reference timestamp under `reference` its buffer carries.
    pub(crate) fn reference(&self, reference: &Caps) -> Option<u64> {
        let buffer = self.buffer();
        if buffer.is_null() {
            return None;
        }
        // SAFETY: a live buffer; the meta answered lives as long as it does, and is read here.
        unsafe {
            let meta =
                (self.gst.buffer_get_reference_timestamp_meta)(buffer, reference.caps.as_ptr());
            (!meta.is_null()).then(|| (*meta).timestamp)
        }
    }

    /// What its caps say of the picture; `None` for caps without a size.
    pub(crate) fn facts(&self) -> Option<PictureFacts> {
        // SAFETY: a live sample's caps and their first structure, read here; the strings are
        // copied before the sample can go.
        unsafe {
            let caps = (self.gst.sample_get_caps)(self.sample.as_ptr());
            if caps.is_null() {
                return None;
            }
            let structure = (self.gst.caps_get_structure)(caps, 0);
            let (mut width, mut height) = (0, 0);
            if (self.gst.structure_get_int)(structure, c"width".as_ptr(), &mut width) == 0
                || (self.gst.structure_get_int)(structure, c"height".as_ptr(), &mut height) == 0
            {
                return None;
            }
            let (mut numerator, mut denominator) = (1, 1);
            if (self.gst.structure_get_fraction)(
                structure,
                c"pixel-aspect-ratio".as_ptr(),
                &mut numerator,
                &mut denominator,
            ) == 0
            {
                (numerator, denominator) = (1, 1);
            }
            let drm_format = (self.gst.structure_get_string)(structure, c"drm-format".as_ptr());
            Some(PictureFacts {
                width: width.max(0) as u32,
                height: height.max(0) as u32,
                pixel_aspect: (numerator.max(1) as u32, denominator.max(1) as u32),
                drm_format: (!drm_format.is_null())
                    .then(|| CStr::from_ptr(drm_format).to_string_lossy().into_owned()),
            })
        }
    }

    /// Runs `read` on the buffer's bytes and the stride of its first plane, mapped for reading.
    pub(crate) fn with_bytes<T>(&self, read: impl FnOnce(&[u8], usize) -> T) -> Option<T> {
        let buffer = self.buffer();
        if buffer.is_null() {
            return None;
        }
        // SAFETY: a live buffer mapped for reading, read only while mapped, and unmapped once.
        unsafe {
            let mut info: MapInfo = std::mem::zeroed();
            if (self.gst.buffer_map)(buffer, &mut info, MAP_READ) == 0 {
                return None;
            }
            let bytes = std::slice::from_raw_parts(info.data, info.size);
            let meta = (self.gst.buffer_get_video_meta)(buffer);
            let (offset, stride) = if meta.is_null() {
                (0, 0)
            } else {
                ((*meta).offset[0], (*meta).stride[0].max(0) as usize)
            };
            let answer = read(bytes.get(offset..).unwrap_or_default(), stride);
            (self.gst.buffer_unmap)(buffer, &mut info);
            Some(answer)
        }
    }

    /// The buffer's one plane as a dmabuf; `None` for a buffer in other memory.
    pub(crate) fn dmabuf_plane(&self) -> Option<DmabufPlane<'_>> {
        let buffer = self.buffer();
        if buffer.is_null() {
            return None;
        }
        // SAFETY: a live buffer's first memory, and its fd, both living as long as the sample;
        // the video meta read for where the plane lies.
        unsafe {
            if (self.gst.buffer_n_memory)(buffer) == 0 {
                return None;
            }
            let memory = (self.gst.buffer_peek_memory)(buffer, 0);
            if memory.is_null() || (self.gst.is_dmabuf_memory)(memory) == 0 {
                return None;
            }
            let fd: RawFd = (self.gst.dmabuf_memory_get_fd)(memory);
            if fd < 0 {
                return None;
            }
            let meta = (self.gst.buffer_get_video_meta)(buffer);
            if meta.is_null() {
                return None;
            }
            Some(DmabufPlane {
                fd: BorrowedFd::borrow_raw(fd),
                offset: (*meta).offset[0] as u32,
                stride: (*meta).stride[0].max(0) as u32,
            })
        }
    }
}

impl Drop for Sample {
    fn drop(&mut self) {
        // SAFETY: the reference this sample owns, released once.
        unsafe { (self.gst.mini_object_unref)(self.sample.as_ptr()) };
    }
}

// SAFETY: GStreamer's objects and mini-objects are reference counted atomically, and every call
// made through these wrappers is one GStreamer documents as safe from any thread.
unsafe impl Send for Element {}
unsafe impl Sync for Element {}
unsafe impl Send for Caps {}
unsafe impl Sync for Caps {}
unsafe impl Send for Sample {}
unsafe impl Sync for Sample {}
unsafe impl Send for Gst {}
unsafe impl Sync for Gst {}

/// The library `name`, kept loaded for the process's life.
fn library(name: &str) -> Result<&'static libloading::Library, String> {
    // SAFETY: a system library whose initializers are safe to run.
    let library = unsafe { libloading::Library::new(name) }.map_err(|error| error.to_string())?;
    Ok(Box::leak(Box::new(library)))
}

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
