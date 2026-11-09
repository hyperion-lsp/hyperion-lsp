// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for framework-specific security checks (Phase 4)

use hyperion_security::{ScanConfig, SecurityScanner, Severity, VulnType};
use std::path::Path;

// ========== Spring Security Tests ==========

#[test]
fn test_csrf_disabled_method_chaining() {
    let source = r#"
@Configuration
public class SecurityConfig extends WebSecurityConfigurerAdapter {
    @Override
    protected void configure(HttpSecurity http) throws Exception {
        http
            .authorizeRequests()
            .anyRequest().authenticated()
            .and()
            .csrf().disable();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should detect CSRF disabled with method chaining"
    );
    assert_eq!(
        findings
            .iter()
            .find(|f| f.vuln_type == VulnType::CsrfDisabled)
            .unwrap()
            .severity,
        Severity::High
    );
}

#[test]
fn test_csrf_disabled_lambda_style() {
    let source = r#"
@Configuration
public class SecurityConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) throws Exception {
        http
            .csrf(csrf -> csrf.disable())
            .authorizeHttpRequests(auth -> auth.anyRequest().authenticated());
        return http.build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should detect CSRF disabled in lambda style"
    );
}

#[test]
fn test_csrf_disabled_method_reference() {
    let source = r#"
public class SecurityConfig {
    public void configure() {
        http.csrf(AbstractHttpConfigurer::disable)
            .authorizeHttpRequests(auth -> auth.anyRequest().authenticated());
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should detect CSRF disabled with method reference"
    );
}

#[test]
fn test_csrf_enabled_no_finding() {
    let source = r#"
@Configuration
public class SecurityConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) throws Exception {
        http
            .csrf(csrf -> csrf.csrfTokenRepository(CookieCsrfTokenRepository.withHttpOnlyFalse()))
            .authorizeHttpRequests(auth -> auth.anyRequest().authenticated());
        return http.build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should not flag CSRF when it's configured but not disabled"
    );
}

// ========== CSRF False Positive Reduction - JWT/Stateless API Detection ==========

#[test]
fn test_csrf_disabled_stateless_jwt_api() {
    let source = r#"
@Configuration
public class SecurityConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) {
        return http
            .sessionManagement(s -> s.sessionCreationPolicy(SessionCreationPolicy.STATELESS))
            .csrf(csrf -> csrf.disable())
            .addFilterBefore(jwtAuthenticationFilter, UsernamePasswordAuthenticationFilter.class)
            .build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should NOT flag CSRF disabled in stateless JWT API"
    );
}

#[test]
fn test_csrf_disabled_stateless_api_key() {
    let source = r#"
@Configuration
public class ApiSecurityConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) {
        return http
            .sessionManagement().sessionCreationPolicy(STATELESS)
            .and()
            .csrf().disable()
            .addFilterBefore(apiKeyFilter, UsernamePasswordAuthenticationFilter.class)
            .build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ApiSecurityConfig.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should NOT flag CSRF disabled in stateless API key authentication"
    );
}

#[test]
fn test_csrf_disabled_bearer_token_auth() {
    let source = r#"
@Configuration
public class ResourceServerConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) {
        return http
            .sessionManagement(s -> s.sessionCreationPolicy(SessionCreationPolicy.STATELESS))
            .csrf(csrf -> csrf.disable())
            .oauth2ResourceServer(oauth2 -> oauth2.jwt())
            .build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ResourceServerConfig.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should NOT flag CSRF disabled in OAuth2 resource server with JWT"
    );
}

#[test]
fn test_csrf_disabled_jwt_decoder() {
    let source = r#"
@Configuration
public class JwtSecurityConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) {
        return http
            .sessionManagement(s -> s.sessionCreationPolicy(STATELESS))
            .csrf().disable()
            .addFilterBefore(new JwtAuthenticationFilter(jwtDecoder()), UsernamePasswordAuthenticationFilter.class)
            .build();
    }

    @Bean
    public JwtDecoder jwtDecoder() {
        return JwtDecoders.fromIssuerLocation("https://auth.example.com");
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JwtSecurityConfig.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should NOT flag CSRF disabled when using JwtDecoder"
    );
}

#[test]
fn test_csrf_disabled_session_based_app() {
    let source = r#"
@Configuration
public class SecurityConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) {
        return http
            .csrf(csrf -> csrf.disable())
            .formLogin()
            .build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should flag CSRF disabled in session-based app"
    );
}

