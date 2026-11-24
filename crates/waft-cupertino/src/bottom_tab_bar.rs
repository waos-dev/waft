//! Flutter counterpart: `cupertino/bottom_tab_bar.dart`.

use std::fmt::{self, Debug};
use std::rc::Rc;

use waft_embedder::{Color, ImageFilter, Size};
use waft_foundation::{App, K_IS_WEB, Listener, ValueChanged};
use waft_painting::{
    AnyColor, Border, BorderSide, BorderStyle, BoxDecoration, EdgeInsets, TextStyle,
};
use waft_rendering::{CrossAxisAlignment, HitTestBehavior, MainAxisAlignment};
use waft_services::{MouseCursor, SystemMouseCursors};
use waft_widgets::{
    AnySize, BackdropFilter, BottomNavigationBarItem, BuildContext, Center, ClipRect, Column,
    DecoratedBox, DefaultTextStyle, Expanded, GestureDetector, IconTheme, IconThemeData,
    IntoWidget, KeyRef, MediaQuery, MouseRegion, Padding, PreferredSizeWidget, Row, Semantics,
    SizedBox, StatelessWidget, Text, TextFieldTapRegion, WidgetRef, debug_check_has_media_query,
};

use crate::colors::{CupertinoColors, CupertinoDynamicColor};
use crate::localizations::CupertinoLocalizations;
use crate::nav_bar::alpha;
use crate::theme::CupertinoTheme;

// Standard iOS 10 tab bar height.
const K_TAB_BAR_HEIGHT: f64 = 50.0;

const K_DEFAULT_TAB_BAR_BORDER_COLOR: CupertinoDynamicColor =
    CupertinoDynamicColor::with_brightness(Color::new(0x4D000000), Color::new(0x29000000));
const K_DEFAULT_TAB_BAR_INACTIVE_COLOR: AnyColor = CupertinoColors::INACTIVE_GRAY;

/// Dart's `Border(top:)` of the constructor's default: only the top side is drawn. A
/// [`BorderSide`] carries a plain color, so the side holds the dynamic color's light value, and
/// [`CupertinoTabBar::build`](StatelessWidget::build) resolves the dynamic color itself for a
/// border that is still this one.
const K_DEFAULT_TAB_BAR_BORDER: Border = Border {
    top: BorderSide {
        color: K_DEFAULT_TAB_BAR_BORDER_COLOR.color,
        // 0.0 means one physical pixel
        width: 0.0,
        style: BorderStyle::Solid,
        stroke_align: BorderSide::STROKE_ALIGN_INSIDE,
    },
    right: BorderSide::NONE,
    bottom: BorderSide::NONE,
    left: BorderSide::NONE,
};

/// An iOS-styled bottom navigation tab bar.
///
/// Displays multiple tabs using [`BottomNavigationBarItem`] with one tab being active, the first
/// tab by default.
///
/// This [`StatelessWidget`] doesn't store the active tab itself. You must listen to the
/// [`on_tap`](Self::on_tap) callbacks and call `set_state` with a new
/// [`current_index`](Self::current_index) for the new selection to reflect. This can also be done
/// automatically by wrapping this with a `CupertinoTabScaffold`.
///
/// Tab changes typically trigger a switch between `Navigator`s, each with its own navigation
/// stack, per standard iOS design. This can be done by using `CupertinoTabView`s inside each tab
/// builder in `CupertinoTabScaffold`.
///
/// If the given [`background_color`](Self::background_color)'s opacity is not 1.0 (which is the
/// case by default), it will produce a blurring effect to the content behind it.
///
/// When used as `CupertinoTabScaffold.tabBar`, by default [`CupertinoTabBar`] disables text
/// scaling to match the native iOS behavior. To override this behavior, wrap each of the
/// `navigationBar`'s components inside a [`MediaQuery`] with the desired `TextScaler`.
///
/// See also:
///
///  * `CupertinoTabScaffold`, which hosts the [`CupertinoTabBar`] at the bottom.
///  * [`BottomNavigationBarItem`], an item in a [`CupertinoTabBar`].
///  * <https://developer.apple.com/design/human-interface-guidelines/ios/bars/tab-bars/>
#[derive(Clone)]
pub struct CupertinoTabBar {
    pub key: Option<KeyRef>,

