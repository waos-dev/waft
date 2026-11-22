//! Flutter counterpart: `foundation/diagnostics.dart` (everything but JSON serialization).

use std::any::Any;
use std::cell::OnceCell;
use std::fmt;
use std::rc::Rc;
use std::sync::LazyLock;

use indexmap::IndexMap;

/// The various priority levels used to filter which diagnostics are shown and omitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticLevel {
    /// Diagnostics that should not be shown.
    Hidden,
    /// A diagnostic that is likely to be low value but where the display is
    /// just as high quality as a diagnostic with a higher level.
    Fine,
    /// Diagnostics that should only be shown when performing fine grained
    /// debugging of an object.
    Debug,
    /// Interesting diagnostics that should be typically shown.
    Info,
    /// Very important diagnostics that indicate problematic property values.
    Warning,
    /// Diagnostics that provide a hint about best practices.
    Hint,
    /// Diagnostics that summarize other diagnostics present.
    Summary,
    /// Diagnostics that indicate errors or unexpected conditions.
    Error,
    /// Special level indicating that no diagnostics should be shown.
    Off,
}

impl DiagnosticLevel {
    /// Dart's `DiagnosticLevel.index`; [`DiagnosticsNode::is_filtered`] compares these.
    pub fn index(self) -> usize {
        match self {
            DiagnosticLevel::Hidden => 0,
            DiagnosticLevel::Fine => 1,
            DiagnosticLevel::Debug => 2,
            DiagnosticLevel::Info => 3,
            DiagnosticLevel::Warning => 4,
            DiagnosticLevel::Hint => 5,
            DiagnosticLevel::Summary => 6,
            DiagnosticLevel::Error => 7,
            DiagnosticLevel::Off => 8,
        }
    }
}

/// Styles for displaying a node in a [`DiagnosticsNode`] tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticsTreeStyle {
    /// A style that does not display the tree.
    None,
    /// Sparse style for displaying trees.
    Sparse,
    /// Connects a node to its parent with a dashed line.
    Offstage,
    /// Slightly more compact version of the [`Sparse`](Self::Sparse) style.
    Dense,
    /// Style that enables transitioning from nodes of one style to children of
    /// another.
    Transition,
    /// Style for displaying content describing an error.
    Error,
    /// Render the tree just using whitespace without connecting parents to
    /// children using lines.
    Whitespace,
    /// Render the tree without indenting children at all.
    Flat,
    /// Render the tree on a single line without showing children.
    SingleLine,
    /// Render the tree using a style appropriate for properties that are part
    /// of an error message.
    ErrorProperty,
    /// Render only the immediate properties of a node instead of the full tree.
    Shallow,
    /// Render only the children of a node truncating before the tree becomes too
    /// large.
    TruncateChildren,
}

/// Configuration specifying how a particular [`DiagnosticsTreeStyle`] should be
/// rendered as text art.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextTreeConfiguration {
    /// Prefix to add to the first line to display a child with this style.
    pub prefix_line_one: String,
    /// Suffix to add to end of the first line to make its length match the footer.
    pub suffix_line_one: String,
    /// Prefix to add to other lines to display a child with this style.
    pub prefix_other_lines: String,
    /// Prefix to add to the first line to display the last child of a node with
    /// this style.
    pub prefix_last_child_line_one: String,
    /// Additional prefix to add to other lines of a node if this is the root
    /// node of the tree.
    pub prefix_other_lines_root_node: String,
    /// Prefix to add before each property if the node has children.
    pub property_prefix_if_children: String,
    /// Prefix to add before each property if the node does not have children.
    pub property_prefix_no_children: String,
    /// Character to use to draw line linking parent to child.
    pub link_character: String,
    /// Whitespace to draw instead of the child link character if this node is the
    /// last child of its parent so no link line is required.
    pub child_link_space: String,
    /// Character(s) to use to separate lines.
    pub line_break: String,
    /// Whether to place line breaks between properties or to leave all
    /// properties on one line.
    pub line_break_properties: bool,
    /// Text added immediately before the name of the node.
    pub before_name: String,
    /// Text added immediately after the name of the node.
    pub after_name: String,
    /// Text to add immediately after the description line of a node with
    /// properties and/or children if the node has a body.
    pub after_description_if_body: String,
    /// Text to add immediately after the description line of a node with
    /// properties and/or children.
    pub after_description: String,
    /// Optional string to add before the properties of a node.
    pub before_properties: String,
    /// Optional string to add after the properties of a node.
    pub after_properties: String,
    /// Mandatory string to add after the properties of a node regardless of
    /// whether the node has any properties.
    pub mandatory_after_properties: String,
    /// Property separator to add between properties.
    pub property_separator: String,
    /// Prefix to add to all lines of the body of the tree node.
    pub body_indent: String,
    /// Whether the children of a node should be shown.
    pub show_children: bool,
    /// Whether to add a blank line at the end of the output for a node if it has
    /// no children.
    pub add_blank_line_if_no_children: bool,
    /// Whether the name should be displayed on the same line as the description.
    pub is_name_on_own_line: bool,
    /// Footer to add as its own line at the end of a non-root node.
    pub footer: String,
    /// Footer to add even for root nodes.
    pub mandatory_footer: String,
    /// Add a blank line between properties and children if both are present.
    pub is_blank_line_between_properties_and_children: bool,
}

impl TextTreeConfiguration {
    /// Creates a configuration with Dart's required arguments; every other
    /// argument has Dart's default.
    pub fn new(
        prefix_line_one: impl Into<String>,
        prefix_other_lines: impl Into<String>,
        prefix_last_child_line_one: impl Into<String>,
        prefix_other_lines_root_node: impl Into<String>,
        link_character: impl Into<String>,
        property_prefix_if_children: impl Into<String>,
        property_prefix_no_children: impl Into<String>,
    ) -> Self {
        let link_character = link_character.into();
        TextTreeConfiguration {
            prefix_line_one: prefix_line_one.into(),
            suffix_line_one: String::new(),
            prefix_other_lines: prefix_other_lines.into(),
            prefix_last_child_line_one: prefix_last_child_line_one.into(),
            prefix_other_lines_root_node: prefix_other_lines_root_node.into(),
            property_prefix_if_children: property_prefix_if_children.into(),
            property_prefix_no_children: property_prefix_no_children.into(),
            child_link_space: " ".repeat(dart_len(&link_character)),
            link_character,
            line_break: "\n".to_string(),
            line_break_properties: true,
            before_name: String::new(),
            after_name: ":".to_string(),
            after_description_if_body: String::new(),
            after_description: String::new(),
            before_properties: String::new(),
            after_properties: String::new(),
            mandatory_after_properties: String::new(),
            property_separator: String::new(),
            body_indent: String::new(),
            show_children: true,
            add_blank_line_if_no_children: true,
            is_name_on_own_line: false,
            footer: String::new(),
            mandatory_footer: String::new(),
            is_blank_line_between_properties_and_children: true,
        }
    }

    /// Dart `TextTreeConfiguration(lineBreak:)`.
    pub fn line_break(mut self, line_break: impl Into<String>) -> Self {
        self.line_break = line_break.into();
        self
    }

    /// Dart `TextTreeConfiguration(lineBreakProperties:)`.
    pub fn line_break_properties(mut self, line_break_properties: bool) -> Self {
        self.line_break_properties = line_break_properties;
        self
    }

    /// Dart `TextTreeConfiguration(beforeName:)`.
    pub fn before_name(mut self, before_name: impl Into<String>) -> Self {
        self.before_name = before_name.into();
        self
    }

    /// Dart `TextTreeConfiguration(afterName:)`.
    pub fn after_name(mut self, after_name: impl Into<String>) -> Self {
        self.after_name = after_name.into();
        self
    }

    /// Dart `TextTreeConfiguration(afterDescriptionIfBody:)`.
    pub fn after_description_if_body(mut self, text: impl Into<String>) -> Self {
        self.after_description_if_body = text.into();
        self
    }

    /// Dart `TextTreeConfiguration(afterDescription:)`.
    pub fn after_description(mut self, text: impl Into<String>) -> Self {
        self.after_description = text.into();
        self
    }

    /// Dart `TextTreeConfiguration(beforeProperties:)`.
    pub fn before_properties(mut self, text: impl Into<String>) -> Self {
        self.before_properties = text.into();
        self
    }

    /// Dart `TextTreeConfiguration(afterProperties:)`.
    pub fn after_properties(mut self, text: impl Into<String>) -> Self {
        self.after_properties = text.into();
        self
    }

    /// Dart `TextTreeConfiguration(mandatoryAfterProperties:)`.
    pub fn mandatory_after_properties(mut self, text: impl Into<String>) -> Self {
        self.mandatory_after_properties = text.into();
        self
    }

    /// Dart `TextTreeConfiguration(propertySeparator:)`.
    pub fn property_separator(mut self, text: impl Into<String>) -> Self {
        self.property_separator = text.into();
        self
    }

    /// Dart `TextTreeConfiguration(bodyIndent:)`.
    pub fn body_indent(mut self, text: impl Into<String>) -> Self {
        self.body_indent = text.into();
        self
    }

    /// Dart `TextTreeConfiguration(showChildren:)`.
    pub fn show_children(mut self, show_children: bool) -> Self {
        self.show_children = show_children;
        self
    }

    /// Dart `TextTreeConfiguration(addBlankLineIfNoChildren:)`.
    pub fn add_blank_line_if_no_children(mut self, add: bool) -> Self {
        self.add_blank_line_if_no_children = add;
        self
    }

    /// Dart `TextTreeConfiguration(isNameOnOwnLine:)`.
    pub fn is_name_on_own_line(mut self, own_line: bool) -> Self {
        self.is_name_on_own_line = own_line;
        self
    }

    /// Dart `TextTreeConfiguration(footer:)`.
    pub fn footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = footer.into();
        self
    }

    /// Dart `TextTreeConfiguration(mandatoryFooter:)`.
    pub fn mandatory_footer(mut self, footer: impl Into<String>) -> Self {
        self.mandatory_footer = footer.into();
        self
    }

    /// Dart `TextTreeConfiguration(suffixLineOne:)`.
    pub fn suffix_line_one(mut self, suffix: impl Into<String>) -> Self {
        self.suffix_line_one = suffix.into();
        self
    }

    /// Dart `TextTreeConfiguration(isBlankLineBetweenPropertiesAndChildren:)`.
    pub fn is_blank_line_between_properties_and_children(mut self, blank: bool) -> Self {
        self.is_blank_line_between_properties_and_children = blank;
        self
    }
}

/// Default text tree configuration, for [`DiagnosticsTreeStyle::Sparse`].
pub fn sparse_text_configuration() -> &'static TextTreeConfiguration {
    static CONFIG: LazyLock<TextTreeConfiguration> =
        LazyLock::new(|| TextTreeConfiguration::new("├─", " ", "└─", " ", "│", "│ ", "  "));
    &CONFIG
}

/// Dashed configuration, for [`DiagnosticsTreeStyle::Offstage`].
pub fn dashed_text_configuration() -> &'static TextTreeConfiguration {
    static CONFIG: LazyLock<TextTreeConfiguration> =
        LazyLock::new(|| TextTreeConfiguration::new("╎╌", " ", "└╌", " ", "╎", "│ ", "  "));
    &CONFIG
}

/// Dense configuration, for [`DiagnosticsTreeStyle::Dense`].
pub fn dense_text_configuration() -> &'static TextTreeConfiguration {
    static CONFIG: LazyLock<TextTreeConfiguration> = LazyLock::new(|| {
        TextTreeConfiguration::new("├", "", "└", "", "│", "│", " ")
            .property_separator(", ")
            .before_properties("(")
            .after_properties(")")
            .line_break_properties(false)
            .add_blank_line_if_no_children(false)
            .is_blank_line_between_properties_and_children(false)
    });
    &CONFIG
}

/// Configuration that draws a box around a leaf node, for
/// [`DiagnosticsTreeStyle::Transition`].
pub fn transition_text_configuration() -> &'static TextTreeConfiguration {
    static CONFIG: LazyLock<TextTreeConfiguration> = LazyLock::new(|| {
        TextTreeConfiguration::new("╞═╦══ ", " ║ ", "╘═╦══ ", "", "│", "", "")
            .footer(" ╚═══════════")
            .after_name(" ═══")
            .after_description_if_body(":")
            .body_indent("  ")
            .is_name_on_own_line(true)
            .add_blank_line_if_no_children(false)
            .is_blank_line_between_properties_and_children(false)
    });
    &CONFIG
}

/// Configuration that draws a box around a node, for
/// [`DiagnosticsTreeStyle::Error`].
pub fn error_text_configuration() -> &'static TextTreeConfiguration {
    static CONFIG: LazyLock<TextTreeConfiguration> = LazyLock::new(|| {
        TextTreeConfiguration::new("╞═╦", " ║ ", "╘═╦", "", "│", "", "")
            .footer(" ╚═══════════")
            .before_name("══╡ ")
            .suffix_line_one(" ╞══")
            .mandatory_footer("═════")
            .add_blank_line_if_no_children(false)
            .is_blank_line_between_properties_and_children(false)
    });
    &CONFIG
}

/// Whitespace only configuration where children are consistently indented,
/// for [`DiagnosticsTreeStyle::Whitespace`].
pub fn whitespace_text_configuration() -> &'static TextTreeConfiguration {
    static CONFIG: LazyLock<TextTreeConfiguration> = LazyLock::new(|| {
        TextTreeConfiguration::new("", " ", "", "  ", " ", "", "")
            .add_blank_line_if_no_children(false)
            .after_description_if_body(":")
            .is_blank_line_between_properties_and_children(false)
    });
    &CONFIG
}

/// Whitespace only configuration where children are not indented, for
/// [`DiagnosticsTreeStyle::Flat`].
pub fn flat_text_configuration() -> &'static TextTreeConfiguration {
    static CONFIG: LazyLock<TextTreeConfiguration> = LazyLock::new(|| {
        TextTreeConfiguration::new("", "", "", "", "", "", "")
            .add_blank_line_if_no_children(false)
            .after_description_if_body(":")
            .is_blank_line_between_properties_and_children(false)
    });
    &CONFIG
}

