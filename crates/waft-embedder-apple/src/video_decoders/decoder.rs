//! A decoder the app holds: a stream's chunks in, its pictures out in the order they are shown.
//!
//! Each decoder decodes on a queue of its own, one chunk after another, so the app's thread
//! never waits on VideoToolbox and pictures are decoded in the order their chunks came. What
//! the queue works on (`Work`) is the stream as far as it is known (`waft-video-codecs`' reader
//! of its codec), the session made for it, and the pictures decoded and not yet shown. Each chunk
//! reaches VideoToolbox as a sample: H.264's and HEVC's units each after four bytes of length,
//! their parameter sets left to the format, and a VP9 or AV1 chunk whole.
//!
//! What a session is made for (a stream's format: H.264's and HEVC's parameter sets, VP9's
//! record and the size its key frame says, AV1's sequence header) may come ahead, in its setup,
//! or in its chunks, and may change. A session is made once the stream's reader has a whole
//! description, and made again when VideoToolbox does not go on with a new one; the pictures
//! still held then are shown first, as they come before the change. VideoToolbox's pool grows as
//! frames are held, so the decoder holds itself back: it decodes no more while as many frames as
//! the app said it holds at once wait to be read.
//!
//! A reset drops what the decoder holds at the moment the app asks, though chunks handed over
//! before it may still wait on the queue, or be decoding: each carries the count of resets it
//! was handed over after, and one handed over before the last reset is not decoded, or its
//! picture not put out (`output`).
//!
//! The invariant: everything in `Work` is touched only on the decoder's queue; a chunk's answer
//! is given once, whether or not the app still holds the decoder; and no picture of a chunk
//! handed over before a reset comes out after it.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use waft_embedder::video_decoders::{
    CodecChoice, CodecKind, DecodedFrameFuture, DecoderError, EncodedChunk, VideoCodec,
    VideoDecoder, VideoDecoderFuture, VideoDecoderOptions, VideoStreamSetup,
};
use waft_embedder::{ColorRange, PixelAspect, VideoColor, VideoFrame};

use waft_video_codecs::StreamFacts;
use waft_video_codecs::framing::with_lengths;
use waft_video_codecs::presentation_order::PresentationOrder;

use waft_video_codecs::stream_reader::{Description, ReadChunk, StreamReader, reader_for};

use super::format::Format;
use super::output::Output;
use super::session::{DecodedPicture, PictureFormat, Session, add_vp9_decoder};
use crate::answer::answer;
use crate::media::CoreVideoFrame;
use crate::media::DecodedFacts;

/// What a decoder's queue works on.
struct Work {
    /// The stream, as far as it has come.
    stream: Box<dyn StreamReader>,
    choice: CodecChoice,
    /// The colour the app gave for what the stream leaves unsaid.
    default_color: Option<VideoColor>,
    /// The session made for the stream's description, once it is whole, with what it says of
    /// the stream.
    session: Option<(Session, StreamFacts)>,
    /// The pictures decoded and not yet shown.
    order: PresentationOrder<Arc<dyn VideoFrame>>,
    output: Output,
    /// How many frames may wait to be read before no more is decoded: as many as the app said
    /// it holds at once.
    frame_limit: usize,
    /// How many resets what the queue is doing now was asked after.
    asked_after_resets: u64,
}

impl Work {
    /// Goes on with the stream's description, where it has a whole one: with the session there
    /// is, where VideoToolbox does, and with one made for it otherwise, the pictures held shown
    /// first.
    fn adopt_description(&mut self) -> Result<(), DecoderError> {
        let Some(Description {
            configuration,
            facts,
        }) = self.stream.description()?
        else {
            return Ok(());
        };
        let format = Format::of(&configuration)?;
        let goes_on = matches!(&self.session, Some((session, _)) if session.accepts(&format));
        let session = match self.session.take() {
            Some((session, _)) if goes_on => session,
            _ => {
                self.output.put(self.order.all(), self.asked_after_resets);
                let pictures = PictureFormat {
                    bit_depth: facts.bit_depth,
                    full_range: self.colour_of(&facts).range == Some(ColorRange::Full),
                };
                Session::open(&format, self.choice, pictures)?
            }
        };
        self.order.set_depth(facts.reorder_depth);
        self.session = Some((session, facts));
        Ok(())
    }

