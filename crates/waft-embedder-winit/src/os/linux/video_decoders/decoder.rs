//! A decoder the app holds: a stream's chunks in, its pictures out in the order they are shown.
//!
//! Each decoder has two threads of its own beside GStreamer's. One feeds it (`Feeder`): it takes
//! what the app asks, one job after another, so the app's thread never waits on GStreamer and
//! chunks reach the decoder in the order they came. The other takes its pictures (`Taker`): it
//! pulls each from the pipeline's sink, finds what the chunk it came from said, and puts its frame
//! out. Both wait for room among the frames waiting to be read, so GStreamer decodes no further
//! ahead than the app reads.
//!
//! Each chunk is numbered as it is pushed, and its number rides on its buffer to its picture (a
//! reference timestamp, which GStreamer's decoders carry over); the picture's frame takes the time
//! the chunk gave by that number, so the stream's order is its own whatever the times say.
//!
//! A key frame is due at the stream's start and after every drain, reset and other setup, each of
//! which runs the pipeline afresh; a chunk that is no key frame where one is due is answered
//! `BadData` and not pushed.
//!
//! A reset drops what the decoder holds at the moment the app asks, though chunks handed over
//! before it may still wait for the feeder: each carries the count of resets it was handed over
//! after, and one handed over before the last reset is not pushed, or its picture not put out
//! (`output`).
//!
//! The invariant: a job's answer is given once, whether or not the app still holds the decoder;
//! and no picture of a chunk handed over before a reset comes out after it.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use waft_embedder::VideoFrame;
use waft_embedder::video_decoders::{
    CodecKind, DecodedFrameFuture, DecoderError, EncodedChunk, VideoDecoder, VideoDecoderFuture,
    VideoDecoderOptions, VideoStreamSetup,
};

use super::super::gstreamer::{Gst, Pulled, Sample};
use super::frame::{ChunkFacts, DecodedFrame, FramePool};
use super::output::Output;
use super::pipeline::{Framing, Path, Pipeline};
use crate::answer::{Answer, answer};

/// How long the taker waits for a picture before it looks again whether the decoder is done.
const PULL_WAIT: Duration = Duration::from_millis(50);

/// What the chunks pushed said of their pictures, by their numbers, and the count of resets each
/// was handed over after; a chunk's entry goes as its picture comes.
type Chunks = Arc<Mutex<HashMap<u64, (ChunkFacts, u64)>>>;

/// What the app asks of the feeder, each with the count of resets it was asked after.
enum Job {
    Decode(EncodedChunk, Answer<Result<(), DecoderError>>, u64),
    Drain(Answer<Result<(), DecoderError>>),
    Configure(VideoStreamSetup, Answer<Result<(), DecoderError>>),
    /// Drops everything the pipeline holds, as the app's reset asked.
    Reset,
}

/// A GStreamer decoder as the app holds it.
pub(super) struct Decoder {
    jobs: Sender<Job>,
    output: Output,
    kind: CodecKind,
}

impl Decoder {
    /// A decoder for the stream `setup` begins, on `path`, its pipeline running; pictures
    /// decoded on the processor are copied into `pool`'s dmabufs.
    pub(super) fn open(
        gst: &'static Gst,
        path: Path,
        pool: Option<Arc<FramePool>>,
        setup: VideoStreamSetup,
        options: &VideoDecoderOptions,
    ) -> Result<Decoder, DecoderError> {
        let framing = Framing::of(&setup)?;
        let pipeline = Arc::new(Pipeline::open(gst, path, &framing)?);
        let output = Output::default();
        let chunks = Chunks::default();
        let frame_limit = options.frame_limit.max(1) as usize;
        let fed = Arc::new(AtomicBool::new(false));
        let taker = Taker {
            pipeline: Arc::clone(&pipeline),
            pool,
            chunks: Arc::clone(&chunks),
            output: output.clone(),
            frame_limit,
            fed: Arc::clone(&fed),
        };
        let feeder = Feeder {
            pipeline,
            framing,
            chunks,
            output: output.clone(),
            frame_limit,
            next_number: 0,
            key_frame_due: true,
            fed,
        };
        let (jobs, asked) = channel();
        spawn("waft-video-taker", move || taker.take_pictures())?;
        spawn("waft-video-feeder", move || feeder.feed(asked))?;
        let kind = match path {
            Path::Hardware(_) => CodecKind::Hardware,
            Path::Software(_) => CodecKind::Software,
        };
        Ok(Decoder { jobs, output, kind })
    }

