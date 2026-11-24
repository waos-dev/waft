//! The Toolkit's `SettingsExpander` with the styles of `SettingsExpander/SettingsExpander.xaml`: a settings card that opens to show more cards under it.
//!
//! `DefaultSettingsExpanderStyle`'s template is an `Expander` in `SettingsExpanderExpanderStyle`, whose `Header` is a `SettingsCard` without background or border carrying the expander's `Header`, `Description`, `HeaderIcon` and `Content`, and whose `Content` is a `Grid` of `ItemsHeader`, the `PART_ItemsRepeater` rows and `ItemsFooter`. `SettingsExpanderItemStyleSelector` gives each row that is a `SettingsCard` `DefaultSettingsExpanderItemStyle`, or `ClickableSettingsExpanderItemStyle` when it can be clicked.
//!
//! `SettingsExpanderExpanderStyle` is `DefaultExpanderStyle` with these changes: `ExpanderHeader` takes `SettingsExpanderHeaderDownStyle` and no `Background` of its own; `ExpanderContent` paints no background, is at least `SettingsExpanderContentMinHeight` high and holds its `ContentPresenter` with `Margin="0,-2,0,0"`; and the style's `Padding` is 0. `SettingsExpanderHeaderDownStyle` is `ExpanderHeaderDownStyle` with the settings card's background and border per state on `ToggleButtonGrid`, no padding, and `ExpandCollapseChevronBorder` as a 32 × 32 `ContentControl`, which paints no background or border, with a tooltip. `SettingsExpander` never sets `ExpandDirection`, so the `Up` state of `ExpandDirectionStates` is never entered and is not transcribed.

use crate::{
    Brush, ColumnDefinition, CommonState, ControlBorder, ControlStates, ExpandState, Expander,
    ExpanderExpandedChangedHandler, ExpanderTemplateParts, ExpanderTemplateStates, FluentSymbol,
    FocusVisual, FontIcon, Grid, GridCell, GridLength, Margin, RowDefinition,
    SETTINGS_CARD_MIN_HEIGHT, SETTINGS_CARD_MIN_WIDTH, SettingsCard, ThemeResources, ToggleButton,
    ToolTip, ToolTipService, control_text_style,
};
use std::{fmt, rc::Rc};
use waft_embedder::{Color, FontWeight};
use waft_foundation::{App, Listener};
use waft_painting::AlignmentGeometry;
use waft_rendering::BoxConstraints;
use waft_widgets::*;

/// `SettingsExpanderChevronToolTip`.
pub const SETTINGS_EXPANDER_CHEVRON_TOOL_TIP: &str = "Show all settings";
/// `SettingsExpanderHeaderPadding`: left, top, right, bottom.
pub const SETTINGS_EXPANDER_HEADER_PADDING: [f64; 4] = [16.0, 16.0, 4.0, 16.0];
/// `SettingsExpanderItemPadding`: left, top, right, bottom.
pub const SETTINGS_EXPANDER_ITEM_PADDING: [f64; 4] = [58.0, 8.0, 44.0, 8.0];
/// `SettingsExpanderItemBorderThickness`: left, top, right, bottom.
pub const SETTINGS_EXPANDER_ITEM_BORDER_THICKNESS: [f64; 4] = [0.0, 1.0, 0.0, 0.0];
/// `ClickableSettingsExpanderItemPadding`: left, top, right, bottom.
pub const CLICKABLE_SETTINGS_EXPANDER_ITEM_PADDING: [f64; 4] = [58.0, 8.0, 16.0, 8.0];
/// `MinHeight` of `DefaultSettingsExpanderItemStyle`.
pub const SETTINGS_EXPANDER_ITEM_MIN_HEIGHT: f64 = 52.0;
/// `SettingsExpanderContentMinHeight`.
pub const SETTINGS_EXPANDER_CONTENT_MIN_HEIGHT: f64 = 16.0;
/// `SettingsExpanderChevronButtonWidth`.
pub const SETTINGS_EXPANDER_CHEVRON_BUTTON_WIDTH: f64 = 32.0;
/// `SettingsExpanderChevronButtonHeight`.
pub const SETTINGS_EXPANDER_CHEVRON_BUTTON_HEIGHT: f64 = 32.0;
/// `ExpandCollapseChevronBorder`'s `Margin`: left, top, right, bottom.
const CHEVRON_MARGIN: [f64; 4] = [0.0, 0.0, 8.0, 0.0];
/// `ExpandCollapseChevron`'s `Width` and `Height`, and its fallback icon's `FontSize`.
const CHEVRON_SIZE: f64 = 16.0;
/// `Margin` of the `ContentPresenter` in `ExpanderContent`: left, top, right, bottom.
const CONTENT_PRESENTER_MARGIN: [f64; 4] = [0.0, -2.0, 0.0, 0.0];