    /// The interactive items laid out within the bottom navigation bar.
    pub items: Vec<BottomNavigationBarItem>,

    /// The callback that is called when a item is tapped.
    ///
    /// The widget creating the bottom navigation bar needs to keep track of the current index
    /// and call `set_state` to rebuild it with the newly provided index.
    pub on_tap: Option<ValueChanged<usize>>,

    /// The index into [`items`](Self::items) of the current active item.
    ///
    /// Must be between 0 and the number of tabs minus 1, inclusive.
    pub current_index: usize,

    /// The background color of the tab bar. If it contains transparency, the tab bar will
    /// automatically produce a blurring effect to the content behind it.
    ///
    /// Defaults to [`CupertinoTheme`]'s `bar_background_color` when null.
    pub background_color: Option<AnyColor>,

    /// The foreground color of the icon and title for the [`BottomNavigationBarItem`] of the
    /// selected tab.
    ///
    /// Defaults to [`CupertinoTheme`]'s `primary_color` if null.
    pub active_color: Option<AnyColor>,

    /// The foreground color of the icon and title for the [`BottomNavigationBarItem`]s in the
    /// unselected state.
    ///
    /// Defaults to a [`CupertinoDynamicColor`] that matches the disabled foreground color of the
    /// native `UITabBar` component.
    pub inactive_color: AnyColor,

    /// The size of all of the [`BottomNavigationBarItem`] icons.
    ///
    /// This value is used to configure the [`IconTheme`] for the navigation bar. When a
    /// [`BottomNavigationBarItem::icon`] widget is not an `Icon` the widget should configure
    /// itself to match the icon theme's size and color.
    pub icon_size: f64,

    /// The height of the [`CupertinoTabBar`].
    ///
    /// Defaults to 50.
    pub height: f64,

    /// The border of the [`CupertinoTabBar`].
    ///
    /// The default value is a one physical pixel top border with grey color.
    pub border: Option<Border>,
}

impl CupertinoTabBar {
    /// Creates a tab bar in the iOS style.
    pub fn new(items: Vec<BottomNavigationBarItem>) -> CupertinoTabBar {
        debug_assert!(
            !items.is_empty(),
            "CupertinoTabBar requires at least one tab."
        );
        CupertinoTabBar {
            key: None,
            items,
            on_tap: None,
            current_index: 0,
            background_color: None,
            active_color: None,
            inactive_color: K_DEFAULT_TAB_BAR_INACTIVE_COLOR,
            icon_size: 30.0,
            height: K_TAB_BAR_HEIGHT,
            border: Some(K_DEFAULT_TAB_BAR_BORDER),
        }
    }

    /// Dart `CupertinoTabBar(key:)`.
    pub fn key(mut self, key: KeyRef) -> CupertinoTabBar {
        self.key = Some(key);
        self
    }

    /// Dart `CupertinoTabBar.copyWith(items:)`.
    pub fn items(mut self, items: Vec<BottomNavigationBarItem>) -> CupertinoTabBar {
        debug_assert!(
            !items.is_empty(),
            "CupertinoTabBar requires at least one tab."
        );
        self.items = items;
        self
    }

    /// Dart `CupertinoTabBar(onTap:)`.
    pub fn on_tap(mut self, on_tap: impl Fn(&mut App, usize) + 'static) -> CupertinoTabBar {
        self.on_tap = Some(Rc::new(on_tap));
        self
    }

    /// Dart `CupertinoTabBar(currentIndex:)`.
    pub fn current_index(mut self, current_index: usize) -> CupertinoTabBar {
        self.current_index = current_index;
        self
    }

    /// Dart `CupertinoTabBar(backgroundColor:)`.
    pub fn background_color(mut self, background_color: impl Into<AnyColor>) -> CupertinoTabBar {
        self.background_color = Some(background_color.into());
        self
    }

    /// Dart `CupertinoTabBar(activeColor:)`.
    pub fn active_color(mut self, active_color: impl Into<AnyColor>) -> CupertinoTabBar {
        self.active_color = Some(active_color.into());
        self
    }

