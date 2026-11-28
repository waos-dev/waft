//! A window the app created, as it holds it (`HostWindow`), and how it was when the app last heard
//! of it: whether it was minimized, occluded and full screen, so that the embedder reports a
//! change once, whichever of the system's events it comes with.
//!
//! Whether it is maximized is not among them: winit answers that for a window without decorations
//! by giving it a title bar for a moment, which the system reports as moves and resizes, so
//! reading it at each report would never stop. A window maximized or returned moves, and the app
//! hears of that.
//!
//! The handle keeps what the app asked about showing the window. Where the system shows a window
//! at its view's first frame (`WindowChrome::SHOWS_WINDOW_AT_FIRST_FRAME`, Windows), the window is
//! made hidden and shows once that frame is presented, as Flutter's Windows embedder has it; until
//! then `show` and `hide` only say whether it shows then, and from then on they act at once. A
//! window made with `visible: false` stays hidden until the app shows it. There is no timeout: a
//! view that never presents a frame keeps its window hidden, as in Flutter.
//!
//! The invariant a later editor could break: frames follow whether a view is in sight, and a view
//! out of sight is served none (`frame_scheduler.rs`), so a window held hidden for its first frame
//! must count as in sight. Its view is about to be shown and is served that frame; only the
//! system's window stays hidden until it is presented. Were it out of sight, it would wait for a
//! frame it is never served, and never show.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Weak;
use std::time::Duration;

use waft_embedder::{
    HostWindow, IconImage, RawWindowHandle, Rect, ScreenId, Size, TitleBarAction, ViewRef,
    WindowSizeLimits,
};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::icon::{Icon, RgbaIcon};
use winit::monitor::{Fullscreen, MonitorHandle};
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{Window, WindowId};

use crate::app_requests::{AppRequest, AppRequests, WindowRequest};
use crate::os::System;
use crate::os::capabilities::{Screens, WindowChrome, WindowMotion, WindowVisibility};
use crate::view::WinitView;

/// A created window as the app holds it. Holds the window weakly: the embedder's registry owns it,
/// and closing it there frees it (`windows.rs`); from then on a call here changes nothing.
pub(crate) struct WinitWindow {
    id: WindowId,
    window: Weak<dyn Window>,
    showing: Cell<Showing>,
    view: Rc<WinitView>,
    requests: Rc<AppRequests>,
    /// winit's last `Occluded` event: whether the window is occluded where the system cannot be
    /// asked.
    occluded: Cell<bool>,
    /// How the window was when the app last heard of it.
    reported: Cell<WindowState>,
}

/// When the app's asking to show or hide the window acts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Showing {
    /// The view's first frame is not presented yet: the window is hidden, and shows then if the
    /// app last asked for it `visible`.
    AtFirstFrame { visible: bool },
    /// At once: the first frame was presented, or the system shows a window as it is made.
    AtOnce,
}

/// Whether a window is minimized, occluded and full screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct WindowState {
    minimized: bool,
    occluded: bool,
    fullscreen: bool,
}

impl WinitWindow {
    /// The handle on `window`, made `visible` or not as the app asked: shown already, or to show
    /// at its view's first frame where the system shows a window then.
    pub(crate) fn new(
        id: WindowId,
        window: Weak<dyn Window>,
        visible: bool,
        view: Rc<WinitView>,
        requests: Rc<AppRequests>,
    ) -> WinitWindow {
        let showing = if System::SHOWS_WINDOW_AT_FIRST_FRAME {
            Showing::AtFirstFrame { visible }
        } else {
            Showing::AtOnce
        };
        let window = WinitWindow {
            id,
            window,
            showing: Cell::new(showing),
            view,
            requests,
            occluded: Cell::new(false),
            reported: Cell::default(),
        };
        window.reported.set(window.state());
        window
    }

