//! A hardware buffer imported as a texture on the app's GPU device, on macOS and iOS: a Metal
//! texture over the buffer's `IOSurface` (`newTextureWithDescriptor:iosurface:plane:`), made a
//! wgpu texture through wgpu-hal, for what the buffer's usage allows; and a decoder's frame as one
//! such texture over each of its surface's planes, as wgpu has no texture of two planes on Metal. A video frame's buffer is
//! its pixel buffer's surface (`media::video_frame`), imported the same way, so a frame is drawn
//! with no copy on the device the app gives.
//!
//! The invariant: a texture of a buffer holds the buffer, and the frame it is the memory of, until
//! wgpu frees it, once the GPU is done with it, since a CoreVideo pool hands a frame's surface out
//! again once nothing holds the frame, whatever still reads the surface
//! (`experiments/2026-11-01-corevideo-pool-reuse`).

// Wrapping an `IOSurface` in a Metal texture, and that in a wgpu texture, needs `unsafe`; this
// module writes it for them.
#![allow(unsafe_code)]

use std::any::Any;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTexture, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage,
};
use waft_embedder::{HardwareBuffer, HardwareBufferFormat, HardwareBufferUsage};

use super::hardware_buffers::SurfaceBuffer;

/// `buffer` as a Metal texture over its surface on `device`, made a wgpu texture that holds the
/// buffer until wgpu frees it; `None` for a buffer another host handed out, on a device not on
/// Metal, for a surface not in BGRA, and where Metal makes no texture of the surface.
pub fn import_texture(buffer: &dyn HardwareBuffer, device: &wgpu::Device) -> Option<wgpu::Texture> {
    let buffer = (buffer as &dyn Any).downcast_ref::<SurfaceBuffer>()?;
    if buffer.format() != HardwareBufferFormat::Bgra8 {
        return None;
    }
    plane_texture(buffer, 0, device)
}

/// `buffer` as one Metal texture over each plane of its surface on `device`, brightness first,
/// each in the format and of the size its [`HardwareBufferFormat`] names for the plane, made wgpu
/// textures that each hold the buffer until wgpu frees them, and a view of each, which holds its
/// texture; `None` for a buffer another host
/// handed out, on a device not on Metal or without the planes' formats (P010's 16-bit ones need
/// `TEXTURE_FORMAT_16BIT_NORM`), and where Metal makes no texture of a plane.
pub fn import_planes(
    buffer: &dyn HardwareBuffer,
    device: &wgpu::Device,
) -> Option<Vec<wgpu::TextureView>> {
    let buffer = (buffer as &dyn Any).downcast_ref::<SurfaceBuffer>()?;
    (0..buffer.format().plane_count())
        .map(|plane| {
            plane_texture(buffer, plane, device)
                .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()))
        })
        .collect()
}

/// Plane `plane` of `buffer` as a Metal texture over its surface on `device`, made a wgpu texture
/// that holds the buffer until wgpu frees it; `None` past the last plane, on a device not on Metal
/// or without the plane's format, and where Metal makes no texture of the plane.
fn plane_texture(
    buffer: &SurfaceBuffer,
    plane: usize,
    device: &wgpu::Device,
) -> Option<wgpu::Texture> {
    let format = buffer.format().plane_format(plane)?;
    if !device.features().contains(format.required_features()) {
        return None;
    }
    let [width, height] = buffer
        .format()
        .plane_size(plane, [buffer.width(), buffer.height()])?;
    // SAFETY: the device is only read, and is let go before the texture it makes is handed to
    // wgpu.
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }?;
    let metal = metal_texture(
        buffer.surface(),
        Plane {
            index: plane,
            format: metal_format(format)?,
            size: [width, height],
        },
        buffer.usage(),
        hal.raw_device(),
    )?;
    drop(hal);
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let held = buffer.clone();
    // SAFETY: `metal` is a live 2D texture in `format`, of exactly this size, with one mip level
    // and one layer, made on the device's Metal device.
    let texture = unsafe {
        wgpu::hal::metal::Device::texture_from_raw(
            metal,
            format,
            MTLTextureType::Type2D,
            1,
            1,
            wgpu::hal::CopyExtent {
                width,
                height,
                depth: 1,
            },
            Some(Box::new(move || drop(held))),
        )
    };
    let descriptor = wgpu::TextureDescriptor {
        label: Some("waft hardware buffer"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: texture_usages(buffer.usage()),
        view_formats: &[],
    };
    // SAFETY: the texture was made on `device` and matches the descriptor, whose usage is within
    // what the Metal texture serves; its pixels are as the surface holds them, which wgpu takes as
    // written.
    Some(unsafe {
        device.create_texture_from_hal::<wgpu::hal::api::Metal>(
            texture,
            &descriptor,
            arriving_state(buffer.usage()),
        )
    })
}

/// One plane of a surface as a Metal texture sees it: which, in what pixel format, and its size.
struct Plane {
    index: usize,
    format: MTLPixelFormat,
    size: [u32; 2],
}

