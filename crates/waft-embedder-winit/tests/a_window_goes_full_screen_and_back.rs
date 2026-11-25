//! A window the app asks to go full screen does, and comes back as the app asks: it answers
//! whether it is full screen, never maximized while it is, and the app hears each change.
//!
//! Its own binary, without the test harness: AppKit's event loop runs on the main thread only. It
//! takes the screen for a few seconds, a Space of the window's own, and needs a login session's
//! window server.

#[cfg(target_os = "macos")]
mod support;

#[cfg(target_os = "macos")]
const TEST: &str = "a_window_goes_full_screen_and_back";

/// Long enough for every step, twice over.
#[cfg(target_os = "macos")]
const GIVE_UP_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(target_os = "macos")]
fn main() {
    use waft_embedder_winit::WinitEmbedder;

    support::fail_after(TEST, GIVE_UP_AFTER, || {
        format!("the steps did not end in {GIVE_UP_AFTER:?}")
    });
    WinitEmbedder::default().run(client::FullScreenClient::new);
}

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
mod client {
    use std::time::{Duration, Instant};

    use waft_embedder::{
        EmbedderClient, Frame, KeyData, PlatformRef, PointerReport, RequestId, ViewId,
        WindowConfig, WindowRef,
    };

    use crate::TEST;
    use crate::support;

    /// How long a step is given to settle before the window is asked: the system's move into a
    /// Space and out of it takes about half of it.
    const SETTLE: Duration = Duration::from_millis(2500);

    /// What a step does to the window, and how the app should find it.
    struct Step {
        name: &'static str,
        bring: fn(&WindowRef),
        fullscreen: bool,
        maximized: bool,
        /// Whether the app hears that the window changed in the step; `None` for the first, which
        /// the window may have been heard shown before.
        heard: Option<bool>,
    }

    const STEPS: [Step; 4] = [
        Step {
            name: "shown",
            bring: |_| {},
            fullscreen: false,
            maximized: false,
            heard: None,
        },
        Step {
            name: "full screen",
            bring: |window| window.set_fullscreen(true, None),
            fullscreen: true,
            maximized: false,
            heard: Some(true),
        },
        Step {
            name: "back from full screen",
            bring: |window| window.set_fullscreen(false, None),
            fullscreen: false,
            maximized: false,
            heard: Some(true),
        },
        Step {
            name: "left alone",
            bring: |_| {},
            fullscreen: false,
            maximized: false,
            heard: Some(false),
        },
    ];

    /// Opens a window and brings it through each step, asking it how it is once the step has
    /// settled and counting the changes the app hears of it. Wakes time the steps.
    pub(crate) struct FullScreenClient {
        platform: PlatformRef,
        requested: Vec<RequestId>,
        windows: Vec<WindowRef>,
        step: Option<usize>,
        step_began: Instant,
        changes_heard: u32,
        failures: Vec<String>,
    }

    impl FullScreenClient {
        pub(crate) fn new(platform: PlatformRef) -> FullScreenClient {
            let config = WindowConfig {
                title: "a window going full screen".into(),
                size: [400.0, 300.0],
                position: Some([300.0, 300.0]),
                ..WindowConfig::default()
            };
            let requested = vec![support::request_window(&platform, config)];
            FullScreenClient {
                platform,
                requested,
                windows: Vec::new(),
                step: None,
                step_began: Instant::now(),
                changes_heard: 0,
                failures: Vec::new(),
            }
        }

        /// The first step begins once the window is made.
        fn begin_once_the_window_is_made(&mut self) {
            support::take_opened_windows(&self.platform, &mut self.requested, &mut self.windows);
            if self.step.is_some() || self.windows.is_empty() {
                return;
            }
            self.step = Some(0);
            self.step_began = Instant::now();
            self.changes_heard = 0;
            support::wake_after_a_tick(&self.platform);
        }

        /// Judges the current step once it has settled, and brings the next. Answers whether every
        /// step is done.
        fn go_on(&mut self) -> bool {
            let Some(step) = self.step else {
                return false;
            };
            if self.step_began.elapsed() < SETTLE {
                return false;
            }
            self.judge(&STEPS[step]);
            let next = step + 1;
            let Some(next_step) = STEPS.get(next) else {
                return true;
            };
            self.step = Some(next);
            self.step_began = Instant::now();
            self.changes_heard = 0;
            (next_step.bring)(&self.windows[0]);
            false
        }

        fn judge(&mut self, step: &Step) {
            let window = &self.windows[0];
            let fullscreen = window.is_fullscreen();
            let maximized = window.is_maximized();
            let heard = self.changes_heard > 0;
            let as_expected = fullscreen == step.fullscreen
                && maximized == step.maximized
                && step.heard.is_none_or(|expected| heard == expected);
            println!(
                "{:<26} full screen {:<5} maximized {:<5} changes heard {}  {}",
                step.name,
                fullscreen,
                maximized,
                self.changes_heard,
                if as_expected { "ok" } else { "FAILED" }
            );
            if !as_expected {
                self.failures.push(step.name.to_owned());
            }
        }
    }

    impl EmbedderClient for FullScreenClient {
        fn frame(&mut self, _views: &[ViewId], _frame: Frame) {}

        fn wake(&mut self, _elapsed: Duration) {
            self.begin_once_the_window_is_made();
            if self.step.is_none() {
                return;
            }
            if !self.go_on() {
                support::wake_after_a_tick(&self.platform);
                return;
            }
            assert!(
                self.failures.is_empty(),
                "the window not as expected in: {:?}",
                self.failures
            );
            support::pass(TEST);
        }

        fn view_added(&mut self, _id: ViewId) {
            self.begin_once_the_window_is_made();
        }

        fn window_changed(&mut self, view: ViewId) {
            if self
                .windows
                .first()
                .is_some_and(|window| window.view().id() == view)
            {
                self.changes_heard += 1;
            }
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
