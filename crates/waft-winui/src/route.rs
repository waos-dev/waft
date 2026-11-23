//! WinUI's page navigation: a page enters as a `Frame` shows it, with
//! `NavigationThemeTransition` and its default `EntranceNavigationTransitionInfo`, and the page
//! it covers leaves and returns with the same transition's other storyboards.
//!
//! The values are the ones `ThemeTransitions.cpp` registers for each navigation trigger:
//!
//! - the entering page (`NavigatingTo`) starts 140 effective pixels lower, stays fully
//!   transparent for 150 ms, then slides up over 300 ms on the cubic `(0.1, 0.9, 0.2, 1.0)`
//!   with its opacity switched on at 150 ms;
//! - the covered page (`NavigatingAway`) fades out over 150 ms on the cubic
//!   `(0.7, 0.0, 1.0, 0.5)`;
//! - the page revealed by a back (`BackNavigatingTo`) stays invisible for 150 ms, then fades in
//!   over 300 ms on `(0.1, 0.9, 0.2, 1.0)`;
//! - the page leaving by a back (`BackNavigatingAway`) slides down 140 pixels over 150 ms on
//!   `(0.7, 0.0, 1.0, 0.5)` and is hidden at the end.
//!
//! As Flutter's Material and Cupertino routes are, the transition is a mixin
//! ([`FluentRouteTransitionMixin`]) two routes share: [`FluentPageRoute`], pushed with a builder,
//! and the route a Navigator makes from a [`FluentRoutePage`] of its `pages`, which reads the
//! page's widget from its settings as it builds, so the page the Navigator last gave it is the one
//! shown.

use std::any::Any;
use std::fmt::{self, Debug};
use std::rc::Rc;
use std::time::Duration;

use waft_animation::{AnimationRef, AnimationStatus, Cubic, Curve};
use waft_embedder::Offset;
use waft_foundation::{App, Ref};
use waft_widgets::{
    AnimatedBuilder, AnyTransitionRoute, BuildContext, IntoWidget, KeyRef, LocalHistoryRoute,
    LocalHistoryRouteData, ModalRoute, ModalRouteData, Opacity, OverlayRoute, OverlayRouteData,
    Page, PageRef, PageRoute, PageRouteData, PopInvokedWithResultCallback, PredictiveBackRoute,
    Route, RouteData, RouteRef, RouteResult, RouteSettingsRef, Semantics, Transform,
    TransitionRoute, TransitionRouteData, WidgetBuilder, WidgetRef,
};

/// A page in a Fluent app, entering and leaving with WinUI's `Frame` transition.
///
/// Shaped as `MaterialPageRoute` is: a [`PageRoute`] whose
/// [`build_transitions`](ModalRoute::build_transitions) plays WinUI's entrance for its own page
/// and its other storyboards for the page it covers or reveals.
///
/// By default, when the route is replaced, the previous route remains in memory. Set
/// [`maintain_state`](Self::maintain_state) to false to free its resources.
///
/// See also:
///
///  * [`WidgetsApp::page_route_builder`](waft_widgets::WidgetsApp::page_route_builder), which a
///    Fluent app points at this route.
///  * [`FluentApp`](crate::FluentApp), which builds its pages with it.
pub struct FluentPageRoute {
    route: RouteData,
    overlay_route: OverlayRouteData,
    transition_route: TransitionRouteData,
    local_history: LocalHistoryRouteData,
    modal_route: ModalRouteData,
    page_route: PageRouteData,
    /// Builds the page's content.
    pub builder: WidgetBuilder,
    maintain_state: bool,
    include_route_semantics: bool,
}

impl FluentPageRoute {
    /// How long a page takes to enter: the 150 ms the entrance stays still, then the 300 ms it
    /// slides up.
    pub const TRANSITION_DURATION: Duration = Duration::from_millis(450);

    /// How long a page takes to leave and the one beneath to return: the leaving page's 150 ms
    /// and the revealed page's 300 ms that follows it.
    pub const REVERSE_TRANSITION_DURATION: Duration = Duration::from_millis(450);

    /// How far below its place an entering page starts, in effective pixels.
    pub const TRANSLATION_OFFSET: f64 = 140.0;

    /// The 150 ms the entering page waits, and the covered page takes to fade, out of
    /// [`TRANSITION_DURATION`](Self::TRANSITION_DURATION).
    const OUT_DURATION: Duration = Duration::from_millis(150);

    /// The curve the entering page slides up on, and the revealed page fades in on.
    const IN_CURVE: Cubic = Cubic::new(0.1, 0.9, 0.2, 1.0);

    /// The curve the leaving and covered pages move on.
    const OUT_CURVE: Cubic = Cubic::new(0.7, 0.0, 1.0, 0.5);

