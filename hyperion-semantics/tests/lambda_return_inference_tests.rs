// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_parser::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::{
    Database, FileId, ParseDatabase, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle,
};
use hyperion_semantics::file_semantics::FileSemantics;
use hyperion_semantics::type_checker::TypeChecker;
use hyperion_semantics::{ClassType, Type};
use hyperion_workspace::{Language, WorkspaceIndex};

const DECLARATIONS: &str = r#"
package example;

interface ThrowingFunction<T, R, E extends Exception> {
    R apply(T value) throws E;
}
interface Supplier<R> { R get(); }
interface Operator<T> { T apply(T value); }
interface BoundedOperator<T extends Book> { T apply(T value); }
class Book {}
class BookCatalog {
    boolean hasIndex() { return false; }
    Book createBook() { return new Book(); }
}
class Client<T, E extends Exception> {
    <R> R execute(ThrowingFunction<T, R, E> function) { return null; }
    <V, R> R map(V input, ThrowingFunction<V, R, E> function) { return null; }
    <R> R withFallback(R fallback, ThrowingFunction<T, R, E> function) { return fallback; }
    void accept(ThrowingFunction<T, String, E> function) {}
    Object transform(Operator<? super Book> function) { return null; }
    Object upper(Operator<? extends Book> function) { return null; }
    Object bounded(BoundedOperator<? super Book> function) { return null; }
}
"#;

struct Analysis {
    root: SyntaxNode,
    semantics: Arc<FileSemantics>,
    workspace: Arc<WorkspaceIndex>,
    file: FileId,
}

impl Analysis {
    fn new(body: &str) -> Self {
        let source = format!(
            "{DECLARATIONS}\nclass Usage {{
                Book createBookEntry(BookCatalog service) {{ return new Book(); }}
                void test(Client<BookCatalog, Exception> client) {{ {body} }}
            }}"
        );
        let temp = tempfile::tempdir().expect("temporary workspace");
        let workspace = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        // A hierarchy-complete `String`: errors are only reported about types
        // whose hierarchy the index fully knows.
        for (path, stub) in [
            (
                "java/lang/Object.java",
                "package java.lang; public class Object {}",
            ),
            (
                "java/lang/String.java",
                "package java.lang; public final class String {}",
            ),
        ] {
            workspace
                .index_file_with_source(&temp.path().join(path), stub)
                .expect("stub should index");
        }
        workspace
            .index_file_with_source(&temp.path().join("example/Usage.java"), &source)
            .expect("fixture should index");

        let file = FileId::new(0);
        let mut db = Database::default();
        db.set_file_text(file, Arc::new(source));
        db.set_file_language(file, Language::Java);
        db.set_workspace_index_handle(WorkspaceIndexHandle(Arc::clone(&workspace)));
        db.set_workspace_version(0);
        Self {
            root: db.parse_file(file).syntax(),
            semantics: db.file_semantics(file),
            workspace,
            file,
        }
    }

    fn call_type(&self, prefix: &str) -> Type {
        let call = self
            .root
            .descendants()
            .find(|node| {
                node.kind() == SyntaxKind::METHOD_CALL_EXPR
                    && node.text().to_string().trim().starts_with(prefix)
            })
            .unwrap_or_else(|| panic!("missing call starting with {prefix:?}"));
        self.semantics
            .expression_type_at(call.text_range())
            .cloned()
            .unwrap_or(Type::Unknown)
    }

    fn diagnostic_messages(&self) -> Vec<String> {
        let mut checker = TypeChecker::new();
        checker.set_file_semantics(Arc::clone(&self.semantics));
        checker
            .check_with_context(&self.root, &[], self.file, Arc::clone(&self.workspace))
            .iter()
            .map(|error| error.message())
            .collect()
    }
}

fn book_type() -> Type {
    Type::Class(ClassType::new("Book".into(), Some("example".into())))
}

#[test]
fn book_assignment_uses_nonnull_block_return_and_sam_parameter() {
    let analysis = Analysis::new(
        "Book book;
         book = client.execute(service -> {
             if (service.hasIndex()) return null;
             return createBookEntry(service);
         });",
    );

    assert_eq!(analysis.call_type("client.execute("), book_type());
    assert!(analysis.diagnostic_messages().is_empty());
}