/// Render a node as a single line omitting children, for
/// [`DiagnosticsTreeStyle::SingleLine`].
pub fn single_line_text_configuration() -> &'static TextTreeConfiguration {
    static CONFIG: LazyLock<TextTreeConfiguration> = LazyLock::new(|| {
        TextTreeConfiguration::new("", "", "", "", "", "  ", "  ")
            .property_separator(", ")
            .before_properties("(")
            .after_properties(")")
            .line_break("")
            .line_break_properties(false)
            .add_blank_line_if_no_children(false)
            .show_children(false)
    });
    &CONFIG
}

/// Render the name on a line followed by the body and properties on the next
/// line omitting the children, for [`DiagnosticsTreeStyle::ErrorProperty`].
pub fn error_property_text_configuration() -> &'static TextTreeConfiguration {
    static CONFIG: LazyLock<TextTreeConfiguration> = LazyLock::new(|| {
        TextTreeConfiguration::new("", "", "", "", "", "  ", "  ")
            .property_separator(", ")
            .before_properties("(")
            .after_properties(")")
            .line_break_properties(false)
            .add_blank_line_if_no_children(false)
            .show_children(false)
            .is_name_on_own_line(true)
    });
    &CONFIG
}

/// Render a node on multiple lines omitting children, for
/// [`DiagnosticsTreeStyle::Shallow`].
pub fn shallow_text_configuration() -> &'static TextTreeConfiguration {
    static CONFIG: LazyLock<TextTreeConfiguration> = LazyLock::new(|| {
        TextTreeConfiguration::new("", " ", "", "  ", " ", "", "")
            .add_blank_line_if_no_children(false)
            .after_description_if_body(":")
            .is_blank_line_between_properties_and_children(false)
            .show_children(false)
    });
    &CONFIG
}

/// The text configuration for `style`, or none for
/// [`DiagnosticsTreeStyle::None`].
pub fn text_tree_configuration_for(
    style: DiagnosticsTreeStyle,
) -> Option<&'static TextTreeConfiguration> {
    match style {
        DiagnosticsTreeStyle::None => None,
        DiagnosticsTreeStyle::Dense => Some(dense_text_configuration()),
        DiagnosticsTreeStyle::Sparse => Some(sparse_text_configuration()),
        DiagnosticsTreeStyle::Offstage => Some(dashed_text_configuration()),
        DiagnosticsTreeStyle::Whitespace => Some(whitespace_text_configuration()),
        DiagnosticsTreeStyle::Transition => Some(transition_text_configuration()),
        DiagnosticsTreeStyle::SingleLine => Some(single_line_text_configuration()),
        DiagnosticsTreeStyle::ErrorProperty => Some(error_property_text_configuration()),
        DiagnosticsTreeStyle::Shallow => Some(shallow_text_configuration()),
        DiagnosticsTreeStyle::Error => Some(error_text_configuration()),
        DiagnosticsTreeStyle::Flat => Some(flat_text_configuration()),
        // Truncate children doesn't really need its own text style as the
        // rendering is quite custom.
        DiagnosticsTreeStyle::TruncateChildren => Some(whitespace_text_configuration()),
    }
}

fn is_single_line(style: Option<DiagnosticsTreeStyle>) -> bool {
    style == Some(DiagnosticsTreeStyle::SingleLine)
}

