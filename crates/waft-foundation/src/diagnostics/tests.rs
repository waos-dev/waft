use super::*;

fn node(node: impl DiagnosticsNode + 'static) -> Rc<dyn DiagnosticsNode> {
    Rc::new(node)
}

/// Flutter's `equalsIgnoringHashCodes`: every five hex digits after `#`
/// become `0`.
fn normalize_hash_codes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '#'
            && index + 5 < chars.len()
            && chars[index + 1..index + 6]
                .iter()
                .all(|c| c.is_ascii_hexdigit())
        {
            result.push_str("#00000");
            index += 6;
        } else {
            result.push(chars[index]);
            index += 1;
        }
    }
    result
}

fn assert_deep(actual: &str, expected: &str) {
    assert_eq!(
        normalize_hash_codes(actual),
        normalize_hash_codes(expected),
        "\n--- actual ---\n{actual}\n--- expected ---\n{expected}\n"
    );
}

/// Flutter's `simulateJsonSerialization`: encode and decode, so that every value in the map is one
/// a client can receive.
fn simulate_json_serialization(
    node: &dyn DiagnosticsNode,
    delegate: &dyn DiagnosticsSerializationDelegate,
) -> DiagnosticsJsonMap {
    let json = node.to_json_map(delegate);
    let encoded = serde_json::to_string(&json).expect("a diagnostics node serializes to JSON");
    serde_json::from_str(&encoded).expect("the encoded JSON reads back")
}

/// [`simulate_json_serialization`] with Dart's default delegate.
fn serialize(node: &dyn DiagnosticsNode) -> DiagnosticsJsonMap {
    simulate_json_serialization(
        node,
        &DefaultDiagnosticsSerializationDelegate::new(0, false),
    )
}

fn json_str<'a>(json: &'a DiagnosticsJsonMap, key: &str) -> Option<&'a str> {
    json.get(key).and_then(Value::as_str)
}

fn json_bool(json: &DiagnosticsJsonMap, key: &str) -> Option<bool> {
    json.get(key).and_then(Value::as_bool)
}

fn json_len(json: &DiagnosticsJsonMap, key: &str) -> Option<usize> {
    json.get(key).and_then(Value::as_array).map(Vec::len)
}

fn json_strings<'a>(json: &'a DiagnosticsJsonMap, key: &str) -> Option<Vec<&'a str>> {
    json.get(key).and_then(Value::as_array).map(|values| {
        values
            .iter()
            .map(|value| value.as_str().expect("a list of strings"))
            .collect()
    })
}

/// Flutter's `validateNodeJsonSerializationHelper`.
fn validate_node_json_map(json: &DiagnosticsJsonMap, node: &dyn DiagnosticsNode) {
    let description = node.to_description(None);
    assert_eq!(json_str(json, "name"), node.name());
    assert_eq!(
        json_bool(json, "showSeparator").unwrap_or(true),
        node.show_separator()
    );
    assert_eq!(json_str(json, "description"), Some(description.as_str()));
    assert_eq!(
        json_str(json, "level").unwrap_or("info"),
        node.level().name()
    );
    assert_eq!(
        json_bool(json, "showName").unwrap_or(true),
        node.show_name()
    );
    assert_eq!(
        json_str(json, "emptyBodyDescription"),
        node.empty_body_description()
    );
    assert_eq!(
        json_str(json, "style").unwrap_or("sparse"),
        node.style().unwrap_or(DiagnosticsTreeStyle::Sparse).name()
    );
    assert_eq!(json_str(json, "type"), Some(node.type_name()));
    assert_eq!(
        json_bool(json, "hasChildren").unwrap_or(false),
        !node.get_children().is_empty()
    );
}

/// Flutter's `validateNodeJsonSerialization`.
fn validate_node_json_serialization(node: &dyn DiagnosticsNode) {
    let json = serialize(node);
    validate_node_json_map(&json, node);
}

/// Flutter's `validatePropertyJsonSerializationHelper`.
fn validate_property_json_map<T: Any>(
    json: &DiagnosticsJsonMap,
    node: &dyn DiagnosticsNode,
    data: &PropertyData<T>,
) {
    validate_property_json_map_with_type(json, node, data, data.property_type());
}

fn validate_property_json_map_with_type<T: Any>(
    json: &DiagnosticsJsonMap,
    node: &dyn DiagnosticsNode,
    data: &PropertyData<T>,
    property_type: &str,
) {
    assert_eq!(
        json.contains_key("defaultValue"),
        data.default_value.is_some(),
        "defaultValue"
    );
    assert_eq!(json_str(json, "ifEmpty"), data.if_empty.as_deref());
    assert_eq!(json_str(json, "ifNull"), data.effective_if_null());
    assert_eq!(json_str(json, "tooltip"), data.tooltip.as_deref());
    assert_eq!(json_bool(json, "missingIfNull"), Some(data.missing_if_null));
    assert_eq!(json_str(json, "propertyType"), Some(property_type));
    assert!(json.contains_key("defaultLevel"), "defaultLevel");
    assert_eq!(
        json.contains_key("isDiagnosticableValue"),
        data.node_value().is_some()
    );
    validate_node_json_map(json, node);
}

/// Flutter's `validatePropertyJsonSerialization`.
fn validate_diagnostics_property_json_serialization<T: fmt::Display + PartialEq + Any>(
    property: &DiagnosticsProperty<T>,
) {
    let json = serialize(property);
    validate_property_json_map(&json, property, property.property_data());
}

/// Flutter's `validateStringPropertyJsonSerialization`.
fn validate_string_property_json_serialization(property: &StringProperty) {
    let json = serialize(property);
    assert_eq!(json_bool(&json, "quoted"), Some(property.quoted));
    validate_property_json_map(&json, property, property.property_data());
}

/// Flutter's `validateFlagPropertyJsonSerialization`.
fn validate_flag_property_json_serialization(property: &FlagProperty) {
    let json = serialize(property);
    assert_eq!(json_str(&json, "ifTrue"), property.if_true.as_deref());
    assert_eq!(json_str(&json, "ifFalse"), property.if_false.as_deref());
    validate_property_json_map(&json, property, property.property_data());
}

/// Flutter's `validateDoublePropertyJsonSerialization`, for both a `DoubleProperty` and the
/// `PercentProperty` that inherits its JSON.
fn validate_number_property_json_serialization(
    node: &dyn DiagnosticsNode,
    data: &PropertyData<f64>,
    unit: Option<&String>,
    number_to_string: &str,
) {
    let json = serialize(node);
    assert_eq!(json_str(&json, "unit"), unit.map(String::as_str));
    assert_eq!(json_str(&json, "numberToString"), Some(number_to_string));
    validate_property_json_map(&json, node, data);
}

/// Flutter's `validatePropertyJsonSerialization` for an `EnumProperty`.
fn validate_enum_property_json_serialization<T: fmt::Debug + PartialEq + Any>(
    property: &EnumProperty<T>,
) {
    let json = serialize(property);
    validate_property_json_map(&json, property, property.property_data());
}

/// Flutter's `validateObjectFlagPropertyJsonSerialization`.
fn validate_object_flag_property_json_serialization<T: fmt::Display + Any>(
    property: &ObjectFlagProperty<T>,
) {
    let json = serialize(property);
    assert_eq!(json_str(&json, "ifPresent"), property.if_present.as_deref());
    validate_property_json_map(&json, property, property.property_data());
}

/// Flutter's `validateIterableFlagsPropertyJsonSerialization`.
fn validate_iterable_flags_property_json_serialization<T: Any>(property: &FlagsSummary<T>) {
    let json = serialize(property);
    if property.value().is_empty() {
        assert!(!json.contains_key("values"));
    } else {
        let expected: Vec<&str> = property
            .value()
            .iter()
            .filter_map(|(key, value)| value.is_some().then_some(key.as_str()))
            .collect();
        assert_eq!(json_strings(&json, "values"), Some(expected));
    }
    validate_property_json_map(&json, property, property.property_data());
}

/// Flutter's `validateIterablePropertyJsonSerialization`.
fn validate_iterable_property_json_serialization<T: fmt::Display + PartialEq + Any>(
    property: &IterableProperty<T>,
) {
    let json = serialize(property);
    match property.value() {
        Some(values) => {
            let expected: Vec<String> = values
                .iter()
                .map(|value| dart_value_to_string(value as &dyn Any, value.to_string()))
                .collect();
            assert_eq!(
                json_strings(&json, "values"),
                Some(expected.iter().map(String::as_str).collect())
            );
        }
        None => assert!(!json.contains_key("values")),
    }
    validate_property_json_map(&json, property, property.property_data());
}

type NodeFilter =
    Rc<dyn Fn(Vec<Rc<dyn DiagnosticsNode>>, &dyn DiagnosticsNode) -> Vec<Rc<dyn DiagnosticsNode>>>;
type NodeTruncator = Rc<
    dyn Fn(
        Vec<Rc<dyn DiagnosticsNode>>,
        Option<&dyn DiagnosticsNode>,
    ) -> Vec<Rc<dyn DiagnosticsNode>>,
>;
type NodeDelegator = Rc<
    dyn Fn(
        &dyn DiagnosticsNode,
        &TestDiagnosticsSerializationDelegate,
    ) -> Rc<dyn DiagnosticsSerializationDelegate>,
>;

/// Flutter's `TestDiagnosticsSerializationDelegate`.
#[derive(Clone)]
struct TestDiagnosticsSerializationDelegate {
    include_properties: bool,
    subtree_depth: i32,
    additional_node_properties_map: DiagnosticsJsonMap,
    child_filter: Option<NodeFilter>,
    property_filter: Option<NodeFilter>,
    node_truncator: Option<NodeTruncator>,
    node_delegator: Option<NodeDelegator>,
}

impl TestDiagnosticsSerializationDelegate {
    fn new() -> Self {
        TestDiagnosticsSerializationDelegate {
            include_properties: false,
            subtree_depth: 0,
            additional_node_properties_map: DiagnosticsJsonMap::new(),
            child_filter: None,
            property_filter: None,
            node_truncator: None,
            node_delegator: None,
        }
    }

    fn with_include_properties(mut self, include_properties: bool) -> Self {
        self.include_properties = include_properties;
        self
    }

    fn with_subtree_depth(mut self, subtree_depth: i32) -> Self {
        self.subtree_depth = subtree_depth;
        self
    }

    fn with_additional_node_properties(mut self, json: DiagnosticsJsonMap) -> Self {
        self.additional_node_properties_map = json;
        self
    }

    fn with_child_filter(
        mut self,
        filter: impl Fn(
            Vec<Rc<dyn DiagnosticsNode>>,
            &dyn DiagnosticsNode,
        ) -> Vec<Rc<dyn DiagnosticsNode>>
        + 'static,
    ) -> Self {
        self.child_filter = Some(Rc::new(filter));
        self
    }

    fn with_property_filter(
        mut self,
        filter: impl Fn(
            Vec<Rc<dyn DiagnosticsNode>>,
            &dyn DiagnosticsNode,
        ) -> Vec<Rc<dyn DiagnosticsNode>>
        + 'static,
    ) -> Self {
        self.property_filter = Some(Rc::new(filter));
        self
    }

    fn with_node_truncator(
        mut self,
        truncator: impl Fn(
            Vec<Rc<dyn DiagnosticsNode>>,
            Option<&dyn DiagnosticsNode>,
        ) -> Vec<Rc<dyn DiagnosticsNode>>
        + 'static,
    ) -> Self {
        self.node_truncator = Some(Rc::new(truncator));
        self
    }

    fn with_node_delegator(
        mut self,
        delegator: impl Fn(
            &dyn DiagnosticsNode,
            &TestDiagnosticsSerializationDelegate,
        ) -> Rc<dyn DiagnosticsSerializationDelegate>
        + 'static,
    ) -> Self {
        self.node_delegator = Some(Rc::new(delegator));
        self
    }
}

impl DiagnosticsSerializationDelegate for TestDiagnosticsSerializationDelegate {
    fn additional_node_properties(
        &self,
        _node: &dyn DiagnosticsNode,
        _full_details: bool,
    ) -> DiagnosticsJsonMap {
        self.additional_node_properties_map.clone()
    }

    fn filter_children(
        &self,
        nodes: Vec<Rc<dyn DiagnosticsNode>>,
        owner: &dyn DiagnosticsNode,
    ) -> Vec<Rc<dyn DiagnosticsNode>> {
        match &self.child_filter {
            Some(filter) => filter(nodes, owner),
            None => nodes,
        }
    }

    fn filter_properties(
        &self,
        nodes: Vec<Rc<dyn DiagnosticsNode>>,
        owner: &dyn DiagnosticsNode,
    ) -> Vec<Rc<dyn DiagnosticsNode>> {
        match &self.property_filter {
            Some(filter) => filter(nodes, owner),
            None => nodes,
        }
    }

    fn truncate_nodes_list(
        &self,
        nodes: Vec<Rc<dyn DiagnosticsNode>>,
        owner: Option<&dyn DiagnosticsNode>,
    ) -> Vec<Rc<dyn DiagnosticsNode>> {
        match &self.node_truncator {
            Some(truncator) => truncator(nodes, owner),
            None => nodes,
        }
    }

