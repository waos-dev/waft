//! Flutter counterpart: `cupertino/tab_view.dart`.

use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::rc::Rc;

use waft_foundation::{App, Handle, Ref};
use waft_widgets::{
    BuildContext, GlobalKey, HeroController, IntoWidget, KeyRef, Navigator, NavigatorObserverRef,
    NavigatorPopHandler, NavigatorState, RouteFactory, RouteRef, RouteSettings, RouteSettingsRef,
    State, StateData, StatefulWidget, TickerMode, WidgetBuilder, WidgetRef,
};

use crate::app::CupertinoApp;
use crate::route::CupertinoPageRoute;

/// A single tab view with its own [`Navigator`] state and history.
///
/// A typical tab view is used as the content of each tab in a
/// [`CupertinoTabScaffold`](crate::CupertinoTabScaffold) where multiple tabs with parallel
/// navigation states and history can co-exist.
///
/// [`CupertinoTabView`] configures the top-level [`Navigator`] to search for routes in the
/// following order:
///
///  1. For the `/` route, the [`builder`](Self::builder) property, if non-null, is used.
///
///  2. Otherwise, the [`routes`](Self::routes) table is used, if it has an entry for the route,
///     including `/` if `builder` is not specified.
///
///  3. Otherwise, [`on_generate_route`](Self::on_generate_route) is called, if provided. It
///     should return a non-null value for any _valid_ route not handled by `builder` and
///     `routes`.
///
///  4. Finally if all else fails [`on_unknown_route`](Self::on_unknown_route) is called.
///
/// These navigation properties are not shared with any sibling [`CupertinoTabView`] nor any
/// ancestor or descendant [`Navigator`] instances.
///
/// To push a route above this [`CupertinoTabView`] instead of inside it (such as when showing a
/// dialog on top of all tabs), use the root navigator.
///
/// See also:
///
///  * [`CupertinoTabScaffold`](crate::CupertinoTabScaffold), a typical host that supports
///    switching between tabs.
///  * [`CupertinoPageRoute`], a typical modal page route pushed onto the [`CupertinoTabView`]'s
///    [`Navigator`].
pub struct CupertinoTabView {
    pub key: Option<KeyRef>,

    /// The widget builder for the default route of the tab view
    /// ([`Navigator::DEFAULT_ROUTE_NAME`], which is `/`).
    ///
    /// If a `builder` is specified, then [`routes`](Self::routes) must not include an entry for
    /// `/`, as `builder` takes its place.
    ///
    /// Rebuilding a [`CupertinoTabView`] with a different `builder` will not clear its current
    /// navigation stack or update its descendant. Instead, trigger a rebuild from a descendant
    /// in its subtree. This can be done via methods such as:
    ///
    ///  * Calling `State::set_state` on a descendant [`StatefulWidget`]'s [`State`]
    ///  * Modifying an `InheritedWidget` that a descendant registered itself as a dependent to.
    pub builder: Option<WidgetBuilder>,

    /// A key to use when building this widget's [`Navigator`].
    ///
    /// If a `navigator_key` is specified, the [`Navigator`] can be directly manipulated without
    /// first obtaining it from a [`BuildContext`] via `Navigator::of`: from the `navigator_key`,
    /// use the [`GlobalKey::current_state`] getter.
    ///
    /// If this is changed, a new [`Navigator`] will be created, losing all the tab's state in
    /// the process; in that case, the [`navigator_observers`](Self::navigator_observers) must
    /// also be changed, since the previous observers will be attached to the previous
    /// navigator.
    pub navigator_key: Option<GlobalKey>,

    /// The title of the default route.
    pub default_title: Option<String>,

    /// This tab view's routing table.
    ///
    /// When a named route is pushed with `Navigator::push_named` inside this tab view, the
    /// route name is looked up in this map. If the name is present, the associated
    /// [`WidgetBuilder`] is used to construct a [`CupertinoPageRoute`] that performs an
    /// appropriate transition to the new route.
    ///
    /// If the tab view only has one page, then you can specify it using
    /// [`builder`](Self::builder) instead.
    ///
    /// If `builder` is specified, then it implies an entry in this table for the
    /// [`Navigator::DEFAULT_ROUTE_NAME`] route (`/`), and it is an error to redundantly provide
    /// such a route in the `routes` table.
    ///
    /// If a route is requested that is not specified in this table (or by `builder`), then the
    /// [`on_generate_route`](Self::on_generate_route) callback is called to build the page
    /// instead.
    ///
    /// This routing table is not shared with any routing tables of ancestor or descendant
    /// [`Navigator`]s.
    pub routes: Option<HashMap<String, WidgetBuilder>>,