/// Dart's `String.length`: UTF-16 code units.
fn dart_len(text: &str) -> usize {
    text.encode_utf16().count()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WordWrapParseMode {
    InSpace,
    InWord,
    AtBreak,
}

/// Builder that builds a String with specified prefixes for the first and
/// subsequent lines.
struct PrefixedStringBuilder {
    /// Prefix to add to the first line.
    prefix_line_one: String,
    /// Prefix to add to subsequent lines.
    prefix_other_lines: String,
    next_prefix_other_lines: Option<String>,
    wrap_width: usize,
    /// Buffer containing lines that have already been completely laid out.
    buffer: String,
    /// Buffer containing the current line that has not yet been wrapped.
    current_line: String,
    /// Pairs of integers indicating the start and end of each block of text
    /// within `current_line` that can be wrapped.
    wrappable_ranges: Vec<usize>,
    num_lines: usize,
}

impl PrefixedStringBuilder {
    fn new(prefix_line_one: String, prefix_other_lines: String, wrap_width: usize) -> Self {
        PrefixedStringBuilder {
            prefix_line_one,
            prefix_other_lines,
            next_prefix_other_lines: None,
            wrap_width,
            buffer: String::new(),
            current_line: String::new(),
            wrappable_ranges: Vec::new(),
            num_lines: 0,
        }
    }

    fn increment_prefix_other_lines(&mut self, suffix: &str, update_current_line: bool) {
        let prefix = format!("{}{}", self.prefix_other_lines, suffix);
        if self.current_line.is_empty() || update_current_line {
            self.prefix_other_lines = prefix;
            self.next_prefix_other_lines = None;
        } else {
            self.next_prefix_other_lines = Some(prefix);
        }
    }

    fn requires_multiple_lines(&self) -> bool {
        self.num_lines > 1
            || (self.num_lines == 1 && !self.current_line.is_empty())
            || (dart_len(&self.current_line) + dart_len(self.current_prefix(true))
                > self.wrap_width)
    }

    fn is_current_line_empty(&self) -> bool {
        self.current_line.is_empty()
    }

    fn finalize_line(&mut self, add_trailing_line_break: bool) {
        let first_line = self.buffer.is_empty();
        let text = std::mem::take(&mut self.current_line);

        if self.wrappable_ranges.is_empty() {
            // Fast path. There were no wrappable spans of text.
            self.write_line(&text, add_trailing_line_break, first_line);
            return;
        }
        let lines = Self::word_wrap_line(
            &text,
            &self.wrappable_ranges,
            self.wrap_width,
            if first_line {
                dart_len(&self.prefix_line_one)
            } else {
                dart_len(&self.prefix_other_lines)
            },
            dart_len(&self.prefix_other_lines),
        );
        let length = lines.len();
        for (i, line) in lines.into_iter().enumerate() {
            self.write_line(&line, add_trailing_line_break || i + 1 < length, first_line);
        }
        self.wrappable_ranges.clear();
    }

    /// Wraps the given string at the given width.
    ///
    /// Wrapping occurs at space characters (U+0020). Only the spans listed in
    /// `wrap_ranges` may be broken.
    fn word_wrap_line(
        message: &str,
        wrap_ranges: &[usize],
        width: usize,
        start_offset: usize,
        other_line_offset: usize,
    ) -> Vec<String> {
        let encoded: Vec<u16> = message.encode_utf16().collect();
        if encoded.len() + start_offset < width {
            // Nothing to do. The line doesn't wrap.
            return vec![message.to_string()];
        }
        let mut wrapped_line = Vec::new();
        let mut start_for_length_calculations = -(start_offset as i64);
        let mut index: usize = 0;
        let mut mode = WordWrapParseMode::InSpace;
        let mut last_word_start: usize = 0;
        let mut last_word_end: Option<usize> = None;
        let mut start: usize = 0;
        let mut current_chunk: usize = 0;
        let space = b' ' as u16;

        // This helper is called with increasing indexes.
        let mut no_wrap = |index: usize| -> bool {
            loop {
                if current_chunk >= wrap_ranges.len() {
                    break true;
                }
                if index < wrap_ranges[current_chunk + 1] {
                    return index < wrap_ranges[current_chunk];
                }
                current_chunk += 2;
            }
        };

        loop {
            match mode {
                // At start of break point (or start of line); can't break until
                // the next break.
                WordWrapParseMode::InSpace => {
                    while index < encoded.len() && encoded[index] == space {
                        index += 1;
                    }
                    last_word_start = index;
                    mode = WordWrapParseMode::InWord;
                }
                // Looking for a good break point. Treat all text as non
                // wrappable while `no_wrap` says so.
                WordWrapParseMode::InWord => {
                    while index < encoded.len() && (encoded[index] != space || no_wrap(index)) {
                        index += 1;
                    }
                    mode = WordWrapParseMode::AtBreak;
                }
                // At start of break point.
                WordWrapParseMode::AtBreak => {
                    if (index as i64 - start_for_length_calculations > width as i64)
                        || (index == encoded.len())
                    {
                        // We are over the width line, so break.
                        if ((index as i64 - start_for_length_calculations) <= width as i64)
                            || last_word_end.is_none()
                        {
                            // We should use this point, because either it doesn't
                            // actually go over the end (last line), or it does,
                            // but there was no earlier break point.
                            last_word_end = Some(index);
                        }
                        let end = last_word_end.expect("a break point was chosen");
                        let line = String::from_utf16(&encoded[start..end])
                            .expect("a wrap boundary is at a space or the end");
                        wrapped_line.push(line);
                        if end >= encoded.len() {
                            return wrapped_line;
                        }
                        // Just yielded a line.
                        if end == index {
                            // We broke at the current position; eat all the
                            // wrappable spaces, then set our start point.
                            while index < encoded.len() && encoded[index] == space {
                                index += 1;
                            }
                            start = index;
                            mode = WordWrapParseMode::InWord;
                        } else {
                            // We broke at the previous break point, and we're at
                            // the start of a new one.
                            debug_assert!(last_word_start > end);
                            start = last_word_start;
                            mode = WordWrapParseMode::AtBreak;
                        }
                        start_for_length_calculations = start as i64 - other_line_offset as i64;
                        last_word_end = None;
                    } else {
                        // Save this break point; we're not yet over the line
                        // width.
                        last_word_end = Some(index);
                        // Skip to the end of this break point.
                        mode = WordWrapParseMode::InSpace;
                    }
                }
            }
        }
    }

    /// Writes text ensuring the specified prefixes for the first and subsequent
    /// lines.
    fn write(&mut self, s: &str, allow_wrap: bool) {
        if s.is_empty() {
            return;
        }

        let lines: Vec<&str> = s.split('\n').collect();
        for (i, line) in lines.into_iter().enumerate() {
            if i > 0 {
                self.finalize_line(true);
                self.update_prefix();
            }
            if !line.is_empty() {
                if allow_wrap {
                    let wrap_start = dart_len(&self.current_line);
                    let wrap_end = wrap_start + dart_len(line);
                    if self.wrappable_ranges.last() == Some(&wrap_start) {
                        // Extend the last range.
                        *self.wrappable_ranges.last_mut().expect("checked above") = wrap_end;
                    } else {
                        self.wrappable_ranges.push(wrap_start);
                        self.wrappable_ranges.push(wrap_end);
                    }
                }
                self.current_line.push_str(line);
            }
        }
    }

    fn update_prefix(&mut self) {
        if let Some(prefix) = self.next_prefix_other_lines.take() {
            self.prefix_other_lines = prefix;
        }
    }

    fn write_line(&mut self, line: &str, include_line_break: bool, first_line: bool) {
        let prefixed = format!("{}{}", self.current_prefix(first_line), line);
        self.buffer.push_str(prefixed.trim_end());
        if include_line_break {
            self.buffer.push('\n');
        }
        self.num_lines += 1;
    }

    /// Dart's `_getCurrentPrefix`; the `first_line` argument is unused there
    /// too: the buffer decides.
    fn current_prefix(&self, _first_line: bool) -> &str {
        if self.buffer.is_empty() {
            &self.prefix_line_one
        } else {
            &self.prefix_other_lines
        }
    }

    /// Writes lines assuming the lines obey the specified prefixes. Ensures that
    /// a newline is added if one is not present.
    fn write_raw_lines(&mut self, lines: &str) {
        if lines.is_empty() {
            return;
        }

        if !self.current_line.is_empty() {
            self.finalize_line(true);
        }
        debug_assert!(self.current_line.is_empty());

        self.buffer.push_str(lines);
        if !lines.ends_with('\n') {
            self.buffer.push('\n');
        }
        self.num_lines += 1;
        self.update_prefix();
    }

    /// Finishes the current line with a stretched version of text.
    fn write_stretched(&mut self, text: &str, target_line_length: usize) {
        self.write(text, false);
        let current_line_length =
            dart_len(&self.current_line) + dart_len(self.current_prefix(self.buffer.is_empty()));
        assert!(
            !self.current_line.is_empty(),
            "writeStretched needs text on the current line"
        );
        let target_length = target_line_length as i64 - current_line_length as i64;
        if target_length > 0 {
            assert!(!text.is_empty());
            let last_char = text.chars().last().expect("text is nonempty");
            assert_ne!(last_char, '\n');
            self.current_line
                .extend(std::iter::repeat_n(last_char, target_length as usize));
        }
        // Mark the entire line as not wrappable.
        self.wrappable_ranges.clear();
    }

    fn build(mut self) -> String {
        if !self.current_line.is_empty() {
            self.finalize_line(false);
        }
        self.buffer
    }
}

/// Renderer that creates ASCII art representations of trees of
/// [`DiagnosticsNode`] objects.
pub struct TextTreeRenderer {
    wrap_width: usize,
    wrap_width_properties: usize,
    min_level: DiagnosticLevel,
    max_descendents_truncatable_node: i32,
}

impl TextTreeRenderer {
    /// Creates a renderer with Dart's defaults.
    pub fn new() -> Self {
        TextTreeRenderer {
            wrap_width: 100,
            wrap_width_properties: 65,
            min_level: DiagnosticLevel::Debug,
            max_descendents_truncatable_node: -1,
        }
    }

    /// Dart `TextTreeRenderer(wrapWidth:)`.
    pub fn wrap_width(mut self, wrap_width: usize) -> Self {
        self.wrap_width = wrap_width;
        self
    }

    /// Dart `TextTreeRenderer(wrapWidthProperties:)`.
    pub fn wrap_width_properties(mut self, wrap_width_properties: usize) -> Self {
        self.wrap_width_properties = wrap_width_properties;
        self
    }

    /// Dart `TextTreeRenderer(minLevel:)`.
    pub fn min_level(mut self, min_level: DiagnosticLevel) -> Self {
        self.min_level = min_level;
        self
    }

    /// Dart `TextTreeRenderer(_maxDescendentsTruncatableNode:)`.
    pub fn max_descendents_truncatable_node(mut self, max: i32) -> Self {
        self.max_descendents_truncatable_node = max;
        self
    }

    /// Text configuration to use to connect this node to a `child`.
    fn child_text_configuration<'a>(
        &self,
        child: &'a dyn DiagnosticsNode,
        text_style: &'a TextTreeConfiguration,
    ) -> &'a TextTreeConfiguration {
        let child_style = child.style();
        if is_single_line(child_style) || child_style == Some(DiagnosticsTreeStyle::ErrorProperty) {
            text_style
        } else {
            child
                .text_tree_configuration()
                .expect("a diagnostics node has a text tree configuration")
        }
    }

    /// Renders a [`DiagnosticsNode`] to a String with no prefix.
    pub fn render(&self, node: &dyn DiagnosticsNode) -> String {
        self.render_with(node, "", None, None)
    }

    /// Renders a [`DiagnosticsNode`] to a String.
    pub fn render_with(
        &self,
        node: &dyn DiagnosticsNode,
        prefix_line_one: &str,
        prefix_other_lines: Option<&str>,
        parent_configuration: Option<&TextTreeConfiguration>,
    ) -> String {
        let single_line = is_single_line(node.style())
            && parent_configuration.is_none_or(|parent| !parent.line_break_properties);
        let mut prefix_line_one = prefix_line_one.to_string();
        let mut prefix_other_lines = prefix_other_lines
            .map(str::to_string)
            .unwrap_or_else(|| prefix_line_one.clone());
        if let Some(line_prefix) = node.line_prefix() {
            prefix_line_one.push_str(line_prefix);
            prefix_other_lines.push_str(line_prefix);
        }

        let config = node
            .text_tree_configuration()
            .expect("a diagnostics node has a text tree configuration");
        if prefix_other_lines.is_empty() {
            prefix_other_lines.push_str(&config.prefix_other_lines_root_node);
        }

        if node.style() == Some(DiagnosticsTreeStyle::TruncateChildren) {
            // This style is different enough that it isn't worthwhile to reuse
            // the existing logic.
            let mut descendants = Vec::new();
            const MAX_DEPTH: usize = 5;
            let mut depth = 0;
            const MAX_LINES: usize = 25;
            let mut lines = 0;
            fn visitor(
                node: &dyn DiagnosticsNode,
                prefix_other_lines: &str,
                depth: &mut usize,
                lines: &mut usize,
                descendants: &mut Vec<String>,
            ) {
                for child in node.get_children() {
                    if *lines < MAX_LINES {
                        *depth += 1;
                        descendants.push(format!(
                            "{prefix_other_lines}{}{child}",
                            "  ".repeat(*depth)
                        ));
                        if *depth < MAX_DEPTH {
                            visitor(
                                child.as_ref(),
                                prefix_other_lines,
                                depth,
                                lines,
                                descendants,
                            );
                        }
                        *depth -= 1;
                    } else if *lines == MAX_LINES {
                        descendants.push(format!(
                            "{prefix_other_lines}  ...(descendants list truncated after {lines} lines)"
                        ));
                    }
                    *lines += 1;
                }
            }
            visitor(
                node,
                &prefix_other_lines,
                &mut depth,
                &mut lines,
                &mut descendants,
            );
            let mut information = prefix_line_one;
            if lines > 1 {
                information.push_str(&format!(
                    "This {} had the following descendants (showing up to depth {MAX_DEPTH}):\n",
                    node.name().unwrap_or("null")
                ));
            } else if descendants.len() == 1 {
                information.push_str(&format!(
                    "This {} had the following child:\n",
                    node.name().unwrap_or("null")
                ));
            } else {
                information.push_str(&format!(
                    "This {} has no descendants.\n",
                    node.name().unwrap_or("null")
                ));
            }
            information.push_str(&descendants.join("\n"));
            return information;
        }

        let mut builder = PrefixedStringBuilder::new(
            prefix_line_one,
            prefix_other_lines.clone(),
            self.wrap_width
                .max(dart_len(&prefix_other_lines) + self.wrap_width_properties),
        );

        let mut children = node.get_children();

        let mut description = node.to_description(parent_configuration);
        if !config.before_name.is_empty() {
            builder.write(&config.before_name, false);
        }
        let wrap_name = !single_line && node.allow_name_wrap();
        let wrap_description = !single_line && node.allow_wrap();
        let uppercase_title = node.style() == Some(DiagnosticsTreeStyle::Error);
        let mut name = node.name().map(str::to_string);
        if uppercase_title {
            name = name.map(|name| name.to_uppercase());
        }
        if description.is_empty() {
            if node.show_name()
                && let Some(name) = &name
            {
                builder.write(name, wrap_name);
            }
        } else {
            let mut include_name = false;
            if let Some(name) = &name
                && !name.is_empty()
                && node.show_name()
            {
                include_name = true;
                builder.write(name, wrap_name);
                if node.show_separator() {
                    builder.write(&config.after_name, wrap_name);
                }

                builder.write(
                    if config.is_name_on_own_line || description.contains('\n') {
                        "\n"
                    } else {
                        " "
                    },
                    wrap_name,
                );
            }
            if !single_line && builder.requires_multiple_lines() && !builder.is_current_line_empty()
            {
                // Make sure there is a break between the current line and next
                // one if there is not one already.
                builder.write("\n", false);
            }
            if include_name {
                builder.increment_prefix_other_lines(
                    if children.is_empty() {
                        &config.property_prefix_no_children
                    } else {
                        &config.property_prefix_if_children
                    },
                    true,
                );
            }

            if uppercase_title {
                description = description.to_uppercase();
            }
            builder.write(description.trim_end(), wrap_description);

            if !include_name {
                builder.increment_prefix_other_lines(
                    if children.is_empty() {
                        &config.property_prefix_no_children
                    } else {
                        &config.property_prefix_if_children
                    },
                    false,
                );
            }
        }
        if !config.suffix_line_one.is_empty() {
            builder.write_stretched(&config.suffix_line_one, builder.wrap_width);
        }

        let mut properties: Vec<Rc<dyn DiagnosticsNode>> = node
            .get_properties()
            .into_iter()
            .filter(|node| !node.is_filtered(self.min_level))
            .collect();
        if self.max_descendents_truncatable_node >= 0 && node.allow_truncate() {
            let max = self.max_descendents_truncatable_node as usize;
            if properties.len() < max {
                properties.truncate(max);
                properties.push(Rc::new(message("...")));
            }
            if max < children.len() {
                children.truncate(max);
                children.push(Rc::new(message("...")));
            }
        }

        // If the node does not show a separator and there is no description
        // then we should not place a separator between the name and the value.
        // Essentially in this case the properties are treated a bit like a
        // value.
        if (!properties.is_empty()
            || !children.is_empty()
            || node.empty_body_description().is_some())
            && (node.show_separator() || !description.is_empty())
        {
            builder.write(&config.after_description_if_body, false);
        }

        if config.line_break_properties {
            builder.write(&config.line_break, false);
        }

        if !properties.is_empty() {
            builder.write(&config.before_properties, false);
        }

        builder.increment_prefix_other_lines(&config.body_indent, false);

        if node.empty_body_description().is_some()
            && properties.is_empty()
            && children.is_empty()
            && !builder.prefix_line_one.is_empty()
        {
            builder.write(node.empty_body_description().expect("checked above"), false);
            if config.line_break_properties {
                builder.write(&config.line_break, false);
            }
        }

        for (i, property) in properties.iter().enumerate() {
            if i > 0 {
                builder.write(&config.property_separator, false);
            }

            let property_style = property
                .text_tree_configuration()
                .expect("a diagnostics node has a text tree configuration");
            if is_single_line(property.style()) {
                // We have to treat single line properties slightly differently
                // to deal with cases where a single line property's output may
                // not have a single line break.
                let property_render = self.render_with(
                    property.as_ref(),
                    &property_style.prefix_line_one,
                    Some(&format!(
                        "{}{}",
                        property_style.child_link_space, property_style.prefix_other_lines
                    )),
                    Some(config),
                );
                let property_lines: Vec<&str> = property_render.split('\n').collect();
                if property_lines.len() == 1 && !config.line_break_properties {
                    builder.write(property_lines[0], false);
                } else {
                    builder.write(&property_render, false);
                    if !property_render.ends_with('\n') {
                        builder.write("\n", false);
                    }
                }
            } else {
                let property_render = self.render_with(
                    property.as_ref(),
                    &format!(
                        "{}{}",
                        builder.prefix_other_lines, property_style.prefix_line_one
                    ),
                    Some(&format!(
                        "{}{}{}",
                        builder.prefix_other_lines,
                        property_style.child_link_space,
                        property_style.prefix_other_lines
                    )),
                    Some(config),
                );
                builder.write_raw_lines(&property_render);
            }
        }
        if !properties.is_empty() {
            builder.write(&config.after_properties, false);
        }

        builder.write(&config.mandatory_after_properties, false);

        if !config.line_break_properties {
            builder.write(&config.line_break, false);
        }

        let prefix_children = config.body_indent.clone();
        let prefix_children_raw = format!("{prefix_other_lines}{prefix_children}");
        if children.is_empty()
            && config.add_blank_line_if_no_children
            && builder.requires_multiple_lines()
            && !builder.prefix_other_lines.trim_end().is_empty()
        {
            builder.write(&config.line_break, false);
        }

        if !children.is_empty() && config.show_children {
            if config.is_blank_line_between_properties_and_children
                && !properties.is_empty()
                && children[0]
                    .text_tree_configuration()
                    .is_some_and(|config| config.is_blank_line_between_properties_and_children)
            {
                builder.write(&config.line_break, false);
            }

            builder.prefix_other_lines = prefix_other_lines.clone();

            for i in 0..children.len() {
                let child = &children[i];
                let child_config = self.child_text_configuration(child.as_ref(), config);
                let last = i == children.len() - 1;
                let (child_prefix_line_one, child_prefix_other_lines) = if last {
                    (
                        format!(
                            "{prefix_children_raw}{}",
                            child_config.prefix_last_child_line_one
                        ),
                        format!(
                            "{prefix_children_raw}{}{}",
                            child_config.child_link_space, child_config.prefix_other_lines
                        ),
                    )
                } else {
                    let next_child_style =
                        self.child_text_configuration(children[i + 1].as_ref(), config);
                    (
                        format!("{prefix_children_raw}{}", child_config.prefix_line_one),
                        format!(
                            "{prefix_children_raw}{}{}",
                            next_child_style.link_character, child_config.prefix_other_lines
                        ),
                    )
                };
                let child_render = self.render_with(
                    child.as_ref(),
                    &child_prefix_line_one,
                    Some(&child_prefix_other_lines),
                    Some(config),
                );
                builder.write_raw_lines(&child_render);
                if !child_config.footer.is_empty() {
                    builder.prefix_other_lines = prefix_children_raw.clone();
                    let footer = format!(
                        "{}{}",
                        if last {
                            &child_config.child_link_space
                        } else {
                            &child_config.link_character
                        },
                        child_config.footer
                    );
                    builder.write(&footer, false);
                    if !child_config.mandatory_footer.is_empty() {
                        builder.write_stretched(
                            &child_config.mandatory_footer,
                            builder.wrap_width.max(
                                self.wrap_width_properties + dart_len(&child_prefix_other_lines),
                            ),
                        );
                    }
                    builder.write(&config.line_break, false);
                }
            }
        }
        if parent_configuration.is_none() && !config.mandatory_footer.is_empty() {
            builder.write_stretched(&config.mandatory_footer, builder.wrap_width);
            builder.write(&config.line_break, false);
        }
        builder.build()
    }
}

impl Default for TextTreeRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Defines diagnostics data for a value.
///
/// Dart's `DiagnosticsNode`. Rust keeps Flutter's member set on a trait because
/// each node class has its own storage.
pub trait DiagnosticsNode: fmt::Display {
    /// Label describing the node, typically shown before a separator.
    fn name(&self) -> Option<&str> {
        None
    }

    /// Returns a description with a short summary of the node itself not
    /// including children or properties.
    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String;

    /// Whether to show a separator between [`name`](Self::name) and
    /// [`to_description`](Self::to_description).
    fn show_separator(&self) -> bool {
        true
    }

    /// Whether the diagnostic should be filtered because its level is lower
    /// than `min_level`.
    fn is_filtered(&self, min_level: DiagnosticLevel) -> bool {
        self.level().index() < min_level.index()
    }

    /// Priority level of the diagnostic.
    fn level(&self) -> DiagnosticLevel {
        DiagnosticLevel::Info
    }

    /// Whether the name of the property should be shown when showing the
    /// default view of the tree.
    fn show_name(&self) -> bool {
        true
    }

    /// Prefix to include at the start of each line.
    fn line_prefix(&self) -> Option<&str> {
        None
    }

    /// Description to show if the node has no displayed properties or children.
    fn empty_body_description(&self) -> Option<&str> {
        None
    }

    /// The actual object this diagnostics data is for.
    ///
    /// A snapshot node ([`DiagnosticableNode`]) does not keep the arena object,
    /// so it answers none.
    fn value(&self) -> Option<&dyn Any> {
        None
    }

    /// Hint for how the node should be displayed.
    fn style(&self) -> Option<DiagnosticsTreeStyle>;

    /// Whether to wrap text onto multiple lines.
    fn allow_wrap(&self) -> bool {
        false
    }

    /// Whether to wrap the name onto multiple lines.
    fn allow_name_wrap(&self) -> bool {
        false
    }

    /// Whether to allow truncation when displaying the node and its children.
    fn allow_truncate(&self) -> bool {
        false
    }

