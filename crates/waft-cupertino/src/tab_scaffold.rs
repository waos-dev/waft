//! Flutter counterpart: `cupertino/tab_scaffold.dart`.

use std::fmt::{self, Debug};
use std::rc::Rc;

use waft_foundation::{
    App, ChangeNotifier, ChangeNotifierData, Handle, Listenable, Listener, OptionRetained, Ref,
};
use waft_painting::{AlignmentGeometry, AnyColor, BoxDecoration, EdgeInsets};
use waft_rendering::StackFit;
use waft_services::{RestorationBucket, RestorationData};
use waft_widgets::{
    Align, BuildContext, Builder, DecoratedBox, FocusNodeLeaf, FocusScope, FocusScopeNode,
    HeroMode, IndexedWidgetBuilder, IntoWidget, KeyRef, MediaQuery, Offstage, Padding,
    PreferredSizeWidget, RestorableChangeNotifier, RestorableListenable, RestorableListenableData,
    RestorableProperty, RestorablePropertyData, RestorationMixin, RestorationMixinData, SizedBox,
    Stack, State, StateData, StatefulWidget, TickerMode, WidgetRef,
};

use crate::bottom_tab_bar::CupertinoTabBar;
use crate::colors::CupertinoDynamicColor;
use crate::theme::CupertinoTheme;

/// Coordinates tab selection between a [`CupertinoTabBar`] and a [`CupertinoTabScaffold`].
///
/// The [`index`](Self::index) property is the index of the selected tab. Changing its value
/// updates the actively displayed tab of the [`CupertinoTabScaffold`] the
/// [`CupertinoTabController`] controls, as well as the currently selected tab item of its
/// [`CupertinoTabBar`].
///
/// See also:
///
///  * [`CupertinoTabScaffold`], a tabbed application root layout that can be controlled by a
///    [`CupertinoTabController`].
///  * [`RestorableCupertinoTabController`], which is a restorable version of this controller.
#[derive(Debug)]
pub struct CupertinoTabController {
    change_notifier: ChangeNotifierData,
    is_disposed: bool,
    index: usize,
}

impl CupertinoTabController {
    /// Creates a [`CupertinoTabController`] to control the tab index of
    /// [`CupertinoTabScaffold`] and [`CupertinoTabBar`].
    ///
    /// The initial index is 0; see [`initial_index`](Self::initial_index).
    pub fn new(app: &mut App) -> Ref<CupertinoTabController> {
        Self::initial_index(app, 0)
    }

    /// Dart `CupertinoTabController(initialIndex:)`.
    ///
    /// The value must be less than the total number of tabs.
    pub fn initial_index(app: &mut App, initial_index: usize) -> Ref<CupertinoTabController> {
        app.create(CupertinoTabController {
            change_notifier: ChangeNotifierData::new(),
            is_disposed: false,
            index: initial_index,
        })
    }

    /// The index of the currently selected tab.
    ///
    /// Changing the value of `index` updates the actively displayed tab of the
    /// [`CupertinoTabScaffold`] controlled by this [`CupertinoTabController`], as well as the
    /// currently selected tab item of its [`CupertinoTabScaffold::tab_bar`].
    ///
    /// The value must be less than the total number of tabs.
    pub fn index(self: Handle<Self>, app: &App) -> usize {
        app.get(self).index
    }

    /// Dart's `index` setter.
    pub fn set_index(self: Handle<Self>, app: &mut App, value: usize) {
        if app.get(self).index == value {
            return;
        }
        app.get_mut(self).index = value;
        self.notify_listeners(app);
    }

    /// Discards any resources used by the object; see [`ChangeNotifierData::dispose`].
    pub fn dispose(self: Handle<Self>, app: &mut App) {
        ChangeNotifierData::dispose(app, self);
        app.get_mut(self).is_disposed = true;
    }
}

impl ChangeNotifier for CupertinoTabController {
    fn change_notifier_data(&self) -> &ChangeNotifierData {
        &self.change_notifier
    }

    fn change_notifier_data_mut(&mut self) -> &mut ChangeNotifierData {
        &mut self.change_notifier
    }
}

/// Implements a tabbed iOS application's root layout and behavior structure.
///
/// The scaffold lays out the tab bar at the bottom and the content between or behind the tab
/// bar.
///
/// A [`tab_bar`](Self::tab_bar) and a [`tab_builder`](Self::tab_builder) are required. The
/// [`CupertinoTabScaffold`] will automatically listen to the provided [`CupertinoTabBar`]'s tap
/// callbacks to change the active tab.
///
/// A [`controller`](Self::controller) can be used to provide an initially selected tab index and
/// manage subsequent tab changes. If a controller is not specified, the scaffold will create its
/// own [`CupertinoTabController`] and manage it internally. Otherwise it's up to the owner of
/// `controller` to call `dispose` on it after finish using it.
///
/// Tabs' contents are built with the provided `tab_builder` at the active tab index. The
/// `tab_builder` must be able to build the same number of pages as there are `tab_bar` items.
/// Inactive tabs will be moved [`Offstage`] and their animations disabled.
///
/// Adding/removing tabs, or changing the order of tabs is supported but not recommended. Doing
/// so is against the iOS human interface guidelines, and [`CupertinoTabScaffold`] may lose some
/// tabs' state in the process.
///
/// Use [`CupertinoTabView`](crate::CupertinoTabView) as the root widget of each tab to support
/// tabs with parallel navigation state and history. Since each `CupertinoTabView` contains a
/// `Navigator`, rebuilding the `CupertinoTabView` with a different `WidgetBuilder` instance in
/// `CupertinoTabView::builder` will not recreate the `CupertinoTabView`'s navigation stack or
/// update its UI. To update the contents of the `CupertinoTabView` after it's built, trigger a
/// rebuild (via `State::set_state`, for instance) from its descendant rather than from its
/// ancestor.
///
/// To push a route above all tabs instead of inside the currently selected one (such as when
/// showing a dialog on top of this scaffold), use the root navigator from inside the
/// [`BuildContext`] of a `CupertinoTabView`.
///
/// See also:
///
///  * [`CupertinoTabBar`], the bottom tab bar inserted in the scaffold.
///  * [`CupertinoTabController`], the selection state of this widget.
///  * [`CupertinoTabView`](crate::CupertinoTabView), the typical root content of each tab that
///    holds its own `Navigator` stack.
///  * [`CupertinoPageRoute`](crate::CupertinoPageRoute), a route hosting modal pages with iOS
///    style transitions.
///  * [`CupertinoPageScaffold`](crate::CupertinoPageScaffold), typical contents of an iOS modal
///    page implementing layout with a navigation bar on top.
///  * [iOS human interface guidelines](https://developer.apple.com/design/human-interface-guidelines/ios/bars/tab-bars/).
pub struct CupertinoTabScaffold {
    pub key: Option<KeyRef>,

