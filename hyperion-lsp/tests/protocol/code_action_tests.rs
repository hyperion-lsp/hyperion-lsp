// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionOrCommand, Diagnostic, DiagnosticSeverity, Position, Range,
};

fn expect_actions(
    result: Result<Option<Vec<CodeActionOrCommand>>, String>,
    context: &str,
) -> Vec<CodeActionOrCommand> {
    result
        .unwrap_or_else(|error| panic!("{context}: code-action request failed: {error}"))
        .unwrap_or_else(|| panic!("{context}: expected code actions, got none"))
}

fn action_titles(actions: &[CodeActionOrCommand]) -> Vec<&str> {
    actions
        .iter()
        .map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => action.title.as_str(),
            CodeActionOrCommand::Command(command) => command.title.as_str(),
        })
        .collect()
}

fn expect_action<'a>(
    actions: &'a [CodeActionOrCommand],
    description: &str,
    title_matches: impl Fn(&str) -> bool,
) -> &'a CodeAction {
    actions
        .iter()
        .find_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) if title_matches(&action.title) => Some(action),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected {description} code action; available actions: {:?}",
                action_titles(actions)
            )
        })
}

fn action_edit_text(action: &CodeAction) -> String {
    let changes = action
        .edit
        .as_ref()
        .unwrap_or_else(|| panic!("code action {:?} has no workspace edit", action.title))
        .changes
        .as_ref()
        .unwrap_or_else(|| panic!("code action {:?} has no document changes", action.title));

    assert!(
        !changes.is_empty(),
        "code action {:?} has an empty workspace edit",
        action.title
    );

    changes
        .values()
        .flatten()
        .map(|edit| edit.new_text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn expression_selection_offers_extract_variable_and_method() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Calculator.java",
        r"
        package com.example;

        public class Calculator {
            public int calculate() {
                return 10 + 20 * 30;
            }
        }
    ",
    );

    // Select "10 + 20 * 30"
    let actions = test
        .code_action(
            "com/example/Calculator.java",
            Range::new(Position::new(5, 23), Position::new(5, 35)),
            vec![],
        )
        .expect("code-action request should succeed")
        .expect("an expression selection should produce code actions");
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action.title.as_str()),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect();

    assert!(
        titles.iter().any(|title| {
            let title = title.to_lowercase();
            title.contains("extract") && title.contains("variable")
        }),
        "expression selection should offer extract-variable, got {titles:?}"
    );
    assert!(
        titles.iter().any(|title| {
            let title = title.to_lowercase();
            title.contains("extract") && title.contains("method")
        }),
        "expression selection should offer extract-method, got {titles:?}"
    );
}

#[test]
fn file_scope_offers_organize_imports() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r"
        package com.example;

        import java.util.List;
        import java.util.ArrayList;
        import java.io.File;

        public class Test {
            public void test() {
                List<String> list = new ArrayList<>();
            }
        }
    ",
    );

    let actions = test
        .code_action(
            "com/example/Test.java",
            Range::new(Position::new(0, 0), Position::new(0, 0)),
            vec![],
        )
        .expect("code-action request should succeed")
        .expect("file scope should produce code actions");
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action.title.as_str()),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect();

    assert!(
        titles.iter().any(|title| {
            let title = title.to_lowercase();
            title.contains("organize") && title.contains("import")
        }),
        "file-scope actions should include organize-imports, got {titles:?}"
    );
}

#[test]
fn unused_import_diagnostic_targets_its_own_declaration() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Test.java",
        "package com.example;\nimport java.util.List;\nimport java.util.Map;\nclass Test {}\n",
    );
    let diagnostic = Diagnostic {
        range: Range::new(Position::new(1, 17), Position::new(1, 21)),
        severity: Some(DiagnosticSeverity::WARNING),
        message: "Unused import".into(),
        ..Default::default()
    };

    let actions = expect_actions(
        test.code_action("com/example/Test.java", diagnostic.range, vec![diagnostic]),
        "removing the diagnosed import",
    );
    let action = expect_action(&actions, "remove unused import", |title| {
        title == "Remove unused import"
    });
    let edits = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
    let edit = &edits.values().next().unwrap()[0];
    assert_eq!(edit.range.start.line, 1);
    assert_eq!(edit.range.end.line, 1);
    assert_eq!(edit.new_text, "");
}

