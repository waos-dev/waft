//! An H.264 stream as Media Foundation's decoder takes it: Annex B, each unit after a start code,
//! the parameter sets in the stream. A stream after lengths, as MP4 holds it, is rewritten so, and
//! the parameter sets its setup gives ahead, read by `waft-video-codecs`, are put into the first
//! picture the decoder is handed.
//!
//! The invariant: parameter sets given ahead go where H.264 puts them in an access unit, after its
//! delimiter where it has one (7.4.1.2.3). Put before the delimiter, they are an access unit of
//! their own to Media Foundation's decoder, which then gives the pictures after it the times of
//! the samples before.

use waft_embedder::video_decoders::{DecoderError, VideoStreamSetup};
use waft_video_codecs::framing::{Framing, with_start_codes};
use waft_video_codecs::h264::{self, unit_type};

/// The type of a unit that begins an access unit: its delimiter.
const DELIMITER: u8 = 9;

/// A stream's setup as the decoder takes it: its framing, and its parameter sets given ahead, in
/// Annex B; `Unsupported` for a codec other than H.264.
pub(super) fn framing_and_sets(
    setup: VideoStreamSetup,
) -> Result<(Framing, Vec<u8>), DecoderError> {
    match setup {
        VideoStreamSetup::H264(format) => {
            let (framing, sets) = h264::setup(format).map_err(DecoderError::BadData)?;
            Ok((framing, with_start_codes(sets.units())))
        }
        other => Err(DecoderError::Unsupported(format!(
            "this system's decoders are handed H.264 alone so far, not {:?}",
            other.codec()
        ))),
    }
}

/// `data`, a picture framed as `framing` says, in Annex B.
pub(super) fn in_annex_b(data: &[u8], framing: Framing) -> Result<Vec<u8>, DecoderError> {
    match framing {
        Framing::StartCodes => Ok(data.to_vec()),
        Framing::Lengths(_) => framing
            .units(data)
            .map(with_start_codes)
            .map_err(DecoderError::BadData),
    }
}

/// `picture`, in Annex B, with `sets`, parameter sets in Annex B, put where H.264 puts them: after
/// the picture's delimiter, or first where it has none.
pub(super) fn with_sets_ahead(mut picture: Vec<u8>, sets: Vec<u8>) -> Vec<u8> {
    let at = after_delimiter(&picture).unwrap_or(0);
    picture.splice(at..at, sets);
    picture
}

/// Where the unit after `picture`'s first begins, its start code included, where the first is a
/// delimiter.
fn after_delimiter(picture: &[u8]) -> Option<usize> {
    let start_code = |data: &[u8]| data.windows(3).position(|code| code == [0, 0, 1]);
    let first = start_code(picture)? + 3;
    if unit_type(picture.get(first..)?) != Some(DELIMITER) {
        return None;
    }
    let next = first + start_code(&picture[first..])?;
    Some(if picture[next - 1] == 0 {
        next - 1
    } else {
        next
    })
}

#[cfg(test)]
mod tests {
    use waft_embedder::video_decoders::NalFormat;
    use waft_video_codecs::framing::{units_after_start_codes, with_lengths};
    use waft_video_codecs::h264::ParameterSets;
    use waft_video_codecs::h264::test_clip::{CLIP, record_of};

    use super::*;

    #[test]
    fn a_record_gives_its_sets_in_annex_b_and_the_size_of_a_length() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let setup = VideoStreamSetup::H264(NalFormat::LengthPrefixed(record_of(&sets)));
        let (framing, ahead) = framing_and_sets(setup).expect("a record");
        assert_eq!(framing, Framing::Lengths(4));
        assert_eq!(ahead, with_start_codes(sets.units()));
    }

    #[test]
    fn a_picture_after_lengths_is_put_after_start_codes() {
        let units: [&[u8]; 2] = [&[0x65, 1, 2], &[0x06, 3]];
        let annex_b = in_annex_b(&with_lengths(units), Framing::Lengths(4)).expect("whole");
        assert_eq!(annex_b, [0, 0, 0, 1, 0x65, 1, 2, 0, 0, 0, 1, 0x06, 3]);
    }

    #[test]
    fn sets_ahead_go_after_a_pictures_delimiter() {
        let picture = vec![0, 0, 0, 1, 0x09, 0xF0, 0, 0, 0, 1, 0x65, 1];
        let sets = vec![0, 0, 0, 1, 0x67, 2];
        assert_eq!(
            with_sets_ahead(picture, sets.clone()),
            [
                0, 0, 0, 1, 0x09, 0xF0, 0, 0, 0, 1, 0x67, 2, 0, 0, 0, 1, 0x65, 1
            ]
        );
        let without_delimiter = vec![0, 0, 1, 0x65, 1];
        assert_eq!(
            with_sets_ahead(without_delimiter, sets),
            [0, 0, 0, 1, 0x67, 2, 0, 0, 1, 0x65, 1]
        );
    }

    #[test]
    fn a_length_past_its_chunk_is_bad_data() {
        let cut = [0, 0, 0, 9, 0x65];
        assert!(matches!(
            in_annex_b(&cut, Framing::Lengths(4)),
            Err(DecoderError::BadData(_))
        ));
    }
}
