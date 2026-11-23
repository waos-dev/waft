//! The driver's commands, each flutter_driver's of that name, answering JSON.

use std::time::Duration;

use serde_json::{Value, json};
use waft_embedder::{
    Offset, ServiceExtensionError, ServiceExtensionParams, ServiceExtensionResult, TextAffinity,
    TextEditingValue, TextInputAction, TextSelection, ViewId,
};
use waft_foundation::{App, AsyncApp, Task};
use waft_rendering::{RendererBinding, screenshot_picture};
use waft_services::TextInput;

use super::controller::{self, WAIT_LIMIT};
use super::target::{Reach, Selection, Target, line_of, node_of, target};
use crate::widgets::text::shown_text;
use crate::widgets::widget_inspector::{image_of, number, png_answer, positive};
use crate::{Curves, ScrollPositionAlignmentPolicy, Scrollable};

/// `get_health`: that the app answers, as `{"status": "ok"}`.
pub(super) fn get_health() -> ServiceExtensionResult {
    Ok(json!({ "status": "ok" }))
}

/// `find`: every widget the command names, each as the inspector's node of it, as
/// `{"found": [node, …]}`.
pub(super) fn find(app: &mut App, params: &ServiceExtensionParams) -> ServiceExtensionResult {
    let found: Vec<Value> = Selection::of(params)?
        .named(app)
        .into_iter()
        .map(|element| node_of(app, element))
        .collect();
    Ok(json!({ "found": found }))
}

/// `tap`: a tap at the centre of the one widget the command names that a touch reaches, as
/// `{"tapped": node, "settled": bool}`.
pub(super) async fn tap(
    cx: &mut AsyncApp,
    params: &ServiceExtensionParams,
) -> ServiceExtensionResult {
    synchronize(cx, params).await?;
    let Target { element, at, view } = target(cx, params, Reach::Touch).await?;
    let tapped = cx.update(|app| {
        controller::tap_at(app, at, view);
        node_of(app, element)
    });
    let settled = controller::settle(cx).await;
    Ok(json!({ "tapped": tapped, "settled": settled }))
}

/// `long_press`: a touch held on the one widget the command names past the long-press timeout,
/// flutter_test's `longPress`, as `{"pressed": node, "settled": bool}`.
pub(super) async fn long_press(
    cx: &mut AsyncApp,
    params: &ServiceExtensionParams,
) -> ServiceExtensionResult {
    synchronize(cx, params).await?;
    let Target { element, at, view } = target(cx, params, Reach::Touch).await?;
    let pressed = cx.update(|app| node_of(app, element));
    controller::long_press_at(cx, at, view).await;
    let settled = controller::settle(cx).await;
    Ok(json!({ "pressed": pressed, "settled": settled }))
}

/// `enter_text`: `text=` as the focused field's whole text, the caret after it, as the platform's
/// text input sends a field its value; a widget the command names is tapped first, to focus it.
/// As `{"entered": text, "settled": bool}`.
pub(super) async fn enter_text(
    cx: &mut AsyncApp,
    params: &ServiceExtensionParams,
) -> ServiceExtensionResult {
    let text = params.require("text")?.to_owned();
    synchronize(cx, params).await?;
    if names_a_widget(params) {
        let Target { at, view, .. } = target(cx, params, Reach::Touch).await?;
        cx.update(|app| controller::tap_at(app, at, view));
        synchronize(cx, params).await?;
    }
    cx.update(|app| {
        let caret = text.encode_utf16().count() as i32;
        let value = TextEditingValue::new()
            .text(text.clone())
            .selection(TextSelection::collapsed(caret, TextAffinity::Downstream));
        focused_input(app)?.update_editing_value(app, value);
        Ok::<_, ServiceExtensionError>(())
    })?;
    let settled = controller::settle(cx).await;
    Ok(json!({ "entered": text, "settled": settled }))
}

/// `send_text_input_action`: the focused field's action `action=` (Flutter's `TextInputAction`
/// names: `done`, `search`, `next`, …), as the keyboard's action key sends it. As
/// `{"action": name, "settled": bool}`.
pub(super) async fn send_text_input_action(
    cx: &mut AsyncApp,
    params: &ServiceExtensionParams,
) -> ServiceExtensionResult {
    let name = params.require("action")?.to_owned();
    let action = text_input_action(&name)?;
    synchronize(cx, params).await?;
    cx.update(|app| {
        focused_input(app)?.perform_action(app, action);
        Ok::<_, ServiceExtensionError>(())
    })?;
    let settled = controller::settle(cx).await;
    Ok(json!({ "action": name, "settled": settled }))
}

