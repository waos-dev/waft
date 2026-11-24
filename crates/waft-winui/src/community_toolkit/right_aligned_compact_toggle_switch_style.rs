//! The Toolkit's `RightAlignedCompactToggleSwitchStyle` of `SettingsCard/SettingsCard.xaml`: a `ToggleSwitch` for the end of a settings card, with its `On` / `Off` label before the switch, no minimum width and a height of 36.
//!
//! The style is based on `DefaultToggleSwitchStyle` and replaces its template with a copy whose visual states are the default's. What the copy changes: the root `Grid` has no rows and is the focus target; the inner `Grid` is at the right and centred, in one row with no content margins; `OffContentPresenter` and `OnContentPresenter` are in column 0 and the switch (`OuterBorder`, `SwitchKnobBounds`, `SwitchKnob`) in column 2; and `SwitchKnobOn` and `SwitchKnobOff` sit 3 from the knob's right and left edge in every state, where the default template centres them outside `Pressed`.

use crate::controls::toggle_switch::{
    KnobShapeOffsets, arranged_left, header_content_presenter, outer_border, switch_brushes,
    switch_knob, switch_knob_bounds,
};
use crate::{
    Brush, CONTENT_GAP, ColumnDefinition, FOCUS_VISUAL_MARGIN, FocusVisual, Grid, GridCell,
    GridLength, HorizontalAlignment, KNOB_AREA, SWITCH_AREA_MARGIN, ThemeResources, ToggleSwitch,
    ToggleSwitchTemplateParts, ToggleSwitchTemplateStates, control_text_style, knob_size,
};
use waft_embedder::FontWeight;
use waft_foundation::App;
use waft_painting::{AlignmentGeometry, EdgeInsetsGeometry, TextStyle};
use waft_widgets::*;

/// The style's `Height`.
pub const RIGHT_ALIGNED_COMPACT_TOGGLE_SWITCH_HEIGHT: f64 = 36.0;

/// `RightAlignedCompactToggleSwitchStyle`, for [`ToggleSwitch::template`]: the style a settings card gives the switch in it.
pub fn right_aligned_compact_toggle_switch_style(
    app: &mut App,
    context: BuildContext,
    widget: &ToggleSwitch,
    states: ToggleSwitchTemplateStates,
    parts: &ToggleSwitchTemplateParts,
) -> WidgetRef {
    let resources = ThemeResources::of(app, context);
    let r = &*resources;
    let brushes = switch_brushes(r, states.common);
    let is_on = widget.is_on;
    let text = control_text_style(
        r,
        r.control_content_theme_font_size,
        FontWeight::W400,
        brushes.foreground,
    );
    let off_content = widget
        .off_content
        .clone()
        .unwrap_or_else(|| Text::new("Off").into_widget());
    let on_content = widget
        .on_content
        .clone()
        .unwrap_or_else(|| Text::new("On").into_widget());
    let knob_width = knob_size(states.common).0;
    // The inner `Grid`, which declares no rows: every `Grid.Row` and `Grid.RowSpan` lands in its one row.
    let inner = Grid::new()
        .column_definitions([
            ColumnDefinition::new(GridLength::AUTO),
            ColumnDefinition::new(GridLength::pixel(CONTENT_GAP)).max_width(CONTENT_GAP),
            ColumnDefinition::new(GridLength::AUTO),
        ])
        .children([
            GridCell::new(switch_area_grid(r, brushes.container))
                .column_span(3)
                .into_widget(),
            GridCell::new(content_presenter(text.clone(), off_content, !is_on)).into_widget(),
            GridCell::new(content_presenter(text, on_content, is_on)).into_widget(),
            GridCell::new(centered(outer_border(r, &brushes, is_on)))
                .column(2)
                .into_widget(),
            GridCell::new(centered(switch_knob_bounds(r, &brushes, is_on)))
                .column(2)
                .into_widget(),
            GridCell::new(switch_knob(
                r,
                &brushes,
                states,
                is_on,
                KnobShapeOffsets {
                    // `HorizontalAlignment="Right"` `Margin="0,0,3,0"`.
                    on: arranged_left(
                        KNOB_AREA,
                        knob_width,
                        HorizontalAlignment::Right,
                        (0.0, 3.0),
                    ),
                    // `HorizontalAlignment="Left"` `Margin="3,0,0,0"`.
                    off: arranged_left(
                        KNOB_AREA,
                        knob_width,
                        HorizontalAlignment::Left,
                        (3.0, 0.0),
                    ),
                },
            ))
            .column(2)
            .into_widget(),
            GridCell::new(parts.switch_thumb())
                .column_span(3)
                .into_widget(),
        ]);
    let header = widget
        .header
        .clone()
        .map(|header| header_content_presenter(r, header, brushes.header));
    // `HorizontalAlignment="Right"` `VerticalAlignment="Center"`: as wide as its content, centred down the root.
    let inner = GridCell::new(
        Align::new()
            .alignment(AlignmentGeometry::CENTER_RIGHT)
            .width_factor(1.0)
            .child(inner),
    )
    .into_widget();
    // The root `Grid`: no rows, so the header and the inner `Grid` share its one cell; `Control.IsTemplateFocusTarget`, so the focus visual of the based-on style draws around it.
    let root = Grid::new().children(header.into_iter().chain([inner]));
    // The style's `Height`; its `MinWidth` of 0 leaves the width to the content.
    SizedBox::new()
        .height(RIGHT_ALIGNED_COMPACT_TOGGLE_SWITCH_HEIGHT)
        .child(
            FocusVisual::new(root, r.theme)
                .visible(states.focused)
                .margin(FOCUS_VISUAL_MARGIN)
                .corner_radius(r.control_corner_radius[0]),
        )
        .into_widget()
}

/// A `Rectangle` with a `Width` and a `Height`, centred in a cell that may be larger.
fn centered(element: WidgetRef) -> WidgetRef {
    Center::new()
        .width_factor(1.0)
        .height_factor(1.0)
        .child(element)
        .into_widget()
}

/// `SwitchAreaGrid`: spans the inner grid with `Margin="0,5"` and the container background under the style's `CornerRadius`.
fn switch_area_grid(r: &ThemeResources, container: waft_embedder::Color) -> WidgetRef {
    let [left, top, right, bottom] = SWITCH_AREA_MARGIN;
    Padding::new(EdgeInsetsGeometry::from_ltrb(left, top, right, bottom))
        .child(
            Grid::new()
                .background(Brush::Solid(container))
                .corner_radius(r.control_corner_radius[0]),
        )
        .into_widget()
}

/// `OffContentPresenter` / `OnContentPresenter`: at the right of column 0 (the style's `HorizontalContentAlignment`) and centred down it, with `Opacity` from the `ContentStates` group.
fn content_presenter(text: TextStyle, content: WidgetRef, visible: bool) -> WidgetRef {
    Opacity::new(if visible { 1.0 } else { 0.0 })
        .child(
            Align::new()
                .alignment(AlignmentGeometry::CENTER_RIGHT)
                .width_factor(1.0)
                .height_factor(1.0)
                .child(DefaultTextStyle::new(text, content)),
        )
        .into_widget()
}
