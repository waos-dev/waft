//! GPU memory the system's compositor shows, as a host hands it to the app: on Apple platforms an
//! `IOSurface`, Android's `HardwareBuffer`, a dmabuf behind Wayland's `wl_buffer`, a shared D3D
//! texture; and on Wayland, for a producer without a GPU the compositor imports from, shared memory
//! the compositor maps. Not a dart:ui type: Flutter's engine keeps its buffers to itself. They follow
//! `wasdk:platform/hardware-buffers` one to one.
//!
//! A hardware buffer is memory the app holds: one it allocates
//! ([`HardwareBufferAllocator::allocate`]), as `IOSurfaceCreate` and `AHardwareBuffer_allocate`
//! make one, and an image the system's hardware codecs decode
//! ([`HardwareImageCodecs`](crate::image_codecs::HardwareImageCodecs)), which no system pool waits
//! for. A video's frame is apart, lent with its slot in a decoder's pool
//! ([`VideoFrame`](crate::VideoFrame)), and its memory is one of these too, which keeps the frame
//! while it is held. The app draws into a buffer and samples it through a texture it imports on
//! its own device ([`Platform::import_texture`](crate::Platform::import_texture)), shows it on a
//! composition layer ([`CompositionLayer::present`](crate::CompositionLayer::present)), and has
//! the hardware codecs encode it; its pixels come back to the app's memory only through
//! [`HardwareBuffer::read_pixels`]. A buffer knows only its own facts, its size, how it lays out
//! its pixels and what it can be used for, and names no GPU API: the host, which knows both the buffer and the device,
//! imports it, as each system's GPU API imports such memory.
//!
//! Each host implements [`HardwareBuffer`] over its system's buffers. Platform code reaches the
//! system's own buffer by downcasting through `Any` to the type of the host that handed it out: on
//! Apple platforms `waft-embedder-winit`'s `SurfaceBuffer`, whose `surface` is the `IOSurface`.
//!
//! The invariant a host keeps: a texture imported of a buffer holds the buffer, and the frame it
//! is the memory of, until the GPU is done with it.

use std::any::Any;
use std::sync::Arc;

use crate::image_codecs::{ImageError, ImageFuture, PixelBuffer, PixelFormat};

/// What makes the app's hardware buffers: the role of Android's graphics allocator (gralloc's
/// `IAllocator`), Fuchsia's sysmem `Allocator` and a GBM device on Linux. `None` from
/// [`SystemGpu::hardware_buffer_allocator`](crate::SystemGpu::hardware_buffer_allocator) where the
/// system allocates none. It may be called from any thread, as a producer allocates a buffer where
/// it draws.
pub trait HardwareBufferAllocator: Send + Sync {
    /// A buffer of `width` by `height` pixels in `format`, for at least what `usage` names
    /// ([`HardwareBuffer::usage`] answers all it is for), its pixels as the system leaves new
    /// memory until the app draws them:
    /// [`ImageErrorKind::Unsupported`](crate::image_codecs::ImageErrorKind::Unsupported) for a
    /// format the system does not allocate a buffer in, as Apple platforms allocate only
    /// [`PixelFormat::Bgra8`] and Android's `AHardwareBuffer` only [`PixelFormat::Rgba8`];
    /// [`TooLarge`](crate::image_codecs::ImageErrorKind::TooLarge) for a side past the largest the
    /// system makes a texture of; [`OutOfMemory`](crate::image_codecs::ImageErrorKind::OutOfMemory)
    /// where the system has no memory left for it; and
    /// [`Other`](crate::image_codecs::ImageErrorKind::Other) for a side of no pixels.
    fn allocate(
        &self,
        width: u32,
        height: u32,
        format: PixelFormat,
        usage: HardwareBufferUsage,
    ) -> Result<Arc<dyn HardwareBuffer>, ImageError>;
}

