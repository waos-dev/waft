//! Android's video decoders as a Waft app has them (their GPU's, `Platform::gpu`): MediaCodec handed
//! a short H.264 clip, and a check of each thing a decoder does, shown with the first picture
//! decoded and written to the system's log under the tag `android_video_decoders`, so they can be
//! checked without looking: `adb logcat -s android_video_decoders`.
//!
//! ```text
//! cargo waft run -d emulator-5554 -p android_video_decoders
//! ```
//!
//! The clip, under `media/`, is the one Apple's decoder is checked with: twelve pictures of 320 by
//! 240 in the High profile at thirty a second, FFmpeg's test pattern, each picture decoded up to
//! two ahead of ones shown before it, and each after an access unit delimiter.
//!
//! Elsewhere it says so and quits: the decoders it checks are Android's.
#![feature(arbitrary_self_types)]

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
mod checks;
#[cfg(target_os = "android")]
mod clip;
#[cfg(target_os = "android")]
mod log;

#[waft::main]
fn main(app: &mut waft::App) {
    #[cfg(target_os = "android")]
    android::run(app);
    #[cfg(not(target_os = "android"))]
    elsewhere(app);
}

/// Says that the example is Android's, and quits.
#[cfg(not(target_os = "android"))]
fn elsewhere(app: &mut waft::App) {
    eprintln!("android_video_decoders: the decoders this checks are Android's; run it there");
    if let Some(app_exit) = app.platform().app_exit() {
        app_exit.quit(0);
    }
}
