// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test that complex JPA annotations don't prevent goto-definition
lsp_test_suite! {
    name: "JPA Annotation Navigation",

    test field_with_simple_jpa_annotation {
        files: {
            "com/example/Entity.java" => r#"
                package com.example;

                public class Entity {
                    @Column(name = "id", length = 32, nullable = false)
                    private String id;

                    public String getId() {
                        return i<|>d;
                    }
                }
            "#
        },
        nav: "com/example/Entity.java", "<|>" => "Entity.java"
    }

    test field_with_nested_join_annotation {
        files: {
            "com/example/Fixture.java" => r#"
                package com.example;
                import java.util.*;

                public class Fixture {
                    @JoinTable(
                            name = "fixture_owners",
                            joinColumns = @JoinColumn(name = "fixture_id"),
                            inverseJoinColumns = @JoinColumn(name = "profile_id"))
                    private List<String> ownerIds;

                    public List<String> getOwnerIds() {
                        return owner<|>Ids;
                    }
                }
            "#
        },
        nav: "com/example/Fixture.java", "<|>" => "Fixture.java"
    }

    test method_call_after_complex_annotations {
        files: {
            "com/example/Service.java" => r#"
                package com.example;

                public class Service {
                    @ElementCollection(targetClass = String.class, fetch = FetchType.EAGER)
                    @JoinTable(
                            name = "owners",
                            joinColumns = @JoinColumn(name = "id", columnDefinition = "char(32) NOT NULL"),
                            inverseJoinColumns = @JoinColumn(name = "pid", columnDefinition = "char(32) NOT NULL"))
                    @OrderColumn(name = "ordinal")
                    private List<String> ownerIds;

                    public void process(Helper helper) {
                        helper.do<|>Work();
                    }
                }
            "#,
            "com/example/Helper.java" => r"
                package com.example;

                public class Helper {
                    public void doWork() {}
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "Helper.java"
    }
}