    /// Creates a page route for `builder`'s page; Dart's optional arguments are the setters.
    ///
    /// Dart's defaults: `maintain_state` true, `fullscreen_dialog` false, `allow_snapshotting`
    /// true, `include_route_semantics` true, `barrier_dismissible` false.
    pub fn new(app: &mut waft_foundation::App, builder: WidgetBuilder) -> Ref<FluentPageRoute> {
        let route = RouteData::new(app, None, None);
        let transition_route = TransitionRouteData::new(app);
        let modal_route = ModalRouteData::new(app);
        let this = app.create(FluentPageRoute {
            route,
            overlay_route: OverlayRouteData::new(),
            transition_route,
            local_history: LocalHistoryRouteData::new(),
            modal_route,
            page_route: PageRouteData::new(),
            builder,
            maintain_state: true,
            include_route_semantics: true,
        });
        debug_assert!(TransitionRoute::opaque(this.handle(), app));
        this
    }

    /// The route `settings`, as Dart's `PageRoute` takes them.
    pub fn settings(
        self: Ref<Self>,
        app: &mut waft_foundation::App,
        settings: RouteSettingsRef,
    ) -> Ref<Self> {
        self.route_data_mut(app).set_settings(settings);
        self
    }

    /// Whether the route asks for focus when pushed, as Dart's `PageRoute` takes it.
    pub fn request_focus(
        self: Ref<Self>,
        app: &mut waft_foundation::App,
        request_focus: bool,
    ) -> Ref<Self> {
        self.route_data_mut(app)
            .set_request_focus(Some(request_focus));
        self
    }

    /// Dart `MaterialPageRoute(maintainState:)`.
    pub fn maintain_state(
        self: Ref<Self>,
        app: &mut waft_foundation::App,
        maintain_state: bool,
    ) -> Ref<Self> {
        app.get_mut(&self).maintain_state = maintain_state;
        self
    }

    /// Dart `MaterialPageRoute(fullscreenDialog:)`.
    pub fn fullscreen_dialog(
        self: Ref<Self>,
        app: &mut waft_foundation::App,
        fullscreen_dialog: bool,
    ) -> Ref<Self> {
        self.page_route_data_mut(app).fullscreen_dialog = fullscreen_dialog;
        self
    }

    /// Dart `MaterialPageRoute(allowSnapshotting:)`.
    pub fn allow_snapshotting(
        self: Ref<Self>,
        app: &mut waft_foundation::App,
        allow_snapshotting: bool,
    ) -> Ref<Self> {
        self.page_route_data_mut(app).allow_snapshotting = allow_snapshotting;
        self
    }

    /// Dart `MaterialPageRoute(includeRouteSemantics:)`.
    pub fn include_route_semantics(
        self: Ref<Self>,
        app: &mut waft_foundation::App,
        include: bool,
    ) -> Ref<Self> {
        app.get_mut(&self).include_route_semantics = include;
        self
    }

    /// Dart `MaterialPageRoute(barrierDismissible:)`.
    pub fn barrier_dismissible(
        self: Ref<Self>,
        app: &mut waft_foundation::App,
        dismissible: bool,
    ) -> Ref<Self> {
        self.page_route_data_mut(app).barrier_dismissible = dismissible;
        self
    }
}

/// WinUI's `Frame` transition for a [`PageRoute`]: Flutter's `MaterialRouteTransitionMixin`, for
/// WinUI. A route that implements it plays WinUI's entrance for its own page and the other
/// storyboards for the page it covers or reveals; it gives the mixin's overrides to each route
/// trait with [`fluent_route_transition_mixin_transition_route_overrides!`] and
/// [`fluent_route_transition_mixin_modal_route_overrides!`].
pub trait FluentRouteTransitionMixin: PageRoute {
    /// Builds the primary contents of the route.
    fn build_content(
        self: waft_foundation::Handle<Self>,
        app: &mut waft_foundation::App,
        context: BuildContext,
    ) -> WidgetRef;

    /// Whether this route introduces a route scope in the semantics tree.
    ///
    /// Defaults to true. When true, screen readers can treat pushes and pops of this route as
    /// page-level navigation.
    fn include_route_semantics(
        self: waft_foundation::Handle<Self>,
        app: &waft_foundation::App,
    ) -> bool {
        let _ = (self, app);
        true
    }

    /// [`FluentPageRoute::TRANSITION_DURATION`].
    fn transition_duration(
        self: waft_foundation::Handle<Self>,
        app: &mut waft_foundation::App,
    ) -> Duration {
        let _ = (self, app);
        FluentPageRoute::TRANSITION_DURATION
    }

