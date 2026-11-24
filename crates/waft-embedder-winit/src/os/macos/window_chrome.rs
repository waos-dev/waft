//! What a window's configuration asks of AppKit beyond winit's attributes: its shadow, its keys
//! offered to the framework before the menu bar, presence on every Space, the glass or blur behind a see-through window, and, for a window that never
//! becomes key, the pointer's moves.

use objc2::AnyThread;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSGlassEffectViewStyle, NSTrackingArea, NSTrackingAreaOptions, NSVisualEffectMaterial,
    NSVisualEffectState, NSWindowCollectionBehavior,
};
use waft_embedder::{MacosTitleBar, TitleBarAction, WindowBackground, WindowConfig};
use winit::event_loop::ActiveEventLoop;
use winit::platform::macos::WindowAttributesMacOS;
use winit::window::{Window, WindowAttributes};

use super::{AppKit, MacosBackdrop, menus, ns_view, ns_window, set_backdrop, title_bar};
use crate::LinuxSettings;
use crate::os::capabilities::WindowChrome;

impl WindowChrome for AppKit {
    /// What winit has an AppKit attribute for: the shadow, and a hidden title bar's view under a
    /// transparent, untitled title bar.
    fn extend_attributes(
        _event_loop: &dyn ActiveEventLoop,
        attributes: WindowAttributes,
        config: &WindowConfig,
        _linux: &LinuxSettings,
    ) -> WindowAttributes {
        let mut macos = WindowAttributesMacOS::default().with_has_shadow(config.shadow);
        if let MacosTitleBar::Hidden { .. } = config.macos.title_bar {
            macos = macos
                .with_fullsize_content_view(true)
                .with_titlebar_transparent(true)
                .with_title_hidden(true);
        }
        attributes.with_platform_attributes(Box::new(macos))
    }

    /// The window's keys going to the framework before the menu bar, pointer tracking for a window
    /// that never activates, presence on every Space, the blur or glass behind a see-through
    /// window, and a hidden title bar's buttons in its band.
    /// System Settings' choice (`title_bar`).
    fn title_bar_double_click_action() -> TitleBarAction {
        title_bar::double_click_action()
    }

    fn configure(window: &dyn Window, config: &WindowConfig) {
        menus::app_window_opened(window);
        if let MacosTitleBar::Hidden { height } = config.macos.title_bar {
            title_bar::put_buttons_in_band(window, height);
        }
        if !config.activating {
            track_pointer_always(window);
        }
        if config.all_desktops {
            join_all_spaces(window);
        }
        if matches!(
            config.background,
            WindowBackground::Blurred | WindowBackground::Glass
        ) {
            set_backdrop(window, Some(&backdrop_of(config.background)));
        }
    }
}

/// Has AppKit report the pointer's moves to the window's view whether or not the window is key.
///
/// winit tracks the mouse with a tracking rect, which reports entry and exit alone, and
/// takes moves from `acceptsMouseMovedEvents`, which AppKit honours for the key window
/// only. A window that never activates is never key, so without this it would hear the
/// pointer arrive and leave but never move: no hover, and clicks at stale positions.
/// Flutter's macOS embedder offers the same as its `always` mouse tracking mode. The
/// view is the owner, so winit's own handlers receive what the area reports.
fn track_pointer_always(window: &dyn Window) {
    let Some(view) = ns_view(window) else {
        return;
    };
    let owner: &AnyObject = view.as_ref();
    // SAFETY: the options name no user info, and the view outlives the area it owns.
    let area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            view.bounds(),
            NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect,
            Some(owner),
            None,
        )
    };
    view.addTrackingArea(&area);
}

/// Shows the window on every Space, a full-screen app's included, and leaves it in place when
/// Mission Control spreads the windows out.
fn join_all_spaces(window: &dyn Window) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
}

/// The backdrop a see-through window's `background` asks for: Liquid Glass where the system has
/// it (macOS 26), else the blur, as a popover's.
fn backdrop_of(background: WindowBackground) -> MacosBackdrop {
    if background == WindowBackground::Glass && MacosBackdrop::has_glass() {
        return MacosBackdrop::Glass {
            style: NSGlassEffectViewStyle::Regular,
            tint: None,
            corner_radius: 0.0,
        };
    }
    MacosBackdrop::VisualEffect {
        material: NSVisualEffectMaterial::Popover,
        state: NSVisualEffectState::Active,
        corner_radius: 0.0,
    }
}