/// `scroll`: a touch down at the centre of the one widget the command names, moved by `dx=`,
/// `dy=` logical pixels over `duration=` milliseconds (300 when not given) and lifted. As
/// `{"scrolled": node, "settled": bool}`.
pub(super) async fn scroll(
    cx: &mut AsyncApp,
    params: &ServiceExtensionParams,
) -> ServiceExtensionResult {
    let by = Offset::new(
        finite(params, "dx")?.unwrap_or(0.0),
        finite(params, "dy")?.unwrap_or(0.0),
    );
    let duration = milliseconds(params, "duration")?.unwrap_or(Duration::from_millis(300));
    synchronize(cx, params).await?;
    let Target { element, at, view } = target(cx, params, Reach::Touch).await?;
    // Read before the drag, which can take the widget away, as it scrolls a list's item out.
    let scrolled = cx.update(|app| node_of(app, element));
    controller::drag(cx, at, view, by, duration).await;
    let settled = controller::settle(cx).await;
    Ok(json!({ "scrolled": scrolled, "settled": settled }))
}

/// `scroll_into_view`: scrolls every scrollable around the one widget the command names until it
/// shows, at `alignment=` along the way (0 the start, 1 the end; 0 when not given), as
/// flutter_driver's `scrollIntoView` calls `Scrollable.ensureVisible`. As
/// `{"shown": node, "settled": bool}`.
pub(super) async fn scroll_into_view(
    cx: &mut AsyncApp,
    params: &ServiceExtensionParams,
) -> ServiceExtensionResult {
    let alignment = finite(params, "alignment")?.unwrap_or(0.0);
    synchronize(cx, params).await?;
    let Target { element, .. } = target(cx, params, Reach::Any).await?;
    let shown = cx.update(|app| node_of(app, element));
    let scrolled = cx.update(|app| {
        Scrollable::ensure_visible(
            app,
            element,
            alignment,
            Duration::from_millis(100),
            Curves::ease(),
            ScrollPositionAlignmentPolicy::Explicit,
        )
    });
    scrolled.await;
    let settled = controller::settle(cx).await;
    Ok(json!({ "shown": shown, "settled": settled }))
}

/// `wait_for`: waits until the command names a widget, or with `absent=true` none, for up to
/// `timeout=` milliseconds (5000 when not given), looking after each frame, as flutter_driver's
/// `waitFor` and `waitForAbsent`. As `{"found": [node, …]}`.
pub(super) async fn wait_for(
    cx: &mut AsyncApp,
    params: &ServiceExtensionParams,
) -> ServiceExtensionResult {
    let absent = params.get("absent") == Some("true");
    let limit = milliseconds(params, "timeout")?.unwrap_or(WAIT_LIMIT);
    let selection = Selection::of(params)?;
    let came =
        controller::wait_until(cx, limit, |app| selection.named(app).is_empty() == absent).await;
    let found = cx.update(|app| selection.named(app));
    if !came {
        let what = if absent {
            "still names a widget"
        } else {
            "names no widget"
        };
        return Err(ServiceExtensionError::Failed(format!(
            "`{}` {what} after {} ms",
            params
                .get("selector")
                .or(params.get("id"))
                .unwrap_or_default(),
            limit.as_millis()
        )));
    }
    let found: Vec<Value> = cx.update(|app| {
        found
            .into_iter()
            .map(|element| node_of(app, element))
            .collect()
    });
    Ok(json!({ "found": found }))
}

/// `get_text`: the text the one widget the command names shows, or the first text inside it, as
/// flutter_driver's `get_text` reads a `Text` or a field. As `{"text": text}`.
pub(super) async fn get_text(
    cx: &mut AsyncApp,
    params: &ServiceExtensionParams,
) -> ServiceExtensionResult {
    synchronize(cx, params).await?;
    let Target { element, .. } = target(cx, params, Reach::Any).await?;
    cx.update(|app| {
        let text = std::iter::once(element)
            .chain(element.descendants(app, true))
            .find_map(|element| shown_text(app, element))
            .ok_or_else(|| {
                ServiceExtensionError::Failed(format!("{} shows no text", line_of(app, element)))
            })?;
        Ok(json!({ "text": text }))
    })
}

/// `screenshot`: a PNG of a whole view, `view=` its id, the first view when not given, in its own
/// pixels, as flutter_driver's `screenshot` pictures the first render view's layer. As
/// `{"result": base64, "view": id}`.
pub(super) fn screenshot(
    app: &mut App,
    params: &ServiceExtensionParams,
) -> Task<ServiceExtensionResult> {
    match view_image(app, params) {
        Err(error) => Task::ready(Err(error)),
        Ok((image, id)) => {
            let png = png_answer(app, &image);
            app.spawn(async move |_cx| {
                let mut answer = png.await?;
                answer["view"] = json!(id.0);
                Ok(answer)
            })
        }
    }
}

