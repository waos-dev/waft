//! The system's video decoders on macOS and iOS: VideoToolbox. In the making: what is here is
//! the framing VideoToolbox takes (`nal`), what a stream's parameter sets say (`stream`) and the
//! order its pictures are shown in (`presentation_order`); the decoder itself follows.

// Used by the decoder, which is not written yet.
#![allow(dead_code)]

mod nal;
mod presentation_order;
mod session;
mod stream;