    /// [`FluentPageRoute::REVERSE_TRANSITION_DURATION`].
    fn reverse_transition_duration(
        self: waft_foundation::Handle<Self>,
        app: &mut waft_foundation::App,
    ) -> Duration {
        let _ = (self, app);
        FluentPageRoute::REVERSE_TRANSITION_DURATION
    }

    /// The page beneath runs its covered animation when another page enters on top of it.
    fn can_transition_to(
        self: waft_foundation::Handle<Self>,
        app: &waft_foundation::App,
        next_route: AnyTransitionRoute,
    ) -> bool {
        let _ = self;
        let next = next_route.as_route();
        match next.as_page_route(app) {
            Some(page) => !page.as_modal_route().fullscreen_dialog(app),
            None => false,
        }
    }

    /// A page revealed by a back fades in as the page above leaves.
    fn can_transition_from(
        self: waft_foundation::Handle<Self>,
        app: &waft_foundation::App,
        previous_route: AnyTransitionRoute,
    ) -> bool {
        previous_route.as_route().as_page_route(app).is_some()
            && !PageRoute::fullscreen_dialog(self, app)
    }

    /// The content, in a route scope of the semantics tree unless
    /// [`include_route_semantics`](Self::include_route_semantics) says otherwise.
    fn build_page(
        self: waft_foundation::Handle<Self>,
        app: &mut waft_foundation::App,
        context: BuildContext,
    ) -> WidgetRef {
        let result = FluentRouteTransitionMixin::build_content(self, app, context);
        if !FluentRouteTransitionMixin::include_route_semantics(self, app) {
            return result;
        }
        Semantics::new()
            .scopes_route(true)
            .explicit_child_nodes(true)
            .child(result)
            .into_widget()
    }

    /// The entrance on the route's own animation and the covered page's fade on its secondary
    /// one.
    fn build_transitions(
        self: waft_foundation::Handle<Self>,
        app: &mut waft_foundation::App,
        animation: AnimationRef<f64>,
        secondary_animation: AnimationRef<f64>,
        child: WidgetRef,
    ) -> WidgetRef {
        let _ = self;
        fluent_transitions(app, animation, secondary_animation, child)
    }
}

/// The `TransitionRoute` overrides a [`FluentRouteTransitionMixin`] route inherits. Write it
/// inside `impl TransitionRoute for MyRoute { .. }`, with `opaque`.
#[macro_export]
macro_rules! fluent_route_transition_mixin_transition_route_overrides {
    () => {
        ::waft_widgets::modal_transition_route_overrides!();

        fn allow_snapshotting(
            self: ::waft_foundation::Handle<Self>,
            app: &::waft_foundation::App,
        ) -> bool {
            ::waft_widgets::PageRoute::allow_snapshotting(self, app)
        }

        fn transition_duration(
            self: ::waft_foundation::Handle<Self>,
            app: &mut ::waft_foundation::App,
        ) -> ::std::time::Duration {
            $crate::FluentRouteTransitionMixin::transition_duration(self, app)
        }

        fn reverse_transition_duration(
            self: ::waft_foundation::Handle<Self>,
            app: &mut ::waft_foundation::App,
        ) -> ::std::time::Duration {
            $crate::FluentRouteTransitionMixin::reverse_transition_duration(self, app)
        }

        fn can_transition_to(
            self: ::waft_foundation::Handle<Self>,
            app: &::waft_foundation::App,
            next_route: ::waft_widgets::AnyTransitionRoute,
        ) -> bool {
            $crate::FluentRouteTransitionMixin::can_transition_to(self, app, next_route)
        }

        fn can_transition_from(
            self: ::waft_foundation::Handle<Self>,
            app: &::waft_foundation::App,
            previous_route: ::waft_widgets::AnyTransitionRoute,
        ) -> bool {
            $crate::FluentRouteTransitionMixin::can_transition_from(self, app, previous_route)
        }
    };
}

