//! CSS selectors over the element tree, matched by Servo's `selectors`: how the driver names the
//! widget a command acts on, as a web page's elements are named.
//!
//! A selector's words mean, for an element:
//!
//! - a type selector (`IconButton`): its widget's type;
//! - an id (`#save`): its widget's key, a `ValueKey` of text or a whole number, as
//!   flutter_driver's `ByValueKey` takes one;
//! - an attribute (`[tooltip="Back"]`): one of its widget's diagnostics properties, by name, as
//!   `cargo waft ext tree <id>` prints it; and `text`, the text it shows, as `find.text` reads it;
//! - the tree: the element tree, its parents, children and siblings, framework widgets included.
//!
//! Classes match nothing, as widgets have none; pseudo-classes other than the tree's own
//! (`:nth-child`, `:has`, `:not`, …) and pseudo-elements are refused when parsed.

use std::fmt;

use cssparser::ToCss;
use precomputed_hash::PrecomputedHash;
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::matching::{
    ElementSelectorFlags, MatchingContext, MatchingForInvalidation, MatchingMode,
    NeedsSelectorFlags, QuirksMode, SelectorCaches,
};
use selectors::parser::{self, ParseRelative, SelectorList, SelectorParseErrorKind};
use selectors::{Element, OpaqueElement};
use waft_foundation::{App, DiagnosticPropertiesBuilder, Key, ValueKey};

use crate::AnyElement;
use crate::widgets::text::shown_text;

/// A parsed selector, matched against the elements of a tree.
pub(crate) struct WidgetSelector {
    selectors: SelectorList<WidgetSelectorImpl>,
}

impl WidgetSelector {
    /// `css` parsed, or why it is not a selector the driver matches.
    pub(crate) fn parse(css: &str) -> Result<WidgetSelector, String> {
        let mut parser = cssparser::Parser::new(css);
        SelectorList::parse(&WidgetSelectorParser, &mut parser, ParseRelative::No)
            .map(|selectors| WidgetSelector { selectors })
            .map_err(|error| {
                format!(
                    "`{css}` is not a selector the driver matches: {:?}",
                    error.kind
                )
            })
    }

    /// The onstage elements under `root`, `root` included, that the selector matches, in the
    /// tree's order: what a finder skipping offstage widgets finds.
    pub(crate) fn select(&self, app: &App, root: AnyElement) -> Vec<AnyElement> {
        let mut caches = SelectorCaches::default();
        let mut context = MatchingContext::new(
            MatchingMode::Normal,
            None,
            &mut caches,
            QuirksMode::NoQuirks,
            NeedsSelectorFlags::No,
            MatchingForInvalidation::No,
        );
        std::iter::once(root)
            .chain(root.descendants(app, true))
            .filter(|&element| {
                let element = WidgetElement { app, element };
                self.selectors.slice().iter().any(|selector| {
                    selectors::matching::matches_selector(selector, 0, None, element, &mut context)
                })
            })
            .collect()
    }
}

/// The selector types: names are text, and there are no pseudo-classes or elements of the
/// driver's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WidgetSelectorImpl;

impl parser::SelectorImpl for WidgetSelectorImpl {
    type ExtraMatchingData<'a> = ();
    type AttrValue = CssText;
    type Identifier = CssText;
    type LocalName = CssText;
    type NamespacePrefix = CssText;
    type NamespaceUrl = CssText;
    type BorrowedNamespaceUrl = CssText;
    type BorrowedLocalName = CssText;
    type NonTSPseudoClass = NoPseudoClass;
    type PseudoElement = NoPseudoElement;
}

/// Parses the whole of CSS's selectors, `:has()`, `:is()` and `:where()` included.
struct WidgetSelectorParser;

impl<'i> parser::Parser<'i> for WidgetSelectorParser {
    type Impl = WidgetSelectorImpl;
    type Error = SelectorParseErrorKind;

    fn parse_has(&self) -> bool {
        true
    }

    fn parse_is_and_where(&self) -> bool {
        true
    }
}

/// A name or a value in a selector.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CssText(String);

impl From<&str> for CssText {
    fn from(text: &str) -> CssText {
        CssText(text.to_owned())
    }
}

impl AsRef<str> for CssText {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl ToCss for CssText {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str(&self.0)
    }
}