    fn delegate_for_node(
        &self,
        node: &dyn DiagnosticsNode,
    ) -> Rc<dyn DiagnosticsSerializationDelegate> {
        if let Some(delegator) = &self.node_delegator {
            return delegator(node, self);
        }
        if self.subtree_depth > 0 {
            self.copy_with(Some(self.subtree_depth - 1), None)
        } else {
            Rc::new(self.clone())
        }
    }

    fn subtree_depth(&self) -> i32 {
        self.subtree_depth
    }

    fn include_properties(&self) -> bool {
        self.include_properties
    }

    fn expand_property_values(&self) -> bool {
        false
    }

    fn copy_with(
        &self,
        subtree_depth: Option<i32>,
        include_properties: Option<bool>,
    ) -> Rc<dyn DiagnosticsSerializationDelegate> {
        Rc::new(TestDiagnosticsSerializationDelegate {
            include_properties: include_properties.unwrap_or(self.include_properties),
            subtree_depth: subtree_depth.unwrap_or(self.subtree_depth),
            ..self.clone()
        })
    }
}

struct TestTree {
    name: String,
    style: Option<DiagnosticsTreeStyle>,
    children: Vec<TestTree>,
    properties: Vec<Rc<dyn DiagnosticsNode>>,
}

impl TestTree {
    fn new() -> Self {
        TestTree {
            name: String::new(),
            style: None,
            children: Vec::new(),
            properties: Vec::new(),
        }
    }

    fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    fn style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.style = Some(style);
        self
    }

    fn maybe_style(mut self, style: Option<DiagnosticsTreeStyle>) -> Self {
        self.style = style;
        self
    }

    fn child(mut self, child: TestTree) -> Self {
        self.children.push(child);
        self
    }

    fn property(mut self, property: impl DiagnosticsNode + 'static) -> Self {
        self.properties.push(Rc::new(property));
        self
    }

    fn property_rc(mut self, property: Rc<dyn DiagnosticsNode>) -> Self {
        self.properties.push(property);
        self
    }

    fn debug_node(
        &self,
        name: impl IntoPropertyName,
        style: Option<DiagnosticsTreeStyle>,
    ) -> DiagnosticableTreeNode {
        self.to_diagnostics_tree_node(name.into_property_name(), style)
    }
}

impl Diagnosticable for TestTree {
    fn debug_fill_properties(&self, properties: &mut DiagnosticPropertiesBuilder) {
        if let Some(style) = self.style {
            properties.default_diagnostics_tree_style = style;
        }
        for property in &self.properties {
            properties.add_rc(property.clone());
        }
    }
}

