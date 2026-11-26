//! A Media Foundation decoder of the stream's codec, as one decoder of the app's drives it: H.264
//! and HEVC in Annex B, VP9 and AV1 each chunk whole, pictures out in NV12, or P010 for a stream of
//! more than eight bits to a sample, in the order they are shown, which the decoder keeps itself.
//! H.264's decoder is the system's own (`CLSID_MSH264DecoderMFT`) and VP9's is made by its class
//! (`CLSID_MSVPxDecoder`), as Firefox makes it; HEVC's and AV1's come with Microsoft's Store
//! extensions, found by enumerating the decoders of their subtype (`MFTEnumEx`). Given a D3D11
//! device manager a decoder decodes on the video hardware, through DXVA, into textures of its own
//! pool; given none, on the processor, into samples handed to it. Either way each picture is put
//! out in a shared texture of its own (`device`).
//!
//! A decoder is made as its stream opens, and told its types once the stream's description is
//! whole: its subtype and its pictures' size and, for HEVC, its profile, which Microsoft's HEVC
//! decoder expects for Main 10. It is synchronous: it takes a sample until it has pictures to
//! give, which are taken until it asks for more. A change of the stream's size or format is told
//! as `MF_E_TRANSFORM_STREAM_CHANGE`, and its output type is chosen again.
//!
//! The invariant: a sample's time is the number the decoder side gave it, carried to its picture
//! unchanged, never a time of the stream's; and every picture the decoder gives is put out, in the
//! order it gives them.

use std::mem::ManuallyDrop;
use std::sync::Arc;

use waft_embedder::video_decoders::{
    CodecChoice, CodecKind, DecodeSupport, DecoderError, DecodingPurpose, VideoCodec,
};
use waft_embedder::{ColorRange, HardwareBufferFormat, PixelAspect, PixelRect, VideoColor};
use waft_video_codecs::StreamFacts;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    CLSID_MSH264DecoderMFT, CLSID_MSVPxDecoder, CODECAPI_AVLowLatencyMode, IMF2DBuffer,
    IMFActivate, IMFDXGIBuffer, IMFDXGIDeviceManager, IMFMediaBuffer, IMFMediaType, IMFSample,
    IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE,
    MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION,
    MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX, MF_SA_D3D11_AWARE,
    MFCreateDXGIDeviceManager, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFMediaType_Video, MFSampleExtension_CleanPoint, MFSampleExtension_DecodeTimestamp,
    MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_LOCALMFT, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_ENUM_FLAG_SYNCMFT, MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES,
    MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoArea,
    MFVideoFormat_AV1, MFVideoFormat_H264, MFVideoFormat_HEVC, MFVideoFormat_NV12,
    MFVideoFormat_P010, MFVideoFormat_VP90,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::core::{GUID, Interface as _};

use super::super::shared_textures::SharedTexture;
use super::device::{VideoDevice, sample_bytes};

/// A picture the decoder gave, put out in a texture of its own.
pub(super) struct DecodedPicture {
    pub(super) texture: Arc<SharedTexture>,
    /// The number of the sample it came of, as it was handed over.
    pub(super) sample: i64,
    pub(super) visible: PixelRect,
    pub(super) pixel_aspect: PixelAspect,
    pub(super) color: VideoColor,
}

/// What the decoder's output type says of its pictures.
#[derive(Clone, Copy)]
struct Pictures {
    /// NV12 or P010.
    format: HardwareBufferFormat,
    /// The size of the picture's memory, the decoder's blocks included.
    size: [u32; 2],
    visible: PixelRect,
    pixel_aspect: PixelAspect,
    color: VideoColor,
    /// The bytes between rows of a picture in the decoder's memory, where it says.
    stride: Option<usize>,
}

/// The decoder, set up for one stream.
pub(super) struct Transform {
    decoder: IMFTransform,
    codec: VideoCodec,
    kind: CodecKind,
    /// What its types say, once it was told them.
    streaming: Option<Streaming>,
    /// Whether the stream's samples are of more than eight bits, so its pictures are asked in
    /// P010.
    deep: bool,
    /// The device manager the decoder reaches `device` through, held as long as the decoder.
    _manager: Option<IMFDXGIDeviceManager>,
}