    /// Hands `job` to the feeder, made of the answer it gives, and answers what it answered.
    fn ask(&self, job: impl FnOnce(Answer<Result<(), DecoderError>>) -> Job) -> VideoDecoderFuture {
        let gone = Err(DecoderError::Failed("the decoder went away".to_owned()));
        let (answer, answered) = answer(gone);
        // A feeder that is gone drops the job, whose answer then says so.
        let _ = self.jobs.send(job(answer));
        Box::pin(answered)
    }
}

impl VideoDecoder for Decoder {
    fn kind(&self) -> CodecKind {
        self.kind
    }

    /// The chunk waits its turn with the feeder, and then for room among the frames waiting to
    /// be read. One the app reset the decoder after is dropped unpushed, as the reset asked.
    fn decode(&self, chunk: EncodedChunk) -> VideoDecoderFuture {
        let resets = self.output.resets();
        self.ask(|answer| Job::Decode(chunk, answer, resets))
    }

    fn next_frame(&self) -> DecodedFrameFuture {
        Box::pin(self.output.next_frame())
    }

    fn ended(&self) -> Pin<Box<dyn Future<Output = DecoderError>>> {
        Box::pin(self.output.ended())
    }

    fn drain(&self) -> VideoDecoderFuture {
        self.ask(Job::Drain)
    }

    /// The frames waiting to be read are dropped at once, and the chunks still with the feeder
    /// as their turn comes; what the pipeline holds goes once the feeder reaches this, after the
    /// chunk it may be pushing now.
    fn reset(&self) {
        self.output.reset();
        let _ = self.jobs.send(Job::Reset);
    }

    fn configure(&self, setup: VideoStreamSetup) -> VideoDecoderFuture {
        self.ask(|answer| Job::Configure(setup, answer))
    }
}

impl Drop for Decoder {
    /// The frames end; the pipeline goes once the feeder has done what it was handed.
    fn drop(&mut self) {
        self.output.close();
    }
}

/// What the feeder thread works on: the stream as far as it was handed over.
struct Feeder {
    pipeline: Arc<Pipeline>,
    framing: Framing,
    chunks: Chunks,
    output: Output,
    frame_limit: usize,
    /// The number the next chunk pushed carries.
    next_number: u64,
    /// Whether the next chunk pushed must be a key frame.
    key_frame_due: bool,
    /// Told once the feeder is done, as the app let go of the decoder: the taker then stops.
    fed: Arc<AtomicBool>,
}

impl Feeder {
    /// Does each job the app asks, in order, until the app lets go of the decoder.
    fn feed(mut self, asked: Receiver<Job>) {
        for job in asked {
            match job {
                Job::Decode(chunk, answer, resets) => answer.give(self.decode(&chunk, resets)),
                Job::Drain(answer) => answer.give(self.drain()),
                Job::Configure(setup, answer) => answer.give(self.configure(setup)),
                Job::Reset => self.reset(),
            }
        }
        self.fed.store(true, Ordering::Release);
    }

    /// Pushes `chunk`, handed over after `resets` resets, once the app has read enough of the
    /// frames put out.
    fn decode(&mut self, chunk: &EncodedChunk, resets: u64) -> Result<(), DecoderError> {
        self.output.wait_for_room(self.frame_limit);
        if self.output.resets() != resets {
            return Ok(());
        }
        self.go_on()?;
        if self.key_frame_due && !chunk.key_frame {
            return Err(DecoderError::BadData(
                "a key frame is due: at the stream's start, or after a drain, a reset or another \
                 setup"
                    .to_owned(),
            ));
        }
        self.key_frame_due = false;
        let number = self.next_number;
        self.next_number += 1;
        let facts = ChunkFacts {
            timestamp: chunk.timestamp,
            duration: chunk.duration,
        };
        self.chunks_lock().insert(number, (facts, resets));
        let data = self.framing.framed(&chunk.data, chunk.key_frame);
        self.pipeline
            .push(&data, number)
            .map_err(|why| self.failed(why))
    }

    /// Puts out every picture of the chunks pushed, then runs the pipeline afresh.
    fn drain(&mut self) -> Result<(), DecoderError> {
        self.finish()?;
        self.restart()
    }

