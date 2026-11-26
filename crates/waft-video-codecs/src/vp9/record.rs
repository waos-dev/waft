//! VP9's configuration record (`vpcC`, the VP codec ISO media file format binding, 2.2): what a
//! file says of a VP9 stream beside its frames, the contents of its `vpcC` box, version and
//! flags first. A system's decoder that is configured with one (VideoToolbox) takes the file's,
//! or one made of what the stream's key frame says where the file gives none.
//!
//! The invariant: a record read and written again is the same bytes, but for codec
//! initialization data, which VP9 never has.

use waft_embedder::{ChromaAlignment, ChromaLocation};

use super::KeyFrame;

/// The record's version this reads and writes: the one the binding defines its fields for.
const VERSION: u8 = 1;

/// The bytes of a record of version 1 with no codec initialization data.
const SIZE: usize = 12;

/// H.273's code for a value the stream leaves unsaid.
const UNSPECIFIED: u8 = 2;

/// The record's code for 4:2:0 whose colour samples sit left of their brightness samples, between
/// two rows, as VP9 places them.
const VERTICAL_420: u8 = 0;
/// The record's code for 4:2:0 whose colour samples sit on their first brightness sample.
const COLOCATED_420: u8 = 1;
const SUBSAMPLED_422: u8 = 2;
const FULL_444: u8 = 3;

/// What a `vpcC` box says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub profile: u8,
    /// The level, ten times its number (`21` for 2.1); 0 where the file leaves it unsaid.
    pub level: u8,
    /// Bits in each sample: 8, 10 or 12.
    pub bit_depth: u8,
    /// The record's code for how much colour each picture keeps and where it sits: 0 and 1 for
    /// 4:2:0 (see [`chroma_location`](Self::chroma_location)), 2 for 4:2:2, 3 for 4:4:4.
    pub chroma_subsampling: u8,
    pub full_range: bool,
    /// H.273's codes, 2 where unsaid.
    pub primaries: u8,
    pub transfer: u8,
    pub matrix: u8,
}

impl Record {
    /// The record `bytes` hold, the contents of a `vpcC` box; why not, for bytes that are no
    /// record of version 1.
    pub fn parse(bytes: &[u8]) -> Result<Record, String> {
        let fields: &[u8; SIZE] = bytes
            .get(..SIZE)
            .and_then(|fields| fields.try_into().ok())
            .ok_or("a VP9 configuration record shorter than its fields")?;
        if fields[0] != VERSION {
            return Err(format!(
                "a VP9 configuration record of version {}, not {VERSION}",
                fields[0]
            ));
        }
        Ok(Record {
            profile: fields[4],
            level: fields[5],
            bit_depth: fields[6] >> 4,
            chroma_subsampling: (fields[6] >> 1) & 0x07,
            full_range: fields[6] & 0x01 != 0,
            primaries: fields[7],
            transfer: fields[8],
            matrix: fields[9],
        })
    }

    /// The record a file would give for a stream whose key frame says `key`: its level and its
    /// primaries and transfer unsaid, which no header sends; why not, for a stream whose colour
    /// is kept in a shape no record names (4:4:0).
    pub fn of_key_frame(key: &KeyFrame) -> Result<Record, String> {
        let chroma_subsampling = match (key.subsampling_x, key.subsampling_y) {
            (true, true) => VERTICAL_420,
            (true, false) => SUBSAMPLED_422,
            (false, false) => FULL_444,
            (false, true) => {
                return Err("a VP9 stream in 4:4:0, which no record names".to_owned());
            }
        };
        Ok(Record {
            profile: key.profile,
            level: 0,
            bit_depth: key.bit_depth,
            chroma_subsampling,
            full_range: key.full_range,
            primaries: UNSPECIFIED,
            transfer: UNSPECIFIED,
            matrix: key.matrix().unwrap_or(UNSPECIFIED),
        })
    }

    /// The record as a `vpcC` box holds it.
    pub fn to_bytes(&self) -> Vec<u8> {
        vec![
            VERSION,
            0,
            0,
            0,
            self.profile,
            self.level,
            (self.bit_depth << 4) | (self.chroma_subsampling << 1) | u8::from(self.full_range),
            self.primaries,
            self.transfer,
            self.matrix,
            0,
            0,
        ]
    }

    /// Where the colour samples of a 4:2:0 stream sit; none for the others, whose samples sit
    /// where their brightness samples do.
    pub fn chroma_location(&self) -> Option<ChromaLocation> {
        let vertical = match self.chroma_subsampling {
            VERTICAL_420 => ChromaAlignment::Centered,
            COLOCATED_420 => ChromaAlignment::Cosited,
            _ => return None,
        };
        Some(ChromaLocation {
            horizontal: ChromaAlignment::Cosited,
            vertical,
        })
    }
}

#[cfg(test)]
mod tests {
    use waft_embedder::video_decoders::PictureSize;

    use super::*;

    fn key_frame() -> KeyFrame {
        KeyFrame {
            profile: 0,
            bit_depth: 8,
            color_space: 2,
            full_range: false,
            subsampling_x: true,
            subsampling_y: true,
            size: PictureSize {
                width: 320,
                height: 240,
            },
        }
    }

    #[test]
    fn a_record_read_and_written_again_is_the_same_bytes() {
        // As ffmpeg writes it for a 4:2:0 stream of level 2.1 in BT.709.
        let bytes = [1, 0, 0, 0, 0, 21, 0x80, 1, 1, 1, 0, 0];
        let record = Record::parse(&bytes).unwrap();
        assert_eq!(
            record,
            Record {
                profile: 0,
                level: 21,
                bit_depth: 8,
                chroma_subsampling: VERTICAL_420,
                full_range: false,
                primaries: 1,
                transfer: 1,
                matrix: 1,
            }
        );
        assert_eq!(record.to_bytes(), bytes);
    }

    #[test]
    fn bytes_of_another_version_or_too_few_are_no_record() {
        assert!(Record::parse(&[0, 0, 0, 0, 0, 21, 0x80, 1, 1, 1, 0, 0]).is_err());
        assert!(Record::parse(&[1, 0, 0]).is_err());
    }

    #[test]
    fn a_record_made_of_a_key_frame_says_what_its_header_says() {
        let record = Record::of_key_frame(&key_frame()).unwrap();
        assert_eq!(record.bit_depth, 8);
        assert_eq!(record.chroma_subsampling, VERTICAL_420);
        assert_eq!(record.matrix, 1, "BT.709");
        assert_eq!((record.level, record.primaries), (0, UNSPECIFIED));
        let four_four_zero = KeyFrame {
            subsampling_x: false,
            ..key_frame()
        };
        assert!(Record::of_key_frame(&four_four_zero).is_err());
    }
}
