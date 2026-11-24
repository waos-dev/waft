//! The Toolkit's `SettingsCard` with `DefaultSettingsCardStyle` of `SettingsCard/SettingsCard.xaml`: one setting as a card, its name and description at the start and its control at the end, or a card to click through.
//!
//! Template, element by element: `PART_RootGrid` (rows `*` and Auto; columns Auto, `*`, Auto, Auto; the card's background, border, corner radius and padding, with an 83 ms `BrushTransition` on the background) holds `PART_HeaderIconPresenterHolder` (a `Viewbox` of at most 20 × 20 in column 0), `HeaderPanel` (column 1, a vertical `StackPanel` of `PART_HeaderPresenter` and `PART_DescriptionPresenter`, centred down the row), `PART_ContentPresenter` (column 2, at the right and centred) and `PART_ActionIconPresenterHolder` (a `Viewbox` of at most 13 × 13 in column 3, over both rows).
//!
//! Visual state groups: `CommonStates` swaps the background, border and foregrounds, and leaves `Normal` unless the card can be clicked; `ContentAlignmentStates` moves the content under the header ([`ContentAlignmentState`]); `ContentSpacingStates` puts `SettingsCardVerticalHeaderContentSpacing` between the rows once the content is under a header or description; `ContentVisibilityStates` drops the content presenter of a card without content.
//!
//! Behaviour is `SettingsCard.cs`: a `ButtonBase` that takes pointer and keyboard input only while `IsClickEnabled`, shows its action icon only then, and collapses the presenters of a header, description and header icon that are not set.

use crate::{
    BackgroundSizing, Brush, ColorTransition, ColumnDefinition, CommonState, CommonStates,
    ControlStates, FLUENT_ICON_FONT_FAMILY, FluentSymbol, FocusVisual, FontIcon, Grid, GridCell,
    GridLength, HorizontalAlignment, Margin, RowDefinition, ThemeResources, ToolTip,
    ToolTipService, control_text_style,
};
use std::fmt;
use waft_embedder::{Color, FontWeight};
use waft_foundation::{App, Listener};
use waft_painting::{AlignmentGeometry, TextStyle};
use waft_rendering::BoxConstraints;
use waft_widgets::*;

/// `SettingsCardBorderThickness`: left, top, right, bottom.
pub const SETTINGS_CARD_BORDER_THICKNESS: [f64; 4] = [1.0; 4];
/// `SettingsCardPadding`: left, top, right, bottom.
pub const SETTINGS_CARD_PADDING: [f64; 4] = [16.0; 4];
/// `SettingsCardMinWidth`.
pub const SETTINGS_CARD_MIN_WIDTH: f64 = 148.0;
/// `SettingsCardMinHeight`.
pub const SETTINGS_CARD_MIN_HEIGHT: f64 = 68.0;
/// `SettingsCardDescriptionFontSize`.
pub const SETTINGS_CARD_DESCRIPTION_FONT_SIZE: f64 = 12.0;
/// `SettingsCardHeaderIconMaxSize`.
pub const SETTINGS_CARD_HEADER_ICON_MAX_SIZE: f64 = 20.0;
/// `SettingsCardLeftIndention`: the least width of the header icon's column.
pub const SETTINGS_CARD_LEFT_INDENTION: f64 = 0.0;
/// `SettingsCardContentMinWidth`: the `MinWidth` the card's style gives a `Slider`, `ComboBox` or `TextBox` in it.
pub const SETTINGS_CARD_CONTENT_MIN_WIDTH: f64 = 120.0;
/// `SettingsCardHeaderIconMargin`: left, top, right, bottom.
pub const SETTINGS_CARD_HEADER_ICON_MARGIN: [f64; 4] = [2.0, 0.0, 20.0, 0.0];
/// `SettingsCardActionIconMargin`: left, top, right, bottom.
pub const SETTINGS_CARD_ACTION_ICON_MARGIN: [f64; 4] = [14.0, 0.0, 0.0, 0.0];
/// `SettingsCardActionIconMaxSize`.
pub const SETTINGS_CARD_ACTION_ICON_MAX_SIZE: f64 = 13.0;
/// `SettingsCardVerticalHeaderContentSpacing`.
pub const SETTINGS_CARD_VERTICAL_HEADER_CONTENT_SPACING: f64 = 8.0;
/// `SettingsCardWrapThreshold`: a card narrower than this shows its content under its header.
pub const SETTINGS_CARD_WRAP_THRESHOLD: f64 = 476.0;
/// `SettingsCardWrapNoIconThreshold`: a card narrower than this also hides its header icon.
pub const SETTINGS_CARD_WRAP_NO_ICON_THRESHOLD: f64 = 286.0;
/// `HeaderPanel`'s `Margin` while the content is beside it: left, top, right, bottom.
const HEADER_PANEL_MARGIN: [f64; 4] = [0.0, 0.0, 24.0, 0.0];
/// `FocusVisualMargin` of the style.
const FOCUS_VISUAL_MARGIN: [f64; 4] = [-3.0; 4];