/// What a decoder told its types says of its output.
struct Streaming {
    /// Whether the decoder hands out samples of its own pool, as it does on the video hardware,
    /// rather than filling those handed to it.
    provides_samples: bool,
    /// How big a sample handed to it must be, where it fills them.
    sample_size: u32,
    pictures: Pictures,
}

impl Transform {
    /// The decoder of `codec`, on the video hardware where `choice` and `device` allow, on the
    /// processor otherwise; `Unsupported` where `choice` asks for hardware `device` lacks, or the
    /// system has no decoder of the codec.
    pub(super) fn open(
        device: &VideoDevice,
        codec: VideoCodec,
        choice: CodecChoice,
        purpose: DecodingPurpose,
    ) -> Result<Transform, DecoderError> {
        let hardware = match choice {
            CodecChoice::Software => false,
            CodecChoice::Any => device.decodes(codec),
            CodecChoice::Hardware if device.decodes(codec) => true,
            CodecChoice::Hardware => {
                return Err(DecoderError::Unsupported(format!(
                    "this system's video hardware does not decode {codec:?}"
                )));
            }
        };
        let decoder = decoder_of(codec)?;
        let manager = if hardware {
            Some(hand_device(&decoder, device, codec)?)
        } else {
            None
        };
        if purpose == DecodingPurpose::Live {
            // SAFETY: a call on the decoder's own attributes. A decoder that ignores it decodes
            // as for playback, which is what a refusal leaves.
            let _ = unsafe {
                decoder
                    .GetAttributes()
                    .and_then(|attributes| attributes.SetUINT32(&CODECAPI_AVLowLatencyMode, 1))
            };
        }
        Ok(Transform {
            decoder,
            codec,
            kind: if hardware {
                CodecKind::Hardware
            } else {
                CodecKind::Software
            },
            streaming: None,
            deep: false,
            _manager: manager,
        })
    }

