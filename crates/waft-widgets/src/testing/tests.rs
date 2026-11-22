//! The harness's own rules: its defaults, which frames it runs, when time passes, and what it
//! checks when a case ends.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use waft_embedder::{Size, TargetPlatform, TextDirection};
use waft_foundation::{Listener, Timer};
use waft_scheduler::{FrameTarget, PersistentFrameCallback, SchedulerBinding};

use super::{
    TargetPlatformVariant, WidgetTester, expect, find, finds_nothing, finds_one_widget,
    test_widgets, test_widgets_variant,
};
use crate::IntoWidget;
use crate::framework::GlobalKey;
use crate::widgets::basic::{Center, Directionality, SizedBox};
use crate::widgets::text::Text;

/// A timer that counts how often it fired.
fn counting_timer(tester: &WidgetTester, after: Duration) -> Rc<Cell<u32>> {
    let fired = Rc::new(Cell::new(0));
    let count = Rc::clone(&fired);
    tester.update(|app| {
        Timer::new(
            app,
            after,
            Listener::new(move |_| count.set(count.get() + 1)),
        )
    });
    fired
}

/// Counts the frames from here on, each of the tester's view alone.
fn counting_frames(tester: &WidgetTester) -> Rc<Cell<u32>> {
    let frames = Rc::new(Cell::new(0));
    let count = Rc::clone(&frames);
    let view = waft_embedder::View::id(&**tester.view());
    tester.update(|app| {
        SchedulerBinding::add_persistent_frame_callback(
            app,
            PersistentFrameCallback::new(move |_, views, _| {
                assert_eq!(views, [view]);
                count.set(count.get() + 1);
            }),
        )
    });
    frames
}

#[test]
fn the_view_is_800_by_600_logical_pixels_at_3_on_android() {
    test_widgets(|tester| {
        let key = GlobalKey::new();
        tester.pump_widget(SizedBox::expand().key(key.clone().into()));
        assert_eq!(tester.get_size(&key), Size::new(800.0, 600.0));
        assert_eq!(tester.device_pixel_ratio(), 3.0);
        assert_eq!(
            waft_embedder::View::metrics(&**tester.view()).physical_size,
            [2400.0, 1800.0]
        );
        assert_eq!(tester.target_platform(), TargetPlatform::Android);
    });
}

#[test]
fn text_is_set_in_flutter_test_whose_glyphs_are_squares_of_the_font_size() {
    test_widgets(|tester| {
        let key = GlobalKey::new();
        tester.pump_widget(Directionality::new(
            TextDirection::Ltr,
            Center::new().child(Text::new("Hi").key(key.clone().into())),
        ));
        assert_eq!(tester.get_size(&key), Size::new(28.0, 14.0));
    });
}

// flutter_test's `_runTestBody` pumps `Container(key: UniqueKey(), child: _preTestMessage)`
// before the case, so the case's first `pumpWidget` replaces a tree.
#[test]
fn a_case_starts_on_flutter_tests_pre_test_tree() {
    test_widgets(|tester| {
        expect(tester, &find::text("Test starting..."), finds_one_widget());
        assert_eq!(tester.view().presented(), 1);
        tester.pump_widget(SizedBox::new());
        expect(tester, &find::text("Test starting..."), finds_nothing());
    });
}

#[test]
fn pump_widget_runs_one_frame() {
    test_widgets(|tester| {
        let frames = counting_frames(tester);
        let presented = tester.view().presented();
        tester.pump_widget(SizedBox::new());
        assert_eq!(frames.get(), 1);
        assert_eq!(tester.view().presented(), presented + 1);
    });
}

#[test]
fn a_pump_frames_only_a_view_that_asked() {
    test_widgets(|tester| {
        let frames = counting_frames(tester);
        tester.pump_widget(SizedBox::new());
        tester.pump();
        tester.pump_by(Duration::from_millis(100));
        assert_eq!(frames.get(), 1, "no view asked");

        let view = waft_embedder::View::id(&**tester.view());
        tester.update(|app| SchedulerBinding::schedule_frame(app, FrameTarget::View(view)));
        tester.pump();
        assert_eq!(frames.get(), 2, "the view asked");
    });
}