impl DiagnosticableTree for TestTree {
    fn debug_describe_children(&self) -> Vec<Rc<dyn DiagnosticsNode>> {
        self.children
            .iter()
            .map(|child| {
                Rc::new(
                    child.to_diagnostics_tree_node(
                        Some(format!("child {}", child.name)),
                        child.style,
                    ),
                ) as Rc<dyn DiagnosticsNode>
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TestRect;

impl fmt::Display for TestRect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Rect.fromLTRB(0.0, 0.0, 20.0, 20.0)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TestColor;

impl fmt::Display for TestColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Color(0xffffffff)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestObject {
    Rect,
    Color,
}

impl fmt::Display for TestObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestObject::Rect => TestRect.fmt(f),
            TestObject::Color => TestColor.fmt(f),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TestClosure;

impl fmt::Display for TestClosure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Closure: () => void")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExampleEnum {
    Hello,
    World,
    DeferToChild,
}

fn golden_style_test(
    style: Option<DiagnosticsTreeStyle>,
    last_child_style: Option<DiagnosticsTreeStyle>,
    golden: &str,
) {
    let tree = TestTree::new()
        .maybe_style(last_child_style)
        .child(TestTree::new().name("node A").maybe_style(style))
        .child(
            TestTree::new()
                .name("node B")
                .maybe_style(style)
                .child(TestTree::new().name("node B1").maybe_style(style))
                .child(TestTree::new().name("node B2").maybe_style(style))
                .child(
                    TestTree::new()
                        .name("node B3")
                        .maybe_style(last_child_style.or(style)),
                ),
        )
        .child(
            TestTree::new()
                .name("node C")
                .maybe_style(last_child_style.or(style)),
        );
    let node = tree.debug_node(None, style);
    let actual = node.to_string_deep();
    assert_deep(&actual, golden);
    validate_node_json_serialization(&node);
}

#[test]
fn tree_diagnostics_mixin_control_test() {
    golden_style_test(
        Some(DiagnosticsTreeStyle::Dense),
        None,
        concat!(
            "TestTree#00000\n",
            "├child node A: TestTree#00000\n",
            "├child node B: TestTree#00000\n",
            "│├child node B1: TestTree#00000\n",
            "│├child node B2: TestTree#00000\n",
            "│└child node B3: TestTree#00000\n",
            "└child node C: TestTree#00000\n",
        ),
    );

    golden_style_test(
        Some(DiagnosticsTreeStyle::Sparse),
        None,
        concat!(
            "TestTree#00000\n",
            " ├─child node A: TestTree#00000\n",
            " ├─child node B: TestTree#00000\n",
            " │ ├─child node B1: TestTree#00000\n",
            " │ ├─child node B2: TestTree#00000\n",
            " │ └─child node B3: TestTree#00000\n",
            " └─child node C: TestTree#00000\n",
        ),
    );

    golden_style_test(
        Some(DiagnosticsTreeStyle::Offstage),
        None,
        concat!(
            "TestTree#00000\n",
            " ╎╌child node A: TestTree#00000\n",
            " ╎╌child node B: TestTree#00000\n",
            " ╎ ╎╌child node B1: TestTree#00000\n",
            " ╎ ╎╌child node B2: TestTree#00000\n",
            " ╎ └╌child node B3: TestTree#00000\n",
            " └╌child node C: TestTree#00000\n",
        ),
    );

    golden_style_test(
        Some(DiagnosticsTreeStyle::Sparse),
        Some(DiagnosticsTreeStyle::Transition),
        concat!(
            "TestTree#00000\n",
            " ├─child node A: TestTree#00000\n",
            " ├─child node B: TestTree#00000\n",
            " │ ├─child node B1: TestTree#00000\n",
            " │ ├─child node B2: TestTree#00000\n",
            " │ ╘═╦══ child node B3 ═══\n",
            " │   ║ TestTree#00000\n",
            " │   ╚═══════════\n",
            " ╘═╦══ child node C ═══\n",
            "   ║ TestTree#00000\n",
            "   ╚═══════════\n",
        ),
    );

    golden_style_test(
        Some(DiagnosticsTreeStyle::Sparse),
        Some(DiagnosticsTreeStyle::Error),
        concat!(
            "TestTree#00000\n",
            " ├─child node A: TestTree#00000\n",
            " ├─child node B: TestTree#00000\n",
            " │ ├─child node B1: TestTree#00000\n",
            " │ ├─child node B2: TestTree#00000\n",
            " │ ╘═╦══╡ CHILD NODE B3: TESTTREE#00000 ╞═══════════════════════════════\n",
            " │   ╚══════════════════════════════════════════════════════════════════\n",
            " ╘═╦══╡ CHILD NODE C: TESTTREE#00000 ╞════════════════════════════════\n",
            "   ╚══════════════════════════════════════════════════════════════════\n",
        ),
    );

    golden_style_test(
        Some(DiagnosticsTreeStyle::Transition),
        None,
        concat!(
            "TestTree#00000:\n",
            "  ╞═╦══ child node A ═══\n",
            "  │ ║ TestTree#00000\n",
            "  │ ╚═══════════\n",
            "  ╞═╦══ child node B ═══\n",
            "  │ ║ TestTree#00000:\n",
            "  │ ║   ╞═╦══ child node B1 ═══\n",
            "  │ ║   │ ║ TestTree#00000\n",
            "  │ ║   │ ╚═══════════\n",
            "  │ ║   ╞═╦══ child node B2 ═══\n",
            "  │ ║   │ ║ TestTree#00000\n",
            "  │ ║   │ ╚═══════════\n",
            "  │ ║   ╘═╦══ child node B3 ═══\n",
            "  │ ║     ║ TestTree#00000\n",
            "  │ ║     ╚═══════════\n",
            "  │ ╚═══════════\n",
            "  ╘═╦══ child node C ═══\n",
            "    ║ TestTree#00000\n",
            "    ╚═══════════\n",
        ),
    );

    golden_style_test(
        Some(DiagnosticsTreeStyle::Error),
        None,
        concat!(
            "══╡ TESTTREE#00000 ╞═════════════════════════════════════════════\n",
            "╞═╦══╡ CHILD NODE A: TESTTREE#00000 ╞════════════════════════════════\n",
            "│ ╚══════════════════════════════════════════════════════════════════\n",
            "╞═╦══╡ CHILD NODE B: TESTTREE#00000 ╞════════════════════════════════\n",
            "│ ║ ╞═╦══╡ CHILD NODE B1: TESTTREE#00000 ╞═══════════════════════════════\n",
            "│ ║ │ ╚══════════════════════════════════════════════════════════════════\n",
            "│ ║ ╞═╦══╡ CHILD NODE B2: TESTTREE#00000 ╞═══════════════════════════════\n",
            "│ ║ │ ╚══════════════════════════════════════════════════════════════════\n",
            "│ ║ ╘═╦══╡ CHILD NODE B3: TESTTREE#00000 ╞═══════════════════════════════\n",
            "│ ║   ╚══════════════════════════════════════════════════════════════════\n",
            "│ ╚══════════════════════════════════════════════════════════════════\n",
            "╘═╦══╡ CHILD NODE C: TESTTREE#00000 ╞════════════════════════════════\n",
            "  ╚══════════════════════════════════════════════════════════════════\n",
            "═════════════════════════════════════════════════════════════════\n",
        ),
    );

    golden_style_test(
        Some(DiagnosticsTreeStyle::Whitespace),
        None,
        concat!(
            "TestTree#00000:\n",
            "  child node A: TestTree#00000\n",
            "  child node B: TestTree#00000:\n",
            "    child node B1: TestTree#00000\n",
            "    child node B2: TestTree#00000\n",
            "    child node B3: TestTree#00000\n",
            "  child node C: TestTree#00000\n",
        ),
    );

    // Single line mode does not display children.
    golden_style_test(
        Some(DiagnosticsTreeStyle::SingleLine),
        None,
        "TestTree#00000",
    );
}

fn golden_property_style_test(
    name: impl IntoPropertyName,
    style: Option<DiagnosticsTreeStyle>,
    last_child_style: Option<DiagnosticsTreeStyle>,
    property_style: DiagnosticsTreeStyle,
    golden: &str,
) {
    let tree = TestTree::new()
        .maybe_style(last_child_style)
        .property(
            StringProperty::new("stringProperty1", Some("value1".into()))
                .quoted(false)
                .with_style(property_style),
        )
        .property(DoubleProperty::new("doubleProperty1", Some(42.5)).with_style(property_style))
        .property(
            DoubleProperty::new("roundedProperty", Some(1.0 / 3.0)).with_style(property_style),
        )
        .property(
            StringProperty::new("DO_NOT_SHOW", Some("DO_NOT_SHOW".into()))
                .with_level(DiagnosticLevel::Hidden)
                .quoted(false)
                .with_style(property_style),
        )
        .property(
            DiagnosticsProperty::<TestObject>::new("DO_NOT_SHOW_NULL", None)
                .default_value(None)
                .with_style(property_style),
        )
        .property(
            DiagnosticsProperty::<TestObject>::new("nullProperty", None).with_style(property_style),
        )
        .property(
            StringProperty::new("node_type", Some("<root node>".into()))
                .with_show_name(false)
                .quoted(false)
                .with_style(property_style),
        )
        .child(TestTree::new().name("node A").maybe_style(style))
        .child(
            TestTree::new()
                .name("node B")
                .maybe_style(style)
                .property(
                    StringProperty::new("p1", Some("v1".into()))
                        .quoted(false)
                        .with_style(property_style),
                )
                .property(
                    StringProperty::new("p2", Some("v2".into()))
                        .quoted(false)
                        .with_style(property_style),
                )
                .child(TestTree::new().name("node B1").maybe_style(style))
                .child(
                    TestTree::new().name("node B2").maybe_style(style).property(
                        StringProperty::new("property1", Some("value1".into()))
                            .quoted(false)
                            .with_style(property_style),
                    ),
                )
                .child(
                    TestTree::new()
                        .name("node B3")
                        .maybe_style(last_child_style.or(style))
                        .property(
                            StringProperty::new("node_type", Some("<leaf node>".into()))
                                .with_show_name(false)
                                .quoted(false)
                                .with_style(property_style),
                        )
                        .property(IntProperty::new("foo", 42_i64).with_style(property_style)),
                ),
        )
        .child(
            TestTree::new()
                .name("node C")
                .maybe_style(last_child_style.or(style))
                .property(
                    StringProperty::new("foo", Some("multi\nline\nvalue!".into()))
                        .quoted(false)
                        .with_style(property_style),
                ),
        );
    let node = tree.debug_node(name, style);
    let actual = node.to_string_deep();
    assert_deep(&actual, golden);
    validate_node_json_serialization(&node);
}

#[test]
fn tree_diagnostics_mixin_tree_with_properties_test() {
    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::Sparse),
        None,
        DiagnosticsTreeStyle::SingleLine,
        concat!(
            "TestTree#00000\n",
            " │ stringProperty1: value1\n",
            " │ doubleProperty1: 42.5\n",
            " │ roundedProperty: 0.3\n",
            " │ nullProperty: null\n",
            " │ <root node>\n",
            " │\n",
            " ├─child node A: TestTree#00000\n",
            " ├─child node B: TestTree#00000\n",
            " │ │ p1: v1\n",
            " │ │ p2: v2\n",
            " │ │\n",
            " │ ├─child node B1: TestTree#00000\n",
            " │ ├─child node B2: TestTree#00000\n",
            " │ │   property1: value1\n",
            " │ │\n",
            " │ └─child node B3: TestTree#00000\n",
            " │     <leaf node>\n",
            " │     foo: 42\n",
            " │\n",
            " └─child node C: TestTree#00000\n",
            "     foo:\n",
            "       multi\n",
            "       line\n",
            "       value!\n",
        ),
    );

    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::Sparse),
        None,
        DiagnosticsTreeStyle::ErrorProperty,
        concat!(
            "TestTree#00000\n",
            " │ stringProperty1:\n",
            " │   value1\n",
            " │ doubleProperty1:\n",
            " │   42.5\n",
            " │ roundedProperty:\n",
            " │   0.3\n",
            " │ nullProperty:\n",
            " │   null\n",
            " │ <root node>\n",
            " │\n",
            " ├─child node A: TestTree#00000\n",
            " ├─child node B: TestTree#00000\n",
            " │ │ p1:\n",
            " │ │   v1\n",
            " │ │ p2:\n",
            " │ │   v2\n",
            " │ │\n",
            " │ ├─child node B1: TestTree#00000\n",
            " │ ├─child node B2: TestTree#00000\n",
            " │ │   property1:\n",
            " │ │     value1\n",
            " │ │\n",
            " │ └─child node B3: TestTree#00000\n",
            " │     <leaf node>\n",
            " │     foo:\n",
            " │       42\n",
            " │\n",
            " └─child node C: TestTree#00000\n",
            "     foo:\n",
            "       multi\n",
            "       line\n",
            "       value!\n",
        ),
    );

    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::Dense),
        None,
        DiagnosticsTreeStyle::SingleLine,
        concat!(
            "TestTree#00000(stringProperty1: value1, doubleProperty1: 42.5, roundedProperty: 0.3, nullProperty: null, <root node>)\n",
            "├child node A: TestTree#00000\n",
            "├child node B: TestTree#00000(p1: v1, p2: v2)\n",
            "│├child node B1: TestTree#00000\n",
            "│├child node B2: TestTree#00000(property1: value1)\n",
            "│└child node B3: TestTree#00000(<leaf node>, foo: 42)\n",
            "└child node C: TestTree#00000(foo: multi\\nline\\nvalue!)\n",
        ),
    );

    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::Offstage),
        None,
        DiagnosticsTreeStyle::SingleLine,
        concat!(
            "TestTree#00000\n",
            " │ stringProperty1: value1\n",
            " │ doubleProperty1: 42.5\n",
            " │ roundedProperty: 0.3\n",
            " │ nullProperty: null\n",
            " │ <root node>\n",
            " │\n",
            " ╎╌child node A: TestTree#00000\n",
            " ╎╌child node B: TestTree#00000\n",
            " ╎ │ p1: v1\n",
            " ╎ │ p2: v2\n",
            " ╎ │\n",
            " ╎ ╎╌child node B1: TestTree#00000\n",
            " ╎ ╎╌child node B2: TestTree#00000\n",
            " ╎ ╎   property1: value1\n",
            " ╎ ╎\n",
            " ╎ └╌child node B3: TestTree#00000\n",
            " ╎     <leaf node>\n",
            " ╎     foo: 42\n",
            " ╎\n",
            " └╌child node C: TestTree#00000\n",
            "     foo:\n",
            "       multi\n",
            "       line\n",
            "       value!\n",
        ),
    );

    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::Sparse),
        Some(DiagnosticsTreeStyle::Transition),
        DiagnosticsTreeStyle::SingleLine,
        concat!(
            "TestTree#00000\n",
            " │ stringProperty1: value1\n",
            " │ doubleProperty1: 42.5\n",
            " │ roundedProperty: 0.3\n",
            " │ nullProperty: null\n",
            " │ <root node>\n",
            " │\n",
            " ├─child node A: TestTree#00000\n",
            " ├─child node B: TestTree#00000\n",
            " │ │ p1: v1\n",
            " │ │ p2: v2\n",
            " │ │\n",
            " │ ├─child node B1: TestTree#00000\n",
            " │ ├─child node B2: TestTree#00000\n",
            " │ │   property1: value1\n",
            " │ │\n",
            " │ ╘═╦══ child node B3 ═══\n",
            " │   ║ TestTree#00000:\n",
            " │   ║   <leaf node>\n",
            " │   ║   foo: 42\n",
            " │   ╚═══════════\n",
            " ╘═╦══ child node C ═══\n",
            "   ║ TestTree#00000:\n",
            "   ║   foo:\n",
            "   ║     multi\n",
            "   ║     line\n",
            "   ║     value!\n",
            "   ╚═══════════\n",
        ),
    );

    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::Sparse),
        Some(DiagnosticsTreeStyle::Error),
        DiagnosticsTreeStyle::SingleLine,
        concat!(
            "TestTree#00000\n",
            " │ stringProperty1: value1\n",
            " │ doubleProperty1: 42.5\n",
            " │ roundedProperty: 0.3\n",
            " │ nullProperty: null\n",
            " │ <root node>\n",
            " │\n",
            " ├─child node A: TestTree#00000\n",
            " ├─child node B: TestTree#00000\n",
            " │ │ p1: v1\n",
            " │ │ p2: v2\n",
            " │ │\n",
            " │ ├─child node B1: TestTree#00000\n",
            " │ ├─child node B2: TestTree#00000\n",
            " │ │   property1: value1\n",
            " │ │\n",
            " │ ╘═╦══╡ CHILD NODE B3: TESTTREE#00000 ╞═══════════════════════════════\n",
            " │   ║ <leaf node>\n",
            " │   ║ foo: 42\n",
            " │   ╚══════════════════════════════════════════════════════════════════\n",
            " ╘═╦══╡ CHILD NODE C: TESTTREE#00000 ╞════════════════════════════════\n",
            "   ║ foo:\n",
            "   ║   multi\n",
            "   ║   line\n",
            "   ║   value!\n",
            "   ╚══════════════════════════════════════════════════════════════════\n",
        ),
    );

    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::Transition),
        None,
        DiagnosticsTreeStyle::SingleLine,
        concat!(
            "TestTree#00000:\n",
            "  stringProperty1: value1\n",
            "  doubleProperty1: 42.5\n",
            "  roundedProperty: 0.3\n",
            "  nullProperty: null\n",
            "  <root node>\n",
            "  ╞═╦══ child node A ═══\n",
            "  │ ║ TestTree#00000\n",
            "  │ ╚═══════════\n",
            "  ╞═╦══ child node B ═══\n",
            "  │ ║ TestTree#00000:\n",
            "  │ ║   p1: v1\n",
            "  │ ║   p2: v2\n",
            "  │ ║   ╞═╦══ child node B1 ═══\n",
            "  │ ║   │ ║ TestTree#00000\n",
            "  │ ║   │ ╚═══════════\n",
            "  │ ║   ╞═╦══ child node B2 ═══\n",
            "  │ ║   │ ║ TestTree#00000:\n",
            "  │ ║   │ ║   property1: value1\n",
            "  │ ║   │ ╚═══════════\n",
            "  │ ║   ╘═╦══ child node B3 ═══\n",
            "  │ ║     ║ TestTree#00000:\n",
            "  │ ║     ║   <leaf node>\n",
            "  │ ║     ║   foo: 42\n",
            "  │ ║     ╚═══════════\n",
            "  │ ╚═══════════\n",
            "  ╘═╦══ child node C ═══\n",
            "    ║ TestTree#00000:\n",
            "    ║   foo:\n",
            "    ║     multi\n",
            "    ║     line\n",
            "    ║     value!\n",
            "    ╚═══════════\n",
        ),
    );

    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::Whitespace),
        None,
        DiagnosticsTreeStyle::SingleLine,
        concat!(
            "TestTree#00000:\n",
            "  stringProperty1: value1\n",
            "  doubleProperty1: 42.5\n",
            "  roundedProperty: 0.3\n",
            "  nullProperty: null\n",
            "  <root node>\n",
            "  child node A: TestTree#00000\n",
            "  child node B: TestTree#00000:\n",
            "    p1: v1\n",
            "    p2: v2\n",
            "    child node B1: TestTree#00000\n",
            "    child node B2: TestTree#00000:\n",
            "      property1: value1\n",
            "    child node B3: TestTree#00000:\n",
            "      <leaf node>\n",
            "      foo: 42\n",
            "  child node C: TestTree#00000:\n",
            "    foo:\n",
            "      multi\n",
            "      line\n",
            "      value!\n",
        ),
    );

    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::Flat),
        None,
        DiagnosticsTreeStyle::SingleLine,
        concat!(
            "TestTree#00000:\n",
            "stringProperty1: value1\n",
            "doubleProperty1: 42.5\n",
            "roundedProperty: 0.3\n",
            "nullProperty: null\n",
            "<root node>\n",
            "child node A: TestTree#00000\n",
            "child node B: TestTree#00000:\n",
            "p1: v1\n",
            "p2: v2\n",
            "child node B1: TestTree#00000\n",
            "child node B2: TestTree#00000:\n",
            "property1: value1\n",
            "child node B3: TestTree#00000:\n",
            "<leaf node>\n",
            "foo: 42\n",
            "child node C: TestTree#00000:\n",
            "foo:\n",
            "  multi\n",
            "  line\n",
            "  value!\n",
        ),
    );

    // Single line mode does not display children.
    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::SingleLine),
        None,
        DiagnosticsTreeStyle::SingleLine,
        "TestTree#00000(stringProperty1: value1, doubleProperty1: 42.5, roundedProperty: 0.3, nullProperty: null, <root node>)",
    );

    golden_property_style_test(
        "some name",
        Some(DiagnosticsTreeStyle::SingleLine),
        None,
        DiagnosticsTreeStyle::SingleLine,
        "some name: TestTree#00000(stringProperty1: value1, doubleProperty1: 42.5, roundedProperty: 0.3, nullProperty: null, <root node>)",
    );

    // No name so we don't indent.
    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::ErrorProperty),
        None,
        DiagnosticsTreeStyle::SingleLine,
        "TestTree#00000(stringProperty1: value1, doubleProperty1: 42.5, roundedProperty: 0.3, nullProperty: null, <root node>)\n",
    );

    golden_property_style_test(
        "some name",
        Some(DiagnosticsTreeStyle::ErrorProperty),
        None,
        DiagnosticsTreeStyle::SingleLine,
        concat!(
            "some name:\n",
            "  TestTree#00000(stringProperty1: value1, doubleProperty1: 42.5, roundedProperty: 0.3, nullProperty: null, <root node>)\n",
        ),
    );

    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::Sparse),
        Some(DiagnosticsTreeStyle::SingleLine),
        DiagnosticsTreeStyle::SingleLine,
        concat!(
            "TestTree#00000\n",
            " │ stringProperty1: value1\n",
            " │ doubleProperty1: 42.5\n",
            " │ roundedProperty: 0.3\n",
            " │ nullProperty: null\n",
            " │ <root node>\n",
            " │\n",
            " ├─child node A: TestTree#00000\n",
            " ├─child node B: TestTree#00000\n",
            " │ │ p1: v1\n",
            " │ │ p2: v2\n",
            " │ │\n",
            " │ ├─child node B1: TestTree#00000\n",
            " │ ├─child node B2: TestTree#00000\n",
            " │ │   property1: value1\n",
            " │ │\n",
            " │ └─child node B3: TestTree#00000(<leaf node>, foo: 42)\n",
            " └─child node C: TestTree#00000(foo: multi\\nline\\nvalue!)\n",
        ),
    );

    golden_property_style_test(
        None,
        Some(DiagnosticsTreeStyle::Sparse),
        Some(DiagnosticsTreeStyle::ErrorProperty),
        DiagnosticsTreeStyle::SingleLine,
        concat!(
            "TestTree#00000\n",
            " │ stringProperty1: value1\n",
            " │ doubleProperty1: 42.5\n",
            " │ roundedProperty: 0.3\n",
            " │ nullProperty: null\n",
            " │ <root node>\n",
            " │\n",
            " ├─child node A: TestTree#00000\n",
            " ├─child node B: TestTree#00000\n",
            " │ │ p1: v1\n",
            " │ │ p2: v2\n",
            " │ │\n",
            " │ ├─child node B1: TestTree#00000\n",
            " │ ├─child node B2: TestTree#00000\n",
            " │ │   property1: value1\n",
            " │ │\n",
            " │ └─child node B3:\n",
            " │     TestTree#00000(<leaf node>, foo: 42)\n",
            " └─child node C:\n",
            "     TestTree#00000(foo: multi\\nline\\nvalue!)\n",
        ),
    );
}

