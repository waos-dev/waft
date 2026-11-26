//! Each check of the decoders: what it found, line by line, whether all of it is what the clip
//! holds, and the first picture decoded, to show.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use waft_embedder::image_codecs::PixelBuffer;
use waft_embedder::video_decoders::{
    ChromaSampling, CodecChoice, CodecKind, DecodeSupport, DecoderError, DecodingPurpose,
    EncodedChunk, NalFormat, PictureSize, VideoCodec, VideoDecoder, VideoDecoderOptions,
    VideoDecoders, VideoStreamDescription, VideoStreamSetup,
};
use waft_embedder::{PixelRect, VideoFrame};

use crate::clip::{self, PICTURE, SHOWN_AT, time_of};
use crate::codecs;

/// What one check found.
pub(crate) struct Report {
    pub(crate) title: &'static str,
    pub(crate) lines: Vec<String>,
    pub(crate) passed: bool,
    pub(crate) pictures: Vec<PixelBuffer>,
}

impl Report {
    pub(crate) fn new(title: &'static str) -> Report {
        Report {
            title,
            lines: Vec::new(),
            passed: true,
            pictures: Vec::new(),
        }
    }

    /// Notes `line`, marked as what was expected or not.
    pub(crate) fn expect(&mut self, held: bool, line: String) {
        self.passed &= held;
        self.lines
            .push(format!("{} {line}", if held { "ok" } else { "WRONG" }));
    }

    /// Notes `line`, which says what the system has rather than what is right.
    pub(crate) fn note(&mut self, line: String) {
        self.lines.push(format!("-- {line}"));
    }

    /// Notes that the check could not go on, for `reason`.
    pub(crate) fn failed(mut self, reason: String) -> Report {
        self.expect(false, reason);
        self
    }
}

/// Every check, in order.
pub(crate) async fn all(decoders: &dyn VideoDecoders) -> Vec<Report> {
    vec![
        support(decoders).await,
        in_order_shown(decoders, CodecChoice::Any).await,
        in_order_shown(decoders, CodecChoice::Software).await,
        times_carried(decoders).await,
        after_lengths(decoders).await,
        held_back(decoders).await,
        reset(decoders).await,
        key_frames(decoders).await,
        unsupported(decoders).await,
        codecs::hevc(decoders).await,
        codecs::hevc_after_lengths(decoders).await,
        codecs::hevc_main_10(decoders).await,
        codecs::vp9(decoders).await,
        codecs::av1(decoders).await,
    ]
}

pub(crate) fn options(decoder: CodecChoice) -> VideoDecoderOptions {
    VideoDecoderOptions {
        decoder,
        purpose: DecodingPurpose::Playback,
        frame_limit: 4,
        largest_size: None,
        default_color: None,
    }
}

fn start_codes() -> VideoStreamSetup {
    VideoStreamSetup::H264(NalFormat::StartCodes(None))
}

/// The times of the clip's pictures in the order they are shown.
pub(crate) fn in_order() -> Vec<Option<i64>> {
    (0..12).map(|number| Some(time_of(number))).collect()
}

async fn support(decoders: &dyn VideoDecoders) -> Report {
    let mut report = Report::new("support");
    let description = |codec| VideoStreamDescription {
        codec,
        profile: Some(100),
        level: Some(30),
        bit_depth: 8,
        chroma: ChromaSampling::Yuv420,
        size: PictureSize {
            width: 320,
            height: 240,
        },
        frame_rate: Some(30.0),
    };
    for codec in [
        VideoCodec::H264,
        VideoCodec::Hevc,
        VideoCodec::Vp9,
        VideoCodec::Av1,
    ] {
        let support = decoders.support(&description(codec)).await;
        report.expect(
            support.hardware == DecodeSupport::Unknown
                && support.software == DecodeSupport::Unknown,
            format!("{codec:?} is unknown until tried: {support:?}"),
        );
    }
    let vp8 = decoders.support(&description(VideoCodec::Vp8)).await;
    report.expect(
        vp8.hardware == DecodeSupport::No && vp8.software == DecodeSupport::No,
        format!("VP8 is not handed to MediaCodec: {vp8:?}"),
    );
    report
}