    /// The `tab_bar` is a [`CupertinoTabBar`] drawn at the bottom of the screen that lets the
    /// user switch between different tabs in the main content area when present.
    ///
    /// The [`CupertinoTabBar::current_index`] is only used to initialize a
    /// [`CupertinoTabController`] when no [`controller`](Self::controller) is provided.
    /// Subsequently providing a different `CupertinoTabBar::current_index` does not affect the
    /// scaffold or the tab bar's active tab index. To programmatically change the active tab
    /// index, use a [`CupertinoTabController`].
    ///
    /// If [`CupertinoTabBar::on_tap`] is provided, it will still be called.
    /// [`CupertinoTabScaffold`] automatically also listen to the [`CupertinoTabBar`]'s `on_tap`
    /// to change the `controller`'s `index` and change the actively displayed tab in
    /// [`CupertinoTabScaffold`]'s own main content area.
    ///
    /// If translucent, the main content may slide behind it. Otherwise, the main content's
    /// bottom margin will be offset by its height.
    ///
    /// By default `tab_bar` disables text scaling to match the native iOS behavior. To override
    /// this behavior, wrap each of the `tab_bar`'s items inside a [`MediaQuery`] with the
    /// desired `TextScaler`.
    pub tab_bar: CupertinoTabBar,

    /// Controls the currently selected tab index of the [`tab_bar`](Self::tab_bar), as well as
    /// the active tab index of the [`tab_builder`](Self::tab_builder). Providing a different
    /// `controller` will also update the scaffold's current active index to the new
    /// controller's index value.
    ///
    /// Defaults to null.
    pub controller: Option<Ref<CupertinoTabController>>,

    /// An [`IndexedWidgetBuilder`] that's called when tabs become active.
    ///
    /// The widgets built by [`IndexedWidgetBuilder`] are typically a
    /// [`CupertinoTabView`](crate::CupertinoTabView) in order to achieve the parallel
    /// hierarchical information architecture seen on iOS apps with tab bars.
    ///
    /// When the tab becomes inactive, its content is cached in the widget tree [`Offstage`] and
    /// its animations disabled.
    ///
    /// Content can slide under the [`tab_bar`](Self::tab_bar) when they're translucent. In that
    /// case, the child's [`BuildContext`]'s [`MediaQuery`] will have a bottom padding indicating
    /// the area of obstructing overlap from the `tab_bar`.
    pub tab_builder: IndexedWidgetBuilder,

    /// The color of the widget that underlies the entire scaffold.
    ///
    /// By default uses [`CupertinoTheme`]'s `scaffold_background_color` when null.
    pub background_color: Option<AnyColor>,

    /// Whether the body should size itself to avoid the window's bottom inset.
    ///
    /// For example, if there is an onscreen keyboard displayed above the scaffold, the body can
    /// be resized to avoid overlapping the keyboard, which prevents widgets inside the body from
    /// being obscured by the keyboard.
    ///
    /// Defaults to true.
    pub resize_to_avoid_bottom_inset: bool,

    /// Restoration ID to save and restore the state of the [`CupertinoTabScaffold`].
    ///
    /// This property only has an effect when no [`controller`](Self::controller) has been
    /// provided: If it is non-null (and no `controller` has been provided), the scaffold will
    /// persist and restore the currently selected tab index. If a `controller` has been
    /// provided, it is the responsibility of the owner of that controller to persist and
    /// restore it, e.g. by using a [`RestorableCupertinoTabController`].
    ///
    /// The state of this widget is persisted in a [`RestorationBucket`] claimed from the
    /// surrounding `RestorationScope` using the provided restoration ID.
    ///
    /// See also:
    ///
    ///  * `RestorationManager`, which explains how state restoration works in Flutter.
    pub restoration_id: Option<String>,
}

impl CupertinoTabScaffold {
    /// Creates a layout for applications with a tab bar at the bottom.
    pub fn new(
        tab_bar: CupertinoTabBar,
        tab_builder: impl Fn(&mut App, BuildContext, i32) -> WidgetRef + 'static,
    ) -> CupertinoTabScaffold {
        CupertinoTabScaffold {
            key: None,
            tab_bar,
            tab_builder: Rc::new(tab_builder),
            controller: None,
            background_color: None,
            resize_to_avoid_bottom_inset: true,
            restoration_id: None,
        }
    }

    /// Dart `CupertinoTabScaffold(key:)`.
    pub fn key(mut self, key: KeyRef) -> CupertinoTabScaffold {
        self.key = Some(key);
        self
    }

    /// Dart `CupertinoTabScaffold(controller:)`.
    pub fn controller(mut self, controller: Ref<CupertinoTabController>) -> CupertinoTabScaffold {
        self.controller = Some(controller);
        self
    }

