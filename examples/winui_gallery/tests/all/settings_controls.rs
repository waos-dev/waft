//! The Community Toolkit's `SettingsCard`, `RightAlignedCompactToggleSwitchStyle` and `SettingsExpander`: where the template puts each part, the widths at which a card wraps, and what a click, a drag and the chevron do.
use crate::common::{Fixture, GalleryFixtureExt, mount};
use crate::toggle_switch::drag;
use std::cell::Cell;
use std::rc::Rc;
use waft_embedder::{Offset, PointerChange};
use waft_foundation::{App, Listener};
use waft_painting::AlignmentGeometry;
use waft_rendering::RenderParagraph;
use waft_widgets::*;
use waft_winui::*;
use winui_gallery::Feature;

/// The view the controls are mounted in.
const VIEW: [u32; 2] = [800, 600];
/// A card's border and padding before its content: `SettingsCardBorderThickness` and `SettingsCardPadding`.
const CARD_EDGE: f64 = 1.0 + 16.0;
/// The header icon's column: `SettingsCardHeaderIconMargin` around `SettingsCardHeaderIconMaxSize`.
const ICON_COLUMN: f64 = 2.0 + SETTINGS_CARD_HEADER_ICON_MAX_SIZE + 20.0;

/// A box on the view, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Bounds {
    fn width(&self) -> f64 {
        self.right - self.left
    }

    fn height(&self) -> f64 {
        self.bottom - self.top
    }

    fn center(&self) -> Offset {
        Offset::new(
            (self.left + self.right) / 2.0,
            (self.top + self.bottom) / 2.0,
        )
    }
}

/// `control` at the top left of a light `FluentApp`, `width` wide.
fn mounted(width: f64, control: impl FnOnce(&mut App) -> WidgetRef + 'static) -> Fixture {
    let mut fixture = Fixture::with_root(
        VIEW,
        mount(move |app| {
            winui_gallery::install_fonts(app);
            let control = control(app);
            FluentApp::new()
                .theme(FluentTheme::light())
                .home(
                    Align::new()
                        .alignment(AlignmentGeometry::TOP_LEFT)
                        .child(SizedBox::new().width(width).child(control)),
                )
                .into_widget()
        }),
    );
    // The page's entrance and an expander's opening both end within this.
    fixture.pump();
    fixture.pump();
    fixture
}

/// `child` under `key`, for [`bounds_of`].
fn keyed<K>(key: &GlobalKey, child: impl IntoWidget<K>) -> WidgetRef {
    KeyedSubtree::new(child)
        .key(key.clone().into())
        .into_widget()
}

/// A 60 × 32 box under `key`, standing for a card's control.
fn control(key: &GlobalKey) -> WidgetRef {
    keyed(key, SizedBox::new().width(60.0).height(32.0))
}

/// Where the widget under `key` is, or `None` while nothing is mounted under it.
fn bounds_of(fixture: &Fixture, key: &GlobalKey) -> Option<Bounds> {
    let app = fixture.cell.borrow();
    let object = key
        .current_context(&app)?
        .find_render_object(&app)?
        .as_box()?;
    let size = object.size(&app);
    let origin = object.local_to_global(&app, Offset::new(0.0, 0.0), None);
    Some(Bounds {
        left: origin.dx(),
        top: origin.dy(),
        right: origin.dx() + size.width(),
        bottom: origin.dy() + size.height(),
    })
}

/// Where the label `text` is.
fn text_bounds(fixture: &Fixture, text: &str) -> Bounds {
    let elements = fixture.elements();
    let app = fixture.cell.borrow();
    for element in elements {
        if let Some(object) = element.render_object(&app)
            && let Some(paragraph) = object.downcast::<RenderParagraph>(&app)
            && paragraph.text(&app).to_plain_text(true, true) == text
        {
            let object = object.as_box().unwrap();
            let size = object.size(&app);
            let origin = object.local_to_global(&app, Offset::new(0.0, 0.0), None);
            return Bounds {
                left: origin.dx(),
                top: origin.dy(),
                right: origin.dx() + size.width(),
                bottom: origin.dy() + size.height(),
            };
        }
    }
    panic!("missing label {text}")
}