    /// Dart `CupertinoTabBar(inactiveColor:)`.
    pub fn inactive_color(mut self, inactive_color: impl Into<AnyColor>) -> CupertinoTabBar {
        self.inactive_color = inactive_color.into();
        self
    }

    /// Dart `CupertinoTabBar(iconSize:)`.
    pub fn icon_size(mut self, icon_size: f64) -> CupertinoTabBar {
        self.icon_size = icon_size;
        self
    }

    /// Dart `CupertinoTabBar(height:)`.
    pub fn height(mut self, height: f64) -> CupertinoTabBar {
        debug_assert!(height >= 0.0);
        self.height = height;
        self
    }

    /// Dart `CupertinoTabBar(border:)`.
    pub fn border(mut self, border: Option<Border>) -> CupertinoTabBar {
        self.border = border;
        self
    }

    /// Indicates whether the tab bar is fully opaque or can have contents behind it show through
    /// it.
    pub fn opaque(&self, app: &mut App, context: BuildContext) -> bool {
        let background_color = self
            .background_color
            .clone()
            .unwrap_or_else(|| CupertinoTheme::of(app, context).bar_background_color());
        alpha(&CupertinoDynamicColor::resolve(
            &background_color,
            app,
            context,
        )) == 0xFF
    }

    fn build_tab_items(&self, app: &mut App, context: BuildContext) -> Vec<WidgetRef> {
        let mut result = Vec::new();
        let localizations = <dyn CupertinoLocalizations>::of(app, context);

        for index in 0..self.items.len() {
            let active = index == self.current_index;
            let mut gesture_detector = GestureDetector::new().behavior(HitTestBehavior::Opaque);
            if let Some(on_tap) = self.on_tap.clone() {
                gesture_detector = gesture_detector.on_tap(Listener::new(move |app| {
                    on_tap(app, index);
                }));
            }
            result.push(
                self.wrap_active_item(
                    app,
                    context,
                    Expanded::new(
                        // Make tab items part of the EditableText tap region so that
                        // switching tabs doesn't unfocus text fields.
                        TextFieldTapRegion::new(
                            Semantics::new()
                                .selected(active)
                                .hint(
                                    localizations.tab_semantics_label(
                                        index as i32 + 1,
                                        self.items.len() as i32,
                                    ),
                                )
                                .child(
                                    MouseRegion::new()
                                        .cursor(if K_IS_WEB {
                                            SystemMouseCursors::CLICK.into()
                                        } else {
                                            <dyn MouseCursor>::defer()
                                        })
                                        .child(
                                            gesture_detector.child(
                                                Padding::new(EdgeInsets::only(0.0, 0.0, 0.0, 4.0))
                                                    .child(
                                                        Column::new()
                                                            .main_axis_alignment(
                                                                MainAxisAlignment::End,
                                                            )
                                                            .children(self.build_single_tab_item(
                                                                &self.items[index],
                                                                active,
                                                            )),
                                                    ),
                                            ),
                                        ),
                                ),
                        ),
                    )
                    .into_widget(),
                    active,
                ),
            );
        }

        result
    }

    fn build_single_tab_item(
        &self,
        item: &BottomNavigationBarItem,
        active: bool,
    ) -> Vec<WidgetRef> {
        let mut result = vec![
            Expanded::new(Center::new().child(if active {
                item.active_icon.clone()
            } else {
                item.icon.clone()
            }))
            .into_widget(),
        ];
        if let Some(label) = &item.label {
            let mut text = Text::new(label.clone());
            if let Some(semantics_label) = &item.semantics_label {
                text = text.semantics_label(semantics_label.clone());
            }
            result.push(text.into_widget());
        }
        result
    }

    /// Change the active tab item's icon and title colors to active.
    fn wrap_active_item(
        &self,
        app: &mut App,
        context: BuildContext,
        item: WidgetRef,
        active: bool,
    ) -> WidgetRef {
        if !active {
            return item;
        }

        let active_color = self
            .active_color
            .clone()
            .unwrap_or_else(|| CupertinoTheme::of(app, context).primary_color());
        let active_color = CupertinoDynamicColor::resolve(&active_color, app, context);
        IconTheme::merge(
            None,
            IconThemeData::new().color(active_color.clone()),
            DefaultTextStyle::merge(item).style(TextStyle::new().color(active_color)),
        )
    }

