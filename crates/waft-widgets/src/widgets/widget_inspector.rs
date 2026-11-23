//! The widget inspector's service extensions: Flutter's `WidgetInspectorService`, as far as it is
//! ported. A client names an element or render object by the id the inspector's tree gave it;
//! here that id is the object's arena handle (`inspector-<bits>`), which names it while it lives
//! and nothing after, where Dart keeps a table of ids in object groups.

use std::rc::Rc;

use base64::Engine;
use waft_embedder::{ServiceExtensionError, ServiceExtensionParams, ServiceExtensionResult};
use waft_foundation::{
    App, DiagnosticLevel, DiagnosticsJsonMap, DiagnosticsNode, DiagnosticsSerializationDelegate,
    HandleId, ServiceExtensions, Task,
};
use waft_rendering::{AnyRenderObject, RendererBinding, screenshot_picture, subtree_bounds};

use crate::{AnyElement, ElementFacts, WidgetsBinding};

/// Dart's `WidgetInspectorService.initServiceExtensions`, as far as it is ported.
pub(crate) fn init_service_extensions(app: &mut App) {
    ServiceExtensions::register(
        app,
        "ext.waft.inspector.getRootWidgetTree",
        Rc::new(get_root_widget_tree),
    );
    ServiceExtensions::register(
        app,
        "ext.waft.inspector.getDetailsSubtree",
        Rc::new(get_details_subtree),
    );
    ServiceExtensions::register(app, "ext.waft.inspector.screenshot", Rc::new(screenshot));
    // Dart's signal extension: whether widgets carry their creation locations, which a debug
    // build's do.
    ServiceExtensions::register(
        app,
        "ext.waft.inspector.isWidgetCreationTracked",
        Rc::new(|_app, _params| {
            Task::ready(Ok(serde_json::json!({ "result": cfg!(debug_assertions) })))
        }),
    );
}

/// `ext.waft.inspector.getRootWidgetTree`: the element tree from the root as the inspector's
/// JSON, `{"result": tree}`, every node's `valueId` its inspector id. With `isSummaryTree`, the
/// widgets the app's own code made, which is every widget until creation locations are tracked,
/// as in Dart when they are not. Dart's `withPreviews` and `fullDetails=false` are not ported.
fn get_root_widget_tree(
    app: &mut App,
    params: ServiceExtensionParams,
) -> Task<ServiceExtensionResult> {
    let delegate = InspectorSerializationDelegate {
        group_name: params.get("groupName").map(str::to_owned),
        summary_tree: params.get("isSummaryTree") == Some("true"),
        subtree_depth: 1_000_000,
        ..InspectorSerializationDelegate::default()
    };
    let tree = WidgetsBinding::instance(app)
        .root_element(app)
        .map(|root| root.to_diagnostics_node(app, None).to_json_map(&delegate));
    Task::ready(Ok(serde_json::json!({ "result": tree })))
}

/// `ext.waft.inspector.getDetailsSubtree`: the object `arg` names, with its properties, and its
/// descendants `subtreeDepth` levels down (2 when not given), as `{"result": node}`.
fn get_details_subtree(
    app: &mut App,
    params: ServiceExtensionParams,
) -> Task<ServiceExtensionResult> {
    let subtree_depth = match params
        .get("subtreeDepth")
        .map(str::parse::<i32>)
        .transpose()
    {
        Ok(depth) => depth.unwrap_or(2),
        Err(_) => {
            return Task::ready(Err(ServiceExtensionError::InvalidParams(
                "`subtreeDepth` is a whole number".to_owned(),
            )));
        }
    };
    let delegate = InspectorSerializationDelegate {
        group_name: params.get("objectGroup").map(str::to_owned),
        subtree_depth,
        include_properties: true,
        ..InspectorSerializationDelegate::default()
    };
    let node = params
        .get("arg")
        .and_then(from_inspector_id)
        .and_then(|id| Inspected::named(app, id))
        .map(|object| object.diagnostics_node(app).to_json_map(&delegate));
    Task::ready(Ok(serde_json::json!({ "result": node })))
}