/// The `ModalRoute` overrides a [`FluentRouteTransitionMixin`] route inherits. Write it inside
/// `impl ModalRoute for MyRoute { .. }`, with `maintain_state` and `fullscreen_dialog`.
#[macro_export]
macro_rules! fluent_route_transition_mixin_modal_route_overrides {
    () => {
        ::waft_widgets::modal_route_accessors!();

        fn barrier_dismissible(
            self: ::waft_foundation::Handle<Self>,
            app: &::waft_foundation::App,
        ) -> bool {
            ::waft_widgets::PageRoute::barrier_dismissible(self, app)
        }

        fn barrier_color(
            self: ::waft_foundation::Handle<Self>,
            _app: &::waft_foundation::App,
        ) -> ::std::option::Option<::waft_embedder::Color> {
            let _ = self;
            ::std::option::Option::None
        }

        fn barrier_label(
            self: ::waft_foundation::Handle<Self>,
            _app: &::waft_foundation::App,
        ) -> ::std::option::Option<::std::string::String> {
            let _ = self;
            ::std::option::Option::None
        }

        fn delegated_transition(
            self: ::waft_foundation::Handle<Self>,
            _app: &::waft_foundation::App,
        ) -> ::std::option::Option<::waft_widgets::DelegatedTransitionBuilder> {
            let _ = self;
            ::std::option::Option::None
        }

        fn build_page(
            self: ::waft_foundation::Handle<Self>,
            app: &mut ::waft_foundation::App,
            context: ::waft_widgets::BuildContext,
            _animation: ::waft_animation::AnimationRef<f64>,
            _secondary_animation: ::waft_animation::AnimationRef<f64>,
        ) -> ::waft_widgets::WidgetRef {
            $crate::FluentRouteTransitionMixin::build_page(self, app, context)
        }

        fn build_transitions(
            self: ::waft_foundation::Handle<Self>,
            app: &mut ::waft_foundation::App,
            _context: ::waft_widgets::BuildContext,
            animation: ::waft_animation::AnimationRef<f64>,
            secondary_animation: ::waft_animation::AnimationRef<f64>,
            child: ::waft_widgets::WidgetRef,
        ) -> ::waft_widgets::WidgetRef {
            $crate::FluentRouteTransitionMixin::build_transitions(
                self,
                app,
                animation,
                secondary_animation,
                child,
            )
        }
    };
}

impl Route for FluentPageRoute {
    waft_widgets::page_route_overrides!();
}

impl OverlayRoute for FluentPageRoute {
    waft_widgets::modal_overlay_route_overrides!();
}

impl PredictiveBackRoute for FluentPageRoute {
    waft_widgets::page_predictive_back_route_overrides!();
}

impl TransitionRoute for FluentPageRoute {
    crate::fluent_route_transition_mixin_transition_route_overrides!();

    fn opaque(self: waft_foundation::Handle<Self>, app: &waft_foundation::App) -> bool {
        PageRoute::opaque(self, app)
    }
}

impl LocalHistoryRoute for FluentPageRoute {
    waft_widgets::local_history_route_accessors!();
}

impl ModalRoute for FluentPageRoute {
    crate::fluent_route_transition_mixin_modal_route_overrides!();

    fn maintain_state(self: waft_foundation::Handle<Self>, app: &waft_foundation::App) -> bool {
        app.get(self).maintain_state
    }

    fn fullscreen_dialog(self: waft_foundation::Handle<Self>, app: &waft_foundation::App) -> bool {
        PageRoute::fullscreen_dialog(self, app)
    }
}

impl PageRoute for FluentPageRoute {
    waft_widgets::page_route_accessors!();
}

impl FluentRouteTransitionMixin for FluentPageRoute {
    fn build_content(
        self: waft_foundation::Handle<Self>,
        app: &mut waft_foundation::App,
        context: BuildContext,
    ) -> WidgetRef {
        let builder = Rc::clone(&app.get(self).builder);
        builder(app, context)
    }

    fn include_route_semantics(
        self: waft_foundation::Handle<Self>,
        app: &waft_foundation::App,
    ) -> bool {
        app.get(self).include_route_semantics
    }
}

/// A page of a Navigator's `pages`, shown by a [`FluentPageRoute`]: what Flutter's
/// `MaterialPage` and `CupertinoPage` are to their routes. A Navigator built from pages enters
/// and leaves each with WinUI's `Frame` transition as its pages change, and the page given
/// later under the same key is the one shown.
#[derive(Clone)]
pub struct FluentRoutePage {
    /// See [`Page::key`].
    pub key: Option<KeyRef>,
    /// See [`Page::name`].
    pub name: Option<String>,
    /// See [`Page::arguments`].
    pub arguments: Option<Rc<dyn Any>>,
    /// See [`Page::restoration_id`].
    pub restoration_id: Option<String>,
    /// See [`Page::can_pop`].
    pub can_pop: bool,
    /// See [`Page::on_pop_invoked`].
    pub on_pop_invoked: Option<PopInvokedWithResultCallback>,
    /// The page's content.
    pub child: WidgetRef,
    /// Whether the route keeps its page while another covers it.
    pub maintain_state: bool,
    /// Whether the page is a full-screen dialog, whose arrival the page beneath does not answer.
    pub fullscreen_dialog: bool,
    /// Whether the route may be snapshotted as it moves.
    pub allow_snapshotting: bool,
    /// Whether the page scopes the semantics tree as a screen of its own.
    pub include_route_semantics: bool,
}

