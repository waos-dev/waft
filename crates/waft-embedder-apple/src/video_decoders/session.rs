//! One VideoToolbox decompression session: made for a stream's format (`format`), handed a
//! picture's chunk as a sample, and answering the picture decoded, a `CVPixelBuffer` on an `IOSurface` in
//! the decoder's own layout, 8-bit 4:2:0 in two planes.
//!
//! A picture is decoded where it is asked for, the call returning once VideoToolbox has answered
//! it, so a caller that decodes on a queue of its own sees pictures in the order it handed their
//! samples over, which is the order they were decoded in and not the order they are shown in.
//!
//! The invariant: the session is torn down as this is dropped, and nothing of VideoToolbox's is
//! reached after that.

// VideoToolbox, CoreMedia and CoreVideo are reached here, and nowhere else in this module's
// files but `format`, which makes CoreMedia's formats.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::{Arc, Mutex, PoisonError};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFBoolean, CFDictionary, CFRetained, CFType};
use objc2_core_media::{
    CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMSampleTimingInfo, CMTime,
    kCMBlockBufferAssureMemoryNowFlag, kCMTimeInvalid,
};
use objc2_core_video::{
    CVImageBuffer, CVPixelBuffer, kCVPixelBufferIOSurfacePropertiesKey,
    kCVPixelBufferMetalCompatibilityKey, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use objc2_video_toolbox::{
    VTDecodeFrameFlags, VTDecodeInfoFlags, VTDecompressionSession, VTSessionCopyProperty,
    kVTCouldNotFindVideoDecoderErr,
    kVTDecompressionPropertyKey_UsingHardwareAcceleratedVideoDecoder, kVTInvalidSessionErr,
    kVTVideoDecoderBadDataErr, kVTVideoDecoderNotAvailableNowErr,
    kVTVideoDecoderReferenceMissingErr, kVTVideoDecoderUnsupportedDataFormatErr,
};
use waft_embedder::video_decoders::{CodecChoice, CodecKind, DecoderError};

use super::format::Format;
use crate::media::as_ns_string;

/// What VideoToolbox answers as a status.
type Status = i32;

/// The timescale of a time in nanoseconds.
const NANOSECONDS: i32 = 1_000_000_000;

pub(super) struct Session {
    session: CFRetained<VTDecompressionSession>,
    /// The format the session was made for, which each of its samples names.
    format: Format,
}

// SAFETY: a decompression session is a thread-safe Core Foundation object, made to be used from
// whichever queue decodes; the binding cannot say so.
unsafe impl Send for Session {}

impl Session {
    /// A session for a stream of `format`, of the kind `choice` accepts, its pictures over the
    /// whole range of their samples where `full_range`, and over video's range otherwise.
    pub(super) fn open(
        format: &Format,
        choice: CodecChoice,
        full_range: bool,
    ) -> Result<Session, DecoderError> {
        let specification = decoder_specification(choice)?;
        let specification = specification.as_deref().map(as_cf_dictionary);
        let attributes = picture_attributes(full_range);
        let mut session = ptr::null_mut();
        // SAFETY: a format made above, dictionaries of the keys' own value types, no callback
        // record, since each picture is answered to its own handler, and a valid place for the
        // session.
        let status = unsafe {
            VTDecompressionSession::create(
                None,
                &format.0,
                specification,
                Some(as_cf_dictionary(&attributes)),
                ptr::null(),
                NonNull::from(&mut session),
            )
        };
        let Some(session) = NonNull::new(session).filter(|_| status == 0) else {
            return Err(refusal_to_open(status));
        };
        // SAFETY: created just now, handed over with its one retain.
        let session = unsafe { CFRetained::from_raw(session) };
        Ok(Session {
            session,
            format: format.clone(),
        })
    }

    /// Whether the session goes on with a stream of `format`, as it does with other parameter
    /// sets of the same size and profile.
    pub(super) fn accepts(&self, format: &Format) -> bool {
        // SAFETY: a live session and a format.
        unsafe { self.session.can_accept_format_description(&format.0) }
    }

    /// Where the session decodes.
    pub(super) fn kind(&self) -> CodecKind {
        let mut value: *const CFType = ptr::null();
        // SAFETY: the session is one, the key is VideoToolbox's own, and `value` is a valid
        // place for the property, which arrives retained.
        let status = unsafe {
            let session: &CFType = self.session.as_ref();
            VTSessionCopyProperty(
                session,
                kVTDecompressionPropertyKey_UsingHardwareAcceleratedVideoDecoder,
                None,
                (&raw mut value).cast::<c_void>(),
            )
        };
        let Some(value) = NonNull::new(value.cast_mut()).filter(|_| status == 0) else {
            return CodecKind::Software;
        };
        // SAFETY: the property copied above, with its one retain; it is a Boolean.
        let value = unsafe { CFRetained::from_raw(value) };
        match value.downcast_ref::<CFBoolean>() {
            Some(hardware) if hardware.as_bool() => CodecKind::Hardware,
            _ => CodecKind::Software,
        }
    }

    /// Decodes `sample`, one picture's chunk as its format frames it, shown at `timestamp` for
    /// `duration`, both in nanoseconds: the picture, none where VideoToolbox dropped it, or why
    /// it decoded none.
    pub(super) fn decode(
        &self,
        sample: &[u8],
        timestamp: i64,
        duration: Option<u64>,
    ) -> Result<Option<DecodedPicture>, DecoderError> {
        let buffer = sample_buffer(sample, &self.format.0, timestamp, duration)
            .map_err(|status| DecoderError::Failed(format!("no sample buffer ({status})")))?;
        let answered = Arc::new(Mutex::new(None));
        let answer = Arc::clone(&answered);
        let handler = RcBlock::new(
            move |status: Status,
                  _flags: VTDecodeInfoFlags,
                  image: *mut CVImageBuffer,
                  _time: CMTime,
                  _duration: CMTime| {
                // SAFETY: VideoToolbox hands the picture over for the length of the call; it is
                // retained here to outlive it.
                let picture = NonNull::new(image)
                    .map(|image| DecodedPicture(unsafe { CFRetained::retain(image) }));
                *answer.lock().unwrap_or_else(PoisonError::into_inner) = Some((status, picture));
            },
        );
        // SAFETY: a live session, a sample made above, no flags, so the handler has been called
        // by the time this returns, and nowhere to write the info flags.
        let status = unsafe {
            self.session.decode_frame_with_output_handler(
                &buffer,
                VTDecodeFrameFlags(0),
                ptr::null_mut(),
                RcBlock::as_ptr(&handler),
            )
        };
        let answered = answered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match (status, answered) {
            (0, Some((0, picture))) => Ok(picture),
            (0, None) => Ok(None),
            (0, Some((status, _))) | (status, _) => Err(refusal_to_decode(status)),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: a live session, torn down once.
        unsafe { self.session.invalidate() };
    }
}

/// A picture VideoToolbox decoded: a pixel buffer on an `IOSurface`.
pub(super) struct DecodedPicture(pub(super) CFRetained<CVPixelBuffer>);

// SAFETY: a pixel buffer is made to be shared between threads: its retain count is atomic, and
// nothing here writes its pixels. The binding cannot say so.
unsafe impl Send for DecodedPicture {}

/// Which decoder VideoToolbox is asked for: hardware where it has it, hardware or none, or
/// never hardware. None for the system's own choice, which is the first.
#[cfg(target_os = "macos")]
fn decoder_specification(
    choice: CodecChoice,
) -> Result<Option<Retained<NSDictionary<NSString, AnyObject>>>, DecoderError> {
    use objc2_video_toolbox::{
        kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder,
        kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder,
    };

    // SAFETY: VideoToolbox's constants, which it exports for the life of the process.
    let (key, value) = unsafe {
        match choice {
            CodecChoice::Any => (
                kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder,
                true,
            ),
            CodecChoice::Hardware => (
                kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder,
                true,
            ),
            CodecChoice::Software => (
                kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder,
                false,
            ),
        }
    };
    let value = NSNumber::new_bool(value);
    Ok(Some(NSDictionary::from_slices(
        &[as_ns_string(key)],
        &[value.as_ref() as &AnyObject],
    )))
}

/// Which decoder VideoToolbox is asked for: its own choice, which on iOS is hardware; iOS has no
/// way to ask for a software one.
#[cfg(not(target_os = "macos"))]
fn decoder_specification(
    choice: CodecChoice,
) -> Result<Option<Retained<NSDictionary<NSString, AnyObject>>>, DecoderError> {
    match choice {
        CodecChoice::Any | CodecChoice::Hardware => Ok(None),
        CodecChoice::Software => Err(DecoderError::Unsupported(
            "this system decodes video in hardware alone".to_owned(),
        )),
    }
}

/// What the session's pictures are asked in: 8-bit 4:2:0 in two planes, over the range the
/// stream's samples use, so nothing is rescaled, on an `IOSurface` a Metal texture can be made
/// of.
fn picture_attributes(full_range: bool) -> Retained<NSDictionary<NSString, AnyObject>> {
    // SAFETY: CoreVideo's constants, which it exports for the life of the process.
    let (format_key, surface_key, metal_key) = unsafe {
        (
            as_ns_string(kCVPixelBufferPixelFormatTypeKey),
            as_ns_string(kCVPixelBufferIOSurfacePropertiesKey),
            as_ns_string(kCVPixelBufferMetalCompatibilityKey),
        )
    };
    let format = NSNumber::new_u32(if full_range {
        kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
    } else {
        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
    });
    let surface_properties = NSDictionary::<NSString, AnyObject>::new();
    let metal = NSNumber::new_bool(true);
    NSDictionary::from_slices(
        &[format_key, surface_key, metal_key],
        &[
            format.as_ref() as &AnyObject,
            surface_properties.as_ref(),
            metal.as_ref(),
        ],
    )
}

/// An `NSDictionary` as the `CFDictionary` it is toll-free bridged to.
fn as_cf_dictionary(dictionary: &NSDictionary<NSString, AnyObject>) -> &CFDictionary {
    let pointer: *const NSDictionary<NSString, AnyObject> = dictionary;
    // SAFETY: an NSDictionary is a CFDictionary.
    unsafe { &*pointer.cast::<CFDictionary>() }
}

/// A sample of `data` in `format`, shown at `timestamp` for `duration`, both in nanoseconds.
fn sample_buffer(
    data: &[u8],
    format: &CMFormatDescription,
    timestamp: i64,
    duration: Option<u64>,
) -> Result<CFRetained<CMSampleBuffer>, Status> {
    let mut block = ptr::null_mut();
    // SAFETY: memory of the data's length is allocated by the block itself, at once, and
    // `block` is a valid place for it.
    let status = unsafe {
        CMBlockBuffer::create_with_memory_block(
            None,
            ptr::null_mut(),
            data.len(),
            None,
            ptr::null(),
            0,
            data.len(),
            kCMBlockBufferAssureMemoryNowFlag,
            NonNull::from(&mut block),
        )
    };
    let Some(block) = NonNull::new(block).filter(|_| status == 0) else {
        return Err(status);
    };
    // SAFETY: created just now, handed over with its one retain.
    let block = unsafe { CFRetained::from_raw(block) };
    // SAFETY: `data` is `data.len()` bytes, copied into a block that holds as many.
    let status = unsafe {
        CMBlockBuffer::replace_data_bytes(
            NonNull::from(data).cast::<c_void>(),
            &block,
            0,
            data.len(),
        )
    };
    if status != 0 {
        return Err(status);
    }
    // SAFETY: both are plain values; an invalid time stands for one not given.
    let timing = unsafe {
        CMSampleTimingInfo {
            duration: duration.map_or(kCMTimeInvalid, |duration| {
                CMTime::new(duration as i64, NANOSECONDS)
            }),
            presentationTimeStamp: CMTime::new(timestamp, NANOSECONDS),
            decodeTimeStamp: kCMTimeInvalid,
        }
    };
    let size = data.len();
    let mut sample = ptr::null_mut();
    // SAFETY: one sample of one size with one timing, each read for the call, over a block and
    // a format that are alive, and a valid place for the sample.
    let status = unsafe {
        CMSampleBuffer::create_ready(
            None,
            Some(&block),
            Some(format),
            1,
            1,
            &raw const timing,
            1,
            &raw const size,
            NonNull::from(&mut sample),
        )
    };
    match NonNull::new(sample) {
        // SAFETY: created just now, handed over with its one retain.
        Some(sample) if status == 0 => Ok(unsafe { CFRetained::from_raw(sample) }),
        _ => Err(status),
    }
}

/// Why VideoToolbox opened no session, as the app can act on it.
// The statuses are named as VideoToolbox names them.
#[allow(non_upper_case_globals)]
fn refusal_to_open(status: Status) -> DecoderError {
    match status {
        kVTVideoDecoderNotAvailableNowErr => DecoderError::Busy,
        kVTCouldNotFindVideoDecoderErr | kVTVideoDecoderUnsupportedDataFormatErr => {
            DecoderError::Unsupported(format!("VideoToolbox has no such decoder ({status})"))
        }
        _ => DecoderError::Failed(format!("VideoToolbox opened no decoder ({status})")),
    }
}

/// Why VideoToolbox decoded no picture, as the app can act on it: a stream it cannot follow
/// leaves the session decoding from the next key frame, a session the system invalidated is
/// lost, and anything else is the decoder's failure.
// The statuses are named as VideoToolbox names them.
#[allow(non_upper_case_globals)]
fn refusal_to_decode(status: Status) -> DecoderError {
    match status {
        kVTVideoDecoderBadDataErr | kVTVideoDecoderReferenceMissingErr => DecoderError::BadData(
            format!("VideoToolbox could not decode the chunk ({status})"),
        ),
        kVTInvalidSessionErr => DecoderError::Lost,
        _ => DecoderError::Failed(format!("VideoToolbox failed ({status})")),
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_video::{
        CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount,
        CVPixelBufferGetWidth,
    };

    use waft_video_codecs::framing::{units_after_start_codes, with_lengths};
    use waft_video_codecs::h264::test_clip::{CLIP, pictures_of_the_clip};
    use waft_video_codecs::h264::{ParameterSets, is_parameter_set};

    use super::*;

    /// The format of the H.264 clip.
    fn h264_format() -> Format {
        Format::h264(&ParameterSets::among(&units_after_start_codes(CLIP))).expect("a format")
    }

    #[test]
    fn videotoolbox_decodes_each_picture_of_a_stream_into_two_planes() {
        let format = h264_format();
        let session = Session::open(&format, CodecChoice::Any, false).expect("a session");
        let pictures = pictures_of_the_clip();
        assert_eq!(pictures.len(), 12);
        for (number, units) in pictures.iter().enumerate() {
            let coded = units.iter().copied().filter(|unit| !is_parameter_set(unit));
            let sample = with_lengths(coded);
            let decoded = session
                .decode(&sample, number as i64, None)
                .unwrap_or_else(|error| panic!("picture {number}: {error:?}"))
                .unwrap_or_else(|| panic!("picture {number} was dropped"));
            assert_eq!(CVPixelBufferGetWidth(&decoded.0), 320);
            assert_eq!(CVPixelBufferGetHeight(&decoded.0), 240);
            assert_eq!(CVPixelBufferGetPlaneCount(&decoded.0), 2);
            assert_eq!(
                CVPixelBufferGetPixelFormatType(&decoded.0),
                kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            );
        }
    }

    #[test]
    fn a_chunk_that_is_no_picture_is_bad_data_and_the_session_goes_on() {
        let format = h264_format();
        let session = Session::open(&format, CodecChoice::Any, false).expect("a session");
        let damaged = with_lengths([&[0x65, 0xFF, 0xFF, 0xFF][..]]);
        assert!(matches!(
            session.decode(&damaged, 0, None),
            Err(DecoderError::BadData(_))
        ));
        let first = &pictures_of_the_clip()[0];
        let sample = with_lengths(first.iter().copied().filter(|unit| !is_parameter_set(unit)));
        assert!(matches!(session.decode(&sample, 1, None), Ok(Some(_))));
    }

    #[test]
    fn a_session_says_where_it_decodes_and_goes_on_with_the_same_sets() {
        let format = h264_format();
        let software = Session::open(&format, CodecChoice::Software, false).expect("a session");
        assert_eq!(software.kind(), CodecKind::Software);
        assert!(software.accepts(&h264_format()));
    }
}