/// The Toolkit's `SettingsExpander`: a settings card (`Header`, `Description`, `HeaderIcon`, `Content`) whose chevron shows `Items`, the rows under it, between `ItemsHeader` and `ItemsFooter`.
#[derive(Clone)]
pub struct SettingsExpander {
    /// `IsExpanded`: whether the rows show.
    pub is_expanded: bool,
    /// Asks the owner for a new `IsExpanded`, as [`Expander`] does.
    pub is_expanded_changed: ExpanderExpandedChangedHandler,
    /// `Header`.
    pub header: Option<WidgetRef>,
    /// `Description`.
    pub description: Option<WidgetRef>,
    /// `HeaderIcon`.
    pub header_icon: Option<WidgetRef>,
    /// `Content`: the control at the end of the header card, before the chevron.
    pub content: Option<WidgetRef>,
    /// `ItemsHeader`: shown above the rows.
    pub items_header: Option<WidgetRef>,
    /// `ItemsFooter`: shown under the rows.
    pub items_footer: Option<WidgetRef>,
    /// `Items`: the rows. A row that is a [`SettingsCard`] takes the expander's item style for what it does not set itself; any other widget shows as it is.
    pub items: Vec<WidgetRef>,
    /// `Expanded`, raised when `IsExpanded` becomes true.
    pub expanded: Option<Listener>,
    /// `Collapsed`, raised when `IsExpanded` becomes false.
    pub collapsed: Option<Listener>,
    /// `IsEnabled`.
    pub is_enabled: bool,
    pub key: Option<KeyRef>,
}

impl SettingsExpander {
    /// An expander whose `IsExpanded` its caller owns.
    pub fn new(is_expanded: bool, changed: impl Fn(&mut App, bool) + 'static) -> SettingsExpander {
        SettingsExpander {
            is_expanded,
            is_expanded_changed: Rc::new(changed),
            header: None,
            description: None,
            header_icon: None,
            content: None,
            items_header: None,
            items_footer: None,
            items: Vec::new(),
            expanded: None,
            collapsed: None,
            is_enabled: true,
            key: None,
        }
    }

    /// XAML `Header`.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn header<K>(mut self, header: impl IntoWidget<K>) -> SettingsExpander {
        self.header = Some(header.into_widget());
        self
    }

    /// XAML `Description`.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn description<K>(mut self, description: impl IntoWidget<K>) -> SettingsExpander {
        self.description = Some(description.into_widget());
        self
    }

    /// XAML `HeaderIcon`.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn header_icon<K>(mut self, header_icon: impl IntoWidget<K>) -> SettingsExpander {
        self.header_icon = Some(header_icon.into_widget());
        self
    }

    /// XAML `Content`.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn content<K>(mut self, content: impl IntoWidget<K>) -> SettingsExpander {
        self.content = Some(content.into_widget());
        self
    }

    /// XAML `ItemsHeader`.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn items_header<K>(mut self, items_header: impl IntoWidget<K>) -> SettingsExpander {
        self.items_header = Some(items_header.into_widget());
        self
    }

    /// XAML `ItemsFooter`.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn items_footer<K>(mut self, items_footer: impl IntoWidget<K>) -> SettingsExpander {
        self.items_footer = Some(items_footer.into_widget());
        self
    }

    /// XAML `Items`.
    pub fn items(mut self, items: impl IntoIterator<Item = WidgetRef>) -> SettingsExpander {
        self.items = items.into_iter().collect();
        self
    }

    /// XAML `Expanded`.
    pub fn expanded(mut self, expanded: Listener) -> SettingsExpander {
        self.expanded = Some(expanded);
        self
    }

    /// XAML `Collapsed`.
    pub fn collapsed(mut self, collapsed: Listener) -> SettingsExpander {
        self.collapsed = Some(collapsed);
        self
    }

    /// XAML `IsEnabled`.
    pub fn is_enabled(mut self, is_enabled: bool) -> SettingsExpander {
        self.is_enabled = is_enabled;
        self
    }

    pub fn key(mut self, key: KeyRef) -> SettingsExpander {
        self.key = Some(key);
        self
    }
}

