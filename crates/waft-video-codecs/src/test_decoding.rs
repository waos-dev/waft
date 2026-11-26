//! A clip decoded as an app decodes it, for the hosts' decoder tests and checks: every chunk handed
//! over, the decoder drained, and the frames read as they come.
//!
//! The invariant: a decoder that makes fewer pictures than it was handed chunks fails the check
//! once it has drained, and never leaves it waiting, as a system's decoder may make nothing of a
//! stream and answer no error (the Android emulator's HEVC decoder, a macOS virtual machine's
//! AV1).

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use waft_embedder::VideoFrame;
use waft_embedder::video_decoders::{DecoderError, EncodedChunk, VideoDecoder};

/// Hands every one of `chunks` to `decoder`, drains it, and answers the frames that came out, in
/// the order they came. The chunks are all handed over first, as an app that does not wait for
/// each does, and the frames read as the decoder puts them out.
pub async fn decoded(
    decoder: &dyn VideoDecoder,
    chunks: Vec<EncodedChunk>,
) -> Result<Vec<Arc<dyn VideoFrame>>, String> {
    let count = chunks.len();
    let decoding: Vec<_> = chunks
        .into_iter()
        .map(|chunk| decoder.decode(chunk))
        .collect();
    let mut draining = pin!(decoder.drain());
    let mut drained = false;
    let mut frames = Vec::new();
    while frames.len() < count {
        if drained {
            // Drained, every picture the decoder made is out: one not there now never comes.
            match now(decoder.next_frame()) {
                Some(Some(frame)) => frames.push(frame),
                _ => {
                    let made = frames.len();
                    return Err(format!(
                        "the decoder drained after {made} of {count} pictures"
                    ));
                }
            }
            continue;
        }
        let mut next = pin!(decoder.next_frame());
        let arrived = std::future::poll_fn(|context| {
            if let Poll::Ready(frame) = next.as_mut().poll(context) {
                return Poll::Ready(Ok(frame));
            }
            draining.as_mut().poll(context).map(Err)
        })
        .await;
        match arrived {
            Ok(Some(frame)) => frames.push(frame),
            Ok(None) => return Err(format!("the decoder ended after {} frames", frames.len())),
            Err(drain) => {
                drain.map_err(|error| format!("the drain: {error:?}"))?;
                drained = true;
            }
        }
    }
    answered(decoding)
        .await
        .map_err(|error| format!("a chunk: {error:?}"))?;
    if !drained {
        draining
            .await
            .map_err(|error| format!("the drain: {error:?}"))?;
    }
    Ok(frames)
}

/// Waits for every one of `answers`; the first refusal, if any.
pub async fn answered(
    answers: Vec<impl Future<Output = Result<(), DecoderError>>>,
) -> Result<(), DecoderError> {
    let mut result = Ok(());
    for answer in answers {
        if let Err(error) = answer.await
            && result.is_ok()
        {
            result = Err(error);
        }
    }
    result
}

/// What `future` answers now, if it does without waiting.
pub fn now<T>(future: impl Future<Output = T>) -> Option<T> {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(answer) => Some(answer),
        Poll::Pending => None,
    }
}