impl PrecomputedHash for CssText {
    /// The djb2 hash: `selectors` keeps names in a filter by it.
    fn precomputed_hash(&self) -> u32 {
        self.0.bytes().fold(5381_u32, |hash, byte| {
            hash.wrapping_mul(33) ^ u32::from(byte)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoPseudoClass {}

impl parser::NonTSPseudoClass for NoPseudoClass {
    fn is_active_or_hover(&self) -> bool {
        match *self {}
    }

    fn is_user_action_state(&self) -> bool {
        match *self {}
    }
}

impl ToCss for NoPseudoClass {
    fn to_css<W: fmt::Write>(&self, _dest: &mut W) -> fmt::Result {
        match *self {}
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoPseudoElement {}

impl parser::PseudoElement for NoPseudoElement {}

impl ToCss for NoPseudoElement {
    fn to_css<W: fmt::Write>(&self, _dest: &mut W) -> fmt::Result {
        match *self {}
    }
}

/// An element as `selectors` walks the tree.
#[derive(Clone, Copy)]
struct WidgetElement<'a> {
    app: &'a App,
    element: AnyElement,
}

impl fmt::Debug for WidgetElement<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.element.to_string_short(self.app))
    }
}

impl<'a> WidgetElement<'a> {
    fn with(self, element: AnyElement) -> WidgetElement<'a> {
        WidgetElement { element, ..self }
    }

    fn children(self) -> Vec<AnyElement> {
        let mut children = Vec::new();
        self.element
            .visit_children(self.app, &mut |child| children.push(child));
        children
    }

    /// The element `step` places from this one among its parent's children.
    fn sibling(self, step: isize) -> Option<WidgetElement<'a>> {
        let siblings = self.with(self.element.parent(self.app)?).children();
        let at = siblings
            .iter()
            .position(|sibling| sibling.id() == self.element.id())?;
        let sibling = siblings.get(at.checked_add_signed(step)?)?;
        Some(self.with(*sibling))
    }

    fn type_name(self) -> &'static str {
        self.element.widget(self.app).type_name()
    }

    /// The value of the attribute `name`: the text the widget shows for `text`, else the
    /// widget's diagnostics property of that name, as its description.
    fn attribute(self, name: &str) -> Option<String> {
        if name == "text" {
            return shown_text(self.app, self.element);
        }
        let mut properties = DiagnosticPropertiesBuilder::new();
        self.element
            .widget(self.app)
            .debug_fill_properties(self.app, &mut properties);
        properties
            .properties
            .iter()
            .find(|property| property.name() == Some(name))
            .map(|property| property.to_description(None))
    }
}

/// A key's value, as flutter_driver's `ByValueKey` takes one: a `ValueKey` of text or of a whole
/// number.
fn key_value(key: &dyn Key) -> Option<String> {
    let key = key as &dyn std::any::Any;
    macro_rules! value_of {
        ($($value:ty),*) => {
            $(if let Some(key) = key.downcast_ref::<ValueKey<$value>>() {
                return Some(key.value.to_string());
            })*
        };
    }
    value_of!(String, &'static str, i32, i64, u32, u64, usize);
    None
}

impl Element for WidgetElement<'_> {
    type Impl = WidgetSelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(self.element.data(self.app))
    }

    fn parent_element(&self) -> Option<Self> {
        self.element
            .parent(self.app)
            .map(|parent| self.with(parent))
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.sibling(-1)
    }

    fn next_sibling_element(&self) -> Option<Self> {
        self.sibling(1)
    }

    fn first_element_child(&self) -> Option<Self> {
        self.children().first().map(|child| self.with(*child))
    }

    fn is_html_element_in_html_document(&self) -> bool {
        // Type names are matched as written, not lowered as an HTML document's are.
        false
    }

    fn has_local_name(&self, name: &CssText) -> bool {
        self.type_name() == name.0
    }

    fn has_namespace(&self, _namespace: &CssText) -> bool {
        true
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.type_name() == other.type_name()
    }

    fn attr_matches(
        &self,
        _namespace: &NamespaceConstraint<&CssText>,
        name: &CssText,
        operation: &AttrSelectorOperation<&CssText>,
    ) -> bool {
        self.attribute(&name.0)
            .is_some_and(|value| operation.eval_str(&value))
    }

    fn match_non_ts_pseudo_class(
        &self,
        pseudo_class: &NoPseudoClass,
        _context: &mut MatchingContext<'_, WidgetSelectorImpl>,
    ) -> bool {
        match *pseudo_class {}
    }

    fn match_pseudo_element(
        &self,
        pseudo_element: &NoPseudoElement,
        _context: &mut MatchingContext<'_, WidgetSelectorImpl>,
    ) -> bool {
        match *pseudo_element {}
    }

    fn apply_selector_flags(&self, _flags: ElementSelectorFlags) {}

    fn is_link(&self) -> bool {
        false
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(&self, id: &CssText, case_sensitivity: CaseSensitivity) -> bool {
        self.element
            .widget(self.app)
            .key()
            .and_then(|key| key_value(key.key()))
            .is_some_and(|value| case_sensitivity.eq(value.as_bytes(), id.0.as_bytes()))
    }

    fn has_class(&self, _name: &CssText, _case_sensitivity: CaseSensitivity) -> bool {
        false
    }

    fn has_custom_state(&self, _name: &CssText) -> bool {
        false
    }

    fn imported_part(&self, _name: &CssText) -> Option<CssText> {
        None
    }

    fn is_part(&self, _name: &CssText) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        self.children().is_empty()
    }

    fn is_root(&self) -> bool {
        self.element.parent(self.app).is_none()
    }

    fn add_element_unique_hashes(&self, _filter: &mut BloomFilter) -> bool {
        false
    }
}
