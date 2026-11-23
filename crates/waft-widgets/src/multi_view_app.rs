//! Flutter counterpart: the `MultiViewApp` of Flutter web's embedding guide (not a framework
//! class there), which builds a `View` for every view the page adds.
//!
//! How an app's views work, by design:
//!
//! 1. Only the platform creates a view. The app asks for one in a window
//!    (`Platform::request_view`) or on a composition layer it owns (`ViewEmbedder::add_view`), or
//!    the system gives one unasked: a phone's screen, a web page's element, a task Android brings
//!    back from Recents.
//! 2. Every view arrives the same way, whoever started it: the binding's observers hear it added
//!    (`did_add_view`), removed (`did_remove_view`), or its request refused
//!    (`did_fail_view_request`). Asking returns a request id, never a view.
//! 3. The app decides what a view shows from the view's initial data, in one place: a
//!    [`MultiViewApp`]'s builder.
//! 4. Everything about a view is reached from the view: its window (`View::window`), its host,
//!    its metrics; inside its content, through `View::of` and `WindowScope::of`. An app that hosts
//!    a view on its own layer gives it its metrics, sight, input and removal through the
//!    `ViewEmbedder`, by the view's id, learned as it arrives.
//!
//! A [`MultiViewApp`] is what an app shows in each of its views, as a SwiftUI app's scenes are: its
//! builder answers, for every view the platform hosts for the app, what goes in it, from what the
//! view carries, its initial data, which says what it is for whether the app asked for it, in a
//! window or on a layer, or the system made or restored it, as a scene's activity or an
//! activity's intent does. A view it answers `None` for shows nothing here. It takes the views there at start, then hears each view
//! the platform adds or removes, and each request to close one it shows content in, from the binding.
//! Flutter's reads `platformDispatcher.views` again at each `didChangeMetrics` instead.
//!
//! The invariant a later editor must keep: it gives each view its builder answers for its content once, keyed
//! by its id, so a view keeps its subtree's state while others come and go; and it answers a
//! request to close a view exactly when it shows content in that view.

use std::collections::BTreeSet;
use std::fmt;
use std::rc::Rc;

use waft_embedder::{ViewId, ViewRef};
use waft_foundation::{App, Handle, ValueKey};

use crate::binding::{
    WidgetsBinding, WidgetsBindingObserverObject, WidgetsBindingObserverRef, close_or_ask_to_quit,
};
use crate::framework::{
    BuildContext, IntoWidget, KeyRef, State, StateData, StatefulWidget, WidgetRef,
};
use crate::state_accessors;
use crate::view::{View, ViewCollection};

/// What a [`MultiViewApp`] shows in one of the views the platform hosts for the app; `None` shows
/// nothing there.
///
/// A name of its own, not `ViewBuilder`: Material's `SearchAnchor` already owns that name in
/// Flutter, and a kit's `*` globs beside `waft::*`.
pub type MultiViewBuilder = Rc<dyn Fn(&mut App, &ViewRef) -> Option<WidgetRef>>;

/// What a request to close one of the views a [`MultiViewApp`] shows content in does, given the
/// view.
pub type ViewCloseHandler = Rc<dyn Fn(&mut App, &ViewRef)>;

/// Shows, in each view the platform hosts for the app, what its builder answers for it.
pub struct MultiViewApp {
    pub key: Option<KeyRef>,
    builder: MultiViewBuilder,
    on_close_requested: Option<ViewCloseHandler>,
}

impl MultiViewApp {
    /// An app that shows in each view what `builder` answers for it: the first view, a view the app
    /// asked for (`Platform::request_view`) and a view the system made or restored alike, told
    /// apart by their [`initial_data`](waft_embedder::View::initial_data).
    pub fn new(
        builder: impl Fn(&mut App, &ViewRef) -> Option<WidgetRef> + 'static,
    ) -> MultiViewApp {
        MultiViewApp {
            key: None,
            builder: Rc::new(builder),
            on_close_requested: None,
        }
    }