    /// Properties of this node.
    fn get_properties(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        Vec::new()
    }

    /// Children of this node.
    fn get_children(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        Vec::new()
    }

    /// Configuration specifying how this node should be rendered as text art.
    fn text_tree_configuration(&self) -> Option<&TextTreeConfiguration> {
        self.style().and_then(text_tree_configuration_for)
    }

    /// Dart's `_separator`.
    fn separator(&self) -> &str {
        if self.show_separator() { ":" } else { "" }
    }

    /// Converts the properties of this node to a form useful for timeline
    /// event arguments. Children are omitted.
    fn to_timeline_arguments(&self) -> Option<IndexMap<String, String>> {
        let mut result = IndexMap::new();
        for property in self.get_properties() {
            if let Some(name) = property.name() {
                result.insert(
                    name.to_string(),
                    property.to_description(Some(single_line_text_configuration())),
                );
            }
        }
        Some(result)
    }

    /// Returns a string representation compatible with the style of the parent
    /// if the node is not the root.
    fn to_string_with(
        &self,
        parent_configuration: Option<&TextTreeConfiguration>,
        min_level: DiagnosticLevel,
    ) -> String {
        if is_single_line(self.style()) {
            self.to_string_deep_with("", None, parent_configuration, min_level, 65)
        } else {
            let description = self.to_description(parent_configuration);
            match self.name() {
                Some(name) if !name.is_empty() && self.show_name() => {
                    if description.contains('\n') {
                        format!("{name}{}\n{description}", self.separator())
                    } else {
                        format!("{name}{} {description}", self.separator())
                    }
                }
                _ => description,
            }
        }
    }

    /// Returns a string representation of this node and its descendants with
    /// Dart's default arguments.
    fn to_string_deep(&self) -> String {
        self.to_string_deep_with("", None, None, DiagnosticLevel::Debug, 65)
    }

    /// Returns a string representation of this node and its descendants.
    ///
    /// Implementations forward to [`TextTreeRenderer`]; the method is required
    /// rather than provided because `self` cannot be coerced to
    /// `&dyn DiagnosticsNode` where `Self` is unsized.
    fn to_string_deep_with(
        &self,
        prefix_line_one: &str,
        prefix_other_lines: Option<&str>,
        parent_configuration: Option<&TextTreeConfiguration>,
        min_level: DiagnosticLevel,
        wrap_width: usize,
    ) -> String;
}

macro_rules! render_deep_with {
    () => {
        fn to_string_deep_with(
            &self,
            prefix_line_one: &str,
            prefix_other_lines: Option<&str>,
            parent_configuration: Option<&TextTreeConfiguration>,
            min_level: DiagnosticLevel,
            wrap_width: usize,
        ) -> String {
            TextTreeRenderer::new()
                .min_level(min_level)
                .wrap_width(wrap_width)
                .render_with(
                    self,
                    prefix_line_one,
                    prefix_other_lines,
                    parent_configuration,
                )
        }
    };
}

impl DiagnosticsNode for Rc<dyn DiagnosticsNode> {
    fn name(&self) -> Option<&str> {
        (**self).name()
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        (**self).to_description(parent_configuration)
    }

    fn show_separator(&self) -> bool {
        (**self).show_separator()
    }

    fn level(&self) -> DiagnosticLevel {
        (**self).level()
    }

    fn show_name(&self) -> bool {
        (**self).show_name()
    }

    fn line_prefix(&self) -> Option<&str> {
        (**self).line_prefix()
    }

    fn empty_body_description(&self) -> Option<&str> {
        (**self).empty_body_description()
    }

    fn value(&self) -> Option<&dyn Any> {
        (**self).value()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        (**self).style()
    }

    fn allow_wrap(&self) -> bool {
        (**self).allow_wrap()
    }

    fn allow_name_wrap(&self) -> bool {
        (**self).allow_name_wrap()
    }

    fn allow_truncate(&self) -> bool {
        (**self).allow_truncate()
    }

    fn get_properties(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        (**self).get_properties()
    }

    fn get_children(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        (**self).get_children()
    }

    fn to_string_deep_with(
        &self,
        prefix_line_one: &str,
        prefix_other_lines: Option<&str>,
        parent_configuration: Option<&TextTreeConfiguration>,
        min_level: DiagnosticLevel,
        wrap_width: usize,
    ) -> String {
        (**self).to_string_deep_with(
            prefix_line_one,
            prefix_other_lines,
            parent_configuration,
            min_level,
            wrap_width,
        )
    }
}

/// A property's name, which Dart lets be `null`: a string where there is one, `None` where
/// there is not, so a property reads as Dart's does: `IntProperty::new("depth", depth)`.
pub trait IntoPropertyName {
    fn into_property_name(self) -> Option<String>;
}

impl IntoPropertyName for &str {
    fn into_property_name(self) -> Option<String> {
        Some(self.to_owned())
    }
}

impl IntoPropertyName for String {
    fn into_property_name(self) -> Option<String> {
        Some(self)
    }
}

impl IntoPropertyName for Option<String> {
    fn into_property_name(self) -> Option<String> {
        self
    }
}

/// Signature for computing the value of a property.
///
/// It is called the first time the value is read, as Dart's `computeValue` is. Dart catches what
/// it throws and shows `EXCEPTION`; a panic here ends the program, as any panic does.
pub type ComputePropertyValueCallback<T> = Rc<dyn Fn() -> Option<T>>;

enum PropertyValue<T> {
    Direct(Option<T>),
    Lazy(LazyValue<T>),
}

struct LazyValue<T> {
    compute: ComputePropertyValueCallback<T>,
    value: OnceCell<Option<T>>,
}

/// Dart's `DiagnosticsProperty<T>` fields and shared behavior.
///
/// The property classes have no inheritance in Rust; each one owns one of these and hands it out
/// through `property_data`, as Flutter's subclasses inherit the fields.
pub struct PropertyData<T> {
    /// Label describing the node.
    pub name: Option<String>,
    /// Description of the property value, when it is not given by `Display`.
    pub description: Option<String>,
    /// Description if the property value is null, including `'MISSING'` when
    /// `missingIfNull` is set.
    pub if_null: Option<String>,
    /// Description if the property description would otherwise be empty.
    pub if_empty: Option<String>,
    /// Whether the name of the property should be shown.
    pub show_name: bool,
    /// Whether a separator is shown between the name and description.
    pub show_separator: bool,
    /// The default value of this property. The outer option is Dart's
    /// `kNoDefaultValue`; the inner one is a default of null.
    pub default_value: Option<Option<T>>,
    /// Optional tooltip describing the property.
    pub tooltip: Option<String>,
    /// Whether a null value makes the property a warning.
    pub missing_if_null: bool,
    /// Prefix to include at the start of each line.
    pub line_prefix: Option<String>,
    /// Whether to expose properties and children of the value.
    pub expandable_value: bool,
    /// Whether to wrap text onto multiple lines.
    pub allow_wrap: bool,
    /// Whether to wrap the name onto multiple lines.
    pub allow_name_wrap: bool,
    /// Hint for how the node should be displayed.
    pub style: Option<DiagnosticsTreeStyle>,
    default_level: DiagnosticLevel,
    /// Whether `if_null` was given by the caller rather than filled from
    /// `missing_if_null`.
    if_null_explicit: bool,
    value: PropertyValue<T>,
}

impl<T> PropertyData<T> {
    fn new(name: Option<String>, value: Option<T>) -> Self {
        assert_node_name(&name);
        PropertyData {
            name,
            description: None,
            if_null: None,
            if_empty: None,
            show_name: true,
            show_separator: true,
            default_value: None,
            tooltip: None,
            missing_if_null: false,
            line_prefix: None,
            expandable_value: false,
            allow_wrap: true,
            allow_name_wrap: true,
            style: Some(DiagnosticsTreeStyle::SingleLine),
            default_level: DiagnosticLevel::Info,
            if_null_explicit: false,
            value: PropertyValue::Direct(value),
        }
    }

    fn lazy(name: Option<String>, compute: ComputePropertyValueCallback<T>) -> Self {
        assert_node_name(&name);
        PropertyData {
            name,
            description: None,
            if_null: None,
            if_empty: None,
            show_name: true,
            show_separator: true,
            default_value: None,
            tooltip: None,
            missing_if_null: false,
            line_prefix: None,
            expandable_value: false,
            allow_wrap: true,
            allow_name_wrap: true,
            style: Some(DiagnosticsTreeStyle::SingleLine),
            default_level: DiagnosticLevel::Info,
            if_null_explicit: false,
            value: PropertyValue::Lazy(LazyValue {
                compute,
                value: OnceCell::new(),
            }),
        }
    }

    /// Returns the value of the property, computing it on first read when the
    /// property was built with `lazy`.
    pub fn value(&self) -> Option<&T> {
        match &self.value {
            PropertyValue::Direct(value) => value.as_ref(),
            PropertyValue::Lazy(lazy) => lazy.value.get_or_init(|| (lazy.compute)()).as_ref(),
        }
    }

    /// Dart's `ifNull`, including `'MISSING'` when `missingIfNull` is set.
    pub fn effective_if_null(&self) -> Option<&str> {
        self.if_null.as_deref()
    }

    fn set_if_null(&mut self, if_null: String) {
        self.if_null = Some(if_null);
        self.if_null_explicit = true;
    }

    fn set_missing_if_null(&mut self, missing_if_null: bool) {
        self.missing_if_null = missing_if_null;
        if missing_if_null {
            if self.if_null.is_none() {
                self.if_null = Some("MISSING".to_string());
            }
        } else if !self.if_null_explicit {
            self.if_null = None;
        }
    }

    /// Additional properties are exposed when the value is itself a node.
    fn expandable_node(&self) -> Option<Rc<dyn DiagnosticsNode>>
    where
        T: Any,
    {
        if !self.expandable_value {
            return None;
        }
        let value = self.value()?;
        let any = value as &dyn Any;
        any.downcast_ref::<Rc<dyn DiagnosticsNode>>().cloned()
    }
}

impl<T> PropertyData<T> {
    fn add_tooltip(&self, text: &str) -> String {
        match &self.tooltip {
            Some(tooltip) => format!("{text} ({tooltip})"),
            None => text.to_string(),
        }
    }

    /// Dart's `DiagnosticsProperty.toDescription`, with the subclass's
    /// `valueToString` supplied by the caller.
    fn to_description_with(
        &self,
        parent_configuration: Option<&TextTreeConfiguration>,
        value_to_string: impl FnOnce(Option<&TextTreeConfiguration>) -> String,
    ) -> String {
        if let Some(description) = &self.description {
            return self.add_tooltip(description);
        }

        if let Some(if_null) = self.effective_if_null()
            && self.value().is_none()
        {
            return self.add_tooltip(if_null);
        }

        let mut result = value_to_string(parent_configuration);
        if result.is_empty()
            && let Some(if_empty) = &self.if_empty
        {
            result = if_empty.clone();
        }
        self.add_tooltip(&result)
    }
}

impl<T: PartialEq> PropertyData<T> {
    /// Whether the value differs from Dart's `defaultValue`.
    pub fn is_interesting(&self) -> bool {
        match &self.default_value {
            None => true,
            Some(default_value) => self.value() != default_value.as_ref(),
        }
    }

    /// Dart's `DiagnosticsProperty.level` before a subclass overrides it.
    pub fn base_level(&self) -> DiagnosticLevel {
        if self.default_level == DiagnosticLevel::Hidden {
            return self.default_level;
        }
        if self.value().is_none() && self.missing_if_null {
            return DiagnosticLevel::Warning;
        }
        if !self.is_interesting() {
            return DiagnosticLevel::Fine;
        }
        self.default_level
    }
}

impl<T: fmt::Display> PropertyData<T> {
    /// Dart's `DiagnosticsProperty.valueToString`.
    pub fn base_value_to_string(
        &self,
        _parent_configuration: Option<&TextTreeConfiguration>,
    ) -> String {
        match self.value() {
            Some(value) => value.to_string(),
            None => "null".to_string(),
        }
    }
}

impl<T: fmt::Display + PartialEq> PropertyData<T> {
    /// Dart's `DiagnosticsProperty.toDescription`.
    pub fn base_to_description(
        &self,
        parent_configuration: Option<&TextTreeConfiguration>,
    ) -> String {
        self.to_description_with(parent_configuration, |parent| {
            self.base_value_to_string(parent)
        })
    }
}

fn assert_node_name(name: &Option<String>) {
    debug_assert!(
        name.as_deref().is_none_or(|name| !name.ends_with(':')),
        "Names of diagnostic nodes must not end with colons.\nname:\n  \"{}\"",
        name.as_deref().unwrap_or("")
    );
}

/// Property with a value of type `T`.
///
/// If the default `value.toString()` does not provide an adequate description
/// of the value, specify a `description`.
pub struct DiagnosticsProperty<T> {
    data: PropertyData<T>,
}

impl<T> DiagnosticsProperty<T> {
    /// The fields and behaviour this property shares with every `DiagnosticsProperty`, Dart's
    /// inherited members.
    pub fn property_data(&self) -> &PropertyData<T> {
        &self.data
    }
}

impl<T> DiagnosticsProperty<T> {
    /// Creates a diagnostics property.
    pub fn new(name: impl IntoPropertyName, value: impl Into<Option<T>>) -> Self {
        let (value, name) = (value.into(), name.into_property_name());
        DiagnosticsProperty {
            data: PropertyData::new(name, value),
        }
    }

    /// Property with a value that is computed only when needed.
    ///
    /// Use if computing the value may panic or is expensive.
    pub fn lazy(
        name: impl IntoPropertyName,
        compute_value: ComputePropertyValueCallback<T>,
    ) -> Self {
        let name = name.into_property_name();
        DiagnosticsProperty {
            data: PropertyData::lazy(name, compute_value),
        }
    }