/// The clip comes out in the order it is shown, each frame of its size and with its chunk's time
/// and duration; the first frame's pixels are read back to show.
async fn in_order_shown(decoders: &dyn VideoDecoders, choice: CodecChoice) -> Report {
    let mut report = Report::new(match choice {
        CodecChoice::Software => "start codes, the software decoder",
        _ => "start codes, the system's preferred decoder",
    });
    let started = Instant::now();
    let decoder = match decoders.open(start_codes(), options(choice)).await {
        Ok(decoder) => decoder,
        Err(error) => return report.failed(format!("no decoder: {error:?}")),
    };
    report.note(format!(
        "opened in {:?}, running on {:?}",
        started.elapsed(),
        decoder.kind()
    ));
    if choice == CodecChoice::Software {
        report.expect(
            decoder.kind() == CodecKind::Software,
            format!(
                "the software decoder runs on the processor: {:?}",
                decoder.kind()
            ),
        );
    }
    let started = Instant::now();
    let frames = match decoded(&*decoder, clip::chunks_after_start_codes()).await {
        Ok(frames) => frames,
        Err(error) => return report.failed(error),
    };
    report.note(format!("decoded in {:?}", started.elapsed()));
    let times: Vec<_> = frames.iter().map(|frame| frame.timestamp()).collect();
    report.expect(
        times == in_order(),
        format!("in the order shown: {:?}", in_pictures(&times)),
    );
    let sizes_right = frames.iter().all(|frame| {
        frame.visible_rect()
            == PixelRect {
                x: 0,
                y: 0,
                width: 320,
                height: 240,
            }
    });
    if let Some(first) = frames.first() {
        report.note(format!(
            "the first is {} by {}, its picture {:?}",
            first.width(),
            first.height(),
            first.visible_rect()
        ));
    }
    report.expect(sizes_right, "every picture is 320 by 240".to_owned());
    report.expect(
        frames
            .iter()
            .all(|frame| frame.duration() == Some(Duration::from_nanos(PICTURE as u64))),
        "each with its chunk's duration".to_owned(),
    );
    if let Some(first) = frames.first() {
        report.note(format!("colour {:?}", first.color()));
        match first.read_pixels().await {
            Ok(pixels) => report.pictures.push(pixels),
            Err(error) => report.expect(false, format!("the first frame's pixels: {error:?}")),
        }
    }
    report
}

/// Chunks whose times say the order they are decoded in come out in the order they are shown all
/// the same, each frame with the time its chunk carried: the order is the stream's own.
async fn times_carried(decoders: &dyn VideoDecoders) -> Report {
    let mut report = Report::new("times in the order decoded");
    let Ok(decoder) = decoders
        .open(start_codes(), options(CodecChoice::Any))
        .await
    else {
        return report.failed("no decoder".to_owned());
    };
    let mut chunks = clip::chunks_after_start_codes();
    for (decoded, chunk) in chunks.iter_mut().enumerate() {
        chunk.timestamp = decoded as i64;
    }
    let frames = match decoded(&*decoder, chunks).await {
        Ok(frames) => frames,
        Err(error) => return report.failed(error),
    };
    let mut decoded_in_order_shown: Vec<i64> = (0..12).collect();
    decoded_in_order_shown.sort_by_key(|decoded| SHOWN_AT[*decoded as usize]);
    let carried: Vec<_> = decoded_in_order_shown.into_iter().map(Some).collect();
    let times: Vec<_> = frames.iter().map(|frame| frame.timestamp()).collect();
    report.expect(
        times == carried,
        format!("the times carried, in the order shown: {times:?}"),
    );
    report
}

async fn after_lengths(decoders: &dyn VideoDecoders) -> Report {
    let mut report = Report::new("lengths, with the configuration record");
    let setup = VideoStreamSetup::H264(NalFormat::LengthPrefixed(clip::record()));
    let Ok(decoder) = decoders.open(setup, options(CodecChoice::Any)).await else {
        return report.failed("no decoder".to_owned());
    };
    let frames = match decoded(&*decoder, clip::chunks_after_lengths()).await {
        Ok(frames) => frames,
        Err(error) => return report.failed(error),
    };
    let times: Vec<_> = frames.iter().map(|frame| frame.timestamp()).collect();
    report.expect(
        times == in_order(),
        format!("in the order shown: {:?}", in_pictures(&times)),
    );
    report
}

/// A decoder lets out no more while as many frames as the app holds at once wait to be read.
async fn held_back(decoders: &dyn VideoDecoders) -> Report {
    let mut report = Report::new("held back while frames wait unread");
    let held_at_once = VideoDecoderOptions {
        frame_limit: 2,
        ..options(CodecChoice::Any)
    };
    let Ok(decoder) = decoders.open(start_codes(), held_at_once).await else {
        return report.failed("no decoder".to_owned());
    };
    let decoding: Vec<_> = clip::chunks_after_start_codes()
        .into_iter()
        .map(|chunk| decoder.decode(chunk))
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    let mut ready = 0;
    while now(decoder.next_frame()).flatten().is_some() {
        ready += 1;
    }
    report.expect(ready == 2, format!("{ready} frames waited, of 2 at most"));
    let draining = decoder.drain();
    let mut read = ready;
    while read < 12 {
        if decoder.next_frame().await.is_none() {
            break;
        }
        read += 1;
    }
    report.expect(read == 12, format!("{read} of 12 frames once read on"));
    let answered = answered(decoding).await;
    report.expect(answered.is_ok(), format!("every chunk taken: {answered:?}"));
    report.expect(draining.await.is_ok(), "the decoder drains".to_owned());
    report
}

