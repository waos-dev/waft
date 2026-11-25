//! The chunks a codec was handed whose pictures have not reached the app: what each chunk said of
//! its picture, found again by the number the codec carried for it, and how many pictures the
//! codec rendered that the image stream has not delivered yet.
//!
//! Two threads share it: the decoder's, which hands chunks over and renders their pictures, and
//! the image stream's, which delivers them. A picture is rendered on the one and delivered on the
//! other, so the decoder's thread waits here for the picture it rendered before it renders the next,
//! and before it says a drain is done.
//!
//! The invariant: a chunk's facts are taken once, by the picture of its number; a rendered picture
//! is counted until it is delivered or the codec is flushed, so a drain never waits for a picture
//! a flush dropped.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use waft_embedder::{PixelAspect, VideoColor};

/// What a chunk says of its picture, carried to its frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChunkFacts {
    /// When the picture is shown, in nanoseconds, as the app gave it.
    pub(super) timestamp: i64,
    pub(super) duration: Option<Duration>,
    /// How many resets the chunk was handed over after.
    pub(super) resets: u64,
    pub(super) pixel_aspect: PixelAspect,
    pub(super) color: VideoColor,
}

#[derive(Default)]
pub(super) struct Pending {
    state: Mutex<State>,
    /// Told as a rendered picture is delivered, or the codec flushed.
    delivered: Condvar,
}

#[derive(Default)]
struct State {
    /// Each chunk handed over, by its number, and whether its picture was rendered.
    chunks: HashMap<u64, (ChunkFacts, bool)>,
    /// How many pictures were rendered and not yet delivered.
    rendered: usize,
}

impl Pending {
    /// The codec took the chunk of `number`, which says `facts`.
    pub(super) fn handed(&self, number: u64, facts: ChunkFacts) {
        self.state().chunks.insert(number, (facts, false));
    }

    /// The codec let out the picture of `number`: answers whether it is wanted, a chunk's that is
    /// still pending, and counts it rendered where it is.
    pub(super) fn rendered(&self, number: u64) -> bool {
        let mut state = self.state();
        let Some((_, rendered)) = state.chunks.get_mut(&number) else {
            return false;
        };
        *rendered = true;
        state.rendered += 1;
        true
    }

    /// The facts of the chunk whose picture of `number` the stream delivers; none for a picture
    /// of a chunk no longer pending.
    pub(super) fn take(&self, number: u64) -> Option<ChunkFacts> {
        self.state().chunks.remove(&number).map(|(facts, _)| facts)
    }

    /// A rendered picture was delivered, and its frame put out where it was wanted.
    pub(super) fn delivered(&self) {
        let mut state = self.state();
        state.rendered = state.rendered.saturating_sub(1);
        drop(state);
        self.delivered.notify_all();
    }

    /// How many pictures were rendered and not yet delivered.
    pub(super) fn rendered_count(&self) -> usize {
        self.state().rendered
    }

    /// Whether a chunk handed over has not come out of the codec yet.
    pub(super) fn awaits_pictures(&self) -> bool {
        self.state().chunks.values().any(|(_, rendered)| !rendered)
    }

    /// Waits up to `patience` until every picture rendered was delivered; answers whether it was.
    pub(super) fn wait_until_delivered(&self, patience: Duration) -> bool {
        let state = self.state();
        let (state, _) = self
            .delivered
            .wait_timeout_while(state, patience, |state| state.rendered > 0)
            .unwrap_or_else(PoisonError::into_inner);
        state.rendered == 0
    }

    /// The codec was flushed: no chunk handed over before comes out.
    pub(super) fn clear(&self) {
        let mut state = self.state();
        state.chunks.clear();
        state.rendered = 0;
        drop(state);
        self.delivered.notify_all();
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(timestamp: i64) -> ChunkFacts {
        ChunkFacts {
            timestamp,
            duration: None,
            resets: 0,
            pixel_aspect: PixelAspect {
                width: 1,
                height: 1,
            },
            color: VideoColor {
                primaries: None,
                transfer: None,
                matrix: None,
                range: None,
                chroma_location: None,
            },
        }
    }

    #[test]
    fn a_picture_finds_its_chunk_by_number_once() {
        let pending = Pending::default();
        pending.handed(3, facts(30));
        assert!(pending.awaits_pictures());
        assert!(pending.rendered(3));
        assert!(!pending.awaits_pictures());
        assert_eq!(pending.take(3), Some(facts(30)));
        assert_eq!(pending.take(3), None);
    }

    #[test]
    fn a_picture_of_no_pending_chunk_is_not_wanted() {
        let pending = Pending::default();
        assert!(!pending.rendered(7));
        assert_eq!(pending.rendered_count(), 0);
    }

    #[test]
    fn a_drain_waits_for_what_was_rendered_until_it_is_delivered() {
        let pending = Pending::default();
        pending.handed(1, facts(10));
        pending.rendered(1);
        assert!(!pending.wait_until_delivered(Duration::from_millis(10)));
        pending.take(1);
        pending.delivered();
        assert!(pending.wait_until_delivered(Duration::from_millis(10)));
    }

    #[test]
    fn a_flush_drops_every_chunk_and_what_was_rendered() {
        let pending = Pending::default();
        pending.handed(1, facts(10));
        pending.rendered(1);
        pending.clear();
        assert_eq!(pending.rendered_count(), 0);
        assert_eq!(pending.take(1), None);
        pending.delivered();
        assert_eq!(
            pending.rendered_count(),
            0,
            "a late delivery counts nothing"
        );
    }
}
