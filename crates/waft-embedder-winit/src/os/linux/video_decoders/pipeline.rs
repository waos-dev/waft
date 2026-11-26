//! One stream's GStreamer pipeline: a source the chunks are pushed into, the system's decoder,
//! the conversion to ARGB8888, and a sink the pictures are pulled from. A picture of more than
//! eight bits to a sample (HEVC's Main 10, VP9's profile 2, AV1 at ten bits) is converted to
//! ARGB8888 too, so it reaches the app in eight.
//!
//! On the video hardware the conversion stays on the GPU (`vapostproc`) and the sink is handed
//! the decoder's dmabufs; on the processor it is `videoconvert`, and the sink is handed BGRA in
//! the processor's memory. Stopping the pipeline to its beginning and running it again drops
//! everything it holds, its stream's end too, which is how a decoder is drained to its end, reset
//! and given another setup.
//!
//! The invariants: what the source is told its chunks are (its caps) is set before the pipeline
//! runs, each time it runs; and the sink's end is taken for the stream's only where it comes in
//! the run it was pulled in, as a sink that is stopped answers that its stream ended.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use waft_embedder::video_decoders::{DecoderError, NalFormat, VideoStreamSetup};

use super::super::gstreamer::{Caps, Element, Gst, Pipeline as Running, Pulled};

/// Which of the system's decoders a pipeline runs, by its elements' names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Path {
    /// VA-API's decoder, after the parser it needs, in order.
    Hardware(&'static [&'static str]),
    Software(&'static str),
}

/// What reaches a stream's pipeline of a chunk: the chunk's bytes as the stream frames them, its
/// caps saying how (H.264 and HEVC in Annex B or after lengths with their record, VP9 each chunk
/// whole, AV1 each temporal unit with its record where the file gave one), with the parameter sets
/// the setup gave ahead before a key frame in start codes.
pub(super) struct Framing {
    caps: String,
    sets_ahead: Option<Vec<u8>>,
}

impl Framing {
    /// `Unsupported` for VP8, which GStreamer is not handed.
    pub(super) fn of(setup: &VideoStreamSetup) -> Result<Framing, DecoderError> {
        let framing = |caps: String| Framing {
            caps,
            sets_ahead: None,
        };
        match setup {
            VideoStreamSetup::H264(format) => Ok(of_units("video/x-h264", "avc", format)),
            VideoStreamSetup::Hevc(format) => Ok(of_units("video/x-h265", "hvc1", format)),
            VideoStreamSetup::Vp9(_) => Ok(framing("video/x-vp9".to_owned())),
            VideoStreamSetup::Av1(record) => {
                let mut caps = "video/x-av1,stream-format=obu-stream,alignment=tu".to_owned();
                if let Some(record) = record {
                    caps.push_str(&format!(",codec_data=(buffer){}", hex(record)));
                }
                Ok(framing(caps))
            }
            VideoStreamSetup::Vp8 => Err(DecoderError::Unsupported(
                "this system's decoders are not handed VP8".to_owned(),
            )),
        }
    }

    /// `data`, a chunk's bytes, as the pipeline takes them: after the sets given ahead where it
    /// is a key frame.
    pub(super) fn framed(&self, data: &[u8], key_frame: bool) -> Vec<u8> {
        match &self.sets_ahead {
            Some(sets) if key_frame => [sets.as_slice(), data].concat(),
            _ => data.to_vec(),
        }
    }
}

/// The framing of an H.264 or HEVC stream, whose caps are named `media_type`, framed as `format`
/// says: in Annex B, or after lengths in the stream format `with_lengths` names, its record as
/// the caps' codec data.
fn of_units(media_type: &str, with_lengths: &str, format: &NalFormat) -> Framing {
    match format {
        NalFormat::StartCodes(ahead) => Framing {
            caps: format!("{media_type},stream-format=byte-stream,alignment=au"),
            sets_ahead: ahead.clone().filter(|ahead| !ahead.is_empty()),
        },
        NalFormat::LengthPrefixed(record) => Framing {
            caps: format!(
                "{media_type},stream-format={with_lengths},alignment=au,codec_data=(buffer){}",
                hex(record)
            ),
            sets_ahead: None,
        },
    }
}

/// `bytes` as GStreamer's caps write a buffer: two hexadecimal digits a byte.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A stream's pipeline and the elements the decoder talks to.
pub(super) struct Pipeline {
    gst: &'static Gst,
    running: Running,
    source: Element,
    sink: Element,
    /// The caps of the reference timestamps that carry each chunk's number.
    reference: Caps,
    path: Path,
    runs: Mutex<Runs>,
    runs_changed: Condvar,
}