/// After a reset no picture of a chunk handed over before it comes out.
async fn reset(decoders: &dyn VideoDecoders) -> Report {
    let mut report = Report::new("reset");
    let Ok(decoder) = decoders
        .open(start_codes(), options(CodecChoice::Any))
        .await
    else {
        return report.failed("no decoder".to_owned());
    };
    let before: Vec<_> = clip::chunks_after_start_codes()
        .into_iter()
        .take(6)
        .map(|chunk| decoder.decode(chunk))
        .collect();
    decoder.reset();
    let frames = match decoded(&*decoder, clip::chunks_after_start_codes()).await {
        Ok(frames) => frames,
        Err(error) => return report.failed(error),
    };
    let times: Vec<_> = frames.iter().map(|frame| frame.timestamp()).collect();
    report.expect(
        times == in_order(),
        format!(
            "only the pictures after the reset: {:?}",
            in_pictures(&times)
        ),
    );
    let answered = answered(before).await;
    report.expect(
        answered.is_ok(),
        format!("the chunks dropped are answered: {answered:?}"),
    );
    report.expect(
        now(decoder.next_frame()).is_none(),
        "no frame is left".to_owned(),
    );
    report
}

/// A chunk that is no key frame where one is due is bad data, and the decoder goes on from the
/// next key frame: at the start, before the stream's sets have come, after a drain and after a
/// configure.
async fn key_frames(decoders: &dyn VideoDecoders) -> Report {
    let mut report = Report::new("key frames");
    let Ok(decoder) = decoders
        .open(start_codes(), options(CodecChoice::Any))
        .await
    else {
        return report.failed("no decoder".to_owned());
    };
    let chunks = clip::chunks_after_start_codes();
    let mut later = chunks[1].clone();
    later.key_frame = false;
    let before_sets = decoder.decode(later.clone()).await;
    report.expect(
        matches!(before_sets, Err(DecoderError::BadData(_))),
        format!("a later chunk before the sets: {before_sets:?}"),
    );
    let frames = match decoded(&*decoder, chunks.clone()).await {
        Ok(frames) => frames,
        Err(error) => return report.failed(error),
    };
    report.expect(frames.len() == 12, format!("then {} frames", frames.len()));
    let after_drain = decoder.decode(later.clone()).await;
    report.expect(
        matches!(after_drain, Err(DecoderError::BadData(_))),
        format!("a later chunk after a drain: {after_drain:?}"),
    );
    let configured = decoder.configure(start_codes()).await;
    report.expect(configured.is_ok(), format!("configured: {configured:?}"));
    let after_configure = decoder.decode(later).await;
    report.expect(
        matches!(after_configure, Err(DecoderError::BadData(_))),
        format!("a later chunk after a configure: {after_configure:?}"),
    );
    let frames = match decoded(&*decoder, chunks).await {
        Ok(frames) => frames,
        Err(error) => return report.failed(error),
    };
    let times: Vec<_> = frames.iter().map(|frame| frame.timestamp()).collect();
    report.expect(
        times == in_order(),
        format!("then from the key frame on: {:?}", in_pictures(&times)),
    );
    report
}

async fn unsupported(decoders: &dyn VideoDecoders) -> Report {
    let mut report = Report::new("a codec not handed to MediaCodec");
    let opened = decoders
        .open(VideoStreamSetup::Vp8, options(CodecChoice::Any))
        .await;
    report.expect(
        matches!(opened, Err(DecoderError::Unsupported(_))),
        format!("VP8: {:?}", opened.err()),
    );
    report
}

/// Hands every one of `chunks` to `decoder`, drains it, and answers the frames that came out, in
/// the order they came. The chunks are all handed over first, as an app that does not wait for
/// each does, and the frames read as the decoder puts them out.
pub(crate) async fn decoded(
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
            // Drained, every picture the decoder made is out: one not there now never comes, as a
            // decoder that makes nothing of a stream answers no error.
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
async fn answered(
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

/// Frame times counted in pictures, for the log.
pub(crate) fn in_pictures(times: &[Option<i64>]) -> Vec<Option<i64>> {
    times
        .iter()
        .map(|time| time.map(|time| (time + PICTURE / 2) / PICTURE))
        .collect()
}
