//! An AV1 stream as VideoToolbox takes it: its format made of the configuration record its latest
//! sequence header makes, from the file's record or from a chunk, so a session is made once one
//! has come; and each chunk whole. AV1 never reorders, so each chunk's picture stands after the
//! one before. VideoToolbox decodes AV1 in hardware alone, on the Macs and devices that have it.

use waft_embedder::video_decoders::{DecoderError, VideoStreamSetup};
use waft_video_codecs::av1::{SequenceHeader, obus};
use waft_video_codecs::presentation_order::Place;

use super::{Description, ReadChunk, StreamReader, another_codec};
use crate::video_decoders::format::Format;

pub(super) struct Av1Stream {
    /// The stream's latest sequence header, once one has come.
    sequence: Option<SequenceHeader>,
    /// How many chunks have come: the place of the next one's picture.
    chunks: i64,
}

impl Av1Stream {
    pub(super) fn of(record: Option<Vec<u8>>) -> Result<Av1Stream, DecoderError> {
        Ok(Av1Stream {
            sequence: sequence_of(record)?,
            chunks: 0,
        })
    }

    /// Goes on with `sequence`, where it is another than the one held; whether it did.
    fn take(&mut self, sequence: Option<SequenceHeader>) -> bool {
        let changed = sequence.is_some() && sequence != self.sequence;
        if changed {
            self.sequence = sequence;
        }
        changed
    }
}

impl StreamReader for Av1Stream {
    fn configure(&mut self, setup: VideoStreamSetup) -> Result<bool, DecoderError> {
        let VideoStreamSetup::Av1(record) = setup else {
            return Err(another_codec(&setup));
        };
        let sequence = sequence_of(record)?;
        Ok(self.take(sequence))
    }

    fn read(&mut self, chunk: &[u8]) -> Result<ReadChunk, DecoderError> {
        let units = obus(chunk).map_err(DecoderError::BadData)?;
        let sequence = SequenceHeader::among(&units).map_err(DecoderError::BadData)?;
        let begins_sequence = sequence.is_some();
        let changed = self.take(sequence);
        let place = Place {
            count: self.chunks,
            begins_sequence,
        };
        self.chunks += 1;
        Ok(ReadChunk {
            changed,
            sample: chunk.to_vec(),
            place: Some(place),
        })
    }

    fn description(&self) -> Result<Option<Description>, DecoderError> {
        let Some(sequence) = &self.sequence else {
            return Ok(None);
        };
        Ok(Some(Description {
            format: Format::av1(sequence)?,
            facts: sequence.facts(),
        }))
    }
}

/// The sequence header the file's record (`av1C`) holds, where it gave one that holds one.
fn sequence_of(record: Option<Vec<u8>>) -> Result<Option<SequenceHeader>, DecoderError> {
    let Some(record) = record else {
        return Ok(None);
    };
    SequenceHeader::of_record(&record).map_err(DecoderError::BadData)
}

#[cfg(test)]
mod tests {
    use objc2_core_media::kCMVideoCodecType_AV1;
    use objc2_core_video::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
    use waft_embedder::video_decoders::{CodecKind, EncodedChunk, VideoDecoder};
    use waft_video_codecs::av1::test_clip::chunks_of_the_clip;

    use super::*;
    use crate::video_decoders::decoder::Decoder;
    use crate::video_decoders::session::has_hardware_decoder;
    use crate::video_decoders::test_decoding::{
        chunks_of, frames_decoded, options, pixel_format_of, shown_in_order, times_decoded,
    };

    /// The clip's chunks, each picture shown after the one before.
    fn chunks() -> Vec<EncodedChunk> {
        let data = chunks_of_the_clip()
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect();
        chunks_of(data, &(0..12).collect::<Vec<i64>>())
    }

    /// Whether this machine decodes AV1, which VideoToolbox does in hardware alone: Apple
    /// silicon from the M3 and the A17 Pro. Elsewhere the tests say so and check nothing.
    fn decodes_av1() -> bool {
        let decodes = has_hardware_decoder(kCMVideoCodecType_AV1);
        if !decodes {
            eprintln!("skipped: this machine's VideoToolbox has no AV1 decoder");
        }
        decodes
    }

    #[test]
    fn a_stream_with_no_record_comes_out_in_order_from_its_first_key_frame() {
        if !decodes_av1() {
            return;
        }
        let decoder = Decoder::open(VideoStreamSetup::Av1(None), &options()).expect("a decoder");
        let frames = frames_decoded(&decoder, chunks());
        let times: Vec<Option<i64>> = frames.iter().map(|frame| frame.timestamp()).collect();
        assert_eq!(times, shown_in_order(12), "a picture for each chunk");
        for frame in &frames {
            assert_eq!((frame.width(), frame.height()), (320, 240));
            assert_eq!(
                pixel_format_of(frame),
                kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            );
        }
    }

    #[test]
    fn a_stream_with_its_files_record_decodes_in_hardware_from_the_start() {
        if !decodes_av1() {
            return;
        }
        let units = obus(chunks_of_the_clip()[0]).unwrap();
        let sequence = SequenceHeader::among(&units).unwrap().expect("a header");
        let setup = VideoStreamSetup::Av1(Some(sequence.record()));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
        assert_eq!(decoder.kind(), CodecKind::Hardware);
        assert_eq!(times_decoded(&decoder, chunks()), shown_in_order(12));
    }

    #[test]
    fn a_record_that_is_no_record_is_bad_data() {
        assert!(matches!(
            Decoder::open(VideoStreamSetup::Av1(Some(vec![1, 2])), &options()),
            Err(DecoderError::BadData(_))
        ));
    }
}
