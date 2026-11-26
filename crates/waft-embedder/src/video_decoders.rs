//! The system's video decoders, as a host hands them to the app. Not a dart:ui file: the
//! framework's own video is the system's players ([`Players`](crate::Players)), which read and
//! decode a file themselves. These are the decoders alone, for an app that reads a file or a
//! stream itself and hands over one encoded picture at a time: Apple's VideoToolbox, Android's
//! MediaCodec, Windows' Media Foundation. They follow `wasdk:media/video-decoders` and
//! `wasdk:media/video-streams` one to one, and are a module of their own, named as that
//! interface is.
//!
//! A stream is its codec and its setup bytes, as a container holds them or the stream carries
//! them: every system's decoder is configured from those bytes and reads the profile, the level
//! and the picture's size out of them, so nothing here says them a second time.
//!
//! Chunks go in and frames come out. [`VideoDecoder::decode`] takes a chunk when the decoder has
//! room; its picture comes later from [`VideoDecoder::next_frame`], in the order pictures are
//! shown, which is not the order chunks go in where a stream reorders. A frame is the decoder's
//! own picture, lent from its pool ([`VideoFrame`](crate::VideoFrame)), and says what its stream
//! says of it.
//!
//! The invariants a host keeps: each future resolves, once, whether or not the app still holds
//! the decoder; frames come out in the order they are shown; and a chunk the decoder cannot
//! decode is answered [`DecoderError::BadData`] and leaves the decoder taking chunks, from the
//! next key frame on.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::VideoFrame;

/// A GPU's video decoders, or `None` from
/// [`SystemGpu::video_decoders`](crate::SystemGpu::video_decoders) where the system decodes no
/// video for the app.
pub trait VideoDecoders {
    /// What the system can tell, without trying, about decoding a stream like `description`.
    fn support(&self, description: &VideoStreamDescription) -> VideoDecodeSupport;

    /// A decoder for the stream `setup` begins: [`DecoderError::Unsupported`] where the system
    /// has none of the kind chosen for it, [`DecoderError::Busy`] where every such decoder is in
    /// use.
    fn open(&self, setup: VideoStreamSetup, options: VideoDecoderOptions)
    -> VideoDecoderOpenFuture;
}

/// One decoder the system opened for the app. Dropping it ends its frames; the frames the app
/// still holds stay the system's until they are dropped.
pub trait VideoDecoder {
    /// Where it runs.
    fn kind(&self) -> CodecKind;

    /// Hands `chunk` to the decoder, resolving once it has room for the next; its picture comes
    /// from [`next_frame`](Self::next_frame). [`DecoderError::BadData`] for a chunk it cannot
    /// decode, a chunk that is no key frame where one is due among them: at the stream's start,
    /// and after a [`drain`](Self::drain), a [`reset`](Self::reset), a
    /// [`configure`](Self::configure) and a damaged chunk.
    fn decode(&self, chunk: EncodedChunk) -> VideoDecoderFuture;

    /// The next decoded picture, in the order pictures are shown; `None` once the decoder has
    /// ended. One is asked for at a time.
    fn next_frame(&self) -> DecodedFrameFuture;

    /// Why the decoder ended by itself, as the system taking it away; never resolves for a
    /// decoder that ends only as the app drops it.
    fn ended(&self) -> Pin<Box<dyn Future<Output = DecoderError>>>;

    /// Resolves once every chunk given has come out as a picture, as at a file's end.
    fn drain(&self) -> VideoDecoderFuture;

    /// Drops every chunk and picture the decoder holds, as for a seek.
    fn reset(&self);

    /// The stream's setup from the next chunk on: another size or other parameter sets of the
    /// same codec, as a stream that adapts its quality changes them.
    fn configure(&self, setup: VideoStreamSetup) -> VideoDecoderFuture;
}

/// A family of video streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    H264,
    Hevc,
    Vp8,
    Vp9,
    Av1,
}

/// How an H.264 or HEVC stream frames its units, with its setup in that form. Files hold one
/// form and broadcasts, cameras and calls the other; each system's decoder takes one of them,
/// and the host converts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NalFormat {
    /// As MP4, FLV and Matroska hold it: each unit after its length. The configuration record
    /// (`avcC`, `hvcC`) gives the length's size and the parameter sets, which the stream may
    /// carry too.
    LengthPrefixed(Vec<u8>),
    /// Annex B, as transport streams, RTSP and cameras send it: each unit after a start code.
    /// The parameter sets are in the stream, or given ahead here in the same form.
    StartCodes(Option<Vec<u8>>),
}

/// A stream's codec with what a decoder needs to begin on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoStreamSetup {
    H264(NalFormat),
    Hevc(NalFormat),
    Vp8,
    /// With its `vpcC` record, where the container has one.
    Vp9(Option<Vec<u8>>),
    /// With its `av1C` record, where the container has one. The chunks are AV1's low-overhead
    /// form, as MP4 and WebM hold them.
    Av1(Option<Vec<u8>>),
}