    /// The route generator callback used when the tab view is navigated to a named route.
    ///
    /// This is used if [`routes`](Self::routes) does not contain the requested route.
    pub on_generate_route: Option<RouteFactory>,

    /// Called when [`on_generate_route`](Self::on_generate_route) also fails to generate a
    /// route.
    ///
    /// This callback is typically used for error handling. For example, this callback might
    /// always generate a "not found" page that describes the route that wasn't found.
    ///
    /// The default implementation pushes a route that displays an ugly error message.
    pub on_unknown_route: Option<RouteFactory>,

    /// The list of observers for the [`Navigator`] created in this tab view.
    ///
    /// This list of observers is not shared with ancestor or descendant [`Navigator`]s.
    pub navigator_observers: Vec<NavigatorObserverRef>,

    /// Restoration ID to save and restore the state of the [`Navigator`] built by this
    /// [`CupertinoTabView`].
    ///
    /// See also:
    ///
    ///  * `RestorationManager`, which explains how state restoration works in Flutter.
    pub restoration_scope_id: Option<String>,
}

impl CupertinoTabView {
    /// Creates the content area for a tab in a
    /// [`CupertinoTabScaffold`](crate::CupertinoTabScaffold).
    pub fn new() -> CupertinoTabView {
        CupertinoTabView {
            key: None,
            builder: None,
            navigator_key: None,
            default_title: None,
            routes: None,
            on_generate_route: None,
            on_unknown_route: None,
            navigator_observers: Vec::new(),
            restoration_scope_id: None,
        }
    }

    /// Dart `CupertinoTabView(key:)`.
    pub fn key(mut self, key: KeyRef) -> CupertinoTabView {
        self.key = Some(key);
        self
    }

    /// Dart `CupertinoTabView(builder:)`.
    pub fn builder(
        mut self,
        builder: impl Fn(&mut App, BuildContext) -> WidgetRef + 'static,
    ) -> CupertinoTabView {
        self.builder = Some(Rc::new(builder));
        self
    }

    /// Dart `CupertinoTabView(navigatorKey:)`.
    pub fn navigator_key(mut self, navigator_key: GlobalKey) -> CupertinoTabView {
        self.navigator_key = Some(navigator_key);
        self
    }

    /// Dart `CupertinoTabView(defaultTitle:)`.
    pub fn default_title(mut self, default_title: impl Into<String>) -> CupertinoTabView {
        self.default_title = Some(default_title.into());
        self
    }

    /// Dart `CupertinoTabView(routes:)`.
    pub fn routes(
        mut self,
        routes: impl IntoIterator<Item = (String, WidgetBuilder)>,
    ) -> CupertinoTabView {
        self.routes = Some(routes.into_iter().collect());
        self
    }

    /// Dart `CupertinoTabView(onGenerateRoute:)`.
    pub fn on_generate_route(
        mut self,
        on_generate_route: impl Fn(&mut App, &RouteSettings) -> Option<RouteRef> + 'static,
    ) -> CupertinoTabView {
        self.on_generate_route = Some(Rc::new(on_generate_route));
        self
    }

    /// Dart `CupertinoTabView(onUnknownRoute:)`.
    pub fn on_unknown_route(
        mut self,
        on_unknown_route: impl Fn(&mut App, &RouteSettings) -> Option<RouteRef> + 'static,
    ) -> CupertinoTabView {
        self.on_unknown_route = Some(Rc::new(on_unknown_route));
        self
    }

    /// Dart `CupertinoTabView(navigatorObservers:)`.
    pub fn navigator_observers(
        mut self,
        navigator_observers: impl IntoIterator<Item = NavigatorObserverRef>,
    ) -> CupertinoTabView {
        self.navigator_observers = navigator_observers.into_iter().collect();
        self
    }

    /// Dart `CupertinoTabView(restorationScopeId:)`.
    pub fn restoration_scope_id(
        mut self,
        restoration_scope_id: impl Into<String>,
    ) -> CupertinoTabView {
        self.restoration_scope_id = Some(restoration_scope_id.into());
        self
    }
}

impl Default for CupertinoTabView {
    fn default() -> CupertinoTabView {
        CupertinoTabView::new()
    }
}

impl Debug for CupertinoTabView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CupertinoTabView")
            .field("navigator_key", &self.navigator_key)
            .field("default_title", &self.default_title)
            .field("navigator_observers", &self.navigator_observers)
            .field("restoration_scope_id", &self.restoration_scope_id)
            .finish_non_exhaustive()
    }
}

