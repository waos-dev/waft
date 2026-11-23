//! An element's diagnostics node: Dart's `_ElementDiagnosticableTreeNode`, which adds to a tree
//! node's JSON the element's widget type and whether it is stateful, and which the widget
//! inspector reads its element's facts from.

use std::any::Any;
use std::fmt;
use std::rc::Rc;

use waft_foundation::{
    DiagnosticLevel, DiagnosticableTreeNode, DiagnosticsJsonMap, DiagnosticsNode,
    DiagnosticsSerializationDelegate, DiagnosticsTreeStyle, HandleId, TextTreeConfiguration,
    base_json_map,
};

/// What a node knows of its element, which Dart's node reads from the element itself: the
/// widget inspector's view of `node.value` when the value is an element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementFacts {
    /// The widget's type, as Dart's `widget.runtimeType` prints; `None` once the element has no
    /// widget.
    pub widget_runtime_type: Option<&'static str>,
    /// Whether the element is a `StatefulElement`.
    pub stateful: bool,
}

/// Dart's `_ElementDiagnosticableTreeNode`: an element's node, which answers its
/// [`ElementFacts`] as its value.
pub struct ElementDiagnosticsNode {
    node: DiagnosticableTreeNode,
    facts: ElementFacts,
}

impl ElementDiagnosticsNode {
    pub(crate) fn new(node: DiagnosticableTreeNode, facts: ElementFacts) -> ElementDiagnosticsNode {
        ElementDiagnosticsNode { node, facts }
    }

    /// Sets the node's style, as Dart's `toDiagnosticsNode(style:)` does.
    pub fn with_style(self, style: DiagnosticsTreeStyle) -> ElementDiagnosticsNode {
        ElementDiagnosticsNode {
            node: self.node.with_style(style),
            ..self
        }
    }
}

impl fmt::Display for ElementDiagnosticsNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.node.fmt(f)
    }
}

impl DiagnosticsNode for ElementDiagnosticsNode {
    fn name(&self) -> Option<&str> {
        self.node.name()
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.node.to_description(parent_configuration)
    }

    fn show_separator(&self) -> bool {
        self.node.show_separator()
    }

    fn level(&self) -> DiagnosticLevel {
        self.node.level()
    }

    fn show_name(&self) -> bool {
        self.node.show_name()
    }

    fn line_prefix(&self) -> Option<&str> {
        self.node.line_prefix()
    }

    fn empty_body_description(&self) -> Option<&str> {
        self.node.empty_body_description()
    }

    fn value(&self) -> Option<&dyn Any> {
        Some(&self.facts)
    }

    fn value_id(&self) -> Option<HandleId> {
        self.node.value_id()
    }

    fn type_name(&self) -> &'static str {
        "_ElementDiagnosticableTreeNode"
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.node.style()
    }

    fn allow_wrap(&self) -> bool {
        self.node.allow_wrap()
    }

    fn allow_name_wrap(&self) -> bool {
        self.node.allow_name_wrap()
    }

    fn allow_truncate(&self) -> bool {
        self.node.allow_truncate()
    }

    fn get_properties(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        self.node.get_properties()
    }

    fn get_children(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        self.node.get_children()
    }

    /// Dart's `_ElementDiagnosticableTreeNode.toJsonMap`: the node's, with the widget's type and
    /// whether the element is stateful.
    fn to_json_map(&self, delegate: &dyn DiagnosticsSerializationDelegate) -> DiagnosticsJsonMap {
        let mut json = base_json_map(self, delegate);
        if let Some(widget_runtime_type) = self.facts.widget_runtime_type {
            json.insert("widgetRuntimeType".to_owned(), widget_runtime_type.into());
        }
        json.insert("stateful".to_owned(), self.facts.stateful.into());
        json
    }

    fn to_string_deep_with(
        &self,
        prefix_line_one: &str,
        prefix_other_lines: Option<&str>,
        parent_configuration: Option<&TextTreeConfiguration>,
        min_level: DiagnosticLevel,
        wrap_width: usize,
    ) -> String {
        self.node.to_string_deep_with(
            prefix_line_one,
            prefix_other_lines,
            parent_configuration,
            min_level,
            wrap_width,
        )
    }
}