impl fmt::Debug for SettingsExpander {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsExpander")
            .field("is_expanded", &self.is_expanded)
            .field("items", &self.items.len())
            .field("is_enabled", &self.is_enabled)
            .finish_non_exhaustive()
    }
}

impl StatelessWidget for SettingsExpander {
    fn key(&self) -> Option<&KeyRef> {
        self.key.as_ref()
    }

    /// `DefaultSettingsExpanderStyle`'s `ControlTemplate`.
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        // `Expander.Header`: the card of the expander's own properties, which cannot be clicked.
        let mut header = SettingsCard::new()
            .padding(SETTINGS_EXPANDER_HEADER_PADDING)
            .background(Color::from_argb(0, 0, 0, 0))
            .border_thickness([0.0; 4])
            .is_enabled(self.is_enabled);
        header.header = self.header.clone();
        header.description = self.description.clone();
        header.header_icon = self.header_icon.clone();
        header.content = self.content.clone();
        // `Expander.Content`: a `Grid` of rows Auto, `*` and Auto, each child across its width.
        let rows = self
            .items_header
            .iter()
            .cloned()
            .chain(self.items.iter().map(|item| self.item_container(item)))
            .chain(self.items_footer.iter().cloned())
            .collect::<Vec<_>>();
        let content = Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(rows);
        let changed = self.is_expanded_changed.clone();
        let mut expander = Expander::new(self.is_expanded, move |app, value| changed(app, value))
            .header(header)
            .content(content)
            .is_enabled(self.is_enabled)
            // The template's bindings to the style's `MinWidth` and `MinHeight`, and `SettingsExpanderExpanderStyle`'s `Padding`.
            .min_width(SETTINGS_CARD_MIN_WIDTH)
            .min_height(SETTINGS_CARD_MIN_HEIGHT)
            .padding([0.0; 4])
            .template(settings_expander_expander_style);
        expander.expanding = self.expanded.clone();
        expander.collapsed = self.collapsed.clone();
        // The style's `HorizontalAlignment="Stretch"`, across a bounded width as a `SettingsCard` is.
        LayoutBuilder::new(move |_app, _context, constraints| {
            let width = constraints.max_width;
            if width.is_finite() {
                SizedBox::new()
                    .width(width)
                    .child(expander.clone())
                    .into_widget()
            } else {
                expander.clone().into_widget()
            }
        })
        .into_widget()
    }
}

impl SettingsExpander {
    /// `ItemsRepeater_ElementPrepared` with `SettingsExpanderItemStyleSelector`: a row that is a `SettingsCard` takes `ClickableSettingsExpanderItemStyle` when it can be clicked and `DefaultSettingsExpanderItemStyle` otherwise, under what the card sets itself; `IsEnabled` reaches it from the expander.
    fn item_container(&self, item: &WidgetRef) -> WidgetRef {
        let Some(card) = downcast_widget::<SettingsCard>(item.as_ref()) else {
            return item.clone();
        };
        let mut card = card.clone();
        card.border_thickness
            .get_or_insert(SETTINGS_EXPANDER_ITEM_BORDER_THICKNESS);
        card.min_height
            .get_or_insert(SETTINGS_EXPANDER_ITEM_MIN_HEIGHT);
        card.padding.get_or_insert(if card.is_click_enabled {
            CLICKABLE_SETTINGS_EXPANDER_ITEM_PADDING
        } else {
            SETTINGS_EXPANDER_ITEM_PADDING
        });
        card.corner_radius.get_or_insert(0.0);
        card.is_enabled = card.is_enabled && self.is_enabled;
        card.into_widget()
    }
}

