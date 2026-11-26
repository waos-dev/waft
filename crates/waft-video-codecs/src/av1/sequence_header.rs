//! What an AV1 sequence header says of its stream (AV1, 5.5): its profile and level, the depth,
//! colour and size of its pictures; and the configuration record (`av1C`, the AV1 codec ISO
//! media file format binding, 2.3) a file holds it in, which a system's decoder is configured
//! with (VideoToolbox's format). A file's record may leave the header out, so a stream's header
//! may come only with its first key frame.
//!
//! The invariant: two headers are the same stream's exactly when their units are the same bytes;
//! a decoder goes on with one only where it was configured with its bytes.

use std::io::Cursor;

use scuffle_av1::seq::SequenceHeaderObu;
use scuffle_av1::{AV1CodecConfigurationRecord, ObuHeader};
use waft_embedder::video_decoders::PictureSize;
use waft_embedder::{ChromaAlignment, ChromaLocation, ColorRange, VideoColor};

use super::{Obu, SEQUENCE_HEADER, obus};
use crate::StreamFacts;

/// H.273's code for a value the stream leaves unsaid.
const UNSPECIFIED: u8 = 2;

/// AV1's codes for where the colour samples of a 4:2:0 stream sit.
const VERTICAL: u8 = 1;
const COLOCATED: u8 = 2;

/// A stream's sequence header: its unit, and what it says.
#[derive(Clone, Debug)]
pub struct SequenceHeader {
    unit: Vec<u8>,
    header: SequenceHeaderObu,
}

impl PartialEq for SequenceHeader {
    fn eq(&self, other: &SequenceHeader) -> bool {
        self.unit == other.unit
    }
}

impl Eq for SequenceHeader {}

impl SequenceHeader {
    /// The header a configuration record (`av1C`) holds among its OBUs, where it holds one; why
    /// not, for bytes that are no such record, or a header that cannot be read.
    pub fn of_record(record: &[u8]) -> Result<Option<SequenceHeader>, String> {
        let parsed = AV1CodecConfigurationRecord::demux(&mut Cursor::new(record.to_vec().into()))
            .map_err(|error| format!("the configuration record: {error}"))?;
        let config_obus = obus(&parsed.config_obu)?;
        SequenceHeader::among(&config_obus)
    }

