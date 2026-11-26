//! AV1: a chunk's units (OBUs, open bitstream units), which chunk decodes with nothing before it,
//! and what a sequence header says of its stream (`sequence_header`), from a chunk or from the
//! configuration record (`av1C`) a file gives.
//!
//! A chunk is one temporal unit in the low-overhead form MP4 and WebM hold: OBUs each with its
//! size, but for the last, which may fill the rest of the chunk, and no temporal delimiter, though
//! one is passed over where a stream has it. A system's decoder takes the chunk whole. AV1 never
//! reorders: each chunk's picture is shown after the one before it, a frame decoded earlier and
//! shown now coming in a chunk of its own.
//!
//! The invariant: a stream's size, depth and colour are what its last sequence header said.

mod sequence_header;
#[cfg(any(test, feature = "test-support"))]
pub mod test_clip;

use scuffle_bytes_util::BitReader;

pub use sequence_header::SequenceHeader;

/// The OBU types this reads, as AV1 numbers them (6.2.2).
pub const SEQUENCE_HEADER: u8 = 1;
pub const TEMPORAL_DELIMITER: u8 = 2;
pub const FRAME_HEADER: u8 = 3;
pub const FRAME: u8 = 6;

/// The frame type of a frame that decodes with nothing before it.
const KEY_FRAME: u64 = 0;

/// One OBU of a chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obu<'a> {
    /// Its type, as AV1 numbers it.
    pub kind: u8,
    /// The whole unit: its header, its size where it has one, and its payload.
    pub unit: &'a [u8],
    pub payload: &'a [u8],
}

/// The OBUs of `chunk`, in order; why not, where one's size runs past the chunk or its header is
/// no OBU's.
pub fn obus(chunk: &[u8]) -> Result<Vec<Obu<'_>>, String> {
    let mut obus = Vec::new();
    let mut rest = chunk;
    while !rest.is_empty() {
        let (obu, after) = first_obu(rest)?;
        obus.push(obu);
        rest = after;
    }
    Ok(obus)
}

/// Whether `chunk` decodes with nothing before it: whether it holds a sequence header and its
/// first frame is a key frame shown at once.
pub fn is_key_frame(chunk: &[u8]) -> bool {
    let Ok(obus) = obus(chunk) else {
        return false;
    };
    let Ok(Some(sequence)) = SequenceHeader::among(&obus) else {
        return false;
    };
    obus.iter()
        .find(|obu| matches!(obu.kind, FRAME_HEADER | FRAME))
        .is_some_and(|frame| is_shown_key_frame(frame.payload, &sequence))
}

/// Whether a frame header's payload, in a stream of `sequence`, is a key frame's that shows it
/// at once (5.9.2): any frame of a stream of still pictures with reduced headers is.
fn is_shown_key_frame(payload: &[u8], sequence: &SequenceHeader) -> bool {
    sequence.reduced_still_picture_header() || shows_key_frame(payload).unwrap_or(false)
}

/// Whether a frame header's payload, in a stream whose headers are not reduced, says it is a key
/// frame shown at once, and not one decoded earlier and shown now.
fn shows_key_frame(payload: &[u8]) -> std::io::Result<bool> {
    let mut bits = BitReader::new(payload);
    let show_existing_frame = bits.read_bit()?;
    if show_existing_frame {
        return Ok(false);
    }
    let frame_type = bits.read_bits(2)?;
    let show_frame = bits.read_bit()?;
    Ok(frame_type == KEY_FRAME && show_frame)
}

/// The first OBU of `data`, and what follows it.
fn first_obu(data: &[u8]) -> Result<(Obu<'_>, &[u8]), String> {
    let header = data[0];
    if header & 0x80 != 0 {
        return Err("an OBU whose forbidden bit is set".to_owned());
    }
    let kind = (header >> 3) & 0x0F;
    let has_extension = header & 0x04 != 0;
    let has_size = header & 0x02 != 0;
    let mut at = 1 + usize::from(has_extension);
    let size = if has_size {
        let (size, size_bytes) =
            leb128(data.get(at..).unwrap_or_default()).ok_or("an OBU's size cut short")?;
        at += size_bytes;
        size
    } else {
        data.len().saturating_sub(at)
    };
    let end = at
        .checked_add(size)
        .filter(|end| *end <= data.len())
        .ok_or("an OBU's size runs past its chunk")?;
    let obu = Obu {
        kind,
        unit: &data[..end],
        payload: &data[at..end],
    };
    Ok((obu, &data[end..]))
}

/// The number `data` begins with in AV1's little-endian base-128 form, and the bytes it takes;
/// none where it runs past eight bytes or the data.
fn leb128(data: &[u8]) -> Option<(usize, usize)> {
    let mut value = 0usize;
    for (index, byte) in data.iter().take(8).enumerate() {
        value |= usize::from(byte & 0x7F) << (index * 7);
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::test_clip::chunks_of_the_clip;
    use super::*;

    #[test]
    fn only_the_clips_first_chunk_is_a_key_frame() {
        let keys: Vec<bool> = chunks_of_the_clip()
            .iter()
            .map(|chunk| is_key_frame(chunk))
            .collect();
        assert_eq!(keys.len(), 12);
        assert!(keys[0]);
        assert!(keys[1..].iter().all(|key| !key));
    }

    /// SVT-AV1 decodes frames ahead in the second chunk and shows them later, each in a chunk of
    /// a frame header alone.
    #[test]
    fn a_chunks_obus_are_its_sequence_header_and_frames() {
        let chunks = chunks_of_the_clip();
        let kinds =
            |chunk| -> Vec<u8> { obus(chunk).unwrap().iter().map(|obu| obu.kind).collect() };
        assert_eq!(kinds(chunks[0]), [SEQUENCE_HEADER, FRAME]);
        assert_eq!(kinds(chunks[1]), [FRAME; 4]);
        assert_eq!(kinds(chunks[2]), [FRAME_HEADER]);
    }

    #[test]
    fn a_temporal_delimiter_is_passed_over() {
        let first = chunks_of_the_clip()[0];
        let delimited = [&[TEMPORAL_DELIMITER << 3 | 0x02, 0][..], first].concat();
        assert!(is_key_frame(&delimited));
        assert_eq!(obus(&delimited).unwrap()[0].kind, TEMPORAL_DELIMITER);
    }

    #[test]
    fn the_last_obu_may_fill_the_rest_of_its_chunk() {
        let unsized_frame = [FRAME << 3, 0x10, 0x20];
        let units = obus(&unsized_frame).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].payload, [0x10, 0x20]);
    }

    #[test]
    fn an_obu_whose_size_runs_past_its_chunk_is_no_chunk() {
        assert!(obus(&[FRAME << 3 | 0x02, 9, 0]).is_err());
        assert!(obus(&[0x80]).is_err());
        assert!(!is_key_frame(&[FRAME << 3 | 0x02, 9, 0]));
    }

    #[test]
    fn a_size_takes_seven_bits_a_byte() {
        assert_eq!(leb128(&[0x05]), Some((5, 1)));
        assert_eq!(leb128(&[0x80, 0x01]), Some((128, 2)));
        assert_eq!(leb128(&[0x80]), None);
    }
}
