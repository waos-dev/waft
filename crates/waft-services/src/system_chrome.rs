//! Flutter `services/system_chrome.dart`: the application switcher description, the system
//! overlay style and what a lifecycle change does to it, and the system UI mode. The rest of
//! `SystemChrome` waits for the host capabilities it would talk to.
//!
//! The overlay style and the mode are a view's, sent to the bars over the view
//! ([`ViewHost::system_bars`](waft_embedder::ViewHost::system_bars)), since each window carries
//! its own bars, where Flutter's are the app's. A view with no bars over it, a desktop's window,
//! takes neither, as Flutter's calls do nothing on a desktop. The invariant a later editor will break: the pending and the
//! latest style are kept per view, so one view's style never stands for, or hides, another's.

use std::collections::BTreeMap;

use waft_embedder::{
    AppLifecycleState, ApplicationSwitcherDescription, EdgeSwipe, SystemUiOverlayStyle, ViewId,
};
use waft_foundation::{App, Handle, Listener};

/// Specifies a set of mutually exclusive system UI overlays.
///
/// Used by [`SystemChrome`] to specify the set of overlays that should be visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SystemUiOverlay {
    /// The status bar provided by the embedder on the top of the application
    /// surface, if any.
    Top,

    /// The status bar provided by the embedder on the bottom of the application
    /// surface, if any.
    Bottom,
}

/// Describes different display configurations for both Android and iOS.
///
/// These modes mimic Android-specific display setups. Used by
/// [`SystemChrome::set_enabled_system_ui_mode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SystemUiMode {
    /// Fullscreen display with status and navigation bars presentable by tapping anywhere on the
    /// display. Android no longer has a tap that brings them back: here a swipe from an edge
    /// does, as in [`SystemUiMode::Immersive`].
    LeanBack,
    /// Fullscreen display with status and navigation bars presentable through a swipe gesture at
    /// the edges of the display.
    Immersive,
    /// Fullscreen display with status and navigation elements rendered over the application:
    /// a swipe at an edge shows them for a moment, and the application keeps that touch.
    ImmersiveSticky,
    /// Fullscreen display with status and navigation elements rendered over the application.
    EdgeToEdge,
    /// Declares manually configured [`SystemUiOverlay`]s.
    Manual,
}

/// Dart's `SystemChrome._pendingStyle` and `SystemChrome._latestStyle`, which are static
/// fields of a Dart class and one arena object per [`App`] here, each by view.
#[derive(Default)]
struct SystemChromeStyles {
    pending_style: BTreeMap<ViewId, SystemUiOverlayStyle>,
    latest_style: BTreeMap<ViewId, SystemUiOverlayStyle>,
}

/// Controls specific aspects of the operating system's graphical interface and
/// how it interacts with the application.
pub struct SystemChrome;

impl SystemChrome {
    /// Specifies the description of the current state of the application as it
    /// pertains to the application switcher (also known as "recent tasks").
    ///
    /// Any part of the description that is unsupported on the current platform
    /// will be ignored.
    pub fn set_application_switcher_description(
        app: &App,
        description: &ApplicationSwitcherDescription,
    ) {
        if let Some(chrome) = app.platform().system_chrome() {
            chrome.set_application_switcher_description(description);
        }
    }

    /// Specifies the style to use for the system overlays (e.g. the status bar on
    /// Android or iOS, the system navigation bar on Android) that are visible (if any) over
    /// `view`.
    ///
    /// This method will schedule the embedder update to be run in a microtask.
    /// Any subsequent calls to this method during the current event loop will
    /// overwrite the pending value, such that only the last specified value takes
    /// effect.
    ///
    /// Call this API in code whose lifecycle matches that of the desired
    /// system UI styles. For instance, to change the system UI style on a new
    /// page, consider calling when pushing/popping a new `PageRoute`.
    ///
    /// If a particular style is not supported on the platform, selecting it will
    /// have no effect.
    ///
    /// For more complex control of the system overlay styles, consider using
    /// an `AnnotatedRegion` widget instead of calling this method directly. That widget
    /// places a value directly into the layer tree where it can be hit-tested by the
    /// framework.
    pub fn set_system_ui_overlay_style(app: &mut App, view: ViewId, style: &SystemUiOverlayStyle) {
        let styles = app.singleton::<SystemChromeStyles>();
        if !app.get(styles).pending_style.is_empty() {
            // The microtask has already been queued; just update the pending value.
            app.get_mut(styles).pending_style.insert(view, *style);
            return;
        }
        if Some(style) == app.get(styles).latest_style.get(&view) {
            // Trivial success: no microtask has been queued and the given style is
            // already in effect, so no need to queue a microtask.
            return;
        }
        app.get_mut(styles).pending_style.insert(view, *style);
        app.schedule_microtask(Listener::new(move |app| {
            Self::send_pending_styles(app, styles)
        }));
    }

