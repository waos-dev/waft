//! A GPU of the system's, as a host hands it to the app: what the system makes for that GPU, its
//! video decoders and its hardware buffer allocator, as Direct3D makes a video device on one
//! adapter, VA-API and GBM open one render node, and FFmpeg makes a hardware device before a
//! decoder on it. Memory these make opens on that GPU: a picture a decoder writes, or a buffer the
//! allocator makes, is drawn and shown with no copy across GPUs. Software decoders are among a
//! GPU's too, as the GPU their pictures are copied into. Apple's systems and Android name no GPU,
//! their memory opening on any of the machine's, so they hand out one.
//!
//! The invariant a host keeps: what a GPU hands out makes its memory on that GPU, never on another.

use std::rc::Rc;
use std::sync::Arc;

use crate::GpuId;
use crate::HardwareBufferAllocator;
use crate::video_decoders::VideoDecoders;

/// One of the system's GPUs ([`Platform::gpu`](crate::Platform::gpu)).
pub trait SystemGpu {
    /// The GPU as its system names it; `None` where the system names none.
    fn id(&self) -> Option<GpuId>;

    /// Its video decoders, on its video hardware and on the processor, whose pictures are in its
    /// memory; `None` where the system decodes no video for the app.
    fn video_decoders(&self) -> Option<Rc<dyn VideoDecoders>>;

    /// What allocates its hardware buffers, from any thread; `None` where the system allocates
    /// none.
    fn hardware_buffer_allocator(&self) -> Option<Arc<dyn HardwareBufferAllocator>>;
}

/// A GPU as a host gathers it: its name and what the system made for it, which every host hands
/// out as its [`SystemGpu`].
pub struct HostGpu {
    pub id: Option<GpuId>,
    pub video_decoders: Option<Rc<dyn VideoDecoders>>,
    pub hardware_buffer_allocator: Option<Arc<dyn HardwareBufferAllocator>>,
}

impl SystemGpu for HostGpu {
    fn id(&self) -> Option<GpuId> {
        self.id
    }

    fn video_decoders(&self) -> Option<Rc<dyn VideoDecoders>> {
        self.video_decoders.clone()
    }

    fn hardware_buffer_allocator(&self) -> Option<Arc<dyn HardwareBufferAllocator>> {
        self.hardware_buffer_allocator.clone()
    }
}
