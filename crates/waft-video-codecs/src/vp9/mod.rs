//! VP9: a chunk's frames, which chunk decodes with nothing before it, what a key frame's header
//! says of its stream (`key_frame`), and the configuration record a file gives (`record`).
//!
//! A chunk is one shown picture, as WebM and MP4 hold it: a frame, or a superframe that packs the
//! frames shown never or later (an alternate reference) before the one shown now, with an index
//! of their sizes at its end. A system's decoder takes the chunk whole; its frames are read here
//! only for their headers. VP9 never reorders: each chunk's picture is shown after the one
//! before it.
//!
//! The invariant: a stream's size, depth and colour are what its last key frame said, since only
//! a key frame says them.

mod key_frame;
mod record;
#[cfg(any(test, feature = "test-support"))]
pub mod test_clip;

pub use key_frame::KeyFrame;
pub use record::Record;

/// The frames of `chunk`, each a frame with its own header: those its superframe index sizes, or
/// the chunk itself where it has none; why not, where the sizes run past the chunk.
pub fn frames(chunk: &[u8]) -> Result<Vec<&[u8]>, String> {
    let Some(index) = superframe_index(chunk) else {
        return Ok(vec![chunk]);
    };
    let mut frames = Vec::with_capacity(index.sizes.len());
    let mut rest = &chunk[..index.start];
    for size in index.sizes {
        let (frame, after) = rest
            .split_at_checked(size)
            .ok_or("a superframe's frame runs past its chunk")?;
        frames.push(frame);
        rest = after;
    }
    Ok(frames)
}

/// Whether `chunk` decodes with nothing before it: whether its first frame is a key frame.
pub fn is_key_frame(chunk: &[u8]) -> bool {
    frames(chunk)
        .ok()
        .and_then(|frames| frames.first().copied())
        .is_some_and(key_frame::begins_key_frame)
}

/// Where a superframe's index begins, and the sizes it gives its frames.
struct SuperframeIndex {
    start: usize,
    sizes: Vec<usize>,
}

/// The index at the end of `chunk`, where it is a superframe (VP9's Annex B): a marker byte that
/// gives how many frames and how many bytes each size takes, the sizes, little-endian, and the
/// marker again.
fn superframe_index(chunk: &[u8]) -> Option<SuperframeIndex> {
    let marker = *chunk.last()?;
    if marker & 0xE0 != 0xC0 {
        return None;
    }
    let frame_count = usize::from(marker & 0x07) + 1;
    let size_bytes = usize::from((marker >> 3) & 0x03) + 1;
    let index_size = 2 + size_bytes * frame_count;
    let start = chunk.len().checked_sub(index_size)?;
    if chunk[start] != marker {
        return None;
    }
    let sizes = chunk[start + 1..chunk.len() - 1]
        .chunks_exact(size_bytes)
        .map(|size| {
            size.iter()
                .rev()
                .fold(0usize, |size, byte| (size << 8) | usize::from(*byte))
        })
        .collect();
    Some(SuperframeIndex { start, sizes })
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

    /// libvpx packs its alternate reference, shown never, with the picture after the key frame.
    #[test]
    fn a_superframe_holds_a_frame_shown_never_before_the_one_shown_now() {
        let chunks = chunks_of_the_clip();
        let frame_counts: Vec<usize> = chunks
            .iter()
            .map(|chunk| frames(chunk).unwrap().len())
            .collect();
        assert_eq!(frame_counts, [1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]);
        let superframe = frames(chunks[1]).unwrap();
        let sizes: usize = superframe.iter().map(|frame| frame.len()).sum();
        assert_eq!(sizes + 2 + 2 * 2, chunks[1].len(), "two sizes of two bytes");
    }

    #[test]
    fn a_superframe_whose_sizes_run_past_it_is_no_chunk() {
        // One frame of 0x0100 bytes, the index's size taking two bytes.
        let chunk = [0x80, 0xC8, 0x00, 0x01, 0xC8];
        assert!(frames(&chunk).is_err());
        assert!(!is_key_frame(&chunk));
    }

    #[test]
    fn a_chunk_with_no_index_is_one_frame() {
        let chunk = [0x82, 0x49, 0x83, 0x42];
        assert_eq!(frames(&chunk), Ok(vec![&chunk[..]]));
    }
}