/// The Toolkit's `ContentAlignment`: where a [`SettingsCard`] puts its content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ContentAlignment {
    /// The content is at the right of the header.
    #[default]
    Right,
    /// The content is at the left, and the header, description and header icon are not shown; for a check box, radio buttons or a layout of the app's own.
    Left,
    /// The content is under the header and description.
    Vertical,
}

/// The `ContentAlignmentStates` group, picked from [`ContentAlignment`] and the card's width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentAlignmentState {
    Right,
    /// The card is narrower than `SettingsCardWrapThreshold`: the content is under the header, at the left.
    RightWrapped,
    /// The card is narrower than `SettingsCardWrapNoIconThreshold`: wrapped, and the header icon is not shown.
    RightWrappedNoIcon,
    Left,
    Vertical,
}

impl ContentAlignmentState {
    /// The state for a card `width` wide. Its triggers are all custom ones, so where two are active the one the XAML declares first wins: a narrow card wraps whatever its `ContentAlignment`.
    pub fn of(content_alignment: ContentAlignment, width: f64) -> ContentAlignmentState {
        if width < SETTINGS_CARD_WRAP_NO_ICON_THRESHOLD {
            ContentAlignmentState::RightWrappedNoIcon
        } else if width < SETTINGS_CARD_WRAP_THRESHOLD {
            ContentAlignmentState::RightWrapped
        } else {
            match content_alignment {
                ContentAlignment::Right => ContentAlignmentState::Right,
                ContentAlignment::Left => ContentAlignmentState::Left,
                ContentAlignment::Vertical => ContentAlignmentState::Vertical,
            }
        }
    }

    /// Whether the state hides `PART_HeaderIconPresenterHolder`.
    fn collapses_header_icon(self) -> bool {
        matches!(
            self,
            ContentAlignmentState::RightWrappedNoIcon | ContentAlignmentState::Left
        )
    }

    /// Whether the state hides `PART_HeaderPresenter` and `PART_DescriptionPresenter`.
    fn collapses_header(self) -> bool {
        self == ContentAlignmentState::Left
    }
}

