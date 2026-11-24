# waft-winui/src/community_toolkit
Audience: readers familiar with Flutter and Rust; the headings identify the corresponding Windows Community Toolkit code.

This folder is not ported from microsoft-ui-xaml as the rest of the kit is, but from the Windows Community Toolkit, whose controls Windows apps use beside WinUI's own: https://github.com/CommunityToolkit/Windows. A checkout goes under `.reference/community-toolkit-windows`.

Toolkit home: components/SettingsControls/src (`SettingsCard/`, `SettingsExpander/`, `Helpers/`)
Ported against: a2fa47ad

## mod.rs → shared conventions

- Change: `SettingsCard` and `SettingsExpander` take the whole of a bounded width, and a card picks its wrapped layouts from the width it is offered.
  Reason: framework — a XAML element stretches within the slot its panel gives it and the Toolkit's size triggers read the width it ended with, while a Flutter parent decides a child's width through constraints that are known before the card is built.
  Affect: Under `Center` or in a start-aligned `Column` a card still fills the width; where the width is unbounded, as in a `Row`, it is as wide as its content and keeps its content beside its header.

- Change: The Toolkit's `SettingsCard…` and `SettingsExpander…` theme resources are constants of this folder and the WinUI brushes they alias, rather than fields of `ThemeResources`.
  Reason: framework — `ThemeResources` is generated from the dictionaries of one source, microsoft-ui-xaml, and the Toolkit's dictionaries are a second one.
  Affect: A `ThemeScope` changes a card's colours through the WinUI brush behind them, such as `card_background_fill_color_default_brush`, which other controls share, and cannot change its sizes; `SettingsCard`'s `background`, `padding`, `min_height`, `border_thickness` and `corner_radius` change one card.

- Change: `SettingsCard`'s `action_icon_tool_tip` and `SettingsExpander`'s chevron show the kit's tooltip.
  Reason: framework — Waft shows popups in an `Overlay`, where WinUI's `ToolTipService` opens a popup of its own.
  Affect: A clickable card with a tooltip, and every `SettingsExpander`, needs an `Overlay` ancestor, which `FluentApp` supplies.

## settings_card.rs → `SettingsCard`

- Change: `SettingsCard` does not restyle the controls in its content.
  Reason: framework — the card's XAML style holds implicit styles that every `ToggleSwitch`, `Slider`, `ComboBox` and `TextBox` under it takes, and its description gives one to a `HyperlinkButton`, while the kit's resource scope carries values and no styles.
  Affect: A caller gives a switch in a card `.template(right_aligned_compact_toggle_switch_style)` and wraps a `Slider`, `ComboBox` or `TextBox` in a `ConstrainedBox` of `SETTINGS_CARD_CONTENT_MIN_WIDTH`; without them the switch keeps its label after it and its width of 154, and the others their own widths.

- Change: A disabled `SettingsCard` keeps pointer input and the focus from its content without changing how the content looks.
  Reason: framework — `IsEnabled` reaches every element under a XAML control, while each Waft control is given its own.
  Affect: A caller that disables a card also disables the control in it, which dims it.

- Change: `SettingsCard` shows its header icon in a box of 20 × 20 and its action icon in one of 13 × 13, each scaled to fit.
  Reason: framework — a XAML `Viewbox` takes the size of the child it scaled, and the kit scales with `FittedBox`, which fills the box it is given.
  Affect: A header icon taller than it is wide leaves the header where a square icon would, not nearer.

- Change: `SettingsCard`'s action icon is `FluentSymbol::ChevronRight` by default.
  Reason: os — the Toolkit's glyph, U+E974, is in the icon font that ships with Windows.
  Affect: The chevron's shape can differ from Windows'.

## right_aligned_compact_toggle_switch_style.rs → `RightAlignedCompactToggleSwitchStyle`

- Change: `right_aligned_compact_toggle_switch_style` is a function given to `ToggleSwitch::template`.
  Reason: language — Rust callbacks receive the switch's states and template parts in place of a keyed XAML `Style`.
  Affect: A caller writes `ToggleSwitch::new(..).template(right_aligned_compact_toggle_switch_style)`.

## settings_expander.rs → `SettingsExpander`

- Change: `SettingsExpander` asks the application to change whether its rows show, and reports `expanded` and `collapsed` after layout.
  Reason: framework — like `Expander`, which draws it, it receives a boolean and a callback rather than changing the supplied boolean itself.
  Affect: The application handles the callback and rebuilds with the new value before the rows open or close.

- Change: `SettingsExpander` takes its rows as `items`, a list of widgets, and has no `ItemsSource`, `ItemTemplate` or `ItemContainerStyleSelector`.
  Reason: framework — the Toolkit's `ItemsRepeater` makes an element for each object of a source through a template and only for rows near the viewport, while a Flutter parent is given its children.
  Affect: A caller maps its data to widgets, as in `.items(sites.iter().map(row))`, and every row is built whether or not it is on screen.

- Change: `SettingsExpander` gives its item style to a row that is itself a `SettingsCard`.
  Reason: framework — the Toolkit sets the style on the element the repeater prepared, while a Waft parent can restyle only a widget whose type it can name.
  Affect: A card wrapped in another widget, such as a `KeyedSubtree`, keeps the look of a card on its own; a card takes a key through `SettingsCard::key` instead.

- Change: `SettingsExpander` turns its chevron over 250 ms.
  Reason: os — Windows supplies the original animated icon, while the replacement font supplies only a static arrow that Waft rotates, as for `Expander`.
  Affect: The arrow points the right way, but its shape and movement differ from Windows.

- Change: A disabled `SettingsExpander` disables the rows that are `SettingsCard`s and leaves other rows, `items_header` and `items_footer` as they are.
  Reason: framework — `IsEnabled` reaches every element under a XAML control, while each Waft control is given its own.
  Affect: Rows of a disabled expander cannot show, since it cannot be opened; an application that disables an open expander also disables what it put in those rows.

## Deferred

- Implicit styles, by which a `SettingsCard` restyles the `ToggleSwitch`, `Slider`, `ComboBox` and `TextBox` in its content and the `HyperlinkButton` in its description (`StyleExtensions.Resources`), remain deferred. Trigger: The kit's resource scope carries styles by control type.
- Lightweight styling of the Toolkit's own keys, such as `SettingsCardBackground` and `SettingsCardWrapThreshold`, remains deferred. Trigger: The resource generator reads the Toolkit's dictionaries.
- `BitmapHeaderIconStates`, which shows a `BitmapIcon` header icon at 40% opacity on a disabled card, remains deferred. Trigger: The kit ports `BitmapIcon`.
- The controls' accessibility, `SettingsCardAutomationPeer`, `SettingsExpanderAutomationPeer` and the names the controls give their content, remains deferred. Trigger: The kit gains accessibility support.
- The high-contrast dictionary of the Toolkit's brushes remains deferred. Trigger: A host reports the system palette.
