// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Taint sink detection
//!
//! Identifies where tainted data becomes dangerous.

use crate::finding::VulnType;
use std::collections::HashSet;

/// Registry of known taint sinks
pub struct SinkRegistry {
    sinks: Vec<SinkPattern>,
}

/// Context validation strategy for sinks
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextValidator {
    /// Skip XSS checks in REST controller contexts (JSON responses)
    RestControllerContext,
}

#[derive(Debug, Clone)]
pub struct SinkPattern {
    pub receiver_types: HashSet<String>,
    pub method_names: HashSet<String>,
    pub vuln_type: VulnType,
    pub tainted_param_indices: Vec<usize>, // Which params are dangerous if tainted
    pub tainted_receiver: bool,            // Whether a tainted receiver should trigger this sink
    /// Whether this sink requires context validation before flagging
    pub requires_context_check: bool,
    /// Optional context validator to use
    pub context_validator: Option<ContextValidator>,
}

impl Default for SinkRegistry {
    #[allow(deprecated)]
    fn default() -> Self {
        Self::new()
    }
}

impl SinkRegistry {
    /// Create an empty sink registry
    ///
    /// Use `register_pattern()` to add patterns manually, or use the framework
    /// abstraction layer in the `frameworks` module for structured registration.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::sinks::SinkRegistry;
    /// use hyperion_security::finding::VulnType;
    /// use std::collections::HashSet;
    ///
    /// let mut registry = SinkRegistry::empty();
    /// let methods = ["execute"].iter().map(|s| s.to_string()).collect();
    /// let receivers = ["Statement"].iter().map(|s| s.to_string()).collect();
    /// registry.register_pattern(receivers, methods, VulnType::SqlInjection, vec![0], false);
    /// ```
    #[must_use]
    pub const fn empty() -> Self {
        Self { sinks: Vec::new() }
    }

