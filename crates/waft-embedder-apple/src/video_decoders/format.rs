//! A stream's format as VideoToolbox takes it: a `CMFormatDescription` made of what the stream
//! says of itself, which a session is made for and each of its samples names.
//!
//! The invariant: a format describes samples whose units follow lengths of
//! [`LENGTH_SIZE`] bytes, the framing the stream readers (`streams`) give their samples.

// CoreMedia is reached here to make formats, as `session` reaches VideoToolbox.
#![allow(unsafe_code)]

use std::ffi::c_int;
use std::ptr::{self, NonNull};

use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    CMFormatDescription, CMVideoFormatDescriptionCreateFromH264ParameterSets,
    CMVideoFormatDescriptionCreateFromHEVCParameterSets,
};
use objc2_video_toolbox::kVTVideoDecoderBadDataErr;
use waft_embedder::video_decoders::DecoderError;

use waft_video_codecs::framing::LENGTH_SIZE;
use waft_video_codecs::{h264, hevc};

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
