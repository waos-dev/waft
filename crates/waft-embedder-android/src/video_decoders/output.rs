//! What comes out of a decoder, on its way from the threads that decode and draw to the app that
//! reads: its frames, in the order they are shown, and why it ended.
//!
//! Frames wait here until the app reads them, and a decoder's pictures are large, so the thread
//! that decodes waits for room before it lets more out (`wait_for_room`): an app that hands chunks
//! over faster than it reads frames is held back, as MediaCodec's own pool holds back a decoder
//! whose pictures are not released.
//!
//! A reset drops the frames waiting here and counts itself, and a frame is put out with the
//! count of resets its chunk was handed over after, so one decoded across a reset is dropped
//! and never read.
//!
//! The invariant: a frame is handed out once, in the order it was put in, and never one whose
//! chunk was handed over before the last reset; a reader waiting for one is woken when one comes
//! or the decoder ends, and a thread waiting for room when a frame is read, the decoder is reset
//! or the app lets go of it, so neither waits for ever.

use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Poll, Waker};
use std::time::Duration;

use waft_embedder::VideoFrame;
use waft_embedder::video_decoders::DecoderError;

#[derive(Clone, Default)]
pub(super) struct Output(Arc<Shared>);

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    /// Told as a frame is read and as no more are wanted: what the decoding thread waits on.
    room: Condvar,
}

#[derive(Default)]
struct State {
    frames: VecDeque<Arc<dyn VideoFrame>>,
    /// How many times the app has reset the decoder.
    resets: u64,
    /// Whether no more frames come: the decoder ended by itself, or the app let go of it.
    closed: bool,
    /// Why the decoder ended by itself, until the app is told.
    ended: Option<DecoderError>,
    frame_waiter: Option<Waker>,
    end_waiter: Option<Waker>,
}

impl Output {
    /// Puts `frame` out where its chunk was handed over after `resets` resets and none since;
    /// drops it otherwise.
    pub(super) fn put(&self, frame: Arc<dyn VideoFrame>, resets: u64) {
        let waiter = {
            let mut state = self.state();
            if state.resets != resets || state.closed {
                return;
            }
            state.frames.push_back(frame);
            state.frame_waiter.take()
        };
        wake(waiter);
    }

    /// How many times the app has reset the decoder.
    pub(super) fn resets(&self) -> u64 {
        self.state().resets
    }

    /// Whether the app has let go of the decoder, or it ended.
    pub(super) fn is_closed(&self) -> bool {
        self.state().closed
    }

    /// The decoder ended by itself, for `reason`: no more frames come.
    pub(super) fn end(&self, reason: DecoderError) {
        let waiters = {
            let mut state = self.state();
            state.closed = true;
            state.ended = Some(reason);
            [state.frame_waiter.take(), state.end_waiter.take()]
        };
        self.0.room.notify_all();
        waiters.into_iter().for_each(wake);
    }

    /// The app let go of the decoder: no more frames come, and it is told of no end.
    pub(super) fn close(&self) {
        let waiter = {
            let mut state = self.state();
            state.closed = true;
            state.frame_waiter.take()
        };
        self.0.room.notify_all();
        wake(waiter);
    }

    /// The app resets the decoder: every frame waiting to be read is dropped, and those of
    /// chunks handed over before now never come.
    pub(super) fn reset(&self) {
        {
            let mut state = self.state();
            state.frames.clear();
            state.resets += 1;
        }
        self.0.room.notify_all();
    }

    /// Waits up to `patience`, on the thread that decodes, while `limit` frames or more wait to
    /// be read and the app still holds the decoder; answers whether there is room now.
    pub(super) fn wait_for_room(&self, limit: usize, patience: Duration) -> bool {
        let state = self.state();
        let (state, _) = self
            .0
            .room
            .wait_timeout_while(state, patience, |state| {
                state.frames.len() >= limit && !state.closed
            })
            .unwrap_or_else(PoisonError::into_inner);
        state.frames.len() < limit || state.closed
    }