/// How many labels say `text`, shown or not.
fn label_count(fixture: &Fixture, text: &str) -> usize {
    let elements = fixture.elements();
    let app = fixture.cell.borrow();
    elements
        .into_iter()
        .filter(|element| {
            element
                .render_object(&app)
                .and_then(|object| object.downcast::<RenderParagraph>(&app))
                .is_some_and(|paragraph| paragraph.text(&app).to_plain_text(true, true) == text)
        })
        .count()
}

/// The colour the view shows at `point`, over white.
fn pixel(fixture: &Fixture, point: Offset) -> [u8; 3] {
    let [width, _] = fixture.size();
    let index = (point.dy() as usize * width as usize + point.dx() as usize) * 4;
    let pixels = fixture.pixels();
    [pixels[index], pixels[index + 1], pixels[index + 2]]
}

#[test]
fn settings_card_puts_the_header_at_the_start_and_the_content_at_the_end() {
    let (card, icon, content) = (GlobalKey::new(), GlobalKey::new(), GlobalKey::new());
    let keys = (card.clone(), icon.clone(), content.clone());
    let fixture = mounted(600.0, move |_| {
        let (card, icon, content) = keys;
        keyed(
            &card,
            SettingsCard::new()
                .header(Text::new("Wi-Fi"))
                .description(Text::new("Known networks"))
                .header_icon(keyed(&icon, SizedBox::new().width(40.0).height(40.0)))
                .content(control(&content)),
        )
    });

    // The card takes the width it is given and `SettingsCardMinHeight`.
    let card = bounds_of(&fixture, &card).unwrap();
    assert_eq!(
        (card.width(), card.height()),
        (600.0, SETTINGS_CARD_MIN_HEIGHT)
    );

    // The 40 × 40 icon is scaled into 20 × 20 after the padding and its left margin, centred down the card.
    let icon = bounds_of(&fixture, &icon).unwrap();
    assert_eq!(icon.left, CARD_EDGE + 2.0);
    assert_eq!(
        icon.top + SETTINGS_CARD_HEADER_ICON_MAX_SIZE / 2.0,
        card.center().dy()
    );

    // The header starts after the icon's column, with the description under it at the same edge; the pair is centred down the card.
    let header = text_bounds(&fixture, "Wi-Fi");
    let description = text_bounds(&fixture, "Known networks");
    assert_eq!(header.left, CARD_EDGE + ICON_COLUMN);
    assert_eq!(description.left, header.left);
    assert_eq!(description.top, header.bottom);
    assert_eq!((header.top + description.bottom) / 2.0, card.center().dy());
    assert!(description.height() < header.height());

    // The content ends at the padding, centred down the card.
    let content = bounds_of(&fixture, &content).unwrap();
    assert_eq!(content.right, 600.0 - CARD_EDGE);
    assert_eq!(content.center().dy(), card.center().dy());
    assert_eq!((content.width(), content.height()), (60.0, 32.0));
}

#[test]
fn settings_card_wraps_a_long_description_and_grows() {
    let card = GlobalKey::new();
    let content = GlobalKey::new();
    let keys = (card.clone(), content.clone());
    let long = "Windows features such as Settings and File Explorer appear in this language, and apps that support it follow.";
    let fixture = mounted(500.0, move |_| {
        let (card, content) = keys;
        keyed(
            &card,
            SettingsCard::new()
                .header(Text::new("Display language"))
                .description(Text::new(long))
                .content(control(&content)),
        )
    });
    let card = bounds_of(&fixture, &card).unwrap();
    let description = text_bounds(&fixture, long);
    let content = bounds_of(&fixture, &content).unwrap();
    // The description keeps `HeaderPanel`'s 24 from the content and takes more than one line, which the card grows to hold inside its padding.
    assert!(description.right <= content.left - 24.0);
    assert!(description.height() > 2.0 * 12.0);
    assert!(card.height() > SETTINGS_CARD_MIN_HEIGHT);
    assert_eq!(description.bottom, card.bottom - CARD_EDGE);
}