    /// The body of the microtask Dart's `setSystemUIOverlayStyle` queues, for each view a style
    /// is pending for.
    fn send_pending_styles(app: &mut App, styles: Handle<SystemChromeStyles>) {
        let pending = std::mem::take(&mut app.get_mut(styles).pending_style);
        debug_assert!(!pending.is_empty());
        for (view, style) in pending {
            if app.get(styles).latest_style.get(&view) == Some(&style) {
                continue;
            }
            if let Some(view) = app.platform().view(view)
                && let Some(bars) = view.host().system_bars()
            {
                bars.set_overlay_style(&style);
            }
            app.get_mut(styles).latest_style.insert(view, style);
        }
    }

    /// Specifies the [`SystemUiMode`] to have visible over `view` when the application is
    /// running (Dart's `SystemChrome.setEnabledSystemUIMode`), carried out at once by the bars
    /// over the view, as Dart's sends its message without waiting for a microtask.
    ///
    /// The `overlays` are read for [`SystemUiMode::Manual`] alone: both shown is
    /// [`SystemUiMode::EdgeToEdge`], none shown is [`SystemUiMode::Immersive`]; one alone is not
    /// supported, the bars showing or hiding together, and leaves them as they are.
    ///
    /// A view not in the app, gone, or with no bars over it is not told, as Flutter's call does
    /// nothing on a desktop.
    pub fn set_enabled_system_ui_mode(
        app: &App,
        view: ViewId,
        mode: SystemUiMode,
        overlays: &[SystemUiOverlay],
    ) {
        let Some(view) = app.platform().view(view) else {
            return;
        };
        let Some(bars) = view.host().system_bars() else {
            return;
        };
        match asked_of_the_bars(mode, overlays) {
            BarsAsked::Show => bars.show(),
            BarsAsked::Hide(swipe) => bars.hide(swipe),
            BarsAsked::Nothing => {}
        }
    }

    /// Called by the binding during a transition to a new app lifecycle state.
    pub fn handle_app_lifecycle_state_changed(app: &mut App, state: AppLifecycleState) {
        // When the app is detached, clear the record of the style sent to the host
        // so that it will be sent again when the app is reattached.
        if state == AppLifecycleState::Detached {
            let styles = app.singleton::<SystemChromeStyles>();
            app.schedule_microtask(Listener::new(move |app| {
                app.get_mut(styles).latest_style.clear();
            }));
        }
    }

    /// The last style that was set over `view` using
    /// [`set_system_ui_overlay_style`](SystemChrome::set_system_ui_overlay_style).
    pub fn latest_style(app: &mut App, view: ViewId) -> Option<SystemUiOverlayStyle> {
        let styles = app.singleton::<SystemChromeStyles>();
        app.get(styles).latest_style.get(&view).copied()
    }
}

/// What a mode asks of the bars over a view.
enum BarsAsked {
    Show,
    Hide(EdgeSwipe),
    /// What they cannot do: a manual mode with one overlay alone, the bars showing or hiding
    /// together.
    Nothing,
}

