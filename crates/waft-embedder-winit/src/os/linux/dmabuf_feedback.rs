//! What a Wayland compositor says it imports, as Waft's [`AcceptedBuffers`]: the default feedback
//! of `zwp_linux_dmabuf_v1` from version 4, a format table and tranches each naming the device it
//! is for, or at version 3 the global's own list of modifiers, which names no device.
//!
//! The invariant a later editor must keep: a device the compositor names is turned into its render
//! node before anything compares it, as Chromium does with `drmGetDeviceFromDevId`. The protocol
//! leaves the node type to the compositor ("Clients cannot check two devices for equality by
//! comparing the dev_t value"), and Vulkan and GBM name a GPU by its render node.
//!
//! The format table is shared memory the compositor made, mapped here to be read, which is why
//! this module allows `unsafe`.
#![allow(unsafe_code)]

use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Mutex;

use waft_embedder::{AcceptedBuffers, AcceptedFormat, GpuId, GpuMemory, HardwareBufferFormat};

use super::dmabuf::ARGB8888;

/// `DRM_FORMAT_ABGR8888`, RGBA in memory.
const ABGR8888: u32 = u32::from_le_bytes(*b"AB24");

/// The feedback read so far, filled by its events on Waft's queue.
#[derive(Default)]
pub(crate) struct Feedback(Mutex<Reading>);

#[derive(Default)]
struct Reading {
    /// The format table's entries: a DRM format and a modifier each.
    table: Vec<(u32, u64)>,
    main_device: Option<u64>,
    /// The tranche whose events are arriving: its device, and its formats as the table's indices.
    tranche: (Option<u64>, Vec<u16>),
    /// The tranches done, in the compositor's order of preference.
    tranches: Vec<(u64, Vec<u16>)>,
}

impl Feedback {
    pub(crate) fn format_table(&self, fd: OwnedFd, size: u32) {
        self.0.lock().unwrap().table = read_format_table(&fd, size as usize);
    }

    pub(crate) fn main_device(&self, device: &[u8]) {
        self.0.lock().unwrap().main_device = device_number(device);
    }

    pub(crate) fn tranche_target_device(&self, device: &[u8]) {
        self.0.lock().unwrap().tranche.0 = device_number(device);
    }

    pub(crate) fn tranche_formats(&self, indices: &[u8]) {
        let indices = indices
            .as_chunks::<2>()
            .0
            .iter()
            .map(|index| u16::from_ne_bytes(*index));
        self.0.lock().unwrap().tranche.1.extend(indices);
    }

    pub(crate) fn tranche_done(&self) {
        let reading = &mut *self.0.lock().unwrap();
        let (device, indices) = std::mem::take(&mut reading.tranche);
        if let Some(device) = device {
            reading.tranches.push((device, indices));
        }
    }

    /// What the feedback says, with `shared_memory` as the compositor offers it: one entry per
    /// render node, the main device's first.
    pub(crate) fn accepted_buffers(&self, shared_memory: bool) -> AcceptedBuffers {
        let reading = self.0.lock().unwrap();
        let mut gpu_memory = Vec::<GpuMemory>::new();
        let main_device = reading.main_device.and_then(render_node);
        if main_device.is_some() {
            gpu_memory.push(GpuMemory {
                gpu: main_device,
                formats: Vec::new(),
            });
        }
        for (device, indices) in &reading.tranches {
            let pairs = indices
                .iter()
                .filter_map(|index| reading.table.get(usize::from(*index)).copied());
            add_formats(&mut gpu_memory, render_node(*device), pairs);
        }
        gpu_memory.retain(|memory| !memory.formats.is_empty());
        AcceptedBuffers {
            gpu_memory,
            shared_memory,
        }
    }
}

/// What a compositor at version 3 says: the formats and modifiers of its global's events, for a
/// device it does not name.
pub(crate) fn accepted_at_version_3(
    pairs: impl IntoIterator<Item = (u32, u64)>,
    shared_memory: bool,
) -> AcceptedBuffers {
    let mut gpu_memory = Vec::new();
    add_formats(&mut gpu_memory, None, pairs);
    AcceptedBuffers {
        gpu_memory,
        shared_memory,
    }
}

/// Adds `pairs`, DRM formats with a modifier each, to `gpu`'s entry, made last where it has none;
/// formats Waft has no name for are left out.
fn add_formats(
    gpu_memory: &mut Vec<GpuMemory>,
    gpu: Option<GpuId>,
    pairs: impl IntoIterator<Item = (u32, u64)>,
) {
    let entry = match gpu_memory.iter().position(|memory| memory.gpu == gpu) {
        Some(entry) => entry,
        None => {
            gpu_memory.push(GpuMemory {
                gpu,
                formats: Vec::new(),
            });
            gpu_memory.len() - 1
        }
    };
    let formats = &mut gpu_memory[entry].formats;
    for (drm_format, modifier) in pairs {
        let Some(format) = format_of(drm_format) else {
            continue;
        };
        let accepted = match formats.iter_mut().position(|known| known.format == format) {
            Some(known) => &mut formats[known],
            None => {
                formats.push(AcceptedFormat {
                    format,
                    modifiers: Vec::new(),
                });
                formats.last_mut().expect("just pushed")
            }
        };
        if !accepted.modifiers.contains(&modifier) {
            accepted.modifiers.push(modifier);
        }
    }
}