/// The image of the view `view=` names, the first when not given, in its own pixels, with its id.
fn view_image(
    app: &mut App,
    params: &ServiceExtensionParams,
) -> Result<(waft_embedder::Image, ViewId), ServiceExtensionError> {
    let views = RendererBinding::instance(app).render_views(app);
    let ids: Vec<ViewId> = views
        .iter()
        .map(|view| view.flutter_view(app).id())
        .collect();
    let wanted = match params.get("view") {
        None => ids.first().copied(),
        Some(text) => text.parse().ok().map(ViewId).filter(|id| ids.contains(id)),
    };
    let Some(at) = wanted.and_then(|id| ids.iter().position(|each| *each == id)) else {
        let ids: Vec<String> = ids.iter().map(|id| id.0.to_string()).collect();
        return Err(ServiceExtensionError::InvalidParams(format!(
            "no view of that id; the app's views are {}",
            ids.join(", ")
        )));
    };
    let view = views[at].as_object();
    let bounds = view.paint_bounds(app);
    let picture = screenshot_picture(app, view, bounds, 1.0)
        .ok_or_else(|| ServiceExtensionError::Failed("the view has not drawn yet".to_owned()))?;
    let width = bounds.width().ceil() as u32;
    let height = bounds.height().ceil() as u32;
    Ok((image_of(app, &picture, width, height)?, ids[at]))
}

/// flutter_driver's frame sync before a command acts: waits until the app has settled, and
/// refuses the command where it has not within [`WAIT_LIMIT`]; with `frameSync=false`,
/// flutter_driver's `runUnsynchronized`, acts at once.
async fn synchronize(
    cx: &mut AsyncApp,
    params: &ServiceExtensionParams,
) -> Result<(), ServiceExtensionError> {
    if params.get("frameSync") == Some("false") || controller::settle(cx).await {
        return Ok(());
    }
    let why = cx
        .update(controller::why_no_frames)
        .unwrap_or("something keeps animating, as a spinner does");
    Err(ServiceExtensionError::Failed(format!(
        "the app did not settle within {} seconds: {why}. To act without waiting, turn frame \
         sync off: `--unsynchronized` with cargo waft ext, `frameSync=false` in a call",
        WAIT_LIMIT.as_secs()
    )))
}

/// Whether the command names a widget, by `selector=` or `id=`.
fn names_a_widget(params: &ServiceExtensionParams) -> bool {
    params.get("selector").is_some() || params.get("id").is_some()
}

/// The text input the focused field listens on, or why there is none.
fn focused_input(
    app: &mut App,
) -> Result<waft_foundation::Handle<TextInput>, ServiceExtensionError> {
    let input = TextInput::instance(app);
    match input.current_connection(app) {
        Some(_) => Ok(input),
        None => Err(ServiceExtensionError::Failed(
            "no text field has the focus: name the field to type into".to_owned(),
        )),
    }
}

/// Flutter's `TextInputAction` of that name.
fn text_input_action(name: &str) -> Result<TextInputAction, ServiceExtensionError> {
    Ok(match name {
        "none" => TextInputAction::None,
        "unspecified" => TextInputAction::Unspecified,
        "done" => TextInputAction::Done,
        "go" => TextInputAction::Go,
        "search" => TextInputAction::Search,
        "send" => TextInputAction::Send,
        "next" => TextInputAction::Next,
        "previous" => TextInputAction::Previous,
        "continueAction" => TextInputAction::ContinueAction,
        "join" => TextInputAction::Join,
        "route" => TextInputAction::Route,
        "emergencyCall" => TextInputAction::EmergencyCall,
        "newline" => TextInputAction::Newline,
        other => {
            return Err(ServiceExtensionError::InvalidParams(format!(
                "`{other}` is no text input action; they are done, go, search, send, next, \
                 previous, newline, continueAction, join, route, emergencyCall, unspecified, none"
            )));
        }
    })
}

/// The parameter `name` as a finite number, where the client sent it.
fn finite(
    params: &ServiceExtensionParams,
    name: &str,
) -> Result<Option<f64>, ServiceExtensionError> {
    match number(params, name)? {
        Some(value) if !value.is_finite() => Err(ServiceExtensionError::InvalidParams(format!(
            "`{name}` is a finite number"
        ))),
        value => Ok(value),
    }
}

/// The parameter `name` as milliseconds above zero, where the client sent it.
fn milliseconds(
    params: &ServiceExtensionParams,
    name: &str,
) -> Result<Option<Duration>, ServiceExtensionError> {
    Ok(positive(params, name)?.map(|ms| Duration::from_secs_f64(ms / 1000.0)))
}