    /// Goes on with a stream that says `facts`: the decoder is told its types the first time, and
    /// from then on a stream that grew deeper or shallower is asked in its format at the next
    /// change the decoder tells.
    pub(super) fn adopt(&mut self, facts: &StreamFacts) -> Result<(), DecoderError> {
        self.deep = facts.bit_depth > 8;
        if self.streaming.is_some() {
            return Ok(());
        }
        set_input_type(&self.decoder, self.codec, facts)?;
        let pictures = choose_output_type(&self.decoder, self.deep)?;
        // SAFETY: calls on the decoder, its stream 0 the one a video decoder has.
        let info = unsafe { self.decoder.GetOutputStreamInfo(0) }.map_err(failed)?;
        let provides_samples = info.dwFlags
            & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0)
                as u32
            != 0;
        // SAFETY: messages the decoder takes with no parameter.
        unsafe {
            self.decoder
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(failed)?;
            self.decoder
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(failed)?;
        }
        self.streaming = Some(Streaming {
            provides_samples,
            sample_size: info.cbSize,
            pictures,
        });
        Ok(())
    }

    /// Whether the decoder was told its types, and so takes samples.
    pub(super) fn is_streaming(&self) -> bool {
        self.streaming.is_some()
    }

    pub(super) fn kind(&self) -> CodecKind {
        self.kind
    }

    /// Hands the decoder `data`, one picture as its subtype frames one, numbered `sample`, and
    /// answers the pictures it gave meanwhile; `BadData` before it was told its types.
    pub(super) fn decode(
        &mut self,
        data: &[u8],
        sample: i64,
        key_frame: bool,
        device: &mut VideoDevice,
    ) -> Result<Vec<DecodedPicture>, DecoderError> {
        if self.streaming.is_none() {
            return Err(DecoderError::BadData(
                "what the stream's decoder is made of has not come".to_owned(),
            ));
        }
        let input = input_sample(data, sample, key_frame)?;
        let mut pictures = Vec::new();
        loop {
            // SAFETY: the sample is a live sample of this process, stream 0 the decoder's one.
            match unsafe { self.decoder.ProcessInput(0, &input, 0) } {
                Ok(()) => break,
                // The decoder holds pictures to give first: `MF_E_NOTACCEPTING`.
                Err(_) if self.take_pictures(device, &mut pictures)? > 0 => {}
                Err(error) => return Err(bad_data(error)),
            }
        }
        self.take_pictures(device, &mut pictures)?;
        Ok(pictures)
    }

    /// Asks the decoder for every picture it holds, as at a stream's end, and answers them.
    pub(super) fn drain(
        &mut self,
        device: &mut VideoDevice,
    ) -> Result<Vec<DecodedPicture>, DecoderError> {
        let mut pictures = Vec::new();
        if self.streaming.is_none() {
            return Ok(pictures);
        }
        // SAFETY: a message the decoder takes with no parameter.
        unsafe { self.decoder.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0) }.map_err(failed)?;
        self.take_pictures(device, &mut pictures)?;
        Ok(pictures)
    }

    /// Drops every sample and picture the decoder holds, as for a seek.
    pub(super) fn flush(&mut self) {
        // SAFETY: a message the decoder takes with no parameter; a decoder that refuses it holds
        // nothing to drop.
        let _ = unsafe { self.decoder.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0) };
    }

    /// Takes every picture the decoder has to give into `pictures`, answering how many.
    fn take_pictures(
        &mut self,
        device: &mut VideoDevice,
        pictures: &mut Vec<DecodedPicture>,
    ) -> Result<usize, DecoderError> {
        let before = pictures.len();
        loop {
            let Some(streaming) = &self.streaming else {
                return Ok(0);
            };
            let given = if streaming.provides_samples {
                None
            } else {
                Some(output_sample(streaming.sample_size)?)
            };
            let mut output = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(given),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            };
            let mut status = 0;
            // SAFETY: one output buffer for the decoder's one stream, holding a sample of the
            // size it asked for where it fills them; what it leaves in the buffer is taken back.
            let result = unsafe {
                self.decoder
                    .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
            };
            // SAFETY: the buffer's sample and events are taken once each, after the call.
            let (sample, _events) = unsafe {
                (
                    ManuallyDrop::take(&mut output.pSample),
                    ManuallyDrop::take(&mut output.pEvents),
                )
            };
            match result {
                Ok(()) => {
                    let sample = sample.ok_or_else(|| {
                        DecoderError::Failed("the decoder gave no picture".to_owned())
                    })?;
                    pictures.push(self.put_out(&sample, device)?);
                }
                Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    return Ok(pictures.len() - before);
                }
                Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    let pictures = choose_output_type(&self.decoder, self.deep)?;
                    // SAFETY: a call on the decoder, its stream 0.
                    let info = unsafe { self.decoder.GetOutputStreamInfo(0) }.map_err(failed)?;
                    if let Some(streaming) = &mut self.streaming {
                        streaming.pictures = pictures;
                        streaming.sample_size = info.cbSize;
                    }
                }
                Err(error) => return Err(bad_data(error)),
            }
        }
    }

    /// `sample`, a picture the decoder gave, put out in a shared texture of its own.
    fn put_out(
        &self,
        sample: &IMFSample,
        device: &mut VideoDevice,
    ) -> Result<DecodedPicture, DecoderError> {
        let pictures = self.pictures()?;
        // SAFETY: calls on a sample the decoder gave, its first buffer the picture's.
        let (number, buffer) = unsafe { (sample.GetSampleTime(), sample.GetBufferByIndex(0)) };
        let number = number.map_err(failed)?;
        let buffer = buffer.map_err(failed)?;
        let size = pictures.size;
        let texture = match buffer.cast::<IMFDXGIBuffer>() {
            Ok(dxgi) => {
                let mut resource = std::ptr::null_mut();
                // SAFETY: the buffer names a texture of the decoder's pool; the pointer it answers
                // is a reference this side owns, taken by `from_raw`.
                let (texture, subresource) = unsafe {
                    dxgi.GetResource(&ID3D11Texture2D::IID, &mut resource)
                        .map_err(failed)?;
                    (
                        ID3D11Texture2D::from_raw(resource),
                        dxgi.GetSubresourceIndex().map_err(failed)?,
                    )
                };
                device.copy_out(&texture, subresource, size, pictures.format)?
            }
            Err(_) => upload(&buffer, &pictures, device)?,
        };
        Ok(DecodedPicture {
            texture,
            sample: number,
            visible: pictures.visible,
            pixel_aspect: pictures.pixel_aspect,
            color: pictures.color,
        })
    }

    /// What the decoder's output type says of its pictures, once it was told its types.
    fn pictures(&self) -> Result<Pictures, DecoderError> {
        self.streaming
            .as_ref()
            .map(|streaming| streaming.pictures)
            .ok_or_else(|| DecoderError::Failed("a picture before the decoder's types".to_owned()))
    }
}