    /// Dart `DiagnosticsProperty(description:)`.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.data.description = Some(description.into());
        self
    }

    /// Dart `DiagnosticsProperty(ifNull:)`.
    pub fn if_null(mut self, if_null: impl Into<String>) -> Self {
        self.data.set_if_null(if_null.into());
        self
    }

    /// Dart `DiagnosticsProperty(ifEmpty:)`.
    pub fn if_empty(mut self, if_empty: impl Into<String>) -> Self {
        self.data.if_empty = Some(if_empty.into());
        self
    }

    /// Dart `DiagnosticsProperty(showName:)`.
    pub fn with_show_name(mut self, show_name: bool) -> Self {
        self.data.show_name = show_name;
        self
    }

    /// Dart `DiagnosticsProperty(showSeparator:)`.
    pub fn with_show_separator(mut self, show_separator: bool) -> Self {
        self.data.show_separator = show_separator;
        self
    }

    /// Dart `DiagnosticsProperty(defaultValue:)`. A default of `None` is
    /// Dart's `defaultValue: null`.
    pub fn default_value(mut self, default_value: Option<T>) -> Self {
        self.data.default_value = Some(default_value);
        self
    }

    /// Dart `DiagnosticsProperty(tooltip:)`.
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.data.tooltip = Some(tooltip.into());
        self
    }

    /// Dart `DiagnosticsProperty(missingIfNull:)`.
    pub fn missing_if_null(mut self, missing_if_null: bool) -> Self {
        self.data.set_missing_if_null(missing_if_null);
        self
    }

    /// Dart `DiagnosticsProperty(linePrefix:)`.
    pub fn with_line_prefix(mut self, line_prefix: impl Into<String>) -> Self {
        self.data.line_prefix = Some(line_prefix.into());
        self
    }

    /// Dart `DiagnosticsProperty(expandableValue:)`.
    pub fn expandable_value(mut self, expandable_value: bool) -> Self {
        self.data.expandable_value = expandable_value;
        self
    }

    /// Dart `DiagnosticsProperty(allowWrap:)`.
    pub fn with_allow_wrap(mut self, allow_wrap: bool) -> Self {
        self.data.allow_wrap = allow_wrap;
        self
    }

    /// Dart `DiagnosticsProperty(allowNameWrap:)`.
    pub fn with_allow_name_wrap(mut self, allow_name_wrap: bool) -> Self {
        self.data.allow_name_wrap = allow_name_wrap;
        self
    }

    /// Dart `DiagnosticsProperty(style:)`.
    pub fn with_style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.data.style = Some(style);
        self
    }

    /// Dart `DiagnosticsProperty(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.data.default_level = level;
        self
    }

    /// The value of the property, computed on first read for a lazy property.
    pub fn value(&self) -> Option<&T> {
        self.data.value()
    }
}

impl<T> DiagnosticsNode for DiagnosticsProperty<T>
where
    T: fmt::Display + PartialEq + Any,
{
    fn name(&self) -> Option<&str> {
        self.data.name.as_deref()
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.data.base_to_description(parent_configuration)
    }

    fn show_separator(&self) -> bool {
        self.data.show_separator
    }

    fn level(&self) -> DiagnosticLevel {
        self.data.base_level()
    }

    fn show_name(&self) -> bool {
        self.data.show_name
    }

    fn line_prefix(&self) -> Option<&str> {
        self.data.line_prefix.as_deref()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.data.style
    }

    fn allow_wrap(&self) -> bool {
        self.data.allow_wrap
    }

    fn allow_name_wrap(&self) -> bool {
        self.data.allow_name_wrap
    }

    fn value(&self) -> Option<&dyn Any> {
        self.data.value().map(|value| value as &dyn Any)
    }

    fn get_properties(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        self.data
            .expandable_node()
            .map(|node| node.get_properties())
            .unwrap_or_default()
    }

    fn get_children(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        self.data
            .expandable_node()
            .map(|node| node.get_children())
            .unwrap_or_default()
    }

    render_deep_with!();
}

impl<T> fmt::Display for DiagnosticsProperty<T>
where
    T: fmt::Display + PartialEq + Any,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// Debugging message displayed like a property.
pub struct MessageProperty {
    property: DiagnosticsProperty<String>,
}

impl MessageProperty {
    /// The value of the property, always none for a message.
    pub fn value(&self) -> Option<&String> {
        self.property.value()
    }

    /// Create a diagnostics property that displays a message.
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        MessageProperty {
            property: DiagnosticsProperty::new(Some(name.into()), None).description(message),
        }
    }

    /// Dart `MessageProperty(style:)`.
    pub fn with_style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.property = self.property.with_style(style);
        self
    }

    /// Dart `MessageProperty(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.property = self.property.with_level(level);
        self
    }
}

impl MessageProperty {
    /// The fields and behaviour this property shares with every `DiagnosticsProperty`, Dart's
    /// inherited members.
    pub fn property_data(&self) -> &PropertyData<String> {
        self.property.property_data()
    }
}

impl DiagnosticsNode for MessageProperty {
    fn name(&self) -> Option<&str> {
        self.property.name()
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.property.to_description(parent_configuration)
    }

    fn level(&self) -> DiagnosticLevel {
        self.property.level()
    }

    fn show_name(&self) -> bool {
        self.property.show_name()
    }

    fn value(&self) -> Option<&dyn Any> {
        DiagnosticsNode::value(&self.property)
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.property.style()
    }

    fn allow_wrap(&self) -> bool {
        self.property.allow_wrap()
    }

    fn allow_name_wrap(&self) -> bool {
        self.property.allow_name_wrap()
    }

    render_deep_with!();
}

impl fmt::Display for MessageProperty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// Property which encloses its string value in quotes.
pub struct StringProperty {
    data: PropertyData<String>,
    /// Whether the value is enclosed in double quotes.
    pub quoted: bool,
}

impl StringProperty {
    /// The fields and behaviour this property shares with every `DiagnosticsProperty`, Dart's
    /// inherited members.
    pub fn property_data(&self) -> &PropertyData<String> {
        &self.data
    }
}

impl StringProperty {
    /// The value of the property.
    pub fn value(&self) -> Option<&String> {
        self.data.value()
    }

    /// Create a diagnostics property for strings.
    pub fn new(name: impl IntoPropertyName, value: impl Into<Option<String>>) -> Self {
        let (value, name) = (value.into(), name.into_property_name());
        StringProperty {
            data: PropertyData::new(name, value),
            quoted: true,
        }
    }

    /// Dart `StringProperty(description:)`.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.data.description = Some(description.into());
        self
    }

    /// Dart `StringProperty(tooltip:)`.
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.data.tooltip = Some(tooltip.into());
        self
    }

    /// Dart `StringProperty(showName:)`.
    pub fn with_show_name(mut self, show_name: bool) -> Self {
        self.data.show_name = show_name;
        self
    }

    /// Dart `StringProperty(defaultValue:)`.
    pub fn default_value(mut self, default_value: Option<String>) -> Self {
        self.data.default_value = Some(default_value);
        self
    }

    /// Dart `StringProperty(quoted:)`.
    pub fn quoted(mut self, quoted: bool) -> Self {
        self.quoted = quoted;
        self
    }

    /// Dart `StringProperty(ifEmpty:)`.
    pub fn if_empty(mut self, if_empty: impl Into<String>) -> Self {
        self.data.if_empty = Some(if_empty.into());
        self
    }

    /// Dart `StringProperty(style:)`.
    pub fn with_style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.data.style = Some(style);
        self
    }

    /// Dart `StringProperty(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.data.default_level = level;
        self
    }

    /// Dart's `StringProperty.valueToString`.
    pub fn value_to_string(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        let mut text = self
            .data
            .description
            .clone()
            .or_else(|| self.data.value().cloned());
        if let Some(parent) = parent_configuration
            && !parent.line_break_properties
        {
            // Escape line breaks in multiline strings to avoid confusing
            // output when the parent of this node is trying to display all
            // properties on the same line.
            if let Some(value) = &text {
                text = Some(value.replace('\n', "\\n"));
            }
        }

        match text {
            Some(text) => {
                if self.quoted {
                    // An empty value would not appear empty after being
                    // surrounded with quotes so we have to handle this case
                    // separately.
                    if let Some(if_empty) = &self.data.if_empty
                        && text.is_empty()
                    {
                        return if_empty.clone();
                    }
                    format!("\"{text}\"")
                } else {
                    text
                }
            }
            None => "null".to_string(),
        }
    }
}

impl DiagnosticsNode for StringProperty {
    fn name(&self) -> Option<&str> {
        self.data.name.as_deref()
    }

    fn value(&self) -> Option<&dyn Any> {
        self.data.value().map(|value| value as &dyn Any)
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.data
            .to_description_with(parent_configuration, |parent| self.value_to_string(parent))
    }

    fn show_separator(&self) -> bool {
        self.data.show_separator
    }

    fn level(&self) -> DiagnosticLevel {
        self.data.base_level()
    }

    fn show_name(&self) -> bool {
        self.data.show_name
    }

    fn line_prefix(&self) -> Option<&str> {
        self.data.line_prefix.as_deref()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.data.style
    }

    fn allow_wrap(&self) -> bool {
        self.data.allow_wrap
    }

    fn allow_name_wrap(&self) -> bool {
        self.data.allow_name_wrap
    }

    render_deep_with!();
}

impl fmt::Display for StringProperty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// Property describing a `f64` value with an optional unit of measurement.
pub struct DoubleProperty {
    data: PropertyData<f64>,
    /// Optional unit the value is measured in.
    pub unit: Option<String>,
}

impl DoubleProperty {
    /// The fields and behaviour this property shares with every `DiagnosticsProperty`, Dart's
    /// inherited members.
    pub fn property_data(&self) -> &PropertyData<f64> {
        &self.data
    }
}

impl DoubleProperty {
    /// The value of the property.
    pub fn value(&self) -> Option<&f64> {
        self.data.value()
    }

    /// Create a diagnostics property for doubles.
    pub fn new(name: impl IntoPropertyName, value: impl Into<Option<f64>>) -> Self {
        let (value, name) = (value.into(), name.into_property_name());
        DoubleProperty {
            data: PropertyData::new(name, value),
            unit: None,
        }
    }

    /// Property with a value that is computed only when needed.
    pub fn lazy(
        name: impl IntoPropertyName,
        compute_value: ComputePropertyValueCallback<f64>,
    ) -> Self {
        let name = name.into_property_name();
        DoubleProperty {
            data: PropertyData::lazy(name, compute_value),
            unit: None,
        }
    }

    /// Dart `_NumProperty(ifNull:)`.
    pub fn if_null(mut self, if_null: impl Into<String>) -> Self {
        self.data.set_if_null(if_null.into());
        self
    }

    /// Dart `_NumProperty(unit:)`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Dart `_NumProperty(tooltip:)`.
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.data.tooltip = Some(tooltip.into());
        self
    }

    /// Dart `_NumProperty(defaultValue:)`.
    pub fn default_value(mut self, default_value: Option<f64>) -> Self {
        self.data.default_value = Some(default_value);
        self
    }

    /// Dart `_NumProperty(showName:)`.
    pub fn with_show_name(mut self, show_name: bool) -> Self {
        self.data.show_name = show_name;
        self
    }

    /// Dart `_NumProperty(style:)`.
    pub fn with_style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.data.style = Some(style);
        self
    }

    /// Dart `_NumProperty(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.data.default_level = level;
        self
    }

    /// String describing just the numeric value without a unit suffix.
    pub fn number_to_string(&self) -> String {
        debug_format_double(self.data.value().copied())
    }

    /// Dart's `_NumProperty.valueToString`.
    pub fn value_to_string(&self, _parent_configuration: Option<&TextTreeConfiguration>) -> String {
        if self.data.value().is_none() {
            return "null".to_string();
        }
        match &self.unit {
            Some(unit) => format!("{}{unit}", self.number_to_string()),
            None => self.number_to_string(),
        }
    }
}

impl DiagnosticsNode for DoubleProperty {
    fn name(&self) -> Option<&str> {
        self.data.name.as_deref()
    }

    fn value(&self) -> Option<&dyn Any> {
        self.data.value().map(|value| value as &dyn Any)
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.data
            .to_description_with(parent_configuration, |parent| self.value_to_string(parent))
    }

    fn show_separator(&self) -> bool {
        self.data.show_separator
    }

    fn level(&self) -> DiagnosticLevel {
        self.data.base_level()
    }

    fn show_name(&self) -> bool {
        self.data.show_name
    }

    fn line_prefix(&self) -> Option<&str> {
        self.data.line_prefix.as_deref()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.data.style
    }

    fn allow_wrap(&self) -> bool {
        self.data.allow_wrap
    }

    fn allow_name_wrap(&self) -> bool {
        self.data.allow_name_wrap
    }

    render_deep_with!();
}

impl fmt::Display for DoubleProperty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// An int valued property with an optional unit the value is measured in.
pub struct IntProperty<T = i64> {
    data: PropertyData<T>,
    /// Optional unit the value is measured in.
    pub unit: Option<String>,
}

impl<T> IntProperty<T> {
    /// The fields and behaviour this property shares with every `DiagnosticsProperty`, Dart's
    /// inherited members.
    pub fn property_data(&self) -> &PropertyData<T> {
        &self.data
    }
}

impl<T> IntProperty<T> {
    /// The value of the property.
    pub fn value(&self) -> Option<&T> {
        self.data.value()
    }

    /// Create a diagnostics property for integers.
    pub fn new(name: impl IntoPropertyName, value: impl Into<Option<T>>) -> Self {
        let (value, name) = (value.into(), name.into_property_name());
        IntProperty {
            data: PropertyData::new(name, value),
            unit: None,
        }
    }

    /// Dart `_NumProperty(ifNull:)`.
    pub fn if_null(mut self, if_null: impl Into<String>) -> Self {
        self.data.set_if_null(if_null.into());
        self
    }

    /// Dart `_NumProperty(unit:)`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Dart `_NumProperty(defaultValue:)`.
    pub fn default_value(mut self, default_value: Option<T>) -> Self {
        self.data.default_value = Some(default_value);
        self
    }

