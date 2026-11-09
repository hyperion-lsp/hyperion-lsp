// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::Position;

fn position_of(source: &str, needle: &str) -> Position {
    let offset = source.find(needle).expect("fixture marker");
    let prefix = &source[..offset];
    Position::new(
        prefix.bytes().filter(|byte| *byte == b'\n').count() as u32,
        prefix
            .rsplit_once('\n')
            .map_or(prefix, |(_, line)| line)
            .len() as u32,
    )
}

#[test]
fn test_prepare_call_hierarchy_on_method() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Service.java",
        r#"
        package com.example;

        public class Service {
            public void processData() {
                System.out.println("Processing");
            }

            public void handleRequest() {
                processData();
            }
        }
    "#,
    );

    // Test prepare call hierarchy on the processData method declaration
    let result = test.prepare_call_hierarchy(
        "com/example/Service.java",
        Position::new(4, 24), // On "processData"
    );

    match result {
        Ok(Some(items)) => {
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item.name.as_str(), "processData");
            println!("Call hierarchy item: {:?}", item);
        }
        Ok(None) => {
            panic!("Expected call hierarchy item, got None");
        }
        Err(e) => {
            panic!("prepare_call_hierarchy failed: {}", e);
        }
    }
}

#[test]
fn test_prepare_call_hierarchy_on_constructor() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/User.java",
        r"
        package com.example;

        public class User {
            private String name;

            public User(String name) {
                this.name = name;
            }
        }
    ",
    );

    // Test prepare call hierarchy on constructor
    let result = test.prepare_call_hierarchy(
        "com/example/User.java",
        Position::new(6, 19), // On "User" constructor
    );

    match result {
        Ok(Some(items)) => {
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item.name.as_str(), "User");
            println!("Constructor call hierarchy item: {:?}", item);
        }
        Ok(None) => panic!("Expected constructor call hierarchy item"),
        Err(e) => {
            panic!("prepare_call_hierarchy failed: {}", e);
        }
    }
}

#[test]
fn test_prepare_call_hierarchy_not_on_method() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r"
        package com.example;

        public class Test {
            private int value = 10;
        }
    ",
    );

    // Test prepare call hierarchy on a field (should return None)
    let result = test.prepare_call_hierarchy(
        "com/example/Test.java",
        Position::new(4, 24), // On "value" field
    );

    assert!(matches!(result, Ok(None)));
}

#[test]
fn test_incoming_calls_basic() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Calculator.java",
        r"
        package com.example;

        public class Calculator {
            public int add(int a, int b) {
                return a + b;
            }

            public int calculate() {
                return add(1, 2) + add(3, 4);
            }

            public int total() {
                return add(10, 20);
            }
        }
    ",
    );

    // First prepare call hierarchy on "add" method
    let prepare_result = test.prepare_call_hierarchy(
        "com/example/Calculator.java",
        Position::new(4, 23), // On "add"
    );

    match prepare_result {
        Ok(Some(items)) => {
            assert!(!items.is_empty());
            let item = &items[0];

            // Now get incoming calls
            let incoming_result = test.incoming_calls(item.clone());
            match incoming_result {
                Ok(Some(calls)) => {
                    println!("Found {} incoming callers", calls.len());
                    for call in &calls {
                        println!(
                            "  Caller: {} with {} call sites",
                            call.from.name,
                            call.from_ranges.len()
                        );
                    }
                    assert_eq!(calls.len(), 2);
                    assert_eq!(calls[0].from.name, "calculate");
                    assert_eq!(calls[0].from_ranges.len(), 2);
                    assert_eq!(calls[1].from.name, "total");
                    assert_eq!(calls[1].from_ranges.len(), 1);
                }
                Ok(None) => panic!("Expected incoming calls from calculate and total"),
                Err(e) => {
                    panic!("incoming_calls failed: {}", e);
                }
            }
        }
        Ok(None) => {
            panic!("Expected call hierarchy item");
        }
        Err(e) => {
            panic!("prepare_call_hierarchy failed: {}", e);
        }
    }
}