/// `buffer`, a picture in the decoder's memory of the kind `pictures` says, uploaded into a shared
/// texture of its own.
fn upload(
    buffer: &IMFMediaBuffer,
    pictures: &Pictures,
    device: &mut VideoDevice,
) -> Result<Arc<SharedTexture>, DecoderError> {
    let size = pictures.size;
    let format = pictures.format;
    let rows = size[1] as usize;
    if let Ok(two_d) = buffer.cast::<IMF2DBuffer>() {
        let mut first_row = std::ptr::null_mut();
        let mut pitch = 0;
        // SAFETY: the buffer is locked, read within its picture's planes, and unlocked.
        unsafe {
            two_d.Lock2D(&mut first_row, &mut pitch).map_err(failed)?;
            let pitch = pitch.unsigned_abs() as usize;
            let planes = std::slice::from_raw_parts(first_row, pitch * rows * 3 / 2);
            let uploaded = device.upload(planes, pitch, pitch * rows, size, format);
            let _ = two_d.Unlock2D();
            uploaded
        }
    } else {
        let mut start = std::ptr::null_mut();
        let mut length = 0;
        // SAFETY: the buffer is locked, read within the length it gives, and unlocked.
        unsafe {
            buffer
                .Lock(&mut start, None, Some(&mut length))
                .map_err(failed)?;
            let planes = std::slice::from_raw_parts(start, length as usize);
            let pitch = pictures
                .stride
                .unwrap_or(size[0] as usize * sample_bytes(format));
            let uploaded = device.upload(planes, pitch, pitch * rows, size, format);
            let _ = buffer.Unlock();
            uploaded
        }
    }
}

/// What the system can tell of its decoders of `codec` without making one: none of AV1 or HEVC
/// where no Store extension's decoder enumerates, and otherwise nothing until one is made. VP9's,
/// which the system makes by its class, is not told apart: enumerating may not list it.
pub(super) fn support_of(codec: VideoCodec) -> DecodeSupport {
    match codec {
        VideoCodec::Hevc | VideoCodec::Av1 if enumerated(codec).is_empty() => DecodeSupport::No,
        VideoCodec::Vp8 => DecodeSupport::No,
        _ => DecodeSupport::Unknown,
    }
}

/// A decoder of `codec`, made; `Unsupported` where the system has none.
fn decoder_of(codec: VideoCodec) -> Result<IMFTransform, DecoderError> {
    let unsupported = |why: String| {
        DecoderError::Unsupported(format!("no {codec:?} decoder on this system: {why}"))
    };
    let class = match codec {
        VideoCodec::H264 => CLSID_MSH264DecoderMFT,
        VideoCodec::Vp9 => CLSID_MSVPxDecoder,
        VideoCodec::Hevc | VideoCodec::Av1 => {
            let first = enumerated(codec).into_iter().next().ok_or_else(|| {
                unsupported("none enumerates; its Store extension is missing".to_owned())
            })?;
            // SAFETY: an activation object the enumeration answered, asked once for its decoder.
            return unsafe { first.ActivateObject::<IMFTransform>() }
                .map_err(|error| unsupported(error.to_string()));
        }
        VideoCodec::Vp8 => {
            return Err(unsupported(
                "VP8 is not handed to Media Foundation".to_owned(),
            ));
        }
    };
    // SAFETY: COM is started on this thread (`decoder`); the class is the system's.
    unsafe { CoCreateInstance(&class, None, CLSCTX_INPROC_SERVER) }
        .map_err(|error| unsupported(error.to_string()))
}

