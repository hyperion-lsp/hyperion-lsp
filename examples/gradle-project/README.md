# Hyperion Gradle Example Project

This is a comprehensive multi-module Gradle project designed to test and demonstrate Hyperion LSP's Gradle integration capabilities.

The `core` module includes a small Thrift schema under
`core/src/main/thrift`. Its `compileThrift` task emits a Java source file into
`core/build/generated/thrift`, allowing the LSP fixture to exercise generated
source discovery and indexing without requiring a system Thrift installation.

## Project Structure

```
gradle-project/
├── core/           - Core domain models and business logic
├── api/            - API interfaces and DTOs
├── service/        - Service implementations
└── web/            - Spring Boot web application
```

## Modules

### Core Module
- Domain models (User, Product)
- Uses Lombok, Commons Lang3, Guava
- Basic validation

### API Module
- Service interfaces
- Data Transfer Objects (DTOs)
- JSON serialization with Jackson
- Depends on: core

### Service Module
- Service implementations
- Spring Framework integration
- Database connectivity
- Depends on: api, core

### Web Module
- Spring Boot application
- REST controllers
- Main application entry point
- Depends on: service, api, core

## Dependencies Highlighted

This project includes various common Java dependencies:

- **Spring Boot 3.5.0** - Web framework
- **Spring Framework 6.0.11** - Core Spring features
- **Lombok 1.18.30** - Boilerplate reduction
- **Jackson 2.15.2** - JSON processing
- **JUnit 5.10.0** - Testing framework
- **Mockito 5.5.0** - Mocking framework
- **PostgreSQL 42.6.0** - Database driver
- **HikariCP 5.0.1** - Connection pooling
- **Guava 32.1.2** - Google utilities
- **Commons Lang3 3.13.0** - Apache utilities

## Building

```bash
./gradlew build
```

## Running

```bash
./gradlew :web:bootRun
```

## Testing with Hyperion LSP

This project is designed to test:

1. **Multi-module resolution** - Navigate between modules
2. **External dependency resolution** - Navigate into Spring, Lombok, etc.
3. **Transitive dependencies** - Proper classpath construction
4. **Annotation processing** - Lombok support
5. **API/Implementation separation** - Interface to implementation navigation