    /// Create a sink registry with all built-in patterns
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
        let mut registry = Self { sinks: Vec::new() };
        registry.register_sql_sinks();
        registry.register_command_sinks();
        registry.register_path_sinks();
        registry.register_xss_sinks();
        registry.register_ssrf_sinks();
        registry.register_xml_sinks();
        registry.register_ldap_sinks();
        registry.register_deser_sinks();
        registry.register_log_sinks();
        // Phase 2: New taint sinks
        registry.register_template_sinks();
        registry.register_nosql_sinks();
        registry.register_el_sinks();
        registry.register_email_sinks();
        registry.register_redirect_sinks();
        // Phase 7: Additional sinks
        registry.register_redos_sinks();
        registry
    }

    /// Register a sink pattern
    ///
    /// This is the public API for frameworks to register their sink patterns.
    ///
    /// # Arguments
    /// * `receiver_types` - Set of receiver type names (e.g., "Statement", "stmt")
    /// * `method_names` - Set of method names (e.g., "execute", "executeQuery")
    /// * `vuln_type` - The vulnerability type (e.g., `VulnType::SqlInjection`)
    /// * `tainted_param_indices` - Which parameter indices are dangerous if tainted
    /// * `tainted_receiver` - Whether a tainted receiver triggers this sink
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::sinks::SinkRegistry;
    /// use hyperion_security::finding::VulnType;
    /// use std::collections::HashSet;
    ///
    /// let mut registry = SinkRegistry::empty();
    ///
    /// let methods: HashSet<String> = ["execute", "executeQuery"]
    ///     .iter().map(|s| s.to_string()).collect();
    /// let receivers: HashSet<String> = ["Statement", "stmt"]
    ///     .iter().map(|s| s.to_string()).collect();
    ///
    /// // First parameter is tainted, receiver is not
    /// registry.register_pattern(receivers, methods, VulnType::SqlInjection, vec![0], false);
    /// ```
    pub fn register_pattern(
        &mut self,
        receiver_types: HashSet<String>,
        method_names: HashSet<String>,
        vuln_type: VulnType,
        tainted_param_indices: Vec<usize>,
        tainted_receiver: bool,
    ) {
        self.sinks.push(SinkPattern {
            receiver_types,
            method_names,
            vuln_type,
            tainted_param_indices,
            tainted_receiver,
            requires_context_check: false,
            context_validator: None,
        });
    }

    /// Register a sink pattern with full control over context validation
    ///
    /// This allows frameworks to register sinks that require context validation
    /// (e.g., ResponseEntity.ok() should only trigger XSS in non-JSON contexts).
    ///
    /// # Arguments
    /// * `receiver_types` - Set of receiver type names
    /// * `method_names` - Set of method names
    /// * `vuln_type` - The vulnerability type
    /// * `tainted_param_indices` - Which parameter indices are dangerous if tainted
    /// * `tainted_receiver` - Whether a tainted receiver triggers this sink
    /// * `requires_context_check` - Whether this sink requires context validation
    /// * `context_validator` - Optional context validator to use
    #[allow(clippy::too_many_arguments)]
    pub fn register_pattern_with_context(
        &mut self,
        receiver_types: HashSet<String>,
        method_names: HashSet<String>,
        vuln_type: VulnType,
        tainted_param_indices: Vec<usize>,
        tainted_receiver: bool,
        requires_context_check: bool,
        context_validator: Option<ContextValidator>,
    ) {
        self.sinks.push(SinkPattern {
            receiver_types,
            method_names,
            vuln_type,
            tainted_param_indices,
            tainted_receiver,
            requires_context_check,
            context_validator,
        });
    }

    /// Register core (non-web-framework) sink patterns.
    ///
    /// This is used by the framework registry so that baseline sinks are available even when
    /// scanning via `FrameworkRegistry` (CLI path). Web-framework-specific sinks (Servlet/Spring/JAX-RS)
    /// are registered by their respective frameworks.
    pub fn register_core_sinks(&mut self) {
        self.register_core_sql_sinks();
        self.register_command_sinks();
        self.register_core_path_sinks();
        self.register_ssrf_sinks();
        self.register_xml_sinks();
        self.register_ldap_sinks();
        self.register_deser_sinks();
        self.register_log_sinks();
        // Phase 2: New taint sinks
        self.register_template_sinks();
        self.register_nosql_sinks();
        self.register_el_sinks();
        self.register_email_sinks();
        // Phase 7: Additional sinks
        self.register_redos_sinks();
    }

    fn register_core_sql_sinks(&mut self) {
        // JDBC Statement.execute*
        let statement_methods: HashSet<String> = [
            "execute",
            "executeQuery",
            "executeUpdate",
            "executeBatch",
            "addBatch",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let statement_receivers: HashSet<String> = ["Statement", "stmt", "statement", "st", "s"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: statement_receivers,
            method_names: statement_methods,
            vuln_type: VulnType::SqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // JDBC PreparedStatement/CallableStatement no-arg execute* methods (receiver taint)
        //
        // If a PreparedStatement was created from tainted SQL (e.g., conn.prepareStatement("... " + whoops)),
        // the receiver itself becomes the dangerous object, and later no-arg executeQuery()/executeUpdate()/execute()
        // must still be flagged.
        let ps_methods: HashSet<String> = [
            "execute",
            "executeQuery",
            "executeUpdate",
            "executeBatch",
            "executeLargeUpdate",
            "executeLargeBatch",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let ps_receivers: HashSet<String> = [
            "PreparedStatement",
            "CallableStatement",
            "ps",
            "pstmt",
            "preparedStatement",
            "cs",
            "callableStatement",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: ps_receivers,
            method_names: ps_methods,
            vuln_type: VulnType::SqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: true,
            requires_context_check: false,
            context_validator: None,
        });

        // JDBC Connection.prepareStatement/prepareCall/nativeSQL (SQL string itself tainted)
        let conn_methods: HashSet<String> = ["prepareStatement", "prepareCall", "nativeSQL"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let conn_receivers: HashSet<String> = ["Connection", "conn", "connection", "c"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: conn_receivers,
            method_names: conn_methods,
            vuln_type: VulnType::SqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // JPA/Hibernate
        let jpa_methods: HashSet<String> = ["createQuery", "createNativeQuery", "createSQLQuery"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let jpa_receivers: HashSet<String> =
            ["EntityManager", "entityManager", "em", "Session", "session"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        self.sinks.push(SinkPattern {
            receiver_types: jpa_receivers,
            method_names: jpa_methods,
            vuln_type: VulnType::SqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_core_path_sinks(&mut self) {
        // File constructors and path operations
        let path_methods: HashSet<String> = [
            "get",
            "of",
            "resolve",
            "resolveSibling",
            "readAllBytes",
            "write",
            "delete",
            "copy",
            "move",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let path_receivers: HashSet<String> = ["Paths", "Path", "Files", "path"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: path_receivers,
            method_names: path_methods,
            vuln_type: VulnType::PathTraversal,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_sql_sinks(&mut self) {
        // Statement.execute*
        let statement_methods: HashSet<String> = [
            "execute",
            "executeQuery",
            "executeUpdate",
            "executeBatch",
            "addBatch",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let statement_receivers: HashSet<String> = ["Statement", "stmt", "statement", "st", "s"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: statement_receivers,
            method_names: statement_methods,
            vuln_type: VulnType::SqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // PreparedStatement/CallableStatement no-arg execute* methods (receiver taint)
        let ps_methods: HashSet<String> = [
            "execute",
            "executeQuery",
            "executeUpdate",
            "executeBatch",
            "executeLargeUpdate",
            "executeLargeBatch",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let ps_receivers: HashSet<String> = [
            "PreparedStatement",
            "CallableStatement",
            "ps",
            "pstmt",
            "preparedStatement",
            "cs",
            "callableStatement",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: ps_receivers,
            method_names: ps_methods,
            vuln_type: VulnType::SqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: true,
            requires_context_check: false,
            context_validator: None,
        });

        // Connection.prepareStatement (if the SQL string itself is tainted)
        let conn_methods: HashSet<String> = ["prepareStatement", "prepareCall", "nativeSQL"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let conn_receivers: HashSet<String> = ["Connection", "conn", "connection", "c"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: conn_receivers,
            method_names: conn_methods,
            vuln_type: VulnType::SqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // JPA/Hibernate
        let jpa_methods: HashSet<String> = ["createQuery", "createNativeQuery", "createSQLQuery"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let jpa_receivers: HashSet<String> =
            ["EntityManager", "entityManager", "em", "Session", "session"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        self.sinks.push(SinkPattern {
            receiver_types: jpa_receivers,
            method_names: jpa_methods,
            vuln_type: VulnType::SqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // Spring JdbcTemplate
        let jdbc_methods: HashSet<String> = [
            "execute",
            "query",
            "update",
            "batchUpdate",
            "queryForObject",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let jdbc_receivers: HashSet<String> = [
            "JdbcTemplate",
            "jdbcTemplate",
            "NamedParameterJdbcTemplate",
            "namedParameterJdbcTemplate",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: jdbc_receivers,
            method_names: jdbc_methods,
            vuln_type: VulnType::SqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_command_sinks(&mut self) {
        // Runtime.exec
        let runtime_methods: HashSet<String> = std::iter::once("exec".to_string()).collect();

        let runtime_receivers: HashSet<String> = ["Runtime", "runtime", "rt"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: runtime_receivers,
            method_names: runtime_methods,
            vuln_type: VulnType::CommandInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // ProcessBuilder
        let pb_methods: HashSet<String> = ["command", "start"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let pb_receivers: HashSet<String> = ["ProcessBuilder", "processBuilder", "pb"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: pb_receivers,
            method_names: pb_methods,
            vuln_type: VulnType::CommandInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // ScriptEngine.eval
        let script_methods: HashSet<String> = std::iter::once("eval".to_string()).collect();

        let script_receivers: HashSet<String> = ["ScriptEngine", "scriptEngine", "engine"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: script_receivers,
            method_names: script_methods,
            vuln_type: VulnType::CommandInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_path_sinks(&mut self) {
        // File constructors and path operations
        let path_methods: HashSet<String> = [
            "get",
            "of",
            "resolve",
            "resolveSibling",
            "readAllBytes",
            "write",
            "delete",
            "copy",
            "move",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let path_receivers: HashSet<String> = ["Paths", "Path", "Files", "path"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: path_receivers,
            method_names: path_methods,
            vuln_type: VulnType::PathTraversal,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // Servlet dispatcher
        let dispatch_methods: HashSet<String> = ["getRequestDispatcher", "getResource"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let dispatch_receivers: HashSet<String> = [
            "ServletContext",
            "servletContext",
            "HttpServletRequest",
            "request",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: dispatch_receivers,
            method_names: dispatch_methods,
            vuln_type: VulnType::PathTraversal,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_xss_sinks(&mut self) {
        // Response output
        let response_methods: HashSet<String> = ["write", "print", "println", "append"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let response_receivers: HashSet<String> = [
            "PrintWriter",
            "writer",
            "out",
            "ServletOutputStream",
            "outputStream",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: response_receivers,
            method_names: response_methods,
            vuln_type: VulnType::Xss,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // Spring Model
        let model_methods: HashSet<String> = ["addAttribute", "addObject", "put"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let model_receivers: HashSet<String> = ["Model", "model", "ModelAndView", "modelAndView"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: model_receivers,
            method_names: model_methods,
            vuln_type: VulnType::Xss,
            tainted_param_indices: vec![1], // Second param is the value
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // Spring ResponseEntity - body() and ok() methods
        // Phase 1: These need context validation - JSON responses aren't XSS-vulnerable
        let response_entity_methods: HashSet<String> =
            ["body", "ok"].iter().map(|s| (*s).to_string()).collect();

        let response_entity_receivers: HashSet<String> = [
            "ResponseEntity",
            "responseEntity",
            "BodyBuilder",
            "bodyBuilder",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: response_entity_receivers,
            method_names: response_entity_methods,
            vuln_type: VulnType::Xss,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: true,
            context_validator: Some(ContextValidator::RestControllerContext),
        });

        // JAX-RS Response.entity() - renders response body
        // Phase 1: JAX-RS also needs context validation for JSON responses
        let jaxrs_entity_methods: HashSet<String> = std::iter::once("entity".to_string()).collect();

        let jaxrs_entity_receivers: HashSet<String> =
            ["Response", "ResponseBuilder", "response", "responseBuilder"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        self.sinks.push(SinkPattern {
            receiver_types: jaxrs_entity_receivers,
            method_names: jaxrs_entity_methods,
            vuln_type: VulnType::Xss,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: true,
            context_validator: Some(ContextValidator::RestControllerContext),
        });
    }

    fn register_ssrf_sinks(&mut self) {
        // URL construction
        let url_methods: HashSet<String> = ["openConnection", "openStream"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let url_receivers: HashSet<String> = ["URL", "URI", "url", "uri"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: url_receivers,
            method_names: url_methods,
            vuln_type: VulnType::Ssrf,
            tainted_param_indices: vec![],
            tainted_receiver: true,
            requires_context_check: false,
            context_validator: None,
        });

        // URI.create(...) and similar helpers
        let uri_create_methods: HashSet<String> = std::iter::once("create".to_string()).collect();

        let uri_create_receivers: HashSet<String> =
            ["URI", "URL"].iter().map(|s| (*s).to_string()).collect();

        self.sinks.push(SinkPattern {
            receiver_types: uri_create_receivers,
            method_names: uri_create_methods,
            vuln_type: VulnType::Ssrf,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // URI.create() static method - SSRF sink when the argument is tainted
        let uri_create_methods: HashSet<String> = std::iter::once("create".to_string()).collect();

        let uri_static_receivers: HashSet<String> = std::iter::once("URI".to_string()).collect();

        self.sinks.push(SinkPattern {
            receiver_types: uri_static_receivers,
            method_names: uri_create_methods,
            vuln_type: VulnType::Ssrf,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // HTTP clients
        let http_methods: HashSet<String> = [
            "execute",
            "send",
            "get",
            "post",
            "put",
            "delete",
            "getForObject",
            "postForObject",
            "exchange",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let http_receivers: HashSet<String> = [
            "HttpClient",
            "httpClient",
            "RestTemplate",
            "restTemplate",
            "WebClient",
            "webClient",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: http_receivers,
            method_names: http_methods,
            vuln_type: VulnType::Ssrf,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_xml_sinks(&mut self) {
        // XML parsing
        let xml_methods: HashSet<String> = [
            "parse",
            "unmarshal",
            "createXMLStreamReader",
            "createXMLEventReader",
            "newTransformer",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let xml_receivers: HashSet<String> = [
            "DocumentBuilder",
            "documentBuilder",
            "SAXParser",
            "saxParser",
            "XMLReader",
            "xmlReader",
            "Unmarshaller",
            "unmarshaller",
            "XMLInputFactory",
            "TransformerFactory",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: xml_receivers,
            method_names: xml_methods,
            vuln_type: VulnType::XxeInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_ldap_sinks(&mut self) {
        let ldap_methods: HashSet<String> = ["search", "lookup"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let ldap_receivers: HashSet<String> = [
            "DirContext",
            "dirContext",
            "InitialDirContext",
            "LdapTemplate",
            "ldapTemplate",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: ldap_receivers,
            method_names: ldap_methods,
            vuln_type: VulnType::LdapInjection,
            tainted_param_indices: vec![0, 1],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_deser_sinks(&mut self) {
        // ObjectInputStream
        let deser_methods: HashSet<String> = std::iter::once("readObject".to_string()).collect();

        let deser_receivers: HashSet<String> = ["ObjectInputStream", "ois"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: deser_receivers,
            method_names: deser_methods,
            vuln_type: VulnType::UnsafeDeserialization,
            tainted_param_indices: vec![],
            tainted_receiver: true,
            requires_context_check: false,
            context_validator: None,
        });

        // YAML
        let yaml_methods: HashSet<String> = ["load", "loadAll"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let yaml_receivers: HashSet<String> =
            ["Yaml", "yaml"].iter().map(|s| (*s).to_string()).collect();

        self.sinks.push(SinkPattern {
            receiver_types: yaml_receivers,
            method_names: yaml_methods,
            vuln_type: VulnType::UnsafeDeserialization,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_log_sinks(&mut self) {
        let log_methods: HashSet<String> = [
            "info", "warn", "error", "debug", "trace", "log", "severe", "warning",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let log_receivers: HashSet<String> = ["Logger", "logger", "log", "LOG"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: log_receivers,
            method_names: log_methods,
            vuln_type: VulnType::LogInjection,
            tainted_param_indices: vec![],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    // Phase 2: New taint sink registration functions
    fn register_template_sinks(&mut self) {
        // Velocity
        let velocity_methods: HashSet<String> = ["evaluate", "merge"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let velocity_receivers: HashSet<String> = [
            "VelocityEngine",
            "velocityEngine",
            "Template",
            "template",
            "ve",
            "engine",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: velocity_receivers,
            method_names: velocity_methods,
            vuln_type: VulnType::TemplateInjection,
            tainted_param_indices: vec![0, 1], // Template string or context
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // Freemarker
        let freemarker_methods: HashSet<String> = ["process", "getTemplate"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let freemarker_receivers: HashSet<String> = [
            "Configuration",
            "Template",
            "configuration",
            "freemarkerConfig",
            "config",
            "template",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: freemarker_receivers,
            method_names: freemarker_methods,
            vuln_type: VulnType::TemplateInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // Thymeleaf
        let thymeleaf_methods: HashSet<String> = ["process", "processThrottled"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let thymeleaf_receivers: HashSet<String> = [
            "TemplateEngine",
            "templateEngine",
            "SpringTemplateEngine",
            "springTemplateEngine",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: thymeleaf_receivers,
            method_names: thymeleaf_methods,
            vuln_type: VulnType::TemplateInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // Spring Expression Language (SpEL) - CRITICAL
        let spel_methods: HashSet<String> = ["parseExpression", "getValue"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let spel_receivers: HashSet<String> = [
            "ExpressionParser",
            "expressionParser",
            "SpelExpressionParser",
            "spelExpressionParser",
            "parser",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: spel_receivers,
            method_names: spel_methods,
            vuln_type: VulnType::TemplateInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_nosql_sinks(&mut self) {
        // MongoDB
        let mongo_methods: HashSet<String> = ["find", "findOne", "aggregate", "parse"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let mongo_receivers: HashSet<String> = [
            "MongoCollection",
            "collection",
            "Document",
            "BasicDBObject",
            "mongoCollection",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: mongo_receivers,
            method_names: mongo_methods,
            vuln_type: VulnType::NoSqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // Cassandra
        let cassandra_methods: HashSet<String> = ["execute", "executeAsync"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let cassandra_receivers: HashSet<String> =
            ["Session", "session", "CqlSession", "cqlSession"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        self.sinks.push(SinkPattern {
            receiver_types: cassandra_receivers,
            method_names: cassandra_methods,
            vuln_type: VulnType::NoSqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // Elasticsearch
        let elastic_methods: HashSet<String> = [
            "search",
            "index",
            "update",
            "delete",
            "get",
            "query",
            "searchSource",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let elastic_receivers: HashSet<String> = [
            "RestHighLevelClient",
            "client",
            "SearchRequest",
            "searchRequest",
            "QueryBuilders",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: elastic_receivers,
            method_names: elastic_methods,
            vuln_type: VulnType::NoSqlInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_el_sinks(&mut self) {
        // JSF EL - Expression creation (primary sink)
        let jsf_factory_methods: HashSet<String> =
            ["createValueExpression", "createMethodExpression"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        let jsf_factory_receivers: HashSet<String> =
            ["ExpressionFactory", "expressionFactory", "factory"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        self.sinks.push(SinkPattern {
            receiver_types: jsf_factory_receivers,
            method_names: jsf_factory_methods,
            vuln_type: VulnType::ExpressionLanguageInjection,
            tainted_param_indices: vec![1], // Second parameter is the expression string
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // JSF EL - Expression evaluation (secondary sink)
        let jsf_methods: HashSet<String> = ["setValue", "getValue", "invoke"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let jsf_receivers: HashSet<String> = [
            "ValueExpression",
            "MethodExpression",
            "valueExpression",
            "methodExpression",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: jsf_receivers,
            method_names: jsf_methods,
            vuln_type: VulnType::ExpressionLanguageInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // OGNL (Struts)
        let ognl_methods: HashSet<String> = ["getValue", "setValue", "parseExpression"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let ognl_receivers: HashSet<String> = ["Ognl", "OgnlContext", "ognl"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: ognl_receivers,
            method_names: ognl_methods,
            vuln_type: VulnType::ExpressionLanguageInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // JEXL
        let jexl_methods: HashSet<String> = ["createExpression", "evaluate"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let jexl_receivers: HashSet<String> = ["JexlEngine", "jexlEngine", "JexlExpression"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: jexl_receivers,
            method_names: jexl_methods,
            vuln_type: VulnType::ExpressionLanguageInjection,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_email_sinks(&mut self) {
        let email_methods: HashSet<String> = [
            "setSubject",
            "addRecipient",
            "setFrom",
            "setTo",
            "setCc",
            "setBcc",
            "addHeader",
            "setReplyTo",
            "setSender",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let email_receivers: HashSet<String> = [
            "MimeMessage",
            "SimpleMailMessage",
            "mimeMessage",
            "mailMessage",
            "message",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: email_receivers,
            method_names: email_methods,
            vuln_type: VulnType::EmailHeaderInjection,
            tainted_param_indices: vec![0, 1],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    fn register_redirect_sinks(&mut self) {
        // Servlet
        let servlet_methods: HashSet<String> =
            std::iter::once("sendRedirect".to_string()).collect();

        let servlet_receivers: HashSet<String> = ["HttpServletResponse", "response"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: servlet_receivers,
            method_names: servlet_methods,
            vuln_type: VulnType::OpenRedirect,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // Spring RedirectView
        let spring_redirect_methods: HashSet<String> = ["setUrl", "setRedirectUrl"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let spring_redirect_receivers: HashSet<String> = [
            "RedirectView",
            "redirectView",
            "ModelAndView",
            "modelAndView",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: spring_redirect_receivers,
            method_names: spring_redirect_methods,
            vuln_type: VulnType::OpenRedirect,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // JAX-RS
        let jaxrs_methods: HashSet<String> = ["seeOther", "temporaryRedirect", "created"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let jaxrs_receivers: HashSet<String> = ["Response", "ResponseBuilder", "response"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: jaxrs_receivers,
            method_names: jaxrs_methods,
            vuln_type: VulnType::OpenRedirect,
            tainted_param_indices: vec![0],
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    // Phase 7: ReDoS (Regular Expression Denial of Service) sinks
    fn register_redos_sinks(&mut self) {
        // Pattern.compile - static method that creates regex patterns
        let pattern_methods: HashSet<String> = std::iter::once("compile".to_string()).collect();

        let pattern_receivers: HashSet<String> = ["Pattern", "pattern"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sinks.push(SinkPattern {
            receiver_types: pattern_receivers,
            method_names: pattern_methods,
            vuln_type: VulnType::ReDoS,
            tainted_param_indices: vec![0], // First arg is the pattern string
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });

        // String methods that take regex patterns
        // matches(regex), replaceAll(regex, replacement), replaceFirst(regex, replacement), split(regex)
        let string_regex_methods: HashSet<String> =
            ["matches", "replaceAll", "replaceFirst", "split"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        // For String methods, the receiver is the string instance, but the pattern is arg[0]
        // We need to track when arg[0] is tainted, not the receiver
        let string_receivers: HashSet<String> = [
            "String", "str", "s", "input", "text", "value", "data", "content",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sinks.push(SinkPattern {
            receiver_types: string_receivers,
            method_names: string_regex_methods,
            vuln_type: VulnType::ReDoS,
            tainted_param_indices: vec![0], // First arg is the pattern for all these methods
            tainted_receiver: false,
            requires_context_check: false,
            context_validator: None,
        });
    }

    /// Check if a method call is a sink
    #[must_use]
    pub fn is_sink(&self, receiver: Option<&str>, method_name: &str) -> Option<VulnType> {
        for pattern in &self.sinks {
            if pattern.method_names.contains(method_name) {
                if let Some(recv) = receiver {
                    if pattern.receiver_types.contains(recv) {
                        return Some(pattern.vuln_type);
                    }
                    // Fuzzy matching
                    for recv_type in &pattern.receiver_types {
                        if recv.ends_with(recv_type)
                            || recv.to_lowercase() == recv_type.to_lowercase()
                        {
                            return Some(pattern.vuln_type);
                        }
                    }
                }
            }
        }
        None
    }

    /// Get the sink pattern for a method call
    #[must_use]
    pub fn get_sink_pattern(
        &self,
        receiver: Option<&str>,
        method_name: &str,
    ) -> Option<&SinkPattern> {
        let recv = receiver?;

        // Multiple sink patterns can match via suffix rules (e.g., PreparedStatement ends_with Statement).
        // Prefer the most specific match:
        // - exact/case-insensitive receiver matches beat suffix matches
        // - for suffix matches, longer receiver type strings beat shorter ones
        let mut best: Option<(&SinkPattern, (u8, usize))> = None;

        for pattern in &self.sinks {
            if !pattern.method_names.contains(method_name) {
                continue;
            }

            // Exact match first
            let score: Option<(u8, usize)> = if pattern.receiver_types.contains(recv) {
                Some((2, recv.len()))
            } else {
                // Case-insensitive equality (mostly for resilience; Java types are case-sensitive)
                pattern
                    .receiver_types
                    .iter()
                    .filter(|t| recv.eq_ignore_ascii_case(t.as_str()))
                    .map(std::string::String::len)
                    .max()
                    .map(|matched_len| (2, matched_len))
                    .or_else(|| {
                        // Suffix match (e.g., java.sql.PreparedStatement ends_with PreparedStatement)
                        pattern
                            .receiver_types
                            .iter()
                            .filter(|t| recv.ends_with(t.as_str()))
                            .map(std::string::String::len)
                            .max()
                            .map(|matched_len| (1, matched_len))
                    })
            };

            if let Some(sc) = score {
                let replace = best.is_none_or(|(_, best_sc)| sc > best_sc);
                if replace {
                    best = Some((pattern, sc));
                }
            }
        }

        best.map(|(p, _)| p)
    }
}