/// What a buffer can be used for, named after its users, as Android's `AHardwareBuffer` usage is:
/// what the GPU does with it, which a texture imported of it allows
/// ([`Platform::import_texture`](crate::Platform::import_texture)), whether the system's
/// compositor shows it as an overlay, and whether the CPU reads it. A video encoder joins as it is
/// needed.
///
/// The usage an app allocates a buffer for is the least the buffer is for, and the buffer answers
/// all it is for ([`HardwareBuffer::usage`]), which every check reads: a system that gives a buffer
/// more than it was asked, as Apple's give any `IOSurface` every use, says so there rather than
/// hiding it. Android gives a buffer what it was allocated for, as `AHardwareBuffer_describe`
/// answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HardwareBufferUsage {
    /// The GPU samples it in its shaders: Android's `GPU_SAMPLED_IMAGE`.
    pub gpu_sampled: bool,
    /// The GPU draws into it, as a render pass's colour output, and copies into it: Android's
    /// `GPU_COLOR_OUTPUT`.
    pub gpu_color_output: bool,
    /// The system's compositor may show it as a plane of the display, without compositing it on
    /// the GPU: Android's `COMPOSER_OVERLAY`, which a buffer presented to a composition layer
    /// there wants and SurfaceFlinger otherwise draws with the GPU, at a cost in power. Core
    /// Animation shows any `IOSurface` so, and every buffer on Apple's systems has it.
    pub composer_overlay: bool,
    /// The CPU reads it, through [`HardwareBuffer::read_pixels`], which answers
    /// [`ImageErrorKind::Unsupported`](crate::image_codecs::ImageErrorKind::Unsupported) for a
    /// buffer without it: Android's `CPU_READ_RARELY` or `CPU_READ_OFTEN`. The CPU reads any
    /// `IOSurface` while it is locked, and every buffer on Apple's systems has it.
    pub cpu_read: bool,
}

impl HardwareBufferUsage {
    /// Sampled by the GPU only, as a decoded image's buffer and a video frame's are.
    pub const GPU_SAMPLED: HardwareBufferUsage = HardwareBufferUsage {
        gpu_sampled: true,
        gpu_color_output: false,
        composer_overlay: false,
        cpu_read: false,
    };

    /// Every use, as any `IOSurface` has.
    pub const ALL: HardwareBufferUsage = HardwareBufferUsage {
        gpu_sampled: true,
        gpu_color_output: true,
        composer_overlay: true,
        cpu_read: true,
    };
}

/// One block of pixels in memory the GPU, the system's compositor and other processes all reach.
/// Every method may be called from any thread.
pub trait HardwareBuffer: Any + Send + Sync {
    /// The buffer's width, in its own pixels.
    fn width(&self) -> u32;

    /// The buffer's height, in its own pixels.
    fn height(&self) -> u32;

    /// What the buffer can be used for: at least what it was allocated for, and on some systems
    /// more (see [`HardwareBufferUsage`]). A texture imported of it allows what this allows, and
    /// [`read_pixels`](HardwareBuffer::read_pixels) reads it only where this has
    /// [`cpu_read`](HardwareBufferUsage::cpu_read).
    fn usage(&self) -> HardwareBufferUsage;

    /// How the buffer lays out its pixels: one plane of colour, or a video decoder's planes.
    fn format(&self) -> HardwareBufferFormat;

    /// The buffer's pixels copied into the app's memory, in the layout the buffer holds them, for a
    /// buffer of one plane: BGRA on Apple platforms, RGBA on Android. It reads what the GPU has finished writing, so an app
    /// that drew into the buffer waits for that work first, as it does before presenting it;
    /// [`ImageErrorKind::Unsupported`](crate::image_codecs::ImageErrorKind::Unsupported) for a
    /// buffer whose [`usage`](HardwareBuffer::usage) lacks
    /// [`cpu_read`](HardwareBufferUsage::cpu_read), on every system, and for one whose pixels are
    /// in no [`PixelFormat`], as a decoder's planes ([`HardwareBufferFormat::Nv12`] and
    /// [`P010`](HardwareBufferFormat::P010)) are.
    fn read_pixels(&self) -> ImageFuture<PixelBuffer>;
}

/// How a hardware buffer lays out its pixels: one plane of colour, as an app draws, or a video
/// decoder's own planes of brightness and colour difference, which the system's decoders write and
/// its compositor shows with no conversion. What the samples mean as colour (matrix, range) is the
/// picture's ([`VideoFrame::color`](crate::VideoFrame::color)), not the buffer's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HardwareBufferFormat {
    /// 8-bit blue, green, red and alpha in one plane: CoreVideo's `32BGRA`, DXGI's
    /// `B8G8R8A8_UNORM`, DRM's `ARGB8888`.
    Bgra8,
    /// 8-bit red, green, blue and alpha in one plane: Android's `R8G8B8A8_UNORM`.
    Rgba8,
    /// 8-bit 4:2:0 in two planes: brightness, then blue and red difference interleaved at half
    /// the size each way. CoreVideo's `420v` and `420f`, DXGI's and DRM's `NV12`, Android's
    /// `Y8Cb8Cr8_420`.
    Nv12,
    /// 10-bit 4:2:0 laid out as [`Nv12`](Self::Nv12), each sample in the high ten bits of
    /// sixteen: CoreVideo's `x420` and `xf20`, DXGI's and DRM's `P010`, Android's `YCbCr_P010`.
    P010,
}