#[test]
fn to_string_test() {
    let tree = TestTree::new()
        .property(StringProperty::new("stringProperty1", Some("value1".into())).quoted(false))
        .property(DoubleProperty::new("doubleProperty1", Some(42.5)))
        .property(DoubleProperty::new("roundedProperty", Some(1.0 / 3.0)))
        .property(
            StringProperty::new("DO_NOT_SHOW", Some("DO_NOT_SHOW".into()))
                .with_level(DiagnosticLevel::Hidden)
                .quoted(false),
        )
        .property(
            StringProperty::new("DEBUG_ONLY", Some("DEBUG_ONLY".into()))
                .with_level(DiagnosticLevel::Debug)
                .quoted(false),
        )
        // child to verify that children are not included in the toString.
        .child(TestTree::new().name("node A"));

    assert_deep(
        &tree.to_string(),
        "TestTree#00000(stringProperty1: value1, doubleProperty1: 42.5, roundedProperty: 0.3)",
    );
    assert_deep(
        &tree.to_string_at(DiagnosticLevel::Debug),
        "TestTree#00000(stringProperty1: value1, doubleProperty1: 42.5, roundedProperty: 0.3, DEBUG_ONLY: DEBUG_ONLY)",
    );
}

#[test]
fn transition_test() {
    let tree = TestTree::new()
        .style(DiagnosticsTreeStyle::Sparse)
        .property(StringProperty::new(
            "stringProperty1",
            Some("value1".into()),
        ))
        .child(
            TestTree::new()
                .style(DiagnosticsTreeStyle::Transition)
                .name("node transition")
                .property(StringProperty::new("p1", Some("v1".into())))
                .property_rc(node(
                    TestTree::new()
                        .property(DiagnosticsProperty::<bool>::new("survived", Some(true)))
                        .debug_node("tree property", Some(DiagnosticsTreeStyle::Whitespace)),
                ))
                .child(
                    TestTree::new()
                        .name("dense child")
                        .style(DiagnosticsTreeStyle::Dense),
                )
                .child(
                    TestTree::new()
                        .name("dense")
                        .style(DiagnosticsTreeStyle::Dense)
                        .property(StringProperty::new("property1", Some("value1".into()))),
                )
                .child(
                    TestTree::new()
                        .name("node B3")
                        .style(DiagnosticsTreeStyle::Dense)
                        .property(
                            StringProperty::new("node_type", Some("<leaf node>".into()))
                                .with_show_name(false)
                                .quoted(false),
                        )
                        .property(IntProperty::new("foo", 42_i64)),
                ),
        )
        .child(
            TestTree::new()
                .name("node C")
                .style(DiagnosticsTreeStyle::Sparse)
                .property(
                    StringProperty::new("foo", Some("multi\nline\nvalue!".into())).quoted(false),
                ),
        );

    assert_deep(
        &tree.to_string_deep(),
        concat!(
            "TestTree#00000\n",
            " │ stringProperty1: \"value1\"\n",
            " ╞═╦══ child node transition ═══\n",
            " │ ║ TestTree#00000:\n",
            " │ ║   p1: \"v1\"\n",
            " │ ║   tree property: TestTree#00000:\n",
            " │ ║     survived: true\n",
            " │ ║   ├child dense child: TestTree#00000\n",
            " │ ║   ├child dense: TestTree#00000(property1: \"value1\")\n",
            " │ ║   └child node B3: TestTree#00000(<leaf node>, foo: 42)\n",
            " │ ╚═══════════\n",
            " └─child node C: TestTree#00000\n",
            "     foo:\n",
            "       multi\n",
            "       line\n",
            "       value!\n",
        ),
    );
}

#[test]
fn describe_enum_test() {
    assert_eq!(describe_enum(&ExampleEnum::Hello), "hello");
    assert_eq!(describe_enum(&ExampleEnum::World), "world");
    assert_eq!(describe_enum(&ExampleEnum::DeferToChild), "deferToChild");

    struct NotEnum;

    impl fmt::Debug for NotEnum {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Hello World")
        }
    }

    assert_eq!(describe_enum(&NotEnum), "hello World");
}

#[test]
fn string_property_test() {
    assert_eq!(
        StringProperty::new("name", Some("value".into()))
            .quoted(false)
            .to_string(),
        "name: value"
    );

    let string_property = StringProperty::new("name", Some("value".into()))
        .description("VALUE")
        .if_empty("<hidden>")
        .quoted(false);
    assert_eq!(string_property.to_string(), "name: VALUE");
    validate_string_property_json_serialization(&string_property);

    assert_eq!(
        StringProperty::new("name", Some("value".into()))
            .with_show_name(false)
            .if_empty("<hidden>")
            .quoted(false)
            .to_string(),
        "value"
    );

    assert_eq!(
        StringProperty::new("name", Some(String::new()))
            .if_empty("<hidden>")
            .to_string(),
        "name: <hidden>"
    );

    assert_eq!(
        StringProperty::new("name", Some(String::new()))
            .if_empty("<hidden>")
            .with_show_name(false)
            .to_string(),
        "<hidden>"
    );

    assert!(!StringProperty::new("name", None).is_filtered(DiagnosticLevel::Info));
    assert!(
        StringProperty::new("name", Some("value".into()))
            .with_level(DiagnosticLevel::Hidden)
            .is_filtered(DiagnosticLevel::Info)
    );
    assert!(
        StringProperty::new("name", None)
            .default_value(None)
            .is_filtered(DiagnosticLevel::Info)
    );
    let quoted = StringProperty::new("name", Some("value".into()));
    assert_eq!(quoted.to_string(), "name: \"value\"");
    validate_string_property_json_serialization(&quoted);
    assert_eq!(
        StringProperty::new("name", Some("value".into()))
            .with_show_name(false)
            .to_string(),
        "\"value\""
    );
    assert_eq!(
        StringProperty::new("name", None)
            .with_show_name(false)
            .to_string(),
        "null"
    );
}