/// The Metal pixel format of a plane wgpu samples in `format`; `None` for one no plane is in.
fn metal_format(format: wgpu::TextureFormat) -> Option<MTLPixelFormat> {
    match format {
        wgpu::TextureFormat::Bgra8Unorm => Some(MTLPixelFormat::BGRA8Unorm),
        wgpu::TextureFormat::R8Unorm => Some(MTLPixelFormat::R8Unorm),
        wgpu::TextureFormat::Rg8Unorm => Some(MTLPixelFormat::RG8Unorm),
        wgpu::TextureFormat::R16Unorm => Some(MTLPixelFormat::R16Unorm),
        wgpu::TextureFormat::Rg16Unorm => Some(MTLPixelFormat::RG16Unorm),
        _ => None,
    }
}

/// A Metal texture over `plane` of `surface` on `device`, for `usage`; `None` where Metal makes
/// none.
fn metal_texture(
    surface: &IOSurfaceRef,
    plane: Plane,
    usage: HardwareBufferUsage,
    device: &ProtocolObject<dyn MTLDevice>,
) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
    let descriptor = MTLTextureDescriptor::new();
    descriptor.setTextureType(MTLTextureType::Type2D);
    descriptor.setPixelFormat(plane.format);
    // SAFETY: both fit an `NSUInteger`, and the plane is of this size.
    unsafe {
        descriptor.setWidth(plane.size[0] as usize);
        descriptor.setHeight(plane.size[1] as usize);
    }
    // Shared is the storage mode a texture over an `IOSurface` takes: it makes the texture a view
    // of the surface rather than a private copy of it.
    descriptor.setStorageMode(MTLStorageMode::Shared);
    descriptor.setUsage(metal_usage(usage));
    device.newTextureWithDescriptor_iosurface_plane(&descriptor, surface, plane.index)
}

/// What a texture of a buffer of `usage` is for: sampled where the GPU samples the buffer, drawn
/// into and copied into where it draws into it, and copied from whatever its usage.
fn texture_usages(usage: HardwareBufferUsage) -> wgpu::TextureUsages {
    let mut usages = wgpu::TextureUsages::COPY_SRC;
    if usage.gpu_sampled {
        usages |= wgpu::TextureUsages::TEXTURE_BINDING;
    }
    if usage.gpu_color_output {
        usages |= wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST;
    }
    usages
}

/// What Metal is told a texture of a buffer of `usage` does: it is read by shaders, and drawn
/// into where the usage says. Copies need no usage of Metal's.
fn metal_usage(usage: HardwareBufferUsage) -> MTLTextureUsage {
    if usage.gpu_color_output {
        MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget
    } else {
        MTLTextureUsage::ShaderRead
    }
}

