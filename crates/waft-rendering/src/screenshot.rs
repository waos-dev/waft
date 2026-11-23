//! A picture of one render object as it last painted, apart from what paints around it: the
//! widget inspector's screenshot (`_ScreenshotPaintingContext.toImage`,
//! `_calculateSubtreeBounds`).
//!
//! As in Dart, the nearest repaint boundary at or above the object repaints in place through a
//! painting context that notes what the object paints, and the screenshot borrows those layers
//! through proxy layers, which add a layer of the real tree to another scene without taking it
//! as a child. Dart's context also copies the object's canvas into a recording of its own
//! (`_MulticastCanvas`); this one closes its picture before and after the object paints instead,
//! so the object's drawing is in picture layers of its own, which the screenshot borrows as it
//! borrows the object's other layers. The real tree draws the same; its picture is split where
//! the object starts and ends.

use std::cell::RefCell;
use std::rc::Rc;

use waft_embedder::{Matrix4, Offset, Picture, Rect, SceneBuilder};
use waft_foundation::{App, Handle};
use waft_painting::transform_rect;

use crate::layer::{
    AnnotationSearch, AnyLayer, ContainerLayer, ErasedLayer, Layer, LayerData, OffsetLayer,
    scene_picture,
};
use crate::object::AnyRenderObject;
use crate::painting_context::PaintingContext;

/// Dart's `_ScreenshotData`: what the painting contexts recording one screenshot share.
pub(crate) struct ScreenshotRecording {
    /// The object a picture is taken of.
    pub(crate) target: AnyRenderObject,
    /// The root of the screenshot's layers: proxies of the target's.
    container: Handle<OffsetLayer>,
    /// Whether the target has been reached in the paint.
    pub(crate) found_target: bool,
    /// Whether the layers the context appends now are the target's.
    pub(crate) include_in_screenshot: bool,
    /// Where the target was painted in the context that painted it.
    screenshot_offset: Offset,
}

/// The recording, as the painting contexts hold it.
pub(crate) type SharedRecording = Rc<RefCell<ScreenshotRecording>>;

impl ScreenshotRecording {
    fn new(app: &mut App, target: AnyRenderObject) -> SharedRecording {
        Rc::new(RefCell::new(ScreenshotRecording {
            target,
            container: OffsetLayer::new(app, Offset::ZERO),
            found_target: false,
            include_in_screenshot: false,
            screenshot_offset: Offset::ZERO,
        }))
    }

    /// The target is about to paint at `offset`: Dart's `paintChild` before `super`.
    pub(crate) fn start_target(&mut self, offset: Offset) {
        debug_assert!(!self.include_in_screenshot && !self.found_target);
        self.found_target = true;
        self.screenshot_offset = offset;
        self.include_in_screenshot = true;
    }

    /// The target has painted: Dart's `paintChild` after `super`.
    pub(crate) fn end_target(&mut self) {
        self.include_in_screenshot = false;
    }

    /// Adds `layer`, which the target painted into the real tree, to the screenshot.
    pub(crate) fn borrow_layer(&self, app: &mut App, layer: AnyLayer) {
        let proxy = ProxyLayer::new(app, layer);
        self.container
            .as_container_layer()
            .append(app, proxy.as_layer());
    }
}

/// Dart's `_ProxyLayer`: a layer of another tree added to this one's scene, which stays a child of
/// its own parent.
pub(crate) struct ProxyLayer {
    layer: LayerData,
    proxied: AnyLayer,
}

impl ProxyLayer {
    fn new(app: &mut App, proxied: AnyLayer) -> Handle<ProxyLayer> {
        app.create(ProxyLayer {
            layer: LayerData::new(),
            proxied,
        })
        .into_tree_node(app)
    }
}

impl Layer for ProxyLayer {
    crate::layer_accessors!();

    fn add_to_scene(self: Handle<Self>, app: &mut App, builder: &mut SceneBuilder) {
        let proxied = app.get(self).proxied;
        proxied.add_to_scene(app, builder);
    }

    fn find_annotations(
        self: Handle<Self>,
        app: &App,
        result: &mut AnnotationSearch,
        local_position: Offset,
        only_first: bool,
    ) -> bool {
        app.get(self)
            .proxied
            .find_annotations(app, result, local_position, only_first)
    }
}

/// Dart's `_ScreenshotPaintingContext.toImage`, as the picture the host draws into an image: what
/// `render_object` and its descendants paint within `render_bounds`, in its own coordinates,
/// scaled by `pixel_ratio`. The object must have painted.
pub fn screenshot_picture(
    app: &mut App,
    render_object: AnyRenderObject,
    render_bounds: Rect,
    pixel_ratio: f64,
) -> Picture {
    debug_assert!(
        !render_object.debug_needs_paint(app),
        "the object has painted"
    );
    let mut boundary = render_object;
    while !boundary.is_repaint_boundary(app) {
        boundary = boundary
            .parent(app)
            .expect("a repaint boundary at or above the object");
    }
    let boundary_layer = boundary
        .layer(app)
        .and_then(|layer| layer.as_offset_layer())
        .expect("a repaint boundary's layer is an offset layer");
    let recording = ScreenshotRecording::new(app, render_object);
    if boundary == render_object {
        let mut data = recording.borrow_mut();
        data.borrow_layer(app, boundary_layer.as_layer());
        data.found_target = true;
        data.screenshot_offset = boundary_layer.offset(app);
    } else {
        let context = PaintingContext::recording_screenshot(
            boundary_layer.as_container_layer(),
            boundary.paint_bounds(app),
            Rc::clone(&recording),
        );
        PaintingContext::debug_instrument_repaint_composited_child(app, boundary, context);
    }
    // The boundary repainted in place: Dart's `repaintBoundary.debugLayer!.buildScene`.
    let _ = boundary_layer
        .as_container_layer()
        .build_scene(app, SceneBuilder::new());
    let (container, offset) = {
        let data = recording.borrow();
        debug_assert!(data.found_target, "the boundary's paint reached the object");
        (data.container, data.screenshot_offset)
    };
    let picture = scene_picture(
        app,
        container.as_container_layer(),
        render_bounds,
        offset,
        pixel_ratio,
    );
    container.as_layer().dispose(app);
    picture
}

