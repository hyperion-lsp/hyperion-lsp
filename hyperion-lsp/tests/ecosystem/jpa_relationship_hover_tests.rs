// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

lsp_hover_test! {
    name: many_to_one_hover_reports_target_defaults_and_owning_side,
    files: {
        "com/example/Order.java" => r#"
            package com.example;
            import jakarta.persistence.ManyToOne;

            class Order {
                @ManyToOne
                private User us<|>er;
            }
        "#,
        "com/example/User.java" => "package com.example; class User {}"
    },
    hover: "com/example/Order.java", "<|>",
    contains: [
        "**JPA relationship** (`@ManyToOne`)",
        "Target entity: `User`",
        "Fetch: `EAGER` (default)",
        "Side: owning"
    ]
}

lsp_hover_test! {
    name: one_to_many_hover_reports_generic_target_and_inverse_metadata,
    files: {
        "com/example/User.java" => r#"
            package com.example;
            import jakarta.persistence.*;
            import java.util.List;

            class User {
                @OneToMany(
                    mappedBy = "user",
                    fetch = FetchType.EAGER,
                    cascade = {CascadeType.PERSIST, CascadeType.REMOVE})
                private List<Order> ord<|>ers;
            }
        "#,
        "com/example/Order.java" => "package com.example; class Order {}"
    },
    hover: "com/example/User.java", "<|>",
    contains: [
        "**JPA relationship** (`@OneToMany`)",
        "Target entity: `Order`",
        "Fetch: `EAGER`",
        "Cascade: `PERSIST, REMOVE`",
        "Side: inverse (`mappedBy = \"user\"`)"
    ]
}

lsp_hover_test! {
    name: explicit_target_entity_overrides_collection_argument,
    files: {
        "com/example/Group.java" => r#"
            package com.example;
            import jakarta.persistence.*;
            import java.util.Set;

            class Group {
                @ManyToMany(targetEntity = Account.class, cascade = CascadeType.ALL)
                private Set<Object> memb<|>ers;
            }
        "#,
        "com/example/Account.java" => "package com.example; class Account {}"
    },
    hover: "com/example/Group.java", "<|>",
    contains: [
        "Target entity: `Account`",
        "Fetch: `LAZY` (default)",
        "Cascade: `ALL`"
    ]
}
