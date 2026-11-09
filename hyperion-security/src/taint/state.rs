// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Taint state tracking for security analysis

use crate::finding::{FlowStep as FindingFlowStep, Location};
use crate::sources::SourceKind;
use crate::VulnType;
use bitflags::bitflags;

/// Trust level of taint source data
///
/// Determines how much we trust the data source, which affects
/// severity levels for certain vulnerability types (especially log injection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TrustLevel {
    /// Direct user input - HTTP params, request body, CLI args
    /// Highest risk, results in HIGH severity for log injection
    #[default]
    Untrusted,

    /// Internal data - Kafka records, database records, config values
    /// Lower risk since data is from internal systems, results in LOW severity
    SemiTrusted,

    /// Derived data - parsed/transformed from user input
    /// Medium risk, results in MEDIUM severity
    Derived,
}

bitflags! {
    /// Vulnerability types that a tainted value is dangerous for
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct VulnTypes: u32 {
        const SQL_INJECTION = 1 << 0;
        const COMMAND_INJECTION = 1 << 1;
        const PATH_TRAVERSAL = 1 << 2;
        const XSS = 1 << 3;
        const SSRF = 1 << 4;
        const XXE_INJECTION = 1 << 5;
        const LDAP_INJECTION = 1 << 6;
        const LOG_INJECTION = 1 << 7;
        const UNSAFE_DESERIALIZATION = 1 << 8;
        // Phase 2: New taint sinks
        const TEMPLATE_INJECTION = 1 << 9;
        const NOSQL_INJECTION = 1 << 10;
        const EXPRESSION_LANGUAGE_INJECTION = 1 << 11;
        const EMAIL_HEADER_INJECTION = 1 << 12;
        const OPEN_REDIRECT = 1 << 13;
        // Phase 7: Additional sinks
        const REDOS = 1 << 14;
    }
}

impl VulnTypes {
    /// Create VulnTypes from a specific VulnType
    #[must_use]
    pub const fn from_vuln_type(vuln_type: VulnType) -> Self {
        match vuln_type {
            VulnType::SqlInjection => Self::SQL_INJECTION,
            VulnType::CommandInjection => Self::COMMAND_INJECTION,
            VulnType::PathTraversal => Self::PATH_TRAVERSAL,
            VulnType::Xss => Self::XSS,
            VulnType::Ssrf => Self::SSRF,
            VulnType::XxeInjection => Self::XXE_INJECTION,
            VulnType::LdapInjection => Self::LDAP_INJECTION,
            VulnType::LogInjection => Self::LOG_INJECTION,
            VulnType::UnsafeDeserialization => Self::UNSAFE_DESERIALIZATION,
            // Phase 2: New taint sinks
            VulnType::TemplateInjection => Self::TEMPLATE_INJECTION,
            VulnType::NoSqlInjection => Self::NOSQL_INJECTION,
            VulnType::ExpressionLanguageInjection => Self::EXPRESSION_LANGUAGE_INJECTION,
            VulnType::EmailHeaderInjection => Self::EMAIL_HEADER_INJECTION,
            VulnType::OpenRedirect => Self::OPEN_REDIRECT,
            // Phase 7: Additional sinks
            VulnType::ReDoS => Self::REDOS,
            // Non-taint-tracking vulnerabilities (pattern-based checks)
            VulnType::HardcodedSecret
            | VulnType::WeakCrypto
            | VulnType::InsecureRandom
            | VulnType::WeakEncryptionMode
            | VulnType::HardcodedCryptoIv
            | VulnType::WeakHashAlgorithm
            | VulnType::SmallKeySize
            | VulnType::InsecureTls
            | VulnType::EmptyPasswordCheck
            | VulnType::HardcodedCredentials
            | VulnType::DangerousMethod
            | VulnType::UnsafeReflection
            // Phase 4: Framework-specific checks (non-taint)
            | VulnType::CsrfDisabled
            | VulnType::InsecurePermitAll
            | VulnType::WeakPasswordEncoding
            | VulnType::InsecureDeserializationConfig
            | VulnType::InsecureCookie
            | VulnType::MissingSecurityHeader
            | VulnType::HqlInjection
            | VulnType::JwtMisuse
            | VulnType::MissingAccessControl
            // Phase 9: Additional pattern-based checks
            | VulnType::SensitiveDataLogging
            | VulnType::MassAssignment
            | VulnType::StackTraceExposure
            | VulnType::HardcodedInternalUrl
            // Phase 2 Tier 2: Pattern-based checks (non-taint)
            | VulnType::Idor
            | VulnType::RaceCondition
            | VulnType::InsufficientLogging
            // Phase 3 Tier 3: Pattern-based checks (non-taint)
            | VulnType::MissingRateLimiting
            | VulnType::NullPointerDos
            // Phase 4 Final: Pattern-based checks (non-taint)
            | VulnType::VulnerableDependency => Self::empty(),
        }
    }