    /// What a request to close one of the views it shows content in does. By default the app's last view
    /// asks to quit, as Cmd-Q does, and any other's window closes.
    pub fn on_close_requested(mut self, handler: ViewCloseHandler) -> MultiViewApp {
        self.on_close_requested = Some(handler);
        self
    }
}

impl fmt::Debug for MultiViewApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultiViewApp").finish_non_exhaustive()
    }
}

impl StatefulWidget for MultiViewApp {
    type State = MultiViewAppState;

    fn key(&self) -> Option<&KeyRef> {
        self.key.as_ref()
    }

    fn create_state(&self) -> MultiViewAppState {
        MultiViewAppState {
            state: StateData::new(),
            views: Vec::new(),
            showing: BTreeSet::new(),
            observer: None,
        }
    }
}

/// The state of a [`MultiViewApp`]: the views the platform hosts, in the order they came.
pub struct MultiViewAppState {
    state: StateData<MultiViewApp>,
    views: Vec<ViewRef>,
    /// The views its builder answered content for at the last build, whose close requests it
    /// answers.
    showing: BTreeSet<ViewId>,
    /// Retains the observer identity for removal on disposal.
    observer: Option<WidgetsBindingObserverRef>,
}

impl MultiViewAppState {
    fn find(self: Handle<Self>, app: &App, view: ViewId) -> Option<ViewRef> {
        app.get(self)
            .views
            .iter()
            .find(|shown| shown.id() == view)
            .cloned()
    }
}

impl WidgetsBindingObserverObject for MultiViewAppState {
    fn did_add_view(self: Handle<Self>, app: &mut App, view: &ViewRef) {
        if self.find(app, view.id()).is_none() {
            let view = Rc::clone(view);
            self.set_state(app, |state| state.views.push(view));
        }
    }

    fn did_remove_view(self: Handle<Self>, app: &mut App, view: ViewId) {
        if self.find(app, view).is_some() {
            self.set_state(app, |state| state.views.retain(|shown| shown.id() != view));
        }
    }

    fn did_request_view_close(self: Handle<Self>, app: &mut App, view: ViewId) -> bool {
        if !app.get(self).showing.contains(&view) {
            return false;
        }
        let Some(view) = self.find(app, view) else {
            return false;
        };
        match self.widget(app).on_close_requested.clone() {
            Some(handler) => handler(app, &view),
            None => close_or_ask_to_quit(app, &view),
        }
        true
    }
}

