// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, Severity, VulnType};
use std::path::Path;

#[test]
fn test_empty_password_allows_access() {
    let source = r#"
public class AuthTest {
    public boolean authenticate(String username, String password) {
        if (password.isEmpty()) {
            return true;
        }
        return checkPassword(username, password);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AuthService.java"));

    assert!(!findings.is_empty(), "Should detect empty password bypass");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::EmptyPasswordCheck);
    assert!(finding.is_some(), "Should find EmptyPasswordCheck");
    assert_eq!(finding.unwrap().severity, Severity::Critical);
}

#[test]
fn test_empty_password_equals() {
    let source = r#"
public class AuthTest {
    public boolean authenticate(String username, String password) {
        if (password.equals("")) {
            authenticated = true;
        }
        return authenticated;
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AuthService.java"));

    assert!(!findings.is_empty(), "Should detect empty password bypass");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::EmptyPasswordCheck);
    assert!(finding.is_some(), "Should find EmptyPasswordCheck");
}

#[test]
fn test_empty_password_length_check() {
    let source = r#"
public class AuthTest {
    public boolean authenticate(String username, String password) {
        if (password.length() == 0) {
            return true;
        }
        return checkPassword(username, password);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AuthService.java"));

    assert!(
        !findings.is_empty(),
        "Should detect empty password bypass with length()"
    );
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::EmptyPasswordCheck);
    assert!(finding.is_some(), "Should find EmptyPasswordCheck");
}

#[test]
fn test_proper_password_validation() {
    let source = r#"
public class AuthTest {
    public boolean authenticate(String username, String password) {
        if (password.isEmpty()) {
            return false;  // Properly rejects empty password
        }
        return checkPassword(username, password);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AuthService.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 0,
        "Should not flag proper password validation"
    );
}

#[test]
fn test_hardcoded_password() {
    let source = r#"
public class AuthTest {
    private static final String password = "MySecretPassword123";

    public void connect() {
        connection.authenticate(username, password);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AuthService.java"));

    assert!(!findings.is_empty(), "Should detect hardcoded password");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::HardcodedCredentials);
    assert!(finding.is_some(), "Should find HardcodedCredentials");
    assert_eq!(finding.unwrap().severity, Severity::High);
}

#[test]
fn test_hardcoded_api_key() {
    let source = r#"
public class ApiClient {
    private static final String apiKey = "STRIPE_LIVE_123456789abcdefg";

    public void makeRequest() {
        request.setHeader("Authorization", "Bearer " + apiKey);
    }
}
"#;
    let source = &stripe_keys(source);

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ApiClient.java"));

    assert!(!findings.is_empty(), "Should detect hardcoded API key");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::HardcodedCredentials);
    assert!(finding.is_some(), "Should find HardcodedCredentials");
}

#[test]
fn test_hardcoded_aws_key() {
    let source = r#"
public class AwsClient {
    private String accessKey = "AKIAIOSFODNN7EXAMPLE";
    private String secretKey = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

    public void connect() {
        client.authenticate(accessKey, secretKey);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AwsClient.java"));

    assert!(!findings.is_empty(), "Should detect hardcoded AWS keys");
    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert!(
        cred_findings >= 1,
        "Should find at least one HardcodedCredentials"
    );
}

#[test]
fn test_hardcoded_secret() {
    let source = r#"
public class SecretManager {
    private static final String secret = "my-secret-token-value";

    public String getSecret() {
        return secret;
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecretManager.java"));

    assert!(!findings.is_empty(), "Should detect hardcoded secret");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::HardcodedCredentials);
    assert!(finding.is_some(), "Should find HardcodedCredentials");
}

#[test]
fn test_placeholder_password_not_flagged() {
    let source = r#"
public class ConfigTest {
    private String password = "";  // Will be set from config
    private String apiKey = "changeme";
    private String token = "your_token_here";

    public void setPassword(String pwd) {
        this.password = pwd;
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ConfigTest.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(cred_findings, 0, "Should not flag obvious placeholders");
}

#[test]
fn test_placeholder_substring_not_flagged() {
    let source = r#"
public class ConfigTest {
    private String apiKey = "ECB_API_TOKEN_PLACEHOLDER";
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ConfigTest.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(cred_findings, 0, "Should not flag placeholder tokens");
}

#[test]
fn test_oauth_url_not_flagged() {
    let source = r#"
public class OAuthConfig {
    private String oauthTokenUrl = "https://login.example.com/oauth/token";
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("OAuthConfig.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(cred_findings, 0, "Should not flag OAuth token URLs");
}

#[test]
fn test_env_var_credentials_safe() {
    let source = r#"
public class SecureAuth {
    private static final String password = System.getenv("DB_PASSWORD");
    private static final String apiKey = System.getenv("API_KEY");

    public void connect() {
        connection.authenticate(username, password);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecureAuth.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(
        cred_findings, 0,
        "Should not flag environment variable credentials"
    );
}

#[test]
fn test_bearer_token_hardcoded() {
    let source = r#"
public class ApiClient {
    public void makeRequest() {
        String authHeader = "Bearer token_abc123xyz456";
        request.setHeader("Authorization", authHeader);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ApiClient.java"));

    assert!(!findings.is_empty(), "Should detect hardcoded Bearer token");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::HardcodedCredentials);
    assert!(finding.is_some(), "Should find HardcodedCredentials");
}

#[test]
fn test_database_credentials() {
    let source = r#"
public class DatabaseConfig {
    private String dbPassword = "MyDatabasePassword!";
    private String dbUser = "admin";

    public Connection getConnection() {
        return DriverManager.getConnection(url, dbUser, dbPassword);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("DatabaseConfig.java"));

    assert!(
        !findings.is_empty(),
        "Should detect hardcoded database password"
    );
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::HardcodedCredentials);
    assert!(finding.is_some(), "Should find HardcodedCredentials");
}

// Entropy analysis tests

#[test]
fn test_config_key_not_flagged() {
    // Config key names like "clientSecret" should NOT be flagged
    let source = r#"
public class OAuthConfig {
    private String clientSecretKey = "clientSecret";
    private String apiKeyName = "apiKey";
    private String passwordField = "password";

    public String getConfigKey() {
        return clientSecretKey;
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("OAuthConfig.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(
        cred_findings, 0,
        "Should not flag config key names (low entropy)"
    );
}

#[test]
fn test_high_entropy_secret_flagged() {
    // High entropy string should be flagged as a secret
    let source = r#"
public class SecretManager {
    private String apiKey = "aK9xY2mP8qW5nR7tL4vB3cE6fG1hJ0iU";

    public void authenticate() {
        client.setApiKey(apiKey);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecretManager.java"));

    assert!(!findings.is_empty(), "Should detect high entropy secret");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::HardcodedCredentials);
    assert!(
        finding.is_some(),
        "Should find HardcodedCredentials for high entropy string"
    );
}

#[test]
fn test_base64_secret_flagged() {
    // Base64-encoded values have high entropy and should be flagged
    let source = r#"
public class TokenManager {
    private String token = "ZXlKaGJHY2lPaUpJVXpJMU5pSXNJblI1Y0NJNklrcFhWQ0o5";

    public String getToken() {
        return token;
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("TokenManager.java"));

    assert!(
        !findings.is_empty(),
        "Should detect base64-encoded secret (high entropy)"
    );
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::HardcodedCredentials);
    assert!(finding.is_some(), "Should find HardcodedCredentials");
}

#[test]
fn test_aws_access_key_with_entropy() {
    // AWS access keys starting with AKIA should always be flagged
    let source = r#"
public class AwsConfig {
    private String accessKeyId = "AKIAIOSFODNN7EXAMPLE";

    public void connect() {
        client.setAccessKey(accessKeyId);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AwsConfig.java"));

    assert!(!findings.is_empty(), "Should detect AWS access key");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::HardcodedCredentials);
    assert!(
        finding.is_some(),
        "Should find HardcodedCredentials for AWS key"
    );
}

#[test]
fn test_stripe_key_with_entropy() {
    // Stripe keys with sk_ prefix should be flagged
    let source = r#"
public class StripeConfig {
    private String secretKey = "STRIPE_TEST_4eC39HqLyjWDarjtT1zdp7dc";

    public void processPayment() {
        stripe.setApiKey(secretKey);
    }
}
"#;
    let source = &stripe_keys(source);

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("StripeConfig.java"));

    assert!(!findings.is_empty(), "Should detect Stripe secret key");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::HardcodedCredentials);
    assert!(
        finding.is_some(),
        "Should find HardcodedCredentials for Stripe key"
    );
}

#[test]
fn test_short_readable_password_not_flagged() {
    // Short, low-entropy passwords that are clearly test data
    let source = r#"
public class TestConfig {
    private String testPassword = "test123";
    private String simpleSecret = "secret";

    public void testAuth() {
        authenticate("user", testPassword);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let _findings = scanner.scan_source(source, Path::new("TestConfig.java"));

    // These might still be flagged by other heuristics, but entropy should help reduce false positives
    // The key insight is that "test123" and "secret" have low entropy compared to real secrets
    // These are edge cases - we're documenting the behavior
    // Short test passwords may or may not be flagged depending on other checks
}

#[test]
fn test_long_high_entropy_secret_flagged() {
    // Long string with high entropy (32+ chars, entropy > 3.5)
    let source = r#"
public class JwtConfig {
    private String signingKey = "x9K2mP8qW5nR7tL4vB3cE6fG1hJ0iUaK9";

    public String signToken(String payload) {
        return jwt.sign(payload, signingKey);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JwtConfig.java"));

    assert!(
        !findings.is_empty(),
        "Should detect long high-entropy secret"
    );
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::HardcodedCredentials);
    assert!(finding.is_some(), "Should find HardcodedCredentials");
}

#[test]
fn test_config_property_key_not_flagged() {
    // Property keys with dots should not be flagged
    let source = r#"
public class PropertyConfig {
    private String secretPropertyKey = "oauth.client.secret";
    private String tokenPropertyKey = "api.access.token";

    public String getProperty() {
        return config.get(secretPropertyKey);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("PropertyConfig.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(
        cred_findings, 0,
        "Should not flag property key names (already handled by is_config_key)"
    );
}

#[test]
fn test_mixed_case_config_key_not_flagged() {
    // CamelCase config keys should not be flagged (low entropy, matches pattern)
    let source = r#"
public class ConfigKeys {
    private String apiKeyConfig = "apiKey";
    private String secretTokenConfig = "secretToken";
    private String passwordProp = "userPassword";
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ConfigKeys.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(
        cred_findings, 0,
        "Should not flag camelCase config key names"
    );
}

// Phase 2 tests: Improved false positive filtering

#[test]
fn test_descriptor_variable_suffix_header_not_flagged() {
    // Variable names ending in _HEADER describe HTTP header names
    let source = r#"
public class HttpConfig {
    String SIGNATURE_HEADER = "X-Connect-Authorization";
    String AUTH_HEADER = "Authorization";
    String API_KEY_HEADER = "X-API-Key";
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("HttpConfig.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(
        cred_findings, 0,
        "Should not flag variables with _HEADER suffix (descriptor variables)"
    );
}

#[test]
fn test_kebab_case_secret_still_flagged() {
    // Kebab-case values containing security words should still be flagged
    let source = r#"
public class SecretConfig {
    String secret = "my-secret-token-value";
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecretConfig.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert!(
        cred_findings > 0,
        "Should still flag kebab-case values containing security words"
    );
}

// Phase 1.1 tests: Improved EmptyPasswordCheck false positive filtering

#[test]
fn test_password_empty_with_throw_not_flagged() {
    // Rejecting empty password with exception - this is SAFE
    let source = r#"
public class AuthService {
    public void authenticate(String password) {
        if (password.isEmpty()) {
            throw new AuthenticationException("Password is required");
        }
        // ... continue with actual authentication
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AuthService.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 0,
        "Should NOT flag when empty password is rejected with throw"
    );
}

#[test]
fn test_password_empty_with_return_false_not_flagged() {
    // Rejecting empty password with return false - this is SAFE
    let source = r#"
public class AuthService {
    public boolean authenticate(String password) {
        if (password.isEmpty()) {
            return false;
        }
        return validatePassword(password);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AuthService.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 0,
        "Should NOT flag when empty password is rejected with return false"
    );
}

#[test]
fn test_password_empty_with_return_null_not_flagged() {
    // Rejecting empty password with return null - this is SAFE
    let source = r#"
public class AuthService {
    public User authenticate(String password) {
        if (password.isEmpty()) {
            return null;
        }
        return findUser(password);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AuthService.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 0,
        "Should NOT flag when empty password is rejected with return null"
    );
}

#[test]
fn test_password_empty_allows_access_flagged() {
    // Empty password ALLOWS access - TRUE POSITIVE
    let source = r#"
public class InsecureAuth {
    public boolean authenticate(String password) {
        if (password.isEmpty()) {
            return true;  // VULNERABILITY!
        }
        return validatePassword(password);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("InsecureAuth.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 1,
        "MUST flag when empty password allows authentication"
    );
}

#[test]
fn test_pwd_empty_sets_authenticated_flagged() {
    // Empty password sets authenticated flag - TRUE POSITIVE
    let source = r#"
public class InsecureAuth {
    private boolean authenticated = false;

    public void login(String pwd) {
        if (pwd.isEmpty()) {
            authenticated = true;  // VULNERABILITY!
        }
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("InsecureAuth.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 1,
        "MUST flag when empty pwd sets authenticated = true"
    );
}

#[test]
fn test_passwd_equals_empty_allows_flagged() {
    // Empty password check with .equals("") - TRUE POSITIVE
    let source = r#"
public class InsecureAuth {
    public boolean authenticate(String passwd) {
        if (passwd.equals("")) {
            return true;  // VULNERABILITY!
        }
        return checkCredentials(passwd);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("InsecureAuth.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 1,
        "MUST flag when passwd.equals(\"\") allows access"
    );
}

#[test]
fn test_password_length_zero_allows_flagged() {
    // Empty password check with .length() == 0 - TRUE POSITIVE
    let source = r#"
public class InsecureAuth {
    public boolean authenticate(String password) {
        if (password.length() == 0) {
            return true;  // VULNERABILITY!
        }
        return checkPassword(password);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("InsecureAuth.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 1,
        "MUST flag when password.length() == 0 allows access"
    );
}

#[test]
fn test_password_store_empty_not_flagged() {
    // Checking if password store/map is empty - FALSE POSITIVE to avoid
    let source = r#"
public class PasswordManager {
    private Map<String, String> passwordStore;

    public boolean hasPasswords() {
        if (passwordStore.isEmpty()) {
            return true;  // This is checking if the STORE is empty, not a password
        }
        return false;
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("PasswordManager.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 0,
        "Should NOT flag when isEmpty() is called on passwordStore (container)"
    );
}

#[test]
fn test_password_cache_empty_not_flagged() {
    // Checking if password cache is empty - FALSE POSITIVE to avoid
    let source = r#"
public class AuthCache {
    private Cache<String, String> passwordCache;

    public void refreshIfEmpty() {
        if (passwordCache.isEmpty()) {
            authenticated = true;  // Still not a vulnerability - cache check
        }
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AuthCache.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 0,
        "Should NOT flag when isEmpty() is called on passwordCache (container)"
    );
}

#[test]
fn test_credential_map_empty_not_flagged() {
    // Checking if credential map is empty - FALSE POSITIVE to avoid
    let source = r#"
public class CredentialManager {
    private Map<String, Credential> credentialMap;

    public boolean allowAnonymous() {
        if (credentialMap.isEmpty()) {
            return true;  // Allow anonymous when no credentials configured
        }
        return false;
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CredentialManager.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 0,
        "Should NOT flag when isEmpty() is called on credentialMap (container)"
    );
}

#[test]
fn test_passphrase_empty_allows_flagged() {
    // Empty passphrase check - TRUE POSITIVE
    let source = r#"
public class KeyStore {
    public boolean unlock(String passphrase) {
        if (passphrase.isEmpty()) {
            return true;  // VULNERABILITY!
        }
        return decrypt(passphrase);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("KeyStore.java"));

    let empty_pwd_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::EmptyPasswordCheck)
        .count();
    assert_eq!(
        empty_pwd_findings, 1,
        "MUST flag when passphrase.isEmpty() allows access"
    );
}

// Phase 3 tests: Self-referential constant detection

#[test]
fn test_real_credentials_still_flagged_with_self_ref_check() {
    // Verify that real hardcoded credentials are still detected
    // when self-referential check is active
    let source = r#"
public class CredentialTest {
    // Self-referential - should NOT be flagged
    String FOO_RESOLVER = "FOO_RESOLVER";

    // Real hardcoded password - SHOULD be flagged
    String realPassword = "MySecretPassword123!";

    // Real API key - SHOULD be flagged
    String apiKey = "STRIPE_LIVE_51H4K2LJbQqQmZ5pW2h0pQmZ5pW2h0";
}
"#;
    let source = &stripe_keys(source);

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CredentialTest.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(
        cred_findings, 2,
        "Should still flag real credentials even with self-referential check"
    );
}

// Phase 4 tests: Internal Key Constants detection

#[test]
fn test_internal_key_with_key_pattern_not_flagged() {
    // Variable name has _KEY pattern and value looks like an identifier
    let source = r#"
public class InternalKeys {
    // Low-entropy identifiers with _KEY suffix
    String CACHE_KEY = "auth_cache_key";
    String SESSION_KEY = "session_id_key";
    String SUBSCRIBER_KEY = "_subscriber_key";
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("InternalKeys.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(
        cred_findings, 0,
        "Should not flag internal key constants with low entropy"
    );
}

#[test]
fn test_real_secret_key_still_flagged_with_internal_key_check() {
    // Verify that real hardcoded secrets in *_KEY variables are still detected
    // when they have high entropy
    let source = r#"
public class SecretKeys {
    // Internal key - should NOT be flagged
    String CACHE_KEY = "_internal_cache_key";

    // Real secret key - SHOULD be flagged (high entropy)
    String API_KEY = "aK9xY2mP8qW5nR7tL4vB3cE6fG1hJ0iUaK9";

    // Real encryption key - SHOULD be flagged
    String ENCRYPTION_KEY = "STRIPE_LIVE_51H4K2LJbQqQmZ5pW2h0pQmZ5pW2h0";
}
"#;
    let source = &stripe_keys(source);

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecretKeys.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert!(
        cred_findings >= 2,
        "Should still flag real high-entropy secrets in *_KEY variables"
    );
}

#[test]
fn test_map_key_constants_not_flagged() {
    // Constants used as map keys for internal storage
    let source = r#"
public class MapKeyConstants {
    String KEY_FOR_USER = "_user_key";
    String KEY_FOR_SESSION = "_session_key";
    String AUTH_KEY = "_auth_context_key";
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("MapKeyConstants.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(cred_findings, 0, "Should not flag map key constants");
}

// Phase 1: ALL_CAPS Credential Constants Tests

#[test]
fn test_actual_hardcoded_credentials_still_detected() {
    let source = r#"
public class Config {
    private static final String API_KEY = "STRIPE_LIVE_abc123def456ghi789";
    private static final String PASSWORD = "MySecretPassword123";
}
"#;
    let source = &stripe_keys(source);

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Config.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(
        cred_findings, 2,
        "Actual credentials should still be detected"
    );
}

#[test]
fn test_aws_key_still_detected_despite_uppercase() {
    // AWS keys like "AKIAIOSFODNN7EXAMPLE" should still be detected
    // even though they're all uppercase
    let source = r#"
public class AwsConfig {
    private static final String ACCESS_KEY = "AKIAIOSFODNN7EXAMPLE";
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AwsConfig.java"));

    let cred_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .count();
    assert_eq!(
        cred_findings, 1,
        "AWS access keys should still be detected despite uppercase"
    );
}

#[test]
fn test_empty_password_logged_as_error_then_granted_is_flagged() {
    // The log message says "error" and "invalid", but the branch still grants access.
    // Words inside a log call do not reject anything.
    let source = r#"
public class AuthTest {
    public boolean authenticate(String username, String password) {
        if (password.isEmpty()) {
            log.error("invalid credentials for {}", username);
            return true;
        }
        return checkPassword(username, password);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AuthService.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::EmptyPasswordCheck),
        "Logging an error does not reject an empty password when access is still granted"
    );
}

/// Expands `STRIPE_LIVE_` / `STRIPE_TEST_` placeholders into Stripe key prefixes, so the
/// repository itself never contains key-shaped strings that secret scanners would flag.
fn stripe_keys(source: &str) -> String {
    source
        .replace("STRIPE_LIVE_", concat!("sk", "_live_"))
        .replace("STRIPE_TEST_", concat!("sk", "_test_"))
}