/// Dart's `InspectorSerializationDelegate`: how the inspector's JSON walks a tree, naming each
/// node's object by its inspector id.
#[derive(Clone, Debug)]
pub struct InspectorSerializationDelegate {
    /// The client's object group; a node's `valueId` is given where there is one.
    pub group_name: Option<String>,
    /// Whether children the app's own code did not make are passed over for theirs.
    pub summary_tree: bool,
    /// How many children a node shows before the rest are cut, or every one when negative.
    pub max_descendants_truncatable_node: i32,
    pub expand_property_values: bool,
    pub subtree_depth: i32,
    pub include_properties: bool,
}

impl Default for InspectorSerializationDelegate {
    /// Dart's defaults.
    fn default() -> InspectorSerializationDelegate {
        InspectorSerializationDelegate {
            group_name: None,
            summary_tree: false,
            max_descendants_truncatable_node: -1,
            expand_property_values: true,
            subtree_depth: 1,
            include_properties: false,
        }
    }
}

impl DiagnosticsSerializationDelegate for InspectorSerializationDelegate {
    fn additional_node_properties(
        &self,
        node: &dyn DiagnosticsNode,
        full_details: bool,
    ) -> DiagnosticsJsonMap {
        let mut result = DiagnosticsJsonMap::new();
        if self.summary_tree && full_details {
            result.insert("summaryTree".to_owned(), true.into());
        }
        if self.group_name.is_some() {
            let value_id = node.value_id().map(inspector_id);
            result.insert("valueId".to_owned(), value_id.into());
        }
        if let Some(facts) = element_facts(node)
            && let Some(location) = facts.creation_location
        {
            if full_details {
                result.insert(
                    "creationLocation".to_owned(),
                    serde_json::json!({
                        "file": location.file(),
                        "line": location.line(),
                        "column": location.column(),
                        "name": facts.widget_runtime_type,
                    }),
                );
            }
            if facts.created_by_local_project() {
                result.insert("createdByLocalProject".to_owned(), true.into());
            }
        }
        result
    }

    fn delegate_for_node(
        &self,
        node: &dyn DiagnosticsNode,
    ) -> Rc<dyn DiagnosticsSerializationDelegate> {
        // In the details tree the depth holds until the nodes the summary tree shares, so
        // expanding a node shows the whole subtree down to them.
        if self.summary_tree || self.subtree_depth > 1 || should_show_in_summary_tree(node) {
            self.copy_with(Some(self.subtree_depth - 1), None)
        } else {
            Rc::new(self.clone())
        }
    }

    fn filter_children(
        &self,
        nodes: Vec<Rc<dyn DiagnosticsNode>>,
        _owner: &dyn DiagnosticsNode,
    ) -> Vec<Rc<dyn DiagnosticsNode>> {
        self.children_shown(nodes)
    }

    /// The app's own widgets show their fine properties too.
    fn filter_properties(
        &self,
        nodes: Vec<Rc<dyn DiagnosticsNode>>,
        owner: &dyn DiagnosticsNode,
    ) -> Vec<Rc<dyn DiagnosticsNode>> {
        let created_by_local_project =
            element_facts(owner).is_some_and(ElementFacts::created_by_local_project);
        let level = if created_by_local_project {
            DiagnosticLevel::Fine
        } else {
            DiagnosticLevel::Info
        };
        nodes
            .into_iter()
            .filter(|node| !node.is_filtered(level))
            .collect()
    }

    fn truncate_nodes_list(
        &self,
        nodes: Vec<Rc<dyn DiagnosticsNode>>,
        owner: Option<&dyn DiagnosticsNode>,
    ) -> Vec<Rc<dyn DiagnosticsNode>> {
        let limit = self.max_descendants_truncatable_node;
        let truncates = limit >= 0
            && owner.is_some_and(|owner| owner.allow_truncate())
            && nodes.len() > limit as usize;
        if !truncates {
            return nodes;
        }
        // Dart's `_truncateNodes`: of elements whose locations are tracked, the app's own.
        let facts_of = |node: &Rc<dyn DiagnosticsNode>| element_facts(&**node).cloned();
        if nodes
            .iter()
            .all(|node| facts_of(node).is_some_and(|facts| facts.creation_location.is_some()))
        {
            let local: Vec<_> = nodes
                .iter()
                .filter(|node| facts_of(node).is_some_and(|facts| facts.created_by_local_project()))
                .cloned()
                .collect();
            if !local.is_empty() {
                return local;
            }
        }
        nodes.into_iter().take(limit as usize).collect()
    }

