//! A decoder the app holds: a stream's chunks in, its pictures out in the order they are shown.
//!
//! Each decoder decodes on a thread of its own, which makes its codec, hands it one chunk after
//! another and lets its pictures out, so the app's thread never waits on MediaCodec. What the
//! thread works on (`Work`) is the stream as far as it is known: how it frames its units, its
//! parameter sets, the codec made of them, and whether a key frame is due. While the codec holds
//! chunks whose pictures have not come out, the thread looks for them between the app's asks, as
//! Android's own players poll their codecs.
//!
//! A codec is made once a whole set of parameter sets is known. It goes on with new ones, in the
//! chunks or from `configure`, while their pictures fit the size it was made to grow to, and is
//! made again otherwise, the pictures it holds let out first, as they come before the change.
//! MediaCodec's pool holds back a codec whose pictures are not let out, so the decoder holds itself
//! back: it lets out no more while as many frames as the app said it holds at once wait to be read,
//! and a chunk then waits for the codec to take it. It renders one picture at a time, the next once
//! the last has reached the image stream: a codec's surface drops a picture rendered before the
//! last was taken, as Android's own frame extractors wait for each.
//!
//! A reset drops what the decoder holds at the moment the app asks, though chunks handed over
//! before it may still wait for the thread: each carries the count of resets it was handed over
//! after, and one handed over before the last reset is not decoded, or its picture not put out
//! (`output`).
//!
//! The invariant: `Work` and its codec live on the decoder's thread alone; a chunk's answer is
//! given once, whether or not the app still holds the decoder; and no picture of a chunk handed
//! over before a reset comes out after it.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use waft_embedder::video_decoders::{
    CodecChoice, CodecKind, DecodedFrameFuture, DecoderError, EncodedChunk, VideoDecoder,
    VideoDecoderFuture, VideoDecoderOptions, VideoStreamSetup,
};
use waft_embedder::{HardwareBuffer, PixelAspect, PixelRect, VideoColor, VideoFrame};
use waft_video_codecs::StreamFacts;
use waft_video_codecs::framing::{Framing, with_start_codes};
use waft_video_codecs::h264::{self, ParameterSets};

use super::codec::{Codec, CodecOptions, Coming, Input};
use super::output::Output;
use super::pending::{ChunkFacts, Pending};
use crate::answer::answer;
use crate::hardware_buffers::PrivatePicture;
use crate::media::frame_conversion::to_rgba;
use crate::media::image_stream::StreamImage;
use crate::media::{AndroidVideoFrame, DecodedFacts};

/// How long the thread waits on the codec, or for the app's next ask while the codec holds
/// chunks, before it looks again.
const POLL: Duration = Duration::from_millis(10);

/// A stream's setup as the decoder takes it: its framing, and the parameter sets given ahead.
fn framing_and_sets(setup: VideoStreamSetup) -> Result<(Framing, ParameterSets), DecoderError> {
    match setup {
        VideoStreamSetup::H264(format) => h264::setup(format).map_err(DecoderError::BadData),
        other => Err(DecoderError::Unsupported(format!(
            "this system's decoders are handed H.264 alone so far, not {:?}",
            other.codec()
        ))),
    }
}

/// Whether the codec is let out pictures only while the app has room for their frames, or every
/// one, as a drain lets them out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Room {
    AsTheAppHolds,
    Unlimited,
}

/// What a decoder's thread works on.
struct Work {
    framing: Framing,
    /// The stream's parameter sets, as far as they have come.
    sets: ParameterSets,
    options: CodecOptions,
    /// The colour the app gave for what the stream leaves unsaid.
    default_color: Option<VideoColor>,
    /// The codec made of `sets`, once they are whole, with what they say of the stream.
    codec: Option<(Codec, StreamFacts)>,
    /// Whether the next chunk must decode with nothing before it.
    key_due: bool,
    /// The number the next chunk is handed to the codec with.
    next_number: u64,
    pending: Arc<Pending>,
    output: Output,
    /// How many frames may wait to be read before no more are let out: as many as the app said
    /// it holds at once.
    frame_limit: usize,
    /// How many resets what the thread is doing now was asked after.
    asked_after_resets: u64,
    /// Why the decoder ended by itself, once it did.
    ended: Option<DecoderError>,
}

