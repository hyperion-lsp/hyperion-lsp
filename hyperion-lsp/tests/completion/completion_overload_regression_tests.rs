// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_semantics::{call_resolution::resolve_call, symbol::Type};
use hyperion_workspace::WorkspaceIndex;

#[test]
fn overload_resolution_rejects_incomparable_specificity_without_sorting_panic() {
    let temp = tempfile::tempdir().expect("temp workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let source = r#"
        class BaseA {}
        class ValueA extends BaseA {}
        class BaseB {}
        class ValueB extends BaseB {}
        class BaseC {}
        class ValueC extends BaseC {}

        class Builder {
            Builder append(BaseA value) { return this; }
            Builder append(int value) { return this; }
            Builder append(ValueA value) { return this; }
            Builder append(BaseB value) { return this; }
            Builder append(long value) { return this; }
            Builder append(ValueB value) { return this; }
            Builder append(BaseC value) { return this; }
            Builder append(boolean value) { return this; }
            Builder append(ValueC value) { return this; }
            Builder append(double value) { return this; }
            Builder append(char value) { return this; }
            Builder append(byte value) { return this; }
        }
    "#;
    let path = temp.path().join("Builder.java");
    index
        .index_file_with_source(&path, source)
        .expect("overloads should index");

    let resolved = resolve_call(&index, "Builder", None, "append", &[Type::Unknown]);
    assert!(
        resolved.is_none(),
        "an incomplete call with incomparable overloads has no exact declaration identity"
    );
}
