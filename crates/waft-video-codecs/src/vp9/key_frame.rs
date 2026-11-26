//! What a VP9 key frame's header says of its stream (VP9's uncompressed header, 6.2): its
//! profile, the depth and colour of its samples, and its size. Only a key frame says them, so a
//! decoder that must know them first (VideoToolbox's format) is made at a stream's first key
//! frame, and made again where a key frame says otherwise.
//!
//! The invariant: what is read here is only what the header says, in its order; a field the
//! header does not send for a profile takes the value the standard gives it there.

use std::io;

use scuffle_bytes_util::BitReader;
use waft_embedder::video_decoders::PictureSize;
use waft_embedder::{ColorRange, VideoColor};

use super::Record;
use crate::StreamFacts;

/// The two bits each frame's header begins with.
const FRAME_MARKER: u64 = 2;

/// The three bytes a key frame's header holds after its first fields.
const SYNC_CODE: u64 = 0x49_83_42;

/// The colour space of samples that are red, green and blue already.
const RGB_COLOR_SPACE: u8 = 7;

/// H.273's code for a value the stream leaves unsaid.
const UNSPECIFIED: u8 = 2;

/// What a key frame's header says of its stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyFrame {
    /// The profile: 0 and 2 for 4:2:0, 1 and 3 for the others; 2 and 3 for more than eight bits.
    pub profile: u8,
    /// Bits in each sample: 8, 10 or 12.
    pub bit_depth: u8,
    /// VP9's own code for the matrix its samples are mixed with (`color_space`): 0 unsaid,
    /// 1 BT.601, 2 BT.709, 3 SMPTE 170, 4 SMPTE 240, 5 BT.2020, 7 none, its samples red, green
    /// and blue already.
    pub color_space: u8,
    pub full_range: bool,
    /// Whether its colour planes are half as wide, and half as high, as its brightness.
    pub subsampling_x: bool,
    pub subsampling_y: bool,
    /// The size its frames are decoded at.
    pub size: PictureSize,
}

impl KeyFrame {
    /// What the first frame of `chunk` says, where it is a key frame; none where it is another
    /// frame, and why not, for a header that is no VP9 frame's.
    pub fn of_chunk(chunk: &[u8]) -> Result<Option<KeyFrame>, String> {
        let frames = super::frames(chunk)?;
        let first = frames.first().ok_or("an empty chunk")?;
        read(first).map_err(|error| format!("a VP9 frame's header: {error}"))
    }

    /// What the stream says of its pictures, with what `record`, its file's configuration
    /// record where it has one, says beyond: its level, primaries and transfer, which a VP9
    /// header never sends, and its matrix where the header leaves it unsaid.
    pub fn facts(&self, record: Option<&Record>) -> StreamFacts {
        let said = |code: u8| (code != UNSPECIFIED).then_some(code);
        StreamFacts {
            reorder_depth: 0,
            color: VideoColor {
                primaries: record.and_then(|record| said(record.primaries)),
                transfer: record.and_then(|record| said(record.transfer)),
                matrix: self
                    .matrix()
                    .or_else(|| record.and_then(|record| said(record.matrix))),
                range: Some(if self.full_range {
                    ColorRange::Full
                } else {
                    ColorRange::Limited
                }),
                chroma_location: record.and_then(Record::chroma_location),
            },
            pixel_aspect: None,
            size: Some(self.size),
            profile: u32::from(self.profile),
            level: record
                .map(|record| u32::from(record.level))
                .filter(|level| *level != 0),
        }
    }

    /// The matrix the header's colour space names, as H.273 numbers it; none where it is unsaid
    /// or reserved.
    pub fn matrix(&self) -> Option<u8> {
        match self.color_space {
            1 | 3 => Some(6),
            2 => Some(1),
            4 => Some(7),
            5 => Some(9),
            RGB_COLOR_SPACE => Some(0),
            _ => None,
        }
    }
}

/// Whether `frame` is a key frame, as its header's first bits say.
pub(super) fn begins_key_frame(frame: &[u8]) -> bool {
    matches!(read(frame), Ok(Some(_)))
}

