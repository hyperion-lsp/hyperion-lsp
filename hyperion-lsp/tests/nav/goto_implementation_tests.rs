// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Goto Implementation feature
//!
//! Verifies that goto implementation navigates to:
//! - Implementing classes for interfaces
//! - Overriding methods for abstract methods
//! - The definition itself for concrete classes/methods

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::GotoDefinitionResponse;

/// Helper to extract all locations from GotoDefinitionResponse
fn extract_all_locations(response: GotoDefinitionResponse) -> Vec<tower_lsp::lsp_types::Location> {
    match response {
        GotoDefinitionResponse::Scalar(loc) => vec![loc],
        GotoDefinitionResponse::Array(locs) => locs,
        GotoDefinitionResponse::Link(links) => links
            .into_iter()
            .map(|l| tower_lsp::lsp_types::Location {
                uri: l.target_uri,
                range: l.target_selection_range,
            })
            .collect(),
    }
}

fn implementation_files(result: Result<Option<GotoDefinitionResponse>, String>) -> Vec<String> {
    let response = result
        .expect("goto implementation request should succeed")
        .expect("goto implementation should return locations");
    let mut files: Vec<_> = extract_all_locations(response)
        .into_iter()
        .map(|location| {
            location
                .uri
                .to_file_path()
                .expect("implementation URI should be a file")
                .file_name()
                .expect("implementation path should have a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    files.sort();
    files
}

/// Test: Interface -> Implementing class
#[test]
fn test_goto_implementation_interface_to_class() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/MyInterface.java",
        r"
package com.example;

public interface MyInterface {
    void doSomething();
}
",
    );

    test.add_file(
        "com/example/MyImpl.java",
        r#"
package com.example;

public class MyImpl implements MyInterface {
    @Override
    public void doSomething() {
        System.out.println("Implemented!");
    }
}
"#,
    );

    test.add_file(
        "com/example/Main.java",
        r"
package com.example;

public class Main {
    void test() {
        MyInter<|>face obj = new MyImpl();
    }
}
",
    );

    // Test goto_implementation: should find MyImpl as implementation
    assert_eq!(
        implementation_files(test.goto_implementation("com/example/Main.java", "<|>")),
        vec!["MyImpl.java"]
    );
}

#[test]
fn interface_redeclaration_is_not_reported_as_concrete_implementation() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Parent.java",
        r"package com.example;
public interface Parent { void ru<|>n(); }
",
    );
    test.add_file(
        "com/example/ChildContract.java",
        r"package com.example;
public interface ChildContract extends Parent { void run(); }
",
    );
    test.add_file(
        "com/example/Implementation.java",
        r"package com.example;
public class Implementation implements ChildContract { public void run() {} }
",
    );

    assert_eq!(
        implementation_files(test.goto_implementation("com/example/Parent.java", "<|>")),
        vec!["Implementation.java"]
    );
}

/// Test: Multiple implementations - should return all implementing classes
#[test]
fn test_multiple_implementations() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Animal.java",
        r"
package com.example;

public interface Animal {
    void speak();
}
",
    );

    test.add_file(
        "com/example/Dog.java",
        r#"
package com.example;

public class Dog implements Animal {
    @Override
    public void speak() {
        System.out.println("Woof!");
    }
}
"#,
    );

    test.add_file(
        "com/example/Cat.java",
        r#"
package com.example;

public class Cat implements Animal {
    @Override
    public void speak() {
        System.out.println("Meow!");
    }
}
"#,
    );

    test.add_file(
        "com/example/Main.java",
        r"
package com.example;

public class Main {
    void test(Anim<|>al animal) {
        animal.speak();
    }
}
",
    );

    assert_eq!(
        implementation_files(test.goto_implementation("com/example/Main.java", "<|>")),
        vec!["Cat.java", "Dog.java"]
    );
}

/// Test: Abstract class with concrete implementation
#[test]
fn test_abstract_class_implementation() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/AbstractBase.java",
        r#"
package com.example;

public abstract class AbstractBase {
    public abstract void process();

    public void common() {
        System.out.println("Common behavior");
    }
}
"#,
    );

    test.add_file(
        "com/example/ConcreteImpl.java",
        r#"
package com.example;

public class ConcreteImpl extends AbstractBase {
    @Override
    public void process() {
        System.out.println("Processing...");
    }
}
"#,
    );

    test.add_file(
        "com/example/Main.java",
        r"
package com.example;

public class Main {
    void test() {
        Abstract<|>Base base = new ConcreteImpl();
        base.process();
    }
}
",
    );

    assert_eq!(
        implementation_files(test.goto_implementation("com/example/Main.java", "<|>")),
        vec!["ConcreteImpl.java"]
    );
}

/// Abstract and interface intermediaries must not hide concrete descendants.
#[test]
fn test_goto_implementation_walks_through_abstract_and_interface_layers() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Root.java",
        r"