    /// How many frames wait to be read.
    pub(super) fn waiting(&self) -> usize {
        self.state().frames.len()
    }

    /// The next frame; none once no more come and every one put out was read.
    pub(super) fn next_frame(&self) -> impl Future<Output = Option<Arc<dyn VideoFrame>>> + use<> {
        let output = self.clone();
        poll_fn(move |context| {
            let mut state = output.state();
            match state.frames.pop_front() {
                Some(frame) => {
                    output.0.room.notify_all();
                    Poll::Ready(Some(frame))
                }
                None if state.closed => Poll::Ready(None),
                None => {
                    state.frame_waiter = Some(context.waker().clone());
                    Poll::Pending
                }
            }
        })
    }

    /// Why the decoder ended by itself; never, for one that does not.
    pub(super) fn ended(&self) -> impl Future<Output = DecoderError> + use<> {
        let output = self.clone();
        poll_fn(move |context| {
            let mut state = output.state();
            match state.ended.take() {
                Some(reason) => Poll::Ready(reason),
                None => {
                    state.end_waiter = Some(context.waker().clone());
                    Poll::Pending
                }
            }
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn wake(waiter: Option<Waker>) {
    if let Some(waiter) = waiter {
        waiter.wake();
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use waft_embedder::HardwareBuffer;
    use waft_embedder::image_codecs::{ImageFuture, PixelBuffer};

    use super::*;

    /// A frame known by its number alone.
    struct Numbered(u32);

    impl VideoFrame for Numbered {
        fn width(&self) -> u32 {
            self.0
        }
        fn height(&self) -> u32 {
            1
        }
        fn read_pixels(&self) -> ImageFuture<PixelBuffer> {
            unreachable!("the frames here are never read")
        }
        fn hardware_buffer(&self) -> Arc<dyn HardwareBuffer> {
            unreachable!("the frames here have no memory")
        }
        fn rotation(&self) -> u32 {
            0
        }
        fn flip(&self) -> bool {
            false
        }
    }

    fn frame(number: u32) -> Arc<dyn VideoFrame> {
        Arc::new(Numbered(number))
    }

    /// What `future` answers now, if it does without waiting.
    fn now<T>(future: impl Future<Output = T>) -> Option<T> {
        let mut future = pin!(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(answer) => Some(answer),
            Poll::Pending => None,
        }
    }

    fn number_of(frame: Option<Option<Arc<dyn VideoFrame>>>) -> Option<u32> {
        frame.flatten().map(|frame| frame.width())
    }

    #[test]
    fn frames_come_out_in_the_order_they_were_put_in() {
        let output = Output::default();
        output.put(frame(1), 0);
        output.put(frame(2), 0);
        assert_eq!(number_of(now(output.next_frame())), Some(1));
        assert_eq!(number_of(now(output.next_frame())), Some(2));
        assert!(now(output.next_frame()).is_none(), "the next one waits");
    }

    #[test]
    fn a_frame_of_a_chunk_handed_over_before_a_reset_never_comes_out() {
        let output = Output::default();
        output.put(frame(1), 0);
        output.reset();
        output.put(frame(2), 0);
        output.put(frame(3), 1);
        assert_eq!(number_of(now(output.next_frame())), Some(3));
    }

    #[test]
    fn room_is_made_as_frames_are_read() {
        let output = Output::default();
        output.put(frame(1), 0);
        output.put(frame(2), 0);
        assert!(!output.wait_for_room(2, Duration::from_millis(10)));
        let _ = now(output.next_frame());
        assert!(output.wait_for_room(2, Duration::from_millis(10)));
    }

    #[test]
    fn an_ended_decoder_says_why_and_its_frames_end() {
        let output = Output::default();
        output.end(DecoderError::Lost);
        assert_eq!(now(output.ended()), Some(DecoderError::Lost));
        assert!(matches!(now(output.next_frame()), Some(None)));
    }
}
