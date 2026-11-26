# waft-embedder-winit

Waft's desktop host: winit's event loop and native windows, with each view drawing through valo at the vsync of the screen its window is on, and held while the window is minimized or occluded. It takes an app's start closure through `WinitEmbedder::run`, which `#[waft::main]` and `waft-embedder-default` reach, and provides `WinitPlatform`, Waft's `Platform` on macOS, Windows and Linux. `src/lib.rs` says how the host is built. An application depends on `waft-embedder-default`, not this crate.

What it provides:

- Windows the app asks for, each with a view, and every view's frames, metrics, sight and focus.
- Input: the pointer, the trackpad, touches on Windows, the keyboard, text input and the system's input methods.
- Drawing: valo on the device every window draws with; composition layers, and buffers the system shares imported as textures, where the system has them; the GPU a device runs on (`Platform::gpu_id`), a D3D12 device's adapter LUID on Windows and a Vulkan device's render node on Linux, and none on macOS, which names no GPU.
- The system's shell: the menu bar and pop-up menus, the Dock icon's menu and the notification area, notifications, the clipboard, file dialogs, opening files and links, and quitting through the app's answer.
- The system's media: players, media assets and cameras, image codecs, video frame readers and video decoders, where the system has them: VideoToolbox's on macOS, Media Foundation's H.264 decoder on Windows, its pictures in NV12 textures shared by NT handle, and GStreamer's on Linux.
- The user's appearance, languages and local time zone.
- HTTP over `NSURLSession` and the file system on the embedder's threads, on macOS; Windows and Linux answer `None` until their clients are written.
- macOS only, through `WinitPlatform::macos`: hearing a reopen, a window that slides in, hiding the app, placing a composition layer in a `CALayer` the app holds, showing the system's layer as one, and keeping the app from being throttled while it does work the user is waiting on (`MacOs::begin_user_activity`, with a `UserActivity` that ends the activity when dropped).

## Video decoders on Linux

A GPU's `video_decoders` (`Platform::gpu`, the render node GBM opens) is GStreamer's, handed H.264, HEVC, VP9 and AV1 (`src/os/linux/video_decoders/`). GStreamer is loaded at run time, as GBM is, so a build needs no development package and an app starts where it is missing: there, and where no render node opens, `video_decoders` is `None`. A stream runs through a pipeline of its own, `appsrc ! <decoder> ! <conversion to ARGB8888> ! appsink`, and each picture reaches the app as a dmabuf, Waft's hardware buffer on Linux.

What is there:

- On the video hardware, VA-API's decoders, `vah264dec`, `vah265dec`, `vavp9dec` after `vp9parse` (it takes no superframe) and `vaav1dec` after `av1parse` (it takes a frame at a time, not a temporal unit), their pictures converted on the GPU (`vapostproc`) to ARGB8888 and handed over as the decoder's own dmabufs, their DRM modifier carried over. On the processor, libav's `avdec_h264` (or OpenH264's `openh264dec` where libav is missing), `avdec_h265` and `avdec_vp9`, and dav1d's `dav1ddec` for AV1, of which gst-libav makes no decoder, their pictures converted by `videoconvert` and copied once into a linear dmabuf from a small pool Waft allocates with GBM.
- H.264 and HEVC framed after start codes (`stream-format=byte-stream`), their parameter sets in the stream or given ahead (put before each key frame), or after lengths with their `avcC` or `hvcC` record (`stream-format=avc` or `hvc1`, the record as `codec_data`); VP9 each chunk whole, a superframe and all (`video/x-vp9`), its `vpcC` record unused, as its key frames say what its decoder needs; AV1 each temporal unit (`stream-format=obu-stream,alignment=tu`), its `av1C` record as `codec_data` where the file gave one.
- Frames in the order they are shown, as GStreamer's decoders put them out, each with its chunk's time and duration, whatever times the chunks carry: a chunk is pushed with a number that its picture's buffer carries back (a reference timestamp). A frame's colour is RGB, full range, as its pixels are.
- Key frames: at the start, after a drain, a reset and a `configure`, a chunk that is not marked a key frame is `BadData` and not pushed.
- A drain lets out every picture through the end of the stream, whatever `frame_limit`, then runs the pipeline afresh; a reset runs it afresh at once, and drops the frames waiting and any picture of a chunk handed over before it; `configure` drains, then runs it afresh with the new setup. No more chunks are pushed, and no more pictures taken, while `frame_limit` frames wait unread.
- `support` answers from GStreamer's registry: software `Yes` with a decoder that decodes every profile of its codec (libav's of H.264 and VP9, dav1d), `Unknown` with the others (OpenH264, which decodes H.264's constrained baseline profile only, and libav's HEVC decoder), `No` with none; hardware `Unknown` where the codec's VA-API decoder (and its parser) is there, as the registry does not say which profiles the hardware takes, and `No` otherwise. `CodecChoice::Any` tries the hardware first.

What is not, and how an app sees it:

- No VP8: it is `Unsupported`, and `support` says `No`.
- No ten bits: a stream of ten bits to a sample (HEVC's Main 10, VP9's profile 2, AV1 at ten bits) is converted to ARGB8888 as every picture is, by `vapostproc` on the GPU or `videoconvert`, so the app has it in eight bits.
- The stream's own colour: pictures are converted to ARGB8888 before they reach the app, as Waft's Linux hardware buffers are single-plane ARGB8888, so a frame is not the decoder's NV12 picture as on Apple's systems.
- A damaged chunk is not told apart: GStreamer's decoders conceal what they cannot decode, so such a chunk is answered as taken. A pipeline that fails ends the decoder with `Failed`.
- `DecodingPurpose`, `largest_size` and `default_color` change nothing: the pipeline sizes itself from the stream.

The tests that decode need GStreamer with gst-libav and a render node, which Waft's CI runners lack, so they are ignored there: `cargo test -p waft-embedder-winit -- --ignored os::linux::video_decoders`.
