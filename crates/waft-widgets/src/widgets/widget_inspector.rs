//! The widget inspector's service extensions: Flutter's `WidgetInspectorService`, as far as it is
//! ported. A client names an element or render object by the id the inspector's tree gave it;
//! here that id is the object's arena handle (`inspector-<bits>`), which names it while it lives
//! and nothing after, where Dart keeps a table of ids in object groups.

use std::rc::Rc;

use base64::Engine;
use waft_embedder::{ServiceExtensionError, ServiceExtensionParams, ServiceExtensionResult};
use waft_foundation::{App, HandleId, ServiceExtensions, Task};
use waft_rendering::{AnyRenderObject, RendererBinding, screenshot_picture, subtree_bounds};

use crate::{AnyElement, WidgetsBinding};

/// Dart's `WidgetInspectorService.initServiceExtensions`, as far as it is ported.
pub(crate) fn init_service_extensions(app: &mut App) {
    ServiceExtensions::register(app, "ext.waft.inspector.screenshot", Rc::new(screenshot));
}

/// The id the inspector names `id`'s object by: Dart's `toId`.
pub fn inspector_id(id: HandleId) -> String {
    format!("inspector-{}", id.to_bits())
}

/// The object `text` names, as [`inspector_id`] wrote it: Dart's `toObject`.
pub fn from_inspector_id(text: &str) -> Option<HandleId> {
    text.strip_prefix("inspector-")?
        .parse()
        .ok()
        .map(HandleId::from_bits)
}

/// `ext.waft.inspector.screenshot`: a PNG of the element or render object `id` names and its
/// descendants, fit within `width` by `height` pixels at no more than `maxPixelRatio`, with
/// `margin` around it, as `{"result": base64}`, or `{"result": null}` where it cannot be taken.
/// Dart's `debugPaint` is not ported, and is ignored.
fn screenshot(app: &mut App, params: ServiceExtensionParams) -> Task<ServiceExtensionResult> {
    match screenshot_image(app, &params) {
        Err(error) => Task::ready(Err(error)),
        Ok(None) => Task::ready(Ok(serde_json::json!({ "result": null }))),
        Ok(Some(image)) => {
            let pixels = app.platform().read_image_pixels(&image);
            app.spawn(async move |_cx| {
                let pixels = pixels
                    .await
                    .map_err(|error| ServiceExtensionError::Failed(error.to_string()))?;
                let png =
                    waft_embedder::encode_png(&pixels).map_err(ServiceExtensionError::Failed)?;
                let encoded = base64::engine::general_purpose::STANDARD.encode(png);
                Ok(serde_json::json!({ "result": encoded }))
            })
        }
    }
}

/// Dart's `WidgetInspectorService.screenshot`: the image, or `None` where the object is gone,
/// detached, not laid out, or paints nothing.
fn screenshot_image(
    app: &mut App,
    params: &ServiceExtensionParams,
) -> Result<Option<waft_embedder::Image>, ServiceExtensionError> {
    let id = params.require("id")?;
    let width = number(params, "width")?.ok_or_else(|| missing("width"))?;
    let height = number(params, "height")?.ok_or_else(|| missing("height"))?;
    let margin = number(params, "margin")?.unwrap_or(0.0);
    let max_pixel_ratio = number(params, "maxPixelRatio")?.unwrap_or(1.0);
    let Some(render_object) = from_inspector_id(id).and_then(|id| render_object_named(app, id))
    else {
        return Ok(None);
    };
    if !render_object.attached(app) {
        return Ok(None);
    }
    if render_object.debug_needs_layout(app) {
        let owner = render_object
            .owner(app)
            .expect("an attached object has an owner");
        owner.flush_layout(app);
        owner.flush_compositing_bits(app);
        owner.flush_paint(app);
        // Skipped by layout, the object cannot be painted.
        if render_object.debug_needs_layout(app) {
            return Ok(None);
        }
    }
    let mut bounds = subtree_bounds(app, render_object);
    if margin != 0.0 {
        bounds = bounds.inflate(margin);
    }
    if bounds.is_empty() {
        return Ok(None);
    }
    let pixel_ratio = max_pixel_ratio.min((width / bounds.width()).min(height / bounds.height()));
    let picture = screenshot_picture(app, render_object, bounds, pixel_ratio);
    let image = app.platform().picture_to_image_sync(
        &picture,
        (bounds.width() * pixel_ratio).ceil() as u32,
        (bounds.height() * pixel_ratio).ceil() as u32,
    );
    match image {
        Some(image) => Ok(Some(image)),
        None => Err(ServiceExtensionError::Failed(
            "this host draws no pictures into images".to_owned(),
        )),
    }
}

/// The parameter `name` as a number, where the client sent it.
fn number(
    params: &ServiceExtensionParams,
    name: &str,
) -> Result<Option<f64>, ServiceExtensionError> {
    params
        .get(name)
        .map(|text| {
            text.parse().map_err(|_| {
                ServiceExtensionError::InvalidParams(format!("`{name}` is a number, not `{text}`"))
            })
        })
        .transpose()
}

