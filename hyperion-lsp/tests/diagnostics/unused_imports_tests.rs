// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Unused Import Detection
//!
//! Validates that the LSP correctly identifies unused imports while avoiding
//! false positives for imports that ARE actually used.

use hyperion_lsp::inspections::run_inspections;
use hyperion_parser::{lexer::Lexer, parser::Parser};
use hyperion_workspace::WorkspaceIndex;
use tempfile::TempDir;

/// Helper to check if a diagnostic about unused imports exists for a given import name
fn has_unused_import_diagnostic(source: &str, import_name: &str) -> bool {
    let tokens = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = hyperion_parser::syntax::SyntaxNode::new_root(green_node);

    let temp_dir = TempDir::new().expect("temp dir");
    let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
    let source_path = temp_dir.path().join("Test.java");
    index
        .index_file_with_source(&source_path, source)
        .expect("index source");
    let file_id = index.get_file_id(&source_path).expect("file id");

    let diagnostics = run_inspections(&root, source, &index, file_id);

    diagnostics
        .iter()
        .any(|d| d.message.contains("Unused import") && d.message.contains(import_name))
}

#[test]
fn test_import_used_in_new_expression() {
    let source = r"
import com.example.User;

public class Test {
    public void test() {
        User user = new User();
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "User"),
        "Import IS used in new expression - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_cast_expression() {
    let source = r"
import com.example.User;

public class Test {
    public void test(Object obj) {
        User user = (User) obj;
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "User"),
        "Import IS used in cast expression - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_instanceof() {
    let source = r#"
import com.example.User;

public class Test {
    public void test(Object obj) {
        if (obj instanceof User) {
            System.out.println("It's a user!");
        }
    }
}
    "#;

    assert!(
        !has_unused_import_diagnostic(source, "User"),
        "Import IS used in instanceof check - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_generic_type_argument() {
    let source = r"
import com.example.User;

public class Test {
    public void test() {
        List<User> users = new ArrayList<>();
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "User"),
        "Import IS used in generic type argument - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_array_type() {
    let source = r"
import com.example.User;

public class Test {
    public void test() {
        User[] users = new User[10];
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "User"),
        "Import IS used in array type - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_method_return_type() {
    let source = r"
import com.example.User;

public class Test {
    public User getUser() {
        return null;
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "User"),
        "Import IS used in method return type - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_method_parameter() {
    let source = r"
import com.example.User;

public class Test {
    public void process(User user) {
        System.out.println(user);
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "User"),
        "Import IS used in method parameter - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_nested_type() {
    let source = r"
import com.example.Outer;

public class Test {
    public void process(Outer.Inner value) {
        System.out.println(value);
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "Outer"),
        "Import IS used as nested type qualifier - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_annotation_constant() {
    let source = r#"
import com.example.Api;

@Path(Api.V1 + "foo")
public class Test {}
    "#;

    assert!(
        !has_unused_import_diagnostic(source, "Api"),
        "Import IS used in annotation argument - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_field_type() {
    let source = r"
import com.example.User;

public class Test {
    private User user;
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "User"),
        "Import IS used in field type - should NOT be marked as unused"
    );
}

#[test]
fn test_static_import_used_in_method_call() {
    let source = r#"
import static java.util.Arrays.asList;

public class Test {
    public void test() {
        List<String> list = asList("a", "b", "c");
    }
}
    "#;

    assert!(
        !has_unused_import_diagnostic(source, "asList"),
        "Static import IS used - should NOT be marked as unused"
    );
}

#[test]
fn test_static_import_used_as_static_field() {
    let source = r"
import static java.lang.Math.PI;

public class Test {
    public double getCircumference(double radius) {
        return 2 * PI * radius;
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "PI"),
        "Static import IS used - should NOT be marked as unused"
    );
}

#[test]
fn test_import_actually_unused() {
    let source = r"
import com.example.User;
import com.example.Product;

public class Test {
    public void test() {
        User user = new User();
    }
}
    ";

    assert!(
        has_unused_import_diagnostic(source, "Product"),
        "Product import is NOT used - should be marked as unused"
    );
    assert!(
        !has_unused_import_diagnostic(source, "User"),
        "User import IS used - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_annotation_parameter() {
    let source = r"
import org.springframework.web.bind.annotation.RequestBody;

public class Test {
    public void process(@RequestBody String body) {
        System.out.println(body);
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "RequestBody"),
        "Import IS used as annotation on parameter - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_class_annotation() {
    let source = r#"
import org.springframework.stereotype.Service;

@Service
public class Test {
    public void process() {
        System.out.println("Service");
    }
}
    "#;

    assert!(
        !has_unused_import_diagnostic(source, "Service"),
        "Import IS used as class annotation - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_method_annotation() {
    let source = r"
import org.junit.Test;

public class MyTest {
    @Test
    public void testSomething() {
        // test code
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "Test"),
        "Import IS used as method annotation - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_annotation_argument_qualifier() {
    let source = r#"
import javax.ws.rs.Path;
import com.foo.Api;

@Path(Api.V1 + "foo")
public class Test {
}
    "#;

    assert!(
        !has_unused_import_diagnostic(source, "Api"),
        "Import IS used as annotation argument qualifier - should NOT be marked as unused"
    );
}

#[test]
fn test_wildcard_imports_not_checked() {
    let source = r"
import java.util.*;

public class Test {
    public void test() {
        List<String> list = new ArrayList<>();
    }
}
    ";

    // Wildcard imports should be skipped
    assert!(
        !has_unused_import_diagnostic(source, "*"),
        "Wildcard imports should not be checked"
    );
}

#[test]
fn test_import_used_in_qualified_generic_type() {
    let source = r"
import com.example.User;

public class Test {
    public java.util.List<User> getUsers() {
        return null;
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "User"),
        "Import IS used in generic type argument - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_bounded_type_parameter() {
    let source = r"
import com.example.User;

public class Test<T extends User> {
    private T value;
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "User"),
        "Import IS used in type parameter bound - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_throws_clause() {
    let source = r"
import java.io.IOException;

public class Test {
    public void test() throws IOException {
        // may throw IOException
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "IOException"),
        "Import IS used in throws clause - should NOT be marked as unused"
    );
}

#[test]
fn test_import_used_in_qualified_nested_type() {
    let source = r"
import com.foo.Foo;

public class Test {
    public void myMethod(Foo.Bar bar) {
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "Foo"),
        "Import IS used as qualifying type for nested class - should NOT be marked as unused"
    );
}

// ===== Explicit static import unused detection =====

#[test]
fn test_explicit_static_import_method_lowercase() {
    let source = r"
import static com.example.Utils.helper;

public class Test {
    public void test() {
        helper();
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "helper"),
        "Static import of method IS used in method call - should NOT be marked as unused"
    );
}

#[test]
fn test_explicit_static_import_method_in_expression() {
    let source = r"
import static com.example.Utils.compute;

public class Test {
    public void test() {
        int result = compute(1, 2) + 3;
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "compute"),
        "Static import of method IS used in expression - should NOT be marked as unused"
    );
}

#[test]
fn test_explicit_static_import_method_as_argument() {
    let source = r"
import static com.example.Utils.generate;

public class Test {
    public void test() {
        System.out.println(generate());
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "generate"),
        "Static import of method IS used as argument - should NOT be marked as unused"
    );
}

#[test]
fn test_explicit_static_import_field_lowercase() {
    let source = r"
import static com.example.Constants.defaultValue;

public class Test {
    public void test() {
        int x = defaultValue;
    }
}
    ";

    assert!(
        !has_unused_import_diagnostic(source, "defaultValue"),
        "Static import of lowercase field IS used - should NOT be marked as unused"
    );
}

#[test]
fn test_explicit_static_import_unused() {
    let source = r"
import static com.example.Utils.helper;

public class Test {
    public void test() {
        // helper is not used
    }
}
    ";

    assert!(
        has_unused_import_diagnostic(source, "helper"),
        "Static import of method is NOT used - should be marked as unused"
    );
}