    /// The header among `obus`, where one is; why not, for one that cannot be read.
    pub fn among(obus: &[Obu<'_>]) -> Result<Option<SequenceHeader>, String> {
        let Some(obu) = obus.iter().find(|obu| obu.kind == SEQUENCE_HEADER) else {
            return Ok(None);
        };
        let header = ObuHeader::parse(&mut Cursor::new(obu.unit))
            .and_then(|header| SequenceHeaderObu::parse(header, &mut Cursor::new(obu.payload)))
            .map_err(|error| format!("a sequence header: {error}"))?;
        Ok(Some(SequenceHeader {
            unit: obu.unit.to_vec(),
            header,
        }))
    }

    /// The header's whole unit, as the stream sent it.
    pub fn unit(&self) -> &[u8] {
        &self.unit
    }

    /// Whether each frame of the stream is a still picture with a header of one bit less.
    pub(super) fn reduced_still_picture_header(&self) -> bool {
        self.header.reduced_still_picture_header
    }

    /// The largest size the stream's frames are decoded at, which is theirs where they do not
    /// say another.
    pub fn size(&self) -> PictureSize {
        PictureSize {
            width: self.header.max_frame_width as u32,
            height: self.header.max_frame_height as u32,
        }
    }

    /// Bits in each sample: 8, 10 or 12.
    pub fn bit_depth(&self) -> u8 {
        self.header.color_config.bit_depth as u8
    }

    /// What the header says of the stream's pictures; its level is its first operating point's.
    pub fn facts(&self) -> StreamFacts {
        let color = &self.header.color_config;
        let said = |code: u8| (code != UNSPECIFIED).then_some(code);
        StreamFacts {
            reorder_depth: 0,
            bit_depth: self.bit_depth(),
            color: VideoColor {
                primaries: said(color.color_primaries),
                transfer: said(color.transfer_characteristics),
                matrix: said(color.matrix_coefficients),
                range: Some(if color.full_color_range {
                    ColorRange::Full
                } else {
                    ColorRange::Limited
                }),
                chroma_location: chroma_location(color.chroma_sample_position),
            },
            pixel_aspect: None,
            size: Some(self.size()),
            profile: u32::from(self.header.seq_profile),
            level: self
                .header
                .operating_points
                .first()
                .map(|point| u32::from(point.seq_level_idx)),
        }
    }

    /// The configuration record (`av1C`) a file holds for a stream of this header, the header its
    /// one OBU.
    pub fn record(&self) -> Vec<u8> {
        let color = &self.header.color_config;
        let first_point = self.header.operating_points.first();
        let record = AV1CodecConfigurationRecord {
            seq_profile: self.header.seq_profile,
            seq_level_idx_0: first_point.map_or(0, |point| point.seq_level_idx),
            seq_tier_0: first_point.is_some_and(|point| point.seq_tier),
            high_bitdepth: color.bit_depth > 8,
            twelve_bit: color.bit_depth == 12,
            monochrome: color.mono_chrome,
            chroma_subsampling_x: color.subsampling_x,
            chroma_subsampling_y: color.subsampling_y,
            chroma_sample_position: color.chroma_sample_position,
            hdr_wcg_idc: 0,
            initial_presentation_delay_minus_one: None,
            config_obu: self.unit.clone().into(),
        };
        let mut bytes = Vec::new();
        record
            .mux(&mut bytes)
            .expect("a record is written to memory");
        bytes
    }
}

/// Where the colour samples of a 4:2:0 stream sit, as AV1's chroma sample position says; none
/// where it is unsaid.
fn chroma_location(position: u8) -> Option<ChromaLocation> {
    let vertical = match position {
        VERTICAL => ChromaAlignment::Centered,
        COLOCATED => ChromaAlignment::Cosited,
        _ => return None,
    };
    Some(ChromaLocation {
        horizontal: ChromaAlignment::Cosited,
        vertical,
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_clip::chunks_of_the_clip;
    use super::*;

    fn clips_header() -> SequenceHeader {
        let obus = obus(chunks_of_the_clip()[0]).unwrap();
        SequenceHeader::among(&obus)
            .unwrap()
            .expect("a sequence header")
    }

    #[test]
    fn a_sequence_header_says_its_streams_size_profile_level_and_colour() {
        let facts = clips_header().facts();
        assert_eq!(
            facts.size,
            Some(PictureSize {
                width: 320,
                height: 240
            })
        );
        assert_eq!(facts.profile, 0, "the Main profile");
        assert_eq!(facts.level, Some(0), "level 2.0");
        assert_eq!(facts.reorder_depth, 0);
        assert_eq!(facts.bit_depth, 8);
        assert_eq!(facts.color.range, Some(ColorRange::Limited));
        assert_eq!(facts.color.primaries, None);
        assert_eq!(clips_header().bit_depth(), 8);
    }

    #[test]
    fn a_record_holds_the_header_it_was_made_of() {
        let header = clips_header();
        let record = header.record();
        assert_eq!(record[0], 0x81, "the marker and version 1");
        assert_eq!(SequenceHeader::of_record(&record), Ok(Some(header)));
    }

    #[test]
    fn a_record_may_leave_the_header_out() {
        let record = clips_header().record();
        assert_eq!(SequenceHeader::of_record(&record[..4]), Ok(None));
        assert!(SequenceHeader::of_record(&[0x01, 0, 0, 0]).is_err());
    }

    #[test]
    fn chunks_with_no_header_say_none() {
        let obus = obus(chunks_of_the_clip()[1]).unwrap();
        assert_eq!(SequenceHeader::among(&obus), Ok(None));
    }
}