#[test]
fn pump_fires_no_timer_and_pump_by_fires_those_due() {
    test_widgets(|tester| {
        let now = counting_timer(tester, Duration::ZERO);
        let later = counting_timer(tester, Duration::from_millis(100));

        tester.pump();
        assert_eq!((now.get(), later.get()), (0, 0));

        tester.pump_by(Duration::ZERO);
        assert_eq!((now.get(), later.get()), (1, 0));

        tester.pump_by(Duration::from_millis(99));
        assert_eq!(later.get(), 0);
        tester.pump_by(Duration::from_millis(1));
        assert_eq!(later.get(), 1);
    });
}

#[test]
#[should_panic(expected = "A Timer is still pending even after the widget tree was disposed.")]
fn a_timer_still_pending_at_the_end_fails_the_case() {
    test_widgets(|tester| {
        tester.pump_widget(SizedBox::new());
        counting_timer(tester, Duration::from_secs(1));
    });
}

#[test]
fn a_variant_runs_the_case_once_per_platform_in_order() {
    let ran = Mutex::new(Vec::new());
    test_widgets_variant(TargetPlatformVariant::mobile(), |tester| {
        ran.lock().unwrap().push(tester.target_platform());
    });
    assert_eq!(
        *ran.lock().unwrap(),
        [
            TargetPlatform::Android,
            TargetPlatform::IOS,
            TargetPlatform::Fuchsia
        ]
    );
}

/// flutter_test leaves the tree alone after a test that failed; a case that caught the panic of
/// a build left the tree half built, so its tester ends without disposing it.
#[test]
fn a_case_that_caught_a_build_panic_ends_without_tearing_down() {
    test_widgets(|tester| {
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tester.pump_widget(crate::widgets::basic::Builder::new(|_, _| {
                panic!("the build fails")
            }));
        }));
        assert!(built.is_err());
    });
}

#[test]
fn each_case_has_its_own_app_and_flags() {
    test_widgets(|tester| {
        tester.update(|app| crate::EditableText::set_debug_deterministic_cursor(app, true));
    });
    test_widgets(|tester| {
        assert!(!tester.update(|app| crate::EditableText::debug_deterministic_cursor(app)));
    });
}

/// Two `Text`s and an offstage one, the second keyed, in a column.
fn three_texts(tester: &mut WidgetTester, key: &GlobalKey) {
    use crate::widgets::basic::{Column, Offstage};
    tester.pump_widget(Directionality::new(
        TextDirection::Ltr,
        Column::new().children([
            Text::new("a").into_widget(),
            SizedBox::new()
                .key(key.clone().into())
                .child(Text::new("b"))
                .into_widget(),
            Offstage::new().child(Text::new("hidden")).into_widget(),
        ]),
    ));
}

#[test]
fn a_finder_skips_what_is_offstage_unless_asked() {
    use super::{expect, find, finds_nothing, finds_one_widget};
    test_widgets(|tester| {
        let key = GlobalKey::new();
        three_texts(tester, &key);
        expect(tester, &find::text("a"), finds_one_widget());
        expect(tester, &find::text("hidden"), finds_nothing());
        expect(
            tester,
            &find::text("hidden").skip_offstage(false),
            finds_one_widget(),
        );
        assert_eq!(find::by_type::<Text>().evaluate(tester).len(), 2);
    });
}

#[test]
fn first_last_at_descendant_and_ancestor_follow_tree_order() {
    use super::{expect, find, finds_nothing, finds_one_widget};
    test_widgets(|tester| {
        let key = GlobalKey::new();
        three_texts(tester, &key);
        let texts = find::by_type::<Text>();
        assert_eq!(
            tester
                .widget(texts.clone().first(), |text: &Text| text.data.clone())
                .as_deref(),
            Some("a")
        );
        assert_eq!(
            tester
                .widget(texts.clone().last(), |text: &Text| text.data.clone())
                .as_deref(),
            Some("b")
        );
        expect(tester, &texts.clone().at(1), finds_one_widget());
        expect(tester, &texts.at(2), finds_nothing());
        let under_key = find::descendant(find::by_global_key(&key), find::by_type::<Text>());
        assert_eq!(
            tester
                .widget(&under_key, |text: &Text| text.data.clone())
                .as_deref(),
            Some("b")
        );
        expect(
            tester,
            &find::ancestor(find::text("b"), find::by_type::<SizedBox>()),
            finds_one_widget(),
        );
        expect(
            tester,
            &find::widget_with_text::<SizedBox>("b"),
            finds_one_widget(),
        );
        expect(
            tester,
            &find::widget_with_text::<SizedBox>("a"),
            finds_nothing(),
        );
    });
}