impl Work {
    /// The work of a decoder made with `options`, its frames put out to `output`, before its
    /// stream's setup is taken.
    fn new(options: &VideoDecoderOptions, output: Output) -> Work {
        Work {
            framing: Framing::StartCodes,
            sets: ParameterSets::default(),
            options: CodecOptions {
                choice: options.decoder,
                purpose: options.purpose,
                largest_size: options.largest_size,
            },
            default_color: options.default_color,
            codec: None,
            key_due: true,
            next_number: 0,
            pending: Arc::default(),
            output,
            frame_limit: options.frame_limit.max(1) as usize,
            asked_after_resets: 0,
            ended: None,
        }
    }

    /// Goes on with `sets`: with the codec there is, where their pictures fit it, and with one
    /// made of them otherwise, the pictures held let out first. Answers whether the codec went
    /// on, and so is to be handed the sets.
    fn adopt(&mut self, sets: ParameterSets) -> Result<bool, DecoderError> {
        let facts = sets.facts().map_err(DecoderError::BadData)?;
        let size = facts.size.ok_or_else(|| {
            DecoderError::BadData("the sequence parameter set's size cannot be read".to_owned())
        })?;
        let goes_on = matches!(&self.codec, Some((codec, _)) if codec.fits(size));
        if goes_on {
            self.codec = self.codec.take().map(|(codec, _)| (codec, facts));
        } else {
            self.end_stream()?;
            self.codec = None;
            self.pending.clear();
            let codec = Codec::open(&sets, size, self.options, self.delivery())?;
            self.codec = Some((codec, facts));
            self.key_due = true;
        }
        self.sets = sets;
        Ok(goes_on)
    }

    /// Hands `chunk` to the codec, once it takes it, taking the parameter sets the chunk carries.
    fn decode(&mut self, chunk: &EncodedChunk) -> Result<(), DecoderError> {
        let units = self
            .framing
            .units(&chunk.data)
            .map_err(DecoderError::BadData)?;
        let sets = self.sets.replaced_by(ParameterSets::among(&units));
        if sets.are_complete() && (sets != self.sets || self.codec.is_none()) {
            self.adopt(sets)?;
        }
        let Some((_, facts)) = &self.codec else {
            return Err(DecoderError::BadData(
                "the stream's parameter sets have not come".to_owned(),
            ));
        };
        if self.key_due && !(chunk.key_frame || h264::is_key_frame(&units)) {
            return Err(DecoderError::BadData(
                "a key frame is due, and the chunk is none".to_owned(),
            ));
        }
        let facts = self.chunk_facts(chunk, facts);
        let number = self.next_number;
        if self.feed(
            &with_start_codes(units),
            Input::Chunk(number),
            Room::AsTheAppHolds,
        )? {
            self.next_number += 1;
            self.pending.handed(number, facts);
            self.key_due = false;
        }
        Ok(())
    }

    /// Lets out every picture the codec holds, and makes it ready for the stream's next key
    /// frame: all that was decoded has then come out.
    fn drain(&mut self) -> Result<(), DecoderError> {
        self.end_stream()?;
        self.restart()
    }

    /// Another setup from the next chunk on, which decodes from a key frame.
    fn configure(&mut self, setup: VideoStreamSetup) -> Result<(), DecoderError> {
        let (framing, sets) = framing_and_sets(setup)?;
        self.framing = framing;
        if sets.are_complete() && sets != self.sets && self.adopt(sets)? {
            self.hand_sets()?;
        }
        self.key_due = true;
        Ok(())
    }

    /// Drops every chunk and picture the codec holds, as for a seek.
    fn reset(&mut self) -> Result<(), DecoderError> {
        self.restart()
    }