    /// Another setup from the next chunk on: the pictures of the chunks pushed come out first,
    /// as they come before the change.
    fn configure(&mut self, setup: VideoStreamSetup) -> Result<(), DecoderError> {
        let framing = Framing::of(&setup)?;
        self.finish()?;
        self.framing = framing;
        self.restart()
    }

    /// Puts out every picture of the chunks pushed, whatever the limit.
    fn finish(&self) -> Result<(), DecoderError> {
        self.go_on()?;
        self.output.drain_begins();
        let finished = self.pipeline.finish();
        self.output.drain_ends();
        finished.map_err(|why| self.failed(why))
    }

    fn reset(&mut self) {
        let _ = self.restart();
    }

    /// Runs the pipeline afresh: whatever it held is dropped, and a key frame is due.
    fn restart(&mut self) -> Result<(), DecoderError> {
        self.chunks_lock().clear();
        self.key_frame_due = true;
        self.pipeline
            .run(&self.framing)
            .map_err(|why| self.failed(why))
    }

    /// Whether the decoder goes on: not once its pipeline failed.
    fn go_on(&self) -> Result<(), DecoderError> {
        match self.pipeline.failure() {
            Some(why) => Err(self.failed(why)),
            None => Ok(()),
        }
    }

    /// The decoder ended by itself, for `why`: the app is told beside its frames.
    fn failed(&self, why: String) -> DecoderError {
        let reason = DecoderError::Failed(why);
        self.output.end(reason.clone());
        reason
    }

    fn chunks_lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, (ChunkFacts, u64)>> {
        self.chunks.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the taker thread works on: the pipeline's pictures on their way to frames.
struct Taker {
    pipeline: Arc<Pipeline>,
    pool: Option<Arc<FramePool>>,
    chunks: Chunks,
    output: Output,
    frame_limit: usize,
    fed: Arc<AtomicBool>,
}

impl Taker {
    /// Takes each picture the pipeline puts out until the feeder is done or the pipeline failed.
    fn take_pictures(self) {
        while !self.fed.load(Ordering::Acquire) {
            if let Some(why) = self.pipeline.failure() {
                self.output.end(DecoderError::Failed(why));
                return;
            }
            self.output.wait_for_room(self.frame_limit);
            if let Pulled::Sample(sample) = self.pipeline.pull(PULL_WAIT) {
                self.put_out(sample);
            }
        }
    }

    /// Puts `sample`'s frame out with what its chunk said; drops a picture whose chunk is not
    /// known, as one pushed before the pipeline last ran afresh.
    fn put_out(&self, sample: Sample) {
        let Some(number) = sample.reference(self.pipeline.reference()) else {
            eprintln!("waft: a decoded picture carried no chunk number; dropped");
            return;
        };
        let Some((chunk, resets)) = self
            .chunks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&number)
        else {
            return;
        };
        match self.frame_of(sample, chunk) {
            Ok(frame) => self.output.put([frame], resets),
            Err(why) => self.output.end(DecoderError::Failed(why)),
        }
    }

    fn frame_of(&self, sample: Sample, chunk: ChunkFacts) -> Result<Arc<dyn VideoFrame>, String> {
        let facts = sample
            .facts()
            .ok_or("a decoded picture came with no size")?;
        let frame = match self.pipeline.path() {
            Path::Hardware(_) => DecodedFrame::of_dmabuf(sample, &facts, chunk)
                .ok_or("the video hardware's picture is no ARGB8888 dmabuf")?,
            Path::Software(_) => {
                let pool = self
                    .pool
                    .as_ref()
                    .ok_or("no dmabufs to copy a picture into")?;
                DecodedFrame::copied(&sample, &facts, chunk, pool)
                    .map_err(|error| format!("{error:?}"))?
            }
        };
        Ok(Arc::new(frame))
    }
}

/// A thread named `name` running `work`.
fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> Result<(), DecoderError> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(work)
        .map(drop)
        .map_err(|error| DecoderError::Failed(format!("no thread to decode on: {error}")))
}

#[cfg(test)]
mod tests {
    use pollster::block_on;
    use waft_embedder::PixelRect;
    use waft_embedder::video_decoders::{CodecChoice, DecodingPurpose, NalFormat};
    use waft_video_codecs::h264::test_clip::CLIP;

    use super::*;

    /// When each of the clip's pictures is shown, in the order they are decoded, counted in
    /// pictures: what `ffprobe` reads of the clip's packets.
    const SHOWN_AT: [i64; 12] = [0, 3, 1, 2, 5, 4, 8, 6, 7, 11, 9, 10];