    fn subtree_depth(&self) -> i32 {
        self.subtree_depth
    }

    fn include_properties(&self) -> bool {
        self.include_properties
    }

    fn expand_property_values(&self) -> bool {
        self.expand_property_values
    }

    fn copy_with(
        &self,
        subtree_depth: Option<i32>,
        include_properties: Option<bool>,
    ) -> Rc<dyn DiagnosticsSerializationDelegate> {
        Rc::new(InspectorSerializationDelegate {
            subtree_depth: subtree_depth.unwrap_or(self.subtree_depth),
            include_properties: include_properties.unwrap_or(self.include_properties),
            ..self.clone()
        })
    }
}

impl InspectorSerializationDelegate {
    /// Dart's `_filterChildren`: in the summary tree, a child it does not show is replaced by
    /// its own children shown.
    fn children_shown(&self, nodes: Vec<Rc<dyn DiagnosticsNode>>) -> Vec<Rc<dyn DiagnosticsNode>> {
        let mut children = Vec::new();
        for child in nodes {
            if !self.summary_tree || should_show_in_summary_tree(&*child) {
                children.push(child);
            } else {
                children.extend(self.children_shown(child.get_children()));
            }
        }
        children
    }
}

/// Dart's `_shouldShowInSummaryTree`: an error, a node that is not an element's, and an element
/// whose widget's location is not tracked (a release build) are shown; an element otherwise when
/// the app's own code made its widget.
fn should_show_in_summary_tree(node: &dyn DiagnosticsNode) -> bool {
    if node.level() == DiagnosticLevel::Error {
        return true;
    }
    match element_facts(node) {
        Some(facts) if facts.creation_location.is_some() => facts.created_by_local_project(),
        _ => true,
    }
}

