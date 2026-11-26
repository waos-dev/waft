//! A decoder the app holds: a stream's chunks in, its pictures out in the order they are shown.
//!
//! Each decoder decodes on a thread of its own, one job after another, so the app's thread never
//! waits on Media Foundation and pictures come in the order their chunks did. The thread starts
//! COM and Media Foundation for itself, and its work (`Work`) is the stream as far as it is known
//! (`waft-video-codecs`' reader of its codec), the parameter sets it gave ahead, the decoder, and
//! each chunk handed over and not yet out as a picture. The decoder is made as the stream opens,
//! which settles where it runs, and told its types once the stream's description is whole: from
//! the setup, or from the chunk that completes it.
//!
//! Media Foundation's decoder orders pictures itself, but carries a number, not the app's time,
//! from a sample to its picture: each chunk is handed over numbered, and its picture is given the
//! time and length its chunk carried, whatever order the times are in.
//!
//! A reset drops what the decoder holds at the moment the app asks, though jobs handed over before
//! it may still wait, or be running: each carries the count of resets it was handed over after,
//! and one handed over before the last reset is not decoded, or its picture not put out
//! (`output`).
//!
//! The invariant: everything in `Work` is touched only on the decoder's thread; a job's answer is
//! given once, whether or not the app still holds the decoder; and no picture of a chunk handed
//! over before a reset comes out after it.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;
use windows::Win32::Foundation::LUID;