/// A card `width` wide with an icon, a header and a control; where each ended up.
fn card_at(width: f64) -> (Option<Bounds>, Bounds, Bounds) {
    let (icon, content) = (GlobalKey::new(), GlobalKey::new());
    let keys = (icon.clone(), content.clone());
    let fixture = mounted(width, move |_| {
        let (icon, content) = keys;
        SettingsCard::new()
            .header(Text::new("Language"))
            .header_icon(keyed(&icon, SizedBox::new().width(20.0).height(20.0)))
            .content(control(&content))
            .into_widget()
    });
    (
        bounds_of(&fixture, &icon),
        text_bounds(&fixture, "Language"),
        bounds_of(&fixture, &content).unwrap(),
    )
}

#[test]
fn settings_card_wraps_its_content_under_the_wrap_thresholds() {
    // At `SettingsCardWrapThreshold` the content is still beside the header, at the end.
    let (icon, header, content) = card_at(SETTINGS_CARD_WRAP_THRESHOLD);
    assert!(icon.is_some());
    assert!(content.top < header.bottom && content.left > header.right);
    assert_eq!(content.right, SETTINGS_CARD_WRAP_THRESHOLD - CARD_EDGE);

    // One pixel narrower it is at the header's edge, `SettingsCardVerticalHeaderContentSpacing` under the first row, which is as high as the 20 icon; the icon stays.
    let (icon, header, content) = card_at(SETTINGS_CARD_WRAP_THRESHOLD - 1.0);
    assert!(icon.is_some());
    assert_eq!(header.left, CARD_EDGE + ICON_COLUMN);
    assert_eq!(content.left, header.left);
    assert!(content.top > header.bottom);
    assert_eq!(
        content.top,
        CARD_EDGE
            + SETTINGS_CARD_HEADER_ICON_MAX_SIZE
            + SETTINGS_CARD_VERTICAL_HEADER_CONTENT_SPACING
    );

    // At `SettingsCardWrapNoIconThreshold` the icon is still there; one pixel narrower it is gone and the header starts at the padding.
    let (icon, _, _) = card_at(SETTINGS_CARD_WRAP_NO_ICON_THRESHOLD);
    assert!(icon.is_some());
    let (icon, header, content) = card_at(SETTINGS_CARD_WRAP_NO_ICON_THRESHOLD - 1.0);
    assert!(icon.is_none());
    assert_eq!(header.left, CARD_EDGE);
    assert_eq!(content.left, CARD_EDGE);
}

#[test]
fn settings_card_content_alignments_place_the_content() {
    let (left, vertical, stretched) = (GlobalKey::new(), GlobalKey::new(), GlobalKey::new());
    let keys = (left.clone(), vertical.clone(), stretched.clone());
    let fixture = mounted(600.0, move |_| {
        let (left, vertical, stretched) = keys;
        Column::new()
            .main_axis_size(MainAxisSize::Min)
            .children(vec![
                SettingsCard::new()
                    .header(Text::new("Hidden header"))
                    .content_alignment(ContentAlignment::Left)
                    .content(control(&left))
                    .into_widget(),
                SettingsCard::new()
                    .header(Text::new("Shown header"))
                    .content_alignment(ContentAlignment::Vertical)
                    .content(control(&vertical))
                    .into_widget(),
                SettingsCard::new()
                    .header(Text::new("Stretched header"))
                    .content_alignment(ContentAlignment::Vertical)
                    .horizontal_content_alignment(HorizontalAlignment::Stretch)
                    .content(keyed(&stretched, SizedBox::new().height(32.0)))
                    .into_widget(),
            ])
            .into_widget()
    });
    // `Left`: the content at the start, and no header.
    assert_eq!(bounds_of(&fixture, &left).unwrap().left, CARD_EDGE);
    assert_eq!(label_count(&fixture, "Hidden header"), 0);
    // `Vertical`: under the header, at the end as the style's `HorizontalContentAlignment` says.
    let header = text_bounds(&fixture, "Shown header");
    let content = bounds_of(&fixture, &vertical).unwrap();
    assert_eq!(
        content.top,
        header.bottom + SETTINGS_CARD_VERTICAL_HEADER_CONTENT_SPACING
    );
    assert_eq!(content.right, 600.0 - CARD_EDGE);
    // `Stretch`: across the card's padding box.
    let content = bounds_of(&fixture, &stretched).unwrap();
    assert_eq!(
        (content.left, content.right),
        (CARD_EDGE, 600.0 - CARD_EDGE)
    );
}