#[test]
fn javac_multiline_class_diagnostic_offers_import_edit() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r"
package com.example;

public class Test {
    private ArrayList<String> values;
}
",
    );

    let diagnostic = Diagnostic {
        range: Range::new(Position::new(4, 12), Position::new(4, 21)),
        severity: Some(DiagnosticSeverity::ERROR),
        message: "cannot find symbol\n  symbol:   class ArrayList\n  location: class Test".into(),
        ..Default::default()
    };
    let actions = expect_actions(
        test.code_action("com/example/Test.java", diagnostic.range, vec![diagnostic]),
        "importing a class from a multiline javac diagnostic",
    );
    let action = expect_action(&actions, "import java.util.ArrayList", |title| {
        title == "Import 'java.util.ArrayList'"
    });

    assert!(
        action_edit_text(action).contains("import java.util.ArrayList;"),
        "the import action should insert the resolved javac class"
    );
}

#[test]
fn test_generate_constructor_action() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/User.java",
        r"
        package com.example;

        public class User {
            private String name;
            private int age;
            private String email;
        }
    ",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/User.java",
            Range::new(Position::new(4, 0), Position::new(4, 0)),
            vec![],
        ),
        "constructor generation inside a class",
    );

    expect_action(&actions, "generate constructor", |title| {
        title.to_lowercase().contains("constructor")
    });
}

#[test]
fn test_generate_getters_setters_action() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Person.java",
        r"
        package com.example;

        public class Person {
            private String firstName;
            private String lastName;
            private int age;
        }
    ",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/Person.java",
            Range::new(Position::new(5, 0), Position::new(5, 0)),
            vec![],
        ),
        "getter/setter generation inside a class",
    );

    expect_action(&actions, "generate getters and setters", |title| {
        let title = title.to_lowercase();
        title.contains("getter") && title.contains("setter")
    });
}

#[test]
fn test_code_generation_actions_offered_in_class() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Product.java",
        r"
        package com.example;

        public class Product {
            private String id;
            private String name;
            private double price;
        }
    ",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/Product.java",
            Range::new(Position::new(4, 0), Position::new(4, 0)),
            vec![],
        ),
        "code generation inside a class",
    );

    expect_action(&actions, "generate constructor", |title| {
        title.to_lowercase().contains("constructor")
    });
    expect_action(&actions, "generate getters and setters", |title| {
        let title = title.to_lowercase();
        title.contains("getter") && title.contains("setter")
    });
}

#[test]
fn test_extract_variable_generates_edits() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Extract.java",
        r#"
        package com.example;

        public class Extract {
            public void test() {
                System.out.println(new StringBuilder().append("hello").toString());
            }
        }
    "#,
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/Extract.java",
            Range::new(Position::new(5, 35), Position::new(5, 79)),
            vec![],
        ),
        "extracting a variable from a selected expression",
    );
    let action = expect_action(&actions, "extract variable", |title| {
        let title = title.to_lowercase();
        title.contains("extract") && title.contains("variable")
    });

    assert!(
        !action_edit_text(action).is_empty(),
        "extract-variable edit should insert text"
    );
}

#[test]
fn test_extract_variable_infers_string_type() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/TypeInference.java",
        r#"
        package com.example;

        public class TypeInference {
            public void test() {
                System.out.println("Hello World");
            }
        }
    "#,
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/TypeInference.java",
            Range::new(Position::new(5, 35), Position::new(5, 48)),
            vec![],
        ),
        "extracting a string literal",
    );
    let action = expect_action(&actions, "extract variable", |title| {
        let title = title.to_lowercase();
        title.contains("extract") && title.contains("variable")
    });

    assert!(
        action_edit_text(action).contains("String"),
        "extracting a string literal should declare a String variable"
    );
}