/// `SettingsExpanderExpanderStyle`'s template: `ExpanderHeader` over `ExpanderContentClip`, at least the Expander's `MinWidth` wide.
fn settings_expander_expander_style(
    app: &mut App,
    context: BuildContext,
    expander: &Expander,
    states: ExpanderTemplateStates,
    parts: &ExpanderTemplateParts,
) -> WidgetRef {
    let resources = ThemeResources::of(app, context);
    let r = &*resources;
    let corner_radius = expander.corner_radius.unwrap_or(r.control_corner_radius[0]);
    let min_height = expander.min_height.unwrap_or(r.expander_min_height);
    let changed = expander.is_expanded_changed.clone();
    // `ExpanderHeader`: `IsChecked` bound both ways to `IsExpanded`.
    let expander_header = ToggleButton::new(Some(expander.is_expanded), move |app, value| {
        changed(app, value == Some(true))
    })
    .content(
        expander
            .header
            .clone()
            .unwrap_or_else(|| SizedBox::shrink().into_widget()),
    )
    .is_enabled(expander.is_enabled)
    .template(move |app, context, is_checked, content, common| {
        settings_expander_header_down_style(
            app,
            context,
            HeaderSetters {
                min_height,
                corner_radius,
                expand: states.expand,
            },
            is_checked == Some(true),
            content.unwrap_or_else(|| SizedBox::shrink().into_widget()),
            common,
        )
    });
    // `ExpanderContent`: a `Border` without background, bottom corners rounded.
    let expander_content =
        ConstrainedBox::new(BoxConstraints::new().min_height(SETTINGS_EXPANDER_CONTENT_MIN_HEIGHT))
            .child(
                ControlBorder::new_optional(
                    None,
                    Some(
                        expander
                            .border_brush
                            .unwrap_or(Brush::Solid(r.expander_content_border_brush)),
                    ),
                )
                .border_thickness_ltrb(r.expander_content_down_border_thickness)
                .corner_radius_corners([0.0, 0.0, corner_radius, corner_radius])
                .padding(expander.padding.unwrap_or(r.expander_content_padding))
                .child(Margin::new(
                    CONTENT_PRESENTER_MARGIN,
                    expander
                        .content
                        .clone()
                        .unwrap_or_else(|| SizedBox::shrink().into_widget()),
                )),
            );
    let grid = Grid::new()
        .row_definitions([
            RowDefinition::new(GridLength::AUTO),
            RowDefinition::new(GridLength::STAR),
        ])
        .children([
            GridCell::new(expander_header).into_widget(),
            GridCell::new(parts.expander_content_clip(expander_content))
                .row(1)
                .into_widget(),
        ]);
    ConstrainedBox::new(
        BoxConstraints::new().min_width(expander.min_width.unwrap_or(r.flyout_theme_min_width)),
    )
    .child(grid)
    .into_widget()
}

/// What `ExpanderHeader` binds from the Expander for `SettingsExpanderHeaderDownStyle`'s template.
#[derive(Clone, Copy)]
struct HeaderSetters {
    /// `MinHeight`.
    min_height: f64,
    /// `CornerRadius`, before `ExpandStates` squares the corners towards the content.
    corner_radius: f64,
    expand: ExpandState,
}

/// The brushes of one `CommonStates` state of `SettingsExpanderHeaderDownStyle`.
struct HeaderBrushes {
    /// `ToggleButtonGrid.Background`; `None` leaves the ToggleButton's own `Background`.
    background: Option<Color>,
    /// `ToggleButtonGrid.BorderBrush`.
    border_brush: Brush,
    /// `ContentPresenter.Foreground`.
    foreground: Color,
    /// `ExpandCollapseChevron.Foreground`.
    chevron: Color,
}

impl HeaderBrushes {
    /// The row of `is_checked` (`Normal` … `Disabled`, or `Checked` … `CheckedDisabled`) and the column of `state`.
    fn of(r: &ThemeResources, is_checked: bool, state: CommonState) -> HeaderBrushes {
        // `SettingsCardBorderBrush`, also the grid's own `ExpanderHeaderBorderBrush`.
        let border_brush = Brush::Solid(r.card_stroke_color_default_brush);
        match state {
            CommonState::Normal => HeaderBrushes {
                // `SettingsCardBackground`; `Checked` names no background.
                background: (!is_checked).then_some(r.card_background_fill_color_default_brush),
                border_brush,
                foreground: r.expander_header_foreground,
                chevron: r.expander_chevron_foreground,
            },
            CommonState::PointerOver => HeaderBrushes {
                // `SettingsCardBackgroundPointerOver` and `SettingsCardBorderBrushPointerOver`.
                background: Some(r.control_fill_color_secondary_brush),
                border_brush: Brush::ControlElevation(r.control_elevation_border_brush),
                foreground: r.expander_header_foreground_pointer_over,
                chevron: r.expander_chevron_pointer_over_foreground,
            },
            CommonState::Pressed => HeaderBrushes {
                // `SettingsCardBackgroundPressed` and `SettingsCardBorderBrushPressed`.
                background: Some(r.control_fill_color_tertiary_brush),
                border_brush: Brush::Solid(r.control_stroke_color_default_brush),
                foreground: r.expander_header_foreground_pressed,
                chevron: r.expander_chevron_pressed_foreground,
            },
            CommonState::Disabled => HeaderBrushes {
                background: None,
                // `SettingsCardBorderBrushDisabled`.
                border_brush: Brush::Solid(r.control_stroke_color_default_brush),
                foreground: r.expander_header_disabled_foreground,
                chevron: r.expander_header_disabled_foreground,
            },
        }
    }
}