/// The Toolkit's `SettingsCard`: `Header`, `Description` and `HeaderIcon` at the start, `Content` at the end, and with `IsClickEnabled` a `Click` on the whole card, marked by `ActionIcon`.
#[derive(Clone)]
pub struct SettingsCard {
    /// `Header`: the setting's name; nothing is shown for it when unset.
    pub header: Option<WidgetRef>,
    /// `Description`: the text under the header, which wraps; nothing is shown for it when unset.
    pub description: Option<WidgetRef>,
    /// `HeaderIcon`: the icon before the header, scaled into 20 × 20.
    pub header_icon: Option<WidgetRef>,
    /// `ActionIcon`: the icon at the end of a card that can be clicked, scaled into 13 × 13; a chevron pointing on by default.
    pub action_icon: WidgetRef,
    /// `ActionIconToolTip`: the tooltip of the action icon.
    pub action_icon_tool_tip: Option<String>,
    /// `IsClickEnabled`: whether the card itself is a button.
    pub is_click_enabled: bool,
    /// `ContentAlignment`.
    pub content_alignment: ContentAlignment,
    /// `IsActionIconVisible`: whether a card that can be clicked shows its action icon.
    pub is_action_icon_visible: bool,
    /// `Content`: the setting's control.
    pub content: Option<WidgetRef>,
    /// `Click` of `ButtonBase`, raised only while `is_click_enabled`.
    pub click: Option<Listener>,
    /// `IsEnabled`.
    pub is_enabled: bool,
    /// `HorizontalContentAlignment`, which places the content across the card in the [`ContentAlignment::Vertical`] layout; `Right` in the style.
    pub horizontal_content_alignment: HorizontalAlignment,
    /// `Background`; `SettingsCardBackground` when `None`.
    pub background: Option<Color>,
    /// `BorderThickness`: left, top, right, bottom; `SettingsCardBorderThickness` when `None`.
    pub border_thickness: Option<[f64; 4]>,
    /// `CornerRadius` of every corner; the theme's `ControlCornerRadius` when `None`.
    pub corner_radius: Option<f64>,
    /// `MinHeight`; `SettingsCardMinHeight` when `None`.
    pub min_height: Option<f64>,
    /// `Padding`: left, top, right, bottom; `SettingsCardPadding` when `None`.
    pub padding: Option<[f64; 4]>,
    pub key: Option<KeyRef>,
}

impl Default for SettingsCard {
    fn default() -> SettingsCard {
        SettingsCard {
            header: None,
            description: None,
            header_icon: None,
            // The constructor's `FontIcon`, glyph U+E974 mirrored in right-to-left layout.
            action_icon: FontIcon::symbol(FluentSymbol::ChevronRight)
                .mirrored_when_right_to_left(true)
                .into_widget(),
            action_icon_tool_tip: None,
            is_click_enabled: false,
            content_alignment: ContentAlignment::Right,
            is_action_icon_visible: true,
            content: None,
            click: None,
            is_enabled: true,
            horizontal_content_alignment: HorizontalAlignment::Right,
            background: None,
            border_thickness: None,
            corner_radius: None,
            min_height: None,
            padding: None,
            key: None,
        }
    }
}

impl SettingsCard {
    pub fn new() -> SettingsCard {
        SettingsCard::default()
    }