/// Media Foundation's subtype of `codec`'s samples.
fn subtype_of(codec: VideoCodec) -> Option<GUID> {
    match codec {
        VideoCodec::H264 => Some(MFVideoFormat_H264),
        VideoCodec::Hevc => Some(MFVideoFormat_HEVC),
        VideoCodec::Vp9 => Some(MFVideoFormat_VP90),
        VideoCodec::Av1 => Some(MFVideoFormat_AV1),
        VideoCodec::Vp8 => None,
    }
}

/// The system's synchronous decoders of `codec`, each as an object that makes it, in the order
/// Media Foundation prefers them; none where it has none, or the enumeration fails.
fn enumerated(codec: VideoCodec) -> Vec<IMFActivate> {
    let Some(subtype) = subtype_of(codec) else {
        return Vec::new();
    };
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: subtype,
    };
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0;
    // SAFETY: the type and the out pointers are live locals. The array the call allocates holds
    // `count` references this side owns: each is taken out once, and the array freed after.
    unsafe {
        let enumerated = MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_LOCALMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            None,
            &mut activates,
            &mut count,
        );
        if enumerated.is_err() || activates.is_null() {
            return Vec::new();
        }
        let found = (0..count as usize)
            .filter_map(|index| activates.add(index).replace(None))
            .collect();
        CoTaskMemFree(Some(activates.cast()));
        found
    }
}

/// Hands the decoder `device` through a device manager, so it decodes on the video hardware;
/// `Unsupported` for a decoder that does not take D3D11.
fn hand_device(
    decoder: &IMFTransform,
    device: &VideoDevice,
    codec: VideoCodec,
) -> Result<IMFDXGIDeviceManager, DecoderError> {
    // SAFETY: a call on the decoder's own attributes.
    let aware = unsafe {
        decoder
            .GetAttributes()
            .and_then(|attributes| attributes.GetUINT32(&MF_SA_D3D11_AWARE))
    };
    if aware.unwrap_or(0) == 0 {
        return Err(DecoderError::Unsupported(format!(
            "the system's {codec:?} decoder does not decode on D3D11"
        )));
    }
    let mut token = 0;
    let mut manager = None;
    // SAFETY: the out pointers are live locals; the device is this decoder's, handed to the
    // manager, which the decoder reaches it through from here on.
    unsafe {
        MFCreateDXGIDeviceManager(&mut token, &mut manager).map_err(failed)?;
        let manager =
            manager.ok_or_else(|| DecoderError::Failed("no device manager".to_owned()))?;
        manager
            .ResetDevice(device.device(), token)
            .map_err(failed)?;
        decoder
            .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)
            .map_err(failed)?;
        Ok(manager)
    }
}

/// Tells the decoder it is handed `codec`, of pictures as large as `facts` says and, for HEVC,
/// of the profile it says.
fn set_input_type(
    decoder: &IMFTransform,
    codec: VideoCodec,
    facts: &StreamFacts,
) -> Result<(), DecoderError> {
    let subtype = subtype_of(codec).ok_or_else(|| {
        DecoderError::Unsupported(format!("{codec:?} is not handed to Media Foundation"))
    })?;
    // SAFETY: a media type of this process, set on the decoder's stream 0.
    unsafe {
        let input = MFCreateMediaType().map_err(failed)?;
        input
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(failed)?;
        input.SetGUID(&MF_MT_SUBTYPE, &subtype).map_err(failed)?;
        if let Some(size) = facts.size {
            let packed = (u64::from(size.width) << 32) | u64::from(size.height);
            input.SetUINT64(&MF_MT_FRAME_SIZE, packed).map_err(failed)?;
        }
        if codec == VideoCodec::Hevc {
            // `eAVEncH265VProfile` numbers HEVC's profiles as the stream does.
            input
                .SetUINT32(&MF_MT_MPEG2_PROFILE, facts.profile)
                .map_err(failed)?;
        }
        decoder.SetInputType(0, &input, 0).map_err(failed)
    }
}