    /// Dart `_NumProperty(showName:)`.
    pub fn with_show_name(mut self, show_name: bool) -> Self {
        self.data.show_name = show_name;
        self
    }

    /// Dart `_NumProperty(style:)`.
    pub fn with_style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.data.style = Some(style);
        self
    }

    /// Dart `_NumProperty(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.data.default_level = level;
        self
    }

    /// String describing just the numeric value without a unit suffix.
    pub fn number_to_string(&self) -> String
    where
        T: fmt::Display,
    {
        self.data
            .value()
            .map(ToString::to_string)
            .unwrap_or_else(|| "null".to_string())
    }

    /// Dart's `_NumProperty.valueToString`.
    pub fn value_to_string(&self, _parent_configuration: Option<&TextTreeConfiguration>) -> String
    where
        T: fmt::Display,
    {
        if self.data.value().is_none() {
            return "null".to_string();
        }
        match &self.unit {
            Some(unit) => format!("{}{unit}", self.number_to_string()),
            None => self.number_to_string(),
        }
    }
}

impl<T> DiagnosticsNode for IntProperty<T>
where
    T: fmt::Display + PartialEq + Any,
{
    fn name(&self) -> Option<&str> {
        self.data.name.as_deref()
    }

    fn value(&self) -> Option<&dyn Any> {
        self.data.value().map(|value| value as &dyn Any)
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.data
            .to_description_with(parent_configuration, |parent| self.value_to_string(parent))
    }

    fn show_separator(&self) -> bool {
        self.data.show_separator
    }

    fn level(&self) -> DiagnosticLevel {
        self.data.base_level()
    }

    fn show_name(&self) -> bool {
        self.data.show_name
    }

    fn line_prefix(&self) -> Option<&str> {
        self.data.line_prefix.as_deref()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.data.style
    }

    fn allow_wrap(&self) -> bool {
        self.data.allow_wrap
    }

    fn allow_name_wrap(&self) -> bool {
        self.data.allow_name_wrap
    }

    render_deep_with!();
}

impl<T: fmt::Display + PartialEq + Any> fmt::Display for IntProperty<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// Property which clamps a double to between 0 and 1 and formats it as a
/// percentage.
pub struct PercentProperty {
    inner: DoubleProperty,
}

impl std::ops::Deref for PercentProperty {
    type Target = DoubleProperty;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl PercentProperty {
    /// The value of the property.
    pub fn value(&self) -> Option<&f64> {
        self.inner.data.value()
    }

    /// Create a diagnostics property for doubles that represent percentages or
    /// fractions.
    pub fn new(name: impl IntoPropertyName, fraction: impl Into<Option<f64>>) -> Self {
        let (fraction, name) = (fraction.into(), name.into_property_name());
        PercentProperty {
            inner: DoubleProperty::new(name, fraction),
        }
    }

    /// Dart `PercentProperty(ifNull:)`.
    pub fn if_null(mut self, if_null: impl Into<String>) -> Self {
        self.inner = self.inner.if_null(if_null);
        self
    }

    /// Dart `PercentProperty(showName:)`.
    pub fn with_show_name(mut self, show_name: bool) -> Self {
        self.inner = self.inner.with_show_name(show_name);
        self
    }

    /// Dart `PercentProperty(tooltip:)`.
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.inner = self.inner.tooltip(tooltip);
        self
    }

    /// Dart `PercentProperty(unit:)`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.inner = self.inner.unit(unit);
        self
    }

    /// Dart `PercentProperty(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.inner = self.inner.with_level(level);
        self
    }

    /// Dart's `PercentProperty.numberToString`.
    pub fn number_to_string(&self) -> String {
        match self.inner.data.value() {
            None => "null".to_string(),
            Some(value) => {
                let clamped = value.clamp(0.0, 1.0);
                format!("{:.1}%", clamped * 100.0)
            }
        }
    }

    /// Dart's `PercentProperty.valueToString`.
    pub fn value_to_string(&self, _parent_configuration: Option<&TextTreeConfiguration>) -> String {
        if self.inner.data.value().is_none() {
            return "null".to_string();
        }
        match &self.inner.unit {
            Some(unit) => format!("{} {unit}", self.number_to_string()),
            None => self.number_to_string(),
        }
    }
}

impl DiagnosticsNode for PercentProperty {
    fn name(&self) -> Option<&str> {
        self.inner.name()
    }

    fn value(&self) -> Option<&dyn Any> {
        DiagnosticsNode::value(&self.inner)
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.inner
            .data
            .to_description_with(parent_configuration, |parent| self.value_to_string(parent))
    }

    fn show_separator(&self) -> bool {
        self.inner.show_separator()
    }

    fn level(&self) -> DiagnosticLevel {
        self.inner.level()
    }

    fn show_name(&self) -> bool {
        self.inner.show_name()
    }

    fn line_prefix(&self) -> Option<&str> {
        self.inner.line_prefix()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.inner.style()
    }

    fn allow_wrap(&self) -> bool {
        self.inner.allow_wrap()
    }

    fn allow_name_wrap(&self) -> bool {
        self.inner.allow_name_wrap()
    }

    render_deep_with!();
}

impl fmt::Display for PercentProperty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// Property where the description is either `if_true` or `if_false` depending
/// on whether the value is true or false.
pub struct FlagProperty {
    data: PropertyData<bool>,
    /// Description to use if the property value is true.
    pub if_true: Option<String>,
    /// Description to use if the property value is false.
    pub if_false: Option<String>,
}

impl FlagProperty {
    /// The fields and behaviour this property shares with every `DiagnosticsProperty`, Dart's
    /// inherited members.
    pub fn property_data(&self) -> &PropertyData<bool> {
        &self.data
    }
}

impl FlagProperty {
    /// The value of the property.
    pub fn value(&self) -> Option<&bool> {
        self.data.value()
    }

    /// Constructs a FlagProperty with the specified descriptions.
    ///
    /// `show_name` defaults to false as typically `if_true` and `if_false`
    /// should be descriptions that make the property name redundant.
    pub fn new(name: impl IntoPropertyName, value: impl Into<Option<bool>>) -> Self {
        let (value, name) = (value.into(), name.into_property_name());
        FlagProperty {
            data: PropertyData::new(name, value),
            if_true: None,
            if_false: None,
        }
        .with_show_name(false)
    }

    /// Dart `FlagProperty(ifTrue:)`.
    pub fn if_true(mut self, if_true: impl Into<String>) -> Self {
        self.if_true = Some(if_true.into());
        self
    }

    /// Dart `FlagProperty(ifFalse:)`.
    pub fn if_false(mut self, if_false: impl Into<String>) -> Self {
        self.if_false = Some(if_false.into());
        self
    }

    /// Dart `FlagProperty(showName:)`.
    pub fn with_show_name(mut self, show_name: bool) -> Self {
        self.data.show_name = show_name;
        self
    }

    /// Dart `FlagProperty(defaultValue:)`.
    pub fn default_value(mut self, default_value: Option<bool>) -> Self {
        self.data.default_value = Some(default_value);
        self
    }

    /// Dart `FlagProperty(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.data.default_level = level;
        self
    }

    /// Dart's `FlagProperty.valueToString`.
    pub fn value_to_string(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        match self.data.value() {
            Some(true) if self.if_true.is_some() => self.if_true.clone().expect("checked above"),
            Some(false) if self.if_false.is_some() => self.if_false.clone().expect("checked above"),
            _ => self.data.base_value_to_string(parent_configuration),
        }
    }
}

impl DiagnosticsNode for FlagProperty {
    fn name(&self) -> Option<&str> {
        self.data.name.as_deref()
    }

    fn value(&self) -> Option<&dyn Any> {
        self.data.value().map(|value| value as &dyn Any)
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        debug_assert!(
            self.if_true.is_some() || self.if_false.is_some(),
            "ifTrue or ifFalse must be non-null"
        );
        self.data
            .to_description_with(parent_configuration, |parent| self.value_to_string(parent))
    }

    fn show_separator(&self) -> bool {
        self.data.show_separator
    }

    fn level(&self) -> DiagnosticLevel {
        let value = self.data.value();
        if (value == Some(&true) && self.if_true.is_none())
            || (value == Some(&false) && self.if_false.is_none())
        {
            return DiagnosticLevel::Hidden;
        }
        self.data.base_level()
    }

    fn show_name(&self) -> bool {
        let value = self.data.value();
        if value.is_none()
            || (value == Some(&true) && self.if_true.is_none())
            || (value == Some(&false) && self.if_false.is_none())
        {
            // We are missing a description for the flag value so we need to show
            // the flag name. The property will have DiagnosticLevel.hidden in
            // this case.
            return true;
        }
        self.data.show_name
    }

    fn line_prefix(&self) -> Option<&str> {
        self.data.line_prefix.as_deref()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.data.style
    }

    fn allow_wrap(&self) -> bool {
        self.data.allow_wrap
    }

    fn allow_name_wrap(&self) -> bool {
        self.data.allow_name_wrap
    }

    render_deep_with!();
}

impl fmt::Display for FlagProperty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// Property with an `Iterable<T>` value that can be displayed with different
/// [`DiagnosticsTreeStyle`] for custom rendering.
pub struct IterableProperty<T> {
    data: PropertyData<Vec<T>>,
}

impl<T> IterableProperty<T> {
    /// The fields and behaviour this property shares with every `DiagnosticsProperty`, Dart's
    /// inherited members.
    pub fn property_data(&self) -> &PropertyData<Vec<T>> {
        &self.data
    }
}

impl<T> IterableProperty<T> {
    /// The value of the property.
    pub fn value(&self) -> Option<&Vec<T>> {
        self.data.value()
    }

    /// Create a diagnostics property for iterables (e.g. lists).
    ///
    /// The `if_empty` argument indicates how an iterable value with 0 elements
    /// is displayed.
    pub fn new(name: impl IntoPropertyName, value: impl Into<Option<Vec<T>>>) -> Self {
        let (value, name) = (value.into(), name.into_property_name());
        IterableProperty {
            data: PropertyData::new(name, value),
        }
        .if_empty("[]")
    }

    /// Dart `IterableProperty(defaultValue:)`.
    pub fn default_value(mut self, default_value: Option<Vec<T>>) -> Self {
        self.data.default_value = Some(default_value);
        self
    }

    /// Dart `IterableProperty(ifNull:)`.
    pub fn if_null(mut self, if_null: impl Into<String>) -> Self {
        self.data.set_if_null(if_null.into());
        self
    }

    /// Dart `IterableProperty(ifEmpty:)`.
    pub fn if_empty(mut self, if_empty: impl Into<String>) -> Self {
        self.data.if_empty = Some(if_empty.into());
        self
    }

    /// Dart `IterableProperty(style:)`.
    pub fn with_style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.data.style = Some(style);
        self
    }

    /// Dart `IterableProperty(showName:)`.
    pub fn with_show_name(mut self, show_name: bool) -> Self {
        self.data.show_name = show_name;
        self
    }

    /// Dart `IterableProperty(showSeparator:)`.
    pub fn with_show_separator(mut self, show_separator: bool) -> Self {
        self.data.show_separator = show_separator;
        self
    }

    /// Dart `IterableProperty(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.data.default_level = level;
        self
    }

    /// Dart's `IterableProperty.valueToString`.
    pub fn value_to_string(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String
    where
        T: fmt::Display + Any,
    {
        let Some(values) = self.data.value() else {
            return "null".to_string();
        };

        if values.is_empty() {
            return self
                .data
                .if_empty
                .clone()
                .unwrap_or_else(|| "[]".to_string());
        }

        let formatted_values: Vec<String> = values
            .iter()
            .map(|value| {
                if std::any::TypeId::of::<T>() == std::any::TypeId::of::<f64>() {
                    let any = value as &dyn Any;
                    debug_format_double(any.downcast_ref::<f64>().copied())
                } else {
                    value.to_string()
                }
            })
            .collect();

        if let Some(parent) = parent_configuration
            && !parent.line_break_properties
        {
            // Always display the value as a single line and enclose the
            // iterable value in brackets to avoid ambiguity.
            return format!("[{}]", formatted_values.join(", "));
        }

        formatted_values.join(if is_single_line(self.data.style) {
            ", "
        } else {
            "\n"
        })
    }
}

impl<T> DiagnosticsNode for IterableProperty<T>
where
    T: fmt::Display + PartialEq + Any,
{
    fn name(&self) -> Option<&str> {
        self.data.name.as_deref()
    }

    fn value(&self) -> Option<&dyn Any> {
        self.data.value().map(|value| value as &dyn Any)
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.data
            .to_description_with(parent_configuration, |parent| self.value_to_string(parent))
    }

    fn show_separator(&self) -> bool {
        self.data.show_separator
    }

    fn level(&self) -> DiagnosticLevel {
        let value = self.data.value();
        if self.data.if_empty.is_none()
            && value.is_some_and(|value| value.is_empty())
            && self.data.base_level() != DiagnosticLevel::Hidden
        {
            return DiagnosticLevel::Fine;
        }
        self.data.base_level()
    }

    fn show_name(&self) -> bool {
        self.data.show_name
    }

    fn line_prefix(&self) -> Option<&str> {
        self.data.line_prefix.as_deref()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.data.style
    }

    fn allow_wrap(&self) -> bool {
        self.data.allow_wrap
    }

    fn allow_name_wrap(&self) -> bool {
        self.data.allow_name_wrap
    }

    render_deep_with!();
}

impl<T> fmt::Display for IterableProperty<T>
where
    T: fmt::Display + PartialEq + Any,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// [`DiagnosticsProperty`] that has an enum as value.
///
/// The enum value is displayed with its variant name, first character
/// lowercased, the way Dart's `Enum.name` prints `deferToChild`.
pub struct EnumProperty<T> {
    data: PropertyData<T>,
}

impl<T> EnumProperty<T> {
    /// The fields and behaviour this property shares with every `DiagnosticsProperty`, Dart's
    /// inherited members.
    pub fn property_data(&self) -> &PropertyData<T> {
        &self.data
    }
}