    /// Create a clone of the current [`CupertinoTabBar`] but with provided parameters
    /// overridden.
    pub fn copy_with(&self) -> CupertinoTabBar {
        self.clone()
    }
}

impl Debug for CupertinoTabBar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CupertinoTabBar")
            .field("items", &self.items)
            .field("current_index", &self.current_index)
            .field("background_color", &self.background_color)
            .field("active_color", &self.active_color)
            .field("inactive_color", &self.inactive_color)
            .field("icon_size", &self.icon_size)
            .field("height", &self.height)
            .field("border", &self.border)
            .finish_non_exhaustive()
    }
}

impl PreferredSizeWidget for CupertinoTabBar {
    fn preferred_size(&self) -> AnySize {
        Size::from_height(self.height).into()
    }
}

impl StatelessWidget for CupertinoTabBar {
    fn key(&self) -> Option<&KeyRef> {
        self.key.as_ref()
    }

    fn debug_check_constructor(&self) {
        debug_assert!(self.current_index < self.items.len());
    }

    fn as_preferred_size_widget(&self) -> Option<&dyn PreferredSizeWidget> {
        Some(self)
    }

    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        debug_assert!(debug_check_has_media_query(app, context));
        let bottom_padding = MediaQuery::view_padding_of(app, context).bottom;

        let background_color = self
            .background_color
            .clone()
            .unwrap_or_else(|| CupertinoTheme::of(app, context).bar_background_color());
        let background_color = CupertinoDynamicColor::resolve(&background_color, app, context);

        let is_default_border = self.border == Some(K_DEFAULT_TAB_BAR_BORDER);
        let mut resolve_border_side = |side: BorderSide| {
            if side == BorderSide::NONE {
                side
            } else {
                let color = if is_default_border {
                    K_DEFAULT_TAB_BAR_BORDER_COLOR.into_any()
                } else {
                    AnyColor::new(side.color)
                };
                side.copy_with(
                    Some(CupertinoDynamicColor::resolve(&color, app, context).color()),
                    None,
                    None,
                    None,
                )
            }
        };

        let resolved_border = self.border.map(|border| Border {
            top: resolve_border_side(border.top),
            left: resolve_border_side(border.left),
            bottom: resolve_border_side(border.bottom),
            right: resolve_border_side(border.right),
        });

        let inactive = CupertinoDynamicColor::resolve(&self.inactive_color, app, context);
        let tab_label_text_style = CupertinoTheme::of(app, context)
            .text_theme()
            .tab_label_text_style()
            .copy_with()
            .color(inactive.clone());
        let tab_items = self.build_tab_items(app, context);
        let mut decoration = BoxDecoration::new().color(background_color);
        if let Some(border) = resolved_border {
            decoration = decoration.border(border);
        }
        let mut result = DecoratedBox::new(decoration)
            .child(
                SizedBox::new()
                    .height(self.height + bottom_padding)
                    .child(IconTheme::merge(
                        None,
                        // Default with the inactive state.
                        IconThemeData::new().color(inactive).size(self.icon_size),
                        DefaultTextStyle::new(
                            // Default with the inactive state.
                            tab_label_text_style,
                            Padding::new(EdgeInsets::only(0.0, 0.0, 0.0, bottom_padding)).child(
                                Semantics::new().explicit_child_nodes(true).child(
                                    Row::new()
                                        // Align bottom since we want the labels to be aligned.
                                        .cross_axis_alignment(CrossAxisAlignment::End)
                                        .children(tab_items),
                                ),
                            ),
                        ),
                    )),
            )
            .into_widget();

