//! The GPU a device of wgpu's Vulkan backend runs on, as Linux names it to every API and process:
//! the device number of its DRM render node, which Vulkan gives where the driver offers
//! `VK_EXT_physical_device_drm` (`VkPhysicalDeviceDrmPropertiesEXT`), as Mesa's drivers and
//! NVIDIA's do. What `Platform::gpu_id` answers, in the form GBM's node and the compositor's
//! dmabuf feedback are named in (`dmabuf_feedback`), so the same GPU compares equal.

// Vulkan's properties are read through ash's raw calls, which need `unsafe`; this module writes it
// for them.
#![allow(unsafe_code)]

use ash::vk;
use waft_embedder::GpuId;
use wgpu::hal::api::Vulkan;
use wgpu::hal::vulkan::Device;

/// The render node of the GPU `device` runs on; `None` for a device not on Vulkan, and for one
/// whose driver names no render node.
pub(super) fn render_node_of(device: &wgpu::Device) -> Option<GpuId> {
    // SAFETY: the device is only read, and is let go before this returns.
    let hal = unsafe { device.as_hal::<Vulkan>() }?;
    if !offers_drm_properties(&hal) {
        return None;
    }
    let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
    read_properties(
        &hal,
        &mut vk::PhysicalDeviceProperties2::default().push_next(&mut drm),
    )?;
    (drm.has_render == vk::TRUE).then_some(GpuId::DrmRenderNode {
        major: drm.render_major as u32,
        minor: drm.render_minor as u32,
    })
}

/// Whether the driver of `device`'s physical device offers `VK_EXT_physical_device_drm`.
fn offers_drm_properties(device: &Device) -> bool {
    let instance = device.shared_instance().raw_instance();
    // SAFETY: a live physical device of this instance.
    let extensions =
        unsafe { instance.enumerate_device_extension_properties(device.raw_physical_device()) };
    extensions.is_ok_and(|extensions| {
        extensions.iter().any(|extension| {
            extension.extension_name_as_c_str() == Ok(ash::ext::physical_device_drm::NAME)
        })
    })
}

/// `properties` and the structures chained to it filled in for `device`'s physical device, as
/// wgpu-hal reads them: through `VK_KHR_get_physical_device_properties2` where the instance has
/// it, and through Vulkan 1.1 otherwise; `None` for an instance that has neither.
fn read_properties(device: &Device, properties: &mut vk::PhysicalDeviceProperties2) -> Option<()> {
    let shared = device.shared_instance();
    let instance = shared.raw_instance();
    let physical_device = device.raw_physical_device();
    if shared
        .extensions()
        .contains(&ash::khr::get_physical_device_properties2::NAME)
    {
        let extended =
            ash::khr::get_physical_device_properties2::Instance::new(shared.entry(), instance);
        // SAFETY: a live physical device of the instance the extension is enabled on, and a chain
        // of structures that live across the call.
        unsafe { extended.get_physical_device_properties2(physical_device, properties) };
    } else if shared.instance_api_version() >= vk::API_VERSION_1_1 {
        // SAFETY: a live physical device of a Vulkan 1.1 instance, and a chain of structures that
        // live across the call.
        unsafe { instance.get_physical_device_properties2(physical_device, properties) };
    } else {
        return None;
    }
    Some(())
}