#[test]
fn test_extract_method_generates_edits() {
    let mut test = LspTest::new();

    // Use minimal indentation to get predictable positions
    test.add_file(
        "com/example/ExtractMethod.java",
        "package com.example;

public class ExtractMethod {
    public void processData() {
        int x = 10;
        int y = 20;
        int sum = x + y;
        System.out.println(sum);
    }
}",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/ExtractMethod.java",
            Range::new(Position::new(4, 8), Position::new(6, 24)),
            vec![],
        ),
        "extracting a selected statement block",
    );
    let action = expect_action(&actions, "extract method", |title| {
        let title = title.to_lowercase();
        title.contains("extract") && title.contains("method")
    });
    let edit_text = action_edit_text(action);

    assert!(
        edit_text.contains("private void") && edit_text.contains("extractedMethod"),
        "extract-method edit should create the extracted method; got {edit_text:?}"
    );
}

#[test]
fn test_extract_variable_suggests_meaningful_name() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Naming.java",
        r"
        package com.example;

        public class Naming {
            public void test() {
                User user = new User();
                String name = user.getName();
            }
        }
    ",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/Naming.java",
            Range::new(Position::new(6, 30), Position::new(6, 44)),
            vec![],
        ),
        "extracting the result of getName()",
    );
    let action = expect_action(&actions, "extract variable", |title| {
        let title = title.to_lowercase();
        title.contains("extract") && title.contains("variable")
    });

    assert!(
        action.title.to_lowercase().contains("name"),
        "extract-variable action should suggest a name derived from getName(); got {:?}",
        action.title
    );
}

// ===== Generate equals/hashCode/toString Tests =====

#[test]
fn test_generate_equals_hashcode_action() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Person.java",
        r"
package com.example;

public class Person {
    private String name;
    private int age;
}
",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/Person.java",
            Range::new(Position::new(4, 10), Position::new(4, 10)),
            vec![],
        ),
        "generating equals and hashCode",
    );
    let action = expect_action(&actions, "generate equals and hashCode", |title| {
        let title = title.to_lowercase();
        title.contains("equals") && title.contains("hashcode")
    });
    let edit_text = action_edit_text(action);

    for expected in [
        "@Override",
        "boolean equals(Object",
        "int hashCode()",
        "Objects.equals",
        "Objects.hash",
    ] {
        assert!(
            edit_text.contains(expected),
            "equals/hashCode edit should contain {expected:?}; got {edit_text:?}"
        );
    }
}

#[test]
fn test_generate_tostring_action() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/User.java",
        r"
package com.example;

public class User {
    private String username;
    private String email;
    private int id;
}
",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/User.java",
            Range::new(Position::new(4, 10), Position::new(4, 10)),
            vec![],
        ),
        "generating toString",
    );
    let action = expect_action(&actions, "generate toString", |title| {
        title.to_lowercase().contains("tostring")
    });
    let edit_text = action_edit_text(action);

    for expected in ["@Override", "String toString()", "username", "email", "id"] {
        assert!(
            edit_text.contains(expected),
            "toString edit should contain {expected:?}; got {edit_text:?}"
        );
    }
}

#[test]
fn test_generate_equals_with_primitives() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Point.java",
        r"
package com.example;

public class Point {
    private int x;
    private int y;
}
",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/Point.java",
            Range::new(Position::new(4, 10), Position::new(4, 10)),
            vec![],
        ),
        "generating equals for primitive fields",
    );
    let action = expect_action(&actions, "generate equals and hashCode", |title| {
        title.to_lowercase().contains("equals")
    });
    let edit_text = action_edit_text(action);

    assert!(
        edit_text.contains("this.x == other.x"),
        "primitive equality should use ==; got {edit_text:?}"
    );
    assert!(
        !edit_text.contains("Objects.equals(this.x"),
        "primitive equality must not use Objects.equals; got {edit_text:?}"
    );
}

// ===== Introduce Constant Tests =====

#[test]
fn test_introduce_constant_action() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Constants.java",
        r"
package com.example;