impl VideoStreamSetup {
    pub fn codec(&self) -> VideoCodec {
        match self {
            VideoStreamSetup::H264(_) => VideoCodec::H264,
            VideoStreamSetup::Hevc(_) => VideoCodec::Hevc,
            VideoStreamSetup::Vp8 => VideoCodec::Vp8,
            VideoStreamSetup::Vp9(_) => VideoCodec::Vp9,
            VideoStreamSetup::Av1(_) => VideoCodec::Av1,
        }
    }
}

/// One picture of a stream, encoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedChunk {
    /// The whole of one picture (an access unit), framed as the stream's setup says.
    pub data: Vec<u8>,
    /// When the picture is shown, in nanoseconds on the stream's own timeline, which may begin
    /// before zero: carried to the picture's frame unchanged. A decoder does not order by it:
    /// frames come out in the stream's own order, whatever the times say.
    pub timestamp: i64,
    /// How long it is shown, in nanoseconds, where the stream says.
    pub duration: Option<u64>,
    /// Whether it decodes with nothing before it.
    pub key_frame: bool,
}

/// Which of the system's codecs the app accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CodecChoice {
    /// Whichever the system picks, hardware first.
    #[default]
    Any,
    /// One that runs on the system's video hardware, or none.
    Hardware,
    /// One that runs on the processor, or none.
    Software,
}

/// Where a codec runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CodecKind {
    Hardware,
    Software,
}

/// A picture's size in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PictureSize {
    pub width: u32,
    pub height: u32,
}

/// How much of a picture's colour is kept beside its brightness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChromaSampling {
    /// Half in each direction, 4:2:0: nearly all video.
    Yuv420,
    /// Half across, 4:2:2.
    Yuv422,
    /// All of it, 4:4:4.
    Yuv444,
    /// None: a grey picture.
    Gray,
}

/// A stream as an app knows it before it has a decoder, from a manifest or a file's header: what
/// the systems need to say whether they decode it.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoStreamDescription {
    pub codec: VideoCodec,
    /// The profile as the codec numbers it (H.264's `profile_idc`, HEVC's
    /// `general_profile_idc`, VP9's and AV1's profile); `None` where the app does not know.
    pub profile: Option<u32>,
    /// The level as the codec numbers it; `None` where the app does not know.
    pub level: Option<u32>,
    /// Bits in each sample: 8, 10 or 12.
    pub bit_depth: u8,
    pub chroma: ChromaSampling,
    pub size: PictureSize,
    /// Pictures a second, where the app knows.
    pub frame_rate: Option<f32>,
}

/// What a system says of decoding a stream, for each kind of decoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoDecodeSupport {
    pub hardware: DecodeSupport,
    pub software: DecodeSupport,
}

/// What a system can tell without trying. Android answers from its decoders' own lists; Apple's
/// systems tell only whether a codec has hardware at all, so a profile or a size is
/// [`Unknown`](DecodeSupport::Unknown) there until a decoder is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeSupport {
    Yes,
    No,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoDecoderOptions {
    /// Which decoder the app accepts.
    pub decoder: CodecChoice,
    /// What the pictures are for.
    pub purpose: DecodingPurpose,
    /// The most frames the app has at one time: what a system that sizes a decoder's pool as it
    /// is made sizes it for.
    pub frame_limit: u32,
    /// The largest size the stream will reach, where the app knows: what lets Android's decoders
    /// change size in place.
    pub largest_size: Option<PictureSize>,
    /// The colour a file gives for the stream. It fills in only what the stream itself leaves
    /// unsaid: the stream describes its own pixels, and a file's header is often wrong.
    pub default_color: Option<crate::VideoColor>,
}

/// What the pictures are for, which decides how the system runs the decoder. These are settings
/// the systems take, not promises.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DecodingPurpose {
    /// Shown as they arrive, as in a call: each picture comes out as soon as the stream's order
    /// allows.
    Live,
    /// Played at the video's own rate.
    #[default]
    Playback,
    /// Not watched while decoding, as an export or thumbnails: the system may take its time to
    /// save power.
    Background,
}

/// Why a decoder was not opened, a chunk not decoded, or a decoder ended. Only
/// [`BadData`](DecoderError::BadData) leaves the decoder taking chunks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecoderError {
    /// The system has no decoder of the kind chosen for this stream; another choice may work.
    Unsupported(String),
    /// Every such decoder is in use now; fewer at once, or software, may work.
    Busy,
    /// The stream is damaged, or began without a key frame. The decoder goes on from the next
    /// key frame.
    BadData(String),
    /// The system took the decoder away. Another decoder goes on from a key frame.
    Lost,
    /// The system's decoder failed.
    Failed(String),
}

pub type VideoDecoderOpenFuture =
    Pin<Box<dyn Future<Output = Result<Arc<dyn VideoDecoder>, DecoderError>>>>;

pub type VideoDecoderFuture = Pin<Box<dyn Future<Output = Result<(), DecoderError>>>>;

pub type DecodedFrameFuture = Pin<Box<dyn Future<Output = Option<Arc<dyn VideoFrame>>>>>;