use waft_embedder::VideoColor;
use waft_embedder::VideoFrame;
use waft_embedder::video_decoders::{
    CodecKind, DecodedFrameFuture, DecoderError, EncodedChunk, VideoDecoder, VideoDecoderFuture,
    VideoDecoderOptions, VideoStreamSetup,
};
use waft_video_codecs::stream_reader::{StreamReader, reader_for};
use windows::Win32::Media::MediaFoundation::{
    MF_VERSION, MFSTARTUP_NOSOCKET, MFShutdown, MFStartup,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

use super::annex_b::{parameter_sets, sample_of};
use super::device::VideoDevice;
use super::frame::DecodedFrame;
use super::output::Output;
use super::transform::{DecodedPicture, Transform};
use crate::answer::{Answer, answer};

/// What the app asks of the decoder's thread, each with the count of resets it was asked after.
enum Job {
    Decode(EncodedChunk, u64, Answer<Result<(), DecoderError>>),
    Drain(u64, Answer<Result<(), DecoderError>>),
    Configure(VideoStreamSetup, Answer<Result<(), DecoderError>>),
    /// Drop what the decoder holds.
    Reset,
}

/// What a decoder's thread works on.
struct Work {
    device: VideoDevice,
    transform: Transform,
    /// The stream, as far as it has come.
    stream: Box<dyn StreamReader>,
    /// H.264's or HEVC's parameter sets the stream held as its setup came, put into the next key
    /// frame.
    sets_ahead: Vec<Vec<u8>>,
    /// Whether the next chunk must be a key frame: at the stream's start, and after a drain, a
    /// reset, a new setup and a damaged chunk.
    key_frame_due: bool,
    /// The number the next chunk is handed over with.
    next_number: i64,
    /// The time and length of each chunk handed over and not yet out as a picture, by its number.
    chunks: HashMap<i64, (i64, Option<u64>)>,
    /// The colour the app gave for what the stream leaves unsaid.
    default_color: Option<VideoColor>,
    output: Output,
    /// How many frames may wait to be read before no more is decoded.
    frame_limit: usize,
}

impl Work {
    /// Decodes `chunk` once the app has read enough of the frames put out, and puts out the
    /// pictures the decoder gave meanwhile.
    fn decode(&mut self, chunk: EncodedChunk, resets: u64) -> Result<(), DecoderError> {
        self.output.wait_for_room(self.frame_limit);
        if self.output.resets() != resets {
            return Ok(());
        }
        let read = self.stream.read(&chunk.data)?;
        if read.changed || !self.transform.is_streaming() {
            self.adopt_description()?;
        }
        let key_frame = chunk.key_frame || read.key_frame;
        if self.key_frame_due && !key_frame {
            return Err(DecoderError::BadData(
                "a key frame is due, and this chunk is none".to_owned(),
            ));
        }
        let sets = if key_frame {
            std::mem::take(&mut self.sets_ahead)
        } else {
            Vec::new()
        };
        let data = sample_of(&read, self.stream.codec(), &sets);
        let number = self.next_number;
        self.next_number += 1;
        self.chunks
            .insert(number, (chunk.timestamp, chunk.duration));
        let pictures = self
            .transform
            .decode(&data, number, key_frame, &mut self.device)?;
        self.key_frame_due = false;
        self.put_out(pictures, resets);
        Ok(())
    }

    /// Puts out every picture the decoder holds.
    fn drain(&mut self, resets: u64) -> Result<(), DecoderError> {
        let pictures = self.transform.drain(&mut self.device)?;
        self.put_out(pictures, resets);
        self.key_frame_due = true;
        Ok(())
    }

    /// Another setup from the next chunk on.
    fn configure(&mut self, setup: VideoStreamSetup) -> Result<(), DecoderError> {
        if self.stream.configure(setup)? {
            self.adopt_description()?;
        }
        self.sets_ahead = self.sets_held()?;
        self.key_frame_due = true;
        Ok(())
    }

    /// Goes on with the stream's description, where it has a whole one: the decoder is told its
    /// types the first time, and what the stream says of its pictures from then on.
    fn adopt_description(&mut self) -> Result<(), DecoderError> {
        match self.stream.description()? {
            Some(description) => self.transform.adopt(&description.facts),
            None => Ok(()),
        }
    }

    /// The parameter sets the stream holds now, where it has a whole set.
    fn sets_held(&self) -> Result<Vec<Vec<u8>>, DecoderError> {
        Ok(self
            .stream
            .description()?
            .map(|description| parameter_sets(&description.configuration))
            .unwrap_or_default())
    }

    fn reset(&mut self) {
        self.transform.flush();
        self.chunks.clear();
        self.key_frame_due = true;
    }

    fn put_out(&mut self, pictures: Vec<DecodedPicture>, resets: u64) {
        let frames: Vec<Arc<dyn VideoFrame>> = pictures
            .into_iter()
            .map(|picture| self.frame_of(picture))
            .collect();
        self.output.put(frames, resets);
    }

    fn frame_of(&mut self, picture: DecodedPicture) -> Arc<dyn VideoFrame> {
        let (timestamp, duration) = self.chunks.remove(&picture.sample).unwrap_or((0, None));
        Arc::new(DecodedFrame {
            texture: picture.texture,
            timestamp,
            duration: duration.map(Duration::from_nanos),
            visible: picture.visible,
            pixel_aspect: picture.pixel_aspect,
            color: self.colour_of(picture.color),
        })
    }

    /// The stream's colour, with the app's for each value the stream leaves unsaid.
    fn colour_of(&self, said: VideoColor) -> VideoColor {
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

    /// The decoder ended by itself where `result` says it cannot go on: the app is told why
    /// beside its frames. A damaged chunk leaves it taking chunks from the next key frame.
    fn after(&mut self, result: &Result<(), DecoderError>) {
        match result {
            Err(DecoderError::BadData(_)) => self.key_frame_due = true,
            Err(reason @ (DecoderError::Lost | DecoderError::Failed(_))) => {
                self.output.end(reason.clone());
            }
            _ => {}
        }
    }
}

/// A Media Foundation decoder as the app holds it.
pub(super) struct Decoder {
    jobs: Sender<Job>,
    output: Output,
    kind: CodecKind,
}

impl Decoder {
    /// A decoder for the stream `setup` begins, made on a thread of its own.
    pub(super) fn open(
        setup: VideoStreamSetup,
        options: &VideoDecoderOptions,
        adapter: Option<LUID>,
    ) -> Result<Decoder, DecoderError> {
        let stream = reader_for(setup)?;
        let codec = stream.codec();
        let output = Output::default();
        let (jobs, arriving) = channel();
        let (opened, opening) = channel();
        let options = options.clone();
        let thread_output = output.clone();
        std::thread::Builder::new()
            .name("waft video decoder".to_owned())
            .spawn(move || {
                let _started = MediaFoundation::start();
                let work = VideoDevice::new(adapter).and_then(|device| {
                    let transform =
                        Transform::open(&device, codec, options.decoder, options.purpose)?;
                    let mut work = Work {
                        device,
                        transform,
                        stream,
                        sets_ahead: Vec::new(),
                        key_frame_due: true,
                        next_number: 0,
                        chunks: HashMap::new(),
                        default_color: options.default_color,
                        output: thread_output,
                        frame_limit: options.frame_limit.max(1) as usize,
                    };
                    work.adopt_description()?;
                    work.sets_ahead = work.sets_held()?;
                    Ok(work)
                });
                match work {
                    Ok(work) => {
                        let _ = opened.send(Ok(work.transform.kind()));
                        run(work, arriving);
                    }
                    Err(error) => {
                        let _ = opened.send(Err(error));
                    }
                }
            })
            .map_err(|error| DecoderError::Failed(error.to_string()))?;
        let kind = opening
            .recv()
            .map_err(|_| DecoderError::Failed("the decoder's thread ended".to_owned()))??;
        Ok(Decoder { jobs, output, kind })
    }

    /// Hands `job` to the decoder's thread, made with an answer that says the decoder went away
    /// if the thread never gives one.
    fn ask(&self, job: impl FnOnce(Answer<Result<(), DecoderError>>) -> Job) -> VideoDecoderFuture {
        let gone = Err(DecoderError::Failed("the decoder went away".to_owned()));
        let (answer, answered) = answer(gone);
        // A thread that has ended drops the job, and its answer says so.
        let _ = self.jobs.send(job(answer));
        Box::pin(answered)
    }
}

/// Does the jobs that arrive until the app lets go of the decoder.
fn run(mut work: Work, arriving: Receiver<Job>) {
    for job in arriving {
        match job {
            Job::Decode(chunk, resets, answer) => {
                let result = work.decode(chunk, resets);
                work.after(&result);
                answer.give(result);
            }
            Job::Drain(resets, answer) => {
                let result = work.drain(resets);
                work.after(&result);
                answer.give(result);
            }
            Job::Configure(setup, answer) => {
                let result = work.configure(setup);
                answer.give(result);
            }
            Job::Reset => work.reset(),
        }
    }
}

impl VideoDecoder for Decoder {
    fn kind(&self) -> CodecKind {
        self.kind
    }

    /// The chunk waits its turn on the thread, and then for room among the frames waiting to be
    /// read. One the app reset the decoder after is dropped undecoded, as the reset asked.
    fn decode(&self, chunk: EncodedChunk) -> VideoDecoderFuture {
        let resets = self.output.resets();
        self.ask(|answer| Job::Decode(chunk, resets, answer))
    }

    fn next_frame(&self) -> DecodedFrameFuture {
        Box::pin(self.output.next_frame())
    }

    fn ended(&self) -> Pin<Box<dyn Future<Output = DecoderError>>> {
        Box::pin(self.output.ended())
    }

    fn drain(&self) -> VideoDecoderFuture {
        let resets = self.output.resets();
        self.ask(|answer| Job::Drain(resets, answer))
    }

    /// The frames waiting to be read are dropped at once, and the chunks still waiting as their
    /// turn comes; what the decoder holds goes once the thread reaches this.
    fn reset(&self) {
        self.output.reset();
        let _ = self.jobs.send(Job::Reset);
    }

    fn configure(&self, setup: VideoStreamSetup) -> VideoDecoderFuture {
        self.ask(|answer| Job::Configure(setup, answer))
    }
}

impl Drop for Decoder {
    /// The frames end; the thread ends once it has done the jobs it was handed.
    fn drop(&mut self) {
        self.output.close();
    }
}

/// COM and Media Foundation started on a thread, the decoder's or the app's as it asks what the
/// system decodes, and shut down as it ends. A thread whose COM apartment is already of another
/// kind keeps it, and COM is then not undone here.
pub(super) struct MediaFoundation {
    /// Whether COM was started here, and so is to be undone.
    com_started: bool,
}

impl MediaFoundation {
    pub(super) fn start() -> MediaFoundation {
        // SAFETY: the thread's own COM apartment and Media Foundation's count of starts, each
        // undone once by the drop below, COM only where it was started here.
        unsafe {
            let com_started = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let _ = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET);
            MediaFoundation { com_started }
        }
    }
}

impl Drop for MediaFoundation {
    fn drop(&mut self) {
        // SAFETY: undoes the starts above, on the same thread.
        unsafe {
            let _ = MFShutdown();
            if self.com_started {
                CoUninitialize();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pollster::block_on;
    use waft_embedder::video_decoders::{CodecChoice, DecodingPurpose, NalFormat};

    use super::super::tests::{after_start_codes, chunks, options};
    use super::*;

    #[test]
    fn no_more_is_decoded_while_as_many_frames_as_the_app_holds_wait_to_be_read() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        // Live, as for playback the decoder holds this clip's every picture until it drains.
        let held_at_once = VideoDecoderOptions {
            frame_limit: 2,
            purpose: DecodingPurpose::Live,
            ..options(CodecChoice::Any)
        };
        let decoder = Decoder::open(setup, &held_at_once, None).expect("a decoder");
        let decoding: Vec<_> = chunks(after_start_codes)
            .into_iter()
            .map(|chunk| decoder.decode(chunk))
            .collect();
        // The thread decodes until two frames wait, however long that takes on this machine, then
        // stops: nothing reads them.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while decoder.output.waiting() < 2 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(decoder.output.waiting(), 2);
        drop(decoder);
        // Let go of, the decoder decodes on to no one, and every chunk is answered.
        for decoded in decoding {
            let _ = block_on(decoded);
        }
    }
}
