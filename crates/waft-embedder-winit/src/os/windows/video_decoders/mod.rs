//! The system's video decoders on Windows: Media Foundation's H.264 decoder, handed H.264 so far.
//!
//! A stream reaches the decoder as it takes one, in Annex B (`annex_b`), a picture at a time on a
//! thread of the decoder's own (`decoder`); the decoder (`transform`) runs on the video hardware
//! through DXVA where the D3D11 device it is handed (`device`) offers H.264, and on the processor
//! otherwise. It puts its pictures out in the order they are shown, each in an NV12 texture of its
//! own shared by NT handle ([`SharedTexture`](super::SharedTexture)), as a frame (`frame`) that
//! reaches the app from the thread through `output`.

mod annex_b;
mod decoder;
mod device;
mod frame;
mod output;
mod transform;

use std::future::ready;
use std::rc::Rc;

use std::sync::Arc;
use windows::Win32::Foundation::LUID;

use waft_embedder::video_decoders::{
    DecodeSupport, VideoCodec, VideoDecodeSupport, VideoDecoder, VideoDecoderOpenFuture,
    VideoDecoderOptions, VideoDecoders, VideoStreamDescription, VideoStreamSetup,
};

use decoder::Decoder;

/// Media Foundation's decoders on the adapter whose LUID is `adapter`, or on the default adapter:
/// a GPU's decoders, as `SystemGpu::video_decoders` answers them. `None` where no adapter has that
/// LUID.
pub(crate) fn video_decoders(adapter: Option<LUID>) -> Option<Rc<dyn VideoDecoders>> {
    if let Some(luid) = adapter {
        device::adapter_of(luid).ok()?;
    }
    Some(Rc::new(MediaFoundationDecoders { adapter }))
}

/// Media Foundation's decoders, whose pictures are in textures shared on `adapter`.
struct MediaFoundationDecoders {
    adapter: Option<LUID>,
}

impl VideoDecoders for MediaFoundationDecoders {
    /// Media Foundation says nothing of a profile or a size before a decoder is made, so H.264 is
    /// unknown until tried, and what is not handed to it yet is not decoded.
    fn support(&self, description: &VideoStreamDescription) -> VideoDecodeSupport {
        let support = match description.codec {
            VideoCodec::H264 => DecodeSupport::Unknown,
            _ => DecodeSupport::No,
        };
        VideoDecodeSupport {
            hardware: support,
            software: support,
        }
    }

