//! What an H.264 stream's parameter sets say: the sets themselves, which MediaCodec is configured
//! with (its `csd-0` and `csd-1`), and what the first of them says of the stream before a picture
//! is decoded: its pictures' size, which the decoder and the image reader it draws into are made
//! for, and what their samples mean as colour.
//!
//! The invariant: a decoder is made only of a whole set, a sequence set and a picture set, since
//! MediaCodec takes no picture before both.

use h264_reader::avcc::AvcDecoderConfigurationRecord;
use h264_reader::nal::sps::SeqParameterSet;
use h264_reader::rbsp::{BitReader, decode_nal};
use waft_embedder::video_decoders::PictureSize;
use waft_embedder::{ColorRange, PixelAspect, VideoColor};

use super::nal::with_start_codes;

const SEQUENCE_PARAMETER_SET: u8 = 7;
const PICTURE_PARAMETER_SET: u8 = 8;
const KEY_PICTURE_SLICE: u8 = 5;

/// H.273's code for a value the stream leaves unsaid.
const UNSPECIFIED: u8 = 2;

/// What kind of unit `unit` is, as its first byte says; none for an empty one.
fn unit_type(unit: &[u8]) -> Option<u8> {
    unit.first().map(|header| header & 0x1F)
}

/// Whether `units` hold a slice of a key picture, which decodes with nothing before it.
pub(super) fn holds_key_picture(units: &[&[u8]]) -> bool {
    units
        .iter()
        .any(|unit| unit_type(unit) == Some(KEY_PICTURE_SLICE))
}

/// A stream's parameter sets, each a unit without framing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ParameterSets {
    sequence: Vec<Vec<u8>>,
    picture: Vec<Vec<u8>>,
}

impl ParameterSets {
    /// The sets a configuration record (`avcC`) holds, and the size of the length each unit of
    /// its stream follows; why not, for bytes that are no such record.
    pub(super) fn of_record(record: &[u8]) -> Result<(ParameterSets, usize), String> {
        let parsed = AvcDecoderConfigurationRecord::try_from(record)
            .map_err(|error| format!("the configuration record: {error:?}"))?;
        let sequence = owned_sets(parsed.sequence_parameter_sets(), "sequence")?;
        let picture = owned_sets(parsed.picture_parameter_sets(), "picture")?;
        let length_size = usize::from(parsed.length_size_minus_one()) + 1;
        Ok((ParameterSets { sequence, picture }, length_size))
    }

    /// The sets among `units`.
    pub(super) fn among(units: &[&[u8]]) -> ParameterSets {
        let of_type = |wanted: u8| {
            units
                .iter()
                .filter(|unit| unit_type(unit) == Some(wanted))
                .map(|unit| unit.to_vec())
                .collect()
        };
        ParameterSets {
            sequence: of_type(SEQUENCE_PARAMETER_SET),
            picture: of_type(PICTURE_PARAMETER_SET),
        }
    }

    /// These sets with each kind `newer` holds in place of this one's: a stream sends a kind
    /// again whole when it changes.
    pub(super) fn replaced_by(&self, newer: ParameterSets) -> ParameterSets {
        ParameterSets {
            sequence: if newer.sequence.is_empty() {
                self.sequence.clone()
            } else {
                newer.sequence
            },
            picture: if newer.picture.is_empty() {
                self.picture.clone()
            } else {
                newer.picture
            },
        }
    }

    /// Whether a decoder can be made of them: at least one of each kind.
    pub(super) fn are_complete(&self) -> bool {
        !self.sequence.is_empty() && !self.picture.is_empty()
    }

    /// The sequence sets after start codes, MediaCodec's `csd-0`.
    pub(super) fn sequence_data(&self) -> Vec<u8> {
        with_start_codes(self.sequence.iter().map(Vec::as_slice))
    }

    /// The picture sets after start codes, MediaCodec's `csd-1`.
    pub(super) fn picture_data(&self) -> Vec<u8> {
        with_start_codes(self.picture.iter().map(Vec::as_slice))
    }

    /// What the first sequence set says of the stream; why not, for one that cannot be read.
    pub(super) fn facts(&self) -> Result<StreamFacts, String> {
        let first = self
            .sequence
            .first()
            .ok_or("the stream has no sequence parameter set")?;
        StreamFacts::of_sequence_set(first)
    }
}

/// The record's sets of one `kind`, each copied out; why not, for one the record cuts short.
fn owned_sets<'a, Refusal: std::fmt::Debug>(
    sets: impl Iterator<Item = Result<&'a [u8], Refusal>>,
    kind: &str,
) -> Result<Vec<Vec<u8>>, String> {
    sets.map(|set| match set {
        Ok(set) => Ok(set.to_vec()),
        Err(refusal) => Err(format!("the record's {kind} parameter sets: {refusal:?}")),
    })
    .collect()
}

/// What a stream says of its pictures before any is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StreamFacts {
    /// The pictures' size, cropped as the stream crops them.
    pub(super) size: PictureSize,
    /// What its samples mean as colour, as far as the stream says.
    pub(super) color: VideoColor,
    /// The shape of one pixel, where the stream says.
    pub(super) pixel_aspect: Option<PixelAspect>,
}