#[test]
#[should_panic(
    expected = "Expected: exactly one matching candidate\n  Actual: Found 2 widgets \
                           with type \"Text\""
)]
fn a_finder_matcher_fails_with_darts_description() {
    use super::{expect, find, finds_one_widget};
    test_widgets(|tester| {
        let key = GlobalKey::new();
        three_texts(tester, &key);
        expect(tester, &find::by_type::<Text>(), finds_one_widget());
    });
}

#[test]
fn tap_finds_its_one_target_and_uses_the_next_pointer() {
    use super::find;
    use crate::widgets::gesture_detector::GestureDetector;
    test_widgets(|tester| {
        let taps = Rc::new(Cell::new(0));
        let count = Rc::clone(&taps);
        tester.pump_widget(
            Center::new().child(
                GestureDetector::new()
                    .behavior(waft_rendering::HitTestBehavior::Opaque)
                    .on_tap(Listener::new(move |_| count.set(count.get() + 1)))
                    .child(SizedBox::new().width(100.0).height(100.0)),
            ),
        );
        tester.tap(find::by_type::<GestureDetector>());
        tester.tap(find::by_type::<GestureDetector>());
        assert_eq!(taps.get(), 2);
        assert_eq!(tester.next_pointer(), 3);
    });
}

#[test]
#[should_panic(
    expected = "ambiguously found multiple matching widgets. The \"tap()\" method needs a \
                single target."
)]
fn tap_needs_a_single_target() {
    use super::find;
    test_widgets(|tester| {
        let key = GlobalKey::new();
        three_texts(tester, &key);
        tester.tap(find::by_type::<Text>());
    });
}

#[test]
fn pump_frames_pumps_until_the_duration_has_passed() {
    test_widgets(|tester| {
        let frames = counting_frames(tester);
        // Each pump frames only if one was asked; a ticking animation asks each frame.
        tester.pump_frames(SizedBox::new(), Duration::from_millis(50));
        assert_eq!(frames.get(), 1);
        assert_eq!(
            tester.cell.borrow().clock(),
            Duration::from_micros(16_683 * 3)
        );
    });
}

#[test]
fn a_view_metric_set_mid_case_reaches_the_framework_and_reset_puts_it_back() {
    use std::cell::RefCell;

    use waft_embedder::ViewPadding;

    use crate::widgets::basic::Builder;
    use crate::widgets::media_query::{MediaQuery, MediaQueryData};

    test_widgets(|tester| {
        let seen: Rc<RefCell<Option<MediaQueryData>>> = Rc::default();
        let read = Rc::clone(&seen);
        tester.pump_widget(Builder::new(move |app, context| {
            *read.borrow_mut() = Some(MediaQuery::of(app, context));
            SizedBox::new().into_widget()
        }));
        let data = || seen.borrow().clone().expect("built");
        assert_eq!(data().size, Size::new(800.0, 600.0));

        tester.view().set_physical_size(Size::new(1200.0, 2400.0));
        tester.view().set_view_insets(ViewPadding {
            bottom: 300.0,
            ..ViewPadding::ZERO
        });
        tester.pump();
        assert_eq!(data().size, Size::new(400.0, 800.0));
        assert_eq!(data().view_insets.bottom, 100.0);

        tester.view().set_device_pixel_ratio(1.0);
        tester.pump();
        assert_eq!(data().size, Size::new(1200.0, 2400.0));
        assert_eq!(tester.device_pixel_ratio(), 1.0);

        tester.view().reset();
        tester.pump();
        assert_eq!(data().size, Size::new(800.0, 600.0));
        assert_eq!(data().view_insets.bottom, 0.0);
    });
}

mod capturing {
    use waft_embedder::Color;
    use waft_embedder::valo::PixelBuffer;
    use waft_test::TestPlatform;

    use super::super::capture_image;
    use super::*;
    use crate::framework::AnyElement;
    use crate::widgets::basic::{ColoredBox, RepaintBoundary};