    /// Decodes `chunk`, once the app has read enough of the frames put out, taking what it says
    /// of its stream, and puts out the pictures that can be shown now.
    fn decode(&mut self, chunk: &EncodedChunk) -> Result<(), DecoderError> {
        let read = self.stream.read(&chunk.data)?;
        let place = self.stream.place_of(&read)?;
        let sample = sample_of(&read, self.stream.codec());
        if read.changed || self.session.is_none() {
            self.adopt_description()?;
        }
        let Some((session, facts)) = &self.session else {
            return Err(DecoderError::BadData(
                "what the stream's format is made of has not come".to_owned(),
            ));
        };
        let decoded = session.decode(&sample, chunk.timestamp, chunk.duration)?;
        if let Some(picture) = decoded {
            let place = place.ok_or_else(|| {
                DecoderError::BadData("a picture with nothing to tell its order by".to_owned())
            })?;
            let frame = self.frame_of(picture, chunk, facts);
            let shown = self.order.take(place, frame);
            self.output.put(shown, self.asked_after_resets);
        }
        Ok(())
    }

    /// Shows every picture held: all that was decoded has then come out.
    fn drain(&mut self) {
        self.output.put(self.order.all(), self.asked_after_resets);
    }

    /// Another setup from the next chunk on.
    fn configure(&mut self, setup: VideoStreamSetup) -> Result<(), DecoderError> {
        if self.stream.configure(setup)? {
            self.adopt_description()?;
        }
        Ok(())
    }

    /// The decoder ended by itself where `result` says it cannot go on: its session goes, and
    /// the app is told why beside its frames.
    fn end_if_it_cannot_go_on(&mut self, result: &Result<(), DecoderError>) {
        if let Err(reason @ (DecoderError::Lost | DecoderError::Failed(_))) = result {
            self.session = None;
            self.output.end(reason.clone());
        }
    }

    fn frame_of(
        &self,
        picture: DecodedPicture,
        chunk: &EncodedChunk,
        facts: &StreamFacts,
    ) -> Arc<dyn VideoFrame> {
        Arc::new(CoreVideoFrame::decoded(
            picture.0,
            DecodedFacts {
                timestamp: chunk.timestamp,
                duration: chunk.duration.map(Duration::from_nanos),
                pixel_aspect: facts.pixel_aspect.unwrap_or(PixelAspect {
                    width: 1,
                    height: 1,
                }),
                color: self.colour_of(facts),
            },
        ))
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
}

/// A VideoToolbox decoder as the app holds it.
pub(super) struct Decoder {
    queue: DispatchRetained<DispatchQueue>,
    work: Arc<Mutex<Work>>,
    output: Output,
    kind: CodecKind,
}

impl Decoder {
    /// A decoder for the stream `setup` begins, its session made at once where the setup says
    /// what the stream's format is made of, and with the first key frame otherwise.
    pub(super) fn open(
        setup: VideoStreamSetup,
        options: &VideoDecoderOptions,
    ) -> Result<Decoder, DecoderError> {
        let stream = match setup.codec() {
            VideoCodec::Vp8 => {
                return Err(DecoderError::Unsupported(
                    "VideoToolbox has no VP8 decoder".to_owned(),
                ));
            }
            VideoCodec::Vp9 => {
                add_vp9_decoder();
                reader_for(setup)?
            }
            _ => reader_for(setup)?,
        };
        let output = Output::default();
        let mut work = Work {
            stream,
            choice: options.decoder,
            default_color: options.default_color,
            session: None,
            order: PresentationOrder::new(0),
            output: output.clone(),
            frame_limit: options.frame_limit.max(1) as usize,
            asked_after_resets: 0,
        };
        work.adopt_description()?;
        let kind = match &work.session {
            Some((session, _)) => session.kind(),
            None => kind_before_a_session(options.decoder),
        };
        Ok(Decoder {
            queue: DispatchQueue::new("rs.waft.video-decoder", DispatchQueueAttr::SERIAL),
            work: Arc::new(Mutex::new(work)),
            output,
            kind,
        })
    }