    /// XAML `Header`.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn header<K>(mut self, header: impl IntoWidget<K>) -> SettingsCard {
        self.header = Some(header.into_widget());
        self
    }

    /// XAML `Description`.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn description<K>(mut self, description: impl IntoWidget<K>) -> SettingsCard {
        self.description = Some(description.into_widget());
        self
    }

    /// XAML `HeaderIcon`.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn header_icon<K>(mut self, header_icon: impl IntoWidget<K>) -> SettingsCard {
        self.header_icon = Some(header_icon.into_widget());
        self
    }

    /// XAML `ActionIcon`.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn action_icon<K>(mut self, action_icon: impl IntoWidget<K>) -> SettingsCard {
        self.action_icon = action_icon.into_widget();
        self
    }

    /// XAML `ActionIconToolTip`.
    pub fn action_icon_tool_tip(mut self, tool_tip: impl Into<String>) -> SettingsCard {
        self.action_icon_tool_tip = Some(tool_tip.into());
        self
    }

    /// XAML `IsClickEnabled`.
    pub fn is_click_enabled(mut self, is_click_enabled: bool) -> SettingsCard {
        self.is_click_enabled = is_click_enabled;
        self
    }

    /// XAML `ContentAlignment`.
    pub fn content_alignment(mut self, content_alignment: ContentAlignment) -> SettingsCard {
        self.content_alignment = content_alignment;
        self
    }

    /// XAML `IsActionIconVisible`.
    pub fn is_action_icon_visible(mut self, is_action_icon_visible: bool) -> SettingsCard {
        self.is_action_icon_visible = is_action_icon_visible;
        self
    }

    /// XAML `Content`.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn content<K>(mut self, content: impl IntoWidget<K>) -> SettingsCard {
        self.content = Some(content.into_widget());
        self
    }

    /// XAML `Click`.
    pub fn click(mut self, click: Listener) -> SettingsCard {
        self.click = Some(click);
        self
    }

    /// XAML `IsEnabled`.
    pub fn is_enabled(mut self, is_enabled: bool) -> SettingsCard {
        self.is_enabled = is_enabled;
        self
    }

    /// XAML `HorizontalContentAlignment`.
    pub fn horizontal_content_alignment(mut self, alignment: HorizontalAlignment) -> SettingsCard {
        self.horizontal_content_alignment = alignment;
        self
    }

    /// XAML `Background`.
    pub fn background(mut self, background: Color) -> SettingsCard {
        self.background = Some(background);
        self
    }

    /// XAML `BorderThickness`.
    pub fn border_thickness(mut self, border_thickness: [f64; 4]) -> SettingsCard {
        self.border_thickness = Some(border_thickness);
        self
    }

    /// XAML `CornerRadius`.
    pub fn corner_radius(mut self, corner_radius: f64) -> SettingsCard {
        self.corner_radius = Some(corner_radius);
        self
    }

    /// XAML `MinHeight`.
    pub fn min_height(mut self, min_height: f64) -> SettingsCard {
        self.min_height = Some(min_height);
        self
    }

    /// XAML `Padding`.
    pub fn padding(mut self, padding: [f64; 4]) -> SettingsCard {
        self.padding = Some(padding);
        self
    }

    pub fn key(mut self, key: KeyRef) -> SettingsCard {
        self.key = Some(key);
        self
    }
}

impl fmt::Debug for SettingsCard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsCard")
            .field("is_click_enabled", &self.is_click_enabled)
            .field("content_alignment", &self.content_alignment)
            .field("is_enabled", &self.is_enabled)
            .finish_non_exhaustive()
    }
}

impl StatelessWidget for SettingsCard {
    fn key(&self) -> Option<&KeyRef> {
        self.key.as_ref()
    }

    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        let card = self.clone();
        // The `ControlSizeTrigger`s watch `PART_RootGrid`'s width, which the style's `HorizontalAlignment="Stretch"` makes the width the card is given.
        LayoutBuilder::new(move |app, context, constraints| {
            let width = constraints.max_width;
            if !card.is_click_enabled {
                // `DisableButtonInteraction`: no pointer or key handler is attached, so `CommonStates` leaves `Normal` only for `Disabled`.
                let states = ControlStates {
                    common: if card.is_enabled {
                        CommonState::Normal
                    } else {
                        CommonState::Disabled
                    },
                    focused: false,
                };
                return while_enabled(
                    card.is_enabled,
                    template(app, context, &card, states, width),
                );
            }
            let click = card.click.clone().unwrap_or_else(|| Listener::new(|_| {}));
            let templated = card.clone();
            let button = CommonStates::new(click, move |app, context, states| {
                template(app, context, &templated, states, width)
            })
            .is_enabled(card.is_enabled);
            while_enabled(card.is_enabled, button)
        })
        .into_widget()
    }
}

/// `IsEnabled` reaches everything in a control: a disabled card's content takes no pointer and no focus.
#[cfg_attr(debug_assertions, track_caller)]
fn while_enabled<K>(is_enabled: bool, child: impl IntoWidget<K>) -> WidgetRef {
    ExcludeFocus::new(IgnorePointer::new().ignoring(!is_enabled).child(child))
        .excluding(!is_enabled)
        .into_widget()
}

/// The brushes of one `CommonStates` state: the `{ThemeResource SettingsCard…}` values its storyboard names, each the WinUI brush the Toolkit's theme dictionaries alias.
struct CardBrushes {
    /// `PART_RootGrid.Background`; `None` leaves the card's `Background`.
    background: Option<Color>,
    /// `PART_RootGrid.BorderBrush`.
    border_brush: Brush,
    /// `PART_HeaderPresenter.Foreground` and `PART_HeaderIconPresenter.Foreground`.
    header: Color,
    /// `PART_DescriptionPresenter.Foreground`.
    description: Color,
    /// `PART_ActionIconPresenter.Foreground`.
    action_icon: Color,
}

