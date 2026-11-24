//! A window on macOS shows the backdrop the app sets behind its content, AppKit's blur of what
//! lies behind the window: one view, behind the content; setting another replaces it, and setting
//! none leaves the window with none.
//!
//! Its own binary, without the test harness: AppKit's event loop runs on the main thread only. It
//! opens a window for about a second, and needs a login session's window server.

#[cfg(target_os = "macos")]
mod support;

#[cfg(target_os = "macos")]
const TEST: &str = "a_window_shows_one_backdrop_behind_its_content";

/// Long enough for every step, many times over.
#[cfg(target_os = "macos")]
const GIVE_UP_AFTER: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(target_os = "macos")]
fn main() {
    use waft_embedder_winit::WinitEmbedder;

    support::fail_after(TEST, GIVE_UP_AFTER, || {
        format!("the steps did not end in {GIVE_UP_AFTER:?}")
    });
    WinitEmbedder::default().run(client::BackdropClient::new);
}

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
mod client {
    use std::time::Duration;

    use waft_embedder::{
        EmbedderClient, Frame, KeyData, PlatformRef, PointerReport, RequestId, ViewId,
        WindowBackground, WindowConfig, WindowRef,
    };
    use waft_embedder_winit::{
        MacosBackdrop, NSVisualEffectMaterial, NSVisualEffectState, WinitPlatform,
    };

    use crate::TEST;
    use crate::support::{self, appkit};

    const TITLE: &str = "a window with a backdrop";

    /// Opens a see-through window and, once it is open, sets a backdrop, another, and none,
    /// counting after each the backdrops AppKit has behind the window's content.
    pub(crate) struct BackdropClient {
        platform: PlatformRef,
        requested: Vec<RequestId>,
        windows: Vec<WindowRef>,
    }

    impl BackdropClient {
        pub(crate) fn new(platform: PlatformRef) -> BackdropClient {
            let config = WindowConfig {
                background: WindowBackground::Transparent,
                ..support::titled(TITLE)
            };
            let requested = vec![support::request_window(&platform, config)];
            BackdropClient {
                platform,
                requested,
                windows: Vec::new(),
            }
        }

        fn set_backdrop(&self, backdrop: Option<&MacosBackdrop>) {
            let macos = self
                .platform
                .downcast_ref::<WinitPlatform>()
                .and_then(WinitPlatform::macos)
                .unwrap_or_else(|| support::fail(TEST, "the host on macOS offers macOS's own"));
            macos.set_window_backdrop(&self.windows[0], backdrop);
        }

        fn expect(&self, step: &str, backdrops: usize) {
            let counted = appkit::backdrops_behind_content(TITLE);
            println!("{step:<40} backdrops behind the content: {counted}");
            if counted != backdrops {
                support::fail(TEST, &format!("{step}: expected {backdrops}"));
            }
        }

        fn go_on(&mut self) {
            support::take_opened_windows(&self.platform, &mut self.requested, &mut self.windows);
            if self.windows.is_empty() {
                return;
            }
            self.expect("opened", 0);
            let blur = |material, corner_radius| MacosBackdrop::VisualEffect {
                material,
                state: NSVisualEffectState::Active,
                corner_radius,
            };
            self.set_backdrop(Some(&blur(NSVisualEffectMaterial::Popover, 12.0)));
            self.expect("a backdrop set", 1);
            self.set_backdrop(Some(&blur(NSVisualEffectMaterial::HUDWindow, 0.0)));
            self.expect("another set in its place", 1);
            self.set_backdrop(None);
            self.expect("none set", 0);
            support::pass(TEST);
        }
    }

    impl EmbedderClient for BackdropClient {
        fn wake(&mut self, _elapsed: Duration) {
            self.go_on();
        }

        fn view_added(&mut self, _id: ViewId) {
            self.go_on();
        }

        fn frame(&mut self, _views: &[ViewId], _frame: Frame) {}
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
