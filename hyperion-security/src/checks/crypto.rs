// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Cryptography security checks

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use crate::scanner::method_call_pattern;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Check for ECB mode usage in ciphers
pub struct EcbModeCheck;

impl Check for EcbModeCheck {
    fn name(&self) -> &'static str {
        "ecb-mode"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        for call_node in method_call_pattern::find_method_calls(node) {
            let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) else {
                continue;
            };

            if pattern.receiver_name.as_deref() != Some("Cipher")
                || pattern.method_name != "getInstance"
            {
                continue;
            }

            // Extract algorithm argument from the call node
            if let Some(arg_list) = call_node
                .children()
                .find(|c| c.kind() == SyntaxKind::ARG_LIST)
            {
                for arg in arg_list.children() {
                    let arg_text = arg.text().to_string();

                    if arg_text.contains('"') {
                        let algo = arg_text.trim().trim_matches('"');

                        if algo.contains("/ECB/") {
                            return vec![Self::create_finding_explicit(node, context, algo)];
                        }

                        if !algo.contains('/') && (algo.contains("AES") || algo.contains("DES")) {
                            return vec![Self::create_finding_default(node, context, algo)];
                        }
                    }
                }
            }
        }

        vec![]
    }
}

impl EcbModeCheck {
    fn create_finding_explicit(node: &SyntaxNode, context: &CheckContext, algo: &str) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = format!(
            "Insecure ECB mode detected in cipher algorithm '{algo}'. ECB mode leaks patterns in plaintext. Use CBC or GCM mode instead."
        );

        let mut finding = Finding::new(VulnType::WeakEncryptionMode, message, location, code);
        finding.severity = Severity::High;
        finding.remediation =
            "Use AES/GCM/NoPadding or AES/CBC/PKCS5Padding with a unique IV for each encryption"
                .to_string();
        finding
    }

    fn create_finding_default(node: &SyntaxNode, context: &CheckContext, algo: &str) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = format!(
            "Cipher.getInstance(\"{algo}\") defaults to ECB mode. Explicitly specify a secure mode like GCM or CBC."
        );

        let mut finding = Finding::new(VulnType::WeakEncryptionMode, message, location, code);
        finding.severity = Severity::High;
        finding.remediation = "Use Cipher.getInstance(\"AES/GCM/NoPadding\") or \"AES/CBC/PKCS5Padding\" with unique IV".to_string();
        finding
    }
}

/// Check for hardcoded IVs using usage-based detection.
///
/// This check only flags byte arrays that are actually used in cryptographic contexts:
/// - `new IvParameterSpec(hardcodedBytes)`
/// - `new GCMParameterSpec(tagLen, hardcodedNonce)`
///
/// It does NOT flag:
/// - Test data arrays
/// - Protocol magic bytes (gzip header, etc.)
/// - HMAC iteration counters like `mac.doFinal(new byte[]{0, 0, 0, 1})`
/// - Short arrays (< 8 bytes) that are likely magic bytes
pub struct HardcodedIvCheck;