/// Dart's `_calculateSubtreeBounds`: the bounds `object` and its descendants paint in, in its
/// own coordinates, each child's within what its parent says it clips to. A box's paint bounds
/// stand in for Dart's `semanticBounds`, which are the same for a box.
pub fn subtree_bounds(app: &App, object: AnyRenderObject) -> Rect {
    subtree_bounds_in(app, object, Matrix4::IDENTITY)
}

fn subtree_bounds_in(app: &App, object: AnyRenderObject, transform: Matrix4) -> Rect {
    let mut bounds = transform_rect(&transform, object.paint_bounds(app));
    object.visit_children(app, &mut |child| {
        let mut child_transform = transform;
        object.apply_paint_transform(app, child, &mut child_transform);
        let mut child_bounds = subtree_bounds_in(app, child, child_transform);
        if let Some(paint_clip) = object.describe_approximate_paint_clip(app, child) {
            child_bounds = child_bounds.intersect(transform_rect(&transform, paint_clip));
        }
        if child_bounds.is_finite() && !child_bounds.is_empty() {
            bounds = if bounds.is_empty() {
                child_bounds
            } else {
                bounds.expand_to_include(child_bounds)
            };
        }
    });
    bounds
}

#[cfg(test)]
mod tests {
    use waft_embedder::valo::Op;
    use waft_embedder::{Color, flattened_ops};
    use waft_foundation::AppCell;
    use waft_painting::{AlignmentGeometry, EdgeInsets};

    use super::*;
    use crate::box_::{BoxConstraints, RenderBox};
    use crate::proxy_box::{RenderColoredBox, RenderConstrainedBox};
    use crate::shifted_box::RenderPadding;
    use crate::testing::{EnginePhase, layout_with};

    const RED: Color = Color::new(0xFFFF0000);
    const GREEN: Color = Color::new(0xFF00FF00);

    /// A red box, padded, around a green one: the green one's screenshot is its own drawing
    /// alone, at its own origin, though both paint into the one picture of the root's boundary.
    #[test]
    fn a_screenshot_holds_the_object_alone_in_its_own_coordinates() {
        let cell = AppCell::new();
        let mut app = cell.borrow_mut();
        let square = RenderConstrainedBox::new(
            &mut app,
            BoxConstraints::tight_for(Some(20.0), Some(20.0)),
            None,
        );
        let green = RenderColoredBox::new(&mut app, GREEN, true, Some(square.as_box()));
        let padding = RenderPadding::new(
            &mut app,
            EdgeInsets::all(10.0).into(),
            None,
            Some(green.as_box()),
        );
        let red = RenderColoredBox::new(&mut app, RED, true, Some(padding.as_box()));
        layout_with(
            &mut app,
            red.as_box(),
            Some(BoxConstraints::tight(waft_embedder::Size::new(60.0, 60.0))),
            AlignmentGeometry::TOP_LEFT,
            EnginePhase::Paint,
        );
        let bounds = subtree_bounds(&app, green.as_object());
        assert_eq!(bounds, Rect::from_ltwh(0.0, 0.0, 40.0, 40.0));
        let picture = screenshot_picture(&mut app, green.as_object(), bounds, 1.0);
        let ops = flattened_ops(&picture);
        let colors: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                Op::DrawRect { paint, .. } => Some(paint.color),
                _ => None,
            })
            .collect();
        assert!(!colors.is_empty(), "the green box is in the screenshot");
        assert!(
            colors.iter().all(|&color| color == GREEN.into()),
            "and the red one around it is not: {colors:?}"
        );
        let moved_to_its_origin = ops.iter().any(|op| match op {
            Op::Transform(matrix) => {
                let translation = matrix.to_flutter_array();
                (translation[12], translation[13]) == (-10.0, -10.0)
            }
            _ => false,
        });
        assert!(moved_to_its_origin, "drawn at the green box's own origin");
    }

    /// The object's own paint bounds, where nothing paints outside them.
    #[test]
    fn a_boxes_subtree_bounds_are_its_own_where_its_children_stay_inside() {
        let cell = AppCell::new();
        let mut app = cell.borrow_mut();
        let inner = RenderConstrainedBox::new(
            &mut app,
            BoxConstraints::tight_for(Some(20.0), Some(20.0)),
            None,
        );
        let outer = RenderPadding::new(
            &mut app,
            EdgeInsets::all(5.0).into(),
            None,
            Some(inner.as_box()),
        );
        layout_with(
            &mut app,
            outer.as_box(),
            Some(BoxConstraints::tight(waft_embedder::Size::new(30.0, 30.0))),
            AlignmentGeometry::TOP_LEFT,
            EnginePhase::Layout,
        );
        assert_eq!(
            subtree_bounds(&app, outer.as_object()),
            Rect::from_ltwh(0.0, 0.0, 30.0, 30.0)
        );
    }
}
