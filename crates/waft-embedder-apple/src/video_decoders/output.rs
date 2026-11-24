//! What comes out of a decoder, on its way from the queue that decodes to the app that reads:
//! its frames, in the order they are shown, and why it ended.
//!
//! The invariant: a frame is handed out once, in the order it was put in, and a reader waiting
//! for one is woken when one comes or the decoder ends, so it never waits for ever.

use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Poll, Waker};

use waft_embedder::VideoFrame;
use waft_embedder::video_decoders::DecoderError;

#[derive(Clone, Default)]
pub(super) struct Output(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    frames: VecDeque<Arc<dyn VideoFrame>>,
    /// Whether no more frames come: the decoder ended by itself, or the app let go of it.
    closed: bool,
    /// Why the decoder ended by itself, until the app is told.
    ended: Option<DecoderError>,
    frame_waiter: Option<Waker>,
    end_waiter: Option<Waker>,
}

impl Output {
    /// Puts `frames` out, in order.
    pub(super) fn put(&self, frames: impl IntoIterator<Item = Arc<dyn VideoFrame>>) {
        let waiter = {
            let mut state = self.state();
            state.frames.extend(frames);
            state.frame_waiter.take()
        };
        wake(waiter);
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
        wake(waiter);
    }

    /// The next frame; none once no more come and every one put out was read.
    pub(super) fn next_frame(&self) -> impl Future<Output = Option<Arc<dyn VideoFrame>>> + use<> {
        let output = self.clone();
        poll_fn(move |context| {
            let mut state = output.state();
            match state.frames.pop_front() {
                Some(frame) => Poll::Ready(Some(frame)),
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
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn wake(waiter: Option<Waker>) {
    if let Some(waiter) = waiter {
        waiter.wake();
    }
}