        if !self.opaque(app, context) {
            // For non-opaque backgrounds, apply a blur effect.
            result = ClipRect::new()
                .child(BackdropFilter::filter(ImageFilter::blur(10.0, 10.0)).child(result))
                .into_widget();
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use waft_embedder::{Brightness, Locale, TargetPlatform, TextDirection};
    use waft_foundation::Ref;
    use waft_painting::AlignmentGeometry;
    use waft_rendering::{RenderBackdropFilter, RenderDecoratedBox};
    use waft_widgets::testing::{
        TargetPlatformVariant, TestTextField, TestWidgetsApp, WidgetTester, find, test_widgets,
        test_widgets_variant,
    };
    use waft_widgets::{
        Align, DefaultWidgetsLocalizations, Directionality, FocusNode, FocusNodeLeaf, GlobalKey,
        Icon, Localizations, MediaQueryData,
    };

    use super::*;
    use crate::icons::CupertinoIcons;
    use crate::localizations::DefaultCupertinoLocalizations;

    /// Dart's `pumpWidgetWithBoilerplate`: the localizations a tab reads its hint from, and a
    /// text direction.
    fn boilerplate(widget: WidgetRef) -> WidgetRef {
        Localizations::new(
            Locale::new("en"),
            vec![
                DefaultWidgetsLocalizations::delegate(),
                DefaultCupertinoLocalizations::delegate(),
            ],
        )
        .child(Directionality::new(TextDirection::Ltr, widget))
        .into_widget()
    }

    /// Pumps `bar` at the bottom of a view whose media is `data`, as a tab scaffold places it,
    /// which leaves its height its own.
    fn pump_bar(tester: &mut WidgetTester, data: MediaQueryData, bar: CupertinoTabBar) {
        let placed = MediaQuery::new(
            data,
            Align::new()
                .alignment(AlignmentGeometry::BOTTOM_CENTER)
                .child(bar),
        );
        tester.pump_widget(boilerplate(placed.into_widget()));
    }

    fn two_items() -> Vec<BottomNavigationBarItem> {
        vec![
            BottomNavigationBarItem::new(Icon::new(Some(CupertinoIcons::search()))).label("Tab 1"),
            BottomNavigationBarItem::new(Icon::new(Some(CupertinoIcons::settings())))
                .label("Tab 2"),
        ]
    }

    /// The decoration the bar paints its background and border with.
    fn decoration_of(tester: &WidgetTester) -> BoxDecoration {
        let decorated = tester.first::<RenderDecoratedBox>();
        let app = tester.cell.borrow();
        decorated
            .decoration(&app)
            .as_any()
            .downcast_ref::<BoxDecoration>()
            .expect("a box decoration")
            .clone()
    }

    fn top_border_color(tester: &WidgetTester) -> Color {
        let border = decoration_of(tester).border.expect("the bar's border");
        border
            .as_any()
            .downcast_ref::<Border>()
            .expect("a Border")
            .top
            .color
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "CupertinoTabBar requires at least one tab.")]
    fn requires_at_least_one_item() {
        let _ = CupertinoTabBar::new(Vec::new());
    }

    #[test]
    fn supports_one_item() {
        test_widgets(|tester| {
            let item = BottomNavigationBarItem::new(Icon::new(Some(CupertinoIcons::search())))
                .label("Tab 1");
            pump_bar(
                tester,
                MediaQueryData::new(),
                CupertinoTabBar::new(vec![item]),
            );
            assert_eq!(find::by_type::<CupertinoTabBar>().evaluate(tester).len(), 1);
        });
    }

    #[test]
    fn active_and_inactive_colors() {
        test_widgets(|tester| {
            pump_bar(
                tester,
                MediaQueryData::new(),
                CupertinoTabBar::new(two_items())
                    .current_index(1)
                    .active_color(Color::new(0xFF123456))
                    .inactive_color(Color::new(0xFF654321)),
            );
            assert_eq!(tester.text_color("Tab 1"), Color::new(0xFF654321));
            assert_eq!(tester.text_color("Tab 2"), Color::new(0xFF123456));
        });
    }