impl HardwareBufferFormat {
    /// How many planes it has: one, or two for [`Nv12`](Self::Nv12) and [`P010`](Self::P010).
    pub fn plane_count(self) -> usize {
        match self {
            HardwareBufferFormat::Bgra8 | HardwareBufferFormat::Rgba8 => 1,
            HardwareBufferFormat::Nv12 | HardwareBufferFormat::P010 => 2,
        }
    }

    /// The texture format plane `plane` is sampled in: `Bgra8Unorm` or `Rgba8Unorm` for one plane;
    /// `R8Unorm` then `Rg8Unorm` for [`Nv12`](Self::Nv12), as wgpu's `TextureFormat::NV12`
    /// aspects are; `R16Unorm` then `Rg16Unorm` for [`P010`](Self::P010), which need
    /// [`Features::TEXTURE_FORMAT_16BIT_NORM`](wgpu::Features::TEXTURE_FORMAT_16BIT_NORM). `None`
    /// past the last plane.
    pub fn plane_format(self, plane: usize) -> Option<wgpu::TextureFormat> {
        let formats: &[wgpu::TextureFormat] = match self {
            HardwareBufferFormat::Bgra8 => &[wgpu::TextureFormat::Bgra8Unorm],
            HardwareBufferFormat::Rgba8 => &[wgpu::TextureFormat::Rgba8Unorm],
            HardwareBufferFormat::Nv12 => {
                &[wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm]
            }
            HardwareBufferFormat::P010 => &[
                wgpu::TextureFormat::R16Unorm,
                wgpu::TextureFormat::Rg16Unorm,
            ],
        };
        formats.get(plane).copied()
    }

    /// The size of plane `plane` of a buffer of `size` pixels: the whole for the first plane, and
    /// half each way, rounded up, for the colour plane of [`Nv12`](Self::Nv12) and
    /// [`P010`](Self::P010). `None` past the last plane.
    pub fn plane_size(self, plane: usize, size: [u32; 2]) -> Option<[u32; 2]> {
        match plane {
            0 => Some(size),
            1 if self.plane_count() == 2 => Some(size.map(|side| side.div_ceil(2))),
            _ => None,
        }
    }
}

/// A GPU as its system names it to every API on it and to other processes: what anything that
/// makes GPU memory for another is told, so the memory opens where it is read. Memory shared by
/// handle opens only on the adapter that made it: a D3D texture shared by NT handle on Windows, a
/// dmabuf in a tiled layout on Linux. An `IOSurface` and an `AHardwareBuffer` open on any of their
/// system's GPUs, so Apple's systems and Android name none.
///
/// Two ids are the same GPU exactly when they are equal: a host turns whatever its system says
/// into this form first, as Wayland's compositor may name a GPU by its primary node or by its
/// render node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GpuId {
    /// Windows: the adapter's locally unique id, as `DXGI_ADAPTER_DESC1::AdapterLuid` holds it,
    /// `ID3D12Device::GetAdapterLuid` answers it and Vulkan's
    /// `VkPhysicalDeviceIDProperties::deviceLUID` gives it.
    Luid { low: u32, high: i32 },
    /// Linux: the device number of the GPU's DRM render node (226:128 is `/dev/dri/renderD128`),
    /// as `stat(2)` of the node and Vulkan's `VkPhysicalDeviceDrmPropertiesEXT::renderMajor` and
    /// `renderMinor` give it.
    DrmRenderNode { major: u32, minor: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decoders_format_has_a_brightness_plane_then_a_colour_plane_at_half_size() {
        let format = HardwareBufferFormat::Nv12;
        assert_eq!(format.plane_count(), 2);
        assert_eq!(format.plane_format(0), Some(wgpu::TextureFormat::R8Unorm));
        assert_eq!(format.plane_format(1), Some(wgpu::TextureFormat::Rg8Unorm));
        assert_eq!(format.plane_format(2), None);
        assert_eq!(format.plane_size(0, [1919, 1081]), Some([1919, 1081]));
        assert_eq!(format.plane_size(1, [1919, 1081]), Some([960, 541]));
        assert_eq!(format.plane_size(2, [1919, 1081]), None);
        assert_eq!(
            HardwareBufferFormat::P010.plane_format(1),
            Some(wgpu::TextureFormat::Rg16Unorm)
        );
    }

    #[test]
    fn a_format_of_one_plane_is_that_plane_whole() {
        let format = HardwareBufferFormat::Bgra8;
        assert_eq!(format.plane_count(), 1);
        assert_eq!(
            format.plane_format(0),
            Some(wgpu::TextureFormat::Bgra8Unorm)
        );
        assert_eq!(format.plane_format(1), None);
        assert_eq!(format.plane_size(1, [8, 8]), None);
    }
}
