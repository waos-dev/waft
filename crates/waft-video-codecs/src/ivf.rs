//! The frames of an IVF file, the plain container libvpx and the AV1 tools write a stream in:
//! what the VP9 and AV1 test clips are kept as, each frame one chunk as a file holds it.
//!
//! The invariant: each frame comes out whole, in the order the file holds it.

/// The four bytes an IVF file begins with.
const SIGNATURE: &[u8; 4] = b"DKIF";

/// The bytes before each frame: its length in four, and its time in eight.
const FRAME_HEADER_SIZE: usize = 12;

/// The frames of `file`, an IVF file, in order.
///
/// # Panics
///
/// Where `file` is no IVF file, or a frame runs past its end: the clips are the crate's own.
pub fn frames_of(file: &[u8]) -> Vec<&[u8]> {
    assert_eq!(&file[..4], SIGNATURE, "an IVF file");
    let header_size = usize::from(u16::from_le_bytes([file[6], file[7]]));
    let mut frames = Vec::new();
    let mut rest = &file[header_size..];
    while !rest.is_empty() {
        let length = u32::from_le_bytes(rest[..4].try_into().expect("four bytes")) as usize;
        let (frame, after) = rest[FRAME_HEADER_SIZE..].split_at(length);
        frames.push(frame);
        rest = after;
    }
    frames
}