    /// Dart `CupertinoTabScaffold(backgroundColor:)`.
    pub fn background_color(
        mut self,
        background_color: impl Into<AnyColor>,
    ) -> CupertinoTabScaffold {
        self.background_color = Some(background_color.into());
        self
    }

    /// Dart `CupertinoTabScaffold(resizeToAvoidBottomInset:)`.
    pub fn resize_to_avoid_bottom_inset(
        mut self,
        resize_to_avoid_bottom_inset: bool,
    ) -> CupertinoTabScaffold {
        self.resize_to_avoid_bottom_inset = resize_to_avoid_bottom_inset;
        self
    }

    /// Dart `CupertinoTabScaffold(restorationId:)`.
    pub fn restoration_id(mut self, restoration_id: impl Into<String>) -> CupertinoTabScaffold {
        self.restoration_id = Some(restoration_id.into());
        self
    }
}

impl Debug for CupertinoTabScaffold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CupertinoTabScaffold")
            .field("tab_bar", &self.tab_bar)
            .field("controller", &self.controller)
            .field("background_color", &self.background_color)
            .field(
                "resize_to_avoid_bottom_inset",
                &self.resize_to_avoid_bottom_inset,
            )
            .field("restoration_id", &self.restoration_id)
            .finish_non_exhaustive()
    }
}

impl StatefulWidget for CupertinoTabScaffold {
    type State = CupertinoTabScaffoldState;

    fn key(&self) -> Option<&KeyRef> {
        self.key.as_ref()
    }

    fn create_state(&self) -> CupertinoTabScaffoldState {
        CupertinoTabScaffoldState {
            state: StateData::new(),
            restoration: RestorationMixinData::new(),
            internal_controller: None,
        }
    }
}

/// Dart's `_CupertinoTabScaffoldState`.
pub struct CupertinoTabScaffoldState {
    state: StateData<CupertinoTabScaffold>,
    restoration: RestorationMixinData,
    internal_controller: Option<Ref<RestorableCupertinoTabController>>,
}

impl CupertinoTabScaffoldState {
    fn controller(self: Handle<Self>, app: &App) -> Ref<CupertinoTabController> {
        self.widget(app).controller.clone().unwrap_or_else(|| {
            let internal_controller = app
                .get(self)
                .internal_controller
                .handle()
                .expect("a scaffold with no controller of its widget's has one of its own");
            RestorableListenable::value(internal_controller, app)
        })
    }

    /// Dart's constructor assert, which reads the controller: it runs where the state first
    /// has the widget, and each time it is given another.
    fn debug_check_controller_index(self: Handle<Self>, app: &App) {
        let widget = self.widget(app);
        if let Some(controller) = &widget.controller {
            let index = controller.handle().index(app);
            debug_assert!(
                index < widget.tab_bar.items.len(),
                "The CupertinoTabController's current index {index} is out of bounds for the \
                 tab bar with {} tabs",
                widget.tab_bar.items.len()
            );
        }
    }

    fn restore_internal_controller(self: Handle<Self>, app: &mut App) {
        if let Some(internal_controller) = app.get(self).internal_controller.handle() {
            self.register_for_restoration(app, internal_controller.as_property(), "controller");
            RestorableListenable::value(internal_controller, app).add_listener(
                app,
                Listener::handle_method(self, Self::on_current_index_change),
            );
        }
    }

    fn update_tab_controller(
        self: Handle<Self>,
        app: &mut App,
        old_widget_controller: Option<&Ref<CupertinoTabController>>,
    ) {
        if self.widget(app).controller.is_none() && app.get(self).internal_controller.is_none() {
            // No widget-provided controller: create an internal controller.
            let initial_index = self.widget(app).tab_bar.current_index;
            let internal_controller =
                RestorableCupertinoTabController::initial_index(app, initial_index);
            app.get_mut(self).internal_controller = Some(internal_controller);
            if !self.restore_pending(app) {
                // Also adds the listener to the controller.
                self.restore_internal_controller(app);
            }
        }
        if self.widget(app).controller.is_some()
            && let Some(internal_controller) = app.get(self).internal_controller.handle()
        {
            // Use the widget-provided controller.
            self.unregister_from_restoration(app, internal_controller.as_property());
            RestorableProperty::dispose(internal_controller, app);
            app.get_mut(self).internal_controller = None;
        }
        let widget_controller = self.widget(app).controller.clone();
        if old_widget_controller.map(Ref::handle) != widget_controller.as_ref().map(Ref::handle) {
            // The widget-provided controller has changed: move listeners.
            if let Some(old_widget_controller) = old_widget_controller
                && !app.get(old_widget_controller).is_disposed
            {
                old_widget_controller.remove_listener(
                    app,
                    &Listener::handle_method(self, Self::on_current_index_change),
                );
            }
            if let Some(widget_controller) = widget_controller {
                widget_controller.add_listener(
                    app,
                    Listener::handle_method(self, Self::on_current_index_change),
                );
            }
        }
    }

    fn on_current_index_change(self: Handle<Self>, app: &mut App) {
        let index = self.controller(app).handle().index(app);
        debug_assert!(
            index < self.widget(app).tab_bar.items.len(),
            "The CupertinoTabScaffoldState's current index {index} is out of bounds for the tab \
             bar with {} tabs",
            self.widget(app).tab_bar.items.len()
        );

        // The value of `controller.index` has already been updated at this point.
        // Calling `set_state` to rebuild using `controller.index`.
        self.set_state(app, |_| {});
    }
}

impl RestorationMixin for CupertinoTabScaffoldState {
    waft_widgets::restoration_mixin_accessors!();

    fn restoration_id(self: Handle<Self>, app: &App) -> Option<&str> {
        self.widget(app).restoration_id.as_deref()
    }

    fn restore_state(
        self: Handle<Self>,
        app: &mut App,
        _old_bucket: Option<Handle<RestorationBucket>>,
        _initial_restore: bool,
    ) {
        self.restore_internal_controller(app);
    }
}