    /// An access unit delimiter's type, which begins each of the clip's pictures.
    const DELIMITER: u8 = 9;
    const SEQUENCE_PARAMETER_SET: u8 = 7;
    const PICTURE_PARAMETER_SET: u8 = 8;

    /// The time of a picture shown `number` pictures in, in nanoseconds, at thirty a second.
    fn time_of(number: i64) -> i64 {
        number * 33_333_333
    }

    fn options() -> VideoDecoderOptions {
        VideoDecoderOptions {
            decoder: CodecChoice::Software,
            purpose: DecodingPurpose::Playback,
            frame_limit: 4,
            largest_size: None,
            default_color: None,
        }
    }

    fn open(
        setup: VideoStreamSetup,
        options: &VideoDecoderOptions,
    ) -> Result<Decoder, DecoderError> {
        let gst = Gst::get().expect("GStreamer");
        let pool = FramePool::new().map(Arc::new);
        Decoder::open(gst, Path::Software("avdec_h264"), pool, setup, options)
    }

    /// The clip's units, each without its start code.
    fn units_of_the_clip() -> Vec<&'static [u8]> {
        let mut starts = Vec::new();
        let mut at = 0;
        while at + 3 <= CLIP.len() {
            if CLIP[at..at + 3] == [0, 0, 1] {
                starts.push(at + 3);
                at += 3;
            } else {
                at += 1;
            }
        }
        let ends = starts
            .iter()
            .skip(1)
            .map(|start| start - 3)
            .chain([CLIP.len()]);
        starts
            .iter()
            .zip(ends)
            .map(|(&start, end)| {
                let unit = &CLIP[start..end];
                let zeros = unit.iter().rev().take_while(|byte| **byte == 0).count();
                &unit[..unit.len() - zeros]
            })
            .filter(|unit| !unit.is_empty())
            .collect()
    }

    /// The clip's pictures, in the order they are decoded, each its units.
    fn pictures_of_the_clip() -> Vec<Vec<&'static [u8]>> {
        let mut pictures: Vec<Vec<&[u8]>> = Vec::new();
        for unit in units_of_the_clip() {
            if unit[0] & 0x1F == DELIMITER {
                pictures.push(Vec::new());
            }
            if let Some(picture) = pictures.last_mut() {
                picture.push(unit);
            }
        }
        pictures
    }

    fn is_parameter_set(unit: &[u8]) -> bool {
        matches!(
            unit[0] & 0x1F,
            SEQUENCE_PARAMETER_SET | PICTURE_PARAMETER_SET
        )
    }

    /// The clip's chunks in the order they are decoded, each framed by `framed`.
    fn chunks(framed: impl Fn(&[&[u8]]) -> Vec<u8>) -> Vec<EncodedChunk> {
        pictures_of_the_clip()
            .iter()
            .zip(SHOWN_AT)
            .enumerate()
            .map(|(decoded, (units, shown))| EncodedChunk {
                data: framed(units),
                timestamp: time_of(shown),
                duration: Some(33_333_333),
                key_frame: decoded == 0,
            })
            .collect()
    }

    /// `units` each after a start code, as the clip holds them.
    fn after_start_codes(units: &[&[u8]]) -> Vec<u8> {
        units
            .iter()
            .flat_map(|unit| [&[0, 0, 0, 1][..], unit].concat())
            .collect()
    }

    /// `units` other than parameter sets, each after its length in four bytes.
    fn after_lengths(units: &[&[u8]]) -> Vec<u8> {
        units
            .iter()
            .filter(|unit| !is_parameter_set(unit))
            .flat_map(|unit| [&(unit.len() as u32).to_be_bytes()[..], unit].concat())
            .collect()
    }

    /// The configuration record (`avcC`) a file holds for the clip, its units after lengths of
    /// four bytes.
    fn record_of_the_clip() -> Vec<u8> {
        let units = units_of_the_clip();
        let sets_of = |kind| units.iter().filter(move |unit| unit[0] & 0x1F == kind);
        let sequence: Vec<_> = sets_of(SEQUENCE_PARAMETER_SET).collect();
        let first = sequence[0];
        let mut record = vec![1, first[1], first[2], first[3], 0xFF];
        record.push(0xE0 | sequence.len() as u8);
        for set in &sequence {
            record.extend_from_slice(&(set.len() as u16).to_be_bytes());
            record.extend_from_slice(set);
        }
        let picture: Vec<_> = sets_of(PICTURE_PARAMETER_SET).collect();
        record.push(picture.len() as u8);
        for set in &picture {
            record.extend_from_slice(&(set.len() as u16).to_be_bytes());
            record.extend_from_slice(set);
        }
        record
    }

    /// Hands every one of `chunks` to `decoder`, drains it, and answers the frames that came out,
    /// in the order they came. The chunks are all handed over first, as an app that does not wait
    /// for each does, and the frames read as the decoder puts them out.
    fn frames_decoded(decoder: &Decoder, chunks: Vec<EncodedChunk>) -> Vec<Arc<dyn VideoFrame>> {
        let count = chunks.len();
        let decoding: Vec<_> = chunks
            .into_iter()
            .map(|chunk| decoder.decode(chunk))
            .collect();
        let draining = decoder.drain();
        let frames = (0..count)
            .map(|_| block_on(decoder.next_frame()).expect("a frame"))
            .collect();
        for decoded in decoding {
            block_on(decoded).expect("the chunk decodes");
        }
        block_on(draining).expect("the decoder drains");
        frames
    }

    fn times_decoded(decoder: &Decoder, chunks: Vec<EncodedChunk>) -> Vec<Option<i64>> {
        frames_decoded(decoder, chunks)
            .iter()
            .map(|frame| frame.timestamp())
            .collect()
    }

    fn in_order() -> Vec<Option<i64>> {
        (0..12).map(|number| Some(time_of(number))).collect()
    }

    #[test]
    fn the_clip_holds_twelve_pictures_and_its_sets() {
        assert_eq!(pictures_of_the_clip().len(), 12);
        assert!(
            units_of_the_clip()
                .iter()
                .any(|unit| is_parameter_set(unit))
        );
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn a_stream_in_start_codes_comes_out_in_the_order_it_is_shown() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = open(setup, &options()).expect("a decoder");
        let frames = frames_decoded(&decoder, chunks(after_start_codes));
        let times: Vec<_> = frames.iter().map(|frame| frame.timestamp()).collect();
        assert_eq!(times, in_order());
        for frame in &frames {
            assert_eq!((frame.width(), frame.height()), (320, 240));
            let buffer = frame.hardware_buffer();
            assert_eq!((buffer.width(), buffer.height()), (320, 240));
        }
    }

    /// The order is the stream's own: chunks whose times say the order they are decoded in, as
    /// an app that knows no better gives them, come out in the order they are shown all the
    /// same, each frame with the time its chunk carried.
    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn a_streams_order_is_its_own_whatever_times_its_chunks_carry() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = open(setup, &options()).expect("a decoder");
        let mut chunks = chunks(after_start_codes);
        for (decoded, chunk) in chunks.iter_mut().enumerate() {
            chunk.timestamp = decoded as i64;
        }
        let times = times_decoded(&decoder, chunks);
        // The picture decoded second is shown fourth, and so on, as `SHOWN_AT` says.
        let mut decoded_in_order_shown: Vec<i64> = (0..12).collect();
        decoded_in_order_shown.sort_by_key(|decoded| SHOWN_AT[*decoded as usize]);
        let carried: Vec<_> = decoded_in_order_shown.into_iter().map(Some).collect();
        assert_eq!(times, carried);
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn a_stream_after_lengths_with_its_record_comes_out_the_same() {
        let setup = VideoStreamSetup::H264(NalFormat::LengthPrefixed(record_of_the_clip()));
        let decoder = open(setup, &options()).expect("a decoder");
        let times = times_decoded(&decoder, chunks(after_lengths));
        assert_eq!(times, in_order());
    }

    /// Sets given ahead reach the decoder before each key frame, so chunks without them decode.
    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn a_stream_in_start_codes_with_its_sets_ahead_comes_out_the_same() {
        let sets: Vec<&[u8]> = units_of_the_clip()
            .into_iter()
            .filter(|unit| is_parameter_set(unit))
            .collect();
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(Some(after_start_codes(&sets))));
        let decoder = open(setup, &options()).expect("a decoder");
        let without_sets = |units: &[&[u8]]| {
            let coded: Vec<&[u8]> = units
                .iter()
                .copied()
                .filter(|unit| !is_parameter_set(unit))
                .collect();
            after_start_codes(&coded)
        };
        let times = times_decoded(&decoder, chunks(without_sets));
        assert_eq!(times, in_order());
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn a_frame_says_what_its_decoder_and_its_stream_say_of_it() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = open(setup, &options()).expect("a decoder");
        assert_eq!(decoder.kind(), CodecKind::Software);
        let first = chunks(after_start_codes).remove(0);
        block_on(decoder.decode(first)).expect("the chunk decodes");
        block_on(decoder.drain()).expect("the decoder drains");
        let frame = block_on(decoder.next_frame()).expect("a frame");
        assert_eq!((frame.width(), frame.height()), (320, 240));
        assert_eq!(
            frame.visible_rect(),
            PixelRect {
                x: 0,
                y: 0,
                width: 320,
                height: 240
            }
        );
        assert_eq!(frame.timestamp(), Some(0));
        assert_eq!(frame.duration(), Some(Duration::from_nanos(33_333_333)));
        let pixels = block_on(frame.read_pixels()).expect("the frame's pixels");
        assert_eq!(pixels.layout().size, [320, 240]);
        assert!(
            pixels.bytes().iter().any(|byte| *byte != 0),
            "a decoded picture, not an empty buffer"
        );
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn a_chunk_before_a_key_frame_is_bad_data_and_the_decoder_goes_on() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = open(setup, &options()).expect("a decoder");
        let mut later = chunks(after_start_codes).remove(1);
        later.key_frame = false;
        assert!(matches!(
            block_on(decoder.decode(later)),
            Err(DecoderError::BadData(_))
        ));
        let times = times_decoded(&decoder, chunks(after_start_codes));
        assert_eq!(times, in_order());
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn after_a_drain_a_key_frame_is_due_again() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = open(setup, &options()).expect("a decoder");
        let _ = times_decoded(&decoder, chunks(after_start_codes));
        let later = chunks(after_start_codes).remove(1);
        assert!(matches!(
            block_on(decoder.decode(later)),
            Err(DecoderError::BadData(_))
        ));
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn another_setup_decodes_the_stream_on() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = open(setup, &options()).expect("a decoder");
        assert_eq!(
            times_decoded(&decoder, chunks(after_start_codes)),
            in_order()
        );
        let lengths = VideoStreamSetup::H264(NalFormat::LengthPrefixed(record_of_the_clip()));
        block_on(decoder.configure(lengths)).expect("the decoder takes the setup");
        assert_eq!(times_decoded(&decoder, chunks(after_lengths)), in_order());
    }

    #[test]
    fn a_codec_gstreamer_is_not_handed_yet_is_unsupported() {
        assert!(matches!(
            Framing::of(&VideoStreamSetup::Vp9(None)),
            Err(DecoderError::Unsupported(_))
        ));
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn no_more_is_decoded_while_as_many_frames_as_the_app_holds_wait_to_be_read() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let held_at_once = VideoDecoderOptions {
            frame_limit: 2,
            ..options()
        };
        let decoder = open(setup, &held_at_once).expect("a decoder");
        let decoding: Vec<_> = chunks(after_start_codes)
            .into_iter()
            .map(|chunk| decoder.decode(chunk))
            .collect();
        // The decoder decodes until two frames wait, then stops: nothing reads them.
        thread::sleep(Duration::from_millis(500));
        assert_eq!(decoder.output.waiting(), 2);
        drop(decoder);
        // Let go of, the decoder decodes on to no one, and every chunk is answered.
        for decoded in decoding {
            let _ = block_on(decoded);
        }
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn after_a_reset_no_picture_of_a_chunk_handed_over_before_it_comes_out() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = open(setup, &options()).expect("a decoder");
        // Six chunks handed over and none of their frames read, some still with the feeder.
        let before: Vec<_> = chunks(after_start_codes)
            .into_iter()
            .take(6)
            .map(|chunk| decoder.decode(chunk))
            .collect();
        decoder.reset();
        let times = times_decoded(&decoder, chunks(after_start_codes));
        assert_eq!(
            times,
            in_order(),
            "only the pictures handed over after the reset"
        );
        for decoded in before {
            block_on(decoded).expect("a chunk dropped by a reset is answered as taken");
        }
        assert_eq!(decoder.output.waiting(), 0);
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn a_decoder_the_app_lets_go_of_ends_its_frames() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = open(setup, &options()).expect("a decoder");
        let frames = decoder.next_frame();
        drop(decoder);
        assert!(block_on(frames).is_none());
    }
}