#[test]
fn test_outgoing_calls_basic() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/App.java",
        r#"
        package com.example;

        public class App {
            public void init() {
                System.out.println("init");
            }

            public void start() {
                System.out.println("start");
            }

            public void run() {
                init();
                start();
                System.out.println("running");
            }
        }
    "#,
    );

    // Prepare call hierarchy on "run" method
    let prepare_result = test.prepare_call_hierarchy(
        "com/example/App.java",
        Position::new(12, 24), // On "run"
    );

    match prepare_result {
        Ok(Some(items)) => {
            assert!(!items.is_empty());
            let item = &items[0];

            // Now get outgoing calls
            let outgoing_result = test.outgoing_calls(item.clone());
            match outgoing_result {
                Ok(Some(calls)) => {
                    println!("Found {} outgoing calls", calls.len());
                    for call in &calls {
                        println!(
                            "  Calls: {} with {} call sites",
                            call.to.name,
                            call.from_ranges.len()
                        );
                    }
                    assert_eq!(calls.len(), 2);
                    let names: Vec<_> = calls.iter().map(|call| call.to.name.as_str()).collect();
                    assert_eq!(names, vec!["init", "start"]);
                    assert!(calls.iter().all(|call| call.from_ranges.len() == 1));
                }
                Ok(None) => panic!("Expected outgoing calls to init and start"),
                Err(e) => {
                    panic!("outgoing_calls failed: {}", e);
                }
            }
        }
        Ok(None) => {
            panic!("Expected call hierarchy item");
        }
        Err(e) => {
            panic!("prepare_call_hierarchy failed: {}", e);
        }
    }
}

/// Cross-file outgoing calls use the same canonical receiver/type resolution
/// as goto-definition rather than a call-hierarchy-specific approximation.
#[test]
fn test_outgoing_calls_with_receiver_type_inference() {
    let mut test = LspTest::new();

    // User class in a separate file
    test.add_file(
        "com/example/model/User.java",
        r"
        package com.example.model;

        public class User {
            private String name;

            public String getName() {
                return name;
            }

            public void setName(String name) {
                this.name = name;
            }
        }
    ",
    );

    // Service that uses User - THIS is where the bug was
    test.add_file(
        "com/example/Service.java",
        r"
        package com.example;

        import com.example.model.User;

        public class Service {
            public String processUser(User user) {
                // The receiver type must be inferred from the 'user' parameter
                // to resolve user.getName() -> com.example.model.User.getName
                return user.getName();
            }
        }
    ",
    );

    // Prepare call hierarchy on "processUser" method
    let prepare_result = test.prepare_call_hierarchy(
        "com/example/Service.java",
        Position::new(7, 27), // On "processUser"
    );

    match prepare_result {
        Ok(Some(items)) => {
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item.name.as_str(), "processUser");

            // Now get outgoing calls - this should find user.getName()
            let outgoing_result = test.outgoing_calls(item.clone());
            match outgoing_result {
                Ok(Some(calls)) => {
                    println!("Found {} outgoing calls", calls.len());
                    for call in &calls {
                        println!(
                            "  Calls: {} (detail: {:?}) with {} call sites",
                            call.to.name,
                            call.to.detail,
                            call.from_ranges.len()
                        );
                    }
                    // The key assertion: we should find getName
                    // (Previously this would fail because receiver type was "user" not "com.example.model.User")
                    let found_get_name = calls.iter().any(|c| c.to.name == "getName");
                    assert!(found_get_name, "Should find outgoing call to getName()");
                    assert_eq!(calls.len(), 1);
                    assert_eq!(calls[0].from_ranges.len(), 1);
                }
                Ok(None) => panic!("Expected outgoing call to getName"),
                Err(e) => {
                    panic!("outgoing_calls failed: {}", e);
                }
            }
        }
        Ok(None) => {
            panic!("Expected call hierarchy item");
        }
        Err(e) => {
            panic!("prepare_call_hierarchy failed: {}", e);
        }
    }
}