impl CardBrushes {
    fn of(r: &ThemeResources, state: CommonState) -> CardBrushes {
        // `SettingsCardForeground`, the style's `Foreground`, and the description presenter's own.
        let foreground = r.text_fill_color_primary_brush;
        let description = r.text_fill_color_secondary_brush;
        // `SettingsCardBorderBrush`, the style's `BorderBrush`.
        let border_brush = Brush::Solid(r.card_stroke_color_default_brush);
        match state {
            CommonState::Normal => CardBrushes {
                background: None,
                border_brush,
                header: foreground,
                description,
                action_icon: foreground,
            },
            CommonState::PointerOver => CardBrushes {
                // `SettingsCardBackgroundPointerOver`.
                background: Some(r.control_fill_color_secondary_brush),
                // `SettingsCardBorderBrushPointerOver`.
                border_brush: Brush::ControlElevation(r.control_elevation_border_brush),
                // `SettingsCardForegroundPointerOver`.
                header: r.text_fill_color_primary_brush,
                description,
                action_icon: foreground,
            },
            CommonState::Pressed => CardBrushes {
                // `SettingsCardBackgroundPressed`.
                background: Some(r.control_fill_color_tertiary_brush),
                // `SettingsCardBorderBrushPressed`.
                border_brush: Brush::Solid(r.control_stroke_color_default_brush),
                // `SettingsCardForegroundPressed`.
                header: r.text_fill_color_secondary_brush,
                description: r.text_fill_color_secondary_brush,
                action_icon: foreground,
            },
            // `SettingsCardForegroundDisabled`; the state names no background or border.
            CommonState::Disabled => CardBrushes {
                background: None,
                border_brush,
                header: r.text_fill_color_disabled_brush,
                description: r.text_fill_color_disabled_brush,
                action_icon: r.text_fill_color_disabled_brush,
            },
        }
    }
}