impl StatefulWidget for CupertinoTabView {
    type State = CupertinoTabViewState;

    fn key(&self) -> Option<&KeyRef> {
        self.key.as_ref()
    }

    fn create_state(&self) -> CupertinoTabViewState {
        CupertinoTabViewState {
            state: StateData::new(),
            hero_controller: None,
            navigator_observers: Vec::new(),
            owned_navigator_key: None,
        }
    }
}

/// Dart's `_CupertinoTabViewState`.
pub struct CupertinoTabViewState {
    state: StateData<CupertinoTabView>,
    hero_controller: Option<Ref<HeroController>>,
    navigator_observers: Vec<NavigatorObserverRef>,
    owned_navigator_key: Option<GlobalKey>,
}

impl CupertinoTabViewState {
    fn update_observers(self: Handle<Self>, app: &mut App) {
        let mut navigator_observers = self.widget(app).navigator_observers.clone();
        let hero_controller = app
            .get(self)
            .hero_controller
            .clone()
            .expect("init_state creates the controller");
        navigator_observers.push(hero_controller.erase());
        app.get_mut(self).navigator_observers = navigator_observers;
    }

    fn navigator_key(self: Handle<Self>, app: &mut App) -> GlobalKey {
        if let Some(navigator_key) = self.widget(app).navigator_key.clone() {
            return navigator_key;
        }
        app.get_mut(self)
            .owned_navigator_key
            .get_or_insert_with(GlobalKey::new)
            .clone()
    }

    // Whether this tab is currently the active tab.
    #[expect(
        deprecated,
        reason = "the `TickerMode.of` tab_view.dart still reads although it is deprecated upstream"
    )]
    fn is_active(self: Handle<Self>, app: &mut App) -> bool {
        let context = self.context(app);
        TickerMode::of(app, context)
    }

    fn on_generate_route(
        self: Handle<Self>,
        app: &mut App,
        settings: &RouteSettings,
    ) -> Option<RouteRef> {
        let name = settings.name.clone();
        let widget = self.widget(app);
        let route_builder: Option<WidgetBuilder>;
        let mut title = None;
        if name.as_deref() == Some(Navigator::DEFAULT_ROUTE_NAME) && widget.builder.is_some() {
            route_builder = widget.builder.clone();
            title = widget.default_title.clone();
        } else {
            route_builder = name
                .as_ref()
                .and_then(|name| widget.routes.as_ref()?.get(name))
                .cloned();
        }
        if let Some(route_builder) = route_builder {
            let mut route = CupertinoPageRoute::new(app, route_builder);
            if let Some(title) = title {
                route = route.title(app, title);
            }
            let route = route.settings(app, RouteSettingsRef::Settings(settings.clone()));
            return Some(route.into_erased());
        }
        let on_generate_route = self.widget(app).on_generate_route.clone();
        on_generate_route.and_then(|on_generate_route| on_generate_route(app, settings))
    }

    fn on_unknown_route(
        self: Handle<Self>,
        app: &mut App,
        settings: &RouteSettings,
    ) -> Option<RouteRef> {
        let on_unknown_route = self
            .widget(app)
            .on_unknown_route
            .clone()
            .unwrap_or_else(|| {
                panic!(
                    "Could not find a generator for route {settings:?} in the \
                     _CupertinoTabViewState.\n\
                     Generators for routes are searched for in the following order:\n \
                     1. For the \"/\" route, the \"builder\" property, if non-null, is used.\n \
                     2. Otherwise, the \"routes\" table is used, if it has an entry for the \
                     route.\n \
                     3. Otherwise, onGenerateRoute is called. It should return a non-null value \
                     for any valid route not handled by \"builder\" and \"routes\".\n \
                     4. Finally if all else fails onUnknownRoute is called.\n\
                     Unfortunately, onUnknownRoute was not set."
                )
            });
        let result = on_unknown_route(app, settings);
        Some(result.unwrap_or_else(|| {
            panic!(
                "The onUnknownRoute callback returned null.\n\
                 When the _CupertinoTabViewState requested the route {settings:?} from its \
                 onUnknownRoute callback, the callback returned null. Such callbacks must never \
                 return null."
            )
        }))
    }
}