impl State for CupertinoTabScaffoldState {
    type Widget = CupertinoTabScaffold;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        self.debug_check_controller_index(app);
        self.update_tab_controller(app, None);
    }

    fn did_change_dependencies(self: Handle<Self>, app: &mut App) {
        self.did_change_dependencies_restoration(app);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, old_widget: &CupertinoTabScaffold) {
        self.debug_check_controller_index(app);
        self.did_update_restoration_id(app);
        let widget_controller = self.widget(app).controller.as_ref().map(Ref::handle);
        if widget_controller != old_widget.controller.as_ref().map(Ref::handle) {
            self.update_tab_controller(app, old_widget.controller.as_ref());
        } else {
            let controller = self.controller(app);
            let tab_count = self.widget(app).tab_bar.items.len();
            if controller.handle().index(app) >= tab_count {
                // If a new `tab_bar` with less than (controller.index + 1) items is provided,
                // clamp the current index.
                controller.handle().set_index(app, tab_count - 1);
            }
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let existing_media_query = MediaQuery::of(app, context);
        let mut new_media_query = MediaQuery::of(app, context);

        let current_index = self.controller(app).handle().index(app);
        let widget = self.widget(app);
        let tab_bar = widget.tab_bar.clone();
        let resize_to_avoid_bottom_inset = widget.resize_to_avoid_bottom_inset;
        let background_color = widget.background_color.clone();
        let content = TabSwitchingView::new(
            current_index,
            tab_bar.items.len(),
            widget.tab_builder.clone(),
        );
        let mut content_padding = EdgeInsets::ZERO;

        if resize_to_avoid_bottom_inset {
            // Remove the view inset and add it back as a padding in the inner content.
            new_media_query = new_media_query.remove_view_insets(false, false, false, true);
            content_padding =
                EdgeInsets::only(0.0, 0.0, 0.0, existing_media_query.view_insets.bottom);
        }

        // Only pad the content with the height of the tab bar if the tab
        // isn't already entirely obstructed by a keyboard or other view insets.
        // Don't double pad.
        if !resize_to_avoid_bottom_inset
            || tab_bar.preferred_size().height() > existing_media_query.view_insets.bottom
        {
            // TODO(xster): Use real size after partial layout instead of preferred size.
            // https://github.com/flutter/flutter/issues/12912
            let bottom_padding =
                tab_bar.preferred_size().height() + existing_media_query.padding.bottom;

            // If tab bar opaque, directly stop the main content higher. If
            // translucent, let main content draw behind the tab bar but hint the
            // obstructed area.
            if tab_bar.opaque(app, context) {
                content_padding = EdgeInsets::only(0.0, 0.0, 0.0, bottom_padding);
                new_media_query = new_media_query.remove_padding(false, false, false, true);
            } else {
                let padding =
                    new_media_query
                        .padding
                        .copy_with(None, None, None, Some(bottom_padding));
                new_media_query = new_media_query.copy_with().padding(padding);
            }
        }

        let content = MediaQuery::new(
            new_media_query,
            Padding::new(content_padding).child(content),
        );

        let color = CupertinoDynamicColor::maybe_resolve(background_color.as_ref(), app, context)
            .unwrap_or_else(|| CupertinoTheme::of(app, context).scaffold_background_color());
        DecoratedBox::new(BoxDecoration::new().color(color))
            .child(
                Stack::new().children([
                    // The main content being at the bottom is added to the stack first.
                    content.into_widget(),
                    MediaQuery::with_no_text_scaling(
                        None,
                        Align::new()
                            .alignment(AlignmentGeometry::BOTTOM_CENTER)
                            // Override the tab bar's current_index to the current tab and hook in
                            // our own listener to update the controller's index on top of a
                            // possibly user provided callback.
                            .child(tab_bar.copy_with().current_index(current_index).on_tap(
                                move |app, new_index| {
                                    self.controller(app).handle().set_index(app, new_index);
                                    // Chain the user's original callback.
                                    if let Some(on_tap) = self.widget(app).tab_bar.on_tap.clone() {
                                        on_tap(app, new_index);
                                    }
                                },
                            )),
                    ),
                ]),
            )
            .into_widget()
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(widget_controller) = self.widget(app).controller.clone()
            && !app.get(&widget_controller).is_disposed
        {
            widget_controller.remove_listener(
                app,
                &Listener::handle_method(self, Self::on_current_index_change),
            );
        }
        if let Some(internal_controller) = app.get(self).internal_controller.handle() {
            RestorableProperty::dispose(internal_controller, app);
        }
        self.dispose_restoration(app);
    }
}

/// A widget laying out multiple tabs with only one active tab being built at a time and on
/// stage. Off stage tabs' animations are stopped.
struct TabSwitchingView {
    current_tab_index: usize,
    tab_count: usize,
    tab_builder: IndexedWidgetBuilder,
}

impl TabSwitchingView {
    fn new(
        current_tab_index: usize,
        tab_count: usize,
        tab_builder: IndexedWidgetBuilder,
    ) -> TabSwitchingView {
        debug_assert!(tab_count > 0);
        TabSwitchingView {
            current_tab_index,
            tab_count,
            tab_builder,
        }
    }
}

impl Debug for TabSwitchingView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TabSwitchingView")
            .field("current_tab_index", &self.current_tab_index)
            .field("tab_count", &self.tab_count)
            .finish_non_exhaustive()
    }
}

impl StatefulWidget for TabSwitchingView {
    type State = TabSwitchingViewState;

    fn create_state(&self) -> TabSwitchingViewState {
        TabSwitchingViewState {
            state: StateData::new(),
            should_build_tab: Vec::new(),
            tab_focus_nodes: Vec::new(),
            discarded_nodes: Vec::new(),
        }
    }
}