    /// The view's first frame was presented: a window waiting for it shows if the app asked for
    /// it, activating as its configuration says, and the app's asking acts at once from now on.
    pub(crate) fn first_frame_presented(&self) {
        if let Showing::AtFirstFrame { visible } = self.showing.replace(Showing::AtOnce) {
            self.set_visible(visible);
        }
    }

    /// Shows or hides the window, as the app asks: at once, or at its view's first frame.
    fn ask_visible(&self, visible: bool) {
        match self.showing.get() {
            Showing::AtFirstFrame { .. } => self.showing.set(Showing::AtFirstFrame { visible }),
            Showing::AtOnce => self.set_visible(visible),
        }
    }

    fn set_visible(&self, visible: bool) {
        if let Some(window) = self.window.upgrade() {
            window.set_visible(visible);
        }
    }

    /// winit says the window is occluded, or shown: what it answers where the system cannot be
    /// asked.
    pub(crate) fn set_occluded(&self, occluded: bool) {
        self.occluded.set(occluded);
    }

    /// Reads again whether the window is minimized, occluded and full screen, and answers whether
    /// any changed since the app last heard of the window, which it is to hear of now.
    pub(crate) fn refresh_state(&self) -> bool {
        let state = self.state();
        self.reported.replace(state) != state
    }

    fn state(&self) -> WindowState {
        WindowState {
            minimized: self.is_minimized(),
            occluded: self.is_occluded(),
            fullscreen: self.is_fullscreen(),
        }
    }
}

impl HostWindow for WinitWindow {
    fn view(&self) -> ViewRef {
        let view: ViewRef = self.view.clone();
        view
    }

    fn frame(&self) -> Rect {
        let Some(window) = self.window.upgrade() else {
            return Rect::from_ltwh(0.0, 0.0, 0.0, 0.0);
        };
        let scale = window.scale_factor();
        let position = window
            .outer_position()
            .map(|position| position.to_logical::<f64>(scale))
            .unwrap_or(LogicalPosition::new(0.0, 0.0));
        let size = window.outer_size().to_logical::<f64>(scale);
        Rect::from_ltwh(position.x, position.y, size.width, size.height)
    }

    fn set_frame(&self, frame: Rect, animate: Option<Duration>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        if let Some(duration) = animate
            && System::animate_frame(&*window, frame, duration)
        {
            return;
        }
        if System::set_frame(&*window, frame) {
            return;
        }
        // Size before position: a system that anchors a window at its bottom-left, as
        // AppKit does, places the top edge from the size the window has at the time.
        let _ = window.request_surface_size(LogicalSize::new(frame.width(), frame.height()).into());
        window.set_outer_position(LogicalPosition::new(frame.left, frame.top).into());
    }

    fn set_title(&self, title: &str) {
        if let Some(window) = self.window.upgrade() {
            window.set_title(title);
        }
    }

    /// winit's window icon, the title bar's on Windows, and the taskbar's beside it; the window
    /// manager's on X11 and on a Wayland compositor with `xdg_toplevel_icon`; nothing on macOS,
    /// whose windows show the app's icon alone.
    fn set_icon(&self, icon: Option<&IconImage>) {
        if let Some(window) = self.window.upgrade() {
            window.set_window_icon(window_icon(icon));
            System::set_taskbar_icon(&*window, icon);
        }
    }

    fn show(&self) {
        self.ask_visible(true);
    }

    fn hide(&self) {
        self.ask_visible(false);
    }

    /// winit's answer, the system's: never on Wayland, which does not tell a client.
    fn is_minimized(&self) -> bool {
        self.window
            .upgrade()
            .and_then(|window| window.is_minimized())
            .unwrap_or(false)
    }

    /// The system's answer where it can be asked, as AppKit can; winit's last word otherwise,
    /// which Windows and Wayland never give.
    fn is_occluded(&self) -> bool {
        let Some(window) = self.window.upgrade() else {
            return false;
        };
        System::is_occluded(&*window).unwrap_or(self.occluded.get())
    }