impl State for CupertinoTabViewState {
    type Widget = CupertinoTabView;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let hero_controller = CupertinoApp::create_cupertino_hero_controller(app);
        app.get_mut(self).hero_controller = Some(hero_controller);
        self.update_observers(app);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, old_widget: &CupertinoTabView) {
        let widget = self.widget(app);
        if widget.navigator_key != old_widget.navigator_key
            || !same_observers(&widget.navigator_observers, &old_widget.navigator_observers)
        {
            self.update_observers(app);
        }
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(hero_controller) = app.get_mut(self).hero_controller.take() {
            hero_controller.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let navigator_key = self.navigator_key(app);
        let mut child = Navigator::new()
            .key(navigator_key.clone().into())
            .on_generate_route(move |app, settings| self.on_generate_route(app, settings))
            .on_unknown_route(move |app, settings| self.on_unknown_route(app, settings))
            .observers(app.get(self).navigator_observers.clone());
        if let Some(restoration_scope_id) = self.widget(app).restoration_scope_id.clone() {
            child = child.restoration_scope_id(restoration_scope_id);
        }

        // Handle system back gestures only if the tab is currently active.
        NavigatorPopHandler::new(child)
            .enabled(self.is_active(app))
            .on_pop_with_result(move |app, _result| {
                if !self.is_active(app) {
                    return;
                }
                let navigator_key = self.navigator_key(app);
                navigator_key
                    .current_state::<NavigatorState>(app)
                    .expect("the tab view's navigator is built")
                    .maybe_pop(app, None);
            })
            .into_widget()
    }
}

/// Dart's `widget.navigatorObservers != oldWidget.navigatorObservers`, of two lists of the same
/// observers in the same order.
fn same_observers(a: &[NavigatorObserverRef], b: &[NavigatorObserverRef]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| a.handle().id() == b.handle().id())
}

#[cfg(test)]
mod tests {
    use waft_embedder::{EmbedderClient, View};
    use waft_widgets::testing::{WidgetTester, find, test_widgets};
    use waft_widgets::{BottomNavigationBarItem, Icon, Text};

    use super::*;
    use crate::bottom_tab_bar::CupertinoTabBar;
    use crate::icons::CupertinoIcons;
    use crate::tab_scaffold::CupertinoTabScaffold;

    fn pump_app<K>(tester: &mut WidgetTester, home: impl IntoWidget<K>) {
        tester.pump_widget(CupertinoApp::new().home(home));
    }

    fn onstage(tester: &WidgetTester, text: &str) -> usize {
        find::text(text).evaluate(tester).len()
    }

    /// A route table of one page, which shows `text`.
    fn route(name: &str, text: &'static str) -> (String, WidgetBuilder) {
        let builder: WidgetBuilder = Rc::new(move |_, _| Text::new(text).into_widget());
        (name.to_owned(), builder)
    }

    fn navigator_of(tester: &WidgetTester, key: &GlobalKey) -> Handle<NavigatorState> {
        key.current_state::<NavigatorState>(&tester.cell.borrow())
            .expect("the tab view's navigator")
    }

    /// Pushes the route named `name` in the navigator under `key`, to the end of its
    /// transition.
    fn push_named(tester: &mut WidgetTester, key: &GlobalKey, name: &str) {
        let navigator = navigator_of(tester, key);
        tester.update(|app| {
            navigator.push_named(app, name, None);
        });
        tester.pump_and_settle();
    }

    #[test]
    fn use_home() {
        test_widgets(|tester| {
            pump_app(
                tester,
                CupertinoTabView::new().builder(|_, _| Text::new("home").into_widget()),
            );
            assert_eq!(onstage(tester, "home"), 1);
        });
    }

    #[test]
    fn use_routes() {
        test_widgets(|tester| {
            pump_app(
                tester,
                CupertinoTabView::new().routes([route("/", "first route")]),
            );
            assert_eq!(onstage(tester, "first route"), 1);
        });
    }

    #[test]
    fn use_home_and_named_routes_and_pop_inside_the_tab() {
        test_widgets(|tester| {
            let key = GlobalKey::new();
            pump_app(
                tester,
                CupertinoTabView::new()
                    .navigator_key(key.clone())
                    .builder(|_, _| Text::new("first page").into_widget())
                    .routes([route("/2", "second named route")]),
            );
            assert_eq!(onstage(tester, "first page"), 1);

            push_named(tester, &key, "/2");
            assert_eq!(onstage(tester, "second named route"), 1);
            assert_eq!(onstage(tester, "first page"), 0);

            let navigator = navigator_of(tester, &key);
            assert!(tester.update(|app| navigator.maybe_pop(app, None)));
            tester.pump_and_settle();
            assert_eq!(onstage(tester, "first page"), 1);
            assert_eq!(onstage(tester, "second named route"), 0);
        });
    }

