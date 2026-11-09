// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for cross-module enum navigation in Gradle multi-module projects
//!
//! These tests verify that enum goto-definition and find-references work
//! across module boundaries (e.g., shared-types module → app module).

// Test 1: Cross-module enum constant goto-definition
// Simulates: shared-types/src/.../Status.java used from app/src/.../Main.java
lsp_goto_test! {
    name: gd_cross_module_enum_constant,

    files: {
        "shared-types/src/main/java/com/shared/types/Status.java" => r"
            package com.shared.types;

            public enum Status {
                ACTIVE,
                INACTIVE,
                PENDING
            }
        ",
        "app/src/main/java/com/app/Main.java" => r"
            package com.app;

            import com.shared.types.Status;

            public class Main {
                public void process() {
                    Status status = Status.ACT<|>IVE;
                }
            }
        "
    },

    nav: "app/src/main/java/com/app/Main.java", "<|>" => "Status.java"
}

// Test 2: Cross-module enum type goto-definition
lsp_goto_test! {
    name: gd_cross_module_enum_type,

    files: {
        "core/src/main/java/com/core/model/Priority.java" => r"
            package com.core.model;

            public enum Priority {
                LOW,
                MEDIUM,
                HIGH
            }
        ",
        "service/src/main/java/com/service/Handler.java" => r"
            package com.service;

            import com.core.model.Priority;

            public class Handler {
                public void setPriority(Prio<|>rity p) {
                }
            }
        "
    },

    nav: "service/src/main/java/com/service/Handler.java", "<|>" => "Priority.java"
}

// Test 3: Cross-module enum constant in switch statement
lsp_goto_test! {
    name: gd_cross_module_enum_in_switch,

    files: {
        "common/src/main/java/com/common/EventType.java" => r"
            package com.common;

            public enum EventType {
                CREATED,
                UPDATED,
                DELETED
            }
        ",
        "processor/src/main/java/com/processor/EventProcessor.java" => r"
            package com.processor;

            import com.common.EventType;

            public class EventProcessor {
                public void process(EventType type) {
                    switch (type) {
                        case CRE<|>ATED:
                            break;
                        case UPDATED:
                            break;
                    }
                }
            }
        "
    },

    nav: "processor/src/main/java/com/processor/EventProcessor.java", "<|>" => "EventType.java"
}

// Test 4: Disambiguation - two enums with same constant name in different modules
lsp_goto_test! {
    name: gd_cross_module_enum_disambiguation,

    files: {
        "module-a/src/main/java/com/modulea/Status.java" => r"
            package com.modulea;

            public enum Status {
                ACTIVE,
                INACTIVE
            }
        ",
        "module-b/src/main/java/com/moduleb/Status.java" => r"
            package com.moduleb;

            public enum Status {
                ACTIVE,
                PENDING
            }
        ",
        "consumer/src/main/java/com/consumer/Consumer.java" => r"
            package com.consumer;

            import com.modulea.Status;  // Imports from module-a

            public class Consumer {
                public void check() {
                    Status s = Status.ACT<|>IVE;  // Should go to module-a, not module-b
                }
            }
        "
    },

    nav: "consumer/src/main/java/com/consumer/Consumer.java", "<|>" => "module-a"
}

// Test 4b (regression): Suffix substring must not match container.
//
// Bug: enum_resolver previously used `container.ends_with(receiver_simple)`,
// which would treat `com.foo.OrderStatus` as a match for receiver `Status`.
// We give the matching enum a uniquely-named path segment (`statuspkg`) so the
// `loc.uri.contains(expected)` assertion can't be misled by the substring
// `Status.java` appearing inside `OrderStatus.java`.
lsp_goto_test! {
    name: gd_enum_substring_no_match_status_vs_orderstatus,

    files: {
        "src/main/java/com/statuspkg/Status.java" => r"
            package com.statuspkg;

            public enum Status {
                ACTIVE,
                INACTIVE
            }
        ",
        "src/main/java/com/orderpkg/OrderStatus.java" => r"
            package com.orderpkg;

            public enum OrderStatus {
                ACTIVE,
                CANCELLED
            }
        ",
        "src/main/java/com/consumer/Consumer.java" => r"
            package com.consumer;

            import com.statuspkg.Status;

            public class Consumer {
                public void check() {
                    Status s = Status.ACT<|>IVE;
                }
            }
        "
    },

    nav: "src/main/java/com/consumer/Consumer.java", "<|>" => "statuspkg"
}

// Test 5: Cross-module enum with wildcard import
lsp_goto_test! {
    name: gd_cross_module_enum_wildcard_import,

    files: {
        "types/src/main/java/com/types/Level.java" => r"
            package com.types;

            public enum Level {
                DEBUG,
                INFO,
                WARN,
                ERROR
            }
        ",
        "logger/src/main/java/com/logger/Logger.java" => r"
            package com.logger;

            import com.types.*;

            public class Logger {
                public void log(Level level) {
                    if (level == Level.ERR<|>OR) {
                        System.err.println(level);
                    }
                }
            }
        "
    },

    nav: "logger/src/main/java/com/logger/Logger.java", "<|>" => "Level.java"
}
