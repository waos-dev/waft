//! What a driver command does to a running app, as flutter_driver's `LiveWidgetController` does:
//! it waits on the app's frames, and sends pointer events into the gesture binding, the way a
//! host's touches arrive.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use waft_embedder::{AppLifecycleState, Offset, ViewId};
use waft_foundation::{App, AsyncApp, Completer, CompleterFuture, Listener, Timer, delayed};
use waft_gestures::{
    GestureBinding, HitTestResult, K_LONG_PRESS_TIMEOUT, K_PRESS_TIMEOUT, PointerDownEvent,
    PointerEvent, PointerMoveEvent, PointerUpEvent,
};
use waft_rendering::{
    AnyRenderBox, AnyRenderObject, BoxHitTestEntry, RenderView, SliverHitTestEntry,
};
use waft_scheduler::{FrameCallback, SchedulerBinding};

use crate::AnyElement;

/// How long a command waits, on the app's clock, for the app to settle and for the widget it
/// names to appear: flutter_driver's commands take a timeout, and ours is this one.
pub(super) const WAIT_LIMIT: Duration = Duration::from_secs(5);

/// How often a drag moves: a frame's time.
const FRAME: Duration = Duration::from_millis(16);

/// Waits until `holds` holds, looking now and after each frame, as flutter_driver's
/// `_waitUntilFrame` does with post-frame callbacks, for up to `limit` on the app's clock;
/// whether it came to hold. A frame is when the tree changes, so between frames there is
/// nothing new to look at.
pub(super) async fn wait_until(
    cx: &mut AsyncApp,
    limit: Duration,
    holds: impl Fn(&mut App) -> bool,
) -> bool {
    let deadline = cx.update(|app| app.clock()) + limit;
    loop {
        if cx.update(&holds) {
            return true;
        }
        let now = cx.update(|app| app.clock());
        if now >= deadline {
            return false;
        }
        cx.update(|app| next_frame_or_after(app, deadline - now))
            .await;
    }
}

/// A future that completes after the app's next frame, or after `timeout`, whichever is first;
/// the timer is cancelled when the frame comes first.
fn next_frame_or_after(app: &mut App, timeout: Duration) -> CompleterFuture<()> {
    let completer: Completer<()> = Completer::new();
    let future = completer.future();
    let timer: Rc<Cell<Option<Timer>>> = Rc::default();
    let finish = {
        let timer = Rc::clone(&timer);
        Rc::new(move |app: &mut App| {
            if completer.is_completed() {
                return;
            }
            if let Some(timer) = timer.take() {
                timer.cancel(app);
            }
            completer.complete(app, ());
        })
    };
    let after_frame = Rc::clone(&finish);
    SchedulerBinding::add_post_frame_callback(
        app,
        FrameCallback::new(move |app, _| after_frame(app)),
        None,
    );
    timer.set(Some(Timer::new(
        app,
        timeout,
        Listener::new(move |app| finish(app)),
    )));
    future
}

/// flutter_driver's frame sync: waits until the app has no frame scheduled and nothing
/// animating, for up to [`WAIT_LIMIT`]; whether it settled.
pub(super) async fn settle(cx: &mut AsyncApp) -> bool {
    wait_until(cx, WAIT_LIMIT, |app| {
        !SchedulerBinding::has_scheduled_frame(app)
            && SchedulerBinding::transient_callback_count(app) == 0
    })
    .await
}

/// Why the app draws no frames, where it knows: its windows are out of sight.
pub(super) fn why_no_frames(app: &mut App) -> Option<&'static str> {
    matches!(
        SchedulerBinding::lifecycle_state(app),
        Some(AppLifecycleState::Hidden | AppLifecycleState::Paused)
    )
    .then_some("its windows are out of sight, where it draws no frames")
}

/// Where a touch on `element` lands: the centre of its render box, globally, in the view it
/// renders into; `None` for an element that renders no box.
pub(super) fn center_of(app: &App, element: AnyElement) -> Option<(Offset, ViewId)> {
    let render = element.find_render_object(app)?.as_box()?;
    let center = render.size(app).center(Offset::ZERO);
    let at = render.local_to_global(app, center, None);
    Some((at, view_of(app, render)?))
}