#[test]
fn var_initializer_infers_result_without_an_assignment_target() {
    let analysis = Analysis::new(
        "var book = client.execute(service -> {
             if (service.hasIndex()) return null;
             return createBookEntry(service);
         });",
    );

    assert_eq!(analysis.call_type("client.execute("), book_type());
}

#[test]
fn incompatible_assignment_still_reports_the_concrete_lambda_result() {
    let analysis =
        Analysis::new("String incompatible = client.execute(service -> createBookEntry(service));");

    assert_eq!(analysis.call_type("client.execute("), book_type());
    let messages = analysis.diagnostic_messages();
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("Book") && messages[0].contains("String"));
}

#[test]
fn ordinary_argument_supplies_generic_lambda_input_type() {
    let analysis = Analysis::new(
        "Book book = client.map(new BookCatalog(), service -> service.createBook());",
    );

    assert_eq!(analysis.call_type("client.map("), book_type());
}

#[test]
fn lambda_return_preserves_a_compatible_ordinary_argument_bound() {
    let analysis =
        Analysis::new("Object book = client.withFallback(new Object(), service -> new Book());");

    assert_eq!(
        analysis.call_type("client.withFallback("),
        Type::Class(ClassType::object())
    );
}

#[test]
fn block_return_uses_the_complete_binary_expression() {
    let analysis =
        Analysis::new("String value = client.execute(service -> { return 1 + \"value\"; });");

    assert_eq!(
        analysis.call_type("client.execute("),
        Type::Class(ClassType::string())
    );
}

#[test]
fn explicitly_typed_parameter_cannot_override_the_sam_input() {
    let analysis =
        Analysis::new("Object invalid = client.execute((String service) -> new Book());");

    assert_eq!(analysis.call_type("client.execute("), Type::Unknown);
}

#[test]
fn incompatible_sam_return_rejects_the_candidate() {
    let analysis = Analysis::new("client.accept(service -> new Book());");

    assert_eq!(analysis.call_type("client.accept("), Type::Unknown);
}

#[test]
fn nested_lambda_and_anonymous_class_returns_do_not_constrain_outer_result() {
    let analysis = Analysis::new(
        "Book book = client.execute(service -> {
             Supplier<String> nested = () -> { return \"nested\"; };
             Object anonymous = new Object() {
                 String value() { return \"anonymous\"; }
             };
             return service.createBook();
         });",
    );

    assert_eq!(analysis.call_type("client.execute("), book_type());
}

#[test]
fn primitive_lambda_result_is_boxed_for_generic_return() {
    let analysis = Analysis::new("var number = client.execute(service -> 42);");

    assert_eq!(
        analysis.call_type("client.execute("),
        Type::Class(ClassType::new("Integer".into(), Some("java.lang".into())))
    );
}

#[test]
fn explicit_wildcard_lambda_projects_both_input_and_output() {
    let analysis = Analysis::new("Object result = client.transform((Object book) -> book);");
    assert_eq!(
        analysis.call_type("client.transform("),
        Type::Class(ClassType::object())
    );
}

#[test]
fn wildcard_lambda_rejects_unrelated_explicit_input() {
    let analysis = Analysis::new("Object result = client.transform((BookCatalog book) -> book);");
    assert_eq!(analysis.call_type("client.transform("), Type::Unknown);
}

#[test]
fn implicit_wildcard_lambda_retains_ground_output_contract() {
    let analysis = Analysis::new("Object result = client.transform(book -> new BookCatalog());");
    assert_eq!(analysis.call_type("client.transform("), Type::Unknown);
}

#[test]
fn explicit_wildcard_inputs_obey_upper_and_declaration_bounds() {
    for method in ["upper", "bounded"] {
        let valid = Analysis::new(&format!("client.{method}((Book value) -> value);"));
        assert_eq!(
            valid.call_type(&format!("client.{method}(")),
            Type::Class(ClassType::object())
        );
        let invalid = Analysis::new(&format!("client.{method}((Object value) -> value);"));
        assert_eq!(
            invalid.call_type(&format!("client.{method}(")),
            Type::Unknown
        );
    }
}