struct TabSwitchingViewState {
    state: StateData<TabSwitchingView>,
    should_build_tab: Vec<bool>,
    tab_focus_nodes: Vec<Ref<FocusScopeNode>>,

    // When focus nodes are no longer needed, we need to dispose of them, but we
    // can't be sure that nothing else is listening to them until this widget is
    // disposed of, so when they are no longer needed, we move them to this list,
    // and dispose of them when we dispose of this widget.
    discarded_nodes: Vec<Ref<FocusScopeNode>>,
}

impl TabSwitchingViewState {
    // Will focus the active tab if the FocusScope above it has focus already. If
    // not, then it will just mark it as the preferred focus for that scope.
    fn focus_active_tab(self: Handle<Self>, app: &mut App) {
        let tab_count = self.widget(app).tab_count;
        let node_count = app.get(self).tab_focus_nodes.len();
        if node_count != tab_count {
            if node_count > tab_count {
                let state = app.get_mut(self);
                let discarded = state.tab_focus_nodes.split_off(tab_count);
                state.discarded_nodes.extend(discarded);
            } else {
                for index in 0..tab_count - node_count {
                    let node = FocusScopeNode::new(app);
                    node.handle().set_debug_label(
                        app,
                        Some(format!("CupertinoTabScaffold Tab {}", index + node_count)),
                    );
                    app.get_mut(self).tab_focus_nodes.push(node);
                }
            }
        }
        let current_tab_index = self.widget(app).current_tab_index;
        let active = app.get(self).tab_focus_nodes[current_tab_index].handle();
        let context = self.context(app);
        FocusScope::of(app, context, true).set_first_focus(app, active);
    }
}

impl State for TabSwitchingViewState {
    type Widget = TabSwitchingView;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let tab_count = self.widget(app).tab_count;
        app.get_mut(self).should_build_tab = vec![false; tab_count];
    }

    fn did_change_dependencies(self: Handle<Self>, app: &mut App) {
        self.focus_active_tab(app);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, _old_widget: &TabSwitchingView) {
        // Only partially invalidate the tabs cache to avoid breaking the current
        // behavior. We assume that the only possible change is either:
        // - new tabs are appended to the tab list, or
        // - some trailing tabs are removed.
        // If the above assumption is not true, some tabs may lose their state.
        let tab_count = self.widget(app).tab_count;
        app.get_mut(self).should_build_tab.resize(tab_count, false);
        self.focus_active_tab(app);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        let state = app.get_mut(self);
        let tab_focus_nodes = std::mem::take(&mut state.tab_focus_nodes);
        let discarded_nodes = std::mem::take(&mut state.discarded_nodes);
        for focus_scope_node in tab_focus_nodes {
            focus_scope_node.handle().dispose(app);
        }
        for focus_scope_node in discarded_nodes {
            focus_scope_node.handle().dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let tab_count = self.widget(app).tab_count;
        let current_tab_index = self.widget(app).current_tab_index;
        let children: Vec<WidgetRef> = (0..tab_count)
            .map(|index| {
                let active = index == current_tab_index;
                let state = app.get_mut(self);
                state.should_build_tab[index] = active || state.should_build_tab[index];
                let node = state.tab_focus_nodes[index].clone();

                HeroMode::new(
                    Offstage::new().offstage(!active).child(TickerMode::new(
                        active,
                        FocusScope::new(Builder::new(move |app, context| {
                            if app.get(self).should_build_tab[index] {
                                let tab_builder = self.widget(app).tab_builder.clone();
                                tab_builder(app, context, index as i32)
                            } else {
                                SizedBox::shrink().into_widget()
                            }
                        }))
                        .node(node),
                    )),
                )
                .enabled(active)
                .into_widget()
            })
            .collect();
        Stack::new()
            .fit(StackFit::Expand)
            .children(children)
            .into_widget()
    }
}

/// A [`RestorableProperty`] that knows how to store and restore a [`CupertinoTabController`].
///
/// The [`CupertinoTabController`] is accessible via the [`value`](RestorableListenable::value)
/// getter. During state restoration, the property will restore
/// [`CupertinoTabController::index`] to the value it had when the restoration data it is getting
/// restored from was collected.
pub struct RestorableCupertinoTabController {
    change_notifier: ChangeNotifierData,
    property: RestorablePropertyData,
    listenable: RestorableListenableData<Ref<CupertinoTabController>>,
    initial_index: usize,
}

impl RestorableCupertinoTabController {
    /// Creates a [`RestorableCupertinoTabController`] to control the tab index of
    /// [`CupertinoTabScaffold`] and [`CupertinoTabBar`].
    ///
    /// The initial index is 0; see [`initial_index`](Self::initial_index).
    pub fn new(app: &mut App) -> Ref<RestorableCupertinoTabController> {
        Self::initial_index(app, 0)
    }

    /// Dart `RestorableCupertinoTabController(initialIndex:)`.
    ///
    /// The value must be less than the total number of tabs.
    pub fn initial_index(
        app: &mut App,
        initial_index: usize,
    ) -> Ref<RestorableCupertinoTabController> {
        app.create(RestorableCupertinoTabController {
            change_notifier: ChangeNotifierData::new(),
            property: RestorablePropertyData::new(),
            listenable: RestorableListenableData::new(),
            initial_index,
        })
    }
}

impl ChangeNotifier for RestorableCupertinoTabController {
    fn change_notifier_data(&self) -> &ChangeNotifierData {
        &self.change_notifier
    }

    fn change_notifier_data_mut(&mut self) -> &mut ChangeNotifierData {
        &mut self.change_notifier
    }
}

impl RestorableProperty for RestorableCupertinoTabController {
    type Value = Ref<CupertinoTabController>;
    waft_widgets::restorable_property_accessors!();