/// How far the pipeline's runs have gone.
#[derive(Default)]
struct Runs {
    /// How many times the pipeline was run.
    current: u64,
    /// The run whose stream's end the sink handed out.
    ended: u64,
    /// The first error the pipeline posted, after which it does nothing more.
    failure: Option<String>,
}

impl Pipeline {
    /// The pipeline of `path` for a stream framed as `framing`, running.
    pub(super) fn open(
        gst: &'static Gst,
        path: Path,
        framing: &Framing,
    ) -> Result<Pipeline, DecoderError> {
        let unsupported = |why: String| DecoderError::Unsupported(format!("{path:?}: {why}"));
        let element = gst.pipeline(&description(path)).map_err(unsupported)?;
        let source = element
            .by_name("chunks")
            .ok_or_else(|| unsupported("no source".into()))?;
        let sink = element
            .by_name("frames")
            .ok_or_else(|| unsupported("no sink".into()))?;
        let reference = gst
            .caps("timestamp/x-waft-chunk")
            .ok_or_else(|| unsupported("no reference caps".into()))?;
        let pipeline = Pipeline {
            gst,
            running: Running(element),
            source,
            sink,
            reference,
            path,
            runs: Mutex::new(Runs::default()),
            runs_changed: Condvar::new(),
        };
        pipeline.run(framing).map_err(unsupported)?;
        Ok(pipeline)
    }

    pub(super) fn path(&self) -> Path {
        self.path
    }

    pub(super) fn reference(&self) -> &Caps {
        &self.reference
    }

    /// Runs the pipeline afresh for a stream framed as `framing`: whatever it held is dropped.
    pub(super) fn run(&self, framing: &Framing) -> Result<(), String> {
        self.running.0.stop()?;
        let caps = self
            .gst
            .caps(&framing.caps)
            .ok_or_else(|| format!("caps GStreamer does not read: {}", framing.caps))?;
        self.source.set_caps(&caps);
        self.running.0.play()?;
        // Counted once it runs, so an end the sink tells while it is stopped is the last run's.
        self.runs().current += 1;
        self.runs_changed.notify_all();
        Ok(())
    }

    /// Pushes a chunk's bytes, carrying `number` to its picture.
    pub(super) fn push(&self, data: &[u8], number: u64) -> Result<(), String> {
        self.source.push(data, &self.reference, number)
    }

    /// Ends the stream and waits until every picture of it has been pulled; why not, where the
    /// pipeline failed.
    pub(super) fn finish(&self) -> Result<(), String> {
        self.source.end_stream();
        let mut runs = self.runs();
        loop {
            if let Some(failure) = &runs.failure {
                return Err(failure.clone());
            }
            if runs.ended == runs.current {
                return Ok(());
            }
            drop(runs);
            self.failure();
            runs = self.runs();
            runs = self
                .runs_changed
                .wait_timeout(runs, Duration::from_millis(100))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// The next picture, waiting at most `timeout`; the stream's end is told to [`finish`], and
    /// after it this waits for the pipeline's next run, as long.
    ///
    /// [`finish`]: Self::finish
    pub(super) fn pull(&self, timeout: Duration) -> Pulled {
        let run = self.runs().current;
        let pulled = self.sink.pull(timeout);
        if matches!(pulled, Pulled::EndOfStream) {
            let mut runs = self.runs();
            if runs.current == run {
                runs.ended = run;
                self.runs_changed.notify_all();
            }
            let _ = self
                .runs_changed
                .wait_timeout_while(runs, timeout, |runs| runs.current == run)
                .unwrap_or_else(PoisonError::into_inner);
        }
        pulled
    }

    /// Why the pipeline failed, if it did.
    pub(super) fn failure(&self) -> Option<String> {
        let posted = self.running.0.error();
        let mut runs = self.runs();
        if runs.failure.is_none() && posted.is_some() {
            runs.failure = posted;
            self.runs_changed.notify_all();
        }
        runs.failure.clone()
    }

    fn runs(&self) -> MutexGuard<'_, Runs> {
        self.runs.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The pipeline of `path`, in GStreamer's launch syntax.
fn description(path: Path) -> String {
    match path {
        Path::Hardware(elements) => format!(
            "appsrc name=chunks format=time ! {} ! vapostproc ! \
             video/x-raw(memory:DMABuf),format=DMA_DRM,drm-format=AR24 ! \
             appsink name=frames sync=false",
            elements.join(" ! ")
        ),
        Path::Software(decoder) => format!(
            "appsrc name=chunks format=time ! {decoder} ! videoconvert ! \
             video/x-raw,format=BGRA ! appsink name=frames sync=false"
        ),
    }
}
