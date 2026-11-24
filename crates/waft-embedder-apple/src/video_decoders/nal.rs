//! How an H.264 or HEVC stream frames its units, and the one framing VideoToolbox takes: each
//! unit after its length, in four bytes. A stream arrives either so, with lengths of one, two or
//! four bytes as its configuration record says, or as Annex B, each unit after a start code.
//!
//! The invariant: what comes out holds exactly the units that went in, in order, each whole.

/// The bytes of the length VideoToolbox is told each unit follows.
pub(super) const LENGTH_SIZE: usize = 4;

/// The units of `data`, a stream in Annex B: each without its start code, of three or four
/// bytes, or the zeros that pad before one.
pub(super) fn units_after_start_codes(data: &[u8]) -> Vec<&[u8]> {
    let mut units = Vec::new();
    let mut unit_start = None;
    let mut at = 0;
    while at + 3 <= data.len() {
        if data[at..at + 3] == [0, 0, 1] {
            if let Some(start) = unit_start {
                units.push(without_trailing_zeros(&data[start..at]));
            }
            unit_start = Some(at + 3);
            at += 3;
        } else {
            at += 1;
        }
    }
    if let Some(start) = unit_start {
        units.push(without_trailing_zeros(&data[start..]));
    }
    units.retain(|unit| !unit.is_empty());
    units
}

/// The units of `data`, each after a length of `length_size` bytes; none where a length runs
/// past the data, or is of a size no stream has.
pub(super) fn units_after_lengths(data: &[u8], length_size: usize) -> Option<Vec<&[u8]>> {
    if !matches!(length_size, 1 | 2 | 4) {
        return None;
    }
    let mut units = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let (length, after) = rest.split_at_checked(length_size)?;
        let length = length
            .iter()
            .fold(0usize, |length, byte| (length << 8) | usize::from(*byte));
        let (unit, after) = after.split_at_checked(length)?;
        units.push(unit);
        rest = after;
    }
    Some(units)
}

/// `units`, each after its length in [`LENGTH_SIZE`] bytes: a sample as VideoToolbox takes it.
pub(super) fn with_lengths<'a>(units: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
    let mut sample = Vec::new();
    for unit in units {
        sample.extend_from_slice(&(unit.len() as u32).to_be_bytes());
        sample.extend_from_slice(unit);
    }
    sample
}

/// `unit` without the zeros that pad it before the next start code: a unit never ends in one.
fn without_trailing_zeros(unit: &[u8]) -> &[u8] {
    let end = unit
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |last| last + 1);
    &unit[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_follow_start_codes_of_three_or_four_bytes() {
        let stream = [
            0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 4, 5,
        ];
        assert_eq!(
            units_after_start_codes(&stream),
            [&[0x67, 1, 2][..], &[0x68, 3][..], &[0x65, 4, 5][..]]
        );
    }

    #[test]
    fn what_comes_before_the_first_start_code_is_no_unit() {
        assert!(units_after_start_codes(&[9, 9, 9]).is_empty());
        assert_eq!(units_after_start_codes(&[9, 0, 0, 1, 0x65]), [&[0x65][..]]);
    }

    #[test]
    fn units_follow_lengths_of_the_size_the_stream_says() {
        assert_eq!(
            units_after_lengths(&[0, 2, 0x67, 1, 0, 1, 0x68], 2),
            Some(vec![&[0x67, 1][..], &[0x68][..]])
        );
        assert_eq!(units_after_lengths(&[1, 0x65], 1), Some(vec![&[0x65][..]]));
    }

    #[test]
    fn a_length_past_the_data_is_no_stream() {
        assert_eq!(units_after_lengths(&[0, 0, 0, 9, 0x65], 4), None);
        assert_eq!(units_after_lengths(&[0, 0, 0], 4), None);
        assert_eq!(units_after_lengths(&[1, 0x65], 3), None);
    }

    #[test]
    fn a_sample_holds_each_unit_after_four_bytes_of_length() {
        let sample = with_lengths([&[0x65, 4, 5][..], &[0x41][..]]);
        assert_eq!(sample, [0, 0, 0, 3, 0x65, 4, 5, 0, 0, 0, 1, 0x41]);
        assert_eq!(
            units_after_lengths(&sample, LENGTH_SIZE),
            Some(vec![&[0x65, 4, 5][..], &[0x41][..]])
        );
    }
}
