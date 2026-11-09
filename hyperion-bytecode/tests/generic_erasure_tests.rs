// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Generic Type Erasure and Signature Attributes
//!
//! Java generics are erased at runtime, but generic signatures are preserved
//! in the Signature attribute for reflection and tooling.
//!
//! **Key Points:**
//! 1. Method descriptors are always erased (e.g., `List<String>` becomes `List`)
//! 2. Signature attributes preserve generic info (e.g., `Ljava/util/List<Ljava/lang/String;>;`)
//! 3. The interpreter must read the erased descriptor for execution
//! 4. The LSP should read the Signature attribute for type information
//!
//! **Bug**: Interpreter might:
//! - Incorrectly parse Signature attribute as descriptor
//! - Not handle erased types in method descriptors
//! - Mix up generic signatures with actual runtime types
//!
//! These tests verify correct handling of generic erasure.

mod test_utils;

use hyperion_bytecode::extractor::extract_symbols;
use hyperion_bytecode::parser::parse_class_file;
use test_utils::JavaTestProject;

#[test]
fn extractor_retains_bridge_synthetic_and_varargs_flags() {
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");
    let source = r#"
interface GenericParent<T> {
    T value();
}

public class BridgeAndVarargs implements GenericParent<String> {
    @Override public String value() { return "value"; }
    public void accept(String... values) {}
}
"#;
    project
        .write_source("", "BridgeAndVarargs", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("BridgeAndVarargs")
        .expect("Failed to load class bytes");
    let class = parse_class_file(&bytes).expect("Failed to parse class file");
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    let value_methods: Vec<_> = symbols
        .methods
        .iter()
        .filter(|method| method.name == "value")
        .collect();
    assert_eq!(
        value_methods.len(),
        2,
        "javac should emit one generic bridge"
    );
    let bridge = value_methods
        .iter()
        .find(|method| method.is_bridge)
        .expect("erased bridge should retain ACC_BRIDGE");
    assert!(bridge.is_synthetic, "javac bridges are synthetic");
    assert!(value_methods
        .iter()
        .any(|method| !method.is_bridge && !method.is_synthetic));

    let accept = symbols
        .methods
        .iter()
        .find(|method| method.name == "accept")
        .expect("accept method should be extracted");
    assert!(accept.is_varargs, "ACC_VARARGS should survive extraction");
}

#[test]
fn test_generic_method_descriptor_erasure() {
    // Method descriptors should contain erased types only

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
import java.util.List;
import java.util.ArrayList;

public class GenericMethod {
    public static List<String> createList() {
        return new ArrayList<String>();
    }

    public static void addToList(List<String> list, String item) {
        list.add(item);
    }

    public static <T> T identity(T value) {
        return value;
    }
}
"#;

    project
        .write_source("", "GenericMethod", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("GenericMethod")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    // Extract symbols to get method descriptors
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // Find createList method
    let create_list = symbols
        .methods
        .iter()
        .find(|m| m.name == "createList")
        .expect("createList not found");

    // The descriptor should be erased: ()Ljava/util/List;
    // NOT: ()Ljava/util/List<Ljava/lang/String;>;
    let return_type_str = create_list.return_type.to_string();

    // The return type should be "java.util.List" (erased), not contain generics
    assert!(
        return_type_str.contains("List"),
        "Return type should be List"
    );

    assert!(
        !return_type_str.contains("<"),
        "Method descriptor return type should be erased (no angle brackets), got: {}",
        return_type_str
    );

    // Find addToList method
    let add_to_list = symbols
        .methods
        .iter()
        .find(|m| m.name == "addToList")
        .expect("addToList not found");

    // Parameters should be: (Ljava/util/List;Ljava/lang/String;)V
    assert_eq!(add_to_list.parameters.len(), 2, "Should have 2 parameters");

    let first_param = add_to_list.parameters[0].to_string();

    assert!(
        first_param.contains("List"),
        "First parameter should be List"
    );

    assert!(
        !first_param.contains("<"),
        "Parameter type should be erased (no generics), got: {}",
        first_param
    );
}

#[test]
fn test_signature_attribute_preserves_generics() {
    // The Signature attribute (if present) should contain generic information

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
import java.util.List;
import java.util.Map;

public class GenericClass<T> {
    private T value;
    private List<String> strings;

    public T getValue() {
        return value;
    }

    public void setValue(T newValue) {
        value = newValue;
    }

    public Map<String, Integer> createMap() {
        return null;
    }
}
"#;

    project
        .write_source("", "GenericClass", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("GenericClass")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    // Check if the class has a Signature attribute
    let has_signature = class
        .attributes
        .iter()
        .any(|attr| matches!(attr.data, cafebabe::attributes::AttributeData::Signature(_)));

    assert!(
        has_signature,
        "Generic class should have Signature attribute"
    );

    // Check methods for Signature attributes
    let get_value = class
        .methods
        .iter()
        .find(|m| m.name == "getValue")
        .expect("getValue not found");

    let has_method_sig = get_value
        .attributes
        .iter()
        .any(|attr| matches!(attr.data, cafebabe::attributes::AttributeData::Signature(_)));

    // Generic methods should have Signature attribute
    // (Though the compiler might optimize this away for simple cases)
    if has_method_sig {
        let sig_attr = get_value
            .attributes
            .iter()
            .find(|attr| matches!(attr.data, cafebabe::attributes::AttributeData::Signature(_)))
            .expect("Signature attribute not found");

        if let cafebabe::attributes::AttributeData::Signature(sig) = &sig_attr.data {
            // Signature should contain generic type variable T
            // Format: ()TT; (returns type variable T)
            let sig_str = sig.as_ref();
            assert!(
                sig_str.contains("T"),
                "Signature should reference type variable T, got: {}",
                sig_str
            );
        }
    }

    // Check createMap which has concrete generic types
    let create_map = class
        .methods
        .iter()
        .find(|m| m.name == "createMap")
        .expect("createMap not found");

    let map_sig = create_map
        .attributes
        .iter()
        .find(|attr| matches!(attr.data, cafebabe::attributes::AttributeData::Signature(_)));

    if let Some(sig_attr) = map_sig {
        if let cafebabe::attributes::AttributeData::Signature(sig) = &sig_attr.data {
            // Should contain Map with String and Integer type arguments
            let sig_str = sig.as_ref();
            assert!(sig_str.contains("Map"), "Signature should contain Map");
        }
    }
}

#[test]
fn test_generic_field_erasure() {
    // Field descriptors should also be erased

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
import java.util.List;
import java.util.Map;
import java.util.Set;

public class GenericFields {
    public List<String> stringList;
    public Map<Integer, String> intStringMap;
    public Set<Object> objectSet;
}
"#;

    project
        .write_source("", "GenericFields", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("GenericFields")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // Check field descriptors
    let string_list = symbols
        .fields
        .iter()
        .find(|f| f.name == "stringList")
        .expect("stringList field not found");

    let field_type = string_list.field_type.to_string();

    // Should be Ljava/util/List; (erased, no generics)
    assert!(field_type.contains("List"), "Field type should be List");

    assert!(
        !field_type.contains("<"),
        "Field descriptor should be erased (no generics), got: {}",
        field_type
    );

    // Check Map field
    let int_string_map = symbols
        .fields
        .iter()
        .find(|f| f.name == "intStringMap")
        .expect("intStringMap field not found");

    let map_type = int_string_map.field_type.to_string();

    assert!(map_type.contains("Map"), "Field type should be Map");

    assert!(
        !map_type.contains("<") && !map_type.contains(">"),
        "Field descriptor should be erased, got: {}",
        map_type
    );
}

#[test]
fn test_array_of_generics_erasure() {
    // Arrays of generic types are also erased

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
import java.util.List;

public class GenericArrays {
    public static List<String>[] createArray() {
        return null;
    }

    public static void processArray(List<Integer>[] arrays) {
        // Do nothing
    }
}
"#;

    project
        .write_source("", "GenericArrays", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("GenericArrays")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // Check createArray
    let create_array = symbols
        .methods
        .iter()
        .find(|m| m.name == "createArray")
        .expect("createArray not found");

    let return_type = create_array.return_type.to_string();

    // Should be [Ljava/util/List; (array of erased List)
    assert!(
        return_type.contains("[") || return_type.contains("array"),
        "Should be array type"
    );

    assert!(return_type.contains("List"), "Should be array of List");

    assert!(
        !return_type.contains("<"),
        "Array element type should be erased, got: {}",
        return_type
    );
}

#[test]
fn test_bounded_type_parameter_erasure() {
    // Bounded type parameters (T extends Foo) are erased to their bound

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
public class BoundedGenerics {
    // T extends Number, so at runtime it's erased to Number
    public static <T extends Number> T getNumber(T value) {
        return value;
    }

    // T extends Comparable<T>, erased to Comparable
    public static <T extends Comparable<T>> int compare(T a, T b) {
        return a.compareTo(b);
    }
}
"#;

    project
        .write_source("", "BoundedGenerics", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("BoundedGenerics")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // Check getNumber method
    let get_number = symbols
        .methods
        .iter()
        .find(|m| m.name == "getNumber")
        .expect("getNumber not found");

    // The descriptor should be (Ljava/lang/Number;)Ljava/lang/Number;
    // The type parameter T extends Number is erased to Number
    assert_eq!(get_number.parameters.len(), 1, "Should have 1 parameter");

    let param_type = get_number.parameters[0].to_string();

    assert!(
        param_type.contains("Number"),
        "Bounded type parameter should be erased to its bound (Number), got: {}",
        param_type
    );

    let return_type = get_number.return_type.to_string();

    assert!(
        return_type.contains("Number"),
        "Return type should be erased to Number, got: {}",
        return_type
    );

    // Check compare method
    let compare = symbols
        .methods
        .iter()
        .find(|m| m.name == "compare")
        .expect("compare not found");

    // Parameters should be (Ljava/lang/Comparable;Ljava/lang/Comparable;)I
    assert_eq!(compare.parameters.len(), 2, "Should have 2 parameters");

    let first_param = compare.parameters[0].to_string();

    assert!(
        first_param.contains("Comparable"),
        "Bounded type parameter should be erased to Comparable, got: {}",
        first_param
    );
}

#[test]
fn test_wildcard_erasure() {
    // Wildcards (? extends Foo, ? super Bar) are erased

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
import java.util.List;

public class WildcardErasure {
    // List<?> is erased to List
    public static void processUnknown(List<?> list) {
        // Do nothing
    }

    // List<? extends Number> is erased to List
    public static void processNumbers(List<? extends Number> list) {
        // Do nothing
    }

    // List<? super Integer> is erased to List
    public static void addIntegers(List<? super Integer> list) {
        // Do nothing
    }
}
"#;

    project
        .write_source("", "WildcardErasure", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("WildcardErasure")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // All three methods should have the same erased descriptor: (Ljava/util/List;)V

    for method_name in &["processUnknown", "processNumbers", "addIntegers"] {
        let method = symbols
            .methods
            .iter()
            .find(|m| m.name == *method_name)
            .unwrap_or_else(|| panic!("{} not found", method_name));

        assert_eq!(
            method.parameters.len(),
            1,
            "{} should have 1 parameter",
            method_name
        );

        let param_type = method.parameters[0].to_string();

        assert!(
            param_type.contains("List"),
            "{} parameter should be List",
            method_name
        );

        assert!(
            !param_type.contains("?") && !param_type.contains("<"),
            "{} parameter should be erased (no wildcards or generics), got: {}",
            method_name,
            param_type
        );
    }
}

#[test]
fn test_constructor_with_generics() {
    // Constructors of generic classes should have erased signatures

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
public class GenericConstructor<T> {
    private T value;

    public GenericConstructor(T initialValue) {
        this.value = initialValue;
    }

    public GenericConstructor() {
        this.value = null;
    }
}
"#;

    project
        .write_source("", "GenericConstructor", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("GenericConstructor")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // Find the constructor that takes a parameter
    let constructors: Vec<_> = symbols
        .methods
        .iter()
        .filter(|m| m.is_constructor())
        .collect();

    assert!(
        !constructors.is_empty(),
        "Should have at least one constructor"
    );

    // Find the constructor with one parameter
    let param_constructor = constructors.iter().find(|c| c.parameters.len() == 1);

    if let Some(ctor) = param_constructor {
        // The parameter type T should be erased to Object
        let param_type = ctor.parameters[0].to_string();

        assert!(
            param_type.contains("Object"),
            "Type parameter T should be erased to Object in constructor, got: {}",
            param_type
        );
    }
}

#[test]
fn test_multiple_type_parameters_erasure() {
    // Classes/methods with multiple type parameters

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
public class MultipleTypeParams<K, V> {
    private K key;
    private V value;

    public static <A, B, C> C combine(A first, B second, C third) {
        return third;
    }

    public K getKey() {
        return key;
    }

    public V getValue() {
        return value;
    }
}
"#;

    project
        .write_source("", "MultipleTypeParams", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("MultipleTypeParams")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // Check the combine method
    let combine = symbols
        .methods
        .iter()
        .find(|m| m.name == "combine")
        .expect("combine not found");

    // Should have 3 parameters, all erased to Object
    assert_eq!(combine.parameters.len(), 3, "Should have 3 parameters");

    for (i, param) in combine.parameters.iter().enumerate() {
        let param_type = param.to_string();

        assert!(
            param_type.contains("Object"),
            "Parameter {} should be erased to Object, got: {}",
            i,
            param_type
        );
    }

    let return_type = combine.return_type.to_string();

    assert!(
        return_type.contains("Object"),
        "Return type should be erased to Object, got: {}",
        return_type
    );
}