    /// Check if this taint is dangerous for a specific vulnerability type
    #[must_use]
    pub const fn is_dangerous_for(&self, vuln_type: VulnType) -> bool {
        self.contains(Self::from_vuln_type(vuln_type))
    }

    /// All vulnerability types
    #[must_use]
    pub const fn all_vulns() -> Self {
        Self::SQL_INJECTION
            .union(Self::COMMAND_INJECTION)
            .union(Self::PATH_TRAVERSAL)
            .union(Self::XSS)
            .union(Self::SSRF)
            .union(Self::XXE_INJECTION)
            .union(Self::LDAP_INJECTION)
            .union(Self::LOG_INJECTION)
            .union(Self::UNSAFE_DESERIALIZATION)
            .union(Self::TEMPLATE_INJECTION)
            .union(Self::NOSQL_INJECTION)
            .union(Self::EXPRESSION_LANGUAGE_INJECTION)
            .union(Self::EMAIL_HEADER_INJECTION)
            .union(Self::OPEN_REDIRECT)
            .union(Self::REDOS)
    }
}

/// A step in a data flow path
#[derive(Debug, Clone)]
pub struct FlowStep {
    pub location: Location,
    pub description: String,
    pub code_snippet: String,
}

impl FlowStep {
    #[must_use]
    pub const fn new(location: Location, description: String, code_snippet: String) -> Self {
        Self {
            location,
            description,
            code_snippet,
        }
    }

    /// Convert to Finding's FlowStep
    #[must_use]
    pub fn to_finding_step(&self) -> FindingFlowStep {
        FindingFlowStep {
            location: self.location.clone(),
            description: self.description.clone(),
            code_snippet: self.code_snippet.clone(),
        }
    }
}

/// Represents the taint state of a value
#[derive(Debug, Clone)]
pub enum TaintState {
    /// Value is definitely clean/safe
    Clean,

    /// Value is tainted and dangerous for specific vulnerability types
    Tainted {
        /// Description of where the taint originated
        source: String,
        /// Optional high-level classification of the taint source
        source_kind: Option<SourceKind>,
        /// Data flow path from source to current location
        path: Vec<FlowStep>,
        /// Which vulnerability types this taint is dangerous for
        dangerous_for: VulnTypes,
        /// Trust level of the data source - affects severity for certain vulns
        trust_level: TrustLevel,
    },

    /// Taint status is unknown (conservatively treated as potentially tainted)
    Unknown,
}

impl TaintState {
    /// Create a new tainted state from a source with default (Untrusted) trust level
    #[must_use]
    pub fn tainted(
        source: String,
        location: Location,
        code_snippet: String,
        dangerous_for: VulnTypes,
    ) -> Self {
        Self::tainted_with_trust(
            source,
            location,
            code_snippet,
            dangerous_for,
            TrustLevel::Untrusted,
        )
    }