    /// A red box of `width` by `height` inside its own repaint boundary, in the middle of the
    /// view, on a platform with a GPU.
    fn red_box(width: f64, height: f64) -> WidgetTester {
        let mut tester = WidgetTester::with_platform(TestPlatform::new().with_gpu());
        tester.pump_widget(
            Center::new().child(
                RepaintBoundary::new().child(
                    SizedBox::new()
                        .width(width)
                        .height(height)
                        .child(ColoredBox::new(Color::from_argb(255, 255, 0, 0))),
                ),
            ),
        );
        tester
    }

    /// `element` captured, and its image read back.
    fn captured(tester: &WidgetTester, element: AnyElement) -> PixelBuffer {
        tester.update(|app| {
            let image = capture_image(app, element).expect("the platform draws images");
            pollster::block_on(app.platform().read_image_pixels(&image))
                .expect("the platform reads images back")
        })
    }

    #[test]
    fn a_widget_is_captured_with_its_repaint_boundary_at_its_logical_size() {
        let tester = red_box(20.0, 10.0);
        let element = find::by_type::<ColoredBox>().evaluate(&tester)[0];
        let pixels = captured(&tester, element);
        assert_eq!(pixels.layout().size, [20, 10]);
        assert!(
            pixels
                .bytes()
                .chunks(4)
                .all(|pixel| pixel == [255, 0, 0, 255]),
            "red throughout"
        );
    }

    #[test]
    fn the_view_is_captured_in_physical_pixels_with_what_it_leaves_unpainted_clear() {
        let tester = red_box(20.0, 10.0);
        let view = find::by_type::<crate::View>().evaluate(&tester)[0];
        let pixels = captured(&tester, view);
        let [width, height] = pixels.layout().size;
        let ratio = WidgetTester::DEVICE_PIXEL_RATIO;
        assert_eq!(
            [width, height],
            [
                (WidgetTester::SIZE.width() * ratio) as u32,
                (WidgetTester::SIZE.height() * ratio) as u32
            ]
        );
        let at = |x: u32, y: u32| {
            let start = ((y * width + x) * 4) as usize;
            &pixels.bytes()[start..start + 4]
        };
        assert_eq!(at(width / 2, height / 2), [255, 0, 0, 255], "the box");
        assert_eq!(at(0, 0), [0, 0, 0, 0], "nothing painted at the corner");
    }

    #[test]
    fn a_platform_with_no_gpu_draws_no_image() {
        let mut tester = WidgetTester::new();
        tester.pump_widget(RepaintBoundary::new().child(SizedBox::new()));
        let element = find::by_type::<SizedBox>().evaluate(&tester)[0];
        assert!(tester.update(|app| capture_image(app, element)).is_none());
    }
}

mod multi_window {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    use waft_embedder::{
        AppLifecycleState, EmbedderClient, Offset, Platform, Size, TextDirection,
        ViewFocusDirection, ViewFocusEvent, ViewFocusState, ViewId, WindowConfig,
    };
    use waft_foundation::{App, Handle, Listener, Timer};
    use waft_rendering::HitTestBehavior;
    use waft_test::{TestPlatform, TestView, TestWindows};

    use super::super::{TestPointer, WidgetTester, expect, find, finds_nothing, finds_one_widget};
    use crate::framework::{BuildContext, IntoWidget, State, StateData, StatefulWidget, WidgetRef};
    use crate::widgets::basic::{Center, Column, Directionality, SizedBox};
    use crate::widgets::gesture_detector::GestureDetector;
    use crate::widgets::text::Text;
    use crate::{
        MultiViewApp, WindowScope, run_app, run_widget, state_accessors,
    };

    /// A widget that calls `on_dispose` when its state is disposed, to verify teardown across
    /// every window.
    #[derive(Debug)]
    struct DisposeSpy {
        label: &'static str,
        disposed: Rc<RefCell<Vec<&'static str>>>,
    }

    struct DisposeSpyState {
        state: StateData<DisposeSpy>,
    }

    impl StatefulWidget for DisposeSpy {
        type State = DisposeSpyState;

        fn create_state(&self) -> DisposeSpyState {
            DisposeSpyState {
                state: StateData::new(),
            }
        }
    }

    impl State for DisposeSpyState {
        type Widget = DisposeSpy;
        state_accessors!();

