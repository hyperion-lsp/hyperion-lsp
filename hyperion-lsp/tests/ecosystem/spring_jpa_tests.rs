// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive Spring/JPA dependency navigation tests
// Testing: @Autowired, @Repository, @Service, entity relationships
lsp_test_suite! {
    name: "Spring and JPA Patterns",

    // @Autowired field
    test autowired_field {
        files: {
            "com/example/UserService.java" => r"
                package com.example;
                import org.springframework.beans.factory.annotation.Autowired;
                public class UserService {
                    @Autowired
                    private UserReposito<|>ry userRepository;

                    public void save(User user) {
                        userRepository.save(user);
                    }
                }
            ",
            "com/example/UserRepository.java" => r"
                package com.example;
                import org.springframework.stereotype.Repository;
                @Repository
                public class UserRepository {
                    public void save(User user) {}
                }
            ",
            "com/example/User.java" => "package com.example; public class User {}"
        },
        nav: "com/example/UserService.java", "<|>" => "UserRepository.java"
    }

    // @Autowired constructor
    test autowired_constructor {
        files: {
            "com/example/UserService.java" => r"
                package com.example;
                import org.springframework.beans.factory.annotation.Autowired;
                public class UserService {
                    private final UserRepository userRepository;

                    @Autowired
                    public UserService(UserReposito<|>ry userRepository) {
                        this.userRepository = userRepository;
                    }
                }
            ",
            "com/example/UserRepository.java" => r"
                package com.example;
                import org.springframework.stereotype.Repository;
                @Repository
                public class UserRepository {}
            "
        },
        nav: "com/example/UserService.java", "<|>" => "UserRepository.java"
    }

    // @Entity class - navigate from field type to entity
    test entity_class {
        files: {
            "com/example/User.java" => r#"
                package com.example;
                import javax.persistence.*;
                @Entity
                @Table(name = "users")
                public class User {
                    @Id
                    @GeneratedValue(strategy = GenerationType.AUTO)
                    private Long id;
                }
            "#,
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Us<|>er user = new User();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // @OneToMany relationship - navigate from field type to related entity
    test onetomany_relationship {
        files: {
            "com/example/User.java" => r#"
                package com.example;
                import javax.persistence.*;
                import java.util.*;
                @Entity
                public class User {
                    @OneToMany(mappedBy = "user")
                    private List<Order> or<|>ders;
                }
            "#,
            "com/example/Order.java" => r#"
                package com.example;
                import javax.persistence.*;
                @Entity
                public class Order {
                    @ManyToOne
                    @JoinColumn(name = "user_id")
                    private User user;
                }
            "#
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }

    // @ManyToOne relationship
    test manytoone_relationship {
        files: {
            "com/example/Order.java" => r#"
                package com.example;
                import javax.persistence.*;
                @Entity
                public class Order {
                    @ManyToOne
                    @JoinColumn(name = "user_id")
                    private Us<|>er user;
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                import javax.persistence.*;
                @Entity
                public class User {
                    @Id
                    private Long id;
                }
            "
        },
        nav: "com/example/Order.java", "<|>" => "User.java"
    }

    // @Service annotation
    test service_annotation {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        UserServi<|>ce service = null;
                    }
                }
            ",
            "com/example/UserService.java" => r"
                package com.example;
                import org.springframework.stereotype.Service;
                @Service
                public class UserService {
                    public void process() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "UserService.java"
    }

    // @Transactional method
    test transactional_method {
        files: {
            "com/example/UserService.java" => r"
                package com.example;
                import org.springframework.transaction.annotation.Transactional;
                public class UserService {
                    @Transactional
                    public void save(Us<|>er user) {
                        // save logic
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;
                import javax.persistence.*;
                @Entity
                public class User {
                    @Id
                    private Long id;
                }
            "
        },
        nav: "com/example/UserService.java", "<|>" => "User.java"
    }
}