#[test]
fn settings_card_clicks_only_while_click_is_enabled() {
    let clicks = Rc::new(Cell::new(0));
    let (action, hidden) = (GlobalKey::new(), GlobalKey::new());
    let keys = (action.clone(), hidden.clone());
    let counted = clicks.clone();
    let mut fixture = mounted(600.0, move |_| {
        let (action, hidden) = keys;
        let click = move || {
            let counted = counted.clone();
            Listener::new(move |_| counted.set(counted.get() + 1))
        };
        Column::new()
            .main_axis_size(MainAxisSize::Min)
            .children(vec![
                SettingsCard::new()
                    .header(Text::new("Plain"))
                    .action_icon(keyed(&hidden, SizedBox::new().width(13.0).height(13.0)))
                    .click(click())
                    .into_widget(),
                SettingsCard::new()
                    .header(Text::new("Clickable"))
                    .is_click_enabled(true)
                    .action_icon(keyed(&action, SizedBox::new().width(13.0).height(13.0)))
                    .click(click())
                    .into_widget(),
                SettingsCard::new()
                    .header(Text::new("No icon"))
                    .is_click_enabled(true)
                    .is_action_icon_visible(false)
                    .click(click())
                    .into_widget(),
                SettingsCard::new()
                    .header(Text::new("Disabled"))
                    .is_click_enabled(true)
                    .is_enabled(false)
                    .click(click())
                    .into_widget(),
            ])
            .into_widget()
    });

    // The pointer over a card that can be clicked changes its background; over a plain one it does not.
    let header = text_bounds(&fixture, "Clickable");
    let plain = Offset::new(300.0, text_bounds(&fixture, "Plain").center().dy());
    let clickable = Offset::new(300.0, header.center().dy());
    let (plain_before, clickable_before) = (pixel(&fixture, plain), pixel(&fixture, clickable));
    fixture.send_mouse(PointerChange::Hover, plain, 0);
    fixture.pump();
    assert_eq!(pixel(&fixture, plain), plain_before);
    fixture.send_mouse(PointerChange::Hover, clickable, 0);
    fixture.pump();
    assert_ne!(pixel(&fixture, clickable), clickable_before);
    fixture.send_mouse(PointerChange::Hover, Offset::new(700.0, 500.0), 0);
    fixture.pump();

    // A card that cannot be clicked raises no `Click` and shows no action icon.
    fixture.tap("Plain");
    assert_eq!(clicks.get(), 0);
    assert!(bounds_of(&fixture, &hidden).is_none());

    // One that can be clicks anywhere on it; its action icon ends at the padding, centred down the card.
    fixture.tap("Clickable");
    assert_eq!(clicks.get(), 1);
    let icon = bounds_of(&fixture, &action).unwrap();
    assert_eq!(icon.right, 600.0 - CARD_EDGE);
    assert_eq!(icon.width(), SETTINGS_CARD_ACTION_ICON_MAX_SIZE);
    assert_eq!(icon.center().dy(), header.center().dy());
    let end = Offset::new(540.0, header.center().dy());
    fixture.send(PointerChange::Down, end);
    fixture.send(PointerChange::Up, end);
    fixture.pump();
    assert_eq!(clicks.get(), 2);

    fixture.tap("No icon");
    assert_eq!(clicks.get(), 3);
    fixture.tap("Disabled");
    assert_eq!(clicks.get(), 3);
}