        fn dispose(self: Handle<Self>, app: &mut App) {
            let widget = self.widget(app);
            widget.disposed.borrow_mut().push(widget.label);
        }

        fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
            Text::new(self.widget(app).label).into_widget()
        }
    }

    /// A `GestureDetector` button showing `label` inside a fixed-size box.
    fn button(label: &str, on_tap: impl Fn(&mut App) + 'static) -> WidgetRef {
        GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .on_tap(Listener::new(on_tap))
            .child(
                SizedBox::new()
                    .width(200.0)
                    .height(60.0)
                    .child(Center::new().child(Text::new(label))),
            )
            .into_widget()
    }

    // Modelled on flutter_test's multi_view_controller_test.dart and flutter's windowing_test.dart:
    // an app starts with no view via `without_views`, asks for its first window, opens a second
    // window on tap, receives taps in the second window, and closes it cleanly.
    #[test]
    fn desktop_app_opens_second_window_routes_taps_by_view_and_closes_it() {
        let windows = Rc::new(TestWindows::new());
        let platform = TestPlatform::new().with_windows(Rc::clone(&windows));
        let mut tester = WidgetTester::without_views(platform);
        assert!(tester.views().is_empty());

        let first_taps = Rc::new(Cell::new(0));
        let second_taps = Rc::new(Cell::new(0));
        let second_long_presses = Rc::new(Cell::new(0));
        let first_count = Rc::clone(&first_taps);
        let second_count = Rc::clone(&second_taps);
        let second_lp = Rc::clone(&second_long_presses);

        tester.update(|app| {
            app.platform()
                .request_view(
                    WindowConfig {
                        size: [400.0, 300.0],
                        title: "Main".to_owned(),
                        ..WindowConfig::default()
                    },
                    Some("main".to_owned()),
                )
                .expect("a desktop makes windows");
            run_widget(
                app,
                MultiViewApp::new(move |_app, view| {
                    let first_count = Rc::clone(&first_count);
                    let second_count = Rc::clone(&second_count);
                    let second_lp = Rc::clone(&second_lp);
                    match view.initial_data().as_deref() {
                        Some("main") => Some(
                            Directionality::new(
                                TextDirection::Ltr,
                                Center::new().child(Column::new().children([
                                    button("Open Second", move |app| {
                                        first_count.set(first_count.get() + 1);
                                        app.platform()
                                            .request_view(
                                                WindowConfig {
                                                    size: [300.0, 200.0],
                                                    title: "Inspector".to_owned(),
                                                    ..WindowConfig::default()
                                                },
                                                Some("inspector".to_owned()),
                                            )
                                            .expect("second window");
                                    }),
                                ])),
                            )
                            .into_widget(),
                        ),
                        Some("inspector") => {
                            let window = view.window();
                            Some(
                                Directionality::new(
                                    TextDirection::Ltr,
                                    Center::new().child(Column::new().children([
                                        GestureDetector::new()
                                            .behavior(HitTestBehavior::Opaque)
                                            .on_tap(Listener::new(move |_| {
                                                second_count.set(second_count.get() + 1);
                                            }))
                                            .on_long_press(Listener::new(move |_| {
                                                second_lp.set(second_lp.get() + 1);
                                            }))
                                            .child(
                                                SizedBox::new()
                                                    .width(120.0)
                                                    .height(40.0)
                                                    .child(Center::new().child(Text::new("Inspect"))),
                                            )
                                            .into_widget(),
                                        button("Close", move |_| {
                                            if let Some(window) = &window {
                                                window.close();
                                            }
                                        }),
                                    ])),
                                )
                                .into_widget(),
                            )
                        }
                        _ => None,
                    }
                })
                .into_widget(),
            );
        });
        tester.pump_by(Duration::ZERO);

        assert_eq!(tester.views().len(), 1);
        let first_view = tester.views()[0];
        assert_eq!(tester.view_of(find::text("Open Second")), first_view);
        expect(&tester, &find::text("Inspect"), finds_nothing());

        // Tap in window 1 to request window 2; it arrives on the next pump.
        tester.tap(find::text("Open Second"));
        assert_eq!(first_taps.get(), 1);
        tester.pump();

        assert_eq!(tester.views().len(), 2);
        let second_view = tester.views()[1];
        assert_ne!(first_view, second_view);
        assert_eq!(tester.view_of(find::text("Open Second")), first_view);
        assert_eq!(tester.view_of(find::text("Inspect")), second_view);

        // `tap(finder)` and `long_press(finder)` route to the second window automatically.
        tester.tap(find::text("Inspect"));
        assert_eq!(second_taps.get(), 1);
        assert_eq!(first_taps.get(), 1, "first window did not hear the tap");

        tester.long_press(find::text("Inspect"));
        assert_eq!(second_long_presses.get(), 1);

        // `tap_at` with `TestPointer::in_view` sends explicitly to the second window.
        let center = tester.get_center(find::text("Inspect"));
        let pointer = TestPointer::touch(tester.next_pointer()).in_view(second_view);
        tester.tap_at(center, pointer);
        assert_eq!(second_taps.get(), 2);

        // Closing the second window removes it at the next pump.
        tester.tap(find::text("Close"));
        tester.pump();
        assert_eq!(tester.views(), [first_view]);
        expect(&tester, &find::text("Inspect"), finds_nothing());
        expect(&tester, &find::text("Open Second"), finds_one_widget());
    }

    #[test]
    fn without_views_runs_an_apps_own_run_app() {
        let windows = Rc::new(TestWindows::new());
        let platform = TestPlatform::new().with_windows(Rc::clone(&windows));
        let mut tester = WidgetTester::without_views(platform);

        tester.update(|app| {
            run_app(
                app,
                Directionality::new(
                    TextDirection::Ltr,
                    Center::new().child(Text::new("Hello desktop")),
                )
                .into_widget(),
            );
        });
        tester.pump_by(Duration::ZERO);

        assert_eq!(tester.views().len(), 1);
        let view = tester.views()[0];
        assert_eq!(tester.view_of(find::text("Hello desktop")), view);
        assert!(windows.made()[0].test_view().presented() > 0);
    }

    #[test]
    fn give_view_adds_a_view_unasked_and_reports_metric_changes() {
        let mut tester = WidgetTester::without_views(TestPlatform::new());
        let seen_sizes: Rc<RefCell<Vec<Size>>> = Rc::default();
        let sizes = Rc::clone(&seen_sizes);
        tester.update(|app| {
            run_widget(
                app,
                MultiViewApp::new(move |_app, _view| {
                    let sizes = Rc::clone(&sizes);
                    Some(
                        crate::widgets::basic::Builder::new(move |app, context| {
                            let size = crate::MediaQuery::of(app, context).size;
                            sizes.borrow_mut().push(size);
                            Directionality::new(
                                TextDirection::Ltr,
                                Text::new("Phone screen"),
                            )
                            .into_widget()
                        })
                        .into_widget(),
                    )
                })
                .into_widget(),
            );
        });
        tester.pump_by(Duration::ZERO);
        assert!(tester.views().is_empty());

        let given = tester.give_view(
            TestView::with_pixel_ratio(800.0, 1200.0, 2.0).with_id(ViewId(7)),
        );
        tester.pump();
        assert_eq!(tester.views(), [ViewId(7)]);
        assert_eq!(tester.view_of(find::text("Phone screen")), ViewId(7));
        assert_eq!(*seen_sizes.borrow(), [Size::new(400.0, 600.0)]);

        given.set_physical_size(Size::new(600.0, 1000.0));
        tester.pump();
        assert_eq!(
            *seen_sizes.borrow(),
            [Size::new(400.0, 600.0), Size::new(300.0, 500.0)]
        );
    }

    #[test]
    fn shell_delivers_back_focus_close_and_lifecycle_events() {
        let windows = Rc::new(TestWindows::new());
        let mut tester = WidgetTester::without_views(
            TestPlatform::new().with_windows(Rc::clone(&windows)),
        );
        let events: Rc<RefCell<Vec<String>>> = Rc::default();
        let log = Rc::clone(&events);

        struct Observer {
            log: Rc<RefCell<Vec<String>>>,
        }

        impl crate::WidgetsBindingObserver for Observer {
            fn did_pop_route(&self, _app: &mut App, view: ViewId) -> bool {
                self.log.borrow_mut().push(format!("pop:{view:?}"));
                true
            }

            fn did_change_view_focus(&self, _app: &mut App, event: ViewFocusEvent) {
                self.log
                    .borrow_mut()
                    .push(format!("focus:{:?}:{:?}", event.view_id, event.state));
            }

            fn did_change_app_lifecycle_state(&self, _app: &mut App, state: AppLifecycleState) {
                self.log.borrow_mut().push(format!("lifecycle:{state:?}"));
            }
        }

        let observer = Rc::new(Observer {
            log: Rc::clone(&log),
        });
        tester.update(|app| {
            crate::WidgetsBinding::instance(app).add_observer(app, observer);
            run_app(app, SizedBox::new().into_widget());
        });
        tester.pump_by(Duration::ZERO);
        let view = tester.views()[0];

        assert!(tester.shell().pop_route(view));
        tester.shell().view_focus_changed(ViewFocusEvent {
            view_id: view,
            state: ViewFocusState::Focused,
            direction: ViewFocusDirection::Forward,
        });
        tester
            .shell()
            .app_lifecycle_state_changed(AppLifecycleState::Resumed);
        tester
            .shell()
            .app_lifecycle_state_changed(AppLifecycleState::Inactive);

        assert_eq!(
            *events.borrow(),
            [
                format!("pop:{view:?}"),
                format!("focus:{view:?}:Focused"),
                "lifecycle:Resumed".to_owned(),
                "lifecycle:Inactive".to_owned(),
            ]
        );
    }

    #[test]
    fn drop_tears_down_every_window_and_disposes_its_states() {
        let disposed: Rc<RefCell<Vec<&'static str>>> = Rc::default();
        {
            let windows = Rc::new(TestWindows::new());
            let mut tester = WidgetTester::without_views(
                TestPlatform::new().with_windows(Rc::clone(&windows)),
            );
            let spy = Rc::clone(&disposed);
            tester.update(|app| {
                app.platform()
                    .request_view(WindowConfig::default(), Some("one".to_owned()))
                    .unwrap();
                app.platform()
                    .request_view(WindowConfig::default(), Some("two".to_owned()))
                    .unwrap();
                run_widget(
                    app,
                    MultiViewApp::new(move |_app, view| {
                        let label = match view.initial_data().as_deref() {
                            Some("one") => "win-1",
                            Some("two") => "win-2",
                            _ => return None,
                        };
                        Some(
                            Directionality::new(
                                TextDirection::Ltr,
                                DisposeSpy {
                                    label,
                                    disposed: Rc::clone(&spy),
                                },
                            )
                            .into_widget(),
                        )
                    })
                    .into_widget(),
                );
            });
            tester.pump_by(Duration::ZERO);
            assert_eq!(tester.views().len(), 2);
            assert!(disposed.borrow().is_empty());
        }
        assert_eq!(*disposed.borrow(), ["win-1", "win-2"]);
    }

    #[test]
    #[should_panic(expected = "A Timer is still pending even after the widget tree was disposed.")]
    fn drop_catches_a_pending_timer_in_a_multi_window_test() {
        let windows = Rc::new(TestWindows::new());
        let mut tester = WidgetTester::without_views(
            TestPlatform::new().with_windows(Rc::clone(&windows)),
        );
        tester.update(|app| {
            run_app(app, SizedBox::new().into_widget());
            Timer::new(app, Duration::from_secs(5), Listener::new(|_| {}));
        });
        tester.pump_by(Duration::ZERO);
    }

    #[test]
    fn closing_the_only_window_unmounts_its_tree_on_pump() {
        let windows = Rc::new(TestWindows::new());
        let mut tester = WidgetTester::without_views(
            TestPlatform::new().with_windows(Rc::clone(&windows)),
        );
        tester.update(|app| {
            app.platform()
                .request_view(WindowConfig::default(), None)
                .unwrap();
            run_widget(
                app,
                MultiViewApp::new(|_app, _view| {
                    Some(
                        Directionality::new(
                            TextDirection::Ltr,
                            crate::widgets::basic::Builder::new(|app, context| {
                                let window = WindowScope::of(app, context);
                                button("Close Only", move |_| window.close())
                            }),
                        )
                        .into_widget(),
                    )
                })
                .into_widget(),
            );
        });
        tester.pump_by(Duration::ZERO);
        expect(&tester, &find::text("Close Only"), finds_one_widget());

        tester.tap(find::text("Close Only"));
        tester.pump();
        assert!(tester.views().is_empty());
        expect(&tester, &find::text("Close Only"), finds_nothing());
    }

    #[test]
    fn platform_clock_only_moves_with_pump_by() {
        let mut tester = WidgetTester::new();
        tester.pump_widget(SizedBox::new());
        let t0 = tester.platform().now();
        std::thread::sleep(Duration::from_millis(5));
        tester.pump();
        assert_eq!(tester.platform().now(), t0);

        tester.pump_by(Duration::from_millis(50));
        assert_eq!(
            tester.platform().now().duration_since(t0),
            Duration::from_millis(50)
        );
    }

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "tests pump_until waiting for work on a real background thread"
    )]
    fn pump_until_waits_for_work_completing_on_another_thread() {
        let mut tester = WidgetTester::without_views(TestPlatform::new());
        let ready = Rc::new(Cell::new(false));
        let set_ready = Rc::clone(&ready);
        let (tx, rx) = std::sync::mpsc::channel::<()>();

        tester.update(|app| {
            let cx = app.to_async();
            let flag = Rc::clone(&set_ready);
            // A future woken from a real worker thread, as async host I/O or a program thread does.
            let waker_slot: std::sync::Arc<std::sync::Mutex<Option<std::task::Waker>>> =
                std::sync::Arc::default();
            let done_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker_waker = std::sync::Arc::clone(&waker_slot);
            let worker_done = std::sync::Arc::clone(&done_flag);
            drop(app.spawn(async move |_cx| {
                std::future::poll_fn(|task_cx| {
                    if done_flag.load(std::sync::atomic::Ordering::Acquire) {
                        std::task::Poll::Ready(())
                    } else {
                        *waker_slot.lock().unwrap() = Some(task_cx.waker().clone());
                        std::task::Poll::Pending
                    }
                })
                .await;
                flag.set(true);
            }));
            std::thread::spawn(move || {
                rx.recv().unwrap();
                std::thread::sleep(Duration::from_millis(10));
                worker_done.store(true, std::sync::atomic::Ordering::Release);
                if let Some(waker) = worker_waker.lock().unwrap().take() {
                    waker.wake();
                }
            });
            drop(cx);
        });

        tester.pump();
        assert!(!ready.get());
        tx.send(()).unwrap();
        tester.pump_until("worker thread to complete", |_| ready.get());
        assert!(ready.get());
    }

    #[test]
    fn drag_in_second_window_moves_the_second_window_gesture() {
        let windows = Rc::new(TestWindows::new());
        let mut tester = WidgetTester::without_views(
            TestPlatform::new().with_windows(Rc::clone(&windows)),
        );
        let dragged = Rc::new(Cell::new(Offset::ZERO));
        let seen = Rc::clone(&dragged);

        tester.update(|app| {
            app.platform()
                .request_view(WindowConfig::default(), Some("a".to_owned()))
                .unwrap();
            app.platform()
                .request_view(WindowConfig::default(), Some("b".to_owned()))
                .unwrap();
            run_widget(
                app,
                MultiViewApp::new(move |_app, view| match view.initial_data().as_deref() {
                    Some("a") => Some(
                        Directionality::new(TextDirection::Ltr, Text::new("First")).into_widget(),
                    ),
                    Some("b") => {
                        let seen = Rc::clone(&seen);
                        Some(
                            Directionality::new(
                                TextDirection::Ltr,
                                Center::new().child(
                                    GestureDetector::new()
                                        .behavior(HitTestBehavior::Opaque)
                                        .on_pan_update(Rc::new(move |_, details| {
                                            seen.set(seen.get() + details.delta);
                                        }))
                                        .child(
                                            SizedBox::new()
                                                .width(200.0)
                                                .height(200.0)
                                                .child(Center::new().child(Text::new("DragMe"))),
                                        ),
                                ),
                            )
                            .into_widget(),
                        )
                    }
                    _ => None,
                })
                .into_widget(),
            );
        });
        tester.pump_by(Duration::ZERO);
        tester.drag(find::text("DragMe"), Offset::new(50.0, 0.0));
        assert!(dragged.get().dx() > 0.0);
    }
}