    /// Does `job` with the decoder's work, on its queue, and answers what it answered. What it
    /// puts out is dropped where the app resets the decoder meanwhile.
    fn on_queue(
        &self,
        job: impl FnOnce(&mut Work) -> Result<(), DecoderError> + Send + 'static,
    ) -> VideoDecoderFuture {
        let gone = Err(DecoderError::Failed("the decoder went away".to_owned()));
        let (answer, answered) = answer(gone);
        let work = Arc::clone(&self.work);
        let asked_after_resets = self.output.resets();
        self.queue.exec_async(move || {
            let mut work = work.lock().unwrap_or_else(PoisonError::into_inner);
            work.asked_after_resets = asked_after_resets;
            let result = job(&mut work);
            work.end_if_it_cannot_go_on(&result);
            answer.give(result);
        });
        Box::pin(answered)
    }
}

impl VideoDecoder for Decoder {
    fn kind(&self) -> CodecKind {
        self.kind
    }

    /// The chunk waits its turn on the queue, and then for room among the frames waiting to be
    /// read. One the app reset the decoder after is dropped undecoded, as the reset asked.
    fn decode(&self, chunk: EncodedChunk) -> VideoDecoderFuture {
        self.on_queue(move |work| {
            work.output.wait_for_room(work.frame_limit);
            if work.output.resets() != work.asked_after_resets {
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
        self.on_queue(|work| {
            work.drain();
            Ok(())
        })
    }

    /// The frames waiting to be read are dropped at once, and the chunks still on the queue as
    /// their turn comes; the pictures decoded and not yet shown go once the queue reaches this,
    /// after the chunk it may be decoding now.
    fn reset(&self) {
        self.output.reset();
        let work = Arc::clone(&self.work);
        self.queue.exec_async(move || {
            let mut work = work.lock().unwrap_or_else(PoisonError::into_inner);
            work.order.clear();
        });
    }

    fn configure(&self, setup: VideoStreamSetup) -> VideoDecoderFuture {
        self.on_queue(move |work| work.configure(setup))
    }
}

impl Drop for Decoder {
    /// The frames end; the session goes once the queue has done what it was handed.
    fn drop(&mut self) {
        self.output.close();
    }
}

/// The sample VideoToolbox decodes of `chunk`, framed as the stream's format says: H.264's and
/// HEVC's units but their parameter sets, which go to the format, each after its length, and a VP9
/// or AV1 chunk whole.
fn sample_of(chunk: &ReadChunk<'_>, codec: VideoCodec) -> Vec<u8> {
    match codec {
        VideoCodec::H264 | VideoCodec::Hevc => with_lengths(chunk.coded_units()),
        VideoCodec::Vp8 | VideoCodec::Vp9 | VideoCodec::Av1 => chunk.units().concat(),
    }
}

/// Where a decoder with no session yet is taken to run: as its choice asks, hardware first.
fn kind_before_a_session(choice: CodecChoice) -> CodecKind {
    match choice {
        CodecChoice::Any | CodecChoice::Hardware => CodecKind::Hardware,
        CodecChoice::Software => CodecKind::Software,
    }
}

#[cfg(test)]
mod tests {
    use pollster::block_on;
    use waft_embedder::PixelRect;
    use waft_embedder::video_decoders::NalFormat;

    use super::super::test_decoding::{
        after_start_codes, chunks_of, options, shown_in_order, times_decoded,
    };
    use super::*;
    use waft_video_codecs::framing::{units_after_start_codes, with_lengths};
    use waft_video_codecs::h264::test_clip::{CLIP, pictures_of_the_clip, record_of};
    use waft_video_codecs::h264::{ParameterSets, is_parameter_set};

    /// When each of the clip's pictures is shown, in the order they are decoded, counted in
    /// pictures: what `ffprobe` reads of the clip's packets.
    const SHOWN_AT: [i64; 12] = [0, 3, 1, 2, 5, 4, 8, 6, 7, 11, 9, 10];

    /// The clip's chunks in the order they are decoded, each framed by `framed`.
    fn chunks(framed: impl Fn(&[&[u8]]) -> Vec<u8>) -> Vec<EncodedChunk> {
        let data = pictures_of_the_clip()
            .iter()
            .map(|units| framed(units))
            .collect();
        chunks_of(data, &SHOWN_AT)
    }

    #[test]
    fn a_stream_in_start_codes_comes_out_in_the_order_it_is_shown() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
        let times = times_decoded(&decoder, chunks(after_start_codes));
        assert_eq!(times, shown_in_order(12));
    }

    /// The order is the stream's own: chunks whose times say the order they are decoded in, as
    /// an app that knows no better gives them, come out in the order they are shown all the
    /// same, each frame with the time its chunk carried.
    #[test]
    fn a_streams_order_is_its_own_whatever_times_its_chunks_carry() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
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
    fn a_stream_after_lengths_with_its_record_comes_out_the_same() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let setup = VideoStreamSetup::H264(NalFormat::LengthPrefixed(record_of(&sets)));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
        let without_sets = |units: &[&[u8]]| {
            with_lengths(units.iter().copied().filter(|unit| !is_parameter_set(unit)))
        };
        let times = times_decoded(&decoder, chunks(without_sets));
        assert_eq!(times, shown_in_order(12));
    }

    #[test]
    fn a_frame_says_what_its_decoder_and_its_stream_say_of_it() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
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
        assert_eq!(frame.color().range, None, "the clip does not say its range");
    }