    fn create_default_value(self: Handle<Self>, app: &mut App) -> Ref<CupertinoTabController> {
        let initial_index = app.get(self).initial_index;
        CupertinoTabController::initial_index(app, initial_index)
    }

    fn from_primitives(
        self: Handle<Self>,
        app: &mut App,
        data: &RestorationData,
    ) -> Ref<CupertinoTabController> {
        let index = data
            .as_int()
            .expect("RestorableCupertinoTabController stores an int");
        CupertinoTabController::initial_index(app, index as usize)
    }

    fn init_with_value(self: Handle<Self>, app: &mut App, value: Ref<CupertinoTabController>) {
        RestorableChangeNotifier::init_with_value(self, app, value);
    }

    fn to_primitives(self: Handle<Self>, app: &App) -> RestorationData {
        RestorationData::Int(RestorableListenable::value(self, app).handle().index(app) as i64)
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        RestorableChangeNotifier::dispose(self, app);
    }
}

impl RestorableListenable for RestorableCupertinoTabController {
    waft_widgets::restorable_listenable_accessors!();
}

impl RestorableChangeNotifier for RestorableCupertinoTabController {
    fn dispose_value(self: Handle<Self>, app: &mut App, value: Ref<CupertinoTabController>) {
        value.handle().dispose(app);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use waft_embedder::{Color, Rect};
    use waft_painting::TextScaler;
    use waft_services::RestorationMap;
    use waft_widgets::testing::{WidgetTester, find, test_widgets};
    use waft_widgets::{BottomNavigationBarItem, GlobalKey, Icon, MediaQueryData, Text};

    use super::*;
    use crate::app::CupertinoApp;
    use crate::colors::CupertinoColors;
    use crate::icons::CupertinoIcons;

    /// The tabs each `on_tap` of a bar built by [`build_tab_bar`] reported.
    type SelectedTabs = Rc<RefCell<Vec<usize>>>;

    fn tab_generator(index: usize) -> BottomNavigationBarItem {
        BottomNavigationBarItem::new(Icon::new(Some(CupertinoIcons::map())))
            .label(format!("Tab {}", index + 1))
    }

    fn tab_bar_of(tabs: usize) -> CupertinoTabBar {
        CupertinoTabBar::new((0..tabs).map(tab_generator).collect())
    }

    fn build_tab_bar(selected_tabs: &SelectedTabs) -> CupertinoTabBar {
        let selected_tabs = Rc::clone(selected_tabs);
        tab_bar_of(2).on_tap(move |_, new_tab| selected_tabs.borrow_mut().push(new_tab))
    }

    /// A tab builder showing `Page <n>`, which records each tab it builds.
    fn pages(
        tabs_built: &Rc<RefCell<Vec<i32>>>,
    ) -> impl Fn(&mut App, BuildContext, i32) -> WidgetRef + 'static {
        let tabs_built = Rc::clone(tabs_built);
        move |_, _, index| {
            tabs_built.borrow_mut().push(index);
            Text::new(format!("Page {}", index + 1)).into_widget()
        }
    }

    fn pump_app<K>(tester: &mut WidgetTester, home: impl IntoWidget<K>) {
        tester.pump_widget(CupertinoApp::new().home(home));
    }

    fn onstage(tester: &WidgetTester, text: &str) -> usize {
        find::text(text).evaluate(tester).len()
    }

    fn built(tester: &WidgetTester, text: &str) -> usize {
        find::text(text).skip_offstage(false).evaluate(tester).len()
    }

    #[test]
    fn tab_switching() {
        test_widgets(|tester| {
            let selected_tabs = SelectedTabs::default();
            let tabs_built = Rc::default();
            pump_app(
                tester,
                CupertinoTabScaffold::new(build_tab_bar(&selected_tabs), pages(&tabs_built)),
            );
            assert_eq!(onstage(tester, "Page 1"), 1);
            assert_eq!(
                tester.text_color("Tab 1"),
                CupertinoColors::ACTIVE_BLUE.color()
            );
            assert_eq!(tester.text_color("Tab 2"), Color::new(0xFF999999));

            tester.tap(find::text("Tab 2"));
            tester.pump();

            assert_eq!(onstage(tester, "Page 2"), 1);
            assert_eq!(tester.text_color("Tab 1"), Color::new(0xFF999999));
            assert_eq!(
                tester.text_color("Tab 2"),
                CupertinoColors::ACTIVE_BLUE.color()
            );

            tester.tap(find::text("Tab 1"));
            tester.pump();

            assert_eq!(onstage(tester, "Page 1"), 1);
            // CupertinoTabBar's on_tap callbacks are passed on.
            assert_eq!(*selected_tabs.borrow(), [1, 0]);
        });
    }

    #[test]
    fn tabs_are_lazy_built_and_moved_offstage_when_inactive() {
        test_widgets(|tester| {
            let tabs_built = Rc::default();
            pump_app(
                tester,
                CupertinoTabScaffold::new(
                    build_tab_bar(&SelectedTabs::default()),
                    pages(&tabs_built),
                ),
            );

            assert_eq!(*tabs_built.borrow(), [0]);
            assert_eq!(onstage(tester, "Page 1"), 1);
            assert_eq!(built(tester, "Page 2"), 0);
            let first_page = tester.element(find::text("Page 1"));

            tester.tap(find::text("Tab 2"));
            tester.pump();

            // Both tabs are built but only one is onstage.
            assert_eq!(*tabs_built.borrow(), [0, 0, 1]);
            assert_eq!(onstage(tester, "Page 1"), 0);
            assert_eq!(built(tester, "Page 1"), 1);
            assert_eq!(onstage(tester, "Page 2"), 1);
            assert!(
                tester.element(find::text("Page 1").skip_offstage(false)) == first_page,
                "the tab left keeps its subtree, and so its state"
            );

            tester.tap(find::text("Tab 1"));
            tester.pump();

            assert_eq!(*tabs_built.borrow(), [0, 0, 1, 0, 1]);
            assert_eq!(onstage(tester, "Page 1"), 1);
            assert_eq!(onstage(tester, "Page 2"), 0);
            assert_eq!(built(tester, "Page 2"), 1);
        });
    }