#[test]
fn test_csrf_disabled_cookie_auth() {
    let source = r#"
@Configuration
public class SecurityConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) {
        return http
            .csrf().disable()
            .authorizeRequests()
            .and()
            .formLogin().loginPage("/login")
            .build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should flag CSRF disabled with form login"
    );
}

#[test]
fn test_csrf_disabled_remember_me() {
    let source = r#"
@Configuration
public class SecurityConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) {
        return http
            .csrf(AbstractHttpConfigurer::disable)
            .rememberMe()
            .build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should flag CSRF disabled with remember-me (cookie-based)"
    );
}

#[test]
fn test_csrf_disabled_stateless_but_form_login() {
    let source = r#"
@Configuration
public class SecurityConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) {
        return http
            .sessionManagement(s -> s.sessionCreationPolicy(STATELESS))
            .csrf(csrf -> csrf.disable())
            .formLogin()
            .build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should flag CSRF disabled when config has STATELESS but uses form login (inconsistent)"
    );
}

#[test]
fn test_csrf_disabled_no_stateless_with_jwt() {
    let source = r#"
@Configuration
public class SecurityConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) {
        return http
            .csrf(csrf -> csrf.disable())
            .addFilterBefore(jwtAuthenticationFilter, UsernamePasswordAuthenticationFilter.class)
            .build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CsrfDisabled),
        "Should flag CSRF disabled when JWT is present but session is not explicitly STATELESS"
    );
}

#[test]
fn test_permit_all_admin_path() {
    let source = r#"
public class SecurityConfig {
    public void configure() {
        http.authorizeRequests()
            .antMatchers("/admin/**").permitAll()
            .anyRequest().authenticated();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecurePermitAll),
        "Should detect permitAll on /admin path"
    );
}

#[test]
fn test_permit_all_internal_path() {
    let source = r#"
public class SecurityConfig {
    public void configure() {
        http.authorizeHttpRequests(auth -> auth
            .requestMatchers("/internal/metrics").permitAll()
            .anyRequest().authenticated()
        );
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecurePermitAll),
        "Should detect permitAll on /internal path"
    );
}

#[test]
fn test_permit_all_actuator_path() {
    let source = r#"
public class SecurityConfig {
    public void configure() {
        http.authorizeRequests()
            .antMatchers("/actuator/health", "/actuator/info").permitAll()
            .anyRequest().authenticated();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecurePermitAll),
        "Should detect permitAll on /actuator paths"
    );
}

#[test]
fn test_permit_all_public_path_ok() {
    let source = r#"
public class SecurityConfig {
    public void configure() {
        http.authorizeRequests()
            .antMatchers("/public/**", "/login", "/register").permitAll()
            .anyRequest().authenticated();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecurePermitAll),
        "Should not flag permitAll on safe public paths"
    );
}

#[test]
fn test_noop_password_encoder() {
    let source = r#"
public class SecurityConfig {
    @Bean
    public PasswordEncoder passwordEncoder() {
        return NoOpPasswordEncoder.getInstance();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::WeakPasswordEncoding),
        "Should detect NoOpPasswordEncoder"
    );
}

#[test]
fn test_standard_password_encoder() {
    let source = r#"
public class SecurityConfig {
    @Bean
    public PasswordEncoder passwordEncoder() {
        return new StandardPasswordEncoder();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::WeakPasswordEncoding),
        "Should detect StandardPasswordEncoder"
    );
}

#[test]
fn test_md5_password_encoder() {
    let source = r#"
public class SecurityConfig {
    @Bean
    public PasswordEncoder passwordEncoder() {
        return new MessageDigestPasswordEncoder("MD5");
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::WeakPasswordEncoding),
        "Should detect MD5 MessageDigestPasswordEncoder"
    );
}

#[test]
fn test_bcrypt_password_encoder_ok() {
    let source = r#"
public class SecurityConfig {
    @Bean
    public PasswordEncoder passwordEncoder() {
        return new BCryptPasswordEncoder();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SecurityConfig.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::WeakPasswordEncoding),
        "Should not flag BCryptPasswordEncoder"
    );
}

// ========== Jackson Tests ==========

#[test]
fn test_jackson_enable_default_typing() {
    let source = r#"
public class JsonConfig {
    public void configure() {
        ObjectMapper mapper = new ObjectMapper();
        mapper.enableDefaultTyping();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JsonConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecureDeserializationConfig),
        "Should detect enableDefaultTyping()"
    );
    assert_eq!(
        findings
            .iter()
            .find(|f| f.vuln_type == VulnType::InsecureDeserializationConfig)
            .unwrap()
            .severity,
        Severity::Critical
    );
}

