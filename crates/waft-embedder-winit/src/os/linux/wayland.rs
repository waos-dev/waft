//! Waft's own share of the Wayland connection winit opened: the globals composition layers need
//! (`wl_compositor`, `wl_subcompositor`, `wp_viewporter`, `zwp_linux_dmabuf_v1`, `wl_shm`), bound
//! on an event queue of Waft's, which a thread of its own reads, as Mesa's Vulkan WSI shares the
//! connection on a queue of its own. What the compositor imports is read once, as the connection
//! is made (`dmabuf_feedback`). A dmabuf or shared memory becomes a `wl_buffer` here, and its
//! releases come back here. XDG activation, where the compositor offers it, brings a window forward
//! with a launch's token.
//!
//! The invariants a later editor must keep: nothing here makes a request that can fail with a
//! protocol error, since one ends the whole connection, winit's windows with it: a `wl_buffer` is
//! made with the asynchronous `zwp_linux_buffer_params_v1.create`, whose `failed` is answered as
//! the buffer not being shown, as GTK's graphics offload does, never with `create_immed`; a
//! `wl_shm` buffer only of memory it fits in (`ShmBuffer::new`); and every object made here is on
//! Waft's queue, so its events never reach winit's. `zwp_linux_dmabuf_v1` is bound at version 5
//! at most: version 6 sends no main device, which a client must be written for, as GTK 4.18's crash
//! there showed.
//!
//! wayland-client's foreign display and foreign proxies wrap libwayland's pointers, which is why
//! this module allows `unsafe`.
#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::os::fd::AsFd;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_region, wl_registry, wl_shm, wl_shm_pool, wl_subcompositor,
    wl_subsurface, wl_surface,
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, delegate_noop, event_created_child,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_feedback_v1, zwp_linux_dmabuf_v1,
};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1;
use winit::raw_window_handle::RawWindowHandle;
use winit::window::Window;

use waft_embedder::{AcceptedBuffers, HardwareBuffer, HardwareBufferFormat};

use super::dmabuf::{ARGB8888, DmabufBuffer, MOD_INVALID, MOD_LINEAR};
use super::dmabuf_feedback::{Feedback, accepted_at_version_3};
use super::shm::ShmBuffer;

/// How long a `wl_buffer`'s making is waited for before the buffer is taken as refused.
const BUFFER_WAIT: Duration = Duration::from_secs(2);

/// The display winit connected to, where it is Wayland's, set once the event loop is built.
static DISPLAY: OnceLock<usize> = OnceLock::new();

/// Waft's share of the connection, made at its first use; `None` on X11, and where the compositor
/// lacks a global composition layers need.
static WAYLAND: OnceLock<Option<Arc<Wayland>>> = OnceLock::new();

/// Remembers the `wl_display` winit connected to, for [`Wayland::get`].
pub(crate) fn set_display(display: *mut std::ffi::c_void) {
    let _ = DISPLAY.set(display as usize);
}

pub(crate) struct Wayland {
    connection: Connection,
    queue: QueueHandle<Events>,
    pub(crate) compositor: wl_compositor::WlCompositor,
    pub(crate) subcompositor: wl_subcompositor::WlSubcompositor,
    pub(crate) viewporter: wp_viewporter::WpViewporter,
    linux_dmabuf: zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    /// Shared memory; `None` where the compositor offers none, which every compositor does.
    shm: Option<wl_shm::WlShm>,
    /// XDG activation; `None` where the compositor offers none.
    activation: Option<xdg_activation_v1::XdgActivationV1>,
    /// What the compositor imports, as it said when the connection was made.
    accepted: AcceptedBuffers,
}

/// What version 3's global lists: a DRM format and a modifier each.
type ListedModifiers = Mutex<Vec<(u32, u64)>>;

/// What a `wl_buffer` carries: the producers' releases, one for each present not yet released, in
/// the order presented.
#[derive(Default)]
pub(crate) struct BufferData {
    releases: Mutex<VecDeque<Box<dyn FnOnce() + Send + Sync>>>,
}

impl BufferData {
    /// `released` runs at the compositor's next `wl_buffer.release` of this buffer not already
    /// owed to an earlier present.
    pub(crate) fn release_with(&self, released: Box<dyn FnOnce() + Send + Sync>) {
        self.releases.lock().unwrap().push_back(released);
    }
}

/// The events of Waft's queue, read on its thread.
pub(crate) struct Events;

/// What a `zwp_linux_buffer_params_v1` waits for: whether the compositor made the buffer.
struct Asked(Mutex<Option<SyncSender<Option<wl_buffer::WlBuffer>>>>);

impl Wayland {
    /// Waft's share of the connection, made at the first call; `None` on X11 and where the
    /// compositor lacks a global composition layers need, which is logged once.
    pub(crate) fn get() -> Option<Arc<Wayland>> {
        WAYLAND
            .get_or_init(|| {
                let display = *DISPLAY.get()?;
                match Wayland::connect(display as *mut _) {
                    Ok(wayland) => Some(Arc::new(wayland)),
                    Err(missing) => {
                        eprintln!("waft: composition layers are not shown: {missing}");
                        None
                    }
                }
            })
            .clone()
    }