impl HardcodedIvCheck {
    /// Crypto parameter spec constructors that take an IV/nonce
    const CRYPTO_IV_CONSTRUCTORS: &'static [&'static str] = &[
        "IvParameterSpec",
        "GCMParameterSpec",
        "RC2ParameterSpec",
        "RC5ParameterSpec",
    ];

    /// Minimum byte array length to consider as a potential IV (most IVs are 12-16 bytes)
    const MIN_IV_LENGTH: usize = 8;

    /// Check if a byte array initializer has hardcoded values
    fn is_hardcoded_byte_array(node: &SyntaxNode) -> bool {
        let text = node.text().to_string();

        // Check for array initializer with values: new byte[]{1, 2, ...} or byte[]{...}
        if text.contains("byte[]") || text.contains("byte [") {
            // Must have array initializer syntax with at least one comma (multiple values)
            if (text.contains('{') && text.contains('}')) && text.contains(',') {
                return true;
            }
        }

        false
    }

    /// Count the number of elements in a byte array initializer
    fn count_array_elements(text: &str) -> usize {
        // Find the initializer part between { and }
        if let Some(start) = text.find('{') {
            if let Some(end) = text.rfind('}') {
                let initializer = &text[start + 1..end];
                // Count commas + 1 for number of elements (if non-empty)
                if initializer.trim().is_empty() {
                    return 0;
                }
                return initializer.matches(',').count() + 1;
            }
        }
        0
    }

    /// Check if a NEW_EXPR is a crypto IV/nonce constructor with hardcoded bytes
    fn check_crypto_constructor(node: &SyntaxNode, context: &CheckContext) -> Option<Finding> {
        // Extract constructor class name
        let mut class_name: Option<String> = None;
        let mut arg_list: Option<SyntaxNode> = None;

        for child in node.children() {
            match child.kind() {
                SyntaxKind::CLASS_TYPE => {
                    class_name = child
                        .descendants_with_tokens()
                        .filter_map(rowan::NodeOrToken::into_token)
                        .filter(|t| t.kind() == SyntaxKind::IDENT)
                        .last()
                        .map(|t| t.text().to_string());
                }
                SyntaxKind::ARG_LIST => {
                    arg_list = Some(child.clone());
                }
                _ => {}
            }
        }

        let class = class_name?;

        // Check if this is a crypto IV constructor
        if !Self::CRYPTO_IV_CONSTRUCTORS.contains(&class.as_str()) {
            return None;
        }

        let args = arg_list?;

        // For GCMParameterSpec, the nonce is the second argument
        // For IvParameterSpec, the IV is the first argument
        let iv_arg_index = usize::from(class == "GCMParameterSpec");

        let arg_nodes: Vec<_> = args.children().collect();
        let iv_arg = arg_nodes.get(iv_arg_index)?;

        // Check if the IV argument is a hardcoded byte array (inline)
        let iv_text = iv_arg.text().to_string();
        if Self::is_hardcoded_byte_array(iv_arg) {
            // Check minimum length
            let element_count = Self::count_array_elements(&iv_text);
            if element_count >= Self::MIN_IV_LENGTH {
                return Some(Self::create_finding(node, context, &class));
            }
        }

        // Check if the IV argument is a NAME_EXPR (variable reference)
        // We need to check if that variable was initialized with hardcoded bytes
        if iv_arg.kind() == SyntaxKind::NAME_EXPR {
            let var_name = iv_arg
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT)
                .map(|t| t.text().to_string())?;

            // Search for the variable declaration in the method body
            // We need to go up to find the containing method and search its body
            if Self::is_variable_hardcoded_iv(node, &var_name) {
                return Some(Self::create_finding(node, context, &class));
            }
        }

        None
    }

    /// Check if a variable is initialized with a hardcoded byte array
    fn is_variable_hardcoded_iv(node: &SyntaxNode, var_name: &str) -> bool {
        // Find the containing method body
        let Some(method_body) = Self::find_containing_method_body(node) else {
            return false;
        };

        // Search for LOCAL_VAR_DECL with the variable name
        for decl in method_body.descendants() {
            if decl.kind() == SyntaxKind::LOCAL_VAR_DECL {
                // Check if this declaration is for our variable
                let decl_text = decl.text().to_string();

                // Look for pattern: byte[] varName = new byte[]{...}
                // or: byte[] varName = {...}
                if decl_text.contains(var_name) && Self::is_hardcoded_byte_array(&decl) {
                    // Check minimum length
                    let element_count = Self::count_array_elements(&decl_text);
                    if element_count >= Self::MIN_IV_LENGTH {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Find the containing method body (BLOCK_STMT) for a node
    fn find_containing_method_body(node: &SyntaxNode) -> Option<SyntaxNode> {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == SyntaxKind::METHOD_DECL {
                // Find the BLOCK_STMT child (method body)
                for child in parent.children() {
                    if child.kind() == SyntaxKind::BLOCK_STMT {
                        return Some(child);
                    }
                }
                return None;
            }
            current = parent.parent();
        }
        None
    }

    fn create_finding(node: &SyntaxNode, context: &CheckContext, class: &str) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = format!(
            "Hardcoded IV/nonce detected in {class}. IVs and nonces must be unique and unpredictable for each encryption operation. A reused IV can compromise the security of the encryption."
        );

        let mut finding = Finding::new(VulnType::HardcodedCryptoIv, message, location, code);
        finding.severity = Severity::High;
        finding.remediation = "Generate a unique IV using SecureRandom:\n\
            byte[] iv = new byte[16]; // or 12 for GCM\n\
            SecureRandom.getInstanceStrong().nextBytes(iv);"
            .to_string();
        finding
    }
}

impl Check for HardcodedIvCheck {
    fn name(&self) -> &'static str {
        "hardcoded-iv"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        // Target NEW_EXPR to check crypto constructor calls
        &[SyntaxKind::NEW_EXPR]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        // Check if this is a crypto constructor with hardcoded IV
        if let Some(finding) = Self::check_crypto_constructor(node, context) {
            return vec![finding];
        }

        vec![]
    }
}

/// Check for weak hash algorithms
pub struct WeakHashCheck;

impl Check for WeakHashCheck {
    fn name(&self) -> &'static str {
        "weak-hash"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        for call_node in method_call_pattern::find_method_calls(node) {
            let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) else {
                continue;
            };

            // MessageDigest.getInstance()
            if pattern.method_name == "getInstance"
                && pattern.receiver_name.as_deref() == Some("MessageDigest")
            {
                if let Some(arg_list) = call_node
                    .children()
                    .find(|c| c.kind() == SyntaxKind::ARG_LIST)
                {
                    for arg in arg_list.children() {
                        let arg_text = arg.text().to_string();
                        if arg_text.contains("\"MD5\"")
                            || arg_text.contains("\"SHA1\"")
                            || arg_text.contains("\"SHA-1\"")
                        {
                            return vec![Self::create_finding(node, context, &arg_text)];
                        }
                    }
                }
            }

            // DigestUtils weak methods
            if pattern.receiver_name.as_deref() == Some("DigestUtils")
                && matches!(
                    pattern.method_name.as_str(),
                    "md5Hex" | "md5" | "sha1Hex" | "sha1"
                )
            {
                return vec![Self::create_finding_digest_utils(
                    node,
                    context,
                    &pattern.method_name,
                )];
            }
        }

        vec![]
    }
}

