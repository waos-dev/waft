//! An HEVC stream as VideoToolbox takes it: its parameter sets as its format, wherever they
//! came from, its setup or its chunks, and each chunk's other units after lengths of four bytes.

use waft_embedder::video_decoders::{DecoderError, NalFormat, VideoStreamSetup};
use waft_video_codecs::framing::{Framing, with_lengths};
use waft_video_codecs::hevc::{self, ParameterSets, PictureOrder, is_parameter_set};

use super::{Description, ReadChunk, StreamReader, another_codec};
use crate::video_decoders::format::Format;

pub(super) struct HevcStream {
    framing: Framing,
    /// The stream's parameter sets, once a whole set of them has come.
    sets: ParameterSets,
    /// Where each picture stands in the order its stream is shown in, as the stream says.
    picture_order: PictureOrder,
}

impl HevcStream {
    pub(super) fn of(format: NalFormat) -> Result<HevcStream, DecoderError> {
        let (framing, sets) = hevc::setup(format).map_err(DecoderError::BadData)?;
        let mut stream = HevcStream {
            framing,
            sets: ParameterSets::default(),
            picture_order: PictureOrder::default(),
        };
        stream.take_sets(sets)?;
        Ok(stream)
    }

    /// Goes on with `newer` in place of the sets of each kind it holds, where that makes a whole
    /// set other than the one held; whether it did.
    fn take_sets(&mut self, newer: ParameterSets) -> Result<bool, DecoderError> {
        let sets = self.sets.replaced_by(newer);
        if !sets.are_complete() || sets == self.sets {
            return Ok(false);
        }
        self.picture_order
            .adopt(&sets)
            .map_err(DecoderError::BadData)?;
        self.sets = sets;
        Ok(true)
    }
}

impl StreamReader for HevcStream {
    fn configure(&mut self, setup: VideoStreamSetup) -> Result<bool, DecoderError> {
        let VideoStreamSetup::Hevc(format) = setup else {
            return Err(another_codec(&setup));
        };
        let (framing, sets) = hevc::setup(format).map_err(DecoderError::BadData)?;
        self.framing = framing;
        self.take_sets(sets)
    }

    /// The chunk's units but its parameter sets, which go to the format, each after its length,
    /// its picture placed by its first slice segment. A picture is counted before it is decoded,
    /// so one VideoToolbox gives nothing for is counted all the same.
    fn read(&mut self, chunk: &[u8]) -> Result<ReadChunk, DecoderError> {
        let units = self.framing.units(chunk).map_err(DecoderError::BadData)?;
        let changed = self.take_sets(ParameterSets::among(&units))?;
        let coded: Vec<&[u8]> = units
            .into_iter()
            .filter(|unit| !is_parameter_set(unit))
            .collect();
        let place = self
            .picture_order
            .place_of(&coded)
            .map_err(DecoderError::BadData)?;
        Ok(ReadChunk {
            changed,
            sample: with_lengths(coded),
            place,
        })
    }

    fn description(&self) -> Result<Option<Description>, DecoderError> {
        if !self.sets.are_complete() {
            return Ok(None);
        }
        Ok(Some(Description {
            format: Format::hevc(&self.sets)?,
            facts: self.sets.facts().map_err(DecoderError::BadData)?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_video::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
    use waft_embedder::video_decoders::EncodedChunk;
    use waft_video_codecs::framing::units_after_start_codes;
    use waft_video_codecs::hevc::test_clip::{CLIP, MAIN_10_CLIP, pictures_of, record_of};

    use super::*;
    use crate::video_decoders::decoder::Decoder;
    use crate::video_decoders::test_decoding::{
        after_start_codes, chunks_of, frames_decoded, options, pixel_format_of, shown_in_order,
        times_decoded,
    };

    /// When each of the clips' pictures is shown, in the order they are decoded, counted in
    /// pictures: the counts `ffmpeg -bsf:v trace_headers` reads of their slices.
    const SHOWN_AT: [i64; 12] = [0, 3, 2, 1, 5, 4, 8, 7, 6, 11, 10, 9];
    const MAIN_10_SHOWN_AT: [i64; 12] = [0, 2, 1, 4, 3, 6, 5, 8, 7, 11, 10, 9];

    /// `clip`'s chunks in the order they are decoded, each framed by `framed`.
    fn chunks(
        clip: &[u8],
        shown_at: &[i64],
        framed: impl Fn(&[&[u8]]) -> Vec<u8>,
    ) -> Vec<EncodedChunk> {
        let data = pictures_of(clip)
            .iter()
            .map(|units| framed(units))
            .collect();
        chunks_of(data, shown_at)
    }

    #[test]
    fn a_stream_in_start_codes_comes_out_in_the_order_it_is_shown() {
        let setup = VideoStreamSetup::Hevc(NalFormat::StartCodes(None));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
        let times = times_decoded(&decoder, chunks(CLIP, &SHOWN_AT, after_start_codes));
        assert_eq!(times, shown_in_order(12));
    }

    #[test]
    fn a_stream_after_lengths_with_its_record_comes_out_the_same() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let setup = VideoStreamSetup::Hevc(NalFormat::LengthPrefixed(record_of(&sets)));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
        let without_sets = |units: &[&[u8]]| {
            with_lengths(units.iter().copied().filter(|unit| !is_parameter_set(unit)))
        };
        let times = times_decoded(&decoder, chunks(CLIP, &SHOWN_AT, without_sets));
        assert_eq!(times, shown_in_order(12));
    }

    /// A stream of ten bits to a sample comes out as the stream of eight does: VideoToolbox puts
    /// its pictures in the frames' one format, 8-bit 4:2:0 in two planes.
    #[test]
    fn a_main_10_stream_comes_out_in_order_in_eight_bits() {
        let setup = VideoStreamSetup::Hevc(NalFormat::StartCodes(None));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
        let frames = frames_decoded(
            &decoder,
            chunks(MAIN_10_CLIP, &MAIN_10_SHOWN_AT, after_start_codes),
        );
        let times: Vec<Option<i64>> = frames.iter().map(|frame| frame.timestamp()).collect();
        assert_eq!(times, shown_in_order(12));
        for frame in &frames {
            assert_eq!((frame.width(), frame.height()), (320, 240));
            assert_eq!(
                pixel_format_of(frame),
                kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            );
        }
    }
}