/// `DefaultSettingsCardStyle`'s `ControlTemplate` for a card `width` wide in `states`.
fn template(
    app: &mut App,
    context: BuildContext,
    card: &SettingsCard,
    states: ControlStates,
    width: f64,
) -> WidgetRef {
    let resources = ThemeResources::of(app, context);
    let r = &*resources;
    let brushes = CardBrushes::of(r, states.common);
    let alignment = ContentAlignmentState::of(card.content_alignment, width);
    let header = card
        .header
        .clone()
        .filter(|_| !alignment.collapses_header());
    let description = card
        .description
        .clone()
        .filter(|_| !alignment.collapses_header());
    // `CheckVerticalSpacingState`, which reads the properties and not what the state shows.
    let content_spacing = alignment != ContentAlignmentState::Right
        && alignment != ContentAlignmentState::Left
        && card.content.is_some()
        && (card.header.is_some() || card.description.is_some());

    let mut children = Vec::new();
    if let Some(icon) = card.header_icon.clone()
        && !alignment.collapses_header_icon()
    {
        children.push(header_icon_presenter_holder(r, icon, brushes.header));
    }
    children.push(header_panel(r, &brushes, alignment, header, description));
    if let Some(content) = card.content.clone() {
        children.push(content_presenter(r, card, alignment, content));
    }
    if card.is_click_enabled && card.is_action_icon_visible {
        children.push(action_icon_presenter_holder(r, card, brushes.action_icon));
    }

    let corner_radius = card.corner_radius.unwrap_or(r.control_corner_radius[0]);
    let border_thickness = card
        .border_thickness
        .unwrap_or(SETTINGS_CARD_BORDER_THICKNESS);
    let padding = card.padding.unwrap_or(SETTINGS_CARD_PADDING);
    let border_brush = brushes.border_brush;
    // `SettingsCardBackground`, the style's `Background`.
    let background = brushes
        .background
        .or(card.background)
        .unwrap_or(r.card_background_fill_color_default_brush);
    // `PART_RootGrid`, whose `BackgroundTransition` is a `BrushTransition` of 83 ms.
    let root_grid = ColorTransition::new(
        background,
        r.control_faster_animation_duration,
        move |_app, background| {
            Grid::new()
                .row_definitions([
                    RowDefinition::new(GridLength::STAR),
                    RowDefinition::new(GridLength::AUTO),
                ])
                .column_definitions([
                    ColumnDefinition::new(GridLength::AUTO).min_width(SETTINGS_CARD_LEFT_INDENTION),
                    ColumnDefinition::new(GridLength::STAR),
                    ColumnDefinition::new(GridLength::AUTO),
                    ColumnDefinition::new(GridLength::AUTO),
                ])
                .row_spacing(if content_spacing {
                    SETTINGS_CARD_VERTICAL_HEADER_CONTENT_SPACING
                } else {
                    0.0
                })
                .background(Brush::Solid(background))
                .background_sizing(BackgroundSizing::InnerBorderEdge)
                .border_brush(border_brush)
                .border_thickness_ltrb(border_thickness)
                .corner_radius(corner_radius)
                .padding(padding)
                .children(children.clone())
                .into_widget()
        },
    );
    // The style's `MinWidth` and `MinHeight`, and its `HorizontalAlignment="Stretch"` across a bounded width.
    let min_width = if width.is_finite() {
        width.max(SETTINGS_CARD_MIN_WIDTH)
    } else {
        SETTINGS_CARD_MIN_WIDTH
    };
    let sized = ConstrainedBox::new(
        BoxConstraints::new()
            .min_width(min_width)
            .min_height(card.min_height.unwrap_or(SETTINGS_CARD_MIN_HEIGHT)),
    )
    .child(root_grid);
    FocusVisual::new(sized, r.theme)
        .visible(states.focused)
        .margin(FOCUS_VISUAL_MARGIN)
        .corner_radius(corner_radius)
        .into_widget()
}

/// A `Viewbox` with `MaxWidth` and `MaxHeight` `max_size`: `icon` scaled to fit that square, with the presenter's `text` style for what it does not set itself.
fn icon_viewbox(icon: WidgetRef, max_size: f64, text: TextStyle) -> WidgetRef {
    SizedBox::new()
        .width(max_size)
        .height(max_size)
        .child(FittedBox::new().child(DefaultTextStyle::new(text, icon)))
        .into_widget()
}

/// `PART_HeaderIconPresenterHolder`: the header icon in column 0 of the first row, centred down it.
fn header_icon_presenter_holder(
    r: &ThemeResources,
    icon: WidgetRef,
    foreground: Color,
) -> WidgetRef {
    let text = control_text_style(
        r,
        r.control_content_theme_font_size,
        FontWeight::W400,
        foreground,
    );
    GridCell::new(Margin::new(
        SETTINGS_CARD_HEADER_ICON_MARGIN,
        Center::new()
            .width_factor(1.0)
            .height_factor(1.0)
            .child(icon_viewbox(icon, SETTINGS_CARD_HEADER_ICON_MAX_SIZE, text)),
    ))
    .into_widget()
}