impl State for MultiViewAppState {
    type Widget = MultiViewApp;
    state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let observer: WidgetsBindingObserverRef = Rc::new(self);
        WidgetsBinding::instance(app).add_observer(app, observer.clone());
        app.get_mut(self).observer = Some(observer);
        let views = app.platform().views();
        app.get_mut(self).views = views;
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(observer) = app.get_mut(self).observer.take() {
            WidgetsBinding::instance(app).remove_observer(app, &observer);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let builder = Rc::clone(&self.widget(app).builder);
        let views = app.get(self).views.clone();
        let mut showing = BTreeSet::new();
        let view_widgets = views
            .into_iter()
            .filter_map(|view| {
                let content = builder(app, &view)?;
                showing.insert(view.id());
                let key: KeyRef = ValueKey::new(view.id().0).into();
                Some(View::new(view, content).key(key).into_widget())
            })
            .collect();
        app.get_mut(self).showing = showing;
        ViewCollection::new(view_widgets).into_widget()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use waft_embedder::{
        Compositor, EmbedderClient, Platform as _, View as _, ViewId, ViewMetrics, WindowConfig,
    };
    use waft_test::{
        MockCompositor, TestPlatform, TestView, TestViewHost, TestWindow, TestWindows,
    };

    use super::*;
    use crate::test_harness::Quits;
    use crate::testing::WidgetTester;
    use crate::{AppLifecycleListener, SizedBox, run_app, run_app_with};

    struct Harness {
        tester: WidgetTester,
        windows: Rc<TestWindows>,
        quits: Rc<Quits>,
    }

    fn harness(views: &[Rc<TestView>]) -> Harness {
        let windows = Rc::new(TestWindows::new());
        let quits = Rc::new(Quits::default());
        let mut platform = TestPlatform::new()
            .with_windows(Rc::clone(&windows))
            .with_app_exit(Rc::clone(&quits) as Rc<dyn waft_embedder::AppExit>);
        for view in views {
            platform = platform.with_view(Rc::clone(view) as ViewRef);
        }
        let tester = WidgetTester::for_app(platform);
        Harness {
            tester,
            windows,
            quits,
        }
    }

    fn view(id: u64) -> Rc<TestView> {
        Rc::new(TestView::new(300.0, 200.0).with_id(ViewId(id)))
    }

    /// Mounts `root` and runs the frame that shows it.
    fn mount(tester: &mut WidgetTester, root: WidgetRef) {
        tester.update(|app| crate::binding::run_widget(app, root));
        tester.pump_by(Duration::ZERO);
    }

    fn run_app_titled(tester: &mut WidgetTester, title: &str) {
        let config = WindowConfig {
            title: title.to_owned(),
            ..WindowConfig::default()
        };
        tester.update(|app| run_app_with(app, SizedBox::new().into_widget(), config));
        tester.pump_by(Duration::ZERO);
    }

    #[test]
    fn every_view_the_app_is_given_shows_the_content() {
        let (first, second) = (view(1), view(2));
        let mut h = harness(&[Rc::clone(&first), Rc::clone(&second)]);
        run_app_titled(&mut h.tester, "unused where views are given");
        assert!(first.presented() > 0);
        assert!(second.presented() > 0);
        assert!(h.windows.made().is_empty(), "no window asked for");
    }

    #[test]
    fn given_no_view_the_app_asks_for_its_window_and_shows_the_content_in_it() {
        let mut h = harness(&[]);
        run_app_titled(&mut h.tester, "Notes");
        let window = Rc::clone(&h.windows.made()[0]);
        assert_eq!(window.config().title, "Notes");
        assert!(window.test_view().presented() > 0);
    }

    #[test]
    fn a_view_shows_what_its_data_says() {
        let inspector = Rc::new(
            TestView::new(300.0, 200.0)
                .with_id(ViewId(2))
                .with_initial_data("inspector"),
        );
        let mut h = harness(&[view(1), inspector]);
        let shown: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
        let seen = Rc::clone(&shown);
        mount(
            &mut h.tester,
            MultiViewApp::new(move |_app, view| {
                seen.borrow_mut().push(view.initial_data());
                Some(SizedBox::new().into_widget())
            })
            .into_widget(),
        );
        assert_eq!(*shown.borrow(), [None, Some("inspector".to_owned())]);
    }

    #[test]
    fn a_view_the_app_asked_for_arrives_and_shows_what_its_data_says() {
        let mut h = harness(&[]);
        let shown: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
        let seen = Rc::clone(&shown);
        mount(
            &mut h.tester,
            MultiViewApp::new(move |_app, view| {
                seen.borrow_mut().push(view.initial_data());
                (view.initial_data().as_deref() == Some("inspector"))
                    .then(|| SizedBox::new().into_widget())
            })
            .into_widget(),
        );
        h.tester
            .platform()
            .request_view(WindowConfig::default(), Some("inspector".to_owned()))
            .expect("a desktop makes windows");
        h.tester.pump();
        assert!(shown.borrow().contains(&Some("inspector".to_owned())));
        assert!(h.windows.made()[0].test_view().presented() > 0);
    }

    #[test]
    fn a_view_on_a_layer_of_the_apps_arrives_as_every_view_and_shows_what_its_data_says() {
        let compositor = Arc::new(MockCompositor::new());
        let layer = compositor.make_layer(false);
        let mut tester = WidgetTester::for_app(TestPlatform::new().with_compositor(compositor));
        let shown: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
        let seen = Rc::clone(&shown);
        mount(
            &mut tester,
            MultiViewApp::new(move |_app, view| {
                seen.borrow_mut().push(view.initial_data());
                Some(SizedBox::new().into_widget())
            })
            .into_widget(),
        );
        let metrics = ViewMetrics {
            physical_size: [200.0, 100.0],
            device_pixel_ratio: 2.0,
            ..ViewMetrics::default()
        };
        let embedder = tester.platform().view_embedder().expect("a view embedder");
        embedder
            .add_view(
                &layer,
                metrics,
                Rc::new(TestViewHost::default()),
                Some("prompt".to_owned()),
            )
            .expect("a view on the layer");
        let added = Rc::clone(&tester.platform().added_views()[0]);
        tester.pump();
        assert_eq!(*shown.borrow(), [Some("prompt".to_owned())]);
        assert!(added.presented() > 0);
    }

    /// An observer that handles the close requests of `view`, as whoever shows another view of
    /// the app's would.
    struct ClaimsTheView {
        view: ViewId,
        asked: std::cell::Cell<u32>,
    }

    impl crate::WidgetsBindingObserver for ClaimsTheView {
        fn did_request_view_close(&self, _app: &mut App, view: ViewId) -> bool {
            if view != self.view {
                return false;
            }
            self.asked.set(self.asked.get() + 1);
            true
        }
    }

    #[test]
    fn a_view_its_builder_leaves_shows_nothing_and_its_close_request_is_not_the_apps() {
        let window = TestWindow::around(
            TestView::new(300.0, 200.0)
                .with_id(ViewId(1))
                .with_initial_data("someone else's"),
            WindowConfig::default(),
        );
        let mut h = harness(&[Rc::clone(window.test_view()), view(2)]);
        mount(
            &mut h.tester,
            MultiViewApp::new(|_app, view| {
                view.initial_data()
                    .is_none()
                    .then(|| SizedBox::new().into_widget())
            })
            .into_widget(),
        );
        let owner = Rc::new(ClaimsTheView {
            view: ViewId(1),
            asked: std::cell::Cell::new(0),
        });
        h.tester.update(|app| {
            WidgetsBinding::instance(app).add_observer(app, owner.clone());
        });
        assert_eq!(window.test_view().presented(), 0);
        h.tester.shell().view_close_requested(ViewId(1));
        assert_eq!(owner.asked.get(), 1, "its owner decides");
        assert_eq!(window.closes(), 0, "nobody here closes it");
    }

    #[test]
    fn a_close_request_nobody_handles_closes_the_view() {
        let window = TestWindow::around(
            TestView::new(300.0, 200.0).with_id(ViewId(1)),
            WindowConfig::default(),
        );
        let mut h = harness(&[Rc::clone(window.test_view()), view(2)]);
        h.tester.update(|app| {
            crate::binding::run_widget(
                app,
                MultiViewApp::new(|_app, _view| Some(SizedBox::new().into_widget())).into_widget(),
            );
        });
        assert_eq!(
            window.test_view().presented(),
            0,
            "closed before its first frame"
        );
        h.tester.shell().view_close_requested(ViewId(1));
        assert_eq!(window.closes(), 1);
        assert_eq!(h.quits.count(), 0, "not the last view");
        h.tester.pump_by(Duration::ZERO);
    }

    #[test]
    fn its_close_handler_decides_for_the_views_it_shows_content_in() {
        let window = TestWindow::around(
            TestView::new(300.0, 200.0).with_id(ViewId(1)),
            WindowConfig::default(),
        );
        let mut h = harness(&[Rc::clone(window.test_view())]);
        let asked = Rc::new(std::cell::Cell::new(0));
        let counted = Rc::clone(&asked);
        mount(
            &mut h.tester,
            MultiViewApp::new(|_app, _view| Some(SizedBox::new().into_widget()))
                .on_close_requested(Rc::new(move |_app, view| {
                    assert_eq!(view.id(), ViewId(1));
                    counted.set(counted.get() + 1);
                }))
                .into_widget(),
        );
        h.tester.shell().view_close_requested(ViewId(1));
        assert_eq!(asked.get(), 1);
        assert_eq!(h.quits.count(), 0);
        assert_eq!(window.closes(), 0);
    }

    #[test]
    fn closing_the_apps_last_view_asks_to_quit() {
        let mut h = harness(&[]);
        run_app_titled(&mut h.tester, "Notes");
        let view = h.windows.made()[0].test_view().id();

        h.tester.shell().view_close_requested(view);
        assert_eq!(h.quits.count(), 1);
    }

    #[test]
    fn closing_the_apps_last_view_keeps_it_when_the_app_refuses_to_quit() {
        let mut h = harness(&[]);
        let listener = h.tester.update(|app| {
            AppLifecycleListener::new(app).on_exit_requested(
                app,
                Rc::new(|_app| {
                    waft_foundation::Task::ready(waft_embedder::AppExitResponse::Cancel)
                }),
            )
        });
        run_app_titled(&mut h.tester, "Notes");
        let view = h.windows.made()[0].test_view().id();

        h.tester.shell().view_close_requested(view);
        assert_eq!(h.quits.count(), 0);
        assert_eq!(h.windows.made()[0].closes(), 0);
        h.tester.update(|app| listener.dispose(app));
    }

    #[test]
    fn closing_one_of_several_views_closes_just_it() {
        let window = TestWindow::around(
            TestView::new(300.0, 200.0).with_id(ViewId(1)),
            WindowConfig::default(),
        );
        let mut h = harness(&[Rc::clone(window.test_view()), view(2)]);
        run_app_titled(&mut h.tester, "unused");

        h.tester.shell().view_close_requested(ViewId(1));
        assert_eq!(h.quits.count(), 0, "not the last view");
        assert_eq!(window.closes(), 1);
    }

    #[test]
    fn closing_a_given_view_in_a_window_closes_the_window_while_other_views_remain() {
        let window = TestWindow::around(
            TestView::new(300.0, 200.0).with_id(ViewId(1)),
            WindowConfig::default(),
        );
        let mut h = harness(&[Rc::clone(window.test_view()), view(2)]);
        run_app_titled(&mut h.tester, "unused where views are given");

        h.tester.shell().view_close_requested(ViewId(1));
        assert_eq!(window.closes(), 1);
        assert_eq!(h.quits.count(), 0, "not the last view");
    }

    #[test]
    fn a_view_given_after_the_start_shows_the_content() {
        let mut tester = WidgetTester::for_app(TestPlatform::new());
        mount(
            &mut tester,
            MultiViewApp::new(|_app, _view| Some(SizedBox::new().into_widget())).into_widget(),
        );
        let given = Rc::new(TestView::new(300.0, 200.0).with_id(ViewId(1)));
        tester
            .platform()
            .give_view(Rc::clone(&given) as waft_embedder::ViewRef);
        tester.pump();
        assert_eq!(given.presented(), 1);
    }

    #[test]
    fn a_view_taken_back_is_no_longer_shown() {
        let given = view(1);
        let mut tester =
            WidgetTester::for_app(TestPlatform::new().with_view(Rc::clone(&given) as ViewRef));
        mount(
            &mut tester,
            MultiViewApp::new(|_app, _view| Some(SizedBox::new().into_widget())).into_widget(),
        );
        tester.platform().take_view(ViewId(1));
        tester.pump();

        let presented = given.presented();
        tester.pump_by(Duration::from_millis(16));
        assert_eq!(given.presented(), presented, "not drawn once taken back");
    }

    #[test]
    #[should_panic(expected = "starts with MultiViewApp")]
    fn run_app_given_no_view_where_no_window_can_be_made_says_how_to_start() {
        let tester = WidgetTester::for_app(TestPlatform::new());
        tester.update(|app| run_app(app, SizedBox::new().into_widget()));
    }
}
