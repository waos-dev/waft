//! A hardware buffer on Windows: a D3D12 texture another device made, shared by an NT handle, in
//! B8G8R8A8 as an app draws, or in NV12 as the system's video decoder puts out a picture
//! (`video_decoders`), which any D3D12 device on the same adapter opens over the same memory
//! (`ID3D12Device::OpenSharedHandle`). Waft makes none ([`Platform::hardware_buffer_allocator`]
//! answers `None` here); it shows the BGRA ones a producer hands it, copying each present into the
//! layer's own swapchain (`composition`), and imports them on a device for the app to sample: an
//! NV12 one only on a device that samples NV12.
//!
//! The invariant a later editor must keep: every use of the texture on a device of Waft's ends in
//! the common state, the one another device's work on it may start from, since D3D12 tracks a
//! resource's state per device.
//!
//! [`Platform::hardware_buffer_allocator`]: waft_embedder::Platform::hardware_buffer_allocator

use std::os::windows::io::{AsRawHandle as _, OwnedHandle};
use std::sync::Mutex;

use waft_embedder::HardwareBufferUsage;
use waft_embedder::image_codecs::{ImageError, ImageErrorKind, ImageFuture, PixelBuffer};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;

/// The format an app's shared texture is in: the engine's, BGRA as on Apple's systems.
pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// A texture shared by NT handle, `width` by `height` pixels, and the texture Waft's device copies
/// it from, opened once.
pub struct SharedTexture {
    handle: OwnedHandle,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: HardwareBufferUsage,
    /// The texture opened on the device that copies it into a layer, and that device.
    opened: Mutex<Option<(wgpu::Device, wgpu::Texture)>>,
}

impl SharedTexture {
    /// The BGRA texture whose NT handle is `handle`, `size` pixels, made for `usage`. The buffer
    /// owns the handle, and its memory lives as long as any handle or opened texture does.
    pub fn new(handle: OwnedHandle, size: [u32; 2], usage: HardwareBufferUsage) -> SharedTexture {
        SharedTexture::in_format(handle, size, FORMAT, usage)
    }

    /// The texture whose NT handle is `handle`, `size` pixels in `format`, made for `usage`:
    /// [`Bgra8Unorm`](wgpu::TextureFormat::Bgra8Unorm) as an app draws, or
    /// [`NV12`](wgpu::TextureFormat::NV12) as a video decoder puts out a picture.
    pub fn in_format(
        handle: OwnedHandle,
        size: [u32; 2],
        format: wgpu::TextureFormat,
        usage: HardwareBufferUsage,
    ) -> SharedTexture {
        SharedTexture {
            handle,
            width: size[0],
            height: size[1],
            format,
            usage,
            opened: Mutex::new(None),
        }
    }

    /// The NT handle, which a reader duplicates rather than takes.
    pub fn handle(&self) -> &OwnedHandle {
        &self.handle
    }

    /// The format of the texture's pixels.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// The texture on `device` that a copy reads, opened at the first call for that device.
    pub(crate) fn texture_on(&self, device: &wgpu::Device) -> Option<wgpu::Texture> {
        let mut opened = self.opened.lock().unwrap();
        if let Some((on, texture)) = &*opened
            && on == device
        {
            return Some(texture.clone());
        }
        let texture = self.open_on(device, wgpu::TextureUsages::COPY_SRC)?;
        *opened = Some((device.clone(), texture.clone()));
        Some(texture)
    }

    /// The texture opened anew on `device`, for `usage`, as it is in the common state; `None` for
    /// a device not on D3D12, an NV12 texture on a device that does not sample NV12, and where the
    /// handle does not open there: another adapter's.
    pub(crate) fn open_on(
        &self,
        device: &wgpu::Device,
        usage: wgpu::TextureUsages,
    ) -> Option<wgpu::Texture> {
        if self.format == wgpu::TextureFormat::NV12
            && !device
                .features()
                .contains(wgpu::Features::TEXTURE_FORMAT_NV12)
        {
            return None;
        }
        let size = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };
        // SAFETY: the handle is this buffer's and alive; the resource D3D12 answers is owned by
        // the texture made of it, which wgpu-hal takes as a texture whose memory it does not own,
        // of the size and format the handle's texture was made with, one mip, one sample.
        let hal_texture = unsafe {
            let hal_device = device.as_hal::<wgpu_hal::api::Dx12>()?;
            let mut resource: Option<ID3D12Resource> = None;
            hal_device
                .raw_device()
                .OpenSharedHandle(HANDLE(self.handle.as_raw_handle()), &mut resource)
                .map_err(|error| eprintln!("waft: a shared texture does not open: {error}"))
                .ok()?;
            wgpu_hal::dx12::Device::texture_from_raw(
                resource?,
                self.format,
                wgpu::TextureDimension::D2,
                size,
                1,
                1,
            )
        };
        let descriptor = wgpu::TextureDescriptor {
            label: Some("waft shared texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage,
            view_formats: &[],
        };
        // SAFETY: the texture is the resource just opened, in the common state every user leaves
        // it in, as `PRESENT` is to wgpu-hal on D3D12.
        Some(unsafe {
            device.create_texture_from_hal::<wgpu_hal::api::Dx12>(
                hal_texture,
                &descriptor,
                wgpu::TextureUses::PRESENT,
            )
        })
    }
}

impl waft_embedder::HardwareBuffer for SharedTexture {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn usage(&self) -> HardwareBufferUsage {
        self.usage
    }

    /// A shared texture has no CPU mapping: its pixels reach the CPU only through a copy on a
    /// device, which the app makes by importing it.
    fn read_pixels(&self) -> ImageFuture<PixelBuffer> {
        Box::pin(std::future::ready(Err(ImageError::new(
            ImageErrorKind::Unsupported,
            "a shared texture is read through a device, not by the CPU".to_owned(),
        ))))
    }
}