impl StreamFacts {
    fn of_sequence_set(unit: &[u8]) -> Result<StreamFacts, String> {
        let payload = decode_nal(unit).map_err(|error| format!("a parameter set: {error}"))?;
        let set = SeqParameterSet::from_bits(BitReader::new(&payload[..]))
            .map_err(|error| format!("the sequence parameter set: {error:?}"))?;
        let (width, height) = set
            .pixel_dimensions()
            .map_err(|error| format!("the sequence parameter set's size: {error:?}"))?;
        let usability = set.vui_parameters.as_ref();
        let signal = usability.and_then(|usability| usability.video_signal_type.as_ref());
        let described = signal.and_then(|signal| signal.colour_description.as_ref());
        let said = |code: u8| (code != UNSPECIFIED).then_some(code);
        Ok(StreamFacts {
            size: PictureSize { width, height },
            color: VideoColor {
                primaries: described.and_then(|color| said(color.colour_primaries)),
                transfer: described.and_then(|color| said(color.transfer_characteristics)),
                matrix: described.and_then(|color| said(color.matrix_coefficients)),
                range: signal.map(|signal| {
                    if signal.video_full_range_flag {
                        ColorRange::Full
                    } else {
                        ColorRange::Limited
                    }
                }),
                chroma_location: None,
            },
            pixel_aspect: usability
                .and_then(|usability| usability.aspect_ratio_info.as_ref())
                .and_then(|aspect| aspect.get())
                .map(|(width, height)| PixelAspect {
                    width: u32::from(width),
                    height: u32::from(height),
                }),
        })
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::nal::units_after_start_codes;
    use super::*;

    /// Twelve pictures of 320 by 240 in the High profile, each picture decoded up to two ahead
    /// of ones shown before it, as x264 made them (`ffmpeg -f lavfi -i
    /// testsrc=size=320x240:rate=30 -frames:v 12 -c:v libx264 -bf 2 -x264-params
    /// aud=1:keyint=12:bframes=2`, then `-bsf:v h264_mp4toannexb -f h264`): the clip Apple's
    /// decoder is checked with too.
    pub(in super::super) const CLIP: &[u8] = include_bytes!("test_clip.h264");

    #[test]
    fn a_streams_sets_are_found_among_its_units() {
        let units = units_after_start_codes(CLIP);
        let sets = ParameterSets::among(&units);
        assert!(sets.are_complete());
        assert!(
            units_after_start_codes(&sets.sequence_data())
                .iter()
                .all(|unit| unit_type(unit) == Some(SEQUENCE_PARAMETER_SET))
        );
        assert_eq!(units_after_start_codes(&sets.picture_data()).len(), 1);
    }

    #[test]
    fn a_stream_says_its_pictures_size() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let facts = sets.facts().expect("the clip's facts");
        assert_eq!(
            facts.size,
            PictureSize {
                width: 320,
                height: 240
            }
        );
        assert_eq!(facts.color.range, None, "the clip does not say its range");
    }

    #[test]
    fn the_clips_first_picture_is_a_key_picture_and_its_second_is_not() {
        let units = units_after_start_codes(CLIP);
        let delimiters: Vec<usize> = units
            .iter()
            .enumerate()
            .filter(|(_, unit)| unit_type(unit) == Some(9))
            .map(|(at, _)| at)
            .collect();
        let first = &units[delimiters[0]..delimiters[1]];
        let second = &units[delimiters[1]..delimiters[2]];
        assert!(holds_key_picture(first));
        assert!(!holds_key_picture(second));
    }

    #[test]
    fn a_kind_sent_again_takes_the_place_of_the_one_held() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let other_picture_set = ParameterSets {
            sequence: Vec::new(),
            picture: vec![vec![PICTURE_PARAMETER_SET, 1]],
        };
        let replaced = sets.replaced_by(other_picture_set);
        assert_eq!(replaced.sequence, sets.sequence);
        assert_eq!(replaced.picture, [vec![PICTURE_PARAMETER_SET, 1]]);
        assert_eq!(sets.replaced_by(ParameterSets::default()), sets);
    }

    #[test]
    fn a_record_holds_the_sets_and_the_size_of_a_length() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let record = record_of(&sets);
        assert_eq!(ParameterSets::of_record(&record), Ok((sets, 4)));
        assert!(ParameterSets::of_record(&[1, 2, 3]).is_err());
    }

    /// The configuration record (`avcC`) a file holds for a stream of `sets`, its units after
    /// lengths of four bytes.
    pub(in super::super) fn record_of(sets: &ParameterSets) -> Vec<u8> {
        let first = &sets.sequence[0];
        let mut record = vec![1, first[1], first[2], first[3], 0xFF];
        record.push(0xE0 | sets.sequence.len() as u8);
        for set in &sets.sequence {
            record.extend_from_slice(&(set.len() as u16).to_be_bytes());
            record.extend_from_slice(set);
        }
        record.push(sets.picture.len() as u8);
        for set in &sets.picture {
            record.extend_from_slice(&(set.len() as u16).to_be_bytes());
            record.extend_from_slice(set);
        }
        record
    }
}