    #[test]
    fn programmatic_tab_switching_by_changing_the_index_of_an_existing_controller() {
        test_widgets(|tester| {
            let selected_tabs = SelectedTabs::default();
            let controller = tester.update(|app| CupertinoTabController::initial_index(app, 1));
            pump_app(
                tester,
                CupertinoTabScaffold::new(build_tab_bar(&selected_tabs), pages(&Rc::default()))
                    .controller(controller.clone()),
            );
            assert_eq!(onstage(tester, "Page 2"), 1);

            tester.update(|app| controller.handle().set_index(app, 0));
            tester.pump();

            assert_eq!(onstage(tester, "Page 1"), 1);
            assert_eq!(tester.text_color("Tab 2"), Color::new(0xFF999999));
            // on_tap is not called when changing tabs programmatically.
            assert!(selected_tabs.borrow().is_empty());

            // Can still tap out of the programmatically selected tab.
            tester.tap(find::text("Tab 2"));
            tester.pump();

            assert_eq!(onstage(tester, "Page 2"), 1);
            assert_eq!(*selected_tabs.borrow(), [1]);
            assert_eq!(
                tester.update(|app| controller.handle().index(app)),
                1,
                "a tap reports to the controller"
            );
        });
    }

    #[test]
    fn programmatic_tab_switching_by_passing_in_a_new_controller() {
        test_widgets(|tester| {
            let selected_tabs = SelectedTabs::default();
            let old_controller = tester.update(|app| CupertinoTabController::initial_index(app, 1));
            pump_app(
                tester,
                CupertinoTabScaffold::new(build_tab_bar(&selected_tabs), pages(&Rc::default()))
                    .controller(old_controller.clone()),
            );
            assert_eq!(onstage(tester, "Page 2"), 1);

            let new_controller = tester.update(CupertinoTabController::new);
            pump_app(
                tester,
                CupertinoTabScaffold::new(build_tab_bar(&selected_tabs), pages(&Rc::default()))
                    .controller(new_controller.clone()),
            );
            assert_eq!(onstage(tester, "Page 1"), 1);
            // on_tap is not called when changing tabs programmatically.
            assert!(selected_tabs.borrow().is_empty());

            // The controller the scaffold was given before no longer drives it.
            tester.update(|app| old_controller.handle().set_index(app, 0));
            tester.update(|app| old_controller.handle().set_index(app, 1));
            tester.pump();
            assert_eq!(onstage(tester, "Page 1"), 1);

            tester.update(|app| new_controller.handle().set_index(app, 1));
            tester.pump();
            assert_eq!(onstage(tester, "Page 2"), 1);
        });
    }

    #[test]
    fn deleting_tabs_after_selecting_them_should_switch_to_the_last_available_tab() {
        test_widgets(|tester| {
            let selected_tabs = SelectedTabs::default();
            let bar = |tabs: usize| {
                let selected_tabs = Rc::clone(&selected_tabs);
                tab_bar_of(tabs).on_tap(move |_, new_tab| selected_tabs.borrow_mut().push(new_tab))
            };
            pump_app(
                tester,
                CupertinoTabScaffold::new(bar(4), pages(&Rc::default())),
            );
            // We didn't tap any tabs yet.
            assert!(selected_tabs.borrow().is_empty());

            tester.tap(find::text("Tab 4"));
            tester.pump();

            assert_eq!(*selected_tabs.borrow(), [3]);
            assert_eq!(onstage(tester, "Page 4"), 1);

            // Delete 2 tabs while Page 4 is still selected.
            pump_app(
                tester,
                CupertinoTabScaffold::new(bar(2), |_, _, index| {
                    // Change the builder too.
                    Text::new(format!("Different page {}", index + 1)).into_widget()
                }),
            );
            tester.pump();

            // We just deleted a tab.
            assert_eq!(*selected_tabs.borrow(), [3]);
            // Tab 1 was previously built so it's rebuilt again, albeit offstage.
            assert_eq!(onstage(tester, "Different page 1"), 0);
            assert_eq!(built(tester, "Different page 1"), 1);
            // Since all the tabs after tab 2 are deleted, tab 2 is now the last tab and the
            // actively shown tab.
            assert_eq!(onstage(tester, "Different page 2"), 1);
            // No more tab 4 since it's deleted.
            assert_eq!(built(tester, "Different page 4"), 0);
            // We also changed the builder so no tabs should be built with the old builder.
            assert_eq!(built(tester, "Page 1"), 0);
            assert_eq!(built(tester, "Page 4"), 0);
        });
    }

    /// Pumps a scaffold under `data`, whose tab is a box filling it; the box's rect, and the
    /// padding its context's media query reports.
    fn content_under(
        tester: &mut WidgetTester,
        data: MediaQueryData,
        scaffold: impl FnOnce(CupertinoTabScaffold) -> CupertinoTabScaffold,
        tab_bar: CupertinoTabBar,
    ) -> (Rect, EdgeInsets) {
        let content = GlobalKey::new();
        let inner_padding = Rc::new(Cell::new(None));
        let (key, seen) = (content.clone(), Rc::clone(&inner_padding));
        let tab_scaffold = scaffold(CupertinoTabScaffold::new(
            tab_bar,
            move |app, context, _| {
                seen.set(Some(MediaQuery::of(app, context).padding));
                SizedBox::expand().key(key.clone().into()).into_widget()
            },
        ));
        pump_app(tester, MediaQuery::new(data, tab_scaffold).into_widget());
        (
            tester.get_rect(&content),
            inner_padding.get().expect("the tab is built"),
        )
    }

