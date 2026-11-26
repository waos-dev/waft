//! The adapter a device of wgpu's D3D12 backend runs on, as Windows names it to every API and
//! process: its LUID (`ID3D12Device::GetAdapterLuid`), what `Platform::gpu_id` answers and the
//! compositor's own device says of itself. A device on another backend says no adapter here.

use waft_embedder::GpuId;

/// The LUID of the adapter `device` runs on; `None` for a device not on D3D12.
pub(super) fn luid_of(device: &wgpu::Device) -> Option<GpuId> {
    // SAFETY: a live device of wgpu's D3D12 backend, asked only for its adapter's LUID.
    let luid = unsafe {
        device
            .as_hal::<wgpu_hal::api::Dx12>()?
            .raw_device()
            .GetAdapterLuid()
    };
    Some(GpuId::Luid {
        low: luid.LowPart,
        high: luid.HighPart,
    })
}
