//! The Windows Community Toolkit's `SettingsCard` and `SettingsExpander`: settings as cards, with their control at the end, a click through, or rows under them.

use crate::{column, example, section};
use waft_foundation::{App, Handle, Listener, Ref};
use waft_rendering::BoxConstraints;
use waft_widgets::*;
use waft_winui::*;

/// Builds the settings card examples.
pub fn settings_card(resources: &ThemeResources) -> Vec<WidgetRef> {
    section("SettingsCard", resources, SettingsCardDemo.into_widget())
}

/// Builds the settings expander examples.
pub fn settings_expander(resources: &ThemeResources) -> Vec<WidgetRef> {
    section(
        "SettingsExpander",
        resources,
        SettingsExpanderDemo.into_widget(),
    )
}

/// The languages the drop-down cards offer.
const LANGUAGES: [&str; 3] = ["English", "Deutsch", "Français"];

/// A drop-down of [`LANGUAGES`] at the width a settings card gives its content.
fn language_box(selected: usize, changed: impl Fn(&mut App, usize) + 'static) -> WidgetRef {
    let items = LANGUAGES
        .iter()
        .enumerate()
        .map(|(index, language)| ComboBoxItem::text(index.to_string(), *language))
        .collect();
    ConstrainedBox::new(BoxConstraints::new().min_width(SETTINGS_CARD_CONTENT_MIN_WIDTH))
        .child(ComboBox::new(items, Some(selected), move |app, args| {
            if let Some(index) = args.selected_index() {
                changed(app, index);
            }
        }))
        .into_widget()
}

/// Cards with each kind of content, a card to click, and the content alignments.
#[derive(Debug)]
struct SettingsCardDemo;

/// The settings the cards change.
struct SettingsCardDemoState {
    state: StateData<SettingsCardDemo>,
    /// Whether the switch card is on.
    wifi: bool,
    /// The drop-down card's language.
    language: usize,
    /// Whether the left-aligned card's box is checked.
    metered: bool,
    /// How often the card to click was clicked.
    clicks: usize,
    /// The text of the vertical card's field.
    name: Option<Ref<TextEditingController>>,
}

impl StatefulWidget for SettingsCardDemo {
    type State = SettingsCardDemoState;

    fn create_state(&self) -> Self::State {
        SettingsCardDemoState {
            state: StateData::new(),
            wifi: true,
            language: 0,
            metered: false,
            clicks: 0,
            name: None,
        }
    }
}

impl State for SettingsCardDemoState {
    type Widget = SettingsCardDemo;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let name = TextEditingController::new(app).text(app, "Surface");
        app.get_mut(self).name = Some(name);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(name) = app.get_mut(self).name.take() {
            name.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let state = app.get(self);
        let (wifi, language, metered, clicks) =
            (state.wifi, state.language, state.metered, state.clicks);
        let name = state.name.clone().unwrap();

        let cards = column(
            vec![
                SettingsCard::new()
                    .header(Text::new("Wi-Fi"))
                    .description(Text::new("Connect to known networks in range"))
                    .header_icon(FontIcon::symbol(FluentSymbol::Globe))
                    .content(
                        ToggleSwitch::new(wifi, move |app, on| {
                            self.set_state(app, |state| state.wifi = on)
                        })
                        .template(right_aligned_compact_toggle_switch_style),
                    )
                    .into_widget(),
                SettingsCard::new()
                    .header(Text::new("Display language"))
                    .description(Text::new(
                        "Windows features such as Settings and File Explorer appear in this language, and apps that support it follow; signing out may be needed before every part of the system has changed.",
                    ))
                    .header_icon(FontIcon::symbol(FluentSymbol::TextFont))
                    .content(language_box(language, move |app, index| {
                        self.set_state(app, |state| state.language = index)
                    }))
                    .into_widget(),
                SettingsCard::new()
                    .header(Text::new("Storage"))
                    .description(Text::new("Free up space on this device"))
                    .header_icon(FontIcon::symbol(FluentSymbol::Folder))
                    .content(Button::text("Clean up", Listener::new(|_| {})))
                    .into_widget(),
                SettingsCard::new()
                    .header(Text::new("Unavailable"))
                    .description(Text::new("A disabled card dims its text and icon"))
                    .header_icon(FontIcon::symbol(FluentSymbol::Shield))
                    .content(ToggleSwitch::new(false, |_, _| {}).is_enabled(false).template(
                        right_aligned_compact_toggle_switch_style,
                    ))
                    .is_enabled(false)
                    .into_widget(),
            ],
            4.0,
        );

        let clickable = column(
            vec![
                SettingsCard::new()
                    .header(Text::new("Notifications"))
                    .description(Text::new(format!("Opened {clicks} times")))
                    .header_icon(FontIcon::symbol(FluentSymbol::Alert))
                    .is_click_enabled(true)
                    .action_icon_tool_tip("Open notifications")
                    .click(Listener::new(move |app| {
                        self.set_state(app, |state| state.clicks += 1)
                    }))
                    .into_widget(),
                SettingsCard::new()
                    .header(Text::new("About"))
                    .description(Text::new("A card to click without its action icon"))
                    .is_click_enabled(true)
                    .is_action_icon_visible(false)
                    .click(Listener::new(|_| {}))
                    .into_widget(),
            ],
            4.0,
        );

        let alignments = column(
            vec![
                SettingsCard::new()
                    .content_alignment(ContentAlignment::Left)
                    .content(
                        CheckBox::new(Some(metered), move |app, checked| {
                            self.set_state(app, |state| state.metered = checked == Some(true))
                        })
                        .content(Text::new("Metered connection")),
                    )
                    .into_widget(),
                SettingsCard::new()
                    .header(Text::new("Device name"))
                    .description(Text::new(
                        "The content is under the header, across the card",
                    ))
                    .content_alignment(ContentAlignment::Vertical)
                    .horizontal_content_alignment(HorizontalAlignment::Stretch)
                    .content(TextBox::new(name.erase()))
                    .into_widget(),
            ],
            4.0,
        );

        let narrow = |width: f64| {
            SizedBox::new()
                .width(width)
                .child(
                    SettingsCard::new()
                        .header(Text::new("Language"))
                        .description(Text::new(format!("A card {width} wide")))
                        .header_icon(FontIcon::symbol(FluentSymbol::TextFont))
                        .content(language_box(language, move |app, index| {
                            self.set_state(app, |state| state.language = index)
                        })),
                )
                .into_widget()
        };

        column(
            vec![
                example(
                    "A setting and its control",
                    "The header, description and icon are at the start; the control is at the end. A long description wraps.",
                    &resources,
                    cards,
                ),
                example(
                    "A card to click",
                    "The whole card is a button, marked by its action icon.",
                    &resources,
                    clickable,
                ),
                example(
                    "Content alignment",
                    "Left shows only the content; Vertical puts it under the header.",
                    &resources,
                    alignments,
                ),
                example(
                    "Narrow cards",
                    "Under 476 wide the content moves under the header; under 286 the icon goes too.",
                    &resources,
                    column(vec![narrow(400.0), narrow(260.0)], 4.0),
                ),
            ],
            16.0,
        )
    }
}

