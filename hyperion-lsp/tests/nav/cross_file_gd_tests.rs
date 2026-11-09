// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Cross-File Goto Definition Tests
//!
//! Comprehensive tests for goto definition (gd) across multiple files.
//! Tests cover:
//! - Class references
//! - Method calls
//! - Field access
//! - Constructor navigation
//! - Interface implementations
//! - Inheritance
//! - Static member access
//! - Import statements

// ============================================================================
// Basic Cross-File Navigation
// ============================================================================

lsp_test_suite! {
    name: "Basic Cross-File Navigation",

    // Test 1: Navigate from local variable type to class definition
    test variable_type_to_class {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    public void test() {
                        User user = new User();
                        ^
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;

                    public User() {}

                    public String getName() {
                        return name;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "^" => "User.java"
    }

    // Test 2: Navigate from field type to class definition
    test field_type_to_class {
        files: {
            "com/example/Service.java" => r"
                package com.example;
                import com.example.model.User;

                public class Service {
                    private Us<|>er currentUser;
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "User.java"
    }

    // Test 3: Navigate from method return type to class definition
    test return_type_to_class {
        files: {
            "com/example/Factory.java" => r"
                package com.example;
                import com.example.model.User;

                public class Factory {
                    public Us<|>er createUser() {
                        return new User();
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    public User() {}
                }
            "
        },
        nav: "com/example/Factory.java", "<|>" => "User.java"
    }

    // Test 4: Navigate from parameter type to class definition
    test parameter_type_to_class {
        files: {
            "com/example/Processor.java" => r"
                package com.example;
                import com.example.model.User;

                public class Processor {
                    public void process(Us<|>er user) {
                        // process user
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Processor.java", "<|>" => "User.java"
    }
}

// ============================================================================
// Method Call Navigation
// ============================================================================

lsp_test_suite! {
    name: "Cross-File Method Calls",

    // Test 1: Navigate from method call to definition in another file
    test method_call_to_definition {
        files: {
            "com/example/Client.java" => r"
                package com.example;
                import com.example.service.UserService;

                public class Client {
                    private UserService service = new UserService();

                    public void test() {
                        service.get<|>User();
                    }
                }
            ",

            "com/example/service/UserService.java" => r"
                package com.example.service;
                import com.example.model.User;

                public class UserService {
                    public User getUser() {
                        return new User();
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Client.java", "<|>" => "UserService.java"
    }

    // Test 2: Navigate from chained method call (field)
    test chained_method_on_field {
        files: {
            "com/example/Controller.java" => r"
                package com.example;
                import com.example.service.UserService;

                public class Controller {
                    private UserService userService;

                    public void test() {
                        String name = userService.getCurrentUser().get<|>Name();
                    }
                }
            ",

            "com/example/service/UserService.java" => r"
                package com.example.service;
                import com.example.model.User;

                public class UserService {
                    public User getCurrentUser() {
                        return new User();
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;

                    public String getName() {
                        return name;
                    }
                }
            "
        },
        nav: "com/example/Controller.java", "<|>" => "User.java"
    }

    // Test 3: Navigate from method call on parameter
    test method_call_on_parameter {
        files: {
            "com/example/Processor.java" => r"
                package com.example;
                import com.example.model.User;

                public class Processor {
                    public void process(User user) {
                        String name = user.get<|>Name();
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;

                    public String getName() {
                        return name;
                    }
                }
            "
        },
        nav: "com/example/Processor.java", "<|>" => "User.java"
    }

    // Test 4: Navigate from method call on local variable
    test method_call_on_local_var {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    public void test() {
                        User user = new User();
                        String name = user.get<|>Name();
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;

                    public String getName() {
                        return name;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}

// ============================================================================
// Field Access Navigation
// ============================================================================

lsp_test_suite! {
    name: "Cross-File Field Access",

    // Test 1: Navigate to public field in another file
    test public_field_access {
        files: {
            "com/example/Client.java" => r"
                package com.example;
                import com.example.model.Config;

                public class Client {
                    public void test() {
                        Config config = new Config();
                        int value = config.max<|>Size;
                    }
                }
            ",

            "com/example/model/Config.java" => r#"
                package com.example.model;

                public class Config {
                    public int maxSize = 100;
                    public String name = "default";
                }
            "#
        },
        nav: "com/example/Client.java", "<|>" => "Config.java"
    }

    // Test 2: Navigate to static field in another file
    test static_field_access {
        files: {
            "com/example/Client.java" => r"
                package com.example;
                import com.example.Constants;

                public class Client {
                    public void test() {
                        int max = Constants.MAX_<|>SIZE;
                    }
                }
            ",

            "com/example/Constants.java" => r#"
                package com.example;

                public class Constants {
                    public static final int MAX_SIZE = 1000;
                    public static final String VERSION = "1.0";
                }
            "#
        },
        nav: "com/example/Client.java", "<|>" => "Constants.java"
    }
}

// ============================================================================
// Constructor Navigation
// ============================================================================

lsp_test_suite! {
    name: "Cross-File Constructor Navigation",

    // Test 1: Navigate from new expression to constructor
    test new_expression_to_constructor {
        files: {
            "com/example/Factory.java" => r"
                package com.example;
                import com.example.model.User;

                public class Factory {
                    public User createUser() {
                        return new Us<|>er();
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    public User() {}

                    public User(String name) {
                        this.name = name;
                    }

                    private String name;
                }
            "
        },
        nav: "com/example/Factory.java", "<|>" => "User.java"
    }

    // Test 2: Navigate from constructor with arguments
    test parameterized_constructor {
        files: {
            "com/example/Factory.java" => r"
                package com.example;
                import com.example.model.User;

                public class Factory {
                    public User createUser(String name) {
                        return new Us<|>er(name);
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;

                    public User(String name) {
                        this.name = name;
                    }
                }
            "
        },
        nav: "com/example/Factory.java", "<|>" => "User.java"
    }
}

// ============================================================================
// Interface Navigation
// ============================================================================

lsp_test_suite! {
    name: "Interface Implementation Navigation",

    // Test 1: Navigate from interface type to interface definition
    test interface_type_reference {
        files: {
            "com/example/Client.java" => r"
                package com.example;
                import com.example.service.UserService;

                public class Client {
                    private UserSer<|>vice service;
                }
            ",

            "com/example/service/UserService.java" => r"
                package com.example.service;
                import com.example.model.User;

                public interface UserService {
                    User getUser();
                    void saveUser(User user);
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Client.java", "<|>" => "UserService.java"
    }

    // Test 2: Navigate from method call on interface variable
    test interface_method_call {
        files: {
            "com/example/Controller.java" => r"
                package com.example;
                import com.example.service.UserService;

                public class Controller {
                    private UserService service;

                    public void handle() {
                        service.get<|>User();
                    }
                }
            ",

            "com/example/service/UserService.java" => r"
                package com.example.service;
                import com.example.model.User;

                public interface UserService {
                    User getUser();
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {}
            "
        },
        nav: "com/example/Controller.java", "<|>" => "UserService.java"
    }
}

// ============================================================================
// Inheritance Navigation
// ============================================================================

lsp_test_suite! {
    name: "Inheritance Navigation",

    // Test 1: Navigate from extends clause to parent class
    test extends_to_parent {
        files: {
            "com/example/ChildClass.java" => r"
                package com.example;

                public class ChildClass extends Base<|>Class {
                    public void childMethod() {}
                }
            ",

            "com/example/BaseClass.java" => r"
                package com.example;

                public class BaseClass {
                    protected void baseMethod() {}
                }
            "
        },
        nav: "com/example/ChildClass.java", "<|>" => "BaseClass.java"
    }

    // Test 2: Navigate from implements clause to interface
    test implements_to_interface {
        files: {
            "com/example/service/UserServiceImpl.java" => r"
                package com.example.service;
                import com.example.model.User;

                public class UserServiceImpl implements UserSer<|>vice {
                    public User getUser() {
                        return new User();
                    }
                }
            ",

            "com/example/service/UserService.java" => r"
                package com.example.service;
                import com.example.model.User;

                public interface UserService {
                    User getUser();
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {}
            "
        },
        nav: "com/example/service/UserServiceImpl.java", "<|>" => "UserService.java"
    }

    // Test 3: Navigate from child to inherited method
    test call_inherited_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        ChildClass child = new ChildClass();
                        child.base<|>Method();
                    }
                }
            ",

            "com/example/ChildClass.java" => r"
                package com.example;

                public class ChildClass extends BaseClass {
                    public void childMethod() {}
                }
            ",

            "com/example/BaseClass.java" => r"
                package com.example;

                public class BaseClass {
                    public void baseMethod() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "BaseClass.java"
    }
}

// ============================================================================
// Static Member Access
// ============================================================================

lsp_test_suite! {
    name: "Static Member Navigation",

    // Test 1: Navigate from static method call to definition
    test static_method_call {
        files: {
            "com/example/Client.java" => r#"
                package com.example;
                import com.example.util.StringUtils;

                public class Client {
                    public void test() {
                        String result = StringUtils.to<|>UpperCase("hello");
                    }
                }
            "#,

            "com/example/util/StringUtils.java" => r"
                package com.example.util;

                public class StringUtils {
                    public static String toUpperCase(String str) {
                        return str.toUpperCase();
                    }

                    public static String toLowerCase(String str) {
                        return str.toLowerCase();
                    }
                }
            "
        },
        nav: "com/example/Client.java", "<|>" => "StringUtils.java"
    }

    // Test 2: Navigate from static field access
    test static_final_field {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.config.AppConfig;

                public class Main {
                    public void test() {
                        String version = AppConfig.VERS<|>ION;
                    }
                }
            ",

            "com/example/config/AppConfig.java" => r#"
                package com.example.config;

                public class AppConfig {
                    public static final String VERSION = "1.0.0";
                    public static final int MAX_CONNECTIONS = 100;
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "AppConfig.java"
    }
}

// ============================================================================
// Import Statement Navigation
// ============================================================================

lsp_test_suite! {
    name: "Import Statement Navigation",

    // Test 1: Navigate from single import to class
    test single_import_to_class {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.Us<|>er;

                public class Main {
                    public void test() {
                        User user = new User();
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 2: Navigate from import to service interface
    test import_interface {
        files: {
            "com/example/Controller.java" => r"
                package com.example;
                import com.example.service.UserSer<|>vice;

                public class Controller {
                    private UserService service;
                }
            ",

            "com/example/service/UserService.java" => r"
                package com.example.service;

                public interface UserService {
                    void process();
                }
            "
        },
        nav: "com/example/Controller.java", "<|>" => "UserService.java"
    }
}

// ============================================================================
// Multi-Package Navigation
// ============================================================================

lsp_test_suite! {
    name: "Multi-Package Cross-File Navigation",

    // Test 1: Deep package hierarchy
    test deep_package_navigation {
        files: {
            "com/example/app/controller/UserController.java" => r"
                package com.example.app.controller;
                import com.example.app.service.UserService;

                public class UserController {
                    private UserSer<|>vice service;
                }
            ",

            "com/example/app/service/UserService.java" => r"
                package com.example.app.service;
                import com.example.app.repository.UserRepository;

                public class UserService {
                    private UserRepository repository;

                    public void process() {}
                }
            ",

            "com/example/app/repository/UserRepository.java" => r"
                package com.example.app.repository;

                public class UserRepository {
                    public void save() {}
                }
            "
        },
        nav: "com/example/app/controller/UserController.java", "<|>" => "UserService.java"
    }

    // Test 2: Cross-package method chain
    test cross_package_method_chain {
        files: {
            "com/example/frontend/Controller.java" => r"
                package com.example.frontend;
                import com.example.backend.Service;

                public class Controller {
                    private Service service;

                    public void handle() {
                        service.getUser().get<|>Name();
                    }
                }
            ",

            "com/example/backend/Service.java" => r"
                package com.example.backend;
                import com.example.model.User;

                public class Service {
                    public User getUser() {
                        return new User();
                    }
                }
            ",

            "com/example/model/User.java" => r#"
                package com.example.model;

                public class User {
                    public String getName() {
                        return "John";
                    }
                }
            "#
        },
        nav: "com/example/frontend/Controller.java", "<|>" => "User.java"
    }
}

// ============================================================================
// Complex Realistic Scenarios
// ============================================================================

lsp_test_suite! {
    name: "Realistic Cross-File Scenarios",

    // Test 1: Repository pattern with DTO
    test repository_pattern_dto {
        files: {
            "com/example/service/UserService.java" => r"
                package com.example.service;
                import com.example.repository.UserRepository;
                import com.example.dto.UserDTO;

                public class UserService {
                    private UserRepository repository;

                    public UserDTO getUser(Long id) {
                        return repository.find<|>ById(id);
                    }
                }
            ",

            "com/example/repository/UserRepository.java" => r"
                package com.example.repository;
                import com.example.dto.UserDTO;

                public class UserRepository {
                    public UserDTO findById(Long id) {
                        return new UserDTO();
                    }
                }
            ",

            "com/example/dto/UserDTO.java" => r"
                package com.example.dto;

                public class UserDTO {
                    private Long id;
                    private String name;
                }
            "
        },
        nav: "com/example/service/UserService.java", "<|>" => "UserRepository.java"
    }

    // Test 2: Service layer with transformation
    test service_layer_transformation {
        files: {
            "com/example/controller/UserController.java" => r"
                package com.example.controller;
                import com.example.service.UserService;
                import com.example.dto.UserDTO;

                public class UserController {
                    private UserService service;

                    public UserDTO handle() {
                        return service.getCurrentUser().to<|>DTO();
                    }
                }
            ",

            "com/example/service/UserService.java" => r"
                package com.example.service;
                import com.example.model.User;

                public class UserService {
                    public User getCurrentUser() {
                        return new User();
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;
                import com.example.dto.UserDTO;

                public class User {
                    private String name;

                    public UserDTO toDTO() {
                        return new UserDTO();
                    }
                }
            ",

            "com/example/dto/UserDTO.java" => r"
                package com.example.dto;

                public class UserDTO {
                    private String name;
                }
            "
        },
        nav: "com/example/controller/UserController.java", "<|>" => "User.java"
    }
}