    fn connect(display: *mut std::ffi::c_void) -> Result<Wayland, String> {
        // SAFETY: winit's live `wl_display`, which outlives the app's windows and so every use.
        let backend = unsafe { Backend::from_foreign_display(display.cast()) };
        let connection = Connection::from_backend(backend);
        let (globals, mut queue) =
            registry_queue_init::<Events>(&connection).map_err(|error| error.to_string())?;
        let handle = queue.handle();
        let bind_error = |name: &str| format!("the compositor offers no {name}");
        let compositor = globals
            .bind(&handle, 4..=6, ())
            .map_err(|_| bind_error("wl_compositor"))?;
        let subcompositor = globals
            .bind(&handle, 1..=1, ())
            .map_err(|_| bind_error("wl_subcompositor"))?;
        let viewporter = globals
            .bind(&handle, 1..=1, ())
            .map_err(|_| bind_error("wp_viewporter"))?;
        // Version 3 lists the formats and modifiers as events of the global itself; from version
        // 4 a feedback object says them, per device.
        let listed = Arc::new(ListedModifiers::default());
        let linux_dmabuf: zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1 = globals
            .bind(&handle, 3..=5, Arc::clone(&listed))
            .map_err(|_| bind_error("zwp_linux_dmabuf_v1 at version 3"))?;
        let feedback = Arc::new(Feedback::default());
        let feedback_object = (linux_dmabuf.version() >= 4)
            .then(|| linux_dmabuf.get_default_feedback(&handle, Arc::clone(&feedback)));
        let shm: Option<wl_shm::WlShm> = globals.bind(&handle, 1..=1, ()).ok();
        let activation = globals.bind(&handle, 1..=1, ()).ok();
        queue
            .roundtrip(&mut Events)
            .map_err(|error| error.to_string())?;
        let accepted = match feedback_object {
            Some(feedback_object) => {
                feedback_object.destroy();
                feedback.accepted_buffers(shm.is_some())
            }
            None => accepted_at_version_3(listed.lock().unwrap().drain(..), shm.is_some()),
        };
        std::thread::Builder::new()
            .name("waft wayland".into())
            .spawn(
                move || {
                    while queue.blocking_dispatch(&mut Events).is_ok() {}
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(Wayland {
            connection,
            queue: handle,
            compositor,
            subcompositor,
            viewporter,
            linux_dmabuf,
            shm,
            activation,
            accepted,
        })
    }

    /// What the compositor imports, as it said when the connection was made.
    pub(crate) fn accepted_buffers(&self) -> &AcceptedBuffers {
        &self.accepted
    }

    pub(crate) fn queue(&self) -> &QueueHandle<Events> {
        &self.queue
    }

    /// Sends what was asked so far.
    pub(crate) fn flush(&self) {
        let _ = self.connection.flush();
    }

    /// The window's own `wl_surface`, winit's, as a proxy Waft makes requests on; its events stay
    /// winit's.
    pub(crate) fn foreign_surface(
        &self,
        surface: *mut std::ffi::c_void,
    ) -> Option<wl_surface::WlSurface> {
        // SAFETY: a live `wl_surface` of winit's window, of this interface, on this connection.
        let id = unsafe { ObjectId::from_ptr(wl_surface::WlSurface::interface(), surface.cast()) }
            .ok()?;
        wl_surface::WlSurface::from_id(&self.connection, id).ok()
    }

    /// Asks the compositor to bring `surface` forward with `token`. A token the compositor does
    /// not know, or no longer honours, is ignored, never an error.
    fn activate(&self, surface: &wl_surface::WlSurface, token: &str) {
        let Some(activation) = &self.activation else {
            return;
        };
        activation.activate(token.to_owned(), surface);
        self.flush();
    }

    /// The modifier the compositor is told `buffer` has: its own where the compositor takes it,
    /// and for a linear buffer the implicit modifier where that alone is offered, as a driver's
    /// implicit layout is linear for a buffer allocated linear. `None` where it takes neither.
    fn announced_modifier(&self, buffer: &DmabufBuffer) -> Option<u64> {
        let offered = |modifier: u64| {
            self.accepted.gpu_memory.iter().any(|memory| {
                memory.formats.iter().any(|accepted| {
                    accepted.format == HardwareBufferFormat::Bgra8
                        && accepted.modifiers.contains(&modifier)
                })
            })
        };
        if offered(buffer.modifier()) {
            Some(buffer.modifier())
        } else if buffer.modifier() == MOD_LINEAR && offered(MOD_INVALID) {
            Some(MOD_INVALID)
        } else {
            None
        }
    }

    /// `buffer` as a `wl_buffer`, made by the compositor and waited for; `None`, said why, where
    /// the compositor refuses it or does not answer.
    pub(crate) fn wl_buffer(&self, buffer: &DmabufBuffer) -> Option<wl_buffer::WlBuffer> {
        let Some(modifier) = self.announced_modifier(buffer) else {
            eprintln!(
                "waft: the compositor takes ARGB8888 in no modifier this buffer ({:#x}) has: it is not shown",
                buffer.modifier()
            );
            return None;
        };
        let (answer, answered) = sync_channel(1);
        let params = self
            .linux_dmabuf
            .create_params(&self.queue, Asked(Mutex::new(Some(answer))));
        params.add(
            buffer.fd().as_fd(),
            0,
            buffer.offset(),
            buffer.stride(),
            (modifier >> 32) as u32,
            modifier as u32,
        );
        params.create(
            buffer.width() as i32,
            buffer.height() as i32,
            ARGB8888,
            zwp_linux_buffer_params_v1::Flags::empty(),
        );
        self.flush();
        let made = answered.recv_timeout(BUFFER_WAIT).ok().flatten();
        params.destroy();
        if made.is_none() {
            eprintln!("waft: the compositor refused a dmabuf: its layer is not shown");
        }
        made
    }

    /// `buffer` as a `wl_buffer` of a `wl_shm` pool over its memory; `None`, said why, where the
    /// compositor offers no shared memory.
    pub(crate) fn shm_wl_buffer(&self, buffer: &ShmBuffer) -> Option<wl_buffer::WlBuffer> {
        let Some(shm) = &self.shm else {
            eprintln!("waft: the compositor offers no shared memory: the buffer is not shown");
            return None;
        };
        let pool = shm.create_pool(buffer.fd().as_fd(), buffer.length(), &self.queue, ());
        let made = pool.create_buffer(
            0,
            buffer.width() as i32,
            buffer.height() as i32,
            buffer.stride() as i32,
            wl_shm::Format::Argb8888,
            &self.queue,
            Arc::new(BufferData::default()),
        );
        pool.destroy();
        self.flush();
        Some(made)
    }
}

/// Brings `window` forward with `token`, an XDG activation token a launch was given; nothing on X11
/// or where the compositor offers no XDG activation.
pub(crate) fn activate(window: &dyn Window, token: &str) {
    let Some(wayland) = Wayland::get() else {
        return;
    };
    let Ok(handle) = window.rwh_06_window_handle().window_handle() else {
        return;
    };
    let RawWindowHandle::Wayland(handle) = handle.as_raw() else {
        return;
    };
    if let Some(surface) = wayland.foreign_surface(handle.surface.as_ptr()) {
        wayland.activate(&surface, token);
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Events {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, Arc<ListedModifiers>> for Events {
    fn event(
        _: &mut Self,
        _: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        event: zwp_linux_dmabuf_v1::Event,
        listed: &Arc<ListedModifiers>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_linux_dmabuf_v1::Event::Modifier {
            format,
            modifier_hi,
            modifier_lo,
        } = event
        {
            let modifier = (u64::from(modifier_hi) << 32) | u64::from(modifier_lo);
            listed.lock().unwrap().push((format, modifier));
        }
    }
}

impl Dispatch<zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1, Arc<Feedback>> for Events {
    fn event(
        _: &mut Self,
        _: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
        event: zwp_linux_dmabuf_feedback_v1::Event,
        feedback: &Arc<Feedback>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwp_linux_dmabuf_feedback_v1::Event;
        match event {
            Event::FormatTable { fd, size } => feedback.format_table(fd, size),
            Event::MainDevice { device } => feedback.main_device(&device),
            Event::TrancheTargetDevice { device } => feedback.tranche_target_device(&device),
            Event::TrancheFormats { indices } => feedback.tranche_formats(&indices),
            Event::TrancheDone => feedback.tranche_done(),
            _ => {}
        }
    }
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, Asked> for Events {
    fn event(
        _: &mut Self,
        _: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        event: zwp_linux_buffer_params_v1::Event,
        asked: &Asked,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let made = match event {
            zwp_linux_buffer_params_v1::Event::Created { buffer } => Some(buffer),
            zwp_linux_buffer_params_v1::Event::Failed => None,
            _ => return,
        };
        if let Some(answer) = asked.0.lock().unwrap().take() {
            let _ = answer.send(made);
        }
    }

    event_created_child!(Events, zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, [
        zwp_linux_buffer_params_v1::EVT_CREATED_OPCODE => (wl_buffer::WlBuffer, Arc::new(BufferData::default())),
    ]);
}

impl Dispatch<wl_buffer::WlBuffer, Arc<BufferData>> for Events {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        data: &Arc<BufferData>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            let released = data.releases.lock().unwrap().pop_front();
            if let Some(released) = released {
                released();
            }
        }
    }
}

delegate_noop!(Events: ignore wl_compositor::WlCompositor);
delegate_noop!(Events: ignore wl_subcompositor::WlSubcompositor);
delegate_noop!(Events: ignore wl_surface::WlSurface);
delegate_noop!(Events: ignore wl_subsurface::WlSubsurface);
delegate_noop!(Events: ignore wl_region::WlRegion);
delegate_noop!(Events: ignore wp_viewporter::WpViewporter);
delegate_noop!(Events: ignore wl_shm::WlShm);
delegate_noop!(Events: ignore wl_shm_pool::WlShmPool);
delegate_noop!(Events: ignore wp_viewport::WpViewport);
delegate_noop!(Events: ignore xdg_activation_v1::XdgActivationV1);