/// What `node` knows of its element, when it is an element's: Dart's `node.value is Element`.
fn element_facts(node: &dyn DiagnosticsNode) -> Option<&ElementFacts> {
    node.value()?.downcast_ref::<ElementFacts>()
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
    let Some(render_object) = from_inspector_id(id)
        .and_then(|id| Inspected::named(app, id))
        .and_then(|object| object.render_object(app))
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

/// An object an inspector id names: Dart's `toObject`, for the two kinds the inspector names.
#[derive(Clone, Copy)]
enum Inspected {
    Element(AnyElement),
    RenderObject(AnyRenderObject),
}

impl Inspected {
    /// The element or render object `id` names, in the element tree or a view's render tree.
    fn named(app: &mut App, id: HandleId) -> Option<Inspected> {
        if let Some(root) = WidgetsBinding::instance(app).root_element(app)
            && let Some(element) = element_named(app, root, id)
        {
            return Some(Inspected::Element(element));
        }
        let views = RendererBinding::instance(app).render_views(app);
        views
            .into_iter()
            .find_map(|view| render_object_in(app, view.as_object(), id))
            .map(Inspected::RenderObject)
    }

    /// Its render object: Dart's `_renderObjectOrNull`.
    fn render_object(self, app: &App) -> Option<AnyRenderObject> {
        match self {
            Inspected::Element(element) => element.find_render_object(app),
            Inspected::RenderObject(render_object) => Some(render_object),
        }
    }

    /// Its diagnostics node: Dart's `_idToDiagnosticsNode`.
    fn diagnostics_node(self, app: &App) -> Rc<dyn DiagnosticsNode> {
        match self {
            Inspected::Element(element) => Rc::new(element.to_diagnostics_node(app, None)),
            Inspected::RenderObject(render_object) => {
                Rc::new(render_object.to_diagnostics_node(app, None))
            }
        }
    }
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

    /// The answer of an extension that answers at once.
    fn answered_now(app: &mut App, name: &str, params: &[(&str, &str)]) -> serde_json::Value {
        let params = ServiceExtensionParams::new(
            params
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
        );
        let mut task = ServiceExtensions::call(app, name, params);
        match std::pin::Pin::new(&mut task).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(answer) => answer.expect("answered"),
            Poll::Pending => panic!("{name} answers at once"),
        }
    }

    /// The first node of `tree` whose description begins with `description`.
    fn find_node<'a>(
        tree: &'a serde_json::Value,
        description: &str,
    ) -> Option<&'a serde_json::Value> {
        if tree["description"]
            .as_str()
            .is_some_and(|text| text.starts_with(description))
        {
            return Some(tree);
        }
        tree["children"]
            .as_array()?
            .iter()
            .find_map(|child| find_node(child, description))
    }

    /// As DevTools reads it: the root tree names each element by an id, which the details of
    /// one ask for back, answering its widget's properties.
    #[test]
    fn the_root_tree_names_each_element_and_its_details_answer_its_properties() {
        let mut tester = WidgetTester::new();
        tester.pump_widget(Center::new().child(SizedBox::new().width(10.0).height(20.0)));
        tester.update(|app| {
            let tree = answered_now(
                app,
                "ext.waft.inspector.getRootWidgetTree",
                &[("groupName", "tree_1"), ("isSummaryTree", "true")],
            );
            let root = &tree["result"];
            assert_eq!(root["description"], "[root]");
            assert_eq!(root["type"], "_ElementDiagnosticableTreeNode");
            let sized = find_node(root, "SizedBox").expect("the SizedBox is in the tree");
            assert_eq!(sized["widgetRuntimeType"], "SizedBox");
            assert_eq!(sized["stateful"], false);
            let id = sized["valueId"].as_str().expect("an inspector id");

            let details = answered_now(
                app,
                "ext.waft.inspector.getDetailsSubtree",
                &[
                    ("arg", id),
                    ("objectGroup", "details_1"),
                    ("subtreeDepth", "1"),
                ],
            );
            let node = &details["result"];
            assert_eq!(node["valueId"], id);
            let properties: Vec<(&str, &str)> = node["properties"]
                .as_array()
                .expect("the details carry properties")
                .iter()
                .filter_map(|property| {
                    Some((
                        property["name"].as_str()?,
                        property["description"].as_str()?,
                    ))
                })
                .collect();
            assert!(properties.contains(&("width", "10.0")), "{properties:?}");
            assert!(properties.contains(&("height", "20.0")), "{properties:?}");
        });
    }

    #[derive(Debug)]
    struct Greeting;

    impl crate::StatelessWidget for Greeting {
        fn build(&self, _app: &mut App, _context: crate::BuildContext) -> crate::WidgetRef {
            crate::IntoWidget::into_widget(SizedBox::new())
        }
    }

    /// In a debug build a widget carries the line that made it, which the tree names, and the
    /// app's own code made it: Dart's `creationLocation` and `createdByLocalProject`.
    #[test]
    fn the_tree_names_where_each_widget_was_made() {
        let mut tester = WidgetTester::new();
        let line = line!() + 1;
        let greeting = crate::IntoWidget::into_widget(Greeting);
        tester.pump_widget(greeting);
        tester.update(|app| {
            let tree = answered_now(
                app,
                "ext.waft.inspector.getRootWidgetTree",
                &[("groupName", "g")],
            );
            let node = find_node(&tree["result"], "Greeting").expect("the Greeting is in the tree");
            assert_eq!(node["creationLocation"]["file"], file!());
            assert_eq!(node["creationLocation"]["line"], line);
            assert_eq!(node["creationLocation"]["name"], "Greeting");
            assert_eq!(node["createdByLocalProject"], true);
        });
    }

    /// A file Cargo names by a path relative to the workspace is the app's own; a dependency's,
    /// from its registry or a checkout, has an absolute one.
    #[test]
    fn only_the_workspaces_files_are_the_apps_own() {
        assert!(crate::framework::is_local_file(
            "crates/waos-design/src/sketch/home.rs"
        ));
        assert!(!crate::framework::is_local_file(
            "/Users/me/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/waft-widgets-0.9.1/src/widgets/basic.rs"
        ));
    }

    #[test]
    fn the_details_of_an_id_that_names_nothing_are_null() {
        let mut tester = WidgetTester::new();
        tester.pump_widget(SizedBox::new());
        tester.update(|app| {
            let details = answered_now(
                app,
                "ext.waft.inspector.getDetailsSubtree",
                &[("arg", "inspector-1"), ("objectGroup", "g")],
            );
            assert_eq!(details, serde_json::json!({ "result": null }));
        });
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
