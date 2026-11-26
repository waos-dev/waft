//! A VP9 stream as VideoToolbox takes it: its format made of the file's configuration record, or
//! of one made of what its key frame says where the file gives none, and of the size its key
//! frame says, so a session is made at the stream's first key frame; and each chunk whole, a
//! superframe and all. VP9 never reorders, so each chunk's picture stands after the one before.

use waft_embedder::video_decoders::{DecoderError, VideoStreamSetup};
use waft_video_codecs::presentation_order::Place;
use waft_video_codecs::vp9::{KeyFrame, Record};

use super::{Description, ReadChunk, StreamReader, another_codec};
use crate::video_decoders::format::Format;
use crate::video_decoders::session::add_vp9_decoder;

pub(super) struct Vp9Stream {
    /// The configuration record the app's file gave, where it gave one.
    record: Option<Record>,
    /// What the stream's last key frame said, once one has come.
    key: Option<KeyFrame>,
    /// How many chunks have come: the place of the next one's picture.
    chunks: i64,
}

impl Vp9Stream {
    pub(super) fn of(record: Option<Vec<u8>>) -> Result<Vp9Stream, DecoderError> {
        add_vp9_decoder();
        Ok(Vp9Stream {
            record: record_of(record)?,
            key: None,
            chunks: 0,
        })
    }
}

impl StreamReader for Vp9Stream {
    fn configure(&mut self, setup: VideoStreamSetup) -> Result<bool, DecoderError> {
        let VideoStreamSetup::Vp9(record) = setup else {
            return Err(another_codec(&setup));
        };
        let record = record_of(record)?;
        let changed = record != self.record && self.key.is_some();
        self.record = record;
        Ok(changed)
    }

    fn read(&mut self, chunk: &[u8]) -> Result<ReadChunk, DecoderError> {
        let key = KeyFrame::of_chunk(chunk).map_err(DecoderError::BadData)?;
        let changed = key.is_some() && key != self.key;
        if key.is_some() {
            self.key = key;
        }
        let place = Place {
            count: self.chunks,
            begins_sequence: key.is_some(),
        };
        self.chunks += 1;
        Ok(ReadChunk {
            changed,
            sample: chunk.to_vec(),
            place: Some(place),
        })
    }

    fn description(&self) -> Result<Option<Description>, DecoderError> {
        let Some(key) = &self.key else {
            return Ok(None);
        };
        let record = match self.record {
            Some(record) => record,
            None => Record::of_key_frame(key).map_err(DecoderError::BadData)?,
        };
        Ok(Some(Description {
            format: Format::vp9(&record, key.size)?,
            facts: key.facts(self.record.as_ref()),
        }))
    }
}

/// The record of `bytes`, the contents of the file's `vpcC` box, where it gave one.
fn record_of(bytes: Option<Vec<u8>>) -> Result<Option<Record>, DecoderError> {
    bytes
        .map(|bytes| Record::parse(&bytes).map_err(DecoderError::BadData))
        .transpose()
}

#[cfg(test)]
mod tests {
    use objc2_core_video::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
    use pollster::block_on;
    use waft_embedder::video_decoders::{EncodedChunk, VideoDecoder};
    use waft_video_codecs::vp9::test_clip::chunks_of_the_clip;

    use super::*;
    use crate::video_decoders::decoder::Decoder;
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

    #[test]
    fn a_stream_with_no_record_comes_out_in_order_from_its_first_key_frame() {
        let decoder = Decoder::open(VideoStreamSetup::Vp9(None), &options()).expect("a decoder");
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
    fn a_stream_with_its_files_record_comes_out_the_same() {
        let key = KeyFrame::of_chunk(chunks_of_the_clip()[0])
            .unwrap()
            .expect("a key frame");
        let record = Record {
            level: 20,
            ..Record::of_key_frame(&key).unwrap()
        };
        let setup = VideoStreamSetup::Vp9(Some(record.to_bytes()));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
        assert_eq!(times_decoded(&decoder, chunks()), shown_in_order(12));
    }

    #[test]
    fn a_chunk_before_the_first_key_frame_is_bad_data() {
        let decoder = Decoder::open(VideoStreamSetup::Vp9(None), &options()).expect("a decoder");
        let later = chunks().remove(2);
        assert!(matches!(
            block_on(decoder.decode(later)),
            Err(DecoderError::BadData(_))
        ));
    }
}