#[test]
fn test_jackson_enable_default_typing_with_args() {
    let source = r#"
public class JsonConfig {
    public void configure() {
        ObjectMapper mapper = new ObjectMapper();
        mapper.enableDefaultTyping(ObjectMapper.DefaultTyping.NON_FINAL);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JsonConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecureDeserializationConfig),
        "Should detect enableDefaultTyping with arguments"
    );
}

#[test]
fn test_jackson_activate_default_typing() {
    let source = r#"
public class JsonConfig {
    public void configure() {
        mapper.activateDefaultTyping(polymorphicTypeValidator);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JsonConfig.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecureDeserializationConfig),
        "Should detect activateDefaultTyping()"
    );
}

#[test]
fn test_json_type_info_class() {
    let source = r#"
@JsonTypeInfo(use = JsonTypeInfo.Id.CLASS, include = JsonTypeInfo.As.PROPERTY, property = "@class")
public class Command {
    private String action;
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Command.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecureDeserializationConfig),
        "Should detect @JsonTypeInfo with Id.CLASS"
    );
}

#[test]
fn test_json_type_info_minimal_class() {
    let source = r#"
@JsonTypeInfo(use = JsonTypeInfo.Id.MINIMAL_CLASS, include = JsonTypeInfo.As.PROPERTY)
public abstract class Event {
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Event.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecureDeserializationConfig),
        "Should detect @JsonTypeInfo with Id.MINIMAL_CLASS"
    );
}

#[test]
fn test_json_type_info_name_ok() {
    let source = r#"
@JsonTypeInfo(use = JsonTypeInfo.Id.NAME, include = JsonTypeInfo.As.PROPERTY, property = "type")
@JsonSubTypes({
    @JsonSubTypes.Type(value = Dog.class, name = "dog"),
    @JsonSubTypes.Type(value = Cat.class, name = "cat")
})
public abstract class Animal {
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Animal.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecureDeserializationConfig),
        "Should not flag @JsonTypeInfo with Id.NAME (safe variant)"
    );
}

// ========== Servlet Cookie Tests ==========

#[test]
fn test_cookie_set_secure_false() {
    let source = r#"
public class CookieHandler {
    public void handleCookie() {
        Cookie cookie = new Cookie("session", sessionId);
        cookie.setSecure(false);
        response.addCookie(cookie);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CookieHandler.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecureCookie),
        "Should detect setSecure(false)"
    );
}

#[test]
fn test_cookie_set_httponly_false() {
    let source = r#"
public class CookieHandler {
    public void handleCookie() {
        Cookie cookie = new Cookie("token", token);
        cookie.setHttpOnly(false);
        response.addCookie(cookie);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CookieHandler.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecureCookie),
        "Should detect setHttpOnly(false)"
    );
}

#[test]
fn test_cookie_secure_flags_ok() {
    let source = r#"
public class CookieHandler {
    public void handleCookie() {
        Cookie cookie = new Cookie("session", sessionId);
        cookie.setSecure(true);
        cookie.setHttpOnly(true);
        response.addCookie(cookie);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CookieHandler.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::InsecureCookie),
        "Should not flag when both flags are set to true"
    );
}

