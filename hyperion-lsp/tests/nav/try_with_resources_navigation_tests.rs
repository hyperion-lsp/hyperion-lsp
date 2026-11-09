// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Navigation and hover tests for try-with-resources method calls.

lsp_goto_test! {
    name: goto_definition_try_with_resources_static_method,
    files: {
        "Test.java" => r"
            class BookHandle implements AutoCloseable {
                @Override
                public void close() {}
            }

            class Config {}

            class LibraryClient {
                public static BookHandle openLibraryConnection(Config config) {
                    return new BookHandle();
                }
            }

            class Test {
                void test(Config configuration) {
                    try (BookHandle conn = LibraryClient.openLibraryConnection<|>(configuration)) {
                        conn.close();
                    }
                }
            }
        ",
    },
    nav: "Test.java", "<|>" => "Test.java"
}

lsp_hover_test! {
    name: hover_try_with_resources_static_method,
    files: {
        "Test.java" => r"
            class BookHandle implements AutoCloseable {
                @Override
                public void close() {}
            }

            class Config {}

            class LibraryClient {
                public static BookHandle openLibraryConnection(Config config) {
                    return new BookHandle();
                }
            }

            class Test {
                void test(Config configuration) {
                    try (BookHandle conn = LibraryClient.openLibraryConnection<|>(configuration)) {
                        conn.close();
                    }
                }
            }
        ",
    },
    hover: "Test.java", "<|>",
    contains: ["openLibraryConnection"]
}