impl WeakHashCheck {
    fn is_protocol_required_hash(node: &SyntaxNode, source: &str) -> bool {
        let text = node.text().to_string().to_lowercase();
        let source_lower = source.to_lowercase();
        let markers = ["pwnedpasswords", "haveibeenpwned", "hibp", "k-anonymity"];

        markers
            .iter()
            .any(|marker| text.contains(marker) || source_lower.contains(marker))
    }

    fn create_finding(node: &SyntaxNode, context: &CheckContext, algo: &str) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let protocol_required = Self::is_protocol_required_hash(node, context.source);

        let message = if protocol_required {
            format!(
                "Weak hash algorithm {algo} detected, but context suggests a protocol requirement (e.g., HIBP). Document the requirement and avoid using SHA-1 for other security-sensitive use cases."
            )
        } else {
            format!(
                "Weak hash algorithm {algo} detected. MD5 and SHA-1 are cryptographically broken and should not be used for security purposes. Use SHA-256 or stronger."
            )
        };

        let mut finding = Finding::new(VulnType::WeakHashAlgorithm, message, location, code);
        finding.severity = if protocol_required {
            Severity::Info
        } else {
            Severity::Medium
        };
        if !protocol_required {
            finding.remediation = "Use MessageDigest.getInstance(\"SHA-256\") or \"SHA-512\" for security-sensitive hashing".to_string();
        }
        finding
    }

    fn create_finding_digest_utils(
        node: &SyntaxNode,
        context: &CheckContext,
        method: &str,
    ) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let protocol_required = Self::is_protocol_required_hash(node, context.source);

        let message = if protocol_required {
            format!(
                "DigestUtils.{method}() uses a weak hash algorithm, but context suggests a protocol requirement (e.g., HIBP). Document the requirement and avoid SHA-1 elsewhere."
            )
        } else {
            format!(
                "DigestUtils.{method}() uses a weak hash algorithm. Use sha256Hex() or sha512Hex() instead."
            )
        };

        let mut finding = Finding::new(VulnType::WeakHashAlgorithm, message, location, code);
        finding.severity = if protocol_required {
            Severity::Info
        } else {
            Severity::Medium
        };
        finding
    }
}

/// Check for small key sizes
pub struct SmallKeySizeCheck;

impl Check for SmallKeySizeCheck {
    fn name(&self) -> &'static str {
        "small-key-size"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let rsa_family = Self::uses_rsa_family_key(node);
        for call_node in method_call_pattern::find_method_calls(node) {
            let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) else {
                continue;
            };

            // KeyGenerator.init() or KeyPairGenerator.initialize()
            if pattern.method_name != "init" && pattern.method_name != "initialize" {
                continue;
            }

            // Extract numeric argument
            if let Some(arg_list) = call_node
                .children()
                .find(|c| c.kind() == SyntaxKind::ARG_LIST)
            {
                for arg in arg_list.children() {
                    let arg_text = arg.text().to_string().trim().to_string();
                    if let Ok(key_size) = arg_text.parse::<u32>() {
                        if key_size < 128 {
                            return vec![Self::create_finding_very_weak(node, context, key_size)];
                        } else if key_size < 256 {
                            return vec![Self::create_finding_weak(node, context, key_size)];
                        } else if (1024..2048).contains(&key_size)
                            || (rsa_family && (256..1024).contains(&key_size))
                        {
                            return vec![Self::create_finding_rsa(node, context, key_size)];
                        }
                    }
                }
            }
        }

        vec![]
    }
}