#[test]
fn bool_property_test() {
    let true_property = DiagnosticsProperty::<bool>::new("name", Some(true));
    let false_property = DiagnosticsProperty::<bool>::new("name", Some(false));
    assert_eq!(true_property.to_string(), "name: true");
    assert!(!true_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(true_property.value(), Some(&true));
    assert_eq!(false_property.to_string(), "name: false");
    assert_eq!(false_property.value(), Some(&false));
    assert!(!false_property.is_filtered(DiagnosticLevel::Info));
    let truthy_property =
        DiagnosticsProperty::<bool>::new("name", Some(true)).description("truthy");
    assert_eq!(truthy_property.to_string(), "name: truthy");
    assert_eq!(
        DiagnosticsProperty::<bool>::new("name", Some(true))
            .with_show_name(false)
            .to_string(),
        "true"
    );

    assert!(!DiagnosticsProperty::<bool>::new("name", None).is_filtered(DiagnosticLevel::Info));
    assert!(
        DiagnosticsProperty::<bool>::new("name", Some(true))
            .with_level(DiagnosticLevel::Hidden)
            .is_filtered(DiagnosticLevel::Info)
    );
    assert!(
        DiagnosticsProperty::<bool>::new("name", None)
            .default_value(None)
            .is_filtered(DiagnosticLevel::Info)
    );
    let missing_bool = DiagnosticsProperty::<bool>::new("name", None).if_null("missing");
    assert_eq!(missing_bool.to_string(), "name: missing");
    validate_diagnostics_property_json_serialization(&true_property);
    validate_diagnostics_property_json_serialization(&false_property);
    validate_diagnostics_property_json_serialization(&truthy_property);
    validate_diagnostics_property_json_serialization(&missing_bool);
}

#[test]
fn flag_property_test() {
    let true_flag = FlagProperty::new("myFlag", Some(true)).if_true("myFlag");
    let false_flag = FlagProperty::new("myFlag", Some(false)).if_true("myFlag");
    assert_eq!(true_flag.to_string(), "myFlag");
    assert_eq!(true_flag.value(), Some(&true));
    assert_eq!(false_flag.value(), Some(&false));
    assert!(!true_flag.is_filtered(DiagnosticLevel::Fine));
    assert!(false_flag.is_filtered(DiagnosticLevel::Fine));
    validate_flag_property_json_serialization(&true_flag);
    validate_flag_property_json_serialization(&false_flag);
}

#[test]
fn property_with_tooltip_test() {
    let with_tooltip =
        DiagnosticsProperty::<String>::new("name", Some("value".into())).tooltip("tooltip");
    assert_eq!(with_tooltip.to_string(), "name: value (tooltip)");
    assert_eq!(with_tooltip.value(), Some(&"value".to_string()));
    assert!(!with_tooltip.is_filtered(DiagnosticLevel::Fine));
    validate_diagnostics_property_json_serialization(&with_tooltip);
}

#[test]
fn double_property_test() {
    let double_property = DoubleProperty::new("name", Some(42.0));
    assert_eq!(double_property.to_string(), "name: 42.0");
    assert!(!double_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(double_property.value(), Some(&42.0));
    validate_number_property_json_serialization(
        &double_property,
        double_property.property_data(),
        double_property.unit.as_ref(),
        &double_property.number_to_string(),
    );

    assert_eq!(
        DoubleProperty::new("name", Some(1.3333)).to_string(),
        "name: 1.3"
    );
    assert_eq!(DoubleProperty::new("name", None).to_string(), "name: null");
    assert!(!DoubleProperty::new("name", None).is_filtered(DiagnosticLevel::Info));
    assert_eq!(
        DoubleProperty::new("name", None)
            .if_null("missing")
            .to_string(),
        "name: missing"
    );

    let double_with_unit = DoubleProperty::new("name", Some(42.0)).unit("px");
    assert_eq!(double_with_unit.to_string(), "name: 42.0px");
    validate_number_property_json_serialization(
        &double_with_unit,
        double_with_unit.property_data(),
        double_with_unit.unit.as_ref(),
        &double_with_unit.number_to_string(),
    );
}

#[test]
fn double_infinity_serialization_test() {
    let inf_property_1 = DoubleProperty::new("double1", Some(f64::INFINITY));
    assert_eq!(inf_property_1.to_string(), "double1: Infinity");

    let inf_property_2 = DoubleProperty::new("double2", Some(f64::NEG_INFINITY));
    assert_eq!(inf_property_2.to_string(), "double2: -Infinity");
    validate_number_property_json_serialization(
        &inf_property_1,
        inf_property_1.property_data(),
        inf_property_1.unit.as_ref(),
        &inf_property_1.number_to_string(),
    );
    validate_number_property_json_serialization(
        &inf_property_2,
        inf_property_2.property_data(),
        inf_property_2.unit.as_ref(),
        &inf_property_2.number_to_string(),
    );
}

#[test]
fn unsafe_double_property_test() {
    let safe = DoubleProperty::lazy("name", Rc::new(|| Some(42.0)));
    assert_eq!(safe.to_string(), "name: 42.0");
    assert!(!safe.is_filtered(DiagnosticLevel::Info));
    assert_eq!(safe.value(), Some(&42.0));
    validate_number_property_json_serialization(
        &safe,
        safe.property_data(),
        safe.unit.as_ref(),
        &safe.number_to_string(),
    );
    assert_eq!(
        DoubleProperty::lazy("name", Rc::new(|| Some(1.3333))).to_string(),
        "name: 1.3"
    );

    assert_eq!(
        DoubleProperty::lazy("name", Rc::new(|| None)).to_string(),
        "name: null"
    );
    assert!(!DoubleProperty::lazy("name", Rc::new(|| None)).is_filtered(DiagnosticLevel::Info));
}

#[test]
fn percent_property_test() {
    assert_eq!(
        PercentProperty::new("name", Some(0.4)).to_string(),
        "name: 40.0%"
    );

    let complex_percent_property = PercentProperty::new("name", Some(0.99))
        .unit("invisible")
        .tooltip("almost transparent");
    assert_eq!(
        complex_percent_property.to_string(),
        "name: 99.0% invisible (almost transparent)"
    );
    validate_number_property_json_serialization(
        &complex_percent_property,
        complex_percent_property.property_data(),
        complex_percent_property.unit.as_ref(),
        &complex_percent_property.number_to_string(),
    );

    assert_eq!(
        PercentProperty::new("name", None)
            .unit("invisible")
            .tooltip("!")
            .to_string(),
        "name: null (!)"
    );

    assert_eq!(PercentProperty::new("name", Some(0.4)).value(), Some(&0.4));
    assert_eq!(
        PercentProperty::new("name", Some(0.0)).to_string(),
        "name: 0.0%"
    );
    assert_eq!(
        PercentProperty::new("name", Some(-10.0)).to_string(),
        "name: 0.0%"
    );
    assert_eq!(
        PercentProperty::new("name", Some(1.0)).to_string(),
        "name: 100.0%"
    );
    assert_eq!(
        PercentProperty::new("name", Some(3.0)).to_string(),
        "name: 100.0%"
    );
    assert_eq!(PercentProperty::new("name", None).to_string(), "name: null");
    assert_eq!(
        PercentProperty::new("name", None)
            .if_null("missing")
            .to_string(),
        "name: missing"
    );
    assert_eq!(
        PercentProperty::new("name", None)
            .if_null("missing")
            .with_show_name(false)
            .to_string(),
        "missing"
    );
    assert_eq!(
        PercentProperty::new("name", Some(0.5))
            .with_show_name(false)
            .to_string(),
        "50.0%"
    );
}

#[test]
fn callback_property_test() {
    let present = ObjectFlagProperty::<TestClosure>::new("onClick", Some(TestClosure))
        .if_present("clickable");
    let missing = ObjectFlagProperty::<TestClosure>::new("onClick", None).if_present("clickable");

    assert_eq!(present.to_string(), "clickable");
    assert!(!present.is_filtered(DiagnosticLevel::Info));
    assert_eq!(present.value(), Some(&TestClosure));
    assert_eq!(missing.to_string(), "onClick: null");
    assert!(missing.is_filtered(DiagnosticLevel::Fine));
    validate_object_flag_property_json_serialization(&present);
    validate_object_flag_property_json_serialization(&missing);
}

#[test]
fn missing_callback_property_test() {
    let present =
        ObjectFlagProperty::<TestClosure>::new("onClick", Some(TestClosure)).if_null("disabled");
    let missing = ObjectFlagProperty::<TestClosure>::new("onClick", None).if_null("disabled");

    assert_eq!(present.to_string(), "onClick: Closure: () => void");
    assert!(present.is_filtered(DiagnosticLevel::Fine));
    assert_eq!(present.value(), Some(&TestClosure));
    assert_eq!(missing.to_string(), "disabled");
    assert!(!missing.is_filtered(DiagnosticLevel::Info));
    validate_object_flag_property_json_serialization(&present);
    validate_object_flag_property_json_serialization(&missing);
}

#[test]
fn describe_bool_property() {
    let yes = FlagProperty::new("name", Some(true))
        .if_true("YES")
        .if_false("NO")
        .with_show_name(true);
    let no = FlagProperty::new("name", Some(false))
        .if_true("YES")
        .if_false("NO")
        .with_show_name(true);
    assert_eq!(yes.to_string(), "name: YES");
    assert_eq!(yes.level(), DiagnosticLevel::Info);
    assert_eq!(yes.value(), Some(&true));
    validate_flag_property_json_serialization(&yes);
    assert_eq!(no.to_string(), "name: NO");
    assert_eq!(no.level(), DiagnosticLevel::Info);
    assert_eq!(no.value(), Some(&false));
    validate_flag_property_json_serialization(&no);

    assert_eq!(
        FlagProperty::new("name", Some(true))
            .if_true("YES")
            .if_false("NO")
            .to_string(),
        "YES"
    );
    assert_eq!(
        FlagProperty::new("name", Some(false))
            .if_true("YES")
            .if_false("NO")
            .to_string(),
        "NO"
    );
    assert_eq!(
        FlagProperty::new("name", Some(true))
            .if_true("YES")
            .if_false("NO")
            .with_level(DiagnosticLevel::Hidden)
            .with_show_name(true)
            .level(),
        DiagnosticLevel::Hidden
    );
}

#[test]
fn enum_property_test() {
    let hello = EnumProperty::<ExampleEnum>::new("name", Some(ExampleEnum::Hello));
    let world = EnumProperty::<ExampleEnum>::new("name", Some(ExampleEnum::World));
    let defer_to_child = EnumProperty::<ExampleEnum>::new("name", Some(ExampleEnum::DeferToChild));
    let null_enum = EnumProperty::<ExampleEnum>::new("name", None);
    assert_eq!(hello.level(), DiagnosticLevel::Info);
    assert_eq!(hello.value(), Some(&ExampleEnum::Hello));
    assert_eq!(hello.to_string(), "name: hello");

    assert_eq!(world.level(), DiagnosticLevel::Info);
    assert_eq!(world.value(), Some(&ExampleEnum::World));
    assert_eq!(world.to_string(), "name: world");

    assert_eq!(defer_to_child.level(), DiagnosticLevel::Info);
    assert_eq!(defer_to_child.value(), Some(&ExampleEnum::DeferToChild));
    assert_eq!(defer_to_child.to_string(), "name: deferToChild");

    assert_eq!(null_enum.level(), DiagnosticLevel::Info);
    assert_eq!(null_enum.value(), None);
    assert_eq!(null_enum.to_string(), "name: null");

    let matches_default = EnumProperty::<ExampleEnum>::new("name", Some(ExampleEnum::Hello))
        .default_value(Some(ExampleEnum::Hello));
    assert_eq!(matches_default.to_string(), "name: hello");
    assert_eq!(matches_default.value(), Some(&ExampleEnum::Hello));
    assert!(matches_default.is_filtered(DiagnosticLevel::Info));

    assert_eq!(
        EnumProperty::<ExampleEnum>::new("name", Some(ExampleEnum::Hello))
            .with_level(DiagnosticLevel::Hidden)
            .level(),
        DiagnosticLevel::Hidden
    );

    validate_enum_property_json_serialization(&hello);
    validate_enum_property_json_serialization(&world);
    validate_enum_property_json_serialization(&defer_to_child);
    validate_enum_property_json_serialization(&null_enum);
    validate_enum_property_json_serialization(&matches_default);
}

#[test]
fn int_property_test() {
    let regular = IntProperty::new("name", 42_i64);
    assert_eq!(regular.to_string(), "name: 42");
    assert_eq!(regular.value(), Some(&42));
    assert_eq!(regular.level(), DiagnosticLevel::Info);

    let null_value = IntProperty::<i32>::new("name", None);
    assert_eq!(null_value.to_string(), "name: null");
    assert_eq!(null_value.value(), None);
    assert_eq!(null_value.level(), DiagnosticLevel::Info);

    let hide_null = IntProperty::<i32>::new("name", None).default_value(None);
    assert_eq!(hide_null.to_string(), "name: null");
    assert_eq!(hide_null.value(), None);
    assert!(hide_null.is_filtered(DiagnosticLevel::Info));

    let null_description = IntProperty::<i32>::new("name", None).if_null("missing");
    assert_eq!(null_description.to_string(), "name: missing");
    assert_eq!(null_description.value(), None);
    assert_eq!(null_description.level(), DiagnosticLevel::Info);

    let hide_name = IntProperty::new("name", 42_i64).with_show_name(false);
    assert_eq!(hide_name.to_string(), "42");
    assert_eq!(hide_name.value(), Some(&42));
    assert_eq!(hide_name.level(), DiagnosticLevel::Info);

    let with_unit = IntProperty::new("name", 42_i64).unit("pt");
    assert_eq!(with_unit.to_string(), "name: 42pt");
    assert_eq!(with_unit.value(), Some(&42));
    assert_eq!(with_unit.level(), DiagnosticLevel::Info);

    let default_value = IntProperty::new("name", 42_i64).default_value(Some(42));
    assert_eq!(default_value.to_string(), "name: 42");
    assert_eq!(default_value.value(), Some(&42));
    assert!(default_value.is_filtered(DiagnosticLevel::Info));

    let not_default_value = IntProperty::new("name", 43_i64).default_value(Some(42));
    assert_eq!(not_default_value.to_string(), "name: 43");
    assert_eq!(not_default_value.value(), Some(&43));
    assert_eq!(not_default_value.level(), DiagnosticLevel::Info);

    let hidden = IntProperty::new("name", 42_i64).with_level(DiagnosticLevel::Hidden);
    assert_eq!(hidden.to_string(), "name: 42");
    assert_eq!(hidden.value(), Some(&42));
    assert_eq!(hidden.level(), DiagnosticLevel::Hidden);
}

#[test]
fn object_property_test() {
    let rect = TestRect;
    let simple = DiagnosticsProperty::<TestRect>::new("name", Some(rect));
    assert_eq!(simple.value(), Some(&rect));
    assert_eq!(simple.level(), DiagnosticLevel::Info);
    assert_eq!(
        simple.to_string(),
        "name: Rect.fromLTRB(0.0, 0.0, 20.0, 20.0)"
    );
    validate_diagnostics_property_json_serialization(&simple);

    let with_description =
        DiagnosticsProperty::<TestRect>::new("name", Some(rect)).description("small rect");
    assert_eq!(with_description.value(), Some(&rect));
    assert_eq!(with_description.level(), DiagnosticLevel::Info);
    assert_eq!(with_description.to_string(), "name: small rect");
    validate_diagnostics_property_json_serialization(&with_description);

    let null_property = DiagnosticsProperty::<TestObject>::new("name", None);
    assert_eq!(null_property.value(), None);
    assert_eq!(null_property.level(), DiagnosticLevel::Info);
    assert_eq!(null_property.to_string(), "name: null");
    validate_diagnostics_property_json_serialization(&null_property);

    let hide_null_property =
        DiagnosticsProperty::<TestObject>::new("name", None).default_value(None);
    assert_eq!(hide_null_property.value(), None);
    assert!(hide_null_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(hide_null_property.to_string(), "name: null");
    validate_diagnostics_property_json_serialization(&hide_null_property);

    let null_description = DiagnosticsProperty::<TestObject>::new("name", None).if_null("missing");
    assert_eq!(null_description.value(), None);
    assert_eq!(null_description.level(), DiagnosticLevel::Info);
    assert_eq!(null_description.to_string(), "name: missing");
    validate_diagnostics_property_json_serialization(&null_description);

    let hide_name = DiagnosticsProperty::<TestRect>::new("name", Some(rect))
        .with_show_name(false)
        .with_level(DiagnosticLevel::Warning);
    assert_eq!(hide_name.value(), Some(&rect));
    assert_eq!(hide_name.level(), DiagnosticLevel::Warning);
    assert_eq!(hide_name.to_string(), "Rect.fromLTRB(0.0, 0.0, 20.0, 20.0)");
    validate_diagnostics_property_json_serialization(&hide_name);

    let hide_separator =
        DiagnosticsProperty::<TestRect>::new("Creator", Some(rect)).with_show_separator(false);
    assert_eq!(hide_separator.value(), Some(&rect));
    assert_eq!(hide_separator.level(), DiagnosticLevel::Info);
    assert_eq!(
        hide_separator.to_string(),
        "Creator Rect.fromLTRB(0.0, 0.0, 20.0, 20.0)"
    );
    validate_diagnostics_property_json_serialization(&hide_separator);
}

#[test]
fn lazy_object_property_test() {
    let rect = TestRect;
    let simple = DiagnosticsProperty::<TestRect>::lazy("name", Rc::new(move || Some(rect)))
        .description("small rect");
    assert_eq!(simple.value(), Some(&rect));
    assert_eq!(simple.level(), DiagnosticLevel::Info);
    assert_eq!(simple.to_string(), "name: small rect");
    validate_diagnostics_property_json_serialization(&simple);

    let null_property =
        DiagnosticsProperty::<TestObject>::lazy("name", Rc::new(|| None::<TestObject>))
            .description("missing");
    assert_eq!(null_property.value(), None);
    assert!(!null_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(null_property.to_string(), "name: missing");
    validate_diagnostics_property_json_serialization(&null_property);

    let hide_null_property =
        DiagnosticsProperty::<TestObject>::lazy("name", Rc::new(|| None::<TestObject>))
            .description("missing")
            .default_value(None);
    assert_eq!(hide_null_property.value(), None);
    assert!(hide_null_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(hide_null_property.to_string(), "name: missing");
    validate_diagnostics_property_json_serialization(&hide_null_property);

    let hide_name = DiagnosticsProperty::<TestRect>::lazy("name", Rc::new(move || Some(rect)))
        .description("small rect")
        .with_show_name(false);
    assert_eq!(hide_name.value(), Some(&rect));
    assert!(!hide_name.is_filtered(DiagnosticLevel::Info));
    assert_eq!(hide_name.to_string(), "small rect");
    validate_diagnostics_property_json_serialization(&hide_name);
}

#[test]
fn color_property_test() {
    let color = TestColor;
    let simple = DiagnosticsProperty::<TestColor>::new("name", Some(color));
    assert!(!simple.is_filtered(DiagnosticLevel::Info));
    assert_eq!(simple.value(), Some(&color));
    assert_eq!(simple.to_string(), "name: Color(0xffffffff)");
    validate_diagnostics_property_json_serialization(&simple);
}

#[test]
fn flag_property_was_layout_test() {
    let show = FlagProperty::new("wasLayout", Some(true)).if_true("layout computed");
    assert_eq!(show.name(), Some("wasLayout"));
    assert_eq!(show.value(), Some(&true));
    assert!(!show.is_filtered(DiagnosticLevel::Info));
    assert_eq!(show.to_string(), "layout computed");
    validate_flag_property_json_serialization(&show);

    let hide = FlagProperty::new("wasLayout", Some(false)).if_true("layout computed");
    assert_eq!(hide.name(), Some("wasLayout"));
    assert_eq!(hide.value(), Some(&false));
    assert_eq!(hide.level(), DiagnosticLevel::Hidden);
    assert_eq!(hide.to_string(), "wasLayout: false");
    validate_flag_property_json_serialization(&hide);

    let hide_true = FlagProperty::new("wasLayout", Some(true)).if_false("no layout computed");
    assert_eq!(hide_true.name(), Some("wasLayout"));
    assert_eq!(hide_true.value(), Some(&true));
    assert_eq!(hide_true.level(), DiagnosticLevel::Hidden);
    assert_eq!(hide_true.to_string(), "wasLayout: true");
    validate_flag_property_json_serialization(&hide_true);
}

#[test]
fn has_property_test() {
    let has = ObjectFlagProperty::<TestClosure>::has("onClick", Some(TestClosure));
    assert_eq!(has.name(), Some("onClick"));
    assert_eq!(has.value(), Some(&TestClosure));
    assert!(!has.is_filtered(DiagnosticLevel::Info));
    assert_eq!(has.to_string(), "has onClick");
    validate_object_flag_property_json_serialization(&has);

    let missing = ObjectFlagProperty::<TestClosure>::has("onClick", None);
    assert_eq!(missing.name(), Some("onClick"));
    assert_eq!(missing.value(), None);
    assert!(missing.is_filtered(DiagnosticLevel::Info));
    assert_eq!(missing.to_string(), "onClick: null");
    validate_object_flag_property_json_serialization(&missing);
}

#[test]
fn iterable_flags_property_test() {
    let mut value = IndexMap::new();
    value.insert("click".to_string(), Some(TestClosure));
    value.insert("move".to_string(), Some(TestClosure));
    let flags = FlagsSummary::<TestClosure>::new("listeners", value);
    assert_eq!(flags.name(), Some("listeners"));
    assert_eq!(flags.value().len(), 2);
    assert!(!flags.is_filtered(DiagnosticLevel::Info));
    assert_eq!(flags.to_string(), "listeners: click, move");
    validate_iterable_flags_property_json_serialization(&flags);

    let mut value = IndexMap::new();
    value.insert("move".to_string(), Some(TestClosure));
    value.insert("click".to_string(), Some(TestClosure));
    let flags = FlagsSummary::<TestClosure>::new("listeners", value);
    assert_eq!(flags.to_string(), "listeners: move, click");
    assert!(!flags.is_filtered(DiagnosticLevel::Info));
    validate_iterable_flags_property_json_serialization(&flags);

    let mut value = IndexMap::new();
    value.insert("move".to_string(), None);
    value.insert("click".to_string(), Some(TestClosure));
    let flags = FlagsSummary::<TestClosure>::new("listeners", value);
    assert_eq!(flags.to_string(), "listeners: click");
    assert!(!flags.is_filtered(DiagnosticLevel::Info));
    validate_iterable_flags_property_json_serialization(&flags);

    let mut value = IndexMap::new();
    value.insert("enter".to_string(), None::<TestClosure>);
    let flags = FlagsSummary::<TestClosure>::new("listeners", value);
    assert!(flags.is_filtered(DiagnosticLevel::Info));
    validate_iterable_flags_property_json_serialization(&flags);

    let mut value = IndexMap::new();
    value.insert("enter".to_string(), None::<TestClosure>);
    let flags = FlagsSummary::<TestClosure>::new("listeners", value).if_empty("<none>");
    assert_eq!(flags.to_string(), "listeners: <none>");
    assert!(!flags.is_filtered(DiagnosticLevel::Info));
    validate_iterable_flags_property_json_serialization(&flags);
}

#[test]
fn iterable_property_test() {
    let ints = vec![1, 2, 3];
    let ints_property = IterableProperty::<i32>::new("ints", Some(ints.clone()));
    assert_eq!(ints_property.value(), Some(&ints));
    assert!(!ints_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(ints_property.to_string(), "ints: 1, 2, 3");
    validate_iterable_property_json_serialization(&ints_property);

    let doubles = vec![1.0, 2.0, 3.0];
    let doubles_property = IterableProperty::<f64>::new("doubles", Some(doubles.clone()));
    assert_eq!(doubles_property.value(), Some(&doubles));
    assert!(!doubles_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(doubles_property.to_string(), "doubles: 1.0, 2.0, 3.0");
    validate_iterable_property_json_serialization(&doubles_property);
    assert_eq!(
        json_strings(&serialize(&doubles_property), "values"),
        Some(vec!["1.0", "2.0", "3.0"])
    );

    let empty_property = IterableProperty::<TestObject>::new("name", Some(Vec::new()));
    assert_eq!(empty_property.value().map(Vec::len), Some(0));
    assert!(!empty_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(empty_property.to_string(), "name: []");
    validate_iterable_property_json_serialization(&empty_property);

    let null_property = IterableProperty::<TestObject>::new("list", None);
    assert_eq!(null_property.value(), None);
    assert!(!null_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(null_property.to_string(), "list: null");
    validate_iterable_property_json_serialization(&null_property);

    let hide_null_property = IterableProperty::<TestObject>::new("list", None).default_value(None);
    assert_eq!(hide_null_property.value(), None);
    assert!(hide_null_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(hide_null_property.level(), DiagnosticLevel::Fine);
    assert_eq!(hide_null_property.to_string(), "list: null");
    validate_iterable_property_json_serialization(&hide_null_property);

    let objects = vec![TestObject::Rect, TestObject::Color];
    let objects_property = IterableProperty::<TestObject>::new("objects", Some(objects.clone()));
    assert_eq!(objects_property.value(), Some(&objects));
    assert!(!objects_property.is_filtered(DiagnosticLevel::Info));
    validate_iterable_property_json_serialization(&objects_property);
    assert_eq!(
        objects_property.to_string(),
        "objects: Rect.fromLTRB(0.0, 0.0, 20.0, 20.0), Color(0xffffffff)"
    );

    let multi_line_property = IterableProperty::<TestObject>::new("objects", Some(objects.clone()))
        .with_style(DiagnosticsTreeStyle::Whitespace);
    assert_eq!(multi_line_property.value(), Some(&objects));
    assert!(!multi_line_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(
        multi_line_property.to_string(),
        concat!(
            "objects:\n",
            "Rect.fromLTRB(0.0, 0.0, 20.0, 20.0)\n",
            "Color(0xffffffff)",
        )
    );
    assert_eq!(
        multi_line_property.to_string_deep_with("", None, None, DiagnosticLevel::Debug, 100),
        concat!(
            "objects:\n",
            "  Rect.fromLTRB(0.0, 0.0, 20.0, 20.0)\n",
            "  Color(0xffffffff)\n",
        )
    );
    validate_iterable_property_json_serialization(&multi_line_property);

    assert_deep(
        &TestTree::new()
            .property(
                IterableProperty::<TestObject>::new("objects", Some(objects.clone()))
                    .with_style(DiagnosticsTreeStyle::Whitespace),
            )
            .to_string_deep_with("", None, DiagnosticLevel::Debug, 100),
        concat!(
            "TestTree#00000\n",
            "   objects:\n",
            "     Rect.fromLTRB(0.0, 0.0, 20.0, 20.0)\n",
            "     Color(0xffffffff)\n",
        ),
    );

    assert_deep(
        &TestTree::new()
            .property_rc(node(objects_property))
            .property(IntProperty::new("foo", 42_i64))
            .style(DiagnosticsTreeStyle::SingleLine)
            .to_string_deep(),
        "TestTree#00000(objects: [Rect.fromLTRB(0.0, 0.0, 20.0, 20.0), Color(0xffffffff)], foo: 42)",
    );

    // Iterable with a single entry. Verify that rendering is sensible and
    // that multiline rendering isn't used even though it is not helpful.
    let single_element_list = vec![TestObject::Color];

    let object_property =
        IterableProperty::<TestObject>::new("object", Some(single_element_list.clone()))
            .with_style(DiagnosticsTreeStyle::Whitespace);
    assert_eq!(object_property.value(), Some(&single_element_list));
    assert!(!object_property.is_filtered(DiagnosticLevel::Info));
    assert_eq!(object_property.to_string(), "object: Color(0xffffffff)");
    assert_eq!(
        object_property.to_string_deep_with("", None, None, DiagnosticLevel::Debug, 100),
        "object: Color(0xffffffff)\n"
    );
    validate_iterable_property_json_serialization(&object_property);
    assert_deep(
        &TestTree::new()
            .name("root")
            .property_rc(node(object_property))
            .to_string_deep_with("", None, DiagnosticLevel::Debug, 120),
        concat!("TestTree#00000\n", "   object: Color(0xffffffff)\n",),
    );
}

#[test]
fn message_test() {
    let message = message("hello world");
    assert_eq!(message.to_string(), "hello world");
    assert_eq!(message.name(), Some(""));
    assert_eq!(message.value(), None);
    assert!(!message.show_name());
    validate_node_json_serialization(&message);

    let message_property = MessageProperty::new("diagnostics", "hello world");
    assert_eq!(message_property.to_string(), "diagnostics: hello world");
    assert_eq!(message_property.name(), Some("diagnostics"));
    assert_eq!(message_property.value(), None);
    assert!(message_property.show_name());
    let json = serialize(&message_property);
    validate_property_json_map_with_type(
        &json,
        &message_property,
        message_property.property_data(),
        "void",
    );
}

fn create_tree_with_wrapping_nodes(
    root_style: Option<DiagnosticsTreeStyle>,
    property_style: DiagnosticsTreeStyle,
) -> DiagnosticableTreeNode {
    let long_message = "This is a very long message that must wrap as it cannot fit on one line. \
         This is a very long message that must wrap as it cannot fit on one line. \
         This is a very long message that must wrap as it cannot fit on one line.";
    let long_name = "This property has a very long property name that will be allowed to wrap \
         unlike most property names. This property has a very long property name that will be \
         allowed to wrap unlike most property names";
    let matrix = "[1.0, 0.0, 0.0, 0.0]\n\
         [1.0, 1.0, 0.0, 0.0]\n\
         [1.0, 0.0, 1.0, 0.0]\n\
         [1.0, 0.0, 0.0, 1.0]\n";
    TestTree::new()
        .name("Test tree")
        .property(message("--- example property at max length --").with_style(property_style))
        .property(message(long_message).with_style(property_style))
        .property(message("--- example property at max length --").with_style(property_style))
        .property(
            DiagnosticsProperty::<String>::new(
                None,
                Some(
                    "Message that is not allowed to wrap even though it is very long. \
                     Message that is not allowed to wrap even though it is very long. \
                     Message that is not allowed to wrap even though it is very long. \
                     Message that is not allowed to wrap."
                        .to_string(),
                ),
            )
            .with_allow_wrap(false),
        )
        .property(message("--- example property at max length --").with_style(property_style))
        .property(
            message(concat!(
                "This property has a very long property name that will be allowed to wrap unlike most property names. This property has a very long property name that will be allowed to wrap unlike most property names:\n",
                "  http://someverylongurl.com/that-might-be-tempting-to-wrap-even-though-it-is-a-url-so-should-not-wrap.html",
            ))
            .with_style(property_style),
        )
        .property(
            message(concat!(
                "This property has a very long property name that will be allowed to wrap unlike most property names. This property has a very long property name that will be allowed to wrap unlike most property names:\n",
                "  https://goo.gl/",
            ))
            .with_style(property_style),
        )
        .property(
            message(concat!(
                "Click on the following url:\n",
                "  http://someverylongurl.com/that-might-be-tempting-to-wrap-even-though-it-is-a-url-so-should-not-wrap.html",
            ))
            .with_style(property_style),
        )
        .property(
            message(concat!(
                "Click on the following url\n",
                "  https://goo.gl/",
            ))
            .with_style(property_style),
        )
        .property(message("--- example property at max length --").with_style(property_style))
        .property(
            DiagnosticsProperty::<String>::new("multi-line value", Some(matrix.to_string()))
                .with_style(property_style),
        )
        .property(message("--- example property at max length --").with_style(property_style))
        .property(
            DiagnosticsProperty::<String>::new(
                Some(long_name.to_string()),
                Some(long_message.to_string()),
            )
            .with_style(property_style),
        )
        .property(message("--- example property at max length --").with_style(property_style))
        .property(
            DiagnosticsProperty::<String>::new(
                Some(long_name.to_string()),
                Some(matrix.to_string()),
            )
            .with_style(property_style),
        )
        .property(message("--- example property at max length --").with_style(property_style))
        .property(
            MessageProperty::new(
                "diagnosis",
                "insufficient data to draw conclusion (less than five repaints)",
            )
            .with_style(property_style),
        )
        .debug_node(None, root_style)
}

#[test]
fn error_message_style_wrap_test() {
    let renderer = TextTreeRenderer::new()
        .wrap_width(40)
        .wrap_width_properties(40);
    assert_deep(
        &renderer.render(&create_tree_with_wrapping_nodes(
            Some(DiagnosticsTreeStyle::Error),
            DiagnosticsTreeStyle::SingleLine,
        )),
        concat!(
            "══╡ TESTTREE#00000 ╞════════════════════\n",
            "--- example property at max length --\n",
            "This is a very long message that must\n",
            "wrap as it cannot fit on one line. This\n",
            "is a very long message that must wrap as\n",
            "it cannot fit on one line. This is a\n",
            "very long message that must wrap as it\n",
            "cannot fit on one line.\n",
            "--- example property at max length --\n",
            "Message that is not allowed to wrap even though it is very long. Message that is not allowed to wrap even though it is very long. Message that is not allowed to wrap even though it is very long. Message that is not allowed to wrap.\n",
            "--- example property at max length --\n",
            "This property has a very long property\n",
            "name that will be allowed to wrap unlike\n",
            "most property names. This property has a\n",
            "very long property name that will be\n",
            "allowed to wrap unlike most property\n",
            "names:\n",
            "  http://someverylongurl.com/that-might-be-tempting-to-wrap-even-though-it-is-a-url-so-should-not-wrap.html\n",
            "This property has a very long property\n",
            "name that will be allowed to wrap unlike\n",
            "most property names. This property has a\n",
            "very long property name that will be\n",
            "allowed to wrap unlike most property\n",
            "names:\n",
            "  https://goo.gl/\n",
            "Click on the following url:\n",
            "  http://someverylongurl.com/that-might-be-tempting-to-wrap-even-though-it-is-a-url-so-should-not-wrap.html\n",
            "Click on the following url\n",
            "  https://goo.gl/\n",
            "--- example property at max length --\n",
            "multi-line value:\n",
            "  [1.0, 0.0, 0.0, 0.0]\n",
            "  [1.0, 1.0, 0.0, 0.0]\n",
            "  [1.0, 0.0, 1.0, 0.0]\n",
            "  [1.0, 0.0, 0.0, 1.0]\n",
            "--- example property at max length --\n",
            "This property has a very long property\n",
            "name that will be allowed to wrap unlike\n",
            "most property names. This property has a\n",
            "very long property name that will be\n",
            "allowed to wrap unlike most property\n",
            "names:\n",
            "  This is a very long message that must\n",
            "  wrap as it cannot fit on one line.\n",
            "  This is a very long message that must\n",
            "  wrap as it cannot fit on one line.\n",
            "  This is a very long message that must\n",
            "  wrap as it cannot fit on one line.\n",
            "--- example property at max length --\n",
            "This property has a very long property\n",
            "name that will be allowed to wrap unlike\n",
            "most property names. This property has a\n",
            "very long property name that will be\n",
            "allowed to wrap unlike most property\n",
            "names:\n",
            "  [1.0, 0.0, 0.0, 0.0]\n",
            "  [1.0, 1.0, 0.0, 0.0]\n",
            "  [1.0, 0.0, 1.0, 0.0]\n",
            "  [1.0, 0.0, 0.0, 1.0]\n",
            "--- example property at max length --\n",
            "diagnosis: insufficient data to draw\n",
            "  conclusion (less than five repaints)\n",
            "════════════════════════════════════════\n",
        ),
    );

    // This output looks ugly but verifies that no indentation on word wrap
    // leaks in if the style is flat.
    assert_deep(
        &renderer.render(&create_tree_with_wrapping_nodes(
            Some(DiagnosticsTreeStyle::Sparse),
            DiagnosticsTreeStyle::Flat,
        )),
        concat!(
            "TestTree#00000\n",
            "   --- example property at max length --\n",
            "   This is a very long message that must\n",
            "   wrap as it cannot fit on one line. This\n",
            "   is a very long message that must wrap as\n",
            "   it cannot fit on one line. This is a\n",
            "   very long message that must wrap as it\n",
            "   cannot fit on one line.\n",
            "   --- example property at max length --\n",
            "   Message that is not allowed to wrap even though it is very long. Message that is not allowed to wrap even though it is very long. Message that is not allowed to wrap even though it is very long. Message that is not allowed to wrap.\n",
            "   --- example property at max length --\n",
            "   This property has a very long property\n",
            "   name that will be allowed to wrap unlike\n",
            "   most property names. This property has a\n",
            "   very long property name that will be\n",
            "   allowed to wrap unlike most property\n",
            "   names:\n",
            "     http://someverylongurl.com/that-might-be-tempting-to-wrap-even-though-it-is-a-url-so-should-not-wrap.html\n",
            "   This property has a very long property\n",
            "   name that will be allowed to wrap unlike\n",
            "   most property names. This property has a\n",
            "   very long property name that will be\n",
            "   allowed to wrap unlike most property\n",
            "   names:\n",
            "     https://goo.gl/\n",
            "   Click on the following url:\n",
            "     http://someverylongurl.com/that-might-be-tempting-to-wrap-even-though-it-is-a-url-so-should-not-wrap.html\n",
            "   Click on the following url\n",
            "     https://goo.gl/\n",
            "   --- example property at max length --\n",
            "   multi-line value:\n",
            "   [1.0, 0.0, 0.0, 0.0]\n",
            "   [1.0, 1.0, 0.0, 0.0]\n",
            "   [1.0, 0.0, 1.0, 0.0]\n",
            "   [1.0, 0.0, 0.0, 1.0]\n",
            "   --- example property at max length --\n",
            "   This property has a very long property\n",
            "   name that will be allowed to wrap unlike\n",
            "   most property names. This property has a\n",
            "   very long property name that will be\n",
            "   allowed to wrap unlike most property\n",
            "   names:\n",
            "   This is a very long message that must\n",
            "   wrap as it cannot fit on one line. This\n",
            "   is a very long message that must wrap as\n",
            "   it cannot fit on one line. This is a\n",
            "   very long message that must wrap as it\n",
            "   cannot fit on one line.\n",
            "   --- example property at max length --\n",
            "   This property has a very long property\n",
            "   name that will be allowed to wrap unlike\n",
            "   most property names. This property has a\n",
            "   very long property name that will be\n",
            "   allowed to wrap unlike most property\n",
            "   names:\n",
            "   [1.0, 0.0, 0.0, 0.0]\n",
            "   [1.0, 1.0, 0.0, 0.0]\n",
            "   [1.0, 0.0, 1.0, 0.0]\n",
            "   [1.0, 0.0, 0.0, 1.0]\n",
            "   --- example property at max length --\n",
            "   diagnosis: insufficient data to draw\n",
            "   conclusion (less than five repaints)\n",
        ),
    );

    // This case matches the styles that should generally be used for error
    // messages.
    assert_deep(
        &renderer.render(&create_tree_with_wrapping_nodes(
            Some(DiagnosticsTreeStyle::Error),
            DiagnosticsTreeStyle::ErrorProperty,
        )),
        concat!(
            "══╡ TESTTREE#00000 ╞════════════════════\n",
            "--- example property at max length --\n",
            "This is a very long message that must\n",
            "wrap as it cannot fit on one line. This\n",
            "is a very long message that must wrap as\n",
            "it cannot fit on one line. This is a\n",
            "very long message that must wrap as it\n",
            "cannot fit on one line.\n",
            "--- example property at max length --\n",
            "Message that is not allowed to wrap even though it is very long. Message that is not allowed to wrap even though it is very long. Message that is not allowed to wrap even though it is very long. Message that is not allowed to wrap.\n",
            "--- example property at max length --\n",
            "This property has a very long property\n",
            "name that will be allowed to wrap unlike\n",
            "most property names. This property has a\n",
            "very long property name that will be\n",
            "allowed to wrap unlike most property\n",
            "names:\n",
            "  http://someverylongurl.com/that-might-be-tempting-to-wrap-even-though-it-is-a-url-so-should-not-wrap.html\n",
            "This property has a very long property\n",
            "name that will be allowed to wrap unlike\n",
            "most property names. This property has a\n",
            "very long property name that will be\n",
            "allowed to wrap unlike most property\n",
            "names:\n",
            "  https://goo.gl/\n",
            "Click on the following url:\n",
            "  http://someverylongurl.com/that-might-be-tempting-to-wrap-even-though-it-is-a-url-so-should-not-wrap.html\n",
            "Click on the following url\n",
            "  https://goo.gl/\n",
            "--- example property at max length --\n",
            "multi-line value:\n",
            "  [1.0, 0.0, 0.0, 0.0]\n",
            "  [1.0, 1.0, 0.0, 0.0]\n",
            "  [1.0, 0.0, 1.0, 0.0]\n",
            "  [1.0, 0.0, 0.0, 1.0]\n",
            "--- example property at max length --\n",
            "This property has a very long property\n",
            "name that will be allowed to wrap unlike\n",
            "most property names. This property has a\n",
            "very long property name that will be\n",
            "allowed to wrap unlike most property\n",
            "names:\n",
            "  This is a very long message that must\n",
            "  wrap as it cannot fit on one line.\n",
            "  This is a very long message that must\n",
            "  wrap as it cannot fit on one line.\n",
            "  This is a very long message that must\n",
            "  wrap as it cannot fit on one line.\n",
            "--- example property at max length --\n",
            "This property has a very long property\n",
            "name that will be allowed to wrap unlike\n",
            "most property names. This property has a\n",
            "very long property name that will be\n",
            "allowed to wrap unlike most property\n",
            "names:\n",
            "  [1.0, 0.0, 0.0, 0.0]\n",
            "  [1.0, 1.0, 0.0, 0.0]\n",
            "  [1.0, 0.0, 1.0, 0.0]\n",
            "  [1.0, 0.0, 0.0, 1.0]\n",
            "--- example property at max length --\n",
            "diagnosis:\n",
            "  insufficient data to draw conclusion\n",
            "  (less than five repaints)\n",
            "════════════════════════════════════════\n",
        ),
    );
}

#[test]
fn int_property_arguments_passed_to_super() {
    let property = IntProperty::new("Example", 0_i64)
        .if_null("is null")
        .with_show_name(false)
        .default_value(Some(1))
        .with_style(DiagnosticsTreeStyle::None)
        .with_level(DiagnosticLevel::Off);
    assert_eq!(property.value(), Some(&0));
    assert_eq!(
        property.property_data().effective_if_null(),
        Some("is null")
    );
    assert!(!property.show_name());
    assert_eq!(property.property_data().default_value, Some(Some(1)));
    assert_eq!(property.style(), Some(DiagnosticsTreeStyle::None));
    assert_eq!(property.level(), DiagnosticLevel::Off);
}

#[test]
fn arena_object_snapshot_renders_as_a_diagnosticable_tree_node() {
    let mut builder = DiagnosticPropertiesBuilder::new();
    builder.add(IntProperty::new("depth", 3_i64));
    let child = |name: &str| {
        node(DiagnosticableTreeNode::from_parts(
            name,
            "RenderPadding#a1f2c",
            DiagnosticPropertiesBuilder::new(),
            Vec::new(),
        ))
    };
    let node = DiagnosticableTreeNode::from_parts(
        None,
        "RenderPadding#a1f2c",
        builder,
        vec![child("child"), child("child 1")],
    );
    assert_deep(
        &node.to_string_deep(),
        concat!(
            "RenderPadding#00000\n",
            " │ depth: 3\n",
            " │\n",
            " ├─child: RenderPadding#00000\n",
            " └─child 1: RenderPadding#00000\n",
        ),
    );
}

#[test]
fn diagnosticable_tree_string_short_and_shallow_test() {
    let tree = TestTree::new()
        .property(StringProperty::new("name", Some("value".into())).quoted(false))
        .child(TestTree::new().name("child"));
    assert_deep(&tree.to_string_short(), "TestTree#00000");
    assert_deep(
        &tree.to_string_shallow(", ", DiagnosticLevel::Info),
        "TestTree#00000, name: value",
    );
}

#[test]
fn missing_if_null_derives_the_description() {
    let missing = DiagnosticsProperty::<i32>::new("name", None).missing_if_null(true);
    assert_eq!(missing.property_data().if_null.as_deref(), Some("MISSING"));
    assert_eq!(missing.to_string(), "name: MISSING");
    assert_eq!(missing.level(), DiagnosticLevel::Warning);

    let explicit_wins = DiagnosticsProperty::<i32>::new("name", None)
        .missing_if_null(true)
        .if_null("nothing");
    assert_eq!(
        explicit_wins.property_data().if_null.as_deref(),
        Some("nothing")
    );

    let explicit_before = DiagnosticsProperty::<i32>::new("name", None)
        .if_null("nothing")
        .missing_if_null(true);
    assert_eq!(
        explicit_before.property_data().if_null.as_deref(),
        Some("nothing")
    );

    let cleared = DiagnosticsProperty::<i32>::new("name", None)
        .missing_if_null(true)
        .missing_if_null(false);
    assert_eq!(cleared.property_data().if_null, None);
}

#[test]
fn snapshot_node_matches_a_live_tree_node() {
    let tree = TestTree::new()
        .style(DiagnosticsTreeStyle::Sparse)
        .property(IntProperty::new("depth", 3_i64))
        .child(
            TestTree::new()
                .name("child")
                .style(DiagnosticsTreeStyle::Sparse),
        );

    let live = tree
        .to_diagnostics_tree_node(None, Some(DiagnosticsTreeStyle::Sparse))
        .to_string_deep();

    let mut builder = DiagnosticPropertiesBuilder::new();
    tree.debug_fill_properties(&mut builder);
    let snapshot = DiagnosticableTreeNode::from_parts(
        None,
        tree.to_string_short(),
        builder,
        tree.debug_describe_children(),
    )
    .to_string_deep();

    assert_deep(&snapshot, &live);
}

/// The tree `diagnostics_json_test.dart` serializes.
fn serialization_test_tree() -> TestTree {
    TestTree::new()
        .property(StringProperty::new("stringProperty1", Some("value1".into())).quoted(false))
        .property(DoubleProperty::new("doubleProperty1", Some(42.5)))
        .property(DoubleProperty::new("roundedProperty", Some(1.0 / 3.0)))
        .property(
            StringProperty::new("DO_NOT_SHOW", Some("DO_NOT_SHOW".into()))
                .with_level(DiagnosticLevel::Hidden)
                .quoted(false),
        )
        .property(DiagnosticsProperty::<String>::new("DO_NOT_SHOW_NULL", None).default_value(None))
        .property(DiagnosticsProperty::<String>::new("nullProperty", None))
        .property(
            StringProperty::new("node_type", Some("<root node>".into()))
                .with_show_name(false)
                .quoted(false),
        )
        .child(TestTree::new().name("node A"))
        .child(
            TestTree::new()
                .name("node B")
                .property(StringProperty::new("p1", Some("v1".into())).quoted(false))
                .property(StringProperty::new("p2", Some("v2".into())).quoted(false))
                .child(TestTree::new().name("node B1"))
                .child(TestTree::new().name("node B2").property(
                    StringProperty::new("property1", Some("value1".into())).quoted(false),
                ))
                .child(
                    TestTree::new()
                        .name("node B3")
                        .property(
                            StringProperty::new("node_type", Some("<leaf node>".into()))
                                .with_show_name(false)
                                .quoted(false),
                        )
                        .property(IntProperty::new("foo", 42_i64)),
                ),
        )
        .child(
            TestTree::new().name("node C").property(
                StringProperty::new("foo", Some("multi\nline\nvalue!".into())).quoted(false),
            ),
        )
}

#[test]
fn serialization_default() {
    let tree = serialization_test_tree();
    let json = serialize(&tree.debug_node(None, None));
    assert!(!json.contains_key("properties"));
    assert!(!json.contains_key("children"));

    assert!(json.contains_key("description"), "description is included.");
    for key in ["type", "hasChildren", "allowWrap"] {
        assert!(json.contains_key(key), "{key} is included.");
    }
    assert_eq!(json_str(&json, "type"), Some("DiagnosticableTreeNode"));
}

#[test]
fn serialization_subtree_depth_1() {
    let tree = serialization_test_tree();
    let json = simulate_json_serialization(
        &tree.debug_node(None, None),
        &DefaultDiagnosticsSerializationDelegate::new(1, false),
    );
    assert!(!json.contains_key("properties"));
    let children = json
        .get("children")
        .and_then(Value::as_array)
        .expect("children");
    assert_eq!(children.len(), 3);
    for child in children {
        assert!(!child.as_object().expect("a node").contains_key("children"));
    }
}

#[test]
fn serialization_subtree_depth_5() {
    let tree = serialization_test_tree();
    let json = simulate_json_serialization(
        &tree.debug_node(None, None),
        &DefaultDiagnosticsSerializationDelegate::new(5, false),
    );
    assert!(!json.contains_key("properties"));
    let children = json
        .get("children")
        .and_then(Value::as_array)
        .expect("children");
    assert_eq!(
        children[0]
            .as_object()
            .expect("a node")
            .get("children")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        children[1]
            .as_object()
            .expect("a node")
            .get("children")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(
        children[2]
            .as_object()
            .expect("a node")
            .get("children")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[test]
fn serialization_include_properties() {
    let tree = serialization_test_tree();
    let json = simulate_json_serialization(
        &tree.debug_node(None, None),
        &DefaultDiagnosticsSerializationDelegate::new(0, true),
    );
    assert!(!json.contains_key("children"));
    assert_eq!(json_len(&json, "properties"), Some(7));
}

#[test]
fn serialization_include_properties_with_subtree_depth_1() {
    let tree = serialization_test_tree();
    let json = simulate_json_serialization(
        &tree.debug_node(None, None),
        &DefaultDiagnosticsSerializationDelegate::new(1, true),
    );
    assert_eq!(json_len(&json, "properties"), Some(7));
    let children = json
        .get("children")
        .and_then(Value::as_array)
        .expect("children");
    assert_eq!(children.len(), 3);
    assert_eq!(
        json_len(children[0].as_object().expect("a node"), "properties"),
        Some(0)
    );
    assert_eq!(
        json_len(children[1].as_object().expect("a node"), "properties"),
        Some(2)
    );
    assert_eq!(
        json_len(children[2].as_object().expect("a node"), "properties"),
        Some(1)
    );
}

#[test]
fn serialization_additional_node_properties() {
    let tree = serialization_test_tree();
    let mut additional = DiagnosticsJsonMap::new();
    additional.insert("foo".to_string(), Value::Bool(true));
    let delegate = TestDiagnosticsSerializationDelegate::new()
        .with_include_properties(true)
        .with_subtree_depth(1)
        .with_additional_node_properties(additional);
    let json = simulate_json_serialization(&tree.debug_node(None, None), &delegate);
    assert_eq!(json_bool(&json, "foo"), Some(true));
    let properties = json
        .get("properties")
        .and_then(Value::as_array)
        .expect("properties");
    assert_eq!(properties.len(), 7);
    assert!(
        properties
            .iter()
            .all(|property| json_bool(property.as_object().expect("a node"), "foo") == Some(true))
    );

    let children = json
        .get("children")
        .and_then(Value::as_array)
        .expect("children");
    assert_eq!(children.len(), 3);
    assert!(
        children
            .iter()
            .all(|child| json_bool(child.as_object().expect("a node"), "foo") == Some(true))
    );
}

#[test]
fn serialization_filter_properties_sublist() {
    let tree = serialization_test_tree();
    let delegate = TestDiagnosticsSerializationDelegate::new()
        .with_include_properties(true)
        .with_property_filter(|nodes, _owner| {
            nodes
                .into_iter()
                .filter(|node| node.type_name() == "StringProperty")
                .collect()
        });
    let json = simulate_json_serialization(&tree.debug_node(None, None), &delegate);
    let properties = json
        .get("properties")
        .and_then(Value::as_array)
        .expect("properties");
    assert_eq!(properties.len(), 3);
    assert!(properties.iter().all(|property| json_str(
        property.as_object().expect("a node"),
        "type"
    ) == Some("StringProperty")));
}

#[test]
fn serialization_filter_properties_replace() {
    let tree = serialization_test_tree();
    let replaced = Rc::new(std::cell::Cell::new(false));
    let delegate = TestDiagnosticsSerializationDelegate::new()
        .with_include_properties(true)
        .with_property_filter({
            let replaced = replaced.clone();
            move |nodes, _owner| {
                if replaced.get() {
                    return nodes;
                }
                replaced.set(true);
                vec![Rc::new(StringProperty::new("foo", Some("bar".into())))]
            }
        });
    let json = simulate_json_serialization(&tree.debug_node(None, None), &delegate);
    let properties = json
        .get("properties")
        .and_then(Value::as_array)
        .expect("properties");
    assert_eq!(properties.len(), 1);
    assert_eq!(
        json_str(properties[0].as_object().expect("a node"), "name"),
        Some("foo")
    );
}

#[test]
fn serialization_filter_children_sublist() {
    let tree = serialization_test_tree();
    let delegate = TestDiagnosticsSerializationDelegate::new()
        .with_subtree_depth(1)
        .with_child_filter(|nodes, _owner| {
            nodes
                .into_iter()
                .filter(|node| node.get_properties().is_empty())
                .collect()
        });
    let json = simulate_json_serialization(&tree.debug_node(None, None), &delegate);
    let children = json
        .get("children")
        .and_then(Value::as_array)
        .expect("children");
    assert_eq!(children.len(), 1);
}

#[test]
fn serialization_filter_children_replace() {
    let tree = serialization_test_tree();
    let delegate = TestDiagnosticsSerializationDelegate::new()
        .with_subtree_depth(1)
        .with_child_filter(|nodes, _owner| {
            nodes
                .into_iter()
                .flat_map(|node| node.get_children())
                .collect()
        });
    let json = simulate_json_serialization(&tree.debug_node(None, None), &delegate);
    let children = json
        .get("children")
        .and_then(Value::as_array)
        .expect("children");
    assert_eq!(children.len(), 3);
    assert_eq!(
        json_str(children[0].as_object().expect("a node"), "name"),
        Some("child node B1")
    );
}

#[test]
fn serialization_node_truncator() {
    let tree = serialization_test_tree();
    let delegate = TestDiagnosticsSerializationDelegate::new()
        .with_subtree_depth(5)
        .with_include_properties(true)
        .with_node_truncator(|nodes, _owner| nodes.into_iter().take(2).collect());
    let json = simulate_json_serialization(&tree.debug_node(None, None), &delegate);
    let children = json
        .get("children")
        .and_then(Value::as_array)
        .expect("children");
    assert_eq!(children.len(), 3);
    assert_eq!(
        json_bool(children[2].as_object().expect("a node"), "truncated"),
        Some(true)
    );

    let properties = json
        .get("properties")
        .and_then(Value::as_array)
        .expect("properties");
    assert_eq!(properties.len(), 3);
    assert_eq!(
        json_bool(properties[2].as_object().expect("a node"), "truncated"),
        Some(true)
    );
}

#[test]
fn serialization_delegate_for_adding_nodes() {
    let tree = serialization_test_tree();
    let delegate = TestDiagnosticsSerializationDelegate::new()
        .with_subtree_depth(5)
        .with_include_properties(true)
        .with_node_delegator(|_node, delegate| delegate.copy_with(None, Some(false)));
    let json = simulate_json_serialization(&tree.debug_node(None, None), &delegate);
    let properties = json
        .get("properties")
        .and_then(Value::as_array)
        .expect("properties");
    assert_eq!(properties.len(), 7);
    assert!(properties.iter().all(|property| {
        !property
            .as_object()
            .expect("a node")
            .contains_key("properties")
    }));

    let children = json
        .get("children")
        .and_then(Value::as_array)
        .expect("children");
    assert_eq!(children.len(), 3);
    assert!(children.iter().all(|child| {
        !child
            .as_object()
            .expect("a node")
            .contains_key("properties")
    }));
}

#[test]
fn diagnostics_property_for_basic_types_has_value_in_json() {
    let json = serialize(&DiagnosticsProperty::<i64>::new("int1", Some(10)));
    assert_eq!(json_str(&json, "name"), Some("int1"));
    assert_eq!(json.get("value"), Some(&Value::from(10)));

    let json = serialize(&IntProperty::<i64>::new("int2", Some(20)));
    assert_eq!(json_str(&json, "name"), Some("int2"));
    assert_eq!(json.get("value"), Some(&Value::from(20)));

    let json = serialize(&DiagnosticsProperty::<f64>::new("double", Some(33.3)));
    assert_eq!(json_str(&json, "name"), Some("double"));
    assert_eq!(json.get("value"), Some(&serde_json::json!(33.3)));

    let json = serialize(&DoubleProperty::new("double2", Some(33.3)));
    assert_eq!(json_str(&json, "name"), Some("double2"));
    assert_eq!(json.get("value"), Some(&serde_json::json!(33.3)));

    let json = serialize(&DiagnosticsProperty::<bool>::new("bool", Some(true)));
    assert_eq!(json_str(&json, "name"), Some("bool"));
    assert_eq!(json.get("value"), Some(&Value::Bool(true)));

    let json = serialize(&DiagnosticsProperty::<String>::new(
        "string1",
        Some("hello".into()),
    ));
    assert_eq!(json_str(&json, "name"), Some("string1"));
    assert_eq!(json_str(&json, "value"), Some("hello"));

    let json = serialize(&StringProperty::new("string2", Some("world".into())));
    assert_eq!(json_str(&json, "name"), Some("string2"));
    assert_eq!(json_str(&json, "value"), Some("world"));
}

#[test]
fn double_json_sanitizes_non_finite_values() {
    let json = serialize(&DoubleProperty::new("double1", Some(f64::INFINITY)));
    assert_eq!(json_str(&json, "value"), Some("Infinity"));

    let json = serialize(&DoubleProperty::new("double2", Some(f64::NEG_INFINITY)));
    assert_eq!(json_str(&json, "value"), Some("-Infinity"));
}

#[test]
fn json_default_value_strings_are_dart_to_strings() {
    let json = serialize(&DoubleProperty::new("name", Some(42.0)).default_value(Some(1.0)));
    assert_eq!(json_str(&json, "defaultValue"), Some("1.0"));

    let json = serialize(
        &EnumProperty::<ExampleEnum>::new("name", Some(ExampleEnum::Hello))
            .default_value(Some(ExampleEnum::Hello)),
    );
    assert_eq!(json_str(&json, "defaultValue"), Some("ExampleEnum.hello"));

    let json = serialize(
        &IterableProperty::<i64>::new("name", Some(vec![1, 2, 3])).default_value(Some(vec![1, 2])),
    );
    assert_eq!(json_str(&json, "defaultValue"), Some("[1, 2]"));
}

/// A node that leaves `to_json_map` at the trait's default, as painting's property types do.
struct BareNode;

impl fmt::Display for BareNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bare")
    }
}

impl DiagnosticsNode for BareNode {
    fn to_description(&self, _parent_configuration: Option<&TextTreeConfiguration>) -> String {
        "bare".to_string()
    }

    fn style(&self) -> Option<DiagnosticsTreeStyle> {
        Some(DiagnosticsTreeStyle::Sparse)
    }

    render_deep_with!();
}

#[test]
fn a_node_that_leaves_json_at_the_default_serializes_the_base_fields() {
    let json = serialize(&BareNode);
    assert_eq!(json_str(&json, "type"), Some("BareNode"));
    assert_eq!(json_str(&json, "description"), Some("bare"));
    assert!(!json.contains_key("propertyType"));

    // The delegate's per-node properties take the node as a trait object, which a provided
    // method cannot supply.
    let mut additional = DiagnosticsJsonMap::new();
    additional.insert("foo".to_string(), Value::Bool(true));
    let delegate =
        TestDiagnosticsSerializationDelegate::new().with_additional_node_properties(additional);
    let json = simulate_json_serialization(&BareNode, &delegate);
    assert!(!json.contains_key("foo"));
}