/// The view whose render tree `render` is in.
fn view_of(app: &App, render: AnyRenderBox) -> Option<ViewId> {
    std::iter::successors(Some(render.as_object()), |node| node.parent(app)).find_map(|node| {
        node.downcast::<RenderView>(app)
            .map(|view| view.flutter_view(app).id())
    })
}

/// The render objects a touch at `at` in `view` reaches, the deepest first: the hit test the
/// gesture binding runs for a touch.
pub(super) fn hit_at(app: &mut App, at: Offset, view: ViewId) -> Vec<AnyRenderObject> {
    let mut result = HitTestResult::new();
    GestureBinding::instance(app).hit_test_in_view(app, &mut result, at, view);
    result
        .path()
        .iter()
        .filter_map(|entry| {
            let target: &dyn std::any::Any = entry.target();
            target
                .downcast_ref::<BoxHitTestEntry>()
                .map(|entry| entry.target().as_object())
                .or_else(|| {
                    target
                        .downcast_ref::<SliverHitTestEntry>()
                        .map(|entry| entry.target().as_object())
                })
        })
        .collect()
}

/// The pointers the driver's gestures use, apart from the host's.
static NEXT_POINTER: AtomicI64 = AtomicI64::new(1 << 20);

/// One touch of the driver's: a pointer of its own, in one view, stamped by the app's clock as a
/// host stamps its events, so velocity trackers see the time a drag took.
struct Touch {
    pointer: i64,
    view: ViewId,
    at: Offset,
}

impl Touch {
    /// A touch down at `at` in `view`.
    fn down(app: &mut App, at: Offset, view: ViewId) -> Touch {
        let touch = Touch {
            pointer: NEXT_POINTER.fetch_add(1, Ordering::Relaxed),
            view,
            at,
        };
        let down = PointerDownEvent {
            view_id: view,
            pointer: touch.pointer,
            position: at,
            time_stamp: app.clock(),
            ..PointerDownEvent::default()
        };
        dispatch(app, PointerEvent::Down(down));
        touch
    }

    /// The touch moved to `at`.
    fn move_to(&mut self, app: &mut App, at: Offset) {
        let moved = PointerMoveEvent {
            view_id: self.view,
            pointer: self.pointer,
            position: at,
            delta: at - self.at,
            time_stamp: app.clock(),
            ..PointerMoveEvent::default()
        };
        self.at = at;
        dispatch(app, PointerEvent::Move(moved));
    }

    /// The touch lifted where it is.
    fn up(self, app: &mut App) {
        let up = PointerUpEvent {
            view_id: self.view,
            pointer: self.pointer,
            position: self.at,
            time_stamp: app.clock(),
            ..PointerUpEvent::default()
        };
        dispatch(app, PointerEvent::Up(up));
    }
}

fn dispatch(app: &mut App, event: PointerEvent) {
    GestureBinding::instance(app).handle_pointer_event(app, event);
}

/// A touch down and up at `at` in `view`: Dart's `tapAt`.
pub(super) fn tap_at(app: &mut App, at: Offset, view: ViewId) {
    Touch::down(app, at, view).up(app);
}

/// A touch down at `at` in `view`, held past the long-press timeout, then lifted: flutter_test's
/// `longPressAt`, which holds it `kLongPressTimeout + kPressTimeout`.
pub(super) async fn long_press_at(cx: &mut AsyncApp, at: Offset, view: ViewId) {
    let touch = cx.update(|app| Touch::down(app, at, view));
    pause(cx, K_LONG_PRESS_TIMEOUT + K_PRESS_TIMEOUT).await;
    cx.update(|app| touch.up(app));
}

/// A touch down at `from` in `view`, moved by `by` over `duration` a frame at a time, then
/// lifted: flutter_driver's `scroll`.
pub(super) async fn drag(
    cx: &mut AsyncApp,
    from: Offset,
    view: ViewId,
    by: Offset,
    duration: Duration,
) {
    let steps = (duration.as_millis() / FRAME.as_millis()).max(1);
    let mut touch = cx.update(|app| Touch::down(app, from, view));
    for step in 1..=steps {
        pause(cx, FRAME).await;
        let at = from + by * (step as f64 / steps as f64);
        cx.update(|app| touch.move_to(app, at));
    }
    cx.update(|app| touch.up(app));
}

/// Waits `duration` on the app's clock.
async fn pause(cx: &mut AsyncApp, duration: Duration) {
    cx.update(|app| delayed(app, duration)).await;
}