impl SmallKeySizeCheck {
    /// Whether the enclosing method asks for an RSA-family key generator, as in
    /// `KeyPairGenerator.getInstance("RSA")`. Only those keys are weak at 256 to 1023 bits.
    /// A 256-bit AES or EC key is normal, and the size is often set in a later statement
    /// of the same method, so the whole method is searched.
    fn uses_rsa_family_key(node: &SyntaxNode) -> bool {
        let mut scope = node.text().to_string();
        let mut current = node.parent();
        while let Some(ancestor) = current {
            if ancestor.kind() == SyntaxKind::METHOD_DECL {
                scope = ancestor.text().to_string();
                break;
            }
            current = ancestor.parent();
        }
        ["RSA", "DSA", "DH", "DiffieHellman"]
            .iter()
            .any(|algorithm| scope.contains(&format!("getInstance(\"{algorithm}\"")))
    }

    fn create_finding_very_weak(node: &SyntaxNode, context: &CheckContext, size: u32) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = format!(
            "Very small key size {size} detected. This is cryptographically weak. Use at least 256 bits for symmetric encryption."
        );

        let mut finding = Finding::new(VulnType::SmallKeySize, message, location, code);
        finding.severity = Severity::High;
        finding.remediation = "Use key size of at least 256 bits (AES-256)".to_string();
        finding
    }

    fn create_finding_weak(node: &SyntaxNode, context: &CheckContext, size: u32) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = format!(
            "Small key size {size} detected. While not critically weak, 256 bits is recommended for symmetric encryption."
        );

        let mut finding = Finding::new(VulnType::SmallKeySize, message, location, code);
        finding.severity = Severity::Medium;
        finding.remediation = "Use key size of 256 bits (AES-256) for better security".to_string();
        finding
    }

    fn create_finding_rsa(node: &SyntaxNode, context: &CheckContext, size: u32) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = format!(
            "RSA key size {size} is too small. Use at least 2048 bits, preferably 4096 bits."
        );

        let mut finding = Finding::new(VulnType::SmallKeySize, message, location, code);
        finding.severity = Severity::High;
        finding.remediation =
            "Use RSA key size of at least 2048 bits, preferably 4096 bits".to_string();
        finding
    }
}

/// Check for insecure random number generation
pub struct InsecureRandomCheck;

impl Check for InsecureRandomCheck {
    fn name(&self) -> &'static str {
        "insecure-random"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
            SyntaxKind::NEW_EXPR,
            SyntaxKind::METHOD_CALL_EXPR,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();

        // Skip demo/test/seeder contexts
        let class_name = crate::context::get_containing_class_name(node).unwrap_or_default();
        let method_name = crate::context::get_containing_method_name(node);

        if crate::context::is_demo_or_test_context(
            context.file,
            &class_name,
            method_name.as_deref(),
        ) {
            // Don't report for demo/test code - Random is fine for fake data generation
            return vec![];
        }

        // Check for new Random() or java.util.Random in security contexts
        if node_text.contains("new Random()") || node_text.contains("new java.util.Random()") {
            // Check if this is in a security-sensitive context
            if Self::is_security_context(&node_text, node) {
                return vec![Self::create_random_finding(node, context)];
            }
        }

        // Check for Math.random() in security contexts
        if node_text.contains("Math.random()") && Self::is_security_context(&node_text, node) {
            return vec![Self::create_math_random_finding(node, context)];
        }

        vec![]
    }
}