/// `HeaderPanel`: `PART_HeaderPresenter` over `PART_DescriptionPresenter` in column 1, centred down the row; both wrap. Its right margin goes once the content is under it.
fn header_panel(
    r: &ThemeResources,
    brushes: &CardBrushes,
    alignment: ContentAlignmentState,
    header: Option<WidgetRef>,
    description: Option<WidgetRef>,
) -> WidgetRef {
    let header = header.map(|header| {
        DefaultTextStyle::new(
            control_text_style(
                r,
                r.control_content_theme_font_size,
                FontWeight::W400,
                brushes.header,
            ),
            header,
        )
        .into_widget()
    });
    let description = description.map(|description| {
        DefaultTextStyle::new(
            control_text_style(
                r,
                SETTINGS_CARD_DESCRIPTION_FONT_SIZE,
                FontWeight::W400,
                brushes.description,
            ),
            description,
        )
        .into_widget()
    });
    let wrapped = matches!(
        alignment,
        ContentAlignmentState::RightWrapped | ContentAlignmentState::RightWrappedNoIcon
    );
    let panel = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children(header.into_iter().chain(description).collect::<Vec<_>>());
    GridCell::new(Margin::new(
        if wrapped {
            [0.0; 4]
        } else {
            HEADER_PANEL_MARGIN
        },
        Align::new()
            .alignment(AlignmentGeometry::CENTER_LEFT)
            .width_factor(1.0)
            .height_factor(1.0)
            .child(panel),
    ))
    .column(1)
    .into_widget()
}

/// `PART_ContentPresenter`: at the right of the first row, centred down it; or, in every other state, in the second row under the header, where the state says how it sits across the card.
fn content_presenter(
    r: &ThemeResources,
    card: &SettingsCard,
    alignment: ContentAlignmentState,
    content: WidgetRef,
) -> WidgetRef {
    // The style's `Foreground`, `FontFamily`, `FontWeight` and `FontSize`, which content inherits.
    let content = DefaultTextStyle::new(
        control_text_style(
            r,
            r.control_content_theme_font_size,
            FontWeight::W400,
            r.text_fill_color_primary_brush,
        ),
        content,
    );
    let aligned = |alignment: AlignmentGeometry| {
        Align::new()
            .alignment(alignment)
            .width_factor(1.0)
            .height_factor(1.0)
            .child(content.clone())
            .into_widget()
    };
    let across = match alignment {
        ContentAlignmentState::Right => {
            return GridCell::new(aligned(AlignmentGeometry::CENTER_RIGHT))
                .column(2)
                .into_widget();
        }
        // `HorizontalAlignment="Stretch"` with `HorizontalContentAlignment="Left"`.
        ContentAlignmentState::RightWrapped | ContentAlignmentState::RightWrappedNoIcon => {
            HorizontalAlignment::Left
        }
        ContentAlignmentState::Left => HorizontalAlignment::Left,
        // `HorizontalAlignment="Stretch"` with the card's `HorizontalContentAlignment`.
        ContentAlignmentState::Vertical => card.horizontal_content_alignment,
    };
    let presenter = match across {
        HorizontalAlignment::Left => aligned(AlignmentGeometry::CENTER_LEFT),
        HorizontalAlignment::Center => aligned(AlignmentGeometry::CENTER),
        HorizontalAlignment::Right => aligned(AlignmentGeometry::CENTER_RIGHT),
        HorizontalAlignment::Stretch => content.into_widget(),
    };
    GridCell::new(presenter).row(1).column(1).into_widget()
}

/// `PART_ActionIconPresenterHolder`: the action icon in column 3, centred down both rows, with `ActionIconToolTip`.
fn action_icon_presenter_holder(
    r: &ThemeResources,
    card: &SettingsCard,
    foreground: Color,
) -> WidgetRef {
    // `FontFamily="{ThemeResource SymbolThemeFontFamily}"` at the style's `FontSize`.
    let text = TextStyle::new()
        .font_family(FLUENT_ICON_FONT_FAMILY)
        .font_size(r.control_content_theme_font_size)
        .color(foreground);
    let icon = icon_viewbox(
        card.action_icon.clone(),
        SETTINGS_CARD_ACTION_ICON_MAX_SIZE,
        text,
    );
    let icon = match card.action_icon_tool_tip.clone() {
        Some(tool_tip) => ToolTipService::new(icon, ToolTip::text(tool_tip)).into_widget(),
        None => icon,
    };
    GridCell::new(Margin::new(
        SETTINGS_CARD_ACTION_ICON_MARGIN,
        Center::new()
            .width_factor(1.0)
            .height_factor(1.0)
            .child(icon),
    ))
    .column(3)
    .row_span(2)
    .into_widget()
}
