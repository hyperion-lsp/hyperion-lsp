// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! End-to-end contracts between the diagnostic engine and the security scanner.

use std::sync::Arc;

use hyperion_lsp::diagnostic_engine::{
    collect_and_publish_phased, collect_diagnostics_for_file, DiagnosticOptions,
};
use hyperion_lsp::security_diagnostics::SecurityDiagnosticsProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{FileId, Language, SourceDatabase};
use hyperion_workspace::WorkspaceIndex;
use parking_lot::Mutex;
use tower_lsp::lsp_types::{Diagnostic, NumberOrString};

const UNSAFE_SOURCE: &str = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.ResultSet;
import java.sql.Statement;

class UserController {
    ResultSet find(HttpServletRequest request, Connection connection) throws Exception {
        String id = request.getParameter("id");
        String query = "SELECT * FROM users WHERE id = " + id;
        Statement statement = connection.createStatement();
        return statement.executeQuery(query);
    }
}
"#;

const SAFE_SOURCE: &str = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.ResultSet;

class UserController {
    ResultSet find(HttpServletRequest request, Connection connection) throws Exception {
        String id = request.getParameter("id");
        PreparedStatement statement =
            connection.prepareStatement("SELECT * FROM users WHERE id = ?");
        statement.setString(1, id);
        return statement.executeQuery();
    }
}
"#;

struct Fixture {
    _temp: tempfile::TempDir,
    path: std::path::PathBuf,
    file_id: FileId,
    host: Arc<SemanticHost>,
    index: Arc<WorkspaceIndex>,
    security: Arc<SecurityDiagnosticsProvider>,
}

impl Fixture {
    fn new(source: &str) -> Self {
        let temp = tempfile::tempdir().expect("temp workspace");
        let path = temp.path().join("UserController.java");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&path, source)
            .expect("index Java fixture");
        let file_id = index.get_file_id(&path).expect("indexed file id");

        let host = SemanticHost::new(Arc::clone(&index));
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source.to_owned()));
            db.set_file_language(file_id, Language::Java);
        });

        Self {
            _temp: temp,
            path,
            file_id,
            host,
            index,
            security: Arc::new(SecurityDiagnosticsProvider::new()),
        }
    }

    fn replace_source(&self, source: &str) {
        self.index
            .index_file_with_source(&self.path, source)
            .expect("re-index edited Java fixture");
        self.host
            .with_db(|db| db.set_file_text(self.file_id, Arc::new(source.to_owned())));
    }

    async fn collect(&self, source: &str, options: DiagnosticOptions) -> Vec<Diagnostic> {
        collect_diagnostics_for_file(
            self.file_id,
            "java",
            source,
            &self.host,
            Some(&self.security),
            true,
            options,
        )
        .await
    }
}

fn has_code(diagnostics: &[Diagnostic], code: &str) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_ref() == Some(&NumberOrString::String(code.to_owned()))
    })
}

fn security_only_options() -> DiagnosticOptions {
    DiagnosticOptions {
        run_type_check: false,
        run_nullability: false,
        run_inspections: false,
        run_security: true,
        ..DiagnosticOptions::default()
    }
}

#[tokio::test]
async fn security_is_independent_of_semantic_checks_and_refreshes_after_an_edit() {
    let fixture = Fixture::new(UNSAFE_SOURCE);

    let unsafe_diagnostics = fixture
        .collect(UNSAFE_SOURCE, security_only_options())
        .await;
    let finding = unsafe_diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == Some(NumberOrString::String("CWE-89".to_owned())))
        .unwrap_or_else(|| panic!("expected SQL injection diagnostic: {unsafe_diagnostics:#?}"));
    assert_eq!(finding.source.as_deref(), Some("hyperion-security"));
    assert!(
        finding.range.start < finding.range.end,
        "security diagnostic must identify a non-empty source range: {finding:#?}"
    );

    fixture.replace_source(SAFE_SOURCE);
    let safe_diagnostics = fixture.collect(SAFE_SOURCE, security_only_options()).await;
    assert!(
        !has_code(&safe_diagnostics, "CWE-89"),
        "a prepared statement must clear the previous SQL injection finding: {safe_diagnostics:#?}"
    );
}

#[tokio::test]
async fn phased_security_runs_while_workspace_semantics_are_initializing() {
    let fixture = Fixture::new(UNSAFE_SOURCE);
    let publications = Arc::new(Mutex::new(Vec::<Vec<Diagnostic>>::new()));
    let captured = Arc::clone(&publications);

    collect_and_publish_phased(
        fixture.file_id,
        "java",
        UNSAFE_SOURCE,
        &fixture.host,
        Some(&fixture.security),
        false,
        DiagnosticOptions::default(),
        || true,
        move |diagnostics| {
            captured.lock().push(diagnostics);
            std::future::ready(true)
        },
    )
    .await;

    let publications = publications.lock();
    assert!(
        publications.len() >= 2,
        "expected parse and security publications, got {publications:#?}"
    );
    assert!(
        has_code(publications.last().expect("final publication"), "CWE-89"),
        "final publication should contain the SQL injection finding: {publications:#?}"
    );
}