impl InsecureRandomCheck {
    /// Patterns that indicate security-sensitive usage
    const SECURITY_PATTERNS: &'static [&'static str] = &[
        "password", "token", "secret", "key", "salt", "nonce", "iv", "session", "csrf", "otp",
        "code", "verify", "auth",
    ];

    /// Check if the context suggests security-sensitive usage
    fn is_security_context(text: &str, node: &SyntaxNode) -> bool {
        let lower = text.to_lowercase();

        // Check for security-related patterns in the immediate context
        if Self::SECURITY_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern))
        {
            return true;
        }

        // Check variable names in the containing method
        if let Some(method) = Self::find_containing_method(node) {
            let method_text = method.text().to_string().to_lowercase();
            if Self::SECURITY_PATTERNS
                .iter()
                .any(|pattern| method_text.contains(pattern))
            {
                return true;
            }
        }

        false
    }

    /// Find the containing method declaration
    fn find_containing_method(node: &SyntaxNode) -> Option<SyntaxNode> {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == SyntaxKind::METHOD_DECL {
                return Some(parent);
            }
            current = parent.parent();
        }
        None
    }

    fn create_random_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "Insecure random number generator: java.util.Random is not cryptographically secure. \
            It uses a predictable seed and linear congruential algorithm that can be broken with observed output. \
            For security-sensitive operations (tokens, passwords, session IDs), use SecureRandom."
            .to_string();

        let mut finding = Finding::new(VulnType::InsecureRandom, message, location, code);
        finding.severity = Severity::High;
        finding.remediation = "Use SecureRandom for security-sensitive random values:\n\
            SecureRandom random = SecureRandom.getInstanceStrong();\n\
            byte[] token = new byte[32];\n\
            random.nextBytes(token);"
            .to_string();
        finding
    }

    fn create_math_random_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "Insecure random number generator: Math.random() is not cryptographically secure. \
            It is based on java.util.Random and is completely predictable. Never use it for security-sensitive \
            operations like generating tokens, passwords, or session IDs."
            .to_string();

        let mut finding = Finding::new(VulnType::InsecureRandom, message, location, code);
        finding.severity = Severity::High;
        finding.remediation = "Use SecureRandom for security-sensitive random values:\n\
            SecureRandom random = SecureRandom.getInstanceStrong();\n\
            byte[] token = new byte[32];\n\
            random.nextBytes(token);"
            .to_string();
        finding
    }
}

/// Check for insecure TLS configuration
pub struct InsecureTlsCheck;

impl Check for InsecureTlsCheck {
    fn name(&self) -> &'static str {
        "insecure-tls"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
            SyntaxKind::LAMBDA_EXPR,
            SyntaxKind::METHOD_DECL,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();

        // Check for SSLContext.getInstance() with weak protocols
        if node_text.contains("SSLContext")
            && node_text.contains("getInstance")
            && (node_text.contains("\"SSL\"")
                || node_text.contains("\"SSLv3\"")
                || node_text.contains("\"TLSv1\"")
                || node_text.contains("\"TLSv1.1\"")
                || (node_text.contains("\"TLS\"")
                    && !node_text.contains("\"TLSv1.2\"")
                    && !node_text.contains("\"TLSv1.3\"")))
        {
            return vec![Self::create_weak_protocol_finding(node, context)];
        }

        // Check for empty checkServerTrusted (trust all certs)
        if node_text.contains("checkServerTrusted") {
            // Look for empty method body: {}
            if node_text.contains("{}") || node_text.contains("{ }") {
                return vec![Self::create_trust_all_finding(node, context)];
            }
            // Also check for single-line empty body
            if node_text.matches('{').count() == 1 && node_text.matches('}').count() == 1 {
                let body = node_text
                    .split('{')
                    .nth(1)
                    .and_then(|s| s.split('}').next())
                    .unwrap_or("");
                if body.trim().is_empty() {
                    return vec![Self::create_trust_all_finding(node, context)];
                }
            }
        }

        // Check for HostnameVerifier returning true
        if node_text.contains("HostnameVerifier")
            && (node_text.contains("-> true") || node_text.contains("return true"))
        {
            return vec![Self::create_hostname_verify_finding(node, context)];
        }

        vec![]
    }
}

impl InsecureTlsCheck {
    fn create_weak_protocol_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message =
            "Weak TLS protocol detected. SSLv3, TLSv1, and TLSv1.1 are deprecated and insecure. Use TLSv1.2 or TLSv1.3."
                .to_string();

        let mut finding = Finding::new(VulnType::InsecureTls, message, location, code);
        finding.severity = Severity::Critical;
        finding.remediation = "Use SSLContext.getInstance(\"TLSv1.2\") or \"TLSv1.3\"".to_string();
        finding
    }

    fn create_trust_all_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "TrustManager with empty checkServerTrusted() trusts all certificates, including invalid ones. This defeats HTTPS security and enables man-in-the-middle attacks.".to_string();

        let mut finding = Finding::new(VulnType::InsecureTls, message, location, code);
        finding.severity = Severity::Critical;
        finding.remediation =
            "Implement proper certificate validation or use the default TrustManager".to_string();
        finding
    }

    fn create_hostname_verify_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "HostnameVerifier that always returns true disables hostname verification. This enables man-in-the-middle attacks.".to_string();

        let mut finding = Finding::new(VulnType::InsecureTls, message, location, code);
        finding.severity = Severity::Critical;
        finding.remediation =
            "Implement proper hostname verification or use the default HostnameVerifier"
                .to_string();
        finding
    }
}
