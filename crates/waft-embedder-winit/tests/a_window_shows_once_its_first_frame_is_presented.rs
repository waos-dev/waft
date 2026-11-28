//! A Windows window shows once its view's first frame is presented, and not before, so it never
//! shows empty: while it waits, hidden, its view counts as in sight and is served that frame.
//!
//! Its own binary, without the test harness: winit makes one event loop per process. It opens a
//! window for a moment, and needs a desktop session.

// A picture's display list nests auto-trait obligations past the default depth.
#![recursion_limit = "256"]

#[cfg(target_os = "windows")]
mod support;

#[cfg(target_os = "windows")]
const TEST: &str = "a_window_shows_once_its_first_frame_is_presented";

/// Long enough for the window to open and its first frame to be drawn, many times over.
#[cfg(target_os = "windows")]
const GIVE_UP_AFTER: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(target_os = "windows")]
fn main() {
    use waft_embedder_winit::WinitEmbedder;

    support::fail_after(TEST, GIVE_UP_AFTER, || {
        format!("the window's view was served no frame within {GIVE_UP_AFTER:?}")
    });
    WinitEmbedder::default().run(client::FirstFrameClient::new);
}

#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
mod client {
    use std::sync::Arc;
    use std::time::Duration;

    use waft_embedder::{
        Canvas, EmbedderClient, Frame, KeyData, Offset, Paint, PlatformRef, PointerReport, Rect,
        RequestId, Scene, SceneBuilder, ViewId, WindowRef,
    };

    use crate::TEST;
    use crate::support::{self, win32};

    const TITLE: &str = "shown at its first frame";

    /// Asks for a window, and a frame of its view once it is made; at that frame, checks that the
    /// window is still hidden, presents a square, and checks that the window shows.
    pub(crate) struct FirstFrameClient {
        platform: PlatformRef,
        requested: Vec<RequestId>,
        windows: Vec<WindowRef>,
    }

    impl FirstFrameClient {
        pub(crate) fn new(platform: PlatformRef) -> FirstFrameClient {
            let request_id = support::request_window(&platform, support::titled(TITLE));
            FirstFrameClient {
                platform,
                requested: vec![request_id],
                windows: Vec::new(),
            }
        }

        /// Asks for the first frame once the window is made.
        fn ask_for_a_frame_once_the_window_is_made(&mut self) {
            let was_made = !self.windows.is_empty();
            support::take_opened_windows(&self.platform, &mut self.requested, &mut self.windows);
            if let Some(window) = self.windows.first()
                && !was_made
            {
                self.platform.request_frame(window.view().id());
            }
        }
    }

    /// A scene with a square in it.
    fn a_square() -> Arc<Scene> {
        let mut canvas = Canvas::new();
        canvas.draw_rect(Rect::from_ltwh(20.0, 20.0, 100.0, 100.0), &Paint::default());
        let mut scene = SceneBuilder::new();
        scene.add_picture(Offset::ZERO, &Arc::new(canvas.build()), false, false);
        Arc::new(scene.build())
    }

    impl EmbedderClient for FirstFrameClient {
        fn frame(&mut self, _views: &[ViewId], _frame: Frame) {
            let Some(window) = self.windows.first() else {
                return;
            };
            if win32::is_shown(window) {
                support::fail(TEST, "the window showed before its first frame");
            }
            window.view().present(a_square());
            if !win32::is_shown(window) {
                support::fail(
                    TEST,
                    "the window did not show once its first frame was presented",
                );
            }
            support::pass(TEST);
        }

        fn wake(&mut self, _elapsed: Duration) {
            self.ask_for_a_frame_once_the_window_is_made();
        }

        fn view_added(&mut self, _id: ViewId) {
            self.ask_for_a_frame_once_the_window_is_made();
        }
        fn view_metrics_changed(&mut self, _id: ViewId) {}
        fn view_removed(&mut self, _id: ViewId) {}
        fn pointer_reports(&mut self, _reports: Vec<PointerReport>) {}
        fn key_data(&mut self, _data: KeyData) -> bool {
            false
        }
        fn platform_brightness_changed(&mut self) {}
        fn locales_changed(&mut self) {}
    }
}