/// `SettingsExpanderHeaderDownStyle`'s template: `ToggleButtonGrid` (columns `*` and Auto) with `ContentPresenter` and `ExpandCollapseChevronBorder`.
fn settings_expander_header_down_style(
    app: &mut App,
    context: BuildContext,
    setters: HeaderSetters,
    is_checked: bool,
    content: WidgetRef,
    states: ControlStates,
) -> WidgetRef {
    let resources = ThemeResources::of(app, context);
    let r = &*resources;
    let brushes = HeaderBrushes::of(r, is_checked, states.common);
    let radius = setters.corner_radius;
    // `ExpandDown` squares `ExpanderHeader`'s bottom corners through the corner filter.
    let radii = match setters.expand {
        ExpandState::ExpandDown => [radius, radius, 0.0, 0.0],
        ExpandState::ExpandUp => [0.0, 0.0, radius, radius],
        ExpandState::CollapseDown | ExpandState::CollapseUp => [radius; 4],
    };
    // `ExpandCollapseChevron`: the animated chevron, here the fallback glyph turned as `Expander` turns it.
    let chevron = AnimatedRotation::new(
        if is_checked { 0.5 } else { 0.0 },
        r.control_normal_animation_duration,
    )
    .child(
        FontIcon::symbol(FluentSymbol::ChevronDown)
            .font_size(CHEVRON_SIZE)
            .foreground(brushes.chevron),
    );
    // `ExpandCollapseChevronBorder`: a `ContentControl`, whose template shows neither its `Background` nor its border.
    let chevron_border = Margin::new(
        CHEVRON_MARGIN,
        Center::new()
            .width_factor(1.0)
            .height_factor(1.0)
            .child(ToolTipService::new(
                SizedBox::new()
                    .width(SETTINGS_EXPANDER_CHEVRON_BUTTON_WIDTH)
                    .height(SETTINGS_EXPANDER_CHEVRON_BUTTON_HEIGHT)
                    .child(
                        Center::new().child(
                            SizedBox::new()
                                .width(CHEVRON_SIZE)
                                .height(CHEVRON_SIZE)
                                .child(chevron),
                        ),
                    ),
                ToolTip::text(SETTINGS_EXPANDER_CHEVRON_TOOL_TIP),
            )),
    );
    // `ContentPresenter`: across the column (`ExpanderHeaderHorizontalContentAlignment`) and centred down it.
    let content_presenter = Align::new()
        .alignment(AlignmentGeometry::CENTER_LEFT)
        .height_factor(1.0)
        .child(DefaultTextStyle::new(
            control_text_style(
                r,
                r.control_content_theme_font_size,
                FontWeight::W400,
                brushes.foreground,
            ),
            content,
        ))
        .into_widget();
    // The ToggleButton's own `Background` is `DefaultToggleButtonStyle`'s, which the header style does not replace.
    let background = brushes.background.unwrap_or(r.toggle_button_background);
    let toggle_button_grid = ControlBorder::new(Brush::Solid(background), brushes.border_brush)
        .border_thickness_ltrb(r.expander_header_border_thickness)
        .corner_radius_corners(radii)
        .child(
            Grid::new()
                .column_definitions([
                    ColumnDefinition::new(GridLength::STAR),
                    ColumnDefinition::new(GridLength::AUTO),
                ])
                .children([
                    GridCell::new(content_presenter).into_widget(),
                    GridCell::new(chevron_border).column(1).into_widget(),
                ]),
        );
    FocusVisual::new(
        ConstrainedBox::new(BoxConstraints::new().min_height(setters.min_height))
            .child(toggle_button_grid),
        r.theme,
    )
    .visible(states.focused)
    .into_widget()
}