    #[test]
    fn a_chunk_before_the_streams_sets_have_come_is_bad_data() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
        let mut later = chunks(after_start_codes).remove(1);
        later.key_frame = false;
        assert!(matches!(
            block_on(decoder.decode(later)),
            Err(DecoderError::BadData(_))
        ));
    }

    #[test]
    fn a_codec_videotoolbox_has_no_decoder_of_is_unsupported() {
        assert!(matches!(
            Decoder::open(VideoStreamSetup::Vp8, &options()),
            Err(DecoderError::Unsupported(_))
        ));
    }

    #[test]
    fn no_more_is_decoded_while_as_many_frames_as_the_app_holds_wait_to_be_read() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let held_at_once = VideoDecoderOptions {
            frame_limit: 2,
            ..options()
        };
        let decoder = Decoder::open(setup, &held_at_once).expect("a decoder");
        let decoding: Vec<_> = chunks(after_start_codes)
            .into_iter()
            .map(|chunk| decoder.decode(chunk))
            .collect();
        // The queue decodes until two frames wait, then stops: nothing reads them.
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(decoder.output.waiting(), 2);
        drop(decoder);
        // Let go of, the decoder decodes on to no one, and every chunk is answered.
        for decoded in decoding {
            let _ = block_on(decoded);
        }
    }

    #[test]
    fn after_a_reset_no_picture_of_a_chunk_handed_over_before_it_comes_out() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
        // Six chunks handed over and none of their frames read, some still on the queue.
        let before: Vec<_> = chunks(after_start_codes)
            .into_iter()
            .take(6)
            .map(|chunk| decoder.decode(chunk))
            .collect();
        decoder.reset();
        let times = times_decoded(&decoder, chunks(after_start_codes));
        assert_eq!(
            times,
            shown_in_order(12),
            "only the pictures handed over after the reset"
        );
        for decoded in before {
            block_on(decoded).expect("a chunk dropped by a reset is answered as taken");
        }
        assert_eq!(decoder.output.waiting(), 0);
    }

    #[test]
    fn a_decoder_the_app_lets_go_of_ends_its_frames() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
        let frames = decoder.next_frame();
        drop(decoder);
        assert!(block_on(frames).is_none());
    }
}
