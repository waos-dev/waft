//! The D3D11 device a decoder decodes on and puts its pictures out with. A texture shared by NT
//! handle opens only on a device of the adapter that made it, so the device is on the adapter
//! asked for, the app's; on the system's default adapter where none is.
//!
//! Each picture is put out in a texture of its own, in NV12 and shared by NT handle
//! ([`SharedTexture`]), so the app holds it as long as it likes while the decoder goes on: copied
//! on the GPU out of the decoder's own pool where the decoder runs on the video hardware, and
//! uploaded from the decoder's memory where it runs on the processor. The copy is waited for
//! before the picture is put out, since the app's device does not share this one's queue.
//!
//! The invariant: the device's immediate context is used only on the decoder's own thread; the
//! decoder itself, given the device, reaches it from its threads, which the device's multithread
//! protection serialises.

use std::os::windows::io::{FromRawHandle as _, OwnedHandle};
use std::sync::Arc;

use waft_embedder::HardwareBufferUsage;
use waft_embedder::video_decoders::DecoderError;
use windows::Win32::Foundation::{E_POINTER, HMODULE, LUID};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_WRITE, D3D11_CREATE_DEVICE_FLAG,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
    D3D11_DECODER_PROFILE_H264_VLD_NOFGT, D3D11_MAP_WRITE, D3D11_MAPPED_SUBRESOURCE,
    D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_RESOURCE_MISC_SHARED,
    D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Multithread, ID3D11Query, ID3D11Texture2D, ID3D11VideoDevice,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE, IDXGIAdapter,
    IDXGIFactory4, IDXGIResource1,
};
use windows::core::Interface as _;

use super::super::shared_textures::SharedTexture;

/// What a decoded picture's texture is for: the app samples it.
const PICTURE_USAGE: HardwareBufferUsage = HardwareBufferUsage::GPU_SAMPLED;

/// A decoder's D3D11 device and its immediate context.
pub(super) struct VideoDevice {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// Where pictures from the decoder's memory are written before they are copied to the GPU,
    /// made again when the pictures change size.
    staging: Option<(ID3D11Texture2D, [u32; 2])>,
}

impl VideoDevice {
    /// A device on the adapter whose LUID is `adapter`, or on the default adapter, with its
    /// multithread protection on: for video where the adapter has video hardware, and for
    /// textures alone otherwise, as an adapter drawn on the processor is, which the decoder on the
    /// processor needs no more than.
    pub(super) fn new(adapter: Option<LUID>) -> Result<VideoDevice, DecoderError> {
        let adapter = adapter.map(adapter_of).transpose()?;
        let (device, context) = device_on(adapter.as_ref(), D3D11_CREATE_DEVICE_VIDEO_SUPPORT)
            .or_else(|_| device_on(adapter.as_ref(), D3D11_CREATE_DEVICE_FLAG(0)))
            .map_err(|error| DecoderError::Failed(format!("no D3D11 device: {error}")))?;
        let protection: ID3D11Multithread = device.cast().map_err(failed)?;
        // SAFETY: a call on the device's own interface; the old value it answers is not needed.
        let _ = unsafe { protection.SetMultithreadProtected(true) };
        Ok(VideoDevice {
            device,
            context,
            staging: None,
        })
    }

    pub(super) fn device(&self) -> &ID3D11Device {
        &self.device
    }

    /// Whether the video hardware decodes H.264 on this device: whether it offers DXVA's H.264
    /// decoding profile.
    pub(super) fn decodes_h264(&self) -> bool {
        let Ok(video) = self.device.cast::<ID3D11VideoDevice>() else {
            return false;
        };
        // SAFETY: calls on the device's own video interface, each index below the count it gave.
        unsafe {
            (0..video.GetVideoDecoderProfileCount()).any(|index| {
                video
                    .GetVideoDecoderProfile(index)
                    .is_ok_and(|profile| profile == D3D11_DECODER_PROFILE_H264_VLD_NOFGT)
            })
        }
    }

    /// `subresource` of `decoded`, a texture of the decoder's own pool, copied into a texture of
    /// its own, `size` pixels in NV12, shared, and finished on the GPU.
    pub(super) fn copy_out(
        &mut self,
        decoded: &ID3D11Texture2D,
        subresource: u32,
        size: [u32; 2],
    ) -> Result<Arc<SharedTexture>, DecoderError> {
        let (texture, shared) = self.shared_texture(size)?;
        // SAFETY: both textures are live NV12 textures of this device; the source's subresource
        // is the one the decoder named, at least `size` pixels.
        unsafe {
            self.context
                .CopySubresourceRegion(&texture, 0, 0, 0, 0, decoded, subresource, None);
        }
        self.finish()?;
        Ok(shared)
    }