impl FluentRoutePage {
    /// A page showing `child`; the rest are the setters, with `MaterialPage`'s defaults:
    /// `maintain_state` true, `fullscreen_dialog` false, `allow_snapshotting` true,
    /// `include_route_semantics` true, `can_pop` true.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn new<K>(child: impl IntoWidget<K>) -> FluentRoutePage {
        FluentRoutePage {
            key: None,
            name: None,
            arguments: None,
            restoration_id: None,
            can_pop: true,
            on_pop_invoked: None,
            child: child.into_widget(),
            maintain_state: true,
            fullscreen_dialog: false,
            allow_snapshotting: true,
            include_route_semantics: true,
        }
    }

    /// The page's key, by which the Navigator tells it is the same page as before.
    pub fn key(mut self, key: KeyRef) -> FluentRoutePage {
        self.key = Some(key);
        self
    }

    /// The page's name, its route's.
    pub fn name(mut self, name: impl Into<String>) -> FluentRoutePage {
        self.name = Some(name.into());
        self
    }

    /// The arguments its route carries.
    pub fn arguments(mut self, arguments: Rc<dyn Any>) -> FluentRoutePage {
        self.arguments = Some(arguments);
        self
    }

    /// The id its route is restored by.
    pub fn restoration_id(mut self, restoration_id: impl Into<String>) -> FluentRoutePage {
        self.restoration_id = Some(restoration_id.into());
        self
    }

    /// Whether a back may pop the page.
    pub fn can_pop(mut self, can_pop: bool) -> FluentRoutePage {
        self.can_pop = can_pop;
        self
    }

    /// What hears a back the page was asked to take, popped or not.
    pub fn on_pop_invoked(
        mut self,
        on_pop_invoked: PopInvokedWithResultCallback,
    ) -> FluentRoutePage {
        self.on_pop_invoked = Some(on_pop_invoked);
        self
    }

    /// Whether the route keeps its page while another covers it.
    pub fn maintain_state(mut self, maintain_state: bool) -> FluentRoutePage {
        self.maintain_state = maintain_state;
        self
    }

    /// Whether the page is a full-screen dialog.
    pub fn fullscreen_dialog(mut self, fullscreen_dialog: bool) -> FluentRoutePage {
        self.fullscreen_dialog = fullscreen_dialog;
        self
    }

    /// Whether the route may be snapshotted as it moves.
    pub fn allow_snapshotting(mut self, allow_snapshotting: bool) -> FluentRoutePage {
        self.allow_snapshotting = allow_snapshotting;
        self
    }

    /// Whether the page scopes the semantics tree as a screen of its own.
    pub fn include_route_semantics(mut self, include: bool) -> FluentRoutePage {
        self.include_route_semantics = include;
        self
    }
}

impl Debug for FluentRoutePage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FluentRoutePage")
            .field("key", &self.key)
            .field("name", &self.name)
            .field("child", &self.child)
            .finish_non_exhaustive()
    }
}

impl Page for FluentRoutePage {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn key(&self) -> Option<&KeyRef> {
        self.key.as_ref()
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn arguments(&self) -> Option<&Rc<dyn Any>> {
        self.arguments.as_ref()
    }

    fn restoration_id(&self) -> Option<&str> {
        self.restoration_id.as_deref()
    }

    fn can_pop(&self) -> bool {
        self.can_pop
    }

    fn on_pop_invoked(&self, app: &mut App, did_pop: bool, result: RouteResult) {
        if let Some(on_pop_invoked) = self.on_pop_invoked.clone() {
            on_pop_invoked(app, did_pop, result);
        }
    }

    fn create_route(&self, app: &mut App, context: BuildContext, this: &PageRef) -> RouteRef {
        let _ = context;
        PageBasedFluentPageRoute::new(app, Rc::clone(this), self.allow_snapshotting).into_erased()
    }
}

/// A page-based version of [`FluentPageRoute`]: Dart's `_PageBasedMaterialPageRoute`, for WinUI.
///
/// This route reads its content and options from its [`FluentRoutePage`], kept in its settings,
/// so they are up to date after page updates.
struct PageBasedFluentPageRoute {
    route: RouteData,
    overlay_route: OverlayRouteData,
    transition_route: TransitionRouteData,
    local_history: LocalHistoryRouteData,
    modal_route: ModalRouteData,
    page_route: PageRouteData,
}

impl PageBasedFluentPageRoute {
    fn new(
        app: &mut App,
        page: PageRef,
        allow_snapshotting: bool,
    ) -> Ref<PageBasedFluentPageRoute> {
        let route = RouteData::new(app, Some(RouteSettingsRef::Page(page)), None);
        let transition_route = TransitionRouteData::new(app);
        let modal_route = ModalRouteData::new(app);
        let mut page_route = PageRouteData::new();
        page_route.allow_snapshotting = allow_snapshotting;
        let this = app.create(PageBasedFluentPageRoute {
            route,
            overlay_route: OverlayRouteData::new(),
            transition_route,
            local_history: LocalHistoryRouteData::new(),
            modal_route,
            page_route,
        });
        debug_assert!(TransitionRoute::opaque(this.handle(), app));
        this
    }