impl<T> EnumProperty<T> {
    /// The value of the property.
    pub fn value(&self) -> Option<&T> {
        self.data.value()
    }

    /// Create a diagnostics property that displays an enum.
    pub fn new(name: impl IntoPropertyName, value: impl Into<Option<T>>) -> Self {
        let (value, name) = (value.into(), name.into_property_name());
        EnumProperty {
            data: PropertyData::new(name, value),
        }
    }

    /// Dart `EnumProperty(defaultValue:)`.
    pub fn default_value(mut self, default_value: Option<T>) -> Self {
        self.data.default_value = Some(default_value);
        self
    }

    /// Dart `EnumProperty(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.data.default_level = level;
        self
    }
}

impl<T> DiagnosticsNode for EnumProperty<T>
where
    T: fmt::Debug + PartialEq + Any,
{
    fn name(&self) -> Option<&str> {
        self.data.name.as_deref()
    }

    fn value(&self) -> Option<&dyn Any> {
        self.data.value().map(|value| value as &dyn Any)
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.data
            .to_description_with(parent_configuration, |_| match self.data.value() {
                Some(value) => describe_enum(value),
                None => "null".to_string(),
            })
    }

    fn show_separator(&self) -> bool {
        self.data.show_separator
    }

    fn level(&self) -> DiagnosticLevel {
        self.data.base_level()
    }

    fn show_name(&self) -> bool {
        self.data.show_name
    }

    fn line_prefix(&self) -> Option<&str> {
        self.data.line_prefix.as_deref()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.data.style
    }

    fn allow_wrap(&self) -> bool {
        self.data.allow_wrap
    }

    fn allow_name_wrap(&self) -> bool {
        self.data.allow_name_wrap
    }

    render_deep_with!();
}

impl<T> fmt::Display for EnumProperty<T>
where
    T: fmt::Debug + PartialEq + Any,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// A property where the important diagnostic information is primarily whether
/// the value is present (non-null) or absent (null).
pub struct ObjectFlagProperty<T> {
    data: PropertyData<T>,
    /// Description to use if the property value is not null.
    pub if_present: Option<String>,
}

impl<T> ObjectFlagProperty<T> {
    /// The fields and behaviour this property shares with every `DiagnosticsProperty`, Dart's
    /// inherited members.
    pub fn property_data(&self) -> &PropertyData<T> {
        &self.data
    }
}

impl<T> ObjectFlagProperty<T> {
    /// The value of the property.
    pub fn value(&self) -> Option<&T> {
        self.data.value()
    }

    /// Create a diagnostics property for values that can be present (non-null)
    /// or absent (null).
    pub fn new(name: impl IntoPropertyName, value: impl Into<Option<T>>) -> Self {
        let (value, name) = (value.into(), name.into_property_name());
        ObjectFlagProperty {
            data: PropertyData::new(name, value),
            if_present: None,
        }
        .with_show_name(false)
    }

    /// Shorthand constructor to describe whether the property has a value.
    ///
    /// Only use if prefixing the property name with the word 'has' is a good
    /// flag name.
    pub fn has(name: impl Into<String>, value: Option<T>) -> Self {
        let name = name.into();
        ObjectFlagProperty {
            data: PropertyData::new(Some(name.clone()), value),
            if_present: Some(format!("has {name}")),
        }
        .with_show_name(false)
    }

    /// Dart `ObjectFlagProperty(ifPresent:)`.
    pub fn if_present(mut self, if_present: impl Into<String>) -> Self {
        self.if_present = Some(if_present.into());
        self
    }

    /// Dart `ObjectFlagProperty(ifNull:)`.
    pub fn if_null(mut self, if_null: impl Into<String>) -> Self {
        self.data.set_if_null(if_null.into());
        self
    }

    /// Dart `ObjectFlagProperty(showName:)`.
    pub fn with_show_name(mut self, show_name: bool) -> Self {
        self.data.show_name = show_name;
        self
    }

    /// Dart `ObjectFlagProperty(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.data.default_level = level;
        self
    }

    /// Dart's `ObjectFlagProperty.valueToString`.
    pub fn value_to_string(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String
    where
        T: fmt::Display,
    {
        match self.data.value() {
            Some(_) if self.if_present.is_some() => self.if_present.clone().expect("checked above"),
            None if self.data.if_null.is_some() => {
                self.data.if_null.clone().expect("checked above")
            }
            _ => self.data.base_value_to_string(parent_configuration),
        }
    }
}

impl<T> DiagnosticsNode for ObjectFlagProperty<T>
where
    T: fmt::Display + Any,
{
    fn name(&self) -> Option<&str> {
        self.data.name.as_deref()
    }

    fn value(&self) -> Option<&dyn Any> {
        self.data.value().map(|value| value as &dyn Any)
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        debug_assert!(
            self.if_present.is_some() || self.data.if_null.is_some(),
            "ifPresent or ifNull must be non-null"
        );
        self.data
            .to_description_with(parent_configuration, |parent| self.value_to_string(parent))
    }

    fn show_separator(&self) -> bool {
        self.data.show_separator
    }

    fn level(&self) -> DiagnosticLevel {
        let value = self.data.value();
        if (value.is_some() && self.if_present.is_none())
            || (value.is_none() && self.data.if_null.is_none())
        {
            return DiagnosticLevel::Hidden;
        }
        self.data.default_level
    }

    fn show_name(&self) -> bool {
        let value = self.data.value();
        if (value.is_some() && self.if_present.is_none())
            || (value.is_none() && self.data.if_null.is_none())
        {
            // We are missing a description for the flag value so we need to show
            // the flag name. The property will have DiagnosticLevel.hidden in
            // this case.
            return true;
        }
        self.data.show_name
    }

    fn line_prefix(&self) -> Option<&str> {
        self.data.line_prefix.as_deref()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.data.style
    }

    fn allow_wrap(&self) -> bool {
        self.data.allow_wrap
    }

    fn allow_name_wrap(&self) -> bool {
        self.data.allow_name_wrap
    }

    render_deep_with!();
}

impl<T> fmt::Display for ObjectFlagProperty<T>
where
    T: fmt::Display + Any,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// A summary of multiple properties, indicating whether each of them is present
/// (non-null) or absent (null).
pub struct FlagsSummary<T> {
    data: PropertyData<IndexMap<String, Option<T>>>,
}

impl<T> FlagsSummary<T> {
    /// Create a summary for multiple properties.
    pub fn new(name: impl IntoPropertyName, value: IndexMap<String, Option<T>>) -> Self {
        let name = name.into_property_name();
        FlagsSummary {
            data: PropertyData::new(name, Some(value)),
        }
    }

    /// Dart `FlagsSummary(ifEmpty:)`.
    pub fn if_empty(mut self, if_empty: impl Into<String>) -> Self {
        self.data.if_empty = Some(if_empty.into());
        self
    }

    /// Dart `FlagsSummary(showName:)`.
    pub fn with_show_name(mut self, show_name: bool) -> Self {
        self.data.show_name = show_name;
        self
    }

    /// Dart `FlagsSummary(showSeparator:)`.
    pub fn with_show_separator(mut self, show_separator: bool) -> Self {
        self.data.show_separator = show_separator;
        self
    }

    /// Dart `FlagsSummary(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.data.default_level = level;
        self
    }

    /// Dart's `FlagsSummary.value`: the map itself, never null.
    pub fn value(&self) -> &IndexMap<String, Option<T>> {
        self.data
            .value()
            .expect("a FlagsSummary is built with a map")
    }

    fn has_non_null_entry(&self) -> bool {
        self.value().values().any(Option::is_some)
    }

    fn formatted_values(&self) -> impl Iterator<Item = &String> {
        self.value()
            .iter()
            .filter_map(|(key, value)| value.is_some().then_some(key))
    }

    /// Dart's `FlagsSummary.valueToString`.
    pub fn value_to_string(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        if !self.has_non_null_entry()
            && let Some(if_empty) = &self.data.if_empty
        {
            return if_empty.clone();
        }

        let formatted_values: Vec<&String> = self.formatted_values().collect();
        if let Some(parent) = parent_configuration
            && !parent.line_break_properties
        {
            // Always display the value as a single line and enclose the
            // iterable value in brackets to avoid ambiguity.
            return format!(
                "[{}]",
                formatted_values
                    .iter()
                    .map(|value| value.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        formatted_values
            .iter()
            .map(|value| value.as_str())
            .collect::<Vec<_>>()
            .join(if is_single_line(self.data.style) {
                ", "
            } else {
                "\n"
            })
    }
}

impl<T> DiagnosticsNode for FlagsSummary<T>
where
    T: Any,
{
    fn name(&self) -> Option<&str> {
        self.data.name.as_deref()
    }

    fn value(&self) -> Option<&dyn Any> {
        self.data.value().map(|value| value as &dyn Any)
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.data
            .to_description_with(parent_configuration, |parent| self.value_to_string(parent))
    }

    fn show_separator(&self) -> bool {
        self.data.show_separator
    }

    fn level(&self) -> DiagnosticLevel {
        if !self.has_non_null_entry() && self.data.if_empty.is_none() {
            return DiagnosticLevel::Hidden;
        }
        self.data.default_level
    }

    fn show_name(&self) -> bool {
        self.data.show_name
    }

    fn line_prefix(&self) -> Option<&str> {
        self.data.line_prefix.as_deref()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.data.style
    }

    fn allow_wrap(&self) -> bool {
        self.data.allow_wrap
    }

    fn allow_name_wrap(&self) -> bool {
        self.data.allow_name_wrap
    }

    render_deep_with!();
}

impl<T> fmt::Display for FlagsSummary<T>
where
    T: Any,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// Builder to accumulate properties and configuration used to assemble a
/// [`DiagnosticsNode`] from a [`Diagnosticable`] object.
pub struct DiagnosticPropertiesBuilder {
    /// List of properties accumulated so far.
    pub properties: Vec<Rc<dyn DiagnosticsNode>>,
    /// Default style to use for the node if no style is specified.
    pub default_diagnostics_tree_style: DiagnosticsTreeStyle,
    /// Description to show if the node has no displayed properties or children.
    pub empty_body_description: Option<String>,
}

impl DiagnosticPropertiesBuilder {
    /// Creates a builder with no properties.
    pub fn new() -> Self {
        DiagnosticPropertiesBuilder {
            properties: Vec::new(),
            default_diagnostics_tree_style: DiagnosticsTreeStyle::Sparse,
            empty_body_description: None,
        }
    }

    /// Creates a builder with the given properties.
    pub fn from_properties(properties: Vec<Rc<dyn DiagnosticsNode>>) -> Self {
        DiagnosticPropertiesBuilder {
            properties,
            default_diagnostics_tree_style: DiagnosticsTreeStyle::Sparse,
            empty_body_description: None,
        }
    }

    /// Add a property to the list of properties.
    pub fn add(&mut self, property: impl DiagnosticsNode + 'static) {
        self.properties.push(Rc::new(property));
    }

    /// Add a property that is already shared.
    pub fn add_rc(&mut self, property: Rc<dyn DiagnosticsNode>) {
        self.properties.push(property);
    }
}

impl Default for DiagnosticPropertiesBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// A trait for objects that can describe themselves with string and
/// [`DiagnosticsNode`] representations.
///
/// Dart's `Diagnosticable`. Its methods do not take `App`: Flutter's
/// `debugFillProperties` reads only the object's own fields. Arena values that
/// need children are read through [`DiagnosticableTree`].
pub trait Diagnosticable: Sized {
    /// Dart's `Object.runtimeType` as diagnostics print it: the last segment of
    /// the Rust type path, without generic arguments.
    fn type_name(&self) -> &'static str {
        short_type_name(std::any::type_name::<Self>())
    }

    /// Dart's `Object.hashCode` as diagnostics print it: the object's address.
    fn identity_hash(&self) -> u64 {
        self as *const Self as *const () as usize as u64
    }

    /// A brief description of this object, usually just the type and the hash.
    fn to_string_short(&self) -> String {
        describe_identity(self.type_name(), self.identity_hash())
    }

    /// Add additional properties associated with the node.
    fn debug_fill_properties(&self, properties: &mut DiagnosticPropertiesBuilder) {
        let _ = properties;
    }

    /// Returns a debug representation of the object used by debugging tools and
    /// by [`DiagnosticsNode::to_string_deep`].
    fn to_diagnostics_node(&self) -> DiagnosticableNode {
        DiagnosticableNode::new(None, self, None)
    }

    /// Dart `toDiagnosticsNode(name:, style:)`.
    fn to_diagnostics_node_with(
        &self,
        name: Option<String>,
        style: Option<DiagnosticsTreeStyle>,
    ) -> DiagnosticableNode {
        DiagnosticableNode::new(name, self, style)
    }

    /// Dart's `toString`: a single-line description with properties and no
    /// children.
    ///
    /// A type that also implements `Display` must call this as
    /// `Diagnosticable::to_string` because Rust reports the two methods as
    /// ambiguous.
    fn to_string(&self) -> String {
        self.to_string_at(DiagnosticLevel::Info)
    }

    /// Dart's `toString(minLevel:)`.
    fn to_string_at(&self, min_level: DiagnosticLevel) -> String {
        self.to_diagnostics_node_with(None, Some(DiagnosticsTreeStyle::SingleLine))
            .to_string_with(None, min_level)
    }
}

/// A trait for objects that can describe their children as well as themselves.
///
/// Dart's `DiagnosticableTree`. `to_diagnostics_tree_node` is Dart's
/// `toDiagnosticsNode` override; it is renamed because Rust reports a call to
/// two same-named trait methods as ambiguous.
pub trait DiagnosticableTree: Diagnosticable {
    /// Returns a node whose properties and children are collected from `self`.
    fn to_diagnostics_tree_node(
        &self,
        name: Option<String>,
        style: Option<DiagnosticsTreeStyle>,
    ) -> DiagnosticableTreeNode {
        DiagnosticableTreeNode::new(name, self, style)
    }

    /// Returns a list of nodes describing this node's children.
    fn debug_describe_children(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        Vec::new()
    }

    /// Returns a one-line detailed description of the object, including the
    /// same information as [`to_string_deep`](Self::to_string_deep) but without
    /// recursing to children.
    fn to_string_shallow(&self, joiner: &str, min_level: DiagnosticLevel) -> String {
        let mut result = self.to_string_short();
        result.push_str(joiner);
        let mut builder = DiagnosticPropertiesBuilder::new();
        self.debug_fill_properties(&mut builder);
        let properties: Vec<String> = builder
            .properties
            .iter()
            .filter(|node| !node.is_filtered(min_level))
            .map(ToString::to_string)
            .collect();
        result.push_str(&properties.join(joiner));
        result
    }

    /// Returns a string representation of this node and its descendants with
    /// Dart's default arguments.
    fn to_string_deep(&self) -> String {
        self.to_string_deep_with("", None, DiagnosticLevel::Debug, 65)
    }

    /// Returns a string representation of this node and its descendants.
    fn to_string_deep_with(
        &self,
        prefix_line_one: &str,
        prefix_other_lines: Option<&str>,
        min_level: DiagnosticLevel,
        wrap_width: usize,
    ) -> String {
        self.to_diagnostics_tree_node(None, None)
            .to_string_deep_with(
                prefix_line_one,
                prefix_other_lines,
                None,
                min_level,
                wrap_width,
            )
    }
}

/// [`DiagnosticsNode`] that describes a [`Diagnosticable`] value.
///
/// Flutter keeps the value and fills its builder on first read. Waft builds the
/// node eagerly: the value lives in an arena and is read with `&App`, so
/// [`from_parts`](Self::from_parts) takes the pieces the caller already
/// collected.
pub struct DiagnosticableNode {
    name: Option<String>,
    description: String,
    style: DiagnosticsTreeStyle,
    properties: Vec<Rc<dyn DiagnosticsNode>>,
    empty_body_description: Option<String>,
}

impl DiagnosticableNode {
    /// Creates a node for `value`, filling its properties eagerly.
    pub fn new(
        name: impl IntoPropertyName,
        value: &impl Diagnosticable,
        style: Option<DiagnosticsTreeStyle>,
    ) -> Self {
        let name = name.into_property_name();
        let mut builder = DiagnosticPropertiesBuilder::new();
        value.debug_fill_properties(&mut builder);
        let mut node = Self::from_parts(name, value.to_string_short(), builder);
        if let Some(style) = style {
            node.style = style;
        }
        node
    }

    /// Creates a node from a description, properties and style already
    /// collected from a value.
    ///
    /// This is how an arena object's node is built: read the object with
    /// `&App`, fill a [`DiagnosticPropertiesBuilder`], and pass both.
    pub fn from_parts(
        name: impl IntoPropertyName,
        description: impl Into<String>,
        builder: DiagnosticPropertiesBuilder,
    ) -> Self {
        let name = name.into_property_name();
        assert_node_name(&name);
        let DiagnosticPropertiesBuilder {
            properties,
            default_diagnostics_tree_style,
            empty_body_description,
        } = builder;
        DiagnosticableNode {
            name,
            description: description.into(),
            style: default_diagnostics_tree_style,
            properties,
            empty_body_description,
        }
    }

    /// Dart `DiagnosticableNode(style:)`: the node drawn in `style` rather than the one its
    /// properties asked for.
    pub fn with_style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.style = style;
        self
    }

    /// Dart `DiagnosticableNode(emptyBodyDescription:)`, from the builder.
    pub fn with_empty_body_description(mut self, description: impl Into<String>) -> Self {
        self.empty_body_description = Some(description.into());
        self
    }

    /// The node's short description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The node's collected properties.
    pub fn properties(&self) -> &[Rc<dyn DiagnosticsNode>] {
        &self.properties
    }
}