fn missing(name: &str) -> ServiceExtensionError {
    ServiceExtensionError::InvalidParams(format!("`{name}` is required"))
}

/// The render object `id` names: an element's (Dart's `_renderObjectOrNull`), or one in a view's
/// render tree.
fn render_object_named(app: &mut App, id: HandleId) -> Option<AnyRenderObject> {
    if let Some(root) = WidgetsBinding::instance(app).root_element(app)
        && let Some(element) = element_named(app, root, id)
    {
        return element.find_render_object(app);
    }
    let views = RendererBinding::instance(app).render_views(app);
    views
        .into_iter()
        .find_map(|view| render_object_in(app, view.as_object(), id))
}

fn element_named(app: &App, element: AnyElement, id: HandleId) -> Option<AnyElement> {
    if element.id() == id {
        return Some(element);
    }
    let mut found = None;
    element.visit_children(app, &mut |child| {
        if found.is_none() {
            found = element_named(app, child, id);
        }
    });
    found
}

fn render_object_in(app: &App, object: AnyRenderObject, id: HandleId) -> Option<AnyRenderObject> {
    if object.id() == id {
        return Some(object);
    }
    let mut found = None;
    object.visit_children(app, &mut |child| {
        if found.is_none() {
            found = render_object_in(app, child, id);
        }
    });
    found
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    use waft_embedder::Color;
    use waft_test::TestPlatform;

    use super::*;
    use crate::testing::WidgetTester;
    use crate::{Center, ColoredBox, GlobalKey, Padding, SizedBox};

    const GREEN: Color = Color::new(0xFF00FF00);

    /// A green box padded inside a red one: its screenshot by its element's id, at twice its
    /// size, is an 80 by 80 PNG of green alone.
    #[test]
    fn a_screenshot_is_the_named_widget_alone_as_a_png() {
        let mut tester = WidgetTester::with_platform(TestPlatform::new().with_gpu());
        let green = GlobalKey::new();
        tester.pump_widget(
            Center::new().child(
                ColoredBox::new(Color::new(0xFFFF0000)).child(
                    Padding::new(waft_painting::EdgeInsets::all(10.0)).child(
                        SizedBox::new()
                            .width(40.0)
                            .height(40.0)
                            .key(green.clone().into())
                            .child(ColoredBox::new(GREEN)),
                    ),
                ),
            ),
        );
        let mut task = tester.update(|app| {
            let element = green.current_context(app).expect("mounted");
            let params = ServiceExtensionParams::new([
                ("id".to_owned(), inspector_id(element.id())),
                ("width".to_owned(), "1000".to_owned()),
                ("height".to_owned(), "1000".to_owned()),
                ("maxPixelRatio".to_owned(), "2".to_owned()),
            ]);
            ServiceExtensions::call(app, "ext.waft.inspector.screenshot", params)
        });
        tester.pump();
        let answer =
            match std::pin::Pin::new(&mut task).poll(&mut Context::from_waker(Waker::noop())) {
                Poll::Ready(answer) => answer.expect("answered"),
                Poll::Pending => panic!("the screenshot is read back by the next pump"),
            };
        let png = base64::engine::general_purpose::STANDARD
            .decode(answer["result"].as_str().expect("a base64 PNG"))
            .expect("base64");
        use waft_embedder::valo::{AlphaType, PixelBuffer, PixelFormat, PixelLayout};
        let layout = PixelLayout::packed([80, 80], PixelFormat::Rgba8, AlphaType::Straight);
        let all_green = PixelBuffer::new(layout, [0, 255, 0, 255].repeat(80 * 80)).expect("pixels");
        assert!(
            png == waft_embedder::encode_png(&all_green).expect("a PNG"),
            "80 by 80 of green alone, without the red around it"
        );
    }

    /// An id that names nothing answers no picture, as Dart's does for a gone object.
    #[test]
    fn a_screenshot_of_nothing_answers_null() {
        let mut tester = WidgetTester::new();
        tester.pump_widget(SizedBox::new());
        let mut task = tester.update(|app| {
            let params = ServiceExtensionParams::new([
                ("id".to_owned(), "inspector-1".to_owned()),
                ("width".to_owned(), "10".to_owned()),
                ("height".to_owned(), "10".to_owned()),
            ]);
            ServiceExtensions::call(app, "ext.waft.inspector.screenshot", params)
        });
        let answer =
            match std::pin::Pin::new(&mut task).poll(&mut Context::from_waker(Waker::noop())) {
                Poll::Ready(answer) => answer.expect("answered"),
                Poll::Pending => panic!("answered at once"),
            };
        assert_eq!(answer, serde_json::json!({ "result": null }));
    }

    #[test]
    fn an_inspector_id_names_its_object_back() {
        let cell = waft_foundation::AppCell::new();
        let mut app = cell.borrow_mut();
        let object = app.create(()).id();
        assert_eq!(from_inspector_id(&inspector_id(object)), Some(object));
        assert_eq!(from_inspector_id("inspector-x"), None);
        assert_eq!(from_inspector_id("e12"), None);
    }
}
