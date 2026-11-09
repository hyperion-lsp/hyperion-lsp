// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Taint source detection
//!
//! Identifies where untrusted data enters the application.

use crate::taint::TrustLevel;
use std::collections::HashSet;

/// A detected taint source
#[derive(Debug, Clone)]
pub struct TaintSource {
    pub kind: SourceKind,
    pub method_name: String,
    pub receiver_pattern: Option<String>,
}

/// Categories of taint sources
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    HttpRequest,
    Database,
    FileSystem,
    Environment,
    Network,
    Deserialization,
    CommandLine,
    MessageBroker, // Phase 1: Kafka, RabbitMQ, JMS
    ExternalApi,   // Phase 1: HTTP client responses
}

impl SourceKind {
    /// Get the trust level for this source kind
    ///
    /// Trust levels determine severity for log injection:
    /// - Untrusted (HTTP, CLI): HIGH severity
    /// - SemiTrusted (Kafka, DB, Config): LOW severity
    /// - Derived: MEDIUM severity
    #[must_use]
    pub const fn trust_level(self) -> TrustLevel {
        match self {
            // Direct user input - highest risk
            Self::HttpRequest | Self::CommandLine => TrustLevel::Untrusted,

            // Everything else is semi-trusted:
            // - MessageBroker/Database/Environment: internal controlled systems
            // - ExternalApi: typically from known APIs
            // - FileSystem/Network: context-dependent, often internal data
            // - Deserialization: data source determines risk, default semi-trusted
            Self::MessageBroker
            | Self::Database
            | Self::Environment
            | Self::ExternalApi
            | Self::FileSystem
            | Self::Network
            | Self::Deserialization => TrustLevel::SemiTrusted,
        }
    }
}

/// Registry of known taint sources
pub struct SourceRegistry {
    sources: Vec<SourcePattern>,
}

#[derive(Debug, Clone)]
pub struct SourcePattern {
    pub receiver_types: HashSet<String>,
    pub method_names: HashSet<String>,
    pub kind: SourceKind,
}

impl Default for SourceRegistry {
    #[allow(deprecated)]
    fn default() -> Self {
        Self::new()
    }
}

