//! A widget's image, as flutter_test's `captureImage` (`_matchers_io.dart`) takes it: the layer
//! of the nearest repaint boundary at or above it, drawn within its paint bounds.

use waft_embedder::Image;
use waft_foundation::App;

use crate::AnyElement;

/// What `element` paints, with everything its nearest repaint boundary paints, drawn into an
/// image at one pixel per unit of the boundary's paint bounds (an app's `View` paints in physical
/// pixels): Dart's `captureImage(element)`, the boundary's layer as a picture
/// (`OffsetLayer::to_picture`) drawn by the platform (`Platform::picture_to_image_sync`). A test
/// reads its pixels back with `Platform::read_image_pixels`. `None` where the platform has no
/// renderer, as a test platform until given a GPU (`TestPlatform::with_gpu`).
///
/// # Panics
///
/// Where `element` has no render object, or none at or above it is a repaint boundary with a
/// layer, as Dart's asserts.
pub fn capture_image(app: &mut App, element: AnyElement) -> Option<Image> {
    let mut boundary = element
        .render_object(app)
        .expect("the element has a render object");
    while !boundary.is_repaint_boundary(app) {
        boundary = boundary
            .parent(app)
            .expect("a repaint boundary at or above the element");
    }
    debug_assert!(!boundary.debug_needs_paint(app), "the element is painted");
    let layer = boundary
        .layer(app)
        .and_then(|layer| layer.as_offset_layer())
        .expect("a repaint boundary's layer is an offset layer");
    let bounds = boundary.paint_bounds(app);
    let picture = layer.to_picture(app, bounds, 1.0);
    let width = bounds.width().ceil() as u32;
    let height = bounds.height().ceil() as u32;
    app.platform()
        .picture_to_image_sync(&picture, width, height)
}