/// Chooses among the types the decoder offers its pictures in P010 where the stream is `deep`,
/// NV12 otherwise, or the other of the two where it offers only that, and answers what it says of
/// them.
fn choose_output_type(decoder: &IMFTransform, deep: bool) -> Result<Pictures, DecoderError> {
    let (wanted, other) = if deep {
        (MFVideoFormat_P010, MFVideoFormat_NV12)
    } else {
        (MFVideoFormat_NV12, MFVideoFormat_P010)
    };
    let offered = offered_types(decoder);
    let subtype_of = |offered: &IMFMediaType| {
        // SAFETY: a call on a media type the decoder offered.
        unsafe { offered.GetGUID(&MF_MT_SUBTYPE) }.ok()
    };
    let chosen = offered
        .iter()
        .find(|offered| subtype_of(offered) == Some(wanted))
        .or_else(|| {
            offered
                .iter()
                .find(|offered| subtype_of(offered) == Some(other))
        })
        .ok_or_else(|| {
            DecoderError::Failed("the system's decoder offers neither NV12 nor P010".to_owned())
        })?;
    // SAFETY: the type the decoder offered, set on its stream 0.
    unsafe { decoder.SetOutputType(0, chosen, 0) }.map_err(failed)?;
    let format = if subtype_of(chosen) == Some(MFVideoFormat_P010) {
        HardwareBufferFormat::P010
    } else {
        HardwareBufferFormat::Nv12
    };
    pictures_of(chosen, format)
}

/// The types the decoder offers its pictures in, in its order.
fn offered_types(decoder: &IMFTransform) -> Vec<IMFMediaType> {
    // SAFETY: calls on the decoder's stream 0, each index until it has no more.
    (0..)
        .map_while(|index| unsafe { decoder.GetOutputAvailableType(0, index) }.ok())
        .collect()
}

/// What `media_type`, the decoder's output type in `format`, says of its pictures.
fn pictures_of(
    media_type: &IMFMediaType,
    format: HardwareBufferFormat,
) -> Result<Pictures, DecoderError> {
    // SAFETY: reads of the type's own attributes.
    unsafe {
        let packed = media_type.GetUINT64(&MF_MT_FRAME_SIZE).map_err(failed)?;
        let size = [(packed >> 32) as u32, packed as u32];
        let mut area = MFVideoArea::default();
        let visible = media_type
            .GetBlob(
                &MF_MT_MINIMUM_DISPLAY_APERTURE,
                std::slice::from_raw_parts_mut(
                    (&raw mut area).cast::<u8>(),
                    size_of::<MFVideoArea>(),
                ),
                None,
            )
            .ok()
            .map_or(
                PixelRect {
                    x: 0,
                    y: 0,
                    width: size[0],
                    height: size[1],
                },
                |()| PixelRect {
                    x: area.OffsetX.value.max(0) as u32,
                    y: area.OffsetY.value.max(0) as u32,
                    width: area.Area.cx as u32,
                    height: area.Area.cy as u32,
                },
            );
        let pixel_aspect = media_type
            .GetUINT64(&MF_MT_PIXEL_ASPECT_RATIO)
            .ok()
            .map(|packed| PixelAspect {
                width: (packed >> 32) as u32,
                height: packed as u32,
            })
            .filter(|aspect| aspect.width > 0 && aspect.height > 0)
            .unwrap_or(PixelAspect {
                width: 1,
                height: 1,
            });
        let stride = media_type
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .ok()
            .map(|stride| (stride as i32).unsigned_abs() as usize);
        Ok(Pictures {
            format,
            size,
            visible,
            pixel_aspect,
            color: colour_of(media_type),
            stride,
        })
    }
}