/// The format a DRM fourcc names, where Waft names it.
fn format_of(drm_format: u32) -> Option<HardwareBufferFormat> {
    match drm_format {
        ARGB8888 => Some(HardwareBufferFormat::Bgra8),
        ABGR8888 => Some(HardwareBufferFormat::Rgba8),
        _ => None,
    }
}

/// The table's entries, 16 bytes each: a format, four bytes of padding, a modifier. Empty where the
/// table does not map.
fn read_format_table(fd: &OwnedFd, size: usize) -> Vec<(u32, u64)> {
    if size == 0 {
        return Vec::new();
    }
    // SAFETY: the compositor's table, `size` bytes it never changes once sent, mapped private for
    // reading and unmapped before this returns.
    unsafe {
        let mapped = libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            fd.as_raw_fd(),
            0,
        );
        if mapped == libc::MAP_FAILED {
            return Vec::new();
        }
        let bytes = std::slice::from_raw_parts(mapped.cast::<u8>(), size);
        let entries = bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|entry| {
                let format = u32::from_ne_bytes(entry[0..4].try_into().expect("four bytes"));
                let modifier = u64::from_ne_bytes(entry[8..16].try_into().expect("eight bytes"));
                (format, modifier)
            })
            .collect();
        libc::munmap(mapped, size);
        entries
    }
}

/// A `dev_t` as the protocol sends it, in the machine's byte order.
fn device_number(bytes: &[u8]) -> Option<u64> {
    Some(u64::from_ne_bytes(bytes.try_into().ok()?))
}

/// The render node of the DRM device `device` is a node of, as sysfs lists the device's nodes;
/// `None` where it lists none.
fn render_node(device: u64) -> Option<GpuId> {
    let (major, minor) = (device_major(device), device_minor(device));
    let nodes = std::fs::read_dir(format!("/sys/dev/char/{major}:{minor}/device/drm")).ok()?;
    let render = nodes
        .flatten()
        .find(|node| node.file_name().to_string_lossy().starts_with("renderD"))?;
    let number = std::fs::read_to_string(render.path().join("dev")).ok()?;
    let (major, minor) = number.trim().split_once(':')?;
    Some(GpuId::DrmRenderNode {
        major: major.parse().ok()?,
        minor: minor.parse().ok()?,
    })
}

/// glibc's `major`.
fn device_major(device: u64) -> u32 {
    (((device >> 8) & 0xfff) | ((device >> 32) & 0xffff_f000)) as u32
}

/// glibc's `minor`.
fn device_minor(device: u64) -> u32 {
    ((device & 0xff) | ((device >> 12) & 0xffff_ff00)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_number_splits_as_glibc_splits_it() {
        // makedev(226, 128) and makedev(226, 0).
        assert_eq!((device_major(0xe280), device_minor(0xe280)), (226, 128));
        assert_eq!((device_major(0xe200), device_minor(0xe200)), (226, 0));
    }

    #[test]
    fn formats_waft_names_are_kept_per_gpu_with_their_modifiers_in_order() {
        let gpu = Some(GpuId::DrmRenderNode {
            major: 226,
            minor: 128,
        });
        let nv12 = u32::from_le_bytes(*b"NV12");
        let mut gpu_memory = Vec::new();
        add_formats(
            &mut gpu_memory,
            gpu,
            [(ARGB8888, 5), (nv12, 0), (ARGB8888, 0)],
        );
        add_formats(&mut gpu_memory, gpu, [(ARGB8888, 5), (ABGR8888, 0)]);
        assert_eq!(
            gpu_memory,
            vec![GpuMemory {
                gpu,
                formats: vec![
                    AcceptedFormat {
                        format: HardwareBufferFormat::Bgra8,
                        modifiers: vec![5, 0],
                    },
                    AcceptedFormat {
                        format: HardwareBufferFormat::Rgba8,
                        modifiers: vec![0],
                    },
                ],
            }]
        );
    }

    #[test]
    fn version_3_names_no_gpu() {
        let accepted = accepted_at_version_3([(ARGB8888, 0)], true);
        assert_eq!(accepted.gpu(), None);
        assert_eq!(accepted.gpu_memory.len(), 1);
        assert!(accepted.shared_memory);
    }
}