package com.example;

public interface Root {
    void run();
}
",
    );
    test.add_file(
        "com/example/Derived.java",
        r"
package com.example;

public interface Derived extends Root {}
",
    );
    test.add_file(
        "com/example/AbstractRunner.java",
        r"
package com.example;

public abstract class AbstractRunner implements Derived {
    public abstract void run();
}
",
    );
    test.add_file(
        "com/example/Runner.java",
        r"
package com.example;

public class Runner extends AbstractRunner {
    @Override
    public void run() {}
}
",
    );
    test.add_file(
        "com/example/Main.java",
        r"
package com.example;

class Main {
    Ro<|>ot runner = new Runner();
}
",
    );

    assert_eq!(
        implementation_files(test.goto_implementation("com/example/Main.java", "<|>")),
        vec!["Runner.java"]
    );
}

#[test]
fn goto_implementation_on_method_selects_the_matching_override() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Service.java",
        r"package com.example;

public interface Service {
    void run();
    void ru<|>n(String value);
}",
    );
    test.add_file(
        "com/example/ServiceImpl.java",
        r"package com.example;

public final class ServiceImpl implements Service {
    public void run() {}

    public void run(String value) {}
}",
    );

    let response = test
        .goto_implementation("com/example/Service.java", "<|>")
        .expect("goto implementation request")
        .expect("matching implementation");
    let locations = extract_all_locations(response);

    assert_eq!(
        locations.len(),
        1,
        "expected one exact override: {locations:?}"
    );
    let location = &locations[0];
    assert!(
        location.uri.as_str().ends_with("ServiceImpl.java"),
        "expected concrete implementation file, got {}",
        location.uri
    );
    assert_eq!(
        location.range.start.line, 5,
        "must select run(String), not the class or zero-argument overload"
    );
}

#[test]
fn goto_implementation_on_method_call_selects_the_matching_override() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Service.java",
        r"package com.example;

public interface Service {
    void run();
    void run(String value);
}",
    );
    test.add_file(
        "com/example/ServiceImpl.java",
        r"package com.example;

public final class ServiceImpl implements Service {
    public void run() {}

    public void run(String value) {}
}",
    );
    test.add_file(
        "com/example/Main.java",
        r#"package com.example;

public final class Main {
    void invoke(Service service) {
        service.ru<|>n("value");
    }
}"#,
    );

    let response = test
        .goto_implementation("com/example/Main.java", "<|>")
        .expect("goto implementation request")
        .expect("matching implementation");
    let locations = extract_all_locations(response);

    assert_eq!(
        locations.len(),
        1,
        "expected one exact override: {locations:?}"
    );
    assert!(locations[0].uri.as_str().ends_with("ServiceImpl.java"));
    assert_eq!(locations[0].range.start.line, 5);
}

/// Test: Concrete class - should return the definition itself
#[test]
fn test_concrete_class_returns_definition() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/ConcreteClass.java",
        r#"
package com.example;

public class ConcreteClass {
    public void doWork() {
        System.out.println("Working...");
    }
}
"#,
    );

    test.add_file(
        "com/example/Main.java",
        r"
package com.example;

public class Main {
    void test() {
        Concrete<|>Class obj = new ConcreteClass();
    }
}
",
    );

    assert_eq!(
        implementation_files(test.goto_implementation("com/example/Main.java", "<|>")),
        vec!["ConcreteClass.java"]
    );
}

/// Enums implementing an interface must appear as implementations.
#[test]
fn test_goto_implementation_interface_to_enum() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Printable.java",
        r"
package com.example;

public interface Printable {
    void print();
}
",
    );

    test.add_file(
        "com/example/Color.java",
        r#"
package com.example;

public enum Color implements Printable {
    RED, GREEN;

    @Override
    public void print() {
        System.out.println(name());
    }
}
"#,
    );

    test.add_file(
        "com/example/Main.java",
        r"
package com.example;

public class Main {
    void test() {
        Print<|>able p = Color.RED;
    }
}
",
    );

    assert_eq!(
        implementation_files(test.goto_implementation("com/example/Main.java", "<|>")),
        vec!["Color.java"]
    );
}

/// Records implementing an interface must appear as implementations.
#[test]
fn test_goto_implementation_interface_to_record() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Shape.java",
        r"
package com.example;

public interface Shape {
    double area();
}
",
    );

    test.add_file(
        "com/example/Circle.java",
        r"
package com.example;

public record Circle(double radius) implements Shape {
    @Override
    public double area() {
        return Math.PI * radius * radius;
    }
}
",
    );

    test.add_file(
        "com/example/Main.java",
        r"
package com.example;

public class Main {
    void test() {
        Sha<|>pe s = new Circle(1.0);
    }
}
",
    );

    assert_eq!(
        implementation_files(test.goto_implementation("com/example/Main.java", "<|>")),
        vec!["Circle.java"]
    );
}