    fn close(&self) {
        // Off screen at once, and not shown by a first frame presented meanwhile; the embedder
        // drops the window when it next turns, and the framework lets go of the view when its
        // `View` widget goes.
        self.ask_visible(false);
        self.requests
            .send(AppRequest::Window(WindowRequest::Close(self.id)));
    }

    /// Every window the app asks for is a desktop's, which its user resizes.
    fn size_limits(&self) -> Option<&dyn WindowSizeLimits> {
        Some(self)
    }

    /// winit's, which starts the system's own move from the press the system is handling; none
    /// starts outside a press, as there is nothing to move with.
    fn drag(&self) {
        if let Some(window) = self.window.upgrade() {
            let _ = window.drag_window();
        }
    }

    /// Never while the window is full screen, which AppKit calls zoomed too: the two are states
    /// an app tells apart.
    fn is_maximized(&self) -> bool {
        self.window
            .upgrade()
            .is_some_and(|window| window.fullscreen().is_none() && window.is_maximized())
    }

    fn set_maximized(&self, maximized: bool) {
        if let Some(window) = self.window.upgrade() {
            window.set_maximized(maximized);
        }
    }

    fn set_minimized(&self, minimized: bool) {
        if let Some(window) = self.window.upgrade() {
            window.set_minimized(minimized);
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.window
            .upgrade()
            .is_some_and(|window| window.fullscreen().is_some())
    }

    /// winit's borderless full screen, the system's own: a Space of its own on macOS, the
    /// compositor's state on Wayland and X11, a window over the whole monitor on Windows.
    fn set_fullscreen(&self, fullscreen: bool, screen: Option<ScreenId>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let monitor = screen.and_then(|screen| monitor_numbered(&*window, screen));
        window.set_fullscreen(fullscreen.then_some(Fullscreen::Borderless(monitor)));
    }

    fn title_bar_double_click_action(&self) -> TitleBarAction {
        System::title_bar_double_click_action()
    }

    fn native_handle(&self) -> Option<RawWindowHandle> {
        let window = self.window.upgrade()?;
        window.window_handle().ok().map(|handle| handle.as_raw())
    }
}

/// winit's limits on the window's surface, which every desktop it serves keeps: AppKit's content
/// minimum and maximum, Windows' tracking sizes, X11's size hints and Wayland's toplevel's.
impl WindowSizeLimits for WinitWindow {
    fn set_min_size(&self, size: Option<Size>) {
        if let Some(window) = self.window.upgrade() {
            window.set_min_surface_size(size.map(logical_size));
        }
    }

    fn set_max_size(&self, size: Option<Size>) {
        if let Some(window) = self.window.upgrade() {
            window.set_max_surface_size(size.map(logical_size));
        }
    }
}

/// The monitor the app knows as `screen`, among those `window` could be on; none once the
/// screen is gone.
fn monitor_numbered(window: &dyn Window, screen: ScreenId) -> Option<MonitorHandle> {
    window
        .available_monitors()
        .find(|monitor| System::screen_id(monitor) == screen)
}

fn logical_size(size: Size) -> winit::dpi::Size {
    LogicalSize::new(size.width(), size.height()).into()
}

/// The window icon winit shows for `icon`, a window's own: the app's where it names none, as
/// [`WindowConfig::icon`](waft_embedder::WindowConfig::icon) has it, or where its pixels do not
/// fill its size.
pub(crate) fn window_icon(icon: Option<&IconImage>) -> Option<Icon> {
    icon.and_then(winit_icon).or_else(System::app_icon)
}

/// `image` as winit's window icon; none for an image whose pixels do not fill its size.
pub(crate) fn winit_icon(image: &IconImage) -> Option<Icon> {
    match RgbaIcon::new(image.rgba.to_vec(), image.width, image.height) {
        Ok(icon) => Some(icon.into()),
        Err(error) => {
            eprintln!("waft: a window icon was dropped: {error}");
            None
        }
    }
}