    #[test]
    fn active_and_inactive_colors_and_the_default_border_follow_dark_mode() {
        const DYNAMIC_ACTIVE_COLOR: CupertinoDynamicColor =
            CupertinoDynamicColor::with_brightness(Color::new(0xFF000000), Color::new(0xFF000001));
        const DYNAMIC_INACTIVE_COLOR: CupertinoDynamicColor =
            CupertinoDynamicColor::with_brightness(Color::new(0xFF000002), Color::new(0xFF000003));
        let bar = || {
            CupertinoTabBar::new(two_items())
                .current_index(1)
                .active_color(DYNAMIC_ACTIVE_COLOR.into_any())
                .inactive_color(DYNAMIC_INACTIVE_COLOR.into_any())
        };
        test_widgets(|tester| {
            pump_bar(tester, MediaQueryData::new(), bar());
            assert_eq!(tester.text_color("Tab 1"), Color::new(0xFF000002));
            assert_eq!(tester.text_color("Tab 2"), Color::new(0xFF000000));
            // Border color is resolved correctly.
            assert_eq!(top_border_color(tester), Color::new(0x4D000000));

            // Switch to dark mode.
            pump_bar(
                tester,
                MediaQueryData::new().platform_brightness(Brightness::Dark),
                bar(),
            );
            assert_eq!(tester.text_color("Tab 1"), Color::new(0xFF000003));
            assert_eq!(tester.text_color("Tab 2"), Color::new(0xFF000001));
            // Border color is resolved correctly.
            assert_eq!(top_border_color(tester), Color::new(0x29000000));
        });
    }

    #[test]
    fn tabs_default_to_the_themes_colors() {
        test_widgets(|tester| {
            pump_bar(
                tester,
                MediaQueryData::new(),
                CupertinoTabBar::new(two_items()).current_index(1),
            );
            assert_eq!(tester.text_color("Tab 1"), Color::new(0xFF999999));
            assert_eq!(
                tester.text_color("Tab 2"),
                CupertinoColors::ACTIVE_BLUE.color()
            );
        });
    }

    #[test]
    fn use_active_icon() {
        test_widgets(|tester| {
            let (icon, active_icon) = (GlobalKey::new(), GlobalKey::new());
            let item = |label: &str| {
                BottomNavigationBarItem::new(SizedBox::new().key(icon.clone().into()))
                    .active_icon(SizedBox::new().key(active_icon.clone().into()))
                    .label(label)
            };
            let other =
                BottomNavigationBarItem::new(Icon::new(Some(CupertinoIcons::search()))).label("B");
            pump_bar(
                tester,
                MediaQueryData::new(),
                CupertinoTabBar::new(vec![item("A"), other.clone()]),
            );
            assert_eq!(find::by_global_key(&active_icon).evaluate(tester).len(), 1);
            assert!(find::by_global_key(&icon).evaluate(tester).is_empty());

            pump_bar(
                tester,
                MediaQueryData::new(),
                CupertinoTabBar::new(vec![item("A"), other]).current_index(1),
            );
            assert_eq!(find::by_global_key(&icon).evaluate(tester).len(), 1);
            assert!(
                find::by_global_key(&active_icon)
                    .evaluate(tester)
                    .is_empty()
            );
        });
    }

    #[test]
    fn adjusts_height_to_account_for_bottom_padding() {
        test_widgets(|tester| {
            let bar = find::by_type::<CupertinoTabBar>();
            // Verify height with no bottom padding.
            pump_bar(
                tester,
                MediaQueryData::new(),
                CupertinoTabBar::new(two_items()),
            );
            assert_eq!(tester.get_size(&bar).height(), 50.0);

            // Verify height with bottom padding.
            pump_bar(
                tester,
                MediaQueryData::new().view_padding(EdgeInsets::only(0.0, 0.0, 0.0, 40.0)),
                CupertinoTabBar::new(two_items()),
            );
            assert_eq!(tester.get_size(&bar).height(), 90.0);
        });
    }

    #[test]
    fn set_custom_height() {
        test_widgets(|tester| {
            const TAB_BAR_HEIGHT: f64 = 56.0;
            let bar = find::by_type::<CupertinoTabBar>();
            let tab_bar = CupertinoTabBar::new(two_items()).height(TAB_BAR_HEIGHT);
            assert_eq!(tab_bar.preferred_size().height(), TAB_BAR_HEIGHT);

            pump_bar(tester, MediaQueryData::new(), tab_bar.copy_with());
            assert_eq!(tester.get_size(&bar).height(), TAB_BAR_HEIGHT);

            const BOTTOM_PADDING: f64 = 40.0;
            pump_bar(
                tester,
                MediaQueryData::new().view_padding(EdgeInsets::only(0.0, 0.0, 0.0, BOTTOM_PADDING)),
                tab_bar,
            );
            assert_eq!(
                tester.get_size(&bar).height(),
                TAB_BAR_HEIGHT + BOTTOM_PADDING
            );
        });
    }