#[test]
fn test_missing_security_headers_server_role() {
    let source = r#"
@RestController
public class HeaderController {
    public void handle(HttpServletResponse response) {
        response.setHeader("Cache-Control", "no-store");
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("HeaderController.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::MissingSecurityHeader),
        "Should detect missing security headers on server responses"
    );
}

#[test]
fn test_missing_security_headers_client_role_ignored() {
    let source = r#"
public class ApiClient {
    public void call() throws Exception {
        HttpClient client = HttpClient.newHttpClient();
        HttpRequest request = HttpRequest.newBuilder()
            .uri(URI.create("https://example.com"))
            .setHeader("X-Test", "1")
            .build();
        HttpResponse<String> response = client.send(request, HttpResponse.BodyHandlers.ofString());
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ApiClient.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::MissingSecurityHeader),
        "Should not flag missing headers on HTTP clients"
    );
}

// ========== SQL Injection Tests (Taint-Aware) ==========

#[test]
fn test_hql_injection_string_concat() {
    let source = r#"
public class UserRepository {
    public List<User> findUsersByName(@RequestParam String name) {
        String query = "SELECT u FROM User u WHERE u.name = '" + name + "'";
        return entityManager.createQuery(query, User.class).getResultList();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserRepository.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::SqlInjection),
        "Should detect SQL injection via string concatenation"
    );
    assert_eq!(
        findings
            .iter()
            .find(|f| f.vuln_type == VulnType::SqlInjection)
            .unwrap()
            .severity,
        Severity::Critical
    );
}

#[test]
fn test_hql_injection_string_format() {
    let source = r#"
public class ProductService {
    public void query(@RequestParam String userCategory) {
        String hql = String.format("FROM Product WHERE category = '%s'", userCategory);
        Query query = session.createQuery(hql);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ProductService.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::SqlInjection),
        "Should detect SQL injection via String.format"
    );
}

#[test]
fn test_hql_injection_stringbuilder() {
    let source = r#"
public class OrderService {
    public void query(@RequestParam String status) {
        StringBuilder queryBuilder = new StringBuilder("SELECT * FROM Order WHERE status = '");
        queryBuilder.append(status);
        queryBuilder.append("'");
        Query query = em.createNativeQuery(queryBuilder.toString());
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("OrderService.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::SqlInjection),
        "Should detect SQL injection via StringBuilder"
    );
}

#[test]
fn test_hql_parameterized_ok() {
    let source = r#"
public class UserRepository {
    public User findUser(@RequestParam String name) {
        TypedQuery<User> query = em.createQuery(
            "SELECT u FROM User u WHERE u.name = :name", User.class);
        query.setParameter("name", name);
        return query.getSingleResult();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserRepository.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::SqlInjection),
        "Should not flag parameterized HQL queries"
    );
}

#[test]
fn test_native_query_injection() {
    let source = r#"
public class UserDao {
    public void query(@RequestParam String email) {
        String sql = "SELECT * FROM users WHERE email = '" + email + "'";
        Query query = em.createNativeQuery(sql);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserDao.java"));

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::SqlInjection),
        "Should detect native SQL query injection"
    );
}

#[test]
fn test_criteria_query_ok() {
    let source = r#"
public class UserRepository {
    public void query() {
        CriteriaBuilder cb = em.getCriteriaBuilder();
        CriteriaQuery<User> cq = cb.createQuery(User.class);
        Root<User> user = cq.from(User.class);
        cq.select(user).where(cb.equal(user.get("name"), userName));
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserRepository.java"));

    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::SqlInjection),
        "Should not flag safe Criteria API usage"
    );
}

// ========== Jackson Framework Plugin Detection Tests (Phase 7) ==========

#[test]
fn test_jackson_plugin_interface() {
    let source = r#"
package com.example.plugin;

@JsonTypeInfo(use = JsonTypeInfo.Id.CLASS)
public interface Plugin {
    void execute();
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("plugin/Plugin.java"));

    let jackson_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::InsecureDeserializationConfig)
        .collect();

    assert!(
        !jackson_findings.is_empty(),
        "Should detect Jackson configuration in plugin interface"
    );
    assert_eq!(
        jackson_findings[0].severity,
        Severity::Medium,
        "Should downgrade severity for plugin interfaces"
    );
}

#[test]
fn test_jackson_application_code_critical() {
    let source = r#"
package com.example.app.controller;

@JsonTypeInfo(use = JsonTypeInfo.Id.CLASS)
public class UserCommand {
    private String action;
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("app/controller/UserCommand.java"));

    let jackson_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::InsecureDeserializationConfig)
        .collect();

    assert!(
        !jackson_findings.is_empty(),
        "Should detect Jackson configuration in application code"
    );
    assert_eq!(
        jackson_findings[0].severity,
        Severity::Critical,
        "Should keep CRITICAL severity for application code"
    );
}

#[test]
fn test_jackson_test_directory_medium() {
    let source = r#"
package com.example.test;

public class TestObjectMapper {
    public void configure() {
        ObjectMapper mapper = new ObjectMapper();
        mapper.enableDefaultTyping();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("src/test/java/TestObjectMapper.java"));

    let jackson_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::InsecureDeserializationConfig)
        .collect();

    // Test code might be filtered out or downgraded
    if !jackson_findings.is_empty() {
        assert_eq!(
            jackson_findings[0].severity,
            Severity::Medium,
            "Should downgrade severity for test code"
        );
    }
}
