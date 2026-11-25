//! Media Foundation's H.264 decoder, `CLSID_MSH264DecoderMFT`, as one decoder of the app's
//! drives it: Annex B in, pictures out in NV12, in the order they are shown, which the decoder
//! keeps itself. Given a D3D11 device manager it decodes on the video hardware, through DXVA, into
//! textures of its own pool; given none, on the processor, into samples handed to it. Either way
//! each picture is put out in a shared texture of its own (`device`).
//!
//! The decoder is synchronous: it takes a sample until it has pictures to give, which are taken
//! until it asks for more. A change of the stream's size or format is told as
//! `MF_E_TRANSFORM_STREAM_CHANGE`, and its output type is chosen again.
//!
//! The invariant: a sample's time is the number the decoder side gave it, carried to its picture
//! unchanged, never a time of the stream's; and every picture the decoder gives is put out, in the
//! order it gives them.

use std::mem::ManuallyDrop;
use std::sync::Arc;

use waft_embedder::video_decoders::{CodecChoice, CodecKind, DecoderError, DecodingPurpose};
use waft_embedder::{ColorRange, PixelAspect, PixelRect, VideoColor};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    CLSID_MSH264DecoderMFT, CODECAPI_AVLowLatencyMode, IMF2DBuffer, IMFDXGIBuffer,
    IMFDXGIDeviceManager, IMFMediaBuffer, IMFMediaType, IMFSample, IMFTransform,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_PIXEL_ASPECT_RATIO,
    MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES,
    MF_MT_YUV_MATRIX, MF_SA_D3D11_AWARE, MFCreateDXGIDeviceManager, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFSampleExtension_CleanPoint,
    MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFVideoArea,
    MFVideoFormat_H264, MFVideoFormat_NV12,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::core::Interface as _;

use super::device::VideoDevice;
use super::super::shared_textures::SharedTexture;

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
    kind: CodecKind,
    /// Whether the decoder hands out samples of its own pool, as it does on the video hardware,
    /// rather than filling those handed to it.
    provides_samples: bool,
    /// How big a sample handed to it must be, where it fills them.
    sample_size: u32,
    pictures: Pictures,
    /// The device manager the decoder reaches `device` through, held as long as the decoder.
    _manager: Option<IMFDXGIDeviceManager>,
}

impl Transform {
    /// The decoder for an H.264 stream, on the video hardware where `choice` and `device` allow,
    /// on the processor otherwise; `Unsupported` where `choice` asks for hardware `device` lacks.
    pub(super) fn open(
        device: &VideoDevice,
        choice: CodecChoice,
        purpose: DecodingPurpose,
    ) -> Result<Transform, DecoderError> {
        let hardware = match choice {
            CodecChoice::Software => false,
            CodecChoice::Any => device.decodes_h264(),
            CodecChoice::Hardware if device.decodes_h264() => true,
            CodecChoice::Hardware => {
                return Err(DecoderError::Unsupported(
                    "this system's video hardware does not decode H.264".to_owned(),
                ));
            }
        };
        // SAFETY: COM is started on this thread (`decoder`); the class is the system's.
        let decoder: IMFTransform =
            unsafe { CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| {
                    DecoderError::Unsupported(format!("no H.264 decoder on this system: {error}"))
                })?;
        let manager = if hardware {
            Some(hand_device(&decoder, device)?)
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
        set_input_type(&decoder)?;
        let pictures = choose_output_type(&decoder)?;
        // SAFETY: calls on the decoder, its stream 0 the one an H.264 decoder has.
        let info = unsafe { decoder.GetOutputStreamInfo(0) }.map_err(failed)?;
        let provides_samples = info.dwFlags
            & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0) as u32
            != 0;
        // SAFETY: messages the decoder takes with no parameter.
        unsafe {
            decoder
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(failed)?;
            decoder
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(failed)?;
        }
        Ok(Transform {
            decoder,
            kind: if hardware {
                CodecKind::Hardware
            } else {
                CodecKind::Software
            },
            provides_samples,
            sample_size: info.cbSize,
            pictures,
            _manager: manager,
        })
    }

    pub(super) fn kind(&self) -> CodecKind {
        self.kind
    }