    /// `planes`, a picture in the decoder's memory in NV12 (`size` pixels, its rows `pitch` bytes
    /// apart, its chroma `chroma_offset` bytes in), uploaded into a texture of its own, shared, and
    /// finished on the GPU.
    pub(super) fn upload(
        &mut self,
        planes: &[u8],
        pitch: usize,
        chroma_offset: usize,
        size: [u32; 2],
    ) -> Result<Arc<SharedTexture>, DecoderError> {
        let staging = self.staging_of(size)?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: the staging texture is this device's, mapped for writing on its immediate
        // context and unmapped before anything else uses it.
        unsafe { self.context.Map(&staging, 0, D3D11_MAP_WRITE, 0, Some(&mut mapped)) }
            .map_err(failed)?;
        let row = size[0] as usize;
        let rows = size[1] as usize;
        let mapped_pitch = mapped.RowPitch as usize;
        let mapped_chroma = mapped_pitch * rows;
        let copy_plane = |source_at: usize, mapped_at: usize, rows: usize| {
            for line in 0..rows {
                let from = source_at + line * pitch;
                let Some(source) = planes.get(from..from + row) else {
                    return;
                };
                // SAFETY: the mapping holds the texture's rows `RowPitch` apart, the luma plane's
                // `rows` of them and then the chroma plane's half as many, each `row` bytes wide.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        source.as_ptr(),
                        mapped.pData.cast::<u8>().add(mapped_at + line * mapped_pitch),
                        row,
                    );
                }
            }
        };
        copy_plane(0, 0, rows);
        copy_plane(chroma_offset, mapped_chroma, rows / 2);
        // SAFETY: the texture mapped above.
        unsafe { self.context.Unmap(&staging, 0) };
        let (texture, shared) = self.shared_texture(size)?;
        // SAFETY: both textures are this device's, of the same size and format.
        unsafe { self.context.CopyResource(&texture, &staging) };
        self.finish()?;
        Ok(shared)
    }

    /// A texture of `size` pixels in NV12, shared by an NT handle, and the buffer the app holds of
    /// it.
    fn shared_texture(
        &self,
        size: [u32; 2],
    ) -> Result<(ID3D11Texture2D, Arc<SharedTexture>), DecoderError> {
        let description = D3D11_TEXTURE2D_DESC {
            Width: size[0],
            Height: size[1],
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0)
                as u32,
        };
        let mut texture = None;
        // SAFETY: the description is a live local, and the out pointer one.
        unsafe { self.device.CreateTexture2D(&description, None, Some(&mut texture)) }
            .map_err(failed)?;
        let texture = texture.ok_or_else(|| DecoderError::Failed("no texture made".to_owned()))?;
        let resource: IDXGIResource1 = texture.cast().map_err(failed)?;
        // SAFETY: a call on the texture's own interface; the handle it answers is owned from here.
        let handle = unsafe {
            resource.CreateSharedHandle(
                None,
                (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0,
                None,
            )
        }
        .map_err(failed)?;
        // SAFETY: the handle was just made for this texture and nothing else owns it.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        let shared =
            SharedTexture::in_format(handle, size, wgpu::TextureFormat::NV12, PICTURE_USAGE);
        Ok((texture, Arc::new(shared)))
    }

    /// The staging texture for pictures of `size`, made again for another size.
    fn staging_of(&mut self, size: [u32; 2]) -> Result<ID3D11Texture2D, DecoderError> {
        if let Some((staging, made_for)) = &self.staging
            && *made_for == size
        {
            return Ok(staging.clone());
        }
        let description = D3D11_TEXTURE2D_DESC {
            Width: size[0],
            Height: size[1],
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            MiscFlags: 0,
        };
        let mut staging = None;
        // SAFETY: the description is a live local, and the out pointer one.
        unsafe { self.device.CreateTexture2D(&description, None, Some(&mut staging)) }
            .map_err(failed)?;
        let staging = staging.ok_or_else(|| DecoderError::Failed("no staging made".to_owned()))?;
        self.staging = Some((staging.clone(), size));
        Ok(staging)
    }

    /// Waits until the GPU has done what the context was handed, so another device reads a
    /// finished picture.
    fn finish(&self) -> Result<(), DecoderError> {
        let description = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_EVENT,
            MiscFlags: 0,
        };
        let mut query: Option<ID3D11Query> = None;
        // SAFETY: the description is a live local, and the out pointer one.
        unsafe { self.device.CreateQuery(&description, Some(&mut query)) }.map_err(failed)?;
        let query = query.ok_or_else(|| DecoderError::Failed("no query made".to_owned()))?;
        // SAFETY: the query is this device's; `GetData` writes a BOOL, the size given, into a live
        // local.
        unsafe {
            self.context.End(&query);
            self.context.Flush();
            let mut done = windows::core::BOOL(0);
            loop {
                let answered = self.context.GetData(
                    &query,
                    Some((&raw mut done).cast()),
                    size_of::<windows::core::BOOL>() as u32,
                    0,
                );
                if answered.is_ok() && done.as_bool() {
                    return Ok(());
                }
                answered.map_err(failed)?;
                std::thread::yield_now();
            }
        }
    }
}

/// A D3D11 device made with `flags` on `adapter`, or on the default adapter.
fn device_on(
    adapter: Option<&IDXGIAdapter>,
    flags: D3D11_CREATE_DEVICE_FLAG,
) -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
    let driver_type = match adapter {
        Some(_) => D3D_DRIVER_TYPE_UNKNOWN,
        None => D3D_DRIVER_TYPE_HARDWARE,
    };
    let mut device = None;
    let mut context = None;
    // SAFETY: the adapter is a live one or none; the out pointers are live locals; no software
    // module or feature levels are passed.
    unsafe {
        D3D11CreateDevice(
            adapter,
            driver_type,
            HMODULE::default(),
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    device.zip(context).ok_or_else(|| windows::core::Error::from(E_POINTER))
}

/// The adapter whose LUID is `luid`.
fn adapter_of(luid: LUID) -> Result<IDXGIAdapter, DecoderError> {
    // SAFETY: a factory of this process, asked for one of its adapters.
    unsafe {
        let factory: IDXGIFactory4 = CreateDXGIFactory1().map_err(failed)?;
        factory.EnumAdapterByLuid(luid).map_err(|error| {
            DecoderError::Failed(format!("no adapter of the app's device here: {error}"))
        })
    }
}

fn failed(error: windows::core::Error) -> DecoderError {
    DecoderError::Failed(error.to_string())
}