    /// Create a new tainted state from a source with explicit trust level
    #[must_use]
    pub fn tainted_with_trust(
        source: String,
        location: Location,
        code_snippet: String,
        dangerous_for: VulnTypes,
        trust_level: TrustLevel,
    ) -> Self {
        let initial_step = FlowStep::new(location, format!("Taint source: {source}"), code_snippet);
        Self::Tainted {
            source,
            source_kind: None,
            path: vec![initial_step],
            dangerous_for,
            trust_level,
        }
    }

    /// Create a tainted state with an explicit source kind
    #[must_use]
    pub fn tainted_with_kind(
        source: String,
        location: Location,
        code_snippet: String,
        dangerous_for: VulnTypes,
        trust_level: TrustLevel,
        source_kind: SourceKind,
    ) -> Self {
        let initial_step = FlowStep::new(location, format!("Taint source: {source}"), code_snippet);
        Self::Tainted {
            source,
            source_kind: Some(source_kind),
            path: vec![initial_step],
            dangerous_for,
            trust_level,
        }
    }

    /// Create a clean state
    #[must_use]
    pub const fn clean() -> Self {
        Self::Clean
    }

    /// Create an unknown state
    #[must_use]
    pub const fn unknown() -> Self {
        Self::Unknown
    }

    /// Check if this value is dangerous for a specific vulnerability type
    #[must_use]
    pub const fn is_dangerous_for(&self, vuln_type: VulnType) -> bool {
        match self {
            Self::Tainted { dangerous_for, .. } => dangerous_for.is_dangerous_for(vuln_type),
            Self::Unknown => true, // Conservative: unknown is potentially dangerous
            Self::Clean => false,
        }
    }

    /// Apply sanitization for specific vulnerability types
    #[must_use]
    pub fn sanitize(&self, sanitizes: VulnTypes) -> Self {
        match self {
            Self::Tainted {
                source,
                source_kind,
                path,
                dangerous_for,
                trust_level,
            } => {
                let remaining = *dangerous_for & !sanitizes;
                if remaining.is_empty() {
                    Self::Clean
                } else {
                    Self::Tainted {
                        source: source.clone(),
                        source_kind: *source_kind,
                        path: path.clone(),
                        dangerous_for: remaining,
                        trust_level: *trust_level,
                    }
                }
            }
            Self::Clean => Self::Clean,
            Self::Unknown => Self::Unknown, // Can't sanitize unknown taint
        }
    }

    /// Propagate taint through an operation, adding a flow step
    /// Trust level is preserved during propagation
    #[must_use]
    pub fn propagate(&self, location: Location, description: String, code_snippet: String) -> Self {
        match self {
            Self::Tainted {
                source,
                source_kind,
                path,
                dangerous_for,
                trust_level,
            } => {
                let mut new_path = path.clone();
                new_path.push(FlowStep::new(location, description, code_snippet));
                Self::Tainted {
                    source: source.clone(),
                    source_kind: *source_kind,
                    path: new_path,
                    dangerous_for: *dangerous_for,
                    trust_level: *trust_level,
                }
            }
            Self::Clean => Self::Clean,
            Self::Unknown => Self::Unknown,
        }
    }

    /// Get the data flow path if tainted
    #[must_use]
    pub fn path(&self) -> Option<&[FlowStep]> {
        match self {
            Self::Tainted { path, .. } => Some(path),
            Self::Clean | Self::Unknown => None,
        }
    }