impl SourceRegistry {
    /// Create an empty source registry
    ///
    /// Use `register_pattern()` to add patterns manually, or use the framework
    /// abstraction layer in the `frameworks` module for structured registration.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::sources::{SourceRegistry, SourceKind};
    /// use std::collections::HashSet;
    ///
    /// let mut registry = SourceRegistry::empty();
    /// let methods = ["getParam"].iter().map(|s| s.to_string()).collect();
    /// let receivers = ["Request"].iter().map(|s| s.to_string()).collect();
    /// registry.register_pattern(receivers, methods, SourceKind::HttpRequest);
    /// ```
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    /// Create a source registry with all built-in patterns
    ///
    /// This is the legacy constructor that registers all hardcoded patterns.
    /// For new code, prefer using the framework abstraction layer.
    ///
    /// # Deprecated
    ///
    /// This method is deprecated in favor of using `FrameworkRegistry` for
    /// pluggable framework support. It will be removed in a future version.
    #[must_use]
    #[deprecated(
        since = "0.1.0",
        note = "Use FrameworkRegistry for pluggable framework support"
    )]
    pub fn new() -> Self {
        let mut registry = Self {
            sources: Vec::new(),
        };
        registry.register_servlet_sources();
        registry.register_spring_sources();
        registry.register_jaxrs_sources();
        registry.register_database_sources();
        registry.register_file_sources();
        registry.register_env_sources();
        // Phase 1: Message broker sources
        registry.register_kafka_sources();
        registry.register_rabbitmq_sources();
        registry.register_jms_sources();
        // Phase 1: Database result sources
        registry.register_mybatis_sources();
        // Phase 1: External API sources
        registry.register_http_client_sources();
        // Phase 6: Deserialization sources
        registry.register_deserialization_sources();
        registry
    }

    /// Register a source pattern
    ///
    /// This is the public API for frameworks to register their source patterns.
    ///
    /// # Arguments
    /// * `receiver_types` - Set of receiver type names (e.g., "HttpServletRequest", "request")
    /// * `method_names` - Set of method names (e.g., "getParameter", "getHeader")
    /// * `kind` - The kind of taint source (e.g., `SourceKind::HttpRequest`)
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::sources::{SourceRegistry, SourceKind};
    /// use std::collections::HashSet;
    ///
    /// let mut registry = SourceRegistry::empty();
    ///
    /// let methods: HashSet<String> = ["getParam", "getHeader"]
    ///     .iter().map(|s| s.to_string()).collect();
    /// let receivers: HashSet<String> = ["Request", "HttpRequest"]
    ///     .iter().map(|s| s.to_string()).collect();
    ///
    /// registry.register_pattern(receivers, methods, SourceKind::HttpRequest);
    /// ```
    pub fn register_pattern(
        &mut self,
        receiver_types: HashSet<String>,
        method_names: HashSet<String>,
        kind: SourceKind,
    ) {
        self.sources.push(SourcePattern {
            receiver_types,
            method_names,
            kind,
        });
    }

    /// Register core (non-web-framework) source patterns.
    ///
    /// This is used by the framework registry so that baseline sources are available even when
    /// scanning via `FrameworkRegistry` (CLI path). Web framework sources (Servlet/Spring/JAX-RS)
    /// are registered by their respective frameworks.
    pub fn register_core_sources(&mut self) {
        self.register_database_sources();
        self.register_file_sources();
        self.register_env_sources();
        // Phase 1: Message broker sources
        self.register_kafka_sources();
        self.register_rabbitmq_sources();
        self.register_jms_sources();
        // Phase 1: Database result sources
        self.register_mybatis_sources();
        // Phase 1: External API sources
        self.register_http_client_sources();
        // Phase 6: Deserialization sources
        self.register_deserialization_sources();
    }

    fn register_servlet_sources(&mut self) {
        // HttpServletRequest methods
        let servlet_methods: HashSet<String> = [
            "getParameter",
            "getParameterValues",
            "getParameterMap",
            "getHeader",
            "getHeaders",
            "getCookies",
            "getInputStream",
            "getReader",
            "getRequestURI",
            "getRequestURL",
            "getQueryString",
            "getPathInfo",
            "getRemoteUser",
            "getAttribute",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let servlet_receivers: HashSet<String> = [
            "HttpServletRequest",
            "ServletRequest",
            "request",
            "req",
            "httpRequest",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sources.push(SourcePattern {
            receiver_types: servlet_receivers,
            method_names: servlet_methods,
            kind: SourceKind::HttpRequest,
        });
    }

    fn register_spring_sources(&mut self) {
        // Spring WebRequest
        let spring_methods: HashSet<String> = [
            "getParameter",
            "getHeader",
            "getParameterValues",
            "getParameterMap",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let spring_receivers: HashSet<String> = ["WebRequest", "NativeWebRequest", "webRequest"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sources.push(SourcePattern {
            receiver_types: spring_receivers,
            method_names: spring_methods,
            kind: SourceKind::HttpRequest,
        });
    }

    fn register_jaxrs_sources(&mut self) {
        // JAX-RS UriInfo
        let jaxrs_methods: HashSet<String> =
            ["getQueryParameters", "getPathParameters", "getRequestUri"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        let jaxrs_receivers: HashSet<String> = ["UriInfo", "uriInfo"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sources.push(SourcePattern {
            receiver_types: jaxrs_receivers,
            method_names: jaxrs_methods,
            kind: SourceKind::HttpRequest,
        });
    }

    fn register_database_sources(&mut self) {
        // ResultSet methods
        let db_methods: HashSet<String> = [
            "getString",
            "getObject",
            "getBytes",
            "getInt",
            "getLong",
            "getBoolean",
            "getBlob",
            "getClob",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let db_receivers: HashSet<String> = ["ResultSet", "rs", "resultSet"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sources.push(SourcePattern {
            receiver_types: db_receivers,
            method_names: db_methods,
            kind: SourceKind::Database,
        });

        // JPA/Hibernate query result methods
        let jpa_methods: HashSet<String> = ["getResultList", "getSingleResult", "getResultStream"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let jpa_receivers: HashSet<String> = ["Query", "TypedQuery", "query", "typedQuery"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sources.push(SourcePattern {
            receiver_types: jpa_receivers,
            method_names: jpa_methods,
            kind: SourceKind::Database,
        });
    }

    fn register_file_sources(&mut self) {
        // File reading methods
        let file_methods: HashSet<String> = [
            "read",
            "readLine",
            "readAllBytes",
            "readAllLines",
            "readString",
            "lines",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let file_receivers: HashSet<String> = [
            "BufferedReader",
            "FileReader",
            "FileInputStream",
            "Files",
            "reader",
            "inputStream",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sources.push(SourcePattern {
            receiver_types: file_receivers,
            method_names: file_methods,
            kind: SourceKind::FileSystem,
        });

        // Multipart file uploads (untrusted filenames and content)
        let upload_methods: HashSet<String> = ["getOriginalFilename", "getInputStream", "getBytes"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let upload_receivers: HashSet<String> = ["MultipartFile", "multipartFile"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sources.push(SourcePattern {
            receiver_types: upload_receivers,
            method_names: upload_methods,
            kind: SourceKind::HttpRequest,
        });
    }

    fn register_env_sources(&mut self) {
        // System environment
        let env_methods: HashSet<String> = ["getenv", "getProperty"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let env_receivers: HashSet<String> = std::iter::once("System".to_string()).collect();

        self.sources.push(SourcePattern {
            receiver_types: env_receivers,
            method_names: env_methods,
            kind: SourceKind::Environment,
        });
    }

    fn register_kafka_sources(&mut self) {
        let kafka_methods: HashSet<String> = [
            "value",   // ConsumerRecord.value()
            "key",     // ConsumerRecord.key()
            "headers", // ConsumerRecord.headers()
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let kafka_receivers: HashSet<String> =
            ["ConsumerRecord", "consumerRecord", "record", "kafkaRecord"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        self.sources.push(SourcePattern {
            receiver_types: kafka_receivers,
            method_names: kafka_methods,
            kind: SourceKind::MessageBroker,
        });
    }

    fn register_rabbitmq_sources(&mut self) {
        let rabbit_methods: HashSet<String> = ["getBody", "getMessageProperties", "getHeaders"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let rabbit_receivers: HashSet<String> = ["Message", "message", "amqpMessage"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sources.push(SourcePattern {
            receiver_types: rabbit_receivers,
            method_names: rabbit_methods,
            kind: SourceKind::MessageBroker,
        });
    }

    fn register_jms_sources(&mut self) {
        let jms_methods: HashSet<String> = [
            "getText",
            "getBody",
            "getObject",
            "getString",
            "getStringProperty",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let jms_receivers: HashSet<String> = [
            "Message",
            "TextMessage",
            "ObjectMessage",
            "MapMessage",
            "jmsMessage",
            "textMessage",
            "textMsg",
            "message",
            "msg",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sources.push(SourcePattern {
            receiver_types: jms_receivers,
            method_names: jms_methods,
            kind: SourceKind::MessageBroker,
        });
    }

    fn register_mybatis_sources(&mut self) {
        let mybatis_methods: HashSet<String> = ["selectOne", "selectList", "selectMap"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let mybatis_receivers: HashSet<String> = ["SqlSession", "sqlSession"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sources.push(SourcePattern {
            receiver_types: mybatis_receivers,
            method_names: mybatis_methods,
            kind: SourceKind::Database,
        });
    }

    fn register_http_client_sources(&mut self) {
        // Apache HttpClient - EntityUtils static methods
        let entity_utils_methods: HashSet<String> =
            std::iter::once("toString".to_string()).collect();

        let entity_utils_receivers: HashSet<String> =
            std::iter::once("EntityUtils".to_string()).collect();

        self.sources.push(SourcePattern {
            receiver_types: entity_utils_receivers,
            method_names: entity_utils_methods,
            kind: SourceKind::ExternalApi,
        });

        // Apache HttpClient - HttpResponse and HttpEntity instance methods
        let httpclient_methods: HashSet<String> = ["getEntity", "getContent"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let httpclient_receivers: HashSet<String> =
            ["HttpResponse", "HttpEntity", "response", "entity"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        self.sources.push(SourcePattern {
            receiver_types: httpclient_receivers,
            method_names: httpclient_methods,
            kind: SourceKind::ExternalApi,
        });

        // Spring RestTemplate
        let rest_methods: HashSet<String> = [
            "getForObject",
            "getForEntity",
            "postForObject",
            "exchange",
            "getBody",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let rest_receivers: HashSet<String> = ["RestTemplate", "restTemplate", "ResponseEntity"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sources.push(SourcePattern {
            receiver_types: rest_receivers,
            method_names: rest_methods,
            kind: SourceKind::ExternalApi,
        });

        // Java 11+ HttpClient
        let java_http_methods: HashSet<String> =
            ["body", "send"].iter().map(|s| (*s).to_string()).collect();

        let java_http_receivers: HashSet<String> = [
            "HttpResponse",
            "HttpClient",
            "httpClient",
            "client",
            "response",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sources.push(SourcePattern {
            receiver_types: java_http_receivers,
            method_names: java_http_methods,
            kind: SourceKind::ExternalApi,
        });
    }

    fn register_deserialization_sources(&mut self) {
        // ObjectInputStream - Java native deserialization
        // readObject() and readUnshared() return attacker-controlled objects
        let ois_methods: HashSet<String> = ["readObject", "readUnshared"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let ois_receivers: HashSet<String> = [
            "ObjectInputStream",
            "ois",
            "objectInputStream",
            "inputStream",
            "stream",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sources.push(SourcePattern {
            receiver_types: ois_receivers,
            method_names: ois_methods,
            kind: SourceKind::Deserialization,
        });

        // XStream - XML/JSON serialization library
        // fromXML() returns attacker-controlled objects from untrusted input
        let xstream_methods: HashSet<String> = std::iter::once("fromXML".to_string()).collect();

        let xstream_receivers: HashSet<String> = ["XStream", "xstream", "xStream"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sources.push(SourcePattern {
            receiver_types: xstream_receivers,
            method_names: xstream_methods,
            kind: SourceKind::Deserialization,
        });

        // Kryo - fast Java serialization
        let kryo_methods: HashSet<String> = ["readObject", "readClassAndObject"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let kryo_receivers: HashSet<String> = ["Kryo", "kryo", "Input", "input"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sources.push(SourcePattern {
            receiver_types: kryo_receivers,
            method_names: kryo_methods,
            kind: SourceKind::Deserialization,
        });

        // Hessian - binary web service protocol
        let hessian_methods: HashSet<String> = std::iter::once("readObject".to_string()).collect();

        let hessian_receivers: HashSet<String> = ["HessianInput", "Hessian2Input", "hessianInput"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sources.push(SourcePattern {
            receiver_types: hessian_receivers,
            method_names: hessian_methods,
            kind: SourceKind::Deserialization,
        });

        // SnakeYAML - YAML deserialization (can lead to RCE)
        // Note: Also treated as a sink for unsafe deserialization detection
        let yaml_methods: HashSet<String> = ["load", "loadAs", "loadAll"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let yaml_receivers: HashSet<String> =
            ["Yaml", "yaml"].iter().map(|s| (*s).to_string()).collect();

        self.sources.push(SourcePattern {
            receiver_types: yaml_receivers,
            method_names: yaml_methods,
            kind: SourceKind::Deserialization,
        });
    }

    /// Check if a method call is a taint source
    #[must_use]
    pub fn is_source(&self, receiver: Option<&str>, method_name: &str) -> Option<SourceKind> {
        for pattern in &self.sources {
            if pattern.method_names.contains(method_name) {
                // If we have a receiver, check it matches
                if let Some(recv) = receiver {
                    if pattern.receiver_types.contains(recv) {
                        return Some(pattern.kind);
                    }
                    // Also check if receiver ends with the type name (e.g., "this.request")
                    for recv_type in &pattern.receiver_types {
                        if recv.ends_with(recv_type) || recv == recv_type.to_lowercase().as_str() {
                            return Some(pattern.kind);
                        }
                    }
                } else {
                    // No receiver - could still be a source if method name is distinctive
                    // Be conservative here
                }
            }
        }
        None
    }

    /// Check if an annotation indicates a taint source (Spring @RequestParam, etc.)
    #[must_use]
    pub fn is_source_annotation(annotation_name: &str) -> bool {
        matches!(
            annotation_name,
            "RequestParam"
                | "RequestHeader"
                | "CookieValue"
                | "PathVariable"
                | "RequestBody"
                | "ModelAttribute"
                | "QueryParam"
                | "HeaderParam"
                | "CookieParam"
                | "PathParam"
                | "FormParam"
                | "BeanParam"
                // Phase 1: Message broker annotations
                | "KafkaListener"
                | "RabbitListener"
                | "JmsListener"
                | "Payload"
        )
    }
}
