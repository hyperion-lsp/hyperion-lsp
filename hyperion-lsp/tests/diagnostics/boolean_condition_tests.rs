// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for goto-definition in boolean conditions

lsp_test_suite! {
    name: "Boolean Condition Method Calls",

    test simple_boolean_condition {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    private User user;

                    public boolean check() {
                        if (user.isA<|>ctive()) {
                            return true;
                        }
                        return false;
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    public boolean isActive() { return true; }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "User.java"
    }

    test chained_call_in_condition {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    private User user;

                    public boolean check() {
                        if (user.getProfile().isC<|>omplete()) {
                            return true;
                        }
                        return false;
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    public Profile getProfile() { return null; }
                }
            ",
            "com/example/Profile.java" => r"
                package com.example;

                public class Profile {
                    public boolean isComplete() { return true; }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "Profile.java"
    }

    test and_condition_multiple_calls {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    private User user;

                    public boolean check() {
                        if (user.isA<|1>ctive() && user.isVer<|2>ified()) {
                            return true;
                        }
                        return false;
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    public boolean isActive() { return true; }
                    public boolean isVerified() { return true; }
                }
            "
        },
        nav: "com/example/Test.java", "<|1>" => "User.java"
    }

    test complex_boolean_with_chained_calls {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    private LibraryBook book;

                    public boolean check() {
                        if (book.getMetadata() != null
                                && book.getMetadata().getT<|>ype() != null) {
                            return true;
                        }
                        return false;
                    }
                }
            ",
            "com/example/LibraryBook.java" => r"
                package com.example;

                public class LibraryBook {
                    public BookMetadata getMetadata() { return null; }
                }
            ",
            "com/example/BookMetadata.java" => r"
                package com.example;

                public class BookMetadata {
                    public String getType() { return null; }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "BookMetadata.java"
    }

    test method_call_on_parameter_in_condition {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    public boolean check(Edition version) {
                        return version.bel<|>ow(5);
                    }
                }
            ",
            "com/example/Edition.java" => r"
                package com.example;

                public class Edition {
                    public boolean below(int v) { return false; }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "Edition.java"
    }

    test static_field_method_call_in_condition {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    private static final TypeRegistry TYPES = null;

                    public boolean check(String type) {
                        if (TYPES.cont<|>ains(type)) return true;
                        return false;
                    }
                }
            ",
            "com/example/TypeRegistry.java" => r"
                package com.example;

                public class TypeRegistry {
                    public boolean contains(String type) { return false; }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "TypeRegistry.java"
    }

    test inline_return_with_complex_condition {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    private LibraryBook book;
                    private Edition version;

                    public boolean check() {
                        if (book.getMetadata() != null
                                && book.getMetadata().getTy<|>pe() != null
                                && version.below(5)) return true;
                        return false;
                    }
                }
            ",
            "com/example/LibraryBook.java" => r"
                package com.example;

                public class LibraryBook {
                    public BookMetadata getMetadata() { return null; }
                }
            ",
            "com/example/BookMetadata.java" => r"
                package com.example;

                public class BookMetadata {
                    public String getType() { return null; }
                }
            ",
            "com/example/Edition.java" => r"
                package com.example;

                public class Edition {
                    public boolean below(int v) { return false; }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "BookMetadata.java"
    }

    test lombok_getter_in_condition {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    private User user;

                    public boolean check() {
                        if (user.getN<|>ame() != null) {
                            return true;
                        }
                        return false;
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;
                import lombok.Data;

                @Data
                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "User.java"
    }

    test lombok_getter_chained_in_condition {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    private LibraryBook book;

                    public boolean check() {
                        if (book.getMetadata() != null
                                && book.getMetadata().getT<|>ype() != null) {
                            return true;
                        }
                        return false;
                    }
                }
            ",
            "com/example/LibraryBook.java" => r"
                package com.example;
                import lombok.Data;

                @Data
                public class LibraryBook {
                    private BookMetadata metadata;
                }
            ",
            "com/example/BookMetadata.java" => r"
                package com.example;
                import lombok.Getter;

                public class BookMetadata {
                    @Getter
                    private String type;
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "BookMetadata.java"
    }
}