/// An expander whose rows are a list to remove from and a row to add with.
#[derive(Debug)]
struct SettingsExpanderDemo;

/// The expander's state and its rows.
struct SettingsExpanderDemoState {
    state: StateData<SettingsExpanderDemo>,
    /// Whether the rows show.
    expanded: bool,
    /// The header card's language.
    language: usize,
    /// The sites the rows name.
    sites: Vec<String>,
    /// The text of the row to add with.
    site: Option<Ref<TextEditingController>>,
}

impl StatefulWidget for SettingsExpanderDemo {
    type State = SettingsExpanderDemoState;

    fn create_state(&self) -> Self::State {
        SettingsExpanderDemoState {
            state: StateData::new(),
            expanded: true,
            language: 0,
            sites: vec!["example.com".to_string(), "waft.rs".to_string()],
            site: None,
        }
    }
}

impl State for SettingsExpanderDemoState {
    type Widget = SettingsExpanderDemo;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let site = TextEditingController::new(app);
        app.get_mut(self).site = Some(site);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(site) = app.get_mut(self).site.take() {
            site.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let state = app.get(self);
        let (expanded, language) = (state.expanded, state.language);
        let sites = state.sites.clone();
        let site = state.site.clone().unwrap();

        let mut items: Vec<WidgetRef> = sites
            .iter()
            .enumerate()
            .map(|(index, name)| {
                SettingsCard::new()
                    .header(Text::new(name.clone()))
                    .content(Button::text(
                        "Remove",
                        Listener::new(move |app| {
                            self.set_state(app, |state| {
                                state.sites.remove(index);
                            })
                        }),
                    ))
                    .into_widget()
            })
            .collect();
        let field = site.clone();
        items.push(
            SettingsCard::new()
                .content_alignment(ContentAlignment::Vertical)
                .horizontal_content_alignment(HorizontalAlignment::Stretch)
                .content(Row::new().spacing(8.0).children(vec![
                        Expanded::new(TextBox::new(site.erase()).placeholder_text("Add a site"))
                            .into_widget(),
                        Button::text(
                            "Add",
                            Listener::new(move |app| {
                                let name = field.text_value(app).to_string();
                                if !name.is_empty() {
                                    field.handle().clear(app);
                                    self.set_state(app, |state| state.sites.push(name));
                                }
                            }),
                        )
                        .into_widget(),
                    ]))
                .into_widget(),
        );
        items.push(
            SettingsCard::new()
                .header(Text::new("Advanced"))
                .is_click_enabled(true)
                .click(Listener::new(|_| {}))
                .into_widget(),
        );

        column(
            vec![
                example(
                    "Rows under a card",
                    "The chevron shows the rows. Each is a settings card in the expander's item style.",
                    &resources,
                    SettingsExpander::new(expanded, move |app, value| {
                        self.set_state(app, |state| state.expanded = value)
                    })
                    .header(Text::new("Sites that may open links"))
                    .description(Text::new("Links from these sites open without asking"))
                    .header_icon(FontIcon::symbol(FluentSymbol::Link))
                    .content(language_box(language, move |app, index| {
                        self.set_state(app, |state| state.language = index)
                    }))
                    .items(items)
                    .into_widget(),
                ),
                example(
                    "Disabled",
                    "A disabled expander cannot be opened.",
                    &resources,
                    SettingsExpander::new(false, |_, _| {})
                        .header(Text::new("Managed by your organisation"))
                        .description(Text::new("These rows cannot be shown"))
                        .header_icon(FontIcon::symbol(FluentSymbol::LockClosed))
                        .is_enabled(false)
                        .into_widget(),
                ),
            ],
            16.0,
        )
    }
}
