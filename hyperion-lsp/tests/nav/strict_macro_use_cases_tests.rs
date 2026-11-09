// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

goto_test! {
    goto_definition_resolves_single_exact_target {
    file "A.java" r"
        class Foo {
            void «declaration»foo() {}
        }
    ";

    file "B.java" r"
        class Bar {
            public Foo foo;
            void run() {
                foo.«use1»foo();
            }
        }
    ";

    target "B.java" "use1";

    expect "A.java" "declaration";
    }
}

goto_test! {
    goto_definition_can_be_empty {
    file "Arbitrary.java" r"
        class Arbitrary {
            void test() {
                «marker»;
            }
        }
    ";

    target "Arbitrary.java" "marker";

    expect none;
    }
}

references_test! {
    show_references_only_for_target_member {
    file "A.java" r"
        class Foo {
            void «declaration»foo() {}
        }
    ";

    file "B.java" r"
        class Bar {
            public Foo foo;

            Bar() {
                foo.«use1»foo();
            }

            void baz() {
                foo.«use2»foo();
            }

            void «other_decl»foo() {
                «other_use»foo();
            }
        }
    ";

    target "A.java" "declaration";

    expect {
        "B.java": ["use1", "use2"]
    };

    expect not {
        "B.java": ["other_decl", "other_use"]
    };

    expect contains {
        "B.java": ["use1"]
    };
    }
}

hover_test! {
    hover_contains_type_information {
    file "HoverExample.java" r#"
        class HoverExample {
            void test() {
                Str«target»ing value = "";
            }
        }
    "#;

    target "HoverExample.java" "target";

    expect contains ["String"];
    }
}

completions_test! {
    completions_contain_expected_labels {
    file "CompletionExample.java" r"
        class CompletionTarget {
            void alpha() {}
            void beta() {}
        }

        class CompletionExample {
            void test() {
                CompletionTarget target = new CompletionTarget();
                target.«cursor»
            }
        }
    ";

    target "CompletionExample.java" "cursor";

    expect contains ["alpha", "beta"];
    expect not ["gamma"];
    }
}

signature_test! {
    signature_help_is_none_outside_method_call {
        file "SignatureNone.java" r"
            class SignatureNone {
                void te«cursor»st() {}
            }
        ";

        target "SignatureNone.java" "cursor";

        expect none;
    }
}