public class Constants {
    public void test() {
        int result = 3.14159 * radius * radius;
    }
}
",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/Constants.java",
            Range::new(Position::new(5, 21), Position::new(5, 28)),
            vec![],
        ),
        "introducing a constant from a numeric literal",
    );

    expect_action(&actions, "introduce constant", |title| {
        title.to_lowercase().contains("constant")
    });
}

// ===== Surround With Tests =====

#[test]
fn test_surround_with_actions() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Surround.java",
        r"
package com.example;

public class Surround {
    public void test() {
        doSomething();
    }

    private void doSomething() {}
}
",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/Surround.java",
            Range::new(Position::new(5, 8), Position::new(5, 22)),
            vec![],
        ),
        "surrounding a selected statement",
    );
    let titles = action_titles(&actions);

    for construct in ["if", "try-catch", "while"] {
        assert!(
            titles.iter().any(|title| {
                let title = title.to_lowercase();
                title.contains("surround") && title.contains(construct)
            }),
            "expected surround-with-{construct} action; got {titles:?}"
        );
    }
}

#[test]
fn test_surround_with_try_catch_generates_edits() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/TryCatch.java",
        r"
package com.example;

public class TryCatch {
    public void test() {
        riskyOperation();
    }

    private void riskyOperation() {}
}
",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/TryCatch.java",
            Range::new(Position::new(5, 8), Position::new(5, 25)),
            vec![],
        ),
        "surrounding a statement with try/catch",
    );
    let action = expect_action(&actions, "surround with try-catch", |title| {
        title.to_lowercase().contains("try-catch")
    });
    let edit_text = action_edit_text(action);

    assert!(
        edit_text.contains("try") && edit_text.contains("catch"),
        "try-catch action should generate both clauses; got {edit_text:?}"
    );
}

// ===== Introduce Field Tests =====

#[test]
fn test_introduce_field_action() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/FieldTest.java",
        r"
package com.example;

public class FieldTest {
    public void test() {
        Object obj = new ArrayList<String>();
    }
}
",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/FieldTest.java",
            Range::new(Position::new(5, 21), Position::new(5, 44)),
            vec![],
        ),
        "introducing a field from an object creation expression",
    );

    expect_action(&actions, "introduce field", |title| {
        title.to_lowercase().contains("field")
    });
}

// ===== Create Field/Method Quick Fix Tests =====

#[test]
fn test_create_method_quick_fix_for_undefined_call() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r"
package com.example;

public class Test {
    void foo() {
        unknownMethod();
    }
}
",
    );

    // Create a diagnostic for the undefined method
    let diagnostic = Diagnostic {
        range: Range::new(Position::new(5, 8), Position::new(5, 21)),
        severity: Some(DiagnosticSeverity::ERROR),
        message: "cannot find symbol: method unknownMethod()".into(),
        ..Default::default()
    };

    let actions = expect_actions(
        test.code_action(
            "com/example/Test.java",
            Range::new(Position::new(5, 8), Position::new(5, 21)),
            vec![diagnostic],
        ),
        "quick-fixing an undefined zero-argument method call",
    );

    let action = expect_action(&actions, "create unknownMethod", |title| {
        let title = title.to_lowercase();
        title.contains("create") && title.contains("unknownmethod")
    });
    let edit_text = action_edit_text(action);

    assert!(
        edit_text.contains("private void unknownMethod()"),
        "create-method edit should preserve the unresolved method name; got {edit_text:?}"
    );
}

#[test]
fn test_create_method_with_parameters_uses_diagnostic_types() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r#"
package com.example;

public class Test {
    void foo() {
        unknownMethod("hello", 42);
    }
}
"#,
    );

    let diagnostic = Diagnostic {
        range: Range::new(Position::new(5, 8), Position::new(5, 32)),
        severity: Some(DiagnosticSeverity::ERROR),
        message: "cannot find symbol: method unknownMethod(String, int)".into(),
        ..Default::default()
    };

    let actions = expect_actions(
        test.code_action(
            "com/example/Test.java",
            Range::new(Position::new(5, 8), Position::new(5, 32)),
            vec![diagnostic],
        ),
        "quick-fixing an undefined method call with arguments",
    );
    let action = expect_action(&actions, "create unknownMethod", |title| {
        let title = title.to_lowercase();
        title.contains("create") && title.contains("unknownmethod")
    });
    let edit_text = action_edit_text(action);

    assert!(
        edit_text.contains("private void unknownMethod(String arg0, int arg1)"),
        "create-method edit should preserve the method name and diagnostic parameter types; got {edit_text:?}"
    );
}