    #[test]
    fn tab_contents_are_padded_when_there_are_view_insets() {
        test_widgets(|tester| {
            let (rect, padding) = content_under(
                tester,
                MediaQueryData::new().view_insets(EdgeInsets::only(0.0, 0.0, 0.0, 200.0)),
                |scaffold| scaffold,
                tab_bar_of(2),
            );
            assert_eq!(rect, Rect::from_ltwh(0.0, 0.0, 800.0, 400.0));
            // Don't generate more media query padding from the translucent bottom
            // tab since the tab is behind the keyboard now.
            assert_eq!(padding.bottom, 0.0);
        });
    }

    #[test]
    fn tab_contents_are_not_inset_when_resize_to_avoid_bottom_inset_overridden() {
        test_widgets(|tester| {
            let (rect, padding) = content_under(
                tester,
                MediaQueryData::new().view_insets(EdgeInsets::only(0.0, 0.0, 0.0, 200.0)),
                |scaffold| scaffold.resize_to_avoid_bottom_inset(false),
                tab_bar_of(2),
            );
            assert_eq!(rect, Rect::from_ltwh(0.0, 0.0, 800.0, 600.0));
            // Media query padding shows up in the inner content because it wasn't masked
            // by the view inset.
            assert_eq!(padding.bottom, 50.0);
        });
    }

    #[test]
    fn opaque_tab_bar_consumes_bottom_padding_while_non_opaque_tab_bar_does_not() {
        test_widgets(|tester| {
            let mut content_with_tab_bar_color = |color: u32| {
                content_under(
                    tester,
                    MediaQueryData::new().padding(EdgeInsets::only(0.0, 0.0, 0.0, 50.0)),
                    |scaffold| scaffold,
                    tab_bar_of(2).background_color(Color::new(color)),
                )
            };

            let (rect, padding) = content_with_tab_bar_color(0xAAFFFFFF);
            assert_eq!(padding, EdgeInsets::only(0.0, 0.0, 0.0, 100.0));
            assert_eq!(
                rect,
                Rect::from_ltwh(0.0, 0.0, 800.0, 600.0),
                "the content draws behind a translucent bar"
            );

            let (rect, padding) = content_with_tab_bar_color(0xFFFFFFFF);
            assert_eq!(padding, EdgeInsets::ZERO);
            assert_eq!(
                rect,
                Rect::from_ltwh(0.0, 0.0, 800.0, 500.0),
                "the content stops above an opaque bar"
            );
        });
    }

    #[test]
    fn text_scaling_is_off_for_the_bar_and_kept_for_the_content() {
        test_widgets(|tester| {
            let scalers = Rc::new(RefCell::new(Vec::new()));
            let in_bar = Rc::clone(&scalers);
            let item = move |index: usize| {
                let in_bar = Rc::clone(&in_bar);
                BottomNavigationBarItem::new(Builder::new(move |app, context| {
                    in_bar
                        .borrow_mut()
                        .push(("bar", MediaQuery::text_scaler_of(app, context)));
                    SizedBox::shrink().into_widget()
                }))
                .label(format!("Tab {}", index + 1))
            };
            let in_content = Rc::clone(&scalers);
            let scaffold = CupertinoTabScaffold::new(
                CupertinoTabBar::new(vec![item(0), item(1)]),
                move |app, context, _| {
                    in_content
                        .borrow_mut()
                        .push(("content", MediaQuery::text_scaler_of(app, context)));
                    SizedBox::shrink().into_widget()
                },
            );
            pump_app(
                tester,
                MediaQuery::with_clamped_text_scaling(None, 99.0, 99.0, scaffold),
            );

            let scalers = scalers.borrow();
            let of = |part: &str| -> Vec<TextScaler> {
                let seen = scalers.iter().filter(|(seen_in, _)| *seen_in == part);
                seen.map(|(_, scaler)| scaler.clone()).collect()
            };
            assert_eq!(of("bar"), [TextScaler::NO_SCALING, TextScaler::NO_SCALING]);
            assert_eq!(of("content"), [TextScaler::linear(99.0)]);
        });
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "current index 2 is out of bounds for the tab bar with 2 tabs")]
    fn a_controller_whose_index_is_out_of_bounds_is_refused() {
        test_widgets(|tester| {
            let controller = tester.update(|app| CupertinoTabController::initial_index(app, 2));
            pump_app(
                tester,
                CupertinoTabScaffold::new(tab_bar_of(2), pages(&Rc::default()))
                    .controller(controller),
            );
        });
    }

    #[test]
    fn state_restoration() {
        let app = || {
            CupertinoApp::new().restoration_scope_id("app").home(
                CupertinoTabScaffold::new(tab_bar_of(4), |_, _, i| {
                    Text::new(format!("Content {i}")).into_widget()
                })
                .restoration_id("scaffold"),
            )
        };
        let restart_and_restore = |data: Option<RestorationMap>| {
            let mut tester = WidgetTester::restoring(data);
            tester.pump_widget(app());
            tester
        };
        let shown = |tester: &WidgetTester| -> Vec<usize> {
            (0..4)
                .filter(|i| onstage(tester, &format!("Content {i}")) == 1)
                .collect()
        };

        let mut tester = restart_and_restore(None);
        assert_eq!(shown(&tester), [0]);

        tester.tap(find::text("Tab 3"));
        tester.pump_and_settle();
        assert_eq!(shown(&tester), [2]);

        let data = tester.restoration_data();
        let mut tester = restart_and_restore(data.clone());
        assert_eq!(shown(&tester), [2]);

        tester.tap(find::text("Tab 2"));
        tester.pump_and_settle();
        assert_eq!(shown(&tester), [1]);

        let tester = restart_and_restore(data);
        assert_eq!(shown(&tester), [2]);
    }
}