impl DiagnosticsNode for DiagnosticableNode {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn to_description(&self, _parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.description.clone()
    }

    fn empty_body_description(&self) -> Option<&str> {
        self.empty_body_description.as_deref()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        Some(self.style)
    }

    fn get_properties(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        self.properties.clone()
    }

    render_deep_with!();
}

impl fmt::Display for DiagnosticableNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// [`DiagnosticsNode`] for an instance of [`DiagnosticableTree`].
pub struct DiagnosticableTreeNode {
    inner: DiagnosticableNode,
    children: Vec<Rc<dyn DiagnosticsNode>>,
}

impl DiagnosticableTreeNode {
    /// Creates a node for `value`, filling its properties and children eagerly.
    pub fn new(
        name: impl IntoPropertyName,
        value: &impl DiagnosticableTree,
        style: Option<DiagnosticsTreeStyle>,
    ) -> Self {
        let name = name.into_property_name();
        let mut builder = DiagnosticPropertiesBuilder::new();
        value.debug_fill_properties(&mut builder);
        let mut inner = DiagnosticableNode::from_parts(name, value.to_string_short(), builder);
        if let Some(style) = style {
            inner.style = style;
        }
        DiagnosticableTreeNode {
            inner,
            children: value.debug_describe_children(),
        }
    }

    /// Creates a tree node from a description, properties, style and children
    /// already collected from a value.
    ///
    /// This is how an arena object's node is built: read the object and its
    /// children with `&App`, fill a [`DiagnosticPropertiesBuilder`], and pass
    /// both.
    pub fn from_parts(
        name: impl IntoPropertyName,
        description: impl Into<String>,
        builder: DiagnosticPropertiesBuilder,
        children: Vec<Rc<dyn DiagnosticsNode>>,
    ) -> Self {
        let name = name.into_property_name();
        DiagnosticableTreeNode {
            inner: DiagnosticableNode::from_parts(name, description, builder),
            children,
        }
    }

    /// Sets the node's style, as Dart's `toDiagnosticsNode(style:)` does.
    pub fn with_style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.inner.style = style;
        self
    }

    /// The node's short description.
    pub fn description(&self) -> &str {
        self.inner.description()
    }

    /// The node's collected properties.
    pub fn properties(&self) -> &[Rc<dyn DiagnosticsNode>] {
        self.inner.properties()
    }

    /// The node's children.
    pub fn children(&self) -> &[Rc<dyn DiagnosticsNode>] {
        &self.children
    }
}

impl DiagnosticsNode for DiagnosticableTreeNode {
    fn name(&self) -> Option<&str> {
        self.inner.name()
    }

    fn to_description(&self, parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.inner.to_description(parent_configuration)
    }

    fn show_separator(&self) -> bool {
        self.inner.show_separator()
    }

    fn level(&self) -> DiagnosticLevel {
        self.inner.level()
    }

    fn show_name(&self) -> bool {
        self.inner.show_name()
    }

    fn line_prefix(&self) -> Option<&str> {
        self.inner.line_prefix()
    }

    fn empty_body_description(&self) -> Option<&str> {
        self.inner.empty_body_description()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.inner.style()
    }

    fn allow_wrap(&self) -> bool {
        self.inner.allow_wrap()
    }

    fn allow_name_wrap(&self) -> bool {
        self.inner.allow_name_wrap()
    }

    fn allow_truncate(&self) -> bool {
        self.inner.allow_truncate()
    }

    fn get_properties(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        self.inner.get_properties()
    }

    fn get_children(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        self.children.clone()
    }

    render_deep_with!();
}

impl fmt::Display for DiagnosticableTreeNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// [`DiagnosticsNode`] that exists mainly to provide a container for other
/// diagnostics that typically lacks a meaningful value of its own.
pub struct DiagnosticsBlock {
    /// Label describing the node.
    pub name: Option<String>,
    /// Hint for how the node should be displayed.
    pub style: Option<DiagnosticsTreeStyle>,
    show_name: bool,
    /// Whether a separator is shown between the name and description.
    pub show_separator: bool,
    /// Prefix to include at the start of each line.
    pub line_prefix: Option<String>,
    /// The actual object this diagnostics data is for.
    pub value: Option<Box<dyn Any>>,
    description: String,
    /// Priority level of the diagnostic.
    pub level: DiagnosticLevel,
    /// Whether to allow truncation when displaying the node and its children.
    pub allow_truncate: bool,
    children: Vec<Rc<dyn DiagnosticsNode>>,
    properties: Vec<Rc<dyn DiagnosticsNode>>,
}

impl DiagnosticsBlock {
    /// Creates a diagnostic with no properties or children.
    pub fn new() -> Self {
        DiagnosticsBlock {
            name: None,
            style: Some(DiagnosticsTreeStyle::Whitespace),
            show_name: true,
            show_separator: true,
            line_prefix: None,
            value: None,
            description: String::new(),
            level: DiagnosticLevel::Info,
            allow_truncate: false,
            children: Vec::new(),
            properties: Vec::new(),
        }
    }

    /// Dart `DiagnosticsBlock(name:)`.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        assert_node_name(&Some(name.clone()));
        self.name = Some(name);
        self
    }

    /// Dart `DiagnosticsBlock(style:)`.
    pub fn with_style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Dart `DiagnosticsBlock(showName:)`.
    pub fn with_show_name(mut self, show_name: bool) -> Self {
        self.show_name = show_name;
        self
    }

    /// Dart `DiagnosticsBlock(showSeparator:)`.
    pub fn with_show_separator(mut self, show_separator: bool) -> Self {
        self.show_separator = show_separator;
        self
    }

    /// Dart `DiagnosticsBlock(linePrefix:)`.
    pub fn with_line_prefix(mut self, line_prefix: impl Into<String>) -> Self {
        self.line_prefix = Some(line_prefix.into());
        self
    }

    /// Dart `DiagnosticsBlock(value:)`.
    pub fn value(mut self, value: impl Any) -> Self {
        self.value = Some(Box::new(value));
        self
    }

    /// Dart `DiagnosticsBlock(description:)`.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Dart `DiagnosticsBlock(level:)`.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.level = level;
        self
    }

    /// Dart `DiagnosticsBlock(allowTruncate:)`.
    pub fn allow_truncate(mut self, allow_truncate: bool) -> Self {
        self.allow_truncate = allow_truncate;
        self
    }

    /// Dart `DiagnosticsBlock(children:)`.
    pub fn children(mut self, children: Vec<Rc<dyn DiagnosticsNode>>) -> Self {
        self.children = children;
        self
    }

    /// Dart `DiagnosticsBlock(properties:)`.
    pub fn properties(mut self, properties: Vec<Rc<dyn DiagnosticsNode>>) -> Self {
        self.properties = properties;
        self
    }
}

impl Default for DiagnosticsBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticsNode for DiagnosticsBlock {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn value(&self) -> Option<&dyn Any> {
        self.value.as_deref()
    }

    fn to_description(&self, _parent_configuration: Option<&TextTreeConfiguration>) -> String {
        self.description.clone()
    }

    fn show_separator(&self) -> bool {
        self.show_separator
    }

    fn level(&self) -> DiagnosticLevel {
        self.level
    }

    fn show_name(&self) -> bool {
        self.show_name && self.name.is_some()
    }

    fn line_prefix(&self) -> Option<&str> {
        self.line_prefix.as_deref()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        self.style
    }

    fn allow_truncate(&self) -> bool {
        self.allow_truncate
    }

    fn get_properties(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        self.properties.clone()
    }

    fn get_children(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        self.children.clone()
    }

    render_deep_with!();
}

impl fmt::Display for DiagnosticsBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_with(None, DiagnosticLevel::Info))
    }
}

/// A diagnostics node containing only a string `message`, with no name or
/// value.
///
/// Dart's `DiagnosticsNode.message` factory.
pub fn message(message: impl Into<String>) -> DiagnosticsProperty<String> {
    DiagnosticsProperty::new(Some(String::new()), None)
        .description(message)
        .with_show_name(false)
}

/// Returns a 5 character long hexadecimal string generated from the 20
/// least-significant bits of `hash_code`.
pub fn short_hash(hash_code: u64) -> String {
    format!("{:05x}", hash_code & 0xfffff)
}

/// Returns a summary of the runtime type and hash code of an object.
pub fn describe_identity(runtime_type: &str, hash_code: u64) -> String {
    format!("{runtime_type}#{}", short_hash(hash_code))
}

/// The last segment of a Rust type path, without generic arguments: what
/// diagnostics print where Dart prints `runtimeType`.
pub fn short_type_name(full_type_name: &'static str) -> &'static str {
    let last = full_type_name.rsplit("::").next().unwrap_or(full_type_name);
    last.split('<').next().unwrap_or(last)
}

/// Returns a short description of an enum value: its `Debug` text with the
/// first character lowercased, as Dart's `Enum.name` prints `deferToChild`.
pub fn describe_enum<T: fmt::Debug>(value: &T) -> String {
    let description = format!("{value:?}");
    let mut chars = description.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut result: String = first.to_lowercase().collect();
            result.push_str(chars.as_str());
            result
        }
    }
}

/// Dart's `debugFormatDouble`: one decimal place, and `Infinity` rather than
/// Rust's `inf`.
pub fn debug_format_double(value: Option<f64>) -> String {
    match value {
        None => "null".to_string(),
        Some(value) if value.is_nan() => "NaN".to_string(),
        Some(value) if value == f64::INFINITY => "Infinity".to_string(),
        Some(value) if value == f64::NEG_INFINITY => "-Infinity".to_string(),
        Some(value) => format!("{value:.1}"),
    }
}

/// Dart's string interpolation of a nullable value, `'$value'`: the value's `Display`, or
/// `null`.
pub fn display_or_null<T: fmt::Display>(value: &Option<T>) -> impl fmt::Display + '_ {
    DisplayOrNull(value)
}

struct DisplayOrNull<'a, T>(&'a Option<T>);

impl<T: fmt::Display> fmt::Display for DisplayOrNull<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(value) => value.fmt(f),
            None => f.write_str("null"),
        }
    }
}

#[cfg(test)]
mod tests;