    #[test]
    fn opaque_background_does_not_add_blur_effects() {
        test_widgets(|tester| {
            pump_bar(
                tester,
                MediaQueryData::new(),
                CupertinoTabBar::new(two_items()),
            );
            assert_eq!(tester.all::<RenderBackdropFilter>().len(), 1);

            pump_bar(
                tester,
                MediaQueryData::new(),
                // Opaque white.
                CupertinoTabBar::new(two_items()).background_color(Color::new(0xFFFFFFFF)),
            );
            assert!(tester.all::<RenderBackdropFilter>().is_empty());
        });
    }

    #[test]
    fn tap_callback() {
        test_widgets(|tester| {
            let callback_tab = std::rc::Rc::new(Cell::new(None));
            let tapped = std::rc::Rc::clone(&callback_tab);
            pump_bar(
                tester,
                MediaQueryData::new(),
                CupertinoTabBar::new(two_items())
                    .current_index(1)
                    .on_tap(move |_, tab| tapped.set(Some(tab))),
            );

            tester.tap(find::text("Tab 1"));
            assert_eq!(callback_tab.get(), Some(0));

            tester.tap(find::text("Tab 2"));
            assert_eq!(callback_tab.get(), Some(1));
        });
    }

    #[test]
    fn hide_border_hides_the_top_border_of_the_tab_bar() {
        test_widgets(|tester| {
            pump_bar(
                tester,
                MediaQueryData::new(),
                CupertinoTabBar::new(two_items()),
            );
            assert!(decoration_of(tester).border.is_some());

            pump_bar(
                tester,
                MediaQueryData::new(),
                CupertinoTabBar::new(two_items())
                    // Opaque white.
                    .background_color(Color::new(0xFFFFFFFF))
                    .border(None),
            );
            assert!(decoration_of(tester).border.is_none());
        });
    }

    #[test]
    fn copy_with_overrides_what_it_is_given_and_keeps_the_rest() {
        let bar = CupertinoTabBar::new(two_items())
            .current_index(1)
            .icon_size(20.0)
            .active_color(Color::new(0xFF123456));
        let copy = bar.copy_with().current_index(0).height(60.0).border(None);
        assert_eq!(copy.current_index, 0);
        assert_eq!(copy.height, 60.0);
        assert!(copy.border.is_none());
        assert_eq!(copy.items.len(), 2);
        assert_eq!(copy.icon_size, 20.0);
        assert_eq!(
            copy.active_color.map(|color| color.color()),
            Some(Color::new(0xFF123456))
        );
        assert_eq!(bar.current_index, 1, "the bar copied is as it was");
    }

    fn has_focus(tester: &WidgetTester, node: &Ref<FocusNode>) -> bool {
        tester.update(|app| node.handle().has_focus(app))
    }

    /// A tab is part of a text field's tap region, so a tap on one is no tap outside the
    /// field, which on a desktop takes the field's focus.
    #[test]
    fn tapping_a_tab_keeps_a_text_field_focused() {
        test_widgets_variant(
            TargetPlatformVariant::only(TargetPlatform::MacOS),
            |tester| {
                let focus_node: Ref<FocusNode> = tester.update(FocusNode::new);
                let outside = GlobalKey::new();
                let page = Column::new().children([
                    TestTextField::new()
                        .focus_node(focus_node.erase())
                        .autofocus(true)
                        .into_widget(),
                    Expanded::new(SizedBox::expand().key(outside.clone().into())).into_widget(),
                    CupertinoTabBar::new(two_items()).into_widget(),
                ]);
                tester.pump_widget(TestWidgetsApp::new().home(boilerplate(page.into_widget())));
                tester.pump();
                assert!(has_focus(tester, &focus_node));

                tester.tap(find::text("Tab 2"));
                tester.pump();
                assert!(
                    has_focus(tester, &focus_node),
                    "a tap on a tab leaves the field its focus"
                );

                tester.tap(&outside);
                tester.pump();
                assert!(
                    !has_focus(tester, &focus_node),
                    "a tap anywhere else takes it"
                );
            },
        );
    }
}