/// Test that constructor calls (new ClassName()) are tracked in outgoing calls
#[test]
fn test_outgoing_calls_constructor() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/model/User.java",
        r"
        package com.example.model;

        public class User {
            private String name;

            public User(String name) {
                this.name = name;
            }
        }
    ",
    );

    test.add_file(
        "com/example/Factory.java",
        r"
        package com.example;

        import com.example.model.User;

        public class Factory {
            public User createUser(String name) {
                return new User(name);
            }
        }
    ",
    );

    // Prepare call hierarchy on "createUser" method
    let prepare_result = test.prepare_call_hierarchy(
        "com/example/Factory.java",
        Position::new(7, 24), // On "createUser"
    );

    match prepare_result {
        Ok(Some(items)) => {
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item.name.as_str(), "createUser");

            // Now get outgoing calls - this should find new User()
            let outgoing_result = test.outgoing_calls(item.clone());
            match outgoing_result {
                Ok(Some(calls)) => {
                    println!("Found {} outgoing calls for constructor test", calls.len());
                    for call in &calls {
                        println!(
                            "  Calls: {} (detail: {:?}) with {} call sites",
                            call.to.name,
                            call.to.detail,
                            call.from_ranges.len()
                        );
                    }
                    // Should find the constructor call (might be named "User" or stored as constructor)
                    let found_ctor = calls.iter().any(|c| c.to.name == "User");
                    assert!(found_ctor, "Expected outgoing constructor call to User");
                    assert_eq!(calls.len(), 1);
                    assert_eq!(calls[0].from_ranges.len(), 1);
                }
                Ok(None) => panic!("Expected outgoing constructor call to User"),
                Err(e) => {
                    panic!("outgoing_calls failed: {}", e);
                }
            }
        }
        Ok(None) => {
            panic!("Expected call hierarchy item");
        }
        Err(e) => {
            panic!("prepare_call_hierarchy failed: {}", e);
        }
    }
}

#[test]
fn outgoing_call_uses_receiver_identity_when_names_collide() {
    let mut test = LspTest::new();
    test.add_file(
        "left/Worker.java",
        "package left; public class Worker { public void run() {} }",
    );
    test.add_file(
        "right/Worker.java",
        "package right; public class Worker { public void run() {} }",
    );
    let source = concat!(
        "package app; import right.Worker; ",
        "class Service { void execute(Worker worker) { worker.run(); } }",
    );
    test.add_file("app/Service.java", source);

    let item = test
        .prepare_call_hierarchy("app/Service.java", position_of(source, "execute("))
        .unwrap()
        .unwrap()
        .remove(0);
    let calls = test.outgoing_calls(item).unwrap().unwrap();

    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].to.uri.path().ends_with("/right/Worker.java"),
        "receiver must select right.Worker, got {}",
        calls[0].to.uri
    );
}

#[test]
fn unresolved_receiver_does_not_bind_a_globally_unique_method_name() {
    let mut test = LspTest::new();
    test.add_file(
        "unrelated/Worker.java",
        "package unrelated; public class Worker { public void run() {} }",
    );
    let source = "package app; class Service { void execute() { missing.run(); } }";
    test.add_file("app/Service.java", source);

    let item = test
        .prepare_call_hierarchy("app/Service.java", position_of(source, "execute("))
        .unwrap()
        .unwrap()
        .remove(0);
    let calls = test.outgoing_calls(item).unwrap();

    assert!(
        calls.as_ref().is_none_or(Vec::is_empty),
        "an unresolved receiver must fail closed"
    );
}

#[test]
fn outgoing_call_preserves_overload_identity() {
    let mut test = LspTest::new();
    let target = concat!(
        "package model; public class Target {\n",
        "  public void pick(int value) {}\n",
        "  public void pick(String value) {}\n",
        "}",
    );
    test.add_file("model/Target.java", target);
    let source = concat!(
        "package app; import model.Target; ",
        "class Service { void execute(Target target) { target.pick(\"chosen\"); } }",
    );
    test.add_file("app/Service.java", source);

    let item = test
        .prepare_call_hierarchy("app/Service.java", position_of(source, "execute("))
        .unwrap()
        .unwrap()
        .remove(0);
    let calls = test.outgoing_calls(item).unwrap().unwrap();

    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].to.selection_range.start.line, 2,
        "String overload declaration should be selected"
    );
}

