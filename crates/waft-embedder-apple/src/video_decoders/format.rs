//! A stream's format as VideoToolbox takes it: a `CMFormatDescription` made of what the stream
//! says of itself, which a session is made for and each of its samples names.
//!
//! H.264's and HEVC's formats are made of their parameter sets; VP9's and AV1's of the
//! configuration record a file holds (`vpcC`, `av1C`), handed over as the sample description's
//! extension it is in a file, and the pictures' size.
//!
//! The invariant: an H.264 or HEVC format describes samples whose units follow lengths of
//! [`LENGTH_SIZE`] bytes, the framing the stream readers (`streams`) give their samples.

// CoreMedia is reached here to make formats, as `session` reaches VideoToolbox.
#![allow(unsafe_code)]

use std::ffi::c_int;
use std::ptr::{self, NonNull};

use objc2::runtime::AnyObject;
use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    CMFormatDescription, CMVideoCodecType, CMVideoFormatDescriptionCreate,
    CMVideoFormatDescriptionCreateFromH264ParameterSets,
    CMVideoFormatDescriptionCreateFromHEVCParameterSets,
    kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms, kCMVideoCodecType_VP9,
};
use objc2_foundation::{NSData, NSDictionary, NSString};
use objc2_video_toolbox::kVTVideoDecoderBadDataErr;
use waft_embedder::video_decoders::{DecoderError, PictureSize};

use waft_video_codecs::framing::LENGTH_SIZE;
use waft_video_codecs::{h264, hevc, vp9};

use super::session::as_cf_dictionary;
use crate::media::as_ns_string;

/// What VideoToolbox answers as a status.
type Status = i32;

/// A stream's format.
#[derive(Clone)]
pub(super) struct Format(pub(super) CFRetained<CMFormatDescription>);

// SAFETY: a format description is an immutable Core Foundation object, made to be used from
// whichever queue decodes; the bindings cannot say so.
unsafe impl Send for Format {}

impl Format {
    /// The format of an H.264 stream of `sets`.
    pub(super) fn h264(sets: &h264::ParameterSets) -> Result<Format, DecoderError> {
        let units: Vec<&[u8]> = sets.units().collect();
        // SAFETY: the function, given units of the sizes given and a valid place for the format,
        // as `of_parameter_sets` calls it.
        of_parameter_sets(&units, |count, pointers, sizes, format| unsafe {
            CMVideoFormatDescriptionCreateFromH264ParameterSets(
                None,
                count,
                pointers,
                sizes,
                LENGTH_SIZE as c_int,
                format,
            )
        })
        .map_err(refusal_of_parameter_sets)
    }

    /// The format of an HEVC stream of `sets`.
    pub(super) fn hevc(sets: &hevc::ParameterSets) -> Result<Format, DecoderError> {
        let units: Vec<&[u8]> = sets.units().collect();
        // SAFETY: the function, given units of the sizes given, no extensions, and a valid place
        // for the format, as `of_parameter_sets` calls it.
        of_parameter_sets(&units, |count, pointers, sizes, format| unsafe {
            CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                None,
                count,
                pointers,
                sizes,
                LENGTH_SIZE as c_int,
                None,
                format,
            )
        })
        .map_err(refusal_of_parameter_sets)
    }

    /// The format of a VP9 stream `record` describes, its pictures of `size`.
    pub(super) fn vp9(record: &vp9::Record, size: PictureSize) -> Result<Format, DecoderError> {
        of_record(kCMVideoCodecType_VP9, "vpcC", &record.to_bytes(), size).map_err(|status| {
            DecoderError::BadData(format!(
                "VideoToolbox made no format of the stream's VP9 record ({status})"
            ))
        })
    }
}

/// The format of a stream of `codec` whose configuration record is `record`, the contents of a
/// box named `atom`, its pictures of `size`: the record as the sample description's extension
/// it is in a file.
fn of_record(
    codec: CMVideoCodecType,
    atom: &str,
    record: &[u8],
    size: PictureSize,
) -> Result<Format, Status> {
    let record = NSData::with_bytes(record);
    let atoms = NSDictionary::<NSString, AnyObject>::from_slices(
        &[&*NSString::from_str(atom)],
        &[record.as_ref() as &AnyObject],
    );
    // SAFETY: CoreMedia's constant, which it exports for the life of the process.
    let atoms_key =
        as_ns_string(unsafe { kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms });
    let extensions = NSDictionary::<NSString, AnyObject>::from_slices(
        &[atoms_key],
        &[atoms.as_ref() as &AnyObject],
    );
    let mut format: *const CMFormatDescription = ptr::null();
    // SAFETY: a dictionary of the extensions' own value types, deep-copied by the call, and a
    // valid place for the format.
    let status = unsafe {
        CMVideoFormatDescriptionCreate(
            None,
            codec,
            size.width as i32,
            size.height as i32,
            Some(as_cf_dictionary(&extensions)),
            NonNull::from(&mut format),
        )
    };
    created(status, format)
}

/// The format `create` makes of `units`, each a parameter set: it is handed how many there are,
/// a pointer to each and its size, all alive for the call, which copies them, and a place for
/// the format.
fn of_parameter_sets(
    units: &[&[u8]],
    create: impl FnOnce(
        usize,
        NonNull<NonNull<u8>>,
        NonNull<usize>,
        NonNull<*const CMFormatDescription>,
    ) -> Status,
) -> Result<Format, Status> {
    let mut pointers: Vec<NonNull<u8>> = units
        .iter()
        .map(|unit| NonNull::from(*unit).cast::<u8>())
        .collect();
    let mut sizes: Vec<usize> = units.iter().map(|unit| unit.len()).collect();
    let (Some(pointers), Some(sizes)) = (
        NonNull::new(pointers.as_mut_ptr()),
        NonNull::new(sizes.as_mut_ptr()),
    ) else {
        return Err(kVTVideoDecoderBadDataErr);
    };
    let mut format: *const CMFormatDescription = ptr::null();
    let status = create(units.len(), pointers, sizes, NonNull::from(&mut format));
    created(status, format)
}

/// The format a CoreMedia function answered `status` and `format` for.
fn created(status: Status, format: *const CMFormatDescription) -> Result<Format, Status> {
    match NonNull::new(format.cast_mut()) {
        // SAFETY: created just now, handed over with its one retain.
        Some(format) if status == 0 => Ok(Format(unsafe { CFRetained::from_raw(format) })),
        _ => Err(status),
    }
}

/// Why no format was made of a stream's parameter sets: the stream says something VideoToolbox
/// cannot read.
fn refusal_of_parameter_sets(status: Status) -> DecoderError {
    DecoderError::BadData(format!(
        "VideoToolbox made no format of the stream's parameter sets ({status})"
    ))
}