/// What the decoder says of its pictures' colour, as ITU-T H.273 numbers it; `None` of each value
/// it leaves unknown. Media Foundation's values are `MFVideoPrimaries`, `MFVideoTransferFunction`,
/// `MFVideoTransferMatrix` and `MFNominalRange`.
fn colour_of(media_type: &IMFMediaType) -> VideoColor {
    // SAFETY: reads of the type's own attributes.
    let read = |key| unsafe { media_type.GetUINT32(key) }.ok();
    let primaries = read(&MF_MT_VIDEO_PRIMARIES).and_then(|value| match value {
        2 => Some(1), // BT.709
        3 => Some(4), // BT.470 System M
        4 => Some(5), // BT.470 System B, G
        5 => Some(6), // SMPTE 170M
        6 => Some(7), // SMPTE 240M
        9 => Some(9), // BT.2020
        _ => None,
    });
    let transfer = read(&MF_MT_TRANSFER_FUNCTION).and_then(|value| match value {
        5 => Some(1),   // BT.709
        8 => Some(13),  // sRGB
        15 => Some(16), // SMPTE 2084, PQ
        16 => Some(18), // HLG
        _ => None,
    });
    let matrix = read(&MF_MT_YUV_MATRIX).and_then(|value| match value {
        1 => Some(1), // BT.709
        2 => Some(6), // BT.601
        3 => Some(7), // SMPTE 240M
        4 | 5 => Some(9),
        _ => None,
    });
    let range = read(&MF_MT_VIDEO_NOMINAL_RANGE).and_then(|value| match value {
        1 => Some(ColorRange::Full),
        2 => Some(ColorRange::Limited),
        _ => None,
    });
    VideoColor {
        primaries,
        transfer,
        matrix,
        range,
        chroma_location: None,
    }
}

/// A sample of `data`, numbered `number`, marked as a key frame where it is one.
fn input_sample(data: &[u8], number: i64, key_frame: bool) -> Result<IMFSample, DecoderError> {
    let length = u32::try_from(data.len())
        .map_err(|_| DecoderError::BadData("a chunk of 4 GiB or more".to_owned()))?;
    // SAFETY: a buffer of `length` bytes, locked, written within them and unlocked; a sample of
    // this process.
    unsafe {
        let buffer = MFCreateMemoryBuffer(length).map_err(failed)?;
        let mut start = std::ptr::null_mut();
        buffer.Lock(&mut start, None, None).map_err(failed)?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), start, data.len());
        buffer.Unlock().map_err(failed)?;
        buffer.SetCurrentLength(length).map_err(failed)?;
        let sample = MFCreateSample().map_err(failed)?;
        sample.AddBuffer(&buffer).map_err(failed)?;
        sample.SetSampleTime(number).map_err(failed)?;
        // Without a decode timestamp, Microsoft's HEVC decoder takes sample times that do not rise
        // in presentation order as decode times and reassigns them across outputs instead of
        // carrying each sample's number with its picture.
        sample
            .SetUINT64(&MFSampleExtension_DecodeTimestamp, number as u64)
            .map_err(failed)?;
        if key_frame {
            sample
                .SetUINT32(&MFSampleExtension_CleanPoint, 1)
                .map_err(failed)?;
        }
        Ok(sample)
    }
}

/// A sample of `size` bytes for the decoder to fill.
fn output_sample(size: u32) -> Result<IMFSample, DecoderError> {
    // SAFETY: a buffer and a sample of this process.
    unsafe {
        let buffer = MFCreateMemoryBuffer(size).map_err(failed)?;
        let sample = MFCreateSample().map_err(failed)?;
        sample.AddBuffer(&buffer).map_err(failed)?;
        Ok(sample)
    }
}

fn failed(error: windows::core::Error) -> DecoderError {
    DecoderError::Failed(error.to_string())
}

/// A chunk the decoder refused: damaged, or not a key frame where one is due.
fn bad_data(error: windows::core::Error) -> DecoderError {
    DecoderError::BadData(error.to_string())
}
