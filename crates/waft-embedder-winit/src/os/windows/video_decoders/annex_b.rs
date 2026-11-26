//! An H.264 stream as Media Foundation's decoder takes it: Annex B, each unit after a start code,
//! the parameter sets in the stream. A stream after lengths, as MP4 holds it, is rewritten so, its
//! configuration record's parameter sets put into the first picture the decoder is handed.
//!
//! The invariant: parameter sets given ahead go where H.264 puts them in an access unit, after its
//! delimiter where it has one (7.4.1.2.3). Put before the delimiter, they are an access unit of
//! their own to Media Foundation's decoder, which then gives the pictures after it the times of
//! the samples before.

use waft_embedder::video_decoders::{DecoderError, NalFormat, VideoStreamSetup};

const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// The type of a unit that begins an access unit: its delimiter.
const DELIMITER: u8 = 9;

/// How a stream frames its units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Framing {
    /// Each after a length of this many bytes.
    Lengths(usize),
    StartCodes,
}

/// A stream's setup as the decoder takes it: its framing, and its parameter sets given ahead, in
/// Annex B; `Unsupported` for a codec other than H.264.
pub(super) fn framing_and_sets(
    setup: VideoStreamSetup,
) -> Result<(Framing, Vec<u8>), DecoderError> {
    match setup {
        VideoStreamSetup::H264(NalFormat::LengthPrefixed(record)) => {
            let (sets, length_size) = sets_of_record(&record).map_err(DecoderError::BadData)?;
            Ok((Framing::Lengths(length_size), with_start_codes(sets)))
        }
        VideoStreamSetup::H264(NalFormat::StartCodes(ahead)) => {
            Ok((Framing::StartCodes, ahead.unwrap_or_default()))
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
        Framing::Lengths(size) => units_after_lengths(data, size)
            .map(with_start_codes)
            .ok_or_else(|| DecoderError::BadData("a unit's length runs past its chunk".to_owned())),
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
    if picture.get(first)? & 0x1F != DELIMITER {
        return None;
    }
    let next = first + start_code(&picture[first..])?;
    Some(if picture[next - 1] == 0 {
        next - 1
    } else {
        next
    })
}

/// The units of `data`, each after a big-endian length of `size` bytes; `None` where a length
/// runs past the data.
fn units_after_lengths(data: &[u8], size: usize) -> Option<Vec<&[u8]>> {
    let mut units = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let (length, after) = rest.split_at_checked(size)?;
        let length = length
            .iter()
            .fold(0usize, |length, byte| (length << 8) | usize::from(*byte));
        let (unit, after) = after.split_at_checked(length)?;
        units.push(unit);
        rest = after;
    }
    Some(units)
}

/// `units`, each after a start code.
fn with_start_codes<'a>(units: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
    units
        .into_iter()
        .flat_map(|unit| START_CODE.iter().chain(unit))
        .copied()
        .collect()
}

/// The parameter sets of an `avcC` record (ISO/IEC 14496-15, 5.3.3.1), and the size of the lengths
/// its stream's units come after.
fn sets_of_record(record: &[u8]) -> Result<(Vec<&[u8]>, usize), String> {
    let short = || "the stream's configuration record is cut short".to_owned();
    let length_size = usize::from(record.get(4).ok_or_else(short)? & 0b11) + 1;
    let mut rest = record.get(5..).ok_or_else(short)?;
    let mut sets = Vec::new();
    // The sequence parameter sets, then the picture parameter sets, each counted ahead.
    for count_mask in [0b1_1111, 0xFF] {
        let (count, after) = rest.split_first().ok_or_else(short)?;
        rest = after;
        for _ in 0..(count & count_mask) {
            let (length, after) = rest.split_at_checked(2).ok_or_else(short)?;
            let length = usize::from(u16::from_be_bytes([length[0], length[1]]));
            let (set, after) = after.split_at_checked(length).ok_or_else(short)?;
            sets.push(set);
            rest = after;
        }
    }
    Ok((sets, length_size))
}

#[cfg(test)]
pub(super) mod tests {
    pub(in super::super) use waft_video_codecs::h264::test_clip::CLIP;

    use super::*;

    /// The units of `data`, each after a start code of three or four bytes.
    pub(in super::super) fn units_after_start_codes(data: &[u8]) -> Vec<&[u8]> {
        let mut starts = Vec::new();
        let mut at = 0;
        while at + 3 <= data.len() {
            if data[at..at + 3] == [0, 0, 1] {
                starts.push(at + 3);
                at += 3;
            } else {
                at += 1;
            }
        }
        starts
            .iter()
            .enumerate()
            .map(|(index, &start)| {
                let end = starts.get(index + 1).map_or(data.len(), |&next| {
                    let code = next - 3;
                    if code > start && data[code - 1] == 0 {
                        code - 1
                    } else {
                        code
                    }
                });
                &data[start..end]
            })
            .collect()
    }

    /// `units`, each after a length of four bytes.
    pub(in super::super) fn with_lengths<'a>(units: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
        units
            .into_iter()
            .flat_map(|unit| {
                let length = u32::try_from(unit.len())
                    .expect("a unit under 4 GiB")
                    .to_be_bytes();
                length.into_iter().chain(unit.iter().copied())
            })
            .collect()
    }

    /// An `avcC` record of `sets`, sequence parameter sets first, for units after four-byte
    /// lengths.
    pub(in super::super) fn record_of(sets: &[&[u8]]) -> Vec<u8> {
        let is_sps = |set: &&&[u8]| set[0] & 0x1F == 7;
        let (sps, pps): (Vec<&[u8]>, Vec<&[u8]>) = sets.iter().partition(is_sps);
        let first = sps.first().expect("a sequence parameter set");
        let mut record = vec![
            1,
            first[1],
            first[2],
            first[3],
            0xFF,
            0xE0 | sps.len() as u8,
        ];
        for set in &sps {
            record.extend((set.len() as u16).to_be_bytes());
            record.extend(*set);
        }
        record.push(pps.len() as u8);
        for set in &pps {
            record.extend((set.len() as u16).to_be_bytes());
            record.extend(*set);
        }
        record
    }

    /// Whether `unit` is a sequence or picture parameter set.
    pub(in super::super) fn is_parameter_set(unit: &[u8]) -> bool {
        matches!(unit[0] & 0x1F, 7 | 8)
    }

    #[test]
    fn a_record_gives_its_sets_and_its_length_size() {
        let units = units_after_start_codes(CLIP);
        let sets: Vec<&[u8]> = units
            .into_iter()
            .filter(|unit| is_parameter_set(unit))
            .collect();
        let record = record_of(&sets);
        let (read, length_size) = sets_of_record(&record).expect("a record");
        assert_eq!(read, sets);
        assert_eq!(length_size, 4);
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