/// The state a texture of a buffer of `usage` arrives in, which its usage allows.
fn arriving_state(usage: HardwareBufferUsage) -> wgpu::wgt::TextureUses {
    if usage.gpu_sampled {
        wgpu::wgt::TextureUses::RESOURCE
    } else if usage.gpu_color_output {
        wgpu::wgt::TextureUses::COLOR_TARGET
    } else {
        wgpu::wgt::TextureUses::COPY_SRC
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_video::{
        CVPixelBufferGetIOSurface, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
        kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
    };
    use waft_embedder::image_codecs::PixelFormat;
    use waft_embedder::{HardwareBufferAllocator, VideoFrame};

    use super::*;
    use crate::hardware_buffers::SurfaceBufferAllocator;
    use crate::media::{test_frame, test_frame_in};

    const DRAWN: HardwareBufferUsage = HardwareBufferUsage {
        gpu_color_output: true,
        ..HardwareBufferUsage::GPU_SAMPLED
    };

    #[test]
    fn a_buffers_textures_copy_from_it_whatever_its_usage() {
        assert_eq!(
            texture_usages(HardwareBufferUsage::default()),
            wgpu::TextureUsages::COPY_SRC
        );
        assert_eq!(
            texture_usages(HardwareBufferUsage::GPU_SAMPLED),
            wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING
        );
    }

    #[test]
    fn a_buffer_drawn_into_is_copied_into_too() {
        let usage = HardwareBufferUsage {
            gpu_color_output: true,
            ..HardwareBufferUsage::default()
        };
        assert_eq!(
            texture_usages(usage),
            wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
        );
    }

    #[test]
    fn a_buffer_allocated_is_drawn_into_and_read_back() {
        let gpu = TestGpu::new();
        let buffer = SurfaceBufferAllocator
            .allocate(8, 4, HardwareBufferFormat::Bgra8, DRAWN)
            .expect("an IOSurface of 8 by 4");
        let texture = import_texture(&*buffer, &gpu.device).expect("a Metal device takes it");
        assert_eq!([texture.width(), texture.height()], [8, 4]);
        assert!(
            texture
                .usage()
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST)
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        }));
        gpu.queue.submit([encoder.finish()]);
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device is alive");
        let pixels = pollster::block_on(buffer.read_pixels()).expect("its pixels");
        assert_eq!(pixels.layout().format, PixelFormat::Bgra8);
        assert_eq!(&pixels.bytes()[..4], [0, 0, 255, 255], "red, as BGRA");
    }

    /// The texture of a frame's buffer keeps the frame, and with it the frame's surface in use, so
    /// its pool cannot hand the surface out again while the texture may be drawn; once wgpu frees
    /// the texture, the frame goes.
    #[test]
    fn a_frames_texture_holds_the_frame_until_wgpu_frees_it() {
        let gpu = TestGpu::new();
        let device = &gpu.device;
        let frame = test_frame();
        let surface =
            CVPixelBufferGetIOSurface(Some(frame.pixel_buffer())).expect("the frame's surface");
        let texture =
            import_texture(&*frame.hardware_buffer(), device).expect("a Metal device takes it");
        assert_eq!([texture.width(), texture.height()], [64, 48]);
        assert_eq!(texture.format(), wgpu::TextureFormat::Bgra8Unorm);
        assert!(
            texture
                .usage()
                .contains(wgpu::TextureUsages::TEXTURE_BINDING)
        );

        drop(frame);
        assert!(surface.is_in_use(), "the texture holds the frame");
        drop(texture);
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device is alive");
        assert!(!surface.is_in_use(), "the texture freed, the frame goes");
    }

    #[test]
    fn a_decoded_frames_planes_are_imported_brightness_then_colour_at_half_size() {
        let gpu = TestGpu::new();
        let frame = test_frame_in(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
        let buffer = frame.hardware_buffer();
        assert_eq!(buffer.format(), HardwareBufferFormat::Nv12);
        assert!(
            import_texture(&*buffer, &gpu.device).is_none(),
            "two planes are no one texture"
        );
        let planes = import_planes(&*buffer, &gpu.device).expect("a Metal device takes them");
        let shapes: Vec<_> = planes
            .iter()
            .map(wgpu::TextureView::texture)
            .map(|plane| (plane.format(), [plane.width(), plane.height()]))
            .collect();
        assert_eq!(
            shapes,
            [
                (wgpu::TextureFormat::R8Unorm, [64, 48]),
                (wgpu::TextureFormat::Rg8Unorm, [32, 24])
            ]
        );
        assert!(
            planes[1]
                .texture()
                .usage()
                .contains(wgpu::TextureUsages::TEXTURE_BINDING)
        );
    }

    #[test]
    fn a_10_bit_frames_planes_need_16_bit_textures() {
        let frame = test_frame_in(kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange);
        let buffer = frame.hardware_buffer();
        assert_eq!(buffer.format(), HardwareBufferFormat::P010);
        assert!(
            import_planes(&*buffer, &TestGpu::new().device).is_none(),
            "a device without 16-bit textures takes none"
        );
        let gpu = TestGpu::with_features(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);
        let planes = import_planes(&*buffer, &gpu.device).expect("a Metal device takes them");
        let formats: Vec<_> = planes
            .iter()
            .map(|plane| plane.texture().format())
            .collect();
        assert_eq!(
            formats,
            [
                wgpu::TextureFormat::R16Unorm,
                wgpu::TextureFormat::Rg16Unorm
            ]
        );
    }

    #[test]
    fn a_buffers_one_plane_is_its_texture() {
        let gpu = TestGpu::new();
        let planes =
            import_planes(&*test_frame().hardware_buffer(), &gpu.device).expect("one plane");
        assert_eq!(planes.len(), 1);
        assert_eq!(
            planes[0].texture().format(),
            wgpu::TextureFormat::Bgra8Unorm
        );
    }

    #[test]
    fn a_buffer_another_host_handed_out_is_not_imported() {
        struct Elsewhere;

        impl HardwareBuffer for Elsewhere {
            fn width(&self) -> u32 {
                4
            }

            fn height(&self) -> u32 {
                4
            }

            fn usage(&self) -> HardwareBufferUsage {
                HardwareBufferUsage::ALL
            }

            fn format(&self) -> HardwareBufferFormat {
                HardwareBufferFormat::Bgra8
            }

            fn read_pixels(
                &self,
            ) -> waft_embedder::image_codecs::ImageFuture<waft_embedder::image_codecs::PixelBuffer>
            {
                unreachable!("these tests never read it")
            }
        }

        let gpu = TestGpu::new();
        assert!(import_texture(&Elsewhere, &gpu.device).is_none());
        assert!(import_planes(&Elsewhere, &gpu.device).is_none());
    }

    /// The system's Metal device, as a host's views draw with it.
    struct TestGpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
    }

    impl TestGpu {
        fn new() -> TestGpu {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let options = wgpu::RequestAdapterOptions::default();
            let (_, device, queue) = pollster::block_on(valo::request_device(
                &instance,
                &options,
                valo::FeatureLevel::Core,
            ))
            .expect("a Metal device");
            TestGpu { device, queue }
        }

        /// The system's Metal device with `features` beyond wgpu's defaults, as an app that asks
        /// for them has.
        fn with_features(features: wgpu::Features) -> TestGpu {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter = pollster::block_on(
                instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
            )
            .expect("a Metal adapter");
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    required_features: features,
                    ..Default::default()
                }))
                .expect("a Metal device with the features");
            TestGpu { device, queue }
        }
    }
}