#[test]
fn package_qualified_constructor_uses_the_final_type_identifier() {
    let mut test = LspTest::new();
    test.add_file(
        "left/Widget.java",
        "package left; public class Widget { public Widget() {} }",
    );
    test.add_file(
        "right/Widget.java",
        "package right; public class Widget { public Widget() {} }",
    );
    let source = concat!(
        "package app; class Factory { ",
        "right.Widget create() { return new right.Widget(); } }",
    );
    test.add_file("app/Factory.java", source);

    let item = test
        .prepare_call_hierarchy("app/Factory.java", position_of(source, "create("))
        .unwrap()
        .unwrap()
        .remove(0);
    let calls = test.outgoing_calls(item).unwrap().unwrap();

    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].to.uri.path().ends_with("/right/Widget.java"),
        "qualified constructor must select right.Widget, got {}",
        calls[0].to.uri
    );
}

fn callers_of(test: &mut LspTest, file: &str, position: Position) -> Vec<String> {
    let items = test
        .prepare_call_hierarchy(file, position)
        .expect("prepare call hierarchy")
        .expect("call hierarchy item");
    let calls = test
        .incoming_calls(items[0].clone())
        .expect("incoming calls")
        .unwrap_or_default();
    let mut names = calls
        .iter()
        .map(|call| call.from.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names
}

const SHAPES: &str = "package demo;\n\npublic interface Shape {\n    double area();\n}\n";
const CIRCLE: &str = "package demo;\n\npublic class Circle implements Shape {\n    @Override\n    public double area() {\n        return 3.0;\n    }\n}\n";
const APP: &str = "package demo;\n\npublic class App {\n    static double viaClass(Circle c) {\n        return c.area();\n    }\n\n    static double viaInterface(Shape s) {\n        return s.area();\n    }\n\n    static double local() {\n        Circle c = new Circle();\n        return c.area();\n    }\n}\n";

#[test]
fn incoming_calls_on_an_instance_method_find_callers_through_typed_receivers() {
    let mut test = LspTest::new();
    test.add_file("demo/Shape.java", SHAPES)
        .add_file("demo/Circle.java", CIRCLE)
        .add_file("demo/App.java", APP);
    let position = position_of(CIRCLE, "area()");
    let callers = callers_of(
        &mut test,
        "demo/Circle.java",
        Position::new(position.line, position.character + 1),
    );
    // Exact callers of Circle.area, plus the caller through the interface it
    // implements (dispatch can reach this override).
    assert!(callers.contains(&"viaClass".to_string()), "{callers:?}");
    assert!(callers.contains(&"local".to_string()), "{callers:?}");
    assert!(
        callers.contains(&"viaInterface".to_string()),
        "interface dispatch: {callers:?}"
    );
}

#[test]
fn incoming_calls_on_an_interface_method_find_its_callers() {
    let mut test = LspTest::new();
    test.add_file("demo/Shape.java", SHAPES)
        .add_file("demo/Circle.java", CIRCLE)
        .add_file("demo/App.java", APP);
    let position = position_of(SHAPES, "area()");
    let callers = callers_of(
        &mut test,
        "demo/Shape.java",
        Position::new(position.line, position.character + 1),
    );
    assert!(callers.contains(&"viaInterface".to_string()), "{callers:?}");
}

#[test]
fn incoming_calls_include_calls_nested_in_argument_expressions() {
    let mut test = LspTest::new();
    let app = "package demo;\n\npublic class App {\n    public static void main(String[] args) {\n        Circle c = new Circle();\n        Shape s = c;\n        System.out.println(c.area() + s.area());\n    }\n}\n";
    test.add_file("demo/Shape.java", SHAPES)
        .add_file("demo/Circle.java", CIRCLE)
        .add_file("demo/App.java", app);
    let position = position_of(CIRCLE, "area()");
    let items = test
        .prepare_call_hierarchy(
            "demo/Circle.java",
            Position::new(position.line, position.character + 1),
        )
        .unwrap()
        .unwrap();
    let calls = test
        .incoming_calls(items[0].clone())
        .unwrap()
        .unwrap_or_default();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].from.name, "main");
    assert_eq!(
        calls[0].from_ranges.len(),
        2,
        "both `c.area()` and `s.area()` are call sites"
    );
}