/// What the header of `frame` says, where it is a key frame.
fn read(frame: &[u8]) -> io::Result<Option<KeyFrame>> {
    let mut bits = BitReader::new(frame);
    if bits.read_bits(2)? != FRAME_MARKER {
        return Err(invalid("no frame marker"));
    }
    let profile_low = bits.read_bit()? as u8;
    let profile = ((bits.read_bit()? as u8) << 1) | profile_low;
    if profile == 3 {
        bits.read_bit()?; // reserved_zero
    }
    let show_existing_frame = bits.read_bit()?;
    if show_existing_frame {
        return Ok(None);
    }
    let non_key = bits.read_bit()?; // frame_type
    if non_key {
        return Ok(None);
    }
    bits.read_bit()?; // show_frame
    bits.read_bit()?; // error_resilient_mode
    if bits.read_bits(24)? != SYNC_CODE {
        return Err(invalid("a key frame with no sync code"));
    }
    let bit_depth = if profile >= 2 {
        if bits.read_bit()? { 12 } else { 10 }
    } else {
        8
    };
    let color_space = bits.read_bits(3)? as u8;
    let has_full_subsampling = profile == 1 || profile == 3;
    let (full_range, subsampling_x, subsampling_y) = if color_space != RGB_COLOR_SPACE {
        let full_range = bits.read_bit()?;
        if has_full_subsampling {
            let subsampling = (bits.read_bit()?, bits.read_bit()?);
            bits.read_bit()?; // reserved_zero
            (full_range, subsampling.0, subsampling.1)
        } else {
            (full_range, true, true)
        }
    } else {
        if has_full_subsampling {
            bits.read_bit()?; // reserved_zero
        }
        (true, false, false)
    };
    let width = bits.read_bits(16)? as u32 + 1;
    let height = bits.read_bits(16)? as u32 + 1;
    Ok(Some(KeyFrame {
        profile,
        bit_depth,
        color_space,
        full_range,
        subsampling_x,
        subsampling_y,
        size: PictureSize { width, height },
    }))
}

fn invalid(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}

#[cfg(test)]
mod tests {
    use super::super::test_clip::chunks_of_the_clip;
    use super::*;

    #[test]
    fn a_key_frame_says_its_streams_profile_depth_colour_and_size() {
        let key = KeyFrame::of_chunk(chunks_of_the_clip()[0])
            .unwrap()
            .expect("a key frame");
        assert_eq!(
            key,
            KeyFrame {
                profile: 0,
                bit_depth: 8,
                color_space: 0,
                full_range: false,
                subsampling_x: true,
                subsampling_y: true,
                size: PictureSize {
                    width: 320,
                    height: 240
                },
            }
        );
        let facts = key.facts(None);
        assert_eq!(facts.size, Some(key.size));
        assert_eq!(facts.reorder_depth, 0);
        assert_eq!(facts.color.range, Some(ColorRange::Limited));
        assert_eq!(
            facts.color.matrix, None,
            "libvpx leaves its colour space unsaid"
        );
        assert_eq!(facts.level, None);
    }

    #[test]
    fn a_frame_that_is_no_key_frame_says_nothing_of_its_stream() {
        assert_eq!(KeyFrame::of_chunk(chunks_of_the_clip()[2]), Ok(None));
        assert!(KeyFrame::of_chunk(&[0x00, 0x01]).is_err());
    }

    #[test]
    fn a_record_says_what_a_header_does_not() {
        let key = KeyFrame::of_chunk(chunks_of_the_clip()[0])
            .unwrap()
            .unwrap();
        let record = Record {
            level: 21,
            primaries: 1,
            transfer: 1,
            matrix: 1,
            ..Record::of_key_frame(&key).unwrap()
        };
        let facts = key.facts(Some(&record));
        assert_eq!(facts.level, Some(21));
        assert_eq!(facts.color.primaries, Some(1));
        assert_eq!(facts.color.transfer, Some(1));
        assert_eq!(facts.color.matrix, Some(1), "the header leaves it unsaid");
    }
}
