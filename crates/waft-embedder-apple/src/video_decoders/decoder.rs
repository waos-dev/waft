//! A decoder the app holds: a stream's chunks in, its pictures out in the order they are shown.
//!
//! Each decoder decodes on a queue of its own, one chunk after another, so the app's thread
//! never waits on VideoToolbox and pictures are decoded in the order their chunks came. What
//! the queue works on (`Work`) is the stream as far as it is known: how it frames its units, its
//! parameter sets, the session made of them, and the pictures decoded and not yet shown.
//!
//! A stream's parameter sets may come ahead, in its setup, or in its chunks, and may change. A
//! session is made once a whole set of them is known, and made again when VideoToolbox does not
//! go on with the new ones; the pictures still held then are shown first, as they come before
//! the change. VideoToolbox's pool grows as frames are held, so the decoder holds itself back: it
//! decodes no more while as many frames as the app said it holds at once wait to be read.
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
    CodecChoice, CodecKind, DecodedFrameFuture, DecoderError, EncodedChunk, VideoDecoder,
    VideoDecoderFuture, VideoDecoderOptions, VideoStreamSetup,
};
use waft_embedder::{ColorRange, PixelAspect, VideoColor, VideoFrame};

use waft_video_codecs::StreamFacts;
use waft_video_codecs::framing::{Framing, with_lengths};
use waft_video_codecs::h264::{self, ParameterSets, PictureOrder, is_parameter_set};
use waft_video_codecs::presentation_order::PresentationOrder;

use super::output::Output;
use super::session::{DecodedPicture, Session};
use crate::answer::answer;
use crate::media::CoreVideoFrame;
use crate::media::DecodedFacts;

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

/// What a decoder's queue works on.
struct Work {
    framing: Framing,
    /// The stream's parameter sets, as far as they have come.
    sets: ParameterSets,
    choice: CodecChoice,
    /// The colour the app gave for what the stream leaves unsaid.
    default_color: Option<VideoColor>,
    /// The session made of `sets`, once they are whole, with what they say of the stream.
    session: Option<(Session, StreamFacts)>,
    /// Where each picture stands in the order its stream is shown in, as the stream says.
    picture_order: PictureOrder,
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
    /// Goes on with `sets`: with the session there is, where VideoToolbox does, and with one
    /// made of them otherwise, the pictures held shown first.
    fn adopt(&mut self, sets: ParameterSets) -> Result<(), DecoderError> {
        let facts = sets.facts().map_err(DecoderError::BadData)?;
        self.picture_order
            .adopt(&sets)
            .map_err(DecoderError::BadData)?;
        let goes_on = matches!(&self.session, Some((session, _)) if session.accepts(&sets));
        let session = match self.session.take() {
            Some((session, _)) if goes_on => session,
            _ => {
                self.output.put(self.order.all(), self.asked_after_resets);
                let full_range = self.colour_of(&facts).range == Some(ColorRange::Full);
                Session::open(&sets, self.choice, full_range)?
            }
        };
        self.order.set_depth(facts.reorder_depth);
        self.session = Some((session, facts));
        self.sets = sets;
        Ok(())
    }