    /// Hands the decoder `data`, one picture in Annex B, numbered `sample`, and answers the
    /// pictures it gave meanwhile.
    pub(super) fn decode(
        &mut self,
        data: &[u8],
        sample: i64,
        key_frame: bool,
        device: &mut VideoDevice,
    ) -> Result<Vec<DecodedPicture>, DecoderError> {
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
        // SAFETY: a message the decoder takes with no parameter.
        unsafe { self.decoder.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0) }.map_err(failed)?;
        let mut pictures = Vec::new();
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
            let given = if self.provides_samples {
                None
            } else {
                Some(output_sample(self.sample_size)?)
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
                    self.pictures = choose_output_type(&self.decoder)?;
                    // SAFETY: a call on the decoder, its stream 0.
                    let info = unsafe { self.decoder.GetOutputStreamInfo(0) }.map_err(failed)?;
                    self.sample_size = info.cbSize;
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
        // SAFETY: calls on a sample the decoder gave, its first buffer the picture's.
        let (number, buffer) = unsafe { (sample.GetSampleTime(), sample.GetBufferByIndex(0)) };
        let number = number.map_err(failed)?;
        let buffer = buffer.map_err(failed)?;
        let size = self.pictures.size;
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
                device.copy_out(&texture, subresource, size)?
            }
            Err(_) => self.upload(&buffer, device)?,
        };
        Ok(DecodedPicture {
            texture,
            sample: number,
            visible: self.pictures.visible,
            pixel_aspect: self.pictures.pixel_aspect,
            color: self.pictures.color,
        })
    }

    /// `buffer`, a picture in the decoder's memory, uploaded into a shared texture of its own.
    fn upload(
        &self,
        buffer: &IMFMediaBuffer,
        device: &mut VideoDevice,
    ) -> Result<Arc<SharedTexture>, DecoderError> {
        let size = self.pictures.size;
        let rows = size[1] as usize;
        if let Ok(two_d) = buffer.cast::<IMF2DBuffer>() {
            let mut first_row = std::ptr::null_mut();
            let mut pitch = 0;
            // SAFETY: the buffer is locked, read within its picture's planes, and unlocked.
            unsafe {
                two_d.Lock2D(&mut first_row, &mut pitch).map_err(failed)?;
                let pitch = pitch.unsigned_abs() as usize;
                let planes = std::slice::from_raw_parts(first_row, pitch * rows * 3 / 2);
                let uploaded = device.upload(planes, pitch, pitch * rows, size);
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
                let pitch = self.pictures.stride.unwrap_or(size[0] as usize);
                let uploaded = device.upload(planes, pitch, pitch * rows, size);
                let _ = buffer.Unlock();
                uploaded
            }
        }
    }
}

/// Hands the decoder `device` through a device manager, so it decodes on the video hardware;
/// `Unsupported` for a decoder that does not take D3D11.
fn hand_device(
    decoder: &IMFTransform,
    device: &VideoDevice,
) -> Result<IMFDXGIDeviceManager, DecoderError> {
    // SAFETY: a call on the decoder's own attributes.
    let aware = unsafe {
        decoder
            .GetAttributes()
            .and_then(|attributes| attributes.GetUINT32(&MF_SA_D3D11_AWARE))
    };
    if aware.unwrap_or(0) == 0 {
        return Err(DecoderError::Unsupported(
            "the system's H.264 decoder does not decode on D3D11".to_owned(),
        ));
    }
    let mut token = 0;
    let mut manager = None;
    // SAFETY: the out pointers are live locals; the device is this decoder's, handed to the
    // manager, which the decoder reaches it through from here on.
    unsafe {
        MFCreateDXGIDeviceManager(&mut token, &mut manager).map_err(failed)?;
        let manager = manager.ok_or_else(|| DecoderError::Failed("no device manager".to_owned()))?;
        manager.ResetDevice(device.device(), token).map_err(failed)?;
        decoder
            .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)
            .map_err(failed)?;
        Ok(manager)
    }
}

/// Tells the decoder it is handed H.264.
fn set_input_type(decoder: &IMFTransform) -> Result<(), DecoderError> {
    // SAFETY: a media type of this process, set on the decoder's stream 0.
    unsafe {
        let input = MFCreateMediaType().map_err(failed)?;
        input
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(failed)?;
        input
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
            .map_err(failed)?;
        decoder.SetInputType(0, &input, 0).map_err(failed)
    }
}

/// Chooses NV12 among the types the decoder offers its pictures in, and answers what it says of
/// them.
fn choose_output_type(decoder: &IMFTransform) -> Result<Pictures, DecoderError> {
    for index in 0.. {
        // SAFETY: calls on the decoder's stream 0, each index until it has no more.
        let Ok(offered) = (unsafe { decoder.GetOutputAvailableType(0, index) }) else {
            break;
        };
        // SAFETY: a call on a media type the decoder offered.
        if unsafe { offered.GetGUID(&MF_MT_SUBTYPE) }.is_ok_and(|subtype| subtype == MFVideoFormat_NV12)
        {
            // SAFETY: the type the decoder offered, set on its stream 0.
            unsafe { decoder.SetOutputType(0, &offered, 0) }.map_err(failed)?;
            return pictures_of(&offered);
        }
    }
    Err(DecoderError::Failed(
        "the system's H.264 decoder offers no NV12".to_owned(),
    ))
}

/// What `media_type`, the decoder's output type, says of its pictures.
fn pictures_of(media_type: &IMFMediaType) -> Result<Pictures, DecoderError> {
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
        2 => Some(1),  // BT.709
        3 => Some(4),  // BT.470 System M
        4 => Some(5),  // BT.470 System B, G
        5 => Some(6),  // SMPTE 170M
        6 => Some(7),  // SMPTE 240M
        9 => Some(9),  // BT.2020
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