    /// Get the source description if tainted
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::Tainted { source, .. } => Some(source),
            Self::Clean | Self::Unknown => None,
        }
    }

    /// Get the source kind if tainted
    #[must_use]
    pub const fn source_kind(&self) -> Option<SourceKind> {
        match self {
            Self::Tainted { source_kind, .. } => *source_kind,
            Self::Clean | Self::Unknown => None,
        }
    }

    /// Merge two taint states (e.g., for binary operations)
    /// If either is tainted, the result is tainted
    /// Trust level takes the lower trust (more suspicious) of the two
    #[must_use]
    pub fn merge(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Clean, Self::Clean) => Self::Clean,
            (Self::Tainted { .. }, Self::Clean) => self.clone(),
            (Self::Clean, Self::Tainted { .. }) => other.clone(),
            (
                Self::Tainted {
                    source: source1,
                    source_kind: source_kind1,
                    path: path1,
                    dangerous_for: dangerous1,
                    trust_level: trust1,
                },
                Self::Tainted {
                    dangerous_for: dangerous2,
                    trust_level: trust2,
                    ..
                },
            ) => {
                // Merge the dangerous_for flags
                // Take the lower trust level (Untrusted < Derived < SemiTrusted)
                let merged_trust = Self::lower_trust(*trust1, *trust2);
                Self::Tainted {
                    source: source1.clone(),
                    source_kind: *source_kind1,
                    path: path1.clone(),
                    dangerous_for: *dangerous1 | *dangerous2,
                    trust_level: merged_trust,
                }
            }
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
        }
    }

    /// Get the lower (less trusted) of two trust levels
    /// Untrusted < Derived < SemiTrusted (Untrusted is lowest/worst)
    const fn lower_trust(a: TrustLevel, b: TrustLevel) -> TrustLevel {
        match (a, b) {
            (TrustLevel::Untrusted, _) | (_, TrustLevel::Untrusted) => TrustLevel::Untrusted,
            (TrustLevel::Derived, _) | (_, TrustLevel::Derived) => TrustLevel::Derived,
            (TrustLevel::SemiTrusted, TrustLevel::SemiTrusted) => TrustLevel::SemiTrusted,
        }
    }

    /// Check if this state is tainted
    #[must_use]
    pub const fn is_tainted(&self) -> bool {
        matches!(self, Self::Tainted { .. })
    }

    /// Check if this state is clean
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        matches!(self, Self::Clean)
    }

    /// Get the trust level if tainted
    #[must_use]
    pub const fn trust_level(&self) -> Option<TrustLevel> {
        match self {
            Self::Tainted { trust_level, .. } => Some(*trust_level),
            Self::Clean | Self::Unknown => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dummy_location() -> Location {
        Location {
            file: PathBuf::from("test.java"),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 10,
        }
    }

    #[test]
    fn test_vuln_types_conversion() {
        assert!(
            VulnTypes::from_vuln_type(VulnType::SqlInjection).contains(VulnTypes::SQL_INJECTION)
        );
        assert!(VulnTypes::from_vuln_type(VulnType::CommandInjection)
            .contains(VulnTypes::COMMAND_INJECTION));
        assert!(VulnTypes::from_vuln_type(VulnType::HardcodedSecret).is_empty());
    }

    #[test]
    fn test_taint_state_creation() {
        let state = TaintState::tainted(
            "request.getParameter".to_string(),
            dummy_location(),
            "request.getParameter(\"id\")".to_string(),
            VulnTypes::all_vulns(),
        );

        assert!(state.is_tainted());
        assert!(state.is_dangerous_for(VulnType::SqlInjection));
        assert_eq!(state.source(), Some("request.getParameter"));
    }

    #[test]
    fn test_sanitization() {
        let state = TaintState::tainted(
            "user_input".to_string(),
            dummy_location(),
            "getUserInput()".to_string(),
            VulnTypes::SQL_INJECTION | VulnTypes::COMMAND_INJECTION,
        );

        // Sanitize SQL injection only
        let sanitized = state.sanitize(VulnTypes::SQL_INJECTION);
        assert!(!sanitized.is_dangerous_for(VulnType::SqlInjection));
        assert!(sanitized.is_dangerous_for(VulnType::CommandInjection));

        // Sanitize all
        let fully_sanitized = state.sanitize(VulnTypes::all_vulns());
        assert!(fully_sanitized.is_clean());
    }

    #[test]
    fn test_propagation() {
        let state = TaintState::tainted(
            "source".to_string(),
            dummy_location(),
            "getSource()".to_string(),
            VulnTypes::SQL_INJECTION,
        );

        let propagated = state.propagate(
            dummy_location(),
            "Assigned to variable".to_string(),
            "x = getSource()".to_string(),
        );

        assert!(propagated.is_tainted());
        assert_eq!(propagated.path().map(|p| p.len()), Some(2));
    }

    #[test]
    fn test_merge_states() {
        let tainted1 = TaintState::tainted(
            "source1".to_string(),
            dummy_location(),
            "src1()".to_string(),
            VulnTypes::SQL_INJECTION,
        );

        let tainted2 = TaintState::tainted(
            "source2".to_string(),
            dummy_location(),
            "src2()".to_string(),
            VulnTypes::COMMAND_INJECTION,
        );

        let merged = tainted1.merge(&tainted2);
        assert!(merged.is_dangerous_for(VulnType::SqlInjection));
        assert!(merged.is_dangerous_for(VulnType::CommandInjection));

        let clean_merge = TaintState::clean().merge(&tainted1);
        assert!(clean_merge.is_tainted());
    }

    #[test]
    fn test_trust_level_default() {
        // Default trust level should be Untrusted
        let state = TaintState::tainted(
            "request.getParameter".to_string(),
            dummy_location(),
            "request.getParameter(\"id\")".to_string(),
            VulnTypes::all_vulns(),
        );

        assert_eq!(state.trust_level(), Some(TrustLevel::Untrusted));
    }

    #[test]
    fn test_trust_level_with_explicit() {
        let state = TaintState::tainted_with_trust(
            "record.value".to_string(),
            dummy_location(),
            "record.value()".to_string(),
            VulnTypes::all_vulns(),
            TrustLevel::SemiTrusted,
        );

        assert_eq!(state.trust_level(), Some(TrustLevel::SemiTrusted));
    }

    #[test]
    fn test_trust_level_propagation() {
        // Trust level should be preserved through propagation
        let state = TaintState::tainted_with_trust(
            "record.value".to_string(),
            dummy_location(),
            "record.value()".to_string(),
            VulnTypes::all_vulns(),
            TrustLevel::SemiTrusted,
        );

        let propagated = state.propagate(
            dummy_location(),
            "Assigned to variable".to_string(),
            "x = record.value()".to_string(),
        );

        assert_eq!(propagated.trust_level(), Some(TrustLevel::SemiTrusted));
    }

    #[test]
    fn test_trust_level_merge_takes_lower() {
        // When merging, should take lower trust (Untrusted < Derived < SemiTrusted)
        let untrusted = TaintState::tainted_with_trust(
            "request.param".to_string(),
            dummy_location(),
            "request.getParameter()".to_string(),
            VulnTypes::all_vulns(),
            TrustLevel::Untrusted,
        );

        let semi_trusted = TaintState::tainted_with_trust(
            "record.value".to_string(),
            dummy_location(),
            "record.value()".to_string(),
            VulnTypes::all_vulns(),
            TrustLevel::SemiTrusted,
        );

        let merged = untrusted.merge(&semi_trusted);
        assert_eq!(merged.trust_level(), Some(TrustLevel::Untrusted));

        let merged_reverse = semi_trusted.merge(&untrusted);
        assert_eq!(merged_reverse.trust_level(), Some(TrustLevel::Untrusted));
    }

    #[test]
    fn test_trust_level_sanitization_preserves() {
        let state = TaintState::tainted_with_trust(
            "source".to_string(),
            dummy_location(),
            "source()".to_string(),
            VulnTypes::SQL_INJECTION | VulnTypes::LOG_INJECTION,
            TrustLevel::SemiTrusted,
        );

        let sanitized = state.sanitize(VulnTypes::SQL_INJECTION);
        assert!(sanitized.is_tainted());
        assert_eq!(sanitized.trust_level(), Some(TrustLevel::SemiTrusted));
    }
}