    /// Dart's `_page` getter, read in place.
    fn with_page<T>(
        self: waft_foundation::Handle<Self>,
        app: &App,
        read: impl FnOnce(&FluentRoutePage) -> T,
    ) -> T {
        match Route::settings(self, app) {
            RouteSettingsRef::Page(page) => read(
                page.as_any()
                    .downcast_ref::<FluentRoutePage>()
                    .expect("a page-based Fluent page route's settings are its FluentRoutePage"),
            ),
            RouteSettingsRef::Settings(_) => {
                panic!("a page-based Fluent page route's settings are its FluentRoutePage")
            }
        }
    }
}

impl Route for PageBasedFluentPageRoute {
    waft_widgets::page_route_overrides!();
}

impl OverlayRoute for PageBasedFluentPageRoute {
    waft_widgets::modal_overlay_route_overrides!();
}

impl PredictiveBackRoute for PageBasedFluentPageRoute {
    waft_widgets::page_predictive_back_route_overrides!();
}

impl TransitionRoute for PageBasedFluentPageRoute {
    crate::fluent_route_transition_mixin_transition_route_overrides!();

    fn opaque(self: waft_foundation::Handle<Self>, app: &App) -> bool {
        PageRoute::opaque(self, app)
    }
}

impl LocalHistoryRoute for PageBasedFluentPageRoute {
    waft_widgets::local_history_route_accessors!();
}

impl ModalRoute for PageBasedFluentPageRoute {
    crate::fluent_route_transition_mixin_modal_route_overrides!();

    fn maintain_state(self: waft_foundation::Handle<Self>, app: &App) -> bool {
        self.with_page(app, |page| page.maintain_state)
    }

    fn fullscreen_dialog(self: waft_foundation::Handle<Self>, app: &App) -> bool {
        PageRoute::fullscreen_dialog(self, app)
    }
}

impl PageRoute for PageBasedFluentPageRoute {
    waft_widgets::page_route_accessors!();

    fn fullscreen_dialog(self: waft_foundation::Handle<Self>, app: &App) -> bool {
        self.with_page(app, |page| page.fullscreen_dialog)
    }
}

impl FluentRouteTransitionMixin for PageBasedFluentPageRoute {
    fn build_content(
        self: waft_foundation::Handle<Self>,
        app: &mut App,
        context: BuildContext,
    ) -> WidgetRef {
        let _ = context;
        self.with_page(app, |page| page.child.clone())
    }