fn asked_of_the_bars(mode: SystemUiMode, overlays: &[SystemUiOverlay]) -> BarsAsked {
    match mode {
        SystemUiMode::EdgeToEdge => BarsAsked::Show,
        SystemUiMode::LeanBack | SystemUiMode::Immersive => BarsAsked::Hide(EdgeSwipe::System),
        SystemUiMode::ImmersiveSticky => BarsAsked::Hide(EdgeSwipe::Deferred),
        SystemUiMode::Manual => {
            let top = overlays.contains(&SystemUiOverlay::Top);
            let bottom = overlays.contains(&SystemUiOverlay::Bottom);
            match (top, bottom) {
                (true, true) => BarsAsked::Show,
                (false, false) => BarsAsked::Hide(EdgeSwipe::System),
                _ => BarsAsked::Nothing,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use waft_foundation::AppCell;

    use waft_embedder::{Brightness, Color, TargetPlatform};
    use waft_test::{BarsShown, IMPLICIT_VIEW_ID, TestPlatform, TestView};

    use super::*;

    const VIEW: ViewId = IMPLICIT_VIEW_ID;
    const OTHER_VIEW: ViewId = ViewId(1);

    /// An app with two views, whose hosts record the overlay styles they are handed.
    fn recording_app() -> (Rc<TestPlatform>, Rc<AppCell>) {
        let platform = Rc::new(
            TestPlatform::new()
                .on(TargetPlatform::IOS)
                .with_views(&[VIEW, OTHER_VIEW]),
        );
        (Rc::clone(&platform), AppCell::with_platform(platform))
    }

    /// The styles `view`'s host was handed, oldest first.
    fn styles_of(platform: &TestPlatform, view: ViewId) -> Vec<SystemUiOverlayStyle> {
        let view: Rc<TestView> = platform.test_view(view);
        view.test_host().overlay_styles()
    }

    #[test]
    fn light_and_dark_differ_only_in_the_two_status_bar_brightnesses() {
        assert_eq!(
            SystemUiOverlayStyle::LIGHT.status_bar_icon_brightness,
            Some(Brightness::Light)
        );
        assert_eq!(
            SystemUiOverlayStyle::LIGHT.status_bar_brightness,
            Some(Brightness::Dark)
        );
        assert_eq!(
            SystemUiOverlayStyle::DARK.status_bar_icon_brightness,
            Some(Brightness::Dark)
        );
        assert_eq!(
            SystemUiOverlayStyle::DARK.status_bar_brightness,
            Some(Brightness::Light)
        );
        assert_eq!(
            SystemUiOverlayStyle::LIGHT.system_navigation_bar_color,
            SystemUiOverlayStyle::DARK.system_navigation_bar_color
        );
        assert_ne!(SystemUiOverlayStyle::LIGHT, SystemUiOverlayStyle::DARK);
    }

    #[test]
    fn copy_with_replaces_only_what_it_is_given() {
        let style = SystemUiOverlayStyle::LIGHT
            .copy_with()
            .status_bar_color(Color::new(0xFF112233));
        assert_eq!(style.status_bar_color, Some(Color::new(0xFF112233)));
        assert_eq!(
            style.status_bar_icon_brightness,
            SystemUiOverlayStyle::LIGHT.status_bar_icon_brightness
        );
        assert_eq!(SystemUiOverlayStyle::LIGHT.status_bar_color, None);
    }

    /// How the bars over `view` were hidden or shown, oldest first.
    fn bars_of(platform: &TestPlatform, view: ViewId) -> Vec<BarsShown> {
        let view: Rc<TestView> = platform.test_view(view);
        view.test_host().bars_shown()
    }

    #[test]
    fn each_mode_hides_or_shows_the_bars_of_its_view_alone() {
        let (platform, cell) = recording_app();
        let app = cell.borrow();
        let modes = [
            (SystemUiMode::ImmersiveSticky, vec![]),
            (SystemUiMode::Immersive, vec![]),
            (SystemUiMode::LeanBack, vec![]),
            (SystemUiMode::EdgeToEdge, vec![]),
            (SystemUiMode::Manual, vec![]),
            (SystemUiMode::Manual, vec![SystemUiOverlay::Top]),
            (
                SystemUiMode::Manual,
                vec![SystemUiOverlay::Top, SystemUiOverlay::Bottom],
            ),
        ];
        for (mode, overlays) in modes {
            SystemChrome::set_enabled_system_ui_mode(&app, VIEW, mode, &overlays);
        }
        assert_eq!(
            bars_of(&platform, VIEW),
            [
                BarsShown::Hidden(EdgeSwipe::Deferred),
                BarsShown::Hidden(EdgeSwipe::System),
                BarsShown::Hidden(EdgeSwipe::System),
                BarsShown::Shown,
                BarsShown::Hidden(EdgeSwipe::System),
                BarsShown::Shown,
            ],
            "one overlay alone leaves the bars as they are"
        );
        assert!(
            bars_of(&platform, OTHER_VIEW).is_empty(),
            "the other view is not told"
        );
    }

    /// `system_chrome_test.dart`: `SystemChrome overlay style test`.
    #[test]
    fn only_the_last_style_of_a_turn_reaches_the_host() {
        let (platform, cell) = recording_app();
        let mut app = cell.borrow_mut();
        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::LIGHT);
        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::DARK);
        assert!(
            styles_of(&platform, VIEW).is_empty(),
            "nothing is sent before the microtask runs"
        );
        assert_eq!(SystemChrome::latest_style(&mut app, VIEW), None);

        app.drain_microtasks();
        assert_eq!(
            styles_of(&platform, VIEW).as_slice(),
            [SystemUiOverlayStyle::DARK]
        );
        assert_eq!(
            SystemChrome::latest_style(&mut app, VIEW),
            Some(SystemUiOverlayStyle::DARK)
        );
    }

    #[test]
    fn a_style_already_in_effect_queues_nothing() {
        let (platform, cell) = recording_app();
        let mut app = cell.borrow_mut();
        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::LIGHT);
        app.drain_microtasks();

        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::LIGHT);
        app.drain_microtasks();
        assert_eq!(
            styles_of(&platform, VIEW).as_slice(),
            [SystemUiOverlayStyle::LIGHT],
            "an unchanged style does not reach the host again"
        );

        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::DARK);
        app.drain_microtasks();
        assert_eq!(
            styles_of(&platform, VIEW).as_slice(),
            [SystemUiOverlayStyle::LIGHT, SystemUiOverlayStyle::DARK]
        );
    }

    /// `system_chrome_test.dart`: the end of `SystemChrome overlay style test`.
    #[test]
    fn the_style_is_sent_again_after_the_app_was_detached() {
        let (platform, cell) = recording_app();
        let mut app = cell.borrow_mut();
        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::LIGHT);
        app.drain_microtasks();
        assert_eq!(styles_of(&platform, VIEW).len(), 1);

        // The state should be sent again if the app was detached.
        SystemChrome::handle_app_lifecycle_state_changed(&mut app, AppLifecycleState::Detached);
        app.drain_microtasks();
        SystemChrome::handle_app_lifecycle_state_changed(&mut app, AppLifecycleState::Resumed);
        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::LIGHT);
        app.drain_microtasks();
        assert_eq!(styles_of(&platform, VIEW).len(), 2);
    }

    #[test]
    fn a_style_that_returns_to_the_latest_within_the_turn_sends_nothing() {
        let (platform, cell) = recording_app();
        let mut app = cell.borrow_mut();
        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::LIGHT);
        app.drain_microtasks();

        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::DARK);
        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::LIGHT);
        app.drain_microtasks();
        assert_eq!(
            styles_of(&platform, VIEW).as_slice(),
            [SystemUiOverlayStyle::LIGHT]
        );
        assert_eq!(
            SystemChrome::latest_style(&mut app, VIEW),
            Some(SystemUiOverlayStyle::LIGHT)
        );
    }

    #[test]
    fn each_view_keeps_its_own_style() {
        let (platform, cell) = recording_app();
        let mut app = cell.borrow_mut();
        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::LIGHT);
        SystemChrome::set_system_ui_overlay_style(
            &mut app,
            OTHER_VIEW,
            &SystemUiOverlayStyle::DARK,
        );
        app.drain_microtasks();
        assert_eq!(
            styles_of(&platform, VIEW).as_slice(),
            [SystemUiOverlayStyle::LIGHT]
        );
        assert_eq!(
            styles_of(&platform, OTHER_VIEW).as_slice(),
            [SystemUiOverlayStyle::DARK]
        );

        SystemChrome::set_system_ui_overlay_style(&mut app, VIEW, &SystemUiOverlayStyle::LIGHT);
        app.drain_microtasks();
        assert_eq!(
            styles_of(&platform, VIEW).len(),
            1,
            "a style in effect over its view is not sent again for another view's"
        );
    }
}