    fn open(
        &self,
        setup: VideoStreamSetup,
        options: VideoDecoderOptions,
    ) -> VideoDecoderOpenFuture {
        let opened = Decoder::open(setup, &options, self.adapter)
            .map(|decoder| Arc::new(decoder) as Arc<dyn VideoDecoder>);
        Box::pin(ready(opened))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pollster::block_on;
    use waft_embedder::PixelRect;
    use waft_embedder::video_decoders::{
        CodecChoice, CodecKind, DecoderError, DecodingPurpose, EncodedChunk, NalFormat,
    };

    use windows::Win32::Foundation::LUID;

    use waft_video_codecs::framing::{units_after_start_codes, with_lengths, with_start_codes};
    use waft_video_codecs::h264::test_clip::{CLIP, pictures_of_the_clip, record_of};
    use waft_video_codecs::h264::{ParameterSets, is_parameter_set};

    use super::decoder::MediaFoundation;
    use super::device::VideoDevice;
    use super::transform::Transform;
    use super::*;

    /// When each of the clip's pictures is shown, in the order they are decoded, counted in
    /// pictures: what `ffprobe` reads of the clip's packets.
    const SHOWN_AT: [i64; 12] = [0, 3, 1, 2, 5, 4, 8, 6, 7, 11, 9, 10];

    /// The time of a picture shown `number` pictures in, in nanoseconds, at thirty a second.
    fn time_of(number: i64) -> i64 {
        number * 33_333_333
    }

    pub(super) fn options(decoder: CodecChoice) -> VideoDecoderOptions {
        VideoDecoderOptions {
            decoder,
            purpose: DecodingPurpose::Playback,
            frame_limit: 4,
            largest_size: None,
            default_color: None,
        }
    }

    /// The clip's chunks in the order they are decoded, each framed by `framed`.
    pub(super) fn chunks(framed: impl Fn(&[&[u8]]) -> Vec<u8>) -> Vec<EncodedChunk> {
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

    pub(super) fn after_start_codes(units: &[&[u8]]) -> Vec<u8> {
        with_start_codes(units.iter().copied())
    }

    fn open(choice: CodecChoice) -> Arc<dyn VideoDecoder> {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        block_on(MediaFoundationDecoders { adapter: None }.open(setup, options(choice)))
            .expect("a decoder")
    }

    /// Hands every one of `chunks` to `decoder`, drains it, and answers the times of the frames
    /// that came out, in the order they came, read as the decoder puts them out.
    fn times_decoded(decoder: &dyn VideoDecoder, chunks: Vec<EncodedChunk>) -> Vec<Option<i64>> {
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

    fn in_order() -> Vec<Option<i64>> {
        (0..12).map(|number| Some(time_of(number))).collect()
    }

    #[test]
    fn a_stream_in_start_codes_comes_out_in_the_order_it_is_shown() {
        let decoder = open(CodecChoice::Any);
        eprintln!("decoding on {:?}", decoder.kind());
        assert_eq!(
            times_decoded(&*decoder, chunks(after_start_codes)),
            in_order()
        );
    }

    /// For playback, Media Foundation's decoder holds all of this clip's pictures until it drains;
    /// for a live stream it gives each as soon as the stream's order allows, in the same order.
    #[test]
    fn a_live_stream_comes_out_in_the_order_it_is_shown_as_soon_as_it_allows() {
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        // Room for every frame, as the chunks are decoded one after another before any is read.
        let live = VideoDecoderOptions {
            purpose: DecodingPurpose::Live,
            frame_limit: 12,
            ..options(CodecChoice::Any)
        };
        let decoder = block_on(MediaFoundationDecoders { adapter: None }.open(setup, live))
            .expect("a decoder");
        let mut chunks = chunks(after_start_codes).into_iter();
        for chunk in chunks.by_ref().take(3) {
            block_on(decoder.decode(chunk)).expect("the chunk decodes");
        }
        // Three chunks in, two of them shown after the third, the first picture is out.
        let first = block_on(decoder.next_frame()).expect("a frame");
        assert_eq!(first.timestamp(), Some(0));
        for chunk in chunks {
            block_on(decoder.decode(chunk)).expect("the chunk decodes");
        }
        block_on(decoder.drain()).expect("the decoder drains");
        let mut times = vec![first.timestamp()];
        times.extend((1..12).map(|_| block_on(decoder.next_frame()).expect("a frame").timestamp()));
        assert_eq!(times, in_order());
    }

    #[test]
    fn the_processor_decodes_the_stream_the_same() {
        let decoder = open(CodecChoice::Software);
        assert_eq!(decoder.kind(), CodecKind::Software);
        assert_eq!(
            times_decoded(&*decoder, chunks(after_start_codes)),
            in_order()
        );
    }

    /// The order is the stream's own: chunks whose times say the order they are decoded in come
    /// out in the order they are shown all the same, each frame with the time its chunk carried.
    #[test]
    fn a_streams_order_is_its_own_whatever_times_its_chunks_carry() {
        let decoder = open(CodecChoice::Any);
        let mut chunks = chunks(after_start_codes);
        for (decoded, chunk) in chunks.iter_mut().enumerate() {
            chunk.timestamp = decoded as i64;
        }
        let times = times_decoded(&*decoder, chunks);
        let mut decoded_in_order_shown: Vec<i64> = (0..12).collect();
        decoded_in_order_shown.sort_by_key(|decoded| SHOWN_AT[*decoded as usize]);
        let carried: Vec<_> = decoded_in_order_shown.into_iter().map(Some).collect();
        assert_eq!(times, carried);
    }

    #[test]
    fn a_stream_after_lengths_with_its_record_comes_out_the_same() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let setup = VideoStreamSetup::H264(NalFormat::LengthPrefixed(record_of(&sets)));
        let decoder = block_on(
            MediaFoundationDecoders { adapter: None }.open(setup, options(CodecChoice::Any)),
        )
        .expect("a decoder");
        let without_sets = |units: &[&[u8]]| {
            with_lengths(units.iter().copied().filter(|unit| !is_parameter_set(unit)))
        };
        assert_eq!(times_decoded(&*decoder, chunks(without_sets)), in_order());
    }

    #[test]
    fn a_frame_says_what_its_decoder_and_its_stream_say_of_it() {
        let decoder = open(CodecChoice::Any);
        let first = chunks(after_start_codes).remove(0);
        block_on(decoder.decode(first)).expect("the chunk decodes");
        block_on(decoder.drain()).expect("the decoder drains");
        let frame = block_on(decoder.next_frame()).expect("a frame");
        eprintln!("memory {}x{}", frame.width(), frame.height());
        assert_eq!(
            frame.visible_rect(),
            PixelRect {
                x: 0,
                y: 0,
                width: 320,
                height: 240
            }
        );
        assert!(frame.width() >= 320 && frame.height() >= 240);
        assert_eq!(frame.timestamp(), Some(0));
        assert_eq!(frame.duration(), Some(Duration::from_nanos(33_333_333)));
        let shared = frame.hardware_buffer();
        let shared: &dyn std::any::Any = &*shared;
        let shared = shared
            .downcast_ref::<super::super::SharedTexture>()
            .expect("a shared texture");
        assert_eq!(
            waft_embedder::HardwareBuffer::format(shared),
            waft_embedder::HardwareBufferFormat::Nv12
        );
    }

    /// A D3D12 device of wgpu's that samples NV12, as an app's would be, and the LUID of its
    /// adapter; `None` where this system's has no NV12.
    fn device_that_samples_nv12() -> Option<(wgpu::Device, wgpu::Queue, LUID)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter =
            block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        eprintln!("the app's adapter: {}", adapter.get_info().name);
        if !adapter
            .features()
            .contains(wgpu::Features::TEXTURE_FORMAT_NV12)
        {
            return None;
        }
        // SAFETY: the adapter is a live one of wgpu's D3D12 backend, its DXGI adapter asked only
        // for its description.
        let luid = unsafe {
            let hal = adapter.as_hal::<wgpu_hal::api::Dx12>()?;
            hal.raw_adapter().GetDesc1().ok()?.AdapterLuid
        };
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TEXTURE_FORMAT_NV12,
            ..Default::default()
        }))
        .ok()?;
        Some((device, queue, luid))
    }

    /// The bytes of `plane` of `texture`, copied into a texture of `format` of its own, `size`
    /// texels, and read back row after row.
    fn plane_of(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        plane: wgpu::TextureAspect,
        format: wgpu::TextureFormat,
        size: [u32; 2],
    ) -> Vec<u8> {
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let single = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let row = size[0] * format.block_copy_size(None).expect("a plain format");
        let padded = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(padded * size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: plane,
            },
            single.as_image_copy(),
            extent,
        );
        encoder.copy_texture_to_buffer(
            single.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: None,
                },
            },
            extent,
        );
        queue.submit([encoder.finish()]);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, |mapped| mapped.expect("mapped"));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the copy finishes");
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .expect("the mapped bytes");
        mapped
            .chunks(padded as usize)
            .flat_map(|line| &line[..row as usize])
            .copied()
            .collect()
    }

    /// The first picture, decoded on the app's adapter, opens on the app's device over the
    /// decoder's memory, and holds what H.264 says it decodes to: the sums of its planes are those
    /// of `ffmpeg`'s decoding of the clip.
    #[test]
    fn a_picture_opens_on_a_device_that_samples_nv12_as_the_stream_decodes() {
        const LUMA_SUM: u64 = 9_678_637;
        const CHROMA_SUM: u64 = 4_854_024;
        let Some((device, queue, adapter)) = device_that_samples_nv12() else {
            eprintln!("no D3D12 device here samples NV12; not checked");
            return;
        };
        let _started = MediaFoundation::start();
        let mut video_device = VideoDevice::new(Some(adapter)).expect("a device on the adapter");
        let mut transform =
            Transform::open(&video_device, CodecChoice::Any, DecodingPurpose::Playback)
                .expect("a decoder");
        let first = chunks(after_start_codes).remove(0);
        let mut pictures = transform
            .decode(&first.data, 0, true, &mut video_device)
            .expect("the chunk decodes");
        pictures.extend(
            transform
                .drain(&mut video_device)
                .expect("the decoder drains"),
        );
        let [picture] = &pictures[..] else {
            panic!("{} pictures of one chunk", pictures.len());
        };
        let texture = picture
            .texture
            .open_on(&device, wgpu::TextureUsages::COPY_SRC)
            .expect("the picture opens on the device");
        let sum = |bytes: Vec<u8>| bytes.into_iter().map(u64::from).sum::<u64>();
        let luma = plane_of(
            &device,
            &queue,
            &texture,
            wgpu::TextureAspect::Plane0,
            wgpu::TextureFormat::R8Unorm,
            [320, 240],
        );
        let chroma = plane_of(
            &device,
            &queue,
            &texture,
            wgpu::TextureAspect::Plane1,
            wgpu::TextureFormat::Rg8Unorm,
            [160, 120],
        );
        assert_eq!((sum(luma), sum(chroma)), (LUMA_SUM, CHROMA_SUM));
    }

    #[test]
    fn a_chunk_that_is_no_key_frame_at_the_start_is_bad_data() {
        let decoder = open(CodecChoice::Any);
        let later = chunks(after_start_codes).remove(1);
        assert!(matches!(
            block_on(decoder.decode(later)),
            Err(DecoderError::BadData(_))
        ));
    }

    #[test]
    fn a_codec_not_handed_yet_is_unsupported() {
        let opened = block_on(
            MediaFoundationDecoders { adapter: None }
                .open(VideoStreamSetup::Vp9(None), options(CodecChoice::Any)),
        );
        assert!(matches!(opened, Err(DecoderError::Unsupported(_))));
    }

    #[test]
    fn after_a_reset_no_picture_of_a_chunk_handed_over_before_it_comes_out() {
        let decoder = open(CodecChoice::Any);
        let before: Vec<_> = chunks(after_start_codes)
            .into_iter()
            .take(6)
            .map(|chunk| decoder.decode(chunk))
            .collect();
        decoder.reset();
        let times = times_decoded(&*decoder, chunks(after_start_codes));
        assert_eq!(
            times,
            in_order(),
            "only the pictures handed over after the reset"
        );
        for decoded in before {
            block_on(decoded).expect("a chunk dropped by a reset is answered as taken");
        }
    }

    #[test]
    fn a_decoder_the_app_lets_go_of_ends_its_frames() {
        let decoder = open(CodecChoice::Any);
        let frames = decoder.next_frame();
        drop(decoder);
        assert!(block_on(frames).is_none());
    }
}
