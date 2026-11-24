//! The system's material behind a window's content: AppKit's blur of what lies behind the window
//! (`NSVisualEffectView`) or its Liquid Glass (`NSGlassEffectView`, macOS 26), as an app chose it
//! with AppKit's own options, rounded to the corners the app draws.
//!
//! The view goes behind winit's view, as a sibling in the window's frame view, sized with the
//! window: a subview of winit's view would draw over its Metal layer. Liquid Glass renders as its
//! flat material there rather than as glass, which needs the view inside the content view, where
//! winit's Metal layer did not stay above it.
//!
//! The invariant: a window has one backdrop at most, the view carrying the backdrop's
//! identifier, so setting another replaces it and nothing else of the frame view is touched.

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, Bool};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSGlassEffectView, NSGlassEffectViewStyle,
    NSImage, NSImageResizingMode, NSUserInterfaceItemIdentification, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowOrderingMode,
};
use objc2_foundation::{NSEdgeInsets, NSRect, NSSize, NSString, ns_string};
use waft_embedder::Color;
use winit::window::Window;

use super::ns_window;

/// What a window on macOS shows behind its content, where the app paints nothing: one of
/// AppKit's two backdrop views, with AppKit's own options.
#[derive(Clone, Debug, PartialEq)]
pub enum MacosBackdrop {
    /// AppKit's blur of what lies behind the window (`NSVisualEffectView`, blending behind the
    /// window).
    VisualEffect {
        material: NSVisualEffectMaterial,
        state: NSVisualEffectState,
        /// How round its corners are, in points.
        corner_radius: f64,
    },
    /// Liquid Glass (`NSGlassEffectView`), where [`MacosBackdrop::has_glass`] says the system
    /// has it; elsewhere it shows nothing.
    Glass {
        style: NSGlassEffectViewStyle,
        /// The colour the glass is tinted with; AppKit's own where `None`.
        tint: Option<Color>,
        /// How round its corners are, in points.
        corner_radius: f64,
    },
}

impl MacosBackdrop {
    /// Whether this system has Liquid Glass: macOS 26 and later.
    pub fn has_glass() -> bool {
        AnyClass::get(c"NSGlassEffectView").is_some()
    }
}

/// The identifier of the view a window's backdrop is.
fn backdrop_identifier() -> &'static NSString {
    ns_string!("waft.backdrop")
}

/// Puts `backdrop` behind `window`'s content in place of the backdrop it had; `None` leaves it
/// with none.
pub(crate) fn set_backdrop(window: &dyn Window, backdrop: Option<&MacosBackdrop>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    let Some(content) = ns_window.contentView() else {
        return;
    };
    // SAFETY: the view hierarchy of a live window, read on the main thread.
    let Some(frame_view) = (unsafe { content.superview() }) else {
        return;
    };
    for view in frame_view.subviews().iter() {
        if view
            .identifier()
            .is_some_and(|identifier| &*identifier == backdrop_identifier())
        {
            view.removeFromSuperview();
        }
    }
    if let Some(view) = backdrop.and_then(|backdrop| backdrop_view(mtm, backdrop, content.frame()))
    {
        view.setIdentifier(Some(backdrop_identifier()));
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        frame_view.addSubview_positioned_relativeTo(
            &view,
            NSWindowOrderingMode::Below,
            Some(&content),
        );
    }
    // The window's shadow follows what it shows, which just changed.
    ns_window.invalidateShadow();
}

/// The view `backdrop` asks for, filling `frame`; none for glass where the system has none.
fn backdrop_view(
    mtm: MainThreadMarker,
    backdrop: &MacosBackdrop,
    frame: NSRect,
) -> Option<Retained<NSView>> {
    match backdrop {
        MacosBackdrop::VisualEffect {
            material,
            state,
            corner_radius,
        } => {
            let blur = NSVisualEffectView::initWithFrame(mtm.alloc(), frame);
            blur.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            blur.setMaterial(*material);
            blur.setState(*state);
            if *corner_radius > 0.0 {
                blur.setMaskImage(Some(&rounded_mask(*corner_radius)));
            }
            Some(Retained::into_super(blur))
        }
        MacosBackdrop::Glass {
            style,
            tint,
            corner_radius,
        } => {
            if !MacosBackdrop::has_glass() {
                return None;
            }
            let glass = NSGlassEffectView::initWithFrame(mtm.alloc(), frame);
            glass.setStyle(*style);
            glass.setCornerRadius(*corner_radius);
            let tint = tint.map(|tint| {
                NSColor::colorWithSRGBRed_green_blue_alpha(tint.r, tint.g, tint.b, tint.a)
            });
            glass.setTintColor(tint.as_deref());
            Some(Retained::into_super(glass))
        }
    }
}

/// An image that masks a view of any size to corners `radius` points round: a rounded square
/// whose middle stretches, as AppKit's notes on `NSVisualEffectView.maskImage` draw one.
fn rounded_mask(radius: f64) -> Retained<NSImage> {
    let side = radius * 2.0 + 1.0;
    let draw = RcBlock::new(move |rect: NSRect| -> Bool {
        NSColor::blackColor().set();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius).fill();
        Bool::YES
    });
    let mask = NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(side, side), false, &draw);
    mask.setCapInsets(NSEdgeInsets {
        top: radius,
        left: radius,
        bottom: radius,
        right: radius,
    });
    mask.setResizingMode(NSImageResizingMode::Stretch);
    mask
}