    /// Lets the codec end its stream and waits until every picture it rendered was delivered.
    fn end_stream(&self) -> Result<(), DecoderError> {
        if self.codec.is_none() {
            return Ok(());
        }
        if !self.feed(&[], Input::EndOfStream, Room::Unlimited)? {
            return Ok(());
        }
        while self.let_out(Room::Unlimited, POLL)? != Coming::EndOfStream {
            if self.output.is_closed() {
                return Ok(());
            }
        }
        while !self.pending.wait_until_delivered(POLL) {
            if self.output.is_closed() {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Flushes the codec, handed the stream's sets again, so that it takes chunks from a key
    /// frame on.
    fn restart(&mut self) -> Result<(), DecoderError> {
        let Some((codec, _)) = &self.codec else {
            return Ok(());
        };
        codec.flush()?;
        self.pending.clear();
        self.key_due = true;
        self.hand_sets()
    }

    /// Hands the codec the stream's parameter sets, ahead of the chunks that use them.
    fn hand_sets(&self) -> Result<(), DecoderError> {
        let data = with_start_codes(self.sets.units());
        self.feed(&data, Input::ParameterSets, Room::Unlimited)?;
        Ok(())
    }

    /// Hands `data` to the codec, letting its pictures out while it takes none, as `room`
    /// allows; answers whether it was taken, and not given up on as the app let go of the
    /// decoder or reset it.
    fn feed(&self, data: &[u8], input: Input, room: Room) -> Result<bool, DecoderError> {
        loop {
            let Some((codec, _)) = &self.codec else {
                return Ok(false);
            };
            if codec.feed(data, input, POLL)? {
                return Ok(true);
            }
            if self.output.is_closed() || self.output.resets() != self.asked_after_resets {
                return Ok(false);
            }
            self.let_out(room, Duration::ZERO)?;
            if !self.has_room() && room == Room::AsTheAppHolds {
                self.output.wait_for_room(self.frame_limit, POLL);
            }
        }
    }

    /// Lets out the pictures the codec holds, as `room` allows, waiting up to `patience` for
    /// the first; answers what came last.
    fn let_out(&self, room: Room, patience: Duration) -> Result<Coming, DecoderError> {
        let Some((codec, _)) = &self.codec else {
            return Ok(Coming::Nothing);
        };
        let mut patience = patience;
        loop {
            if room == Room::AsTheAppHolds && !self.has_room() {
                return Ok(Coming::Nothing);
            }
            // A picture rendered before the last was taken from the stream takes its place, as
            // a codec's surface drops pictures, so the next is rendered once the last arrived.
            if !self.pending.wait_until_delivered(patience) {
                return Ok(Coming::Nothing);
            }
            let coming = codec.let_out(patience, |number| self.pending.rendered(number))?;
            if !matches!(coming, Coming::Picture { .. }) {
                return Ok(coming);
            }
            patience = Duration::ZERO;
        }
    }

    /// Whether the app has room for another frame: fewer than it holds at once wait to be read
    /// or are on their way, or it let go of the decoder.
    fn has_room(&self) -> bool {
        self.output.is_closed()
            || self.output.waiting() + self.pending.rendered_count() < self.frame_limit
    }

    /// Whether the codec holds chunks whose pictures have not come out.
    fn awaits_pictures(&self) -> bool {
        self.codec.is_some() && self.ended.is_none() && self.pending.awaits_pictures()
    }

    /// The decoder ended by itself where `result` says it cannot go on: its codec goes, and the
    /// app is told why beside its frames. A chunk it could not decode leaves a key frame due.
    fn after(&mut self, result: &Result<(), DecoderError>) {
        match result {
            Err(DecoderError::BadData(_)) => self.key_due = true,
            Err(reason @ (DecoderError::Lost | DecoderError::Failed(_))) => {
                self.codec = None;
                self.pending.clear();
                self.ended = Some(reason.clone());
                self.output.end(reason.clone());
            }
            _ => {}
        }
    }

    fn chunk_facts(&self, chunk: &EncodedChunk, facts: &StreamFacts) -> ChunkFacts {
        ChunkFacts {
            timestamp: chunk.timestamp,
            duration: chunk.duration.map(Duration::from_nanos),
            resets: self.asked_after_resets,
            pixel_aspect: facts.pixel_aspect.unwrap_or(PixelAspect {
                width: 1,
                height: 1,
            }),
            color: self.colour_of(facts),
        }
    }

    /// The stream's colour, with the app's for each value the stream leaves unsaid.
    fn colour_of(&self, facts: &StreamFacts) -> VideoColor {
        let said = facts.color;
        let Some(default) = self.default_color else {
            return said;
        };
        VideoColor {
            primaries: said.primaries.or(default.primaries),
            transfer: said.transfer.or(default.transfer),
            matrix: said.matrix.or(default.matrix),
            range: said.range.or(default.range),
            chroma_location: said.chroma_location.or(default.chroma_location),
        }
    }

    /// What a codec does with each picture its image stream delivers: draws it into a frame, put
    /// out where its chunk is still pending.
    fn delivery(&self) -> impl Fn(StreamImage) + Send + 'static {
        let pending = Arc::clone(&self.pending);
        let output = self.output.clone();
        move |image| {
            let Some(chunk) = number_of(&image).and_then(|number| pending.take(number)) else {
                return;
            };
            match frame_of(&image, &chunk) {
                Ok(frame) => output.put(frame, chunk.resets),
                Err(error) => crate::log::warn_to_log!("waft: a decoded picture is lost: {error}"),
            }
            pending.delivered();
        }
    }
}

/// The number of the chunk `image` is of, as the codec carried it.
fn number_of(image: &StreamImage) -> Option<u64> {
    let nanoseconds = image.timestamp().ok()?;
    u64::try_from((nanoseconds + 500) / 1000).ok()
}

/// `image` drawn into a frame of its own, with what its chunk says of it.
fn frame_of(image: &StreamImage, chunk: &ChunkFacts) -> Result<Arc<dyn VideoFrame>, String> {
    let picture = image
        .hardware_buffer()
        .map_err(|error| format!("no buffer of the picture: {error}"))?;
    let frame = to_rgba(PrivatePicture::new(picture))?;
    let visible_rect = image.visible_rect().unwrap_or(PixelRect {
        x: 0,
        y: 0,
        width: frame.width(),
        height: frame.height(),
    });
    Ok(Arc::new(AndroidVideoFrame::decoded(
        frame,
        DecodedFacts {
            timestamp: chunk.timestamp,
            duration: chunk.duration,
            visible_rect,
            pixel_aspect: chunk.pixel_aspect,
            color: chunk.color,
        },
    )))
}

/// One thing the decoder's thread does, as the app asked it.
type Job = Box<dyn FnOnce(&mut Work) + Send>;

/// A MediaCodec decoder as the app holds it.
pub(super) struct Decoder {
    jobs: Sender<Job>,
    output: Output,
    kind: CodecKind,
}

impl Decoder {
    /// A decoder for the stream `setup` begins, its codec made at once, on its thread, where the
    /// setup holds the stream's parameter sets, and with the first key frame otherwise.
    pub(super) fn open(
        setup: VideoStreamSetup,
        options: &VideoDecoderOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Decoder, DecoderError>>>> {
        let output = Output::default();
        let (jobs, asked) = mpsc::channel();
        let gone = Err(DecoderError::Failed(
            "the decoder's thread went away".to_owned(),
        ));
        let (opened, answered) = answer(gone);
        let thread_output = output.clone();
        let options = options.clone();
        let started = std::thread::Builder::new()
            .name("waft video decoder".to_owned())
            .spawn(move || {
                let mut work = Work::new(&options, thread_output);
                let begun = begin(&mut work, setup);
                let went_on = begun.is_ok();
                opened.give(begun);
                if went_on {
                    run(work, asked);
                }
            });
        if let Err(error) = started {
            let refused = Err(DecoderError::Failed(format!(
                "no thread to decode on: {error}"
            )));
            return Box::pin(std::future::ready(refused));
        }
        Box::pin(async move {
            let kind = answered.await?;
            Ok(Decoder { jobs, output, kind })
        })
    }

    /// Does `job` with the decoder's work, on its thread, and answers what it answered.
    fn on_thread(
        &self,
        job: impl FnOnce(&mut Work) -> Result<(), DecoderError> + Send + 'static,
    ) -> VideoDecoderFuture {
        let gone = Err(DecoderError::Failed("the decoder went away".to_owned()));
        let (answer, answered) = answer(gone);
        let asked_after_resets = self.output.resets();
        let _ = self.jobs.send(Box::new(move |work: &mut Work| {
            work.asked_after_resets = asked_after_resets;
            let result = match &work.ended {
                Some(reason) => Err(reason.clone()),
                None => job(work),
            };
            work.after(&result);
            answer.give(result);
        }));
        Box::pin(answered)
    }
}

/// Takes the stream's setup on the decoder's thread, its codec made where the setup holds its
/// parameter sets; answers where the codec runs, or why there is none.
fn begin(work: &mut Work, setup: VideoStreamSetup) -> Result<CodecKind, DecoderError> {
    let (framing, sets) = framing_and_sets(setup)?;
    work.framing = framing;
    if sets.are_complete() {
        work.adopt(sets)?;
    }
    Ok(match &work.codec {
        Some((codec, _)) => codec.kind(),
        None => kind_before_a_codec(work.options.choice),
    })
}

/// The decoder's thread: each job the app asks, in turn, and the codec's pictures let out between
/// them, until the app lets go of the decoder.
fn run(mut work: Work, asked: Receiver<Job>) {
    loop {
        let job = if work.awaits_pictures() {
            match asked.recv_timeout(POLL) {
                Ok(job) => Some(job),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match asked.recv() {
                Ok(job) => Some(job),
                Err(_) => return,
            }
        };
        if let Some(job) = job {
            job(&mut work);
        }
        let result = work
            .let_out(Room::AsTheAppHolds, Duration::ZERO)
            .map(|_| ());
        work.after(&result);
    }
}

impl VideoDecoder for Decoder {
    fn kind(&self) -> CodecKind {
        self.kind
    }

    /// The chunk waits its turn on the thread, and then for the codec to take it, which waits
    /// for room among the frames waiting to be read. One the app reset the decoder after is
    /// dropped undecoded, as the reset asked.
    fn decode(&self, chunk: EncodedChunk) -> VideoDecoderFuture {
        self.on_thread(move |work| {
            if work.output.resets() != work.asked_after_resets || work.output.is_closed() {
                return Ok(());
            }
            work.decode(&chunk)
        })
    }

    fn next_frame(&self) -> DecodedFrameFuture {
        Box::pin(self.output.next_frame())
    }

    fn ended(&self) -> Pin<Box<dyn Future<Output = DecoderError>>> {
        Box::pin(self.output.ended())
    }

    fn drain(&self) -> VideoDecoderFuture {
        self.on_thread(|work| work.drain())
    }

    /// The frames waiting to be read are dropped at once, and the chunks still waiting for the
    /// thread as their turn comes; the codec is flushed once the thread reaches this.
    fn reset(&self) {
        self.output.reset();
        let asked_after_resets = self.output.resets();
        let _ = self.jobs.send(Box::new(move |work: &mut Work| {
            work.asked_after_resets = asked_after_resets;
            let result = if work.ended.is_none() {
                work.reset()
            } else {
                Ok(())
            };
            work.after(&result);
        }));
    }

    fn configure(&self, setup: VideoStreamSetup) -> VideoDecoderFuture {
        self.on_thread(move |work| work.configure(setup))
    }
}

impl Drop for Decoder {
    /// The frames end; the codec goes once the thread has done what it was asked.
    fn drop(&mut self) {
        self.output.close();
    }
}

/// Where a decoder with no codec yet is taken to run: as its choice asks, hardware first.
fn kind_before_a_codec(choice: CodecChoice) -> CodecKind {
    match choice {
        CodecChoice::Any | CodecChoice::Hardware => CodecKind::Hardware,
        CodecChoice::Software => CodecKind::Software,
    }
}
