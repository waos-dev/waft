//! What a driver command does to a running app, as flutter_driver's `LiveWidgetController` does:
//! it waits for the app to settle, then sends pointer events into the gesture binding, the way a
//! host's touches arrive.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use waft_embedder::{Offset, ViewId};
use waft_foundation::{App, AsyncApp, delayed};
use waft_gestures::{GestureBinding, PointerDownEvent, PointerEvent, PointerUpEvent};
use waft_rendering::{AnyRenderBox, RenderView};
use waft_scheduler::SchedulerBinding;

use crate::AnyElement;

/// How long a command waits for the app to settle before it acts anyway: an app that animates
/// without end, as a spinner does, never settles.
const SETTLE_LIMIT: Duration = Duration::from_secs(5);

/// How often a waiting command looks again: a frame's time.
const FRAME: Duration = Duration::from_millis(16);

/// Waits until the app has no frame scheduled and nothing animating, flutter_driver's frame
/// sync, for up to [`SETTLE_LIMIT`]; whether it settled.
pub(super) async fn settle(cx: &mut AsyncApp) -> bool {
    let tries = SETTLE_LIMIT.as_millis() / FRAME.as_millis();
    for _ in 0..tries {
        if cx.update(is_settled) {
            return true;
        }
        cx.update(|app| delayed(app, FRAME)).await;
    }
    cx.update(is_settled)
}

fn is_settled(app: &mut App) -> bool {
    !SchedulerBinding::has_scheduled_frame(app)
        && SchedulerBinding::transient_callback_count(app) == 0
}

/// Where a tap on `element` lands: the centre of its render box, globally, in the view it
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

/// The pointers the driver's gestures use, apart from the host's.
static NEXT_POINTER: AtomicI64 = AtomicI64::new(1 << 20);

/// A touch down and up at `at` in `view`: Dart's `tapAt`.
pub(super) fn tap_at(app: &mut App, at: Offset, view: ViewId) {
    let pointer = NEXT_POINTER.fetch_add(1, Ordering::Relaxed);
    let binding = GestureBinding::instance(app);
    let down = PointerDownEvent {
        view_id: view,
        pointer,
        position: at,
        ..PointerDownEvent::default()
    };
    binding.handle_pointer_event(app, PointerEvent::Down(down));
    let up = PointerUpEvent {
        view_id: view,
        pointer,
        position: at,
        ..PointerUpEvent::default()
    };
    binding.handle_pointer_event(app, PointerEvent::Up(up));
}
