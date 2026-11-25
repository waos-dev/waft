//! What comes out of a decoder, on its way from the queue that decodes to the app that reads:
//! its frames, in the order they are shown, and why it ended.
//!
//! Frames wait here until the app reads them, and a decoder's pictures are large, so the queue
//! that decodes waits for room before it decodes more (`wait_for_room`): an app that hands chunks
//! over faster than it reads frames is held back, as a system whose decoder draws from a fixed
//! pool holds it back.
//!
//! A reset drops the frames waiting here and counts itself, and a frame is put out with the
//! count of resets its chunk was handed over after, so one decoded across a reset is dropped
//! and never read.
//!
//! The invariant: a frame is handed out once, in the order it was put in, and never one whose
//! chunk was handed over before the last reset; a reader waiting for one is woken when one comes
//! or the decoder ends, and the queue waiting for room when a frame is read, the decoder is
//! reset or the app lets go of it, so neither waits for ever.

use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Poll, Waker};

use waft_embedder::VideoFrame;
use waft_embedder::video_decoders::DecoderError;

#[derive(Clone, Default)]
pub(super) struct Output(Arc<Shared>);

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    /// Told as a frame is read and as no more are wanted: what the decoding queue waits on.
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
    /// Puts `frames` out, in order, where their chunks were handed over after `resets` resets
    /// and none since; drops them otherwise.
    pub(super) fn put(&self, frames: impl IntoIterator<Item = Arc<dyn VideoFrame>>, resets: u64) {
        let waiter = {
            let mut state = self.state();
            if state.resets != resets {
                return;
            }
            state.frames.extend(frames);
            state.frame_waiter.take()
        };
        wake(waiter);
    }

    /// How many times the app has reset the decoder.
    pub(super) fn resets(&self) -> u64 {
        self.state().resets
    }

    /// The decoder ended by itself, for `reason`: no more frames come.
    pub(super) fn end(&self, reason: DecoderError) {
        let waiters = {
            let mut state = self.state();
            state.closed = true;
            state.ended = Some(reason);
            [state.frame_waiter.take(), state.end_waiter.take()]
        };
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

    /// Waits, on the queue that decodes, while `limit` frames or more wait to be read and the
    /// app still holds the decoder.
    pub(super) fn wait_for_room(&self, limit: usize) {
        let mut state = self.state();
        while state.frames.len() >= limit && !state.closed {
            state = self
                .0
                .room
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// How many frames wait to be read.
    #[cfg(test)]
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