    fn include_route_semantics(self: waft_foundation::Handle<Self>, app: &App) -> bool {
        self.with_page(app, |page| page.include_route_semantics)
    }
}

/// The entrance on the route's own animation and the covered page's fade on its secondary one.
fn fluent_transitions(
    _app: &mut waft_foundation::App,
    animation: AnimationRef<f64>,
    secondary_animation: AnimationRef<f64>,
    child: WidgetRef,
) -> WidgetRef {
    let primary: std::rc::Rc<dyn waft_foundation::Listenable> = std::rc::Rc::new(animation.clone());
    let secondary: std::rc::Rc<dyn waft_foundation::Listenable> =
        std::rc::Rc::new(secondary_animation.clone());
    let entering = AnimatedBuilder::new(primary, move |app, _context, child| {
        let (offset, opacity) = entrance(
            animation.value(app),
            animation.status(app) == AnimationStatus::Reverse,
        );
        let child = child
            .cloned()
            .unwrap_or_else(|| waft_widgets::SizedBox::shrink().into_widget());
        Opacity::new(opacity)
            .child(Transform::translate(Offset::new(0.0, offset)).child(child))
            .into_widget()
    })
    .child(child)
    .into_widget();
    AnimatedBuilder::new(secondary, move |app, _context, child| {
        let opacity = covered(
            secondary_animation.value(app),
            secondary_animation.status(app) == AnimationStatus::Reverse,
        );
        let child = child
            .cloned()
            .unwrap_or_else(|| waft_widgets::SizedBox::shrink().into_widget());
        Opacity::new(opacity).child(child).into_widget()
    })
    .child(entering)
    .into_widget()
}

/// How far down the page is and how visible, for `value` of its own animation: the entrance
/// while it runs forward, `BackNavigatingAway` while it reverses.
fn entrance(value: f64, reversing: bool) -> (f64, f64) {
    let duration = FluentPageRoute::TRANSITION_DURATION.as_secs_f64();
    let waited = FluentPageRoute::OUT_DURATION.as_secs_f64();
    if reversing {
        // The leaving page is away after 150 ms of the 450 ms a back takes; the rest of the
        // time belongs to the page being revealed.
        let elapsed = (1.0 - value) * duration;
        let progress = if elapsed >= waited {
            1.0
        } else {
            elapsed / waited
        };
        (
            FluentPageRoute::TRANSLATION_OFFSET * FluentPageRoute::OUT_CURVE.transform(progress),
            if progress < 1.0 { 1.0 } else { 0.0 },
        )
    } else {
        let elapsed = value * duration;
        let progress = ((elapsed - waited) / (duration - waited)).clamp(0.0, 1.0);
        (
            FluentPageRoute::TRANSLATION_OFFSET
                * (1.0 - FluentPageRoute::IN_CURVE.transform(progress)),
            if elapsed >= waited { 1.0 } else { 0.0 },
        )
    }
}

/// How visible the page is when it is covered, for `value` of its secondary animation: fading
/// out over 150 ms as a page enters, and from 150 ms into a back, fading in over 300 ms.
fn covered(value: f64, reversing: bool) -> f64 {
    let duration = FluentPageRoute::TRANSITION_DURATION.as_secs_f64();
    let waited = FluentPageRoute::OUT_DURATION.as_secs_f64();
    if reversing {
        let elapsed = (1.0 - value) * duration;
        if elapsed < waited {
            0.0
        } else if elapsed >= duration {
            1.0
        } else {
            FluentPageRoute::IN_CURVE.transform((elapsed - waited) / (duration - waited))
        }
    } else {
        let elapsed = (value * duration).min(waited);
        let progress = (elapsed / waited).clamp(0.0, 1.0);
        1.0 - FluentPageRoute::OUT_CURVE.transform(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entrance_waits_150_ms_invisible_140_pixels_low_then_slides_up() {
        let at = |ms: f64| entrance(ms / 450.0, false);
        assert_eq!(at(0.0), (140.0, 0.0));
        assert_eq!(at(149.0), (140.0, 0.0), "still waiting");
        assert_eq!(at(150.0), (140.0, 1.0), "the opacity keyframe at 150 ms");
        let (offset, opacity) = at(300.0);
        assert!(opacity == 1.0, "visible once the slide starts");
        assert!(
            offset > 0.0 && offset < 140.0,
            "already moving up: {offset}"
        );
        assert_eq!(at(450.0), (0.0, 1.0));
    }

    #[test]
    fn leaving_slides_down_over_150_ms_and_the_rest_of_the_back_belongs_to_the_page_below() {
        let at = |ms: f64| entrance(1.0 - ms / 450.0, true);
        assert_eq!(at(0.0), (0.0, 1.0));
        let (offset, opacity) = at(75.0);
        assert!(offset > 0.0 && offset < 140.0, "moving down: {offset}");
        assert_eq!(opacity, 1.0);
        assert_eq!(at(151.0), (140.0, 0.0));
        assert_eq!(at(450.0), (140.0, 0.0), "away while the page below returns");
    }

    #[test]
    fn a_covered_page_fades_out_in_150_ms() {
        assert_eq!(covered(0.0, false), 1.0);
        assert!(covered(75.0 / 450.0, false) > 0.0);
        assert_eq!(covered(150.0 / 450.0, false), 0.0);
        assert_eq!(covered(1.0, false), 0.0);
    }

    #[test]
    fn a_page_revealed_by_a_back_fades_in_after_150_ms() {
        let at = |ms: f64| covered(1.0 - ms / 450.0, true);
        assert_eq!(at(0.0), 0.0, "still covered at the start");
        assert_eq!(at(150.0), 0.0, "invisible until 150 ms");
        let middle = at(300.0);
        assert!(middle > 0.0 && middle < 1.0, "fading in: {middle}");
        assert_eq!(at(450.0), 1.0);
    }

    #[test]
    fn the_route_is_a_page_route_with_its_data() {
        let cell = waft_foundation::AppCell::new();
        let mut app = cell.borrow_mut();
        let route = FluentPageRoute::new(
            &mut app,
            std::rc::Rc::new(|_app, _context| waft_widgets::SizedBox::shrink().into_widget()),
        );
        let erased: waft_widgets::RouteRef = route.into_erased();
        assert!(erased.as_page_route(&app).is_some());
        assert!(erased.as_modal_route(&app).is_some());
        assert!(erased.as_transition_route(&app).is_some());
    }

    /// A Navigator built from `pages`, each a [`FluentRoutePage`] showing its name, that hears
    /// the pages it removes in `removed`.
    fn navigator(
        pages: &[&str],
        removed: &Rc<std::cell::RefCell<Vec<String>>>,
    ) -> waft_widgets::WidgetRef {
        let removed = Rc::clone(removed);
        let pages: Vec<waft_widgets::PageRef> = pages
            .iter()
            .map(|name| {
                let page = FluentRoutePage::new(waft_widgets::Text::new(*name))
                    .key(waft_foundation::ValueKey::new((*name).to_owned()).into())
                    .name(*name);
                Rc::new(page) as waft_widgets::PageRef
            })
            .collect();
        waft_widgets::Directionality::new(
            waft_painting::TextDirection::Ltr,
            waft_widgets::Navigator::new()
                .pages(pages)
                .on_did_remove_page(move |_app, page| {
                    removed
                        .borrow_mut()
                        .push(page.name().unwrap_or_default().to_owned());
                }),
        )
        .into_widget()
    }

    /// Where the text `text` is drawn and how visible, as the Navigator's transition has it now.
    fn shown(tester: &mut waft_widgets::testing::WidgetTester, text: &str) -> Option<f64> {
        let found = tester.update(|app| waft_widgets::testing::find::text(text).evaluate_in(app));
        found.first().map(|_| {
            tester
                .get_top_left(waft_widgets::testing::find::text(text))
                .dy()
        })
    }

    /// A page added to a Navigator's pages enters as WinUI's `Frame` shows a page: 140 pixels
    /// low and unseen for its first 150 ms, then up into place.
    #[test]
    fn a_page_added_to_the_pages_enters_with_winuis_transition() {
        let removed = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut tester = waft_widgets::testing::WidgetTester::new();
        tester.pump_widget(navigator(&["Home"], &removed));
        let home_top = shown(&mut tester, "Home").expect("the home shows");
        tester.pump_widget(navigator(&["Home", "Settings"], &removed));
        tester.pump_by(Duration::from_millis(100));
        let entering = shown(&mut tester, "Settings").expect("the page is pushed");
        assert_eq!(
            entering,
            home_top + FluentPageRoute::TRANSLATION_OFFSET,
            "still low"
        );
        tester.pump_and_settle();
        assert_eq!(shown(&mut tester, "Settings"), Some(home_top), "in place");
        assert!(removed.borrow().is_empty());
    }

    /// A page given again under its key shows what it holds now, as a page-based route reads
    /// its page from its settings.
    #[test]
    fn a_page_given_again_shows_its_new_content() {
        let removed = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut tester = waft_widgets::testing::WidgetTester::new();
        tester.pump_widget(navigator(&["Home"], &removed));
        let page = |text: &str| {
            Rc::new(
                FluentRoutePage::new(waft_widgets::Text::new(text))
                    .key(waft_foundation::ValueKey::new("home".to_owned()).into()),
            ) as waft_widgets::PageRef
        };
        let with = |text: &str| {
            waft_widgets::Directionality::new(
                waft_painting::TextDirection::Ltr,
                waft_widgets::Navigator::new()
                    .pages([page(text)])
                    .on_did_remove_page(|_app, _page| {}),
            )
            .into_widget()
        };
        tester.pump_widget(with("First"));
        tester.pump_and_settle();
        tester.pump_widget(with("Second"));
        assert!(
            shown(&mut tester, "Second").is_some(),
            "the new content shows"
        );
        assert!(shown(&mut tester, "First").is_none());
    }

    /// A page popped from the Navigator leaves with WinUI's back transition and is heard as
    /// removed, so the state its pages come from can follow.
    #[test]
    fn a_popped_page_is_heard_as_removed() {
        let removed = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut tester = waft_widgets::testing::WidgetTester::new();
        tester.pump_widget(navigator(&["Home", "Settings"], &removed));
        tester.pump_and_settle();
        tester.update(|app| {
            let settings = waft_widgets::testing::find::text("Settings").evaluate_in(app)[0];
            waft_widgets::Navigator::pop(app, settings, None);
        });
        tester.pump_and_settle();
        assert_eq!(*removed.borrow(), ["Settings"]);
    }

    #[test]
    fn the_route_uses_winuis_durations() {
        assert_eq!(FluentPageRoute::TRANSITION_DURATION.as_millis(), 450);
        assert_eq!(
            FluentPageRoute::REVERSE_TRANSITION_DURATION.as_millis(),
            450
        );
        assert_eq!(FluentPageRoute::TRANSLATION_OFFSET, 140.0);
    }
}
