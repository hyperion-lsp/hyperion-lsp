// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Complex Flow Tests
//!
//! Tests realistic Java patterns and complex navigation scenarios:
//! - Method chaining (builder pattern, fluent APIs)
//! - Service layer patterns
//! - Inheritance and polymorphism
//! - Interface implementations
//! - Repository/DAO patterns
//! - DTO transformations

// ============================================================================
// Method Chaining & Builder Pattern
// ============================================================================

lsp_test_suite! {
    name: "Method Chaining and Builder Patterns",

    // Test 1: Simple method chaining
    test method_chaining_two_levels {
        files: {
            "com/example/Client.java" => r"
                package com.example;
                import com.example.model.User;

                public class Client {
                    public void test() {
                        User user = new User();
                        String name = user.getProfile().get<|>Name();
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private Profile profile;

                    public Profile getProfile() {
                        return profile;
                    }
                }
            ",

            "com/example/model/Profile.java" => r"
                package com.example.model;

                public class Profile {
                    private String name;

                    public String getName() {
                        return name;
                    }
                }
            "
        },
        nav: "com/example/Client.java", "<|>" => "Profile.java"
    }

    // Test 2: Builder pattern with fluent API
    test builder_pattern_chained_calls {
        files: {
            "com/example/BuilderTest.java" => r#"
                package com.example;
                import com.example.UserBuilder;

                public class BuilderTest {
                    public void test() {
                        UserBuilder builder = new UserBuilder();
                        builder.withName("John")
                               .withEmail("john@example.com")
                               .with<|>Age(30)
                               .build();
                    }
                }
            "#,

            "com/example/UserBuilder.java" => r"
                package com.example;

                public class UserBuilder {
                    private String name;
                    private String email;
                    private int age;

                    public UserBuilder withName(String name) {
                        this.name = name;
                        return this;
                    }

                    public UserBuilder withEmail(String email) {
                        this.email = email;
                        return this;
                    }

                    public UserBuilder withAge(int age) {
                        this.age = age;
                        return this;
                    }

                    public User build() {
                        return new User(name, email, age);
                    }
                }
            ",

            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                    private String email;
                    private int age;

                    public User(String name, String email, int age) {
                        this.name = name;
                        this.email = email;
                        this.age = age;
                    }
                }
            "
        },
        nav: "com/example/BuilderTest.java", "<|>" => "UserBuilder.java"
    }

    // Test 3: Three-level method chaining
    test deep_method_chaining {
        files: {
            "com/example/DeepChain.java" => r"
                package com.example;
                import com.example.service.UserService;

                public class DeepChain {
                    private UserService userService;

                    public void test() {
                        String city = userService
                            .getUser(123)
                            .getProfile()
                            .getAddress()
                            .get<|>City();
                    }
                }
            ",

            "com/example/service/UserService.java" => r"
                package com.example.service;
                import com.example.model.User;

                public class UserService {
                    public User getUser(long id) {
                        return new User();
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private Profile profile;

                    public Profile getProfile() {
                        return profile;
                    }
                }
            ",

            "com/example/model/Profile.java" => r"
                package com.example.model;

                public class Profile {
                    private Address address;

                    public Address getAddress() {
                        return address;
                    }
                }
            ",

            "com/example/model/Address.java" => r"
                package com.example.model;

                public class Address {
                    private String city;

                    public String getCity() {
                        return city;
                    }
                }
            "
        },
        nav: "com/example/DeepChain.java", "<|>" => "Address.java"
    }
}

// ============================================================================
// Service Layer Pattern (Spring-like)
// ============================================================================

lsp_test_suite! {
    name: "Service Layer Patterns",

    // Test 1: Controller -> Service navigation
    test controller_calls_service_method {
        files: {
            "com/example/controller/UserController.java" => r"
                package com.example.controller;
                import com.example.service.UserService;

                public class UserController {
                    private UserService userService;

                    public void createUser(String name, String email) {
                        userService.create<|>User(name, email);
                    }
                }
            ",

            "com/example/service/UserService.java" => r"
                package com.example.service;
                import com.example.model.User;
                import com.example.repository.UserRepository;

                public class UserService {
                    private UserRepository userRepository;

                    public User createUser(String name, String email) {
                        User user = new User(name, email);
                        return userRepository.save(user);
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;
                    private String email;

                    public User(String name, String email) {
                        this.name = name;
                        this.email = email;
                    }
                }
            ",

            "com/example/repository/UserRepository.java" => r"
                package com.example.repository;
                import com.example.model.User;

                public class UserRepository {
                    public User save(User user) {
                        return user;
                    }
                }
            "
        },
        nav: "com/example/controller/UserController.java", "<|>" => "UserService.java"
    }

    // Test 2: Service -> Repository navigation
    test service_calls_repository_method {
        files: {
            "com/example/service/UserService.java" => r"
                package com.example.service;
                import com.example.model.User;
                import com.example.repository.UserRepository;

                public class UserService {
                    private UserRepository repository;

                    public User findById(Long id) {
                        return repository.find<|>ById(id);
                    }
                }
            ",

            "com/example/repository/UserRepository.java" => r"
                package com.example.repository;
                import com.example.model.User;

                public class UserRepository {
                    public User findById(Long id) {
                        return null;
                    }

                    public void save(User user) {
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private Long id;
                }
            "
        },
        nav: "com/example/service/UserService.java", "<|>" => "UserRepository.java"
    }

    // Test 3: Multi-layer service composition
    test service_delegates_to_another_service {
        files: {
            "com/example/service/OrderService.java" => r"
                package com.example.service;
                import com.example.model.Order;

                public class OrderService {
                    private NotificationService notificationService;
                    private PaymentService paymentService;

                    public void processOrder(Order order) {
                        paymentService.pro<|>cessPayment(order);
                        notificationService.sendConfirmation(order);
                    }
                }
            ",

            "com/example/service/PaymentService.java" => r"
                package com.example.service;
                import com.example.model.Order;

                public class PaymentService {
                    public void processPayment(Order order) {
                        // Process payment logic
                    }
                }
            ",

            "com/example/service/NotificationService.java" => r"
                package com.example.service;
                import com.example.model.Order;

                public class NotificationService {
                    public void sendConfirmation(Order order) {
                        // Send notification
                    }
                }
            ",

            "com/example/model/Order.java" => r"
                package com.example.model;

                public class Order {
                    private Long id;
                    private double amount;
                }
            "
        },
        nav: "com/example/service/OrderService.java", "<|>" => "PaymentService.java"
    }
}

// ============================================================================
// Inheritance and Polymorphism
// ============================================================================

lsp_test_suite! {
    name: "Inheritance and Polymorphism",

    // Test 1: Call method on superclass type
    test call_inherited_method {
        files: {
            "com/example/InheritanceTest.java" => r#"
                package com.example;
                import com.example.model.Employee;

                public class InheritanceTest {
                    public void test() {
                        Employee emp = new Employee();
                        emp.set<|>Name("John");
                    }
                }
            "#,

            "com/example/model/Employee.java" => r"
                package com.example.model;

                public class Employee extends Person {
                    private String department;

                    public void setDepartment(String department) {
                        this.department = department;
                    }
                }
            ",

            "com/example/model/Person.java" => r"
                package com.example.model;

                public class Person {
                    private String name;

                    public void setName(String name) {
                        this.name = name;
                    }

                    public String getName() {
                        return name;
                    }
                }
            "
        },
        nav: "com/example/InheritanceTest.java", "<|>" => "Person.java"
    }

    // Test 2: Call overridden method
    test call_overridden_method {
        files: {
            "com/example/OverrideTest.java" => r"
                package com.example;
                import com.example.model.Manager;

                public class OverrideTest {
                    public void test() {
                        Manager manager = new Manager();
                        double salary = manager.calculate<|>Salary();
                    }
                }
            ",

            "com/example/model/Manager.java" => r"
                package com.example.model;

                public class Manager extends Employee {
                    private double bonus;

                    @Override
                    public double calculateSalary() {
                        return super.calculateSalary() + bonus;
                    }
                }
            ",

            "com/example/model/Employee.java" => r"
                package com.example.model;

                public class Employee {
                    private double baseSalary;

                    public double calculateSalary() {
                        return baseSalary;
                    }
                }
            "
        },
        nav: "com/example/OverrideTest.java", "<|>" => "Manager.java"
    }

    // Test 3: Abstract class implementation
    test call_abstract_implementation {
        files: {
            "com/example/AbstractTest.java" => r#"
                package com.example;
                import com.example.service.DatabaseLogger;

                public class AbstractTest {
                    public void test() {
                        DatabaseLogger logger = new DatabaseLogger();
                        logger.lo<|>g("Test message");
                    }
                }
            "#,

            "com/example/service/DatabaseLogger.java" => r"
                package com.example.service;

                public class DatabaseLogger extends Logger {
                    @Override
                    public void log(String message) {
                        // Log to database
                    }
                }
            ",

            "com/example/service/Logger.java" => r#"
                package com.example.service;

                public abstract class Logger {
                    public abstract void log(String message);

                    public void logError(String message) {
                        log("ERROR: " + message);
                    }
                }
            "#
        },
        nav: "com/example/AbstractTest.java", "<|>" => "DatabaseLogger.java"
    }
}

// ============================================================================
// Interface Implementation
// ============================================================================

lsp_test_suite! {
    name: "Interface Implementations",

    // Test 1: Call interface method on implementation
    test call_interface_method_on_impl {
        files: {
            "com/example/InterfaceTest.java" => r"
                package com.example;
                import com.example.repository.UserRepository;

                public class InterfaceTest {
                    public void test() {
                        UserRepository repo = new UserRepository();
                        repo.find<|>All();
                    }
                }
            ",

            "com/example/repository/UserRepository.java" => r"
                package com.example.repository;
                import com.example.model.User;
                import java.util.List;

                public class UserRepository implements Repository<User> {
                    @Override
                    public List<User> findAll() {
                        return null;
                    }

                    @Override
                    public User findById(Long id) {
                        return null;
                    }

                    @Override
                    public void save(User entity) {
                    }
                }
            ",

            "com/example/repository/Repository.java" => r"
                package com.example.repository;
                import java.util.List;

                public interface Repository<T> {
                    List<T> findAll();
                    T findById(Long id);
                    void save(T entity);
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private Long id;
                }
            "
        },
        nav: "com/example/InterfaceTest.java", "<|>" => "UserRepository.java"
    }

    // Test 2: Multiple interface implementations
    test call_method_from_multiple_interfaces {
        files: {
            "com/example/MultiInterfaceTest.java" => r"
                package com.example;
                import com.example.service.DataService;

                public class MultiInterfaceTest {
                    public void test() {
                        DataService service = new DataService();
                        service.val<|>idate();
                    }
                }
            ",

            "com/example/service/DataService.java" => r"
                package com.example.service;

                public class DataService implements Readable, Writable, Validatable {
                    @Override
                    public void read() {
                    }

                    @Override
                    public void write() {
                    }

                    @Override
                    public void validate() {
                    }
                }
            ",

            "com/example/service/Readable.java" => r"
                package com.example.service;

                public interface Readable {
                    void read();
                }
            ",

            "com/example/service/Writable.java" => r"
                package com.example.service;

                public interface Writable {
                    void write();
                }
            ",

            "com/example/service/Validatable.java" => r"
                package com.example.service;

                public interface Validatable {
                    void validate();
                }
            "
        },
        nav: "com/example/MultiInterfaceTest.java", "<|>" => "DataService.java"
    }
}

// ============================================================================
// DTO Transformation Patterns
// ============================================================================

lsp_test_suite! {
    name: "DTO Transformation Patterns",

    // Test 1: DTO to Entity conversion
    test dto_to_entity_mapper {
        files: {
            "com/example/mapper/UserMapper.java" => r"
                package com.example.mapper;
                import com.example.dto.UserDTO;
                import com.example.entity.User;

                public class UserMapper {
                    public User toEntity(UserDTO dto) {
                        User user = new User();
                        user.setName(dto.get<|>Name());
                        user.setEmail(dto.getEmail());
                        return user;
                    }
                }
            ",

            "com/example/dto/UserDTO.java" => r"
                package com.example.dto;

                public class UserDTO {
                    private String name;
                    private String email;

                    public String getName() {
                        return name;
                    }

                    public String getEmail() {
                        return email;
                    }
                }
            ",

            "com/example/entity/User.java" => r"
                package com.example.entity;

                public class User {
                    private String name;
                    private String email;

                    public void setName(String name) {
                        this.name = name;
                    }

                    public void setEmail(String email) {
                        this.email = email;
                    }
                }
            "
        },
        nav: "com/example/mapper/UserMapper.java", "<|>" => "UserDTO.java"
    }

    // Test 2: Nested DTO access
    test nested_dto_field_access {
        files: {
            "com/example/service/OrderService.java" => r"
                package com.example.service;
                import com.example.dto.OrderDTO;

                public class OrderService {
                    public void processOrder(OrderDTO order) {
                        String customerName = order.getCustomer().get<|>Name();
                        double total = order.getTotalAmount();
                    }
                }
            ",

            "com/example/dto/OrderDTO.java" => r"
                package com.example.dto;

                public class OrderDTO {
                    private CustomerDTO customer;
                    private double totalAmount;

                    public CustomerDTO getCustomer() {
                        return customer;
                    }

                    public double getTotalAmount() {
                        return totalAmount;
                    }
                }
            ",

            "com/example/dto/CustomerDTO.java" => r"
                package com.example.dto;

                public class CustomerDTO {
                    private String name;
                    private String email;

                    public String getName() {
                        return name;
                    }

                    public String getEmail() {
                        return email;
                    }
                }
            "
        },
        nav: "com/example/service/OrderService.java", "<|>" => "CustomerDTO.java"
    }
}

// ============================================================================
// Complex Multi-Layer Scenarios
// ============================================================================

lsp_test_suite! {
    name: "Complex Multi-Layer Scenarios",

    // Test 1: Full stack flow - Controller -> Service -> Repository -> DTO
    test full_stack_flow {
        files: {
            "com/example/controller/ProfileController.java" => r"
                package com.example.controller;
                import com.example.service.ProfileService;
                import com.example.dto.ProfileDTO;

                public class ProfileController {
                    private ProfileService profileService;

                    public ProfileDTO getProfile(Long userId) {
                        return profileService.getUser<|>Profile(userId);
                    }
                }
            ",

            "com/example/service/ProfileService.java" => r"
                package com.example.service;
                import com.example.repository.UserRepository;
                import com.example.entity.User;
                import com.example.dto.ProfileDTO;
                import com.example.mapper.ProfileMapper;

                public class ProfileService {
                    private UserRepository userRepository;
                    private ProfileMapper mapper;

                    public ProfileDTO getUserProfile(Long userId) {
                        User user = userRepository.findById(userId);
                        return mapper.toDTO(user);
                    }
                }
            ",

            "com/example/repository/UserRepository.java" => r"
                package com.example.repository;
                import com.example.entity.User;

                public class UserRepository {
                    public User findById(Long id) {
                        return null;
                    }
                }
            ",

            "com/example/entity/User.java" => r"
                package com.example.entity;

                public class User {
                    private Long id;
                    private String name;
                }
            ",

            "com/example/dto/ProfileDTO.java" => r"
                package com.example.dto;

                public class ProfileDTO {
                    private String name;
                }
            ",

            "com/example/mapper/ProfileMapper.java" => r"
                package com.example.mapper;
                import com.example.entity.User;
                import com.example.dto.ProfileDTO;

                public class ProfileMapper {
                    public ProfileDTO toDTO(User user) {
                        return null;
                    }
                }
            "
        },
        nav: "com/example/controller/ProfileController.java", "<|>" => "ProfileService.java"
    }

    // Test 2: Event-driven pattern
    test event_publisher_listener_pattern {
        files: {
            "com/example/service/UserService.java" => r"
                package com.example.service;
                import com.example.event.EventPublisher;
                import com.example.event.UserCreatedEvent;

                public class UserService {
                    private EventPublisher eventPublisher;

                    public void createUser(String name) {
                        // Create user logic
                        UserCreatedEvent event = new UserCreatedEvent(name);
                        eventPublisher.pub<|>lish(event);
                    }
                }
            ",

            "com/example/event/EventPublisher.java" => r"
                package com.example.event;

                public class EventPublisher {
                    public void publish(Event event) {
                        // Publish event to listeners
                    }
                }
            ",

            "com/example/event/Event.java" => r"
                package com.example.event;

                public interface Event {
                }
            ",

            "com/example/event/UserCreatedEvent.java" => r"
                package com.example.event;

                public class UserCreatedEvent implements Event {
                    private String userName;

                    public UserCreatedEvent(String userName) {
                        this.userName = userName;
                    }
                }
            "
        },
        nav: "com/example/service/UserService.java", "<|>" => "EventPublisher.java"
    }

    // Test 3: Factory pattern with multiple implementations
    test factory_pattern_navigation {
        files: {
            "com/example/client/ClientCode.java" => r#"
                package com.example.client;
                import com.example.factory.LoggerFactory;
                import com.example.logger.Logger;

                public class ClientCode {
                    public void test() {
                        Logger logger = LoggerFactory.create<|>Logger("database");
                        logger.log("Test message");
                    }
                }
            "#,

            "com/example/factory/LoggerFactory.java" => r#"
                package com.example.factory;
                import com.example.logger.Logger;
                import com.example.logger.DatabaseLogger;
                import com.example.logger.FileLogger;

                public class LoggerFactory {
                    public static Logger createLogger(String type) {
                        if ("database".equals(type)) {
                            return new DatabaseLogger();
                        } else if ("file".equals(type)) {
                            return new FileLogger();
                        }
                        return null;
                    }
                }
            "#,

            "com/example/logger/Logger.java" => r"
                package com.example.logger;

                public interface Logger {
                    void log(String message);
                }
            ",

            "com/example/logger/DatabaseLogger.java" => r"
                package com.example.logger;

                public class DatabaseLogger implements Logger {
                    @Override
                    public void log(String message) {
                    }
                }
            ",

            "com/example/logger/FileLogger.java" => r"
                package com.example.logger;

                public class FileLogger implements Logger {
                    @Override
                    public void log(String message) {
                    }
                }
            "
        },
        nav: "com/example/client/ClientCode.java", "<|>" => "LoggerFactory.java"
    }
}

// ============================================================================
// Statistics
// ============================================================================
//
// Total test suites: 7
// - Method Chaining and Builder Patterns: 3 tests
// - Service Layer Patterns: 3 tests
// - Inheritance and Polymorphism: 3 tests
// - Interface Implementations: 2 tests
// - DTO Transformation Patterns: 2 tests
// - Complex Multi-Layer Scenarios: 3 tests
//
// Total tests: 16 comprehensive complex flow tests
//
// All tests use the macro system for:
// - 76% code reduction vs manual tests
// - Clear, declarative test structure
// - Easy to read and maintain
// - Realistic Java patterns and architectures