    /// Decodes `chunk`, once the app has read enough of the frames put out, taking the parameter
    /// sets it carries, and puts out the pictures that can be shown now.
    fn decode(&mut self, chunk: &EncodedChunk) -> Result<(), DecoderError> {
        let units = self
            .framing
            .units(&chunk.data)
            .map_err(DecoderError::BadData)?;
        let sets = self.sets.replaced_by(ParameterSets::among(&units));
        if sets.are_complete() && (sets != self.sets || self.session.is_none()) {
            self.adopt(sets)?;
        }
        let Some((session, facts)) = &self.session else {
            return Err(DecoderError::BadData(
                "the stream's parameter sets have not come".to_owned(),
            ));
        };
        let coded: Vec<&[u8]> = units
            .into_iter()
            .filter(|unit| !is_parameter_set(unit))
            .collect();
        // Counted before it is decoded, so a picture VideoToolbox gives nothing for is counted.
        let place = self
            .picture_order
            .place_of(&coded)
            .map_err(DecoderError::BadData)?;
        let decoded = session.decode(&with_lengths(coded), chunk.timestamp, chunk.duration)?;
        if let Some(picture) = decoded {
            let place = place.ok_or_else(|| {
                DecoderError::BadData("a picture with no slice to tell its order by".to_owned())
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
        let (framing, sets) = framing_and_sets(setup)?;
        self.framing = framing;
        if sets.are_complete() && sets != self.sets {
            self.adopt(sets)?;
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
    /// A decoder for the stream `setup` begins, its session made at once where the setup holds
    /// the stream's parameter sets, and with the first key frame otherwise.
    pub(super) fn open(
        setup: VideoStreamSetup,
        options: &VideoDecoderOptions,
    ) -> Result<Decoder, DecoderError> {
        let (framing, sets) = framing_and_sets(setup)?;
        let output = Output::default();
        let mut work = Work {
            framing,
            sets: ParameterSets::default(),
            choice: options.decoder,
            default_color: options.default_color,
            session: None,
            picture_order: PictureOrder::default(),
            order: PresentationOrder::new(0),
            output: output.clone(),
            frame_limit: options.frame_limit.max(1) as usize,
            asked_after_resets: 0,
        };
        if sets.are_complete() {
            work.adopt(sets)?;
        }
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
    use waft_embedder::video_decoders::{DecodingPurpose, NalFormat};

    use super::*;
    use waft_video_codecs::framing::units_after_start_codes;
    use waft_video_codecs::h264::test_clip::{CLIP, pictures_of_the_clip, record_of};

    /// When each of the clip's pictures is shown, in the order they are decoded, counted in
    /// pictures: what `ffprobe` reads of the clip's packets.
    const SHOWN_AT: [i64; 12] = [0, 3, 1, 2, 5, 4, 8, 6, 7, 11, 9, 10];

    /// The time of a picture shown `number` pictures in, in nanoseconds, at thirty a second.
    fn time_of(number: i64) -> i64 {
        number * 33_333_333
    }

    fn options() -> VideoDecoderOptions {
        VideoDecoderOptions {
            decoder: CodecChoice::Any,
            purpose: DecodingPurpose::Playback,
            frame_limit: 4,
            largest_size: None,
            default_color: None,
        }
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

    /// Hands every one of `chunks` to `decoder`, drains it, and answers the times of the frames
    /// that came out, in the order they came. The chunks are all handed over first, as an app
    /// that does not wait for each does, and the frames read as the decoder puts them out.
    fn times_decoded(decoder: &Decoder, chunks: Vec<EncodedChunk>) -> Vec<Option<i64>> {
        let count = chunks.len();
        let decoding: Vec<_> = chunks
            .into_iter()
            .map(|chunk| decoder.decode(chunk))
            .collect();
        let draining = decoder.drain();
        let times = (0..count)
            .map(|_| block_on(decoder.next_frame()).expect("a frame").timestamp())
            .collect();
        for decoded in decoding {
            block_on(decoded).expect("the chunk decodes");
        }
        block_on(draining).expect("the decoder drains");
        times
    }

    #[test]
    fn a_stream_in_start_codes_comes_out_in_the_order_it_is_shown() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let decoder = Decoder::open(setup, &options()).expect("a decoder");
        let times = times_decoded(&decoder, chunks(after_start_codes));
        let in_order: Vec<_> = (0..12).map(|number| Some(time_of(number))).collect();
        assert_eq!(times, in_order);
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
        let in_order: Vec<_> = (0..12).map(|number| Some(time_of(number))).collect();
        assert_eq!(times, in_order);
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
    fn a_codec_videotoolbox_is_not_handed_yet_is_unsupported() {
        assert!(matches!(
            Decoder::open(VideoStreamSetup::Vp9(None), &options()),
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
        let in_order: Vec<_> = (0..12).map(|number| Some(time_of(number))).collect();
        assert_eq!(
            times, in_order,
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
