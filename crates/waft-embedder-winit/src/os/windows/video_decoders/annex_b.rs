//! A chunk as Media Foundation's decoders take one: H.264 and HEVC in Annex B, each unit after a
//! start code, the parameter sets in the stream, whatever framing the chunk came in; a VP9 or AV1
//! chunk whole. The parameter sets a setup gives ahead, as the stream's reader holds them, are put
//! into the first key frame the decoder is handed.
//!
//! The invariant: parameter sets given ahead go where H.264 and HEVC put them in an access unit,
//! after its delimiter where it has one (H.264 7.4.1.2.3, HEVC 7.4.2.4.4). Put before the
//! delimiter, they are an access unit of their own to Media Foundation's H.264 decoder, which then
//! gives the pictures after it the times of the samples before.

use waft_embedder::video_decoders::VideoCodec;
use waft_video_codecs::framing::with_start_codes;
use waft_video_codecs::stream_reader::{Configuration, ReadChunk};
use waft_video_codecs::{h264, hevc};

/// H.264's and HEVC's types of the unit that begins an access unit: its delimiter.
const H264_DELIMITER: u8 = 9;
const HEVC_DELIMITER: u8 = 35;

/// The parameter sets `configuration` holds, each a unit, for a codec that has them.
pub(super) fn parameter_sets(configuration: &Configuration<'_>) -> Vec<Vec<u8>> {
    match configuration {
        Configuration::H264(sets) => sets.units().map(<[u8]>::to_vec).collect(),
        Configuration::Hevc(sets) => sets.units().map(<[u8]>::to_vec).collect(),
        Configuration::Vp9 { .. } | Configuration::Av1(_) => Vec::new(),
    }
}

/// What the decoder of `codec` is handed of `chunk`: an H.264 or HEVC chunk's units in Annex B,
/// `sets` put after its delimiter, or first where it has none; a VP9 or AV1 chunk whole.
pub(super) fn sample_of(chunk: &ReadChunk<'_>, codec: VideoCodec, sets: &[Vec<u8>]) -> Vec<u8> {
    let units = chunk.units();
    let is_delimiter = |unit: &[u8]| match codec {
        VideoCodec::H264 => h264::unit_type(unit) == Some(H264_DELIMITER),
        VideoCodec::Hevc => hevc::unit_type(unit) == Some(HEVC_DELIMITER),
        VideoCodec::Vp8 | VideoCodec::Vp9 | VideoCodec::Av1 => false,
    };
    match codec {
        VideoCodec::H264 | VideoCodec::Hevc => {
            let at = usize::from(units.first().is_some_and(|unit| is_delimiter(unit)));
            let ordered = units[..at]
                .iter()
                .copied()
                .chain(sets.iter().map(Vec::as_slice))
                .chain(units[at..].iter().copied());
            with_start_codes(ordered)
        }
        VideoCodec::Vp8 | VideoCodec::Vp9 | VideoCodec::Av1 => units.concat(),
    }
}

#[cfg(test)]
mod tests {
    use waft_embedder::video_decoders::{NalFormat, VideoStreamSetup};
    use waft_video_codecs::framing::{units_after_start_codes, with_lengths};
    use waft_video_codecs::stream_reader::reader_for;

    use super::*;

    #[test]
    fn a_record_gives_its_sets_and_a_picture_after_lengths_goes_after_start_codes() {
        let clip = h264::test_clip::CLIP;
        let sets = h264::ParameterSets::among(&units_after_start_codes(clip));
        let record = h264::test_clip::record_of(&sets);
        let mut reader = reader_for(VideoStreamSetup::H264(NalFormat::LengthPrefixed(record)))
            .expect("a reader");
        let ahead = parameter_sets(&reader.description().unwrap().unwrap().configuration);
        assert_eq!(ahead, sets.units().map(<[u8]>::to_vec).collect::<Vec<_>>());
        let units: [&[u8]; 2] = [&[0x65, 1, 2], &[0x06, 3]];
        let picture = with_lengths(units);
        let read = reader.read(&picture).expect("whole");
        assert_eq!(
            sample_of(&read, VideoCodec::H264, &[]),
            [0, 0, 0, 1, 0x65, 1, 2, 0, 0, 0, 1, 0x06, 3]
        );
    }

    #[test]
    fn sets_ahead_go_after_a_pictures_delimiter() {
        let mut reader =
            reader_for(VideoStreamSetup::H264(NalFormat::StartCodes(None))).expect("a reader");
        let sets = [vec![0x67, 2]];
        let picture = [0, 0, 0, 1, 0x09, 0xF0, 0, 0, 0, 1, 0x65, 1];
        let read = reader.read(&picture).expect("whole");
        assert_eq!(
            sample_of(&read, VideoCodec::H264, &sets),
            [
                0, 0, 0, 1, 0x09, 0xF0, 0, 0, 0, 1, 0x67, 2, 0, 0, 0, 1, 0x65, 1
            ]
        );
        let without_delimiter = [0, 0, 1, 0x65, 1];
        let read = reader.read(&without_delimiter).expect("whole");
        assert_eq!(
            sample_of(&read, VideoCodec::H264, &sets),
            [0, 0, 0, 1, 0x67, 2, 0, 0, 0, 1, 0x65, 1]
        );
    }

    #[test]
    fn an_hevc_pictures_delimiter_is_its_own_type() {
        let mut reader =
            reader_for(VideoStreamSetup::Hevc(NalFormat::StartCodes(None))).expect("a reader");
        let sets = [vec![0x40, 0x01, 7]];
        let picture = [0, 0, 0, 1, 0x46, 0x01, 0x50, 0, 0, 0, 1, 0x26, 0x01, 9];
        let read = reader.read(&picture).expect("whole");
        assert_eq!(
            sample_of(&read, VideoCodec::Hevc, &sets),
            [
                0, 0, 0, 1, 0x46, 0x01, 0x50, 0, 0, 0, 1, 0x40, 0x01, 7, 0, 0, 0, 1, 0x26, 0x01, 9
            ]
        );
    }

    #[test]
    fn a_vp9_chunk_goes_whole() {
        let mut reader = reader_for(VideoStreamSetup::Vp9(None)).expect("a reader");
        let chunk = waft_video_codecs::vp9::test_clip::chunks_of_the_clip()[1];
        let read = reader.read(chunk).expect("whole");
        assert_eq!(sample_of(&read, VideoCodec::Vp9, &[]), chunk);
    }
}