#[test]
fn right_aligned_compact_toggle_switch_has_its_label_before_the_knob_and_no_minimum_width() {
    let toggled = Rc::new(Cell::new(None));
    let (default, compact, card) = (GlobalKey::new(), GlobalKey::new(), GlobalKey::new());
    let keys = (default.clone(), compact.clone(), card.clone());
    let asked = toggled.clone();
    let mut fixture = mounted(600.0, move |_| {
        let (default, compact, card) = keys;
        Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .children(vec![
                keyed(
                    &default,
                    ToggleSwitch::new(false, |_, _| {}).off_content(Text::new("Default")),
                ),
                keyed(
                    &compact,
                    ToggleSwitch::new(false, move |_, on| asked.set(Some(on)))
                        .off_content(Text::new("Compact"))
                        .template(right_aligned_compact_toggle_switch_style),
                ),
                SettingsCard::new()
                    .header(Text::new("In a card"))
                    .content(keyed(
                        &card,
                        ToggleSwitch::new(true, |_, _| {})
                            .on_content(Text::new("Carded"))
                            .template(right_aligned_compact_toggle_switch_style),
                    ))
                    .into_widget(),
            ])
            .into_widget()
    });

    // The default style: the label after the switch and its gap, in at least `ToggleSwitchThemeMinWidth`.
    let default = bounds_of(&fixture, &default).unwrap();
    let label = text_bounds(&fixture, "Default");
    assert_eq!(default.width(), 154.0);
    assert_eq!(label.left, default.left + TRACK_WIDTH + CONTENT_GAP);

    // The Toolkit's style: the label, the gap, then the switch, and nothing more; 36 high.
    let compact = bounds_of(&fixture, &compact).unwrap();
    let label = text_bounds(&fixture, "Compact");
    assert_eq!(label.left, compact.left);
    assert_eq!(compact.width(), label.width() + CONTENT_GAP + TRACK_WIDTH);
    assert_eq!(compact.height(), RIGHT_ALIGNED_COMPACT_TOGGLE_SWITCH_HEIGHT);
    assert_eq!(label.center().dy(), compact.center().dy());

    // A tap on the label toggles; so does dragging the knob, at the end of the control, past half its track.
    fixture.tap("Compact");
    assert_eq!(toggled.take(), Some(true));
    let knob = Offset::new(
        compact.right - TRACK_WIDTH + KNOB_AREA / 2.0,
        compact.center().dy(),
    );
    drag(&mut fixture, knob, &[8.0, 8.0, 8.0, 6.0]);
    assert_eq!(toggled.take(), Some(true));
    drag(&mut fixture, knob, &[-20.0, -2.0, 18.0, 2.0]);
    assert_eq!(toggled.take(), None);

    // In a card the switch ends where the card's content does.
    let card = bounds_of(&fixture, &card).unwrap();
    assert_eq!(card.right, 600.0 - CARD_EDGE);
    assert_eq!(text_bounds(&fixture, "Carded").left, card.left);
}