#[test]
fn test_create_method_preserves_generic_diagnostic_parameter_boundaries() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r#"
package com.example;

public class Test {
    void foo() {
        transform(null, null);
    }
}
"#,
    );

    let diagnostic = Diagnostic {
        range: Range::new(Position::new(5, 8), Position::new(5, 29)),
        severity: Some(DiagnosticSeverity::ERROR),
        message: "cannot find symbol\n  symbol: method transform(java.util.Map<String, Integer>, String[])\n  location: class Test".into(),
        ..Default::default()
    };
    let actions = expect_actions(
        test.code_action("com/example/Test.java", diagnostic.range, vec![diagnostic]),
        "creating a method from a generic javac signature",
    );
    let action = expect_action(&actions, "create transform", |title| {
        title == "Create method 'transform'"
    });
    let edit_text = action_edit_text(action);

    assert!(
        edit_text
            .contains("private void transform(java.util.Map<String, Integer> arg0, String[] arg1)"),
        "generic commas must not split diagnostic parameter types; got {edit_text:?}"
    );
}

// ===== Introduce Parameter Tests =====

#[test]
fn test_introduce_parameter_action() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/ParamTest.java",
        r"
package com.example;

public class ParamTest {
    public void calculate() {
        int result = getValue() * 2;
    }

    private int getValue() { return 42; }
}
",
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/ParamTest.java",
            Range::new(Position::new(5, 21), Position::new(5, 31)),
            vec![],
        ),
        "introducing a parameter from a selected call",
    );

    expect_action(&actions, "introduce parameter", |title| {
        title.to_lowercase().contains("parameter")
    });
}

#[test]
fn test_extract_interface_action() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/UserService.java",
        r#"
        package com.example;

        public class UserService {
            public String getName() {
                return "John";
            }

            public void setName(String name) {
                // setter
            }

            private void internalMethod() {
                // not included
            }
        }
    "#,
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/UserService.java",
            Range::new(Position::new(3, 21), Position::new(3, 21)),
            vec![],
        ),
        "extracting an interface from public methods",
    );
    let action = expect_action(&actions, "extract interface", |title| {
        let title = title.to_lowercase();
        title.contains("extract") && title.contains("interface")
    });

    assert!(
        !action_edit_text(action).is_empty(),
        "extract-interface action should produce a workspace edit"
    );
}

#[test]
fn test_extract_interface_no_public_methods() {
    let mut test = LspTest::new();

    // Class with no public methods should NOT offer extract interface
    test.add_file(
        "com/example/InternalService.java",
        r"
        package com.example;

        public class InternalService {
            private void process() {
                // private only
            }
        }
    ",
    );

    let actions = test
        .code_action(
            "com/example/InternalService.java",
            Range::new(Position::new(3, 21), Position::new(3, 21)),
            vec![],
        )
        .expect("code-action request should succeed")
        .unwrap_or_default();
    let titles = action_titles(&actions);

    assert!(
        !titles.iter().any(|title| {
            let title = title.to_lowercase();
            title.contains("extract") && title.contains("interface")
        }),
        "a class with no public methods must not offer extract-interface; got {titles:?}"
    );
}

#[test]
fn test_move_static_method_action() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/StringHelper.java",
        r#"
        package com.example;

        public class StringHelper {
            public static String format<|>Name(String first, String last) {
                return first + " " + last;
            }

            public void instanceMethod() {
                // not static
            }
        }
    "#,
    );

    let actions = expect_actions(
        test.code_action(
            "com/example/StringHelper.java",
            Range::new(Position::new(4, 36), Position::new(4, 36)),
            vec![],
        ),
        "moving a static method",
    );

    expect_action(&actions, "move static method", |title| {
        title.to_lowercase().contains("move")
    });
}