    #[test]
    fn use_on_generate_route_then_on_unknown_route() {
        test_widgets(|tester| {
            let key = GlobalKey::new();
            let page = |app: &mut App, text: &'static str| -> Option<RouteRef> {
                let builder: WidgetBuilder = Rc::new(move |_, _| Text::new(text).into_widget());
                Some(CupertinoPageRoute::new(app, builder).into_erased())
            };
            pump_app(
                tester,
                CupertinoTabView::new()
                    .navigator_key(key.clone())
                    .on_generate_route(move |app, settings| match settings.name.as_deref() {
                        Some(Navigator::DEFAULT_ROUTE_NAME) => page(app, "generated home"),
                        Some("/known") => page(app, "generated page"),
                        _ => None,
                    })
                    .on_unknown_route(move |app, _| page(app, "404")),
            );
            assert_eq!(onstage(tester, "generated home"), 1);

            push_named(tester, &key, "/known");
            assert_eq!(onstage(tester, "generated page"), 1);

            push_named(tester, &key, "/nowhere");
            assert_eq!(onstage(tester, "404"), 1);
        });
    }

    #[test]
    #[should_panic(expected = "Could not find a generator for route")]
    fn a_route_nothing_generates_and_no_on_unknown_route_is_an_error() {
        test_widgets(|tester| {
            pump_app(
                tester,
                CupertinoTabView::new().routes([route("/2", "second named route")]),
            );
        });
    }

    #[test]
    fn each_tab_of_a_scaffold_keeps_its_own_navigation() {
        test_widgets(|tester| {
            let keys = [GlobalKey::new(), GlobalKey::new()];
            let tab_keys = keys.clone();
            let item = |label: &str| {
                BottomNavigationBarItem::new(Icon::new(Some(CupertinoIcons::map()))).label(label)
            };
            pump_app(
                tester,
                CupertinoTabScaffold::new(
                    CupertinoTabBar::new(vec![item("Tab 1"), item("Tab 2")]),
                    move |_, _, index| {
                        let home: &'static str = if index == 0 { "home 1" } else { "home 2" };
                        CupertinoTabView::new()
                            .navigator_key(tab_keys[index as usize].clone())
                            .builder(move |_, _| Text::new(home).into_widget())
                            .routes([route("/detail", "detail")])
                            .into_widget()
                    },
                ),
            );
            assert_eq!(onstage(tester, "home 1"), 1);

            push_named(tester, &keys[0], "/detail");
            assert_eq!(onstage(tester, "detail"), 1);

            tester.tap(find::text("Tab 2"));
            tester.pump_and_settle();
            assert_eq!(onstage(tester, "home 2"), 1);
            assert_eq!(
                onstage(tester, "detail"),
                0,
                "pushed in the first tab alone"
            );

            tester.tap(find::text("Tab 1"));
            tester.pump_and_settle();
            assert_eq!(
                onstage(tester, "detail"),
                1,
                "the first tab is as it was left"
            );
            assert_eq!(onstage(tester, "home 2"), 0);
        });
    }

    /// The system's Back pops the route of the tab that shows, and no other tab's.
    #[test]
    fn the_systems_back_pops_inside_the_active_tab_alone() {
        test_widgets(|tester| {
            let keys = [GlobalKey::new(), GlobalKey::new()];
            let tab_keys = keys.clone();
            let item = |label: &str| {
                BottomNavigationBarItem::new(Icon::new(Some(CupertinoIcons::map()))).label(label)
            };
            pump_app(
                tester,
                CupertinoTabScaffold::new(
                    CupertinoTabBar::new(vec![item("Tab 1"), item("Tab 2")]),
                    move |_, _, index| {
                        let home: &'static str = if index == 0 { "home 1" } else { "home 2" };
                        CupertinoTabView::new()
                            .navigator_key(tab_keys[index as usize].clone())
                            .builder(move |_, _| Text::new(home).into_widget())
                            .routes([route("/detail", "detail")])
                            .into_widget()
                    },
                ),
            );
            let view = tester.view().id();
            push_named(tester, &keys[0], "/detail");

            tester.tap(find::text("Tab 2"));
            tester.pump_and_settle();
            tester.shell().pop_route(view);
            tester.pump_and_settle();
            tester.tap(find::text("Tab 1"));
            tester.pump_and_settle();
            assert_eq!(
                onstage(tester, "detail"),
                1,
                "Back on the second tab leaves the first tab's route"
            );

            tester.shell().pop_route(view);
            tester.pump_and_settle();
            assert_eq!(onstage(tester, "home 1"), 1);
            assert_eq!(onstage(tester, "detail"), 0);
        });
    }
}