#[test]
fn settings_expander_shows_its_items_as_rows_under_the_header() {
    let expanded = Rc::new(Cell::new(None));
    let (expander, content, remove, plain, footer) = (
        GlobalKey::new(),
        GlobalKey::new(),
        GlobalKey::new(),
        GlobalKey::new(),
        GlobalKey::new(),
    );
    let keys = (
        expander.clone(),
        content.clone(),
        remove.clone(),
        plain.clone(),
        footer.clone(),
    );
    let asked = expanded.clone();
    let mut fixture = mounted(600.0, move |_| {
        let (expander, content, remove, plain, footer) = keys;
        keyed(
            &expander,
            SettingsExpander::new(true, move |_, value| asked.set(Some(value)))
                .header(Text::new("Sites"))
                .description(Text::new("Allowed to open links"))
                .header_icon(FontIcon::symbol(FluentSymbol::Link))
                .content(control(&content))
                .items(vec![
                    SettingsCard::new()
                        .header(Text::new("example.com"))
                        .content(control(&remove))
                        .into_widget(),
                    SettingsCard::new()
                        .header(Text::new("Advanced"))
                        .is_click_enabled(true)
                        .into_widget(),
                    keyed(&plain, SizedBox::new().height(10.0)),
                ])
                .items_footer(keyed(&footer, SizedBox::new().height(20.0))),
        )
    });

    // The header is a settings card: icon, header and description, then the content and the 32 chevron with its 8 after it, inside the header's padding of 4.
    let header = text_bounds(&fixture, "Sites");
    assert_eq!(header.left, CARD_EDGE + ICON_COLUMN);
    let content = bounds_of(&fixture, &content).unwrap();
    assert_eq!(content.right, 600.0 - 1.0 - 8.0 - 32.0 - 4.0);

    // The header is its card's 68 inside the toggle button's border of 1. The first row overlaps the header's bottom edge by 2 and is 52 high, its own top border included; its header lines up with the expander's, and its content ends where the expander's does.
    let rows_top = SETTINGS_CARD_MIN_HEIGHT + 2.0 - 2.0;
    let row = text_bounds(&fixture, "example.com");
    assert_eq!(row.left, 1.0 + SETTINGS_EXPANDER_ITEM_PADDING[0]);
    assert_eq!(row.left, header.left);
    assert_eq!(
        row.center().dy(),
        rows_top + 0.5 + SETTINGS_EXPANDER_ITEM_MIN_HEIGHT / 2.0
    );
    let remove = bounds_of(&fixture, &remove).unwrap();
    assert_eq!(remove.right, content.right);

    // A row that can be clicked keeps only 16 at its end, for its action icon; rows follow one another.
    let advanced = text_bounds(&fixture, "Advanced");
    assert_eq!(
        advanced.center().dy(),
        row.center().dy() + SETTINGS_EXPANDER_ITEM_MIN_HEIGHT
    );

    // A row that is no settings card shows as it is, across the expander; the footer follows the rows.
    let plain = bounds_of(&fixture, &plain).unwrap();
    assert_eq!((plain.left, plain.right), (1.0, 599.0));
    assert_eq!(
        plain.top,
        rows_top + 2.0 * SETTINGS_EXPANDER_ITEM_MIN_HEIGHT
    );
    let footer = bounds_of(&fixture, &footer).unwrap();
    assert_eq!(footer.top, plain.bottom);
    let expander = bounds_of(&fixture, &expander).unwrap();
    assert_eq!(expander.bottom, footer.bottom + 1.0);
    assert_eq!(expander.width(), 600.0);

    // A click on the header asks to collapse.
    fixture.tap("Sites");
    assert_eq!(expanded.take(), Some(false));
}

#[test]
fn settings_card_page_changes_its_settings() {
    let mut fixture = Fixture::for_feature([1080, 1700], Feature::SettingsCard);
    fixture.find("Opened 0 times");
    fixture.tap("Notifications");
    fixture.find("Opened 1 times");

    // The Wi-Fi card's switch is on; a tap on its label turns it off.
    fixture.find("On");
    fixture.tap("On");
    fixture.pump();
    fixture.capture("gallery_settings_card_light");

    fixture.tap("Dark theme");
    fixture.find("Light theme");
    fixture.capture("gallery_settings_card_dark");
}

#[test]
fn settings_expander_page_removes_rows_and_collapses() {
    let mut fixture = Fixture::for_feature([1080, 900], Feature::SettingsExpander);
    fixture.pump();
    fixture.find("example.com");
    fixture.find("waft.rs");
    fixture.capture("gallery_settings_expander_light");

    // Removing the first row leaves the second.
    assert!(label_count(&fixture, "example.com") > 0);
    let remove = text_bounds(&fixture, "Remove").center();
    fixture.send(PointerChange::Down, remove);
    fixture.send(PointerChange::Up, remove);
    fixture.pump();
    assert_eq!(label_count(&fixture, "example.com"), 0);
    fixture.find("waft.rs");

    // The header collapses the rows.
    let before = text_bounds(&fixture, "Disabled").top;
    fixture.tap("Sites that may open links");
    fixture.pump();
    fixture.pump();
    assert!(text_bounds(&fixture, "Disabled").top < before);
    fixture.capture("gallery_settings_expander_collapsed");

    fixture.tap("Dark theme");
    fixture.find("Light theme");
    fixture.capture("gallery_settings_expander_dark");
}
