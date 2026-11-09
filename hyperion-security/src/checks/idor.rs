// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Enhanced IDOR (Insecure Direct Object Reference) detection
//!
//! Detects use of user-controlled IDs in object retrieval without authorization checks.
//!
//! To reduce false positives, the check recognizes service-layer delegation,
//! authorization keywords and AOP annotations, authentication context passed
//! to services, and read-only public-resource endpoints. Service delegation
//! lowers confidence because authorization may live in another file; explicit
//! evidence suppresses the finding.

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use tracing::debug;

pub struct IdorCheck;

/// Public resource paths that don't need authorization
const PUBLIC_RESOURCE_PATHS: &[&str] = &[
    "/products",
    "/product",
    "/categories",
    "/category",
    "/catalog",
    "/blog",
    "/posts",
    "/articles",
    "/search",
    "/browse",
    "/public",
    "/static",
    "/assets",
];

impl IdorCheck {
    /// Check if the endpoint path indicates a public resource
    ///
    /// Public resources (like product catalog, blog posts) are intentionally accessible
    /// to anyone without authorization checks. This reduces false positives for IDOR
    /// detection on legitimate public endpoints.
    ///
    /// Only suppresses for GET requests - POST/PUT/DELETE on public resources still need auth.
    fn is_public_resource_endpoint(text: &str) -> bool {
        // Only consider GET requests as candidates for public access
        let is_get_request = text.contains("@GetMapping");
        if !is_get_request {
            return false;
        }

        // Extract the path from @GetMapping annotation
        // Patterns: @GetMapping("/products/{id}"), @GetMapping(path = "/products/{id}")
        let has_public_path = PUBLIC_RESOURCE_PATHS.iter().any(|public_path| {
            // Check for the path in the mapping annotation
            text.contains(&format!("@GetMapping(\"{public_path}\""))
                || text.contains(&format!("@GetMapping(\"{public_path}/"))
                || text.contains(&format!("@GetMapping(path = \"{public_path}\""))
                || text.contains(&format!("@GetMapping(path = \"{public_path}/"))
                || text.contains(&format!("@GetMapping(value = \"{public_path}\""))
                || text.contains(&format!("@GetMapping(value = \"{public_path}/"))
        });

        has_public_path
    }

    /// Detect if the controller delegates to a service layer
    ///
    /// Service layer methods often contain authorization checks that the scanner
    /// cannot see in single-file analysis. This is a common architectural pattern.
    fn has_service_layer_delegation(text: &str) -> bool {
        // Detect patterns like: userService.getUser(id), repository.findById(id)
        // Service delegation: calls to objects ending in "Service" or "service"
        let has_service_call = text.contains("Service.")
            || text.contains("service.")
            || text.contains("Service.get")
            || text.contains("service.get")
            || text.contains("Service.find")
            || text.contains("service.find");

        // Repository delegation: direct repository calls from controller
        // These are more suspicious as they bypass service layer
        let has_direct_repo_in_controller = text.contains("Repository.find")
            || text.contains("repository.find")
            || text.contains("Repository.get")
            || text.contains("repository.get");

        // If service layer is used, likely has authz. If direct repo, more suspicious.
        has_service_call && !has_direct_repo_in_controller
    }

    /// Detect authorization-related method calls or keywords
    ///
    /// These patterns suggest authorization logic exists somewhere in the call chain,
    /// even if not visible in the current method.
    fn has_authorization_keywords(text: &str) -> bool {
        text.contains("checkAccess")
            || text.contains("verifyOwnership")
            || text.contains("isAuthorized")
            || text.contains("canAccess")
            || text.contains("hasPermission")
            || text.contains("checkPermission")
            || text.contains("authorizeAccess")
            || text.contains("ensureOwnership")
            || text.contains("requireOwnership")
            || text.contains("validateAccess")
            || text.contains("assertOwnership")
    }

    /// Check if authorization is present at method or class level
    fn has_explicit_authorization(text: &str) -> bool {
        text.contains("@PreAuthorize")
            || text.contains("@Secured")
            || text.contains("@RolesAllowed")
            || text.contains("hasRole(")
            || text.contains("hasAuthority(")
            || text.contains("checkOwnership(")
            || text.contains("verifyOwnership(")
            || text.contains("isOwner(")
            || text.contains("hasAccess(")
            || text.contains("canAccess(")
            || text.contains("authorizeAccess(")
            || text.contains("checkPermission(")
            || Self::has_aop_authorization_annotation(text)
    }

    /// Check if AOP-based authorization annotations are present
    ///
    /// Many Spring Boot applications use custom AOP annotations for authorization
    /// that are applied to controller methods. These annotations trigger aspect logic
    /// that verifies ownership or permissions before the method executes.
    ///
    /// Phase 3.3: Detect common AOP authorization annotation patterns to reduce
    /// IDOR false positives when authorization is handled via AOP.
    fn has_aop_authorization_annotation(text: &str) -> bool {
        // Spring Security annotations
        text.contains("@PreAuthorize")
            || text.contains("@PostAuthorize")
            || text.contains("@Secured")
            || text.contains("@RolesAllowed")
            // Common custom authorization annotations
            || text.contains("@CheckOwnership")
            || text.contains("@VerifyOwner")
            || text.contains("@RequiresOwnership")
            || text.contains("@Authorized")
            || text.contains("@AuthorizeAccess")
            || text.contains("@CheckAccess")
            || text.contains("@RequirePermission")
            // Method security annotations
            || text.contains("@PermissionCheck")
            || text.contains("@AccessControl")
            || text.contains("@RequireOwner")
            || text.contains("@OwnershipCheck")
            || text.contains("@VerifyAccess")
            || text.contains("@EnsureOwnership")
            || text.contains("@ValidateAccess")
            || text.contains("@AuthCheck")
            || text.contains("@SecurityCheck")
    }

    /// Detect if authentication context is passed to service methods
    ///
    /// When authentication context (Authentication, Principal, SecurityContext) is passed
    /// to service methods, it indicates that authorization is handled in the service layer.
    /// This is a common pattern where the service method performs the actual authorization checks.
    ///
    /// Phase 5: Reduce false positives by recognizing auth context delegation to services.
    fn service_receives_auth_context(text: &str) -> bool {
        // Check for Authentication parameter being passed to methods
        // Patterns: service.method(id, auth), service.method(id, authentication)
        let passes_auth_param = text.contains(", auth)")
            || text.contains(", authentication)")
            || text.contains(", authenticatedUser)");

        // Check for Principal parameter being passed
        // Pattern: service.method(id, principal)
        let passes_principal_param = text.contains(", principal)");

        // Check for @AuthenticationPrincipal annotation in method signature
        // This annotation injects the authenticated user into the parameter
        let has_auth_principal_annotation = text.contains("@AuthenticationPrincipal");

        // Check for SecurityContext being passed
        // Pattern: service.method(id, SecurityContextHolder.getContext())
        let passes_security_context =
            text.contains("SecurityContextHolder") || text.contains("SecurityContext");

        // Check for Authentication or Principal in method parameters
        // Patterns: "Authentication auth", "Principal principal"
        let has_auth_in_signature =
            text.contains("Authentication auth") || text.contains("Principal principal");

        passes_auth_param
            || passes_principal_param
            || has_auth_principal_annotation
            || passes_security_context
            || has_auth_in_signature
    }
}

impl Check for IdorCheck {
    fn name(&self) -> &'static str {
        "idor"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::CLASS_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = node.text().to_string();

        // Check if this is an endpoint method
        let is_endpoint = text.contains("@GetMapping")
            || text.contains("@PostMapping")
            || text.contains("@PutMapping")
            || text.contains("@DeleteMapping")
            || text.contains("@PatchMapping")
            || text.contains("@Path(");

        if !is_endpoint {
            return Vec::new();
        }

        // Check for user-controlled ID parameters
        let has_path_var = text.contains("@PathVariable") || text.contains("@PathParam");
        let has_request_param = text.contains("@RequestParam");

        if !(has_path_var || has_request_param) {
            return Vec::new();
        }

        // Check for ID-like parameter names
        let has_id_param = text.contains("userId")
            || text.contains("accountId")
            || text.contains("profileId")
            || text.contains("ownerId")
            || text.contains("documentId")
            || text.contains("orderId")
            || text.contains("customerId")
            || text.contains("productId")
            || text.contains("Long id")
            || text.contains("String id")
            || text.contains("Integer id")
            || text.contains("int id");

        if !has_id_param {
            return Vec::new();
        }

        // Check for object retrieval using ID
        // This includes both direct retrieval and service method calls
        let retrieves_object = text.contains("findById(")
            || text.contains("getById(")
            || text.contains(".find")
            || text.contains(".get")
            || text.contains("Repository.find")
            || text.contains("repository.find")
            || text.contains("Repository.get")
            || text.contains("repository.get")
            || text.contains("Service.get")
            || text.contains("service.get")
            || text.contains("Service.find")
            || text.contains("service.find")
            || text.contains("entityManager.find(")
            || text.contains("session.get(")
            || text.contains("load");

        if !retrieves_object {
            return Vec::new();
        }

        // Check for explicit authorization - if present, suppress entirely
        if Self::has_explicit_authorization(&text) {
            debug!("IDOR check: Explicit authorization found, suppressing finding");
            return Vec::new();
        }

        // Phase 3.1: Check for authorization keywords - if present, suppress
        if Self::has_authorization_keywords(&text) {
            debug!("IDOR check: Authorization keywords found, suppressing finding");
            return Vec::new();
        }

        // Phase 5: Check if authentication context is passed to service methods
        // This indicates authorization is handled in the service layer
        if Self::service_receives_auth_context(&text) {
            debug!(
                "IDOR check: Authentication context passed to service method, suppressing finding"
            );
            return Vec::new();
        }

        // Phase 15: Check if this is a public resource endpoint (GET only)
        // Public resources like product catalogs and blog posts don't require authorization
        if Self::is_public_resource_endpoint(&text) {
            debug!(
                "IDOR check: Public resource endpoint detected (GET request), suppressing finding"
            );
            return Vec::new();
        }

        // Phase 3.1: Check for service layer delegation
        let has_service_delegation = Self::has_service_layer_delegation(&text);

        if has_service_delegation {
            debug!("IDOR check: Service layer delegation detected, reducing severity to LOW");
        }

        // Determine severity, confidence, and message based on context
        let (severity, confidence, message, remediation) = if has_service_delegation {
            // Service layer call - authorization may exist in service method
            // Phase 3.2: Lower confidence to 30% to reflect uncertainty
            (
                Severity::Low,
                0.30_f32,
                "IDOR Review Required: User-controlled ID is passed to service layer. \
                 Controller delegates to service layer - verify authorization is enforced in the service method."
                    .to_string(),
                "Verify that the service layer method performs authorization checks:\n\
                 1. Check for @PreAuthorize or @Secured annotations on the service method\n\
                 2. Check for SecurityContext.getContext().getAuthentication() calls\n\
                 3. Check for ownership validation (e.g., user.getId() == currentUserId)\n\
                 \n\
                 If service does NOT perform authorization, add controller-level check:\n\
                 @PreAuthorize(\"@authService.canAccess(#id, authentication)\")\n\
                 or implement authorization logic in the service method."
                    .to_string(),
            )
        } else {
            // Direct repository/entity manager access - high risk
            (
                Severity::High,
                0.85_f32,
                "Insecure Direct Object Reference: User-controlled ID is used to retrieve an object \
                 without visible authorization check. An attacker could access other users' data by \
                 modifying the ID parameter."
                    .to_string(),
                "Add ownership verification or role-based access control. Use @PreAuthorize(\
                 \"@accessControl.canAccess(#id, authentication)\") or verify ownership in the \
                 service layer before returning the object."
                    .to_string(),
            )
        };

        let location = context.node_location(node);
        let code = context.get_code_snippet(node);

        let mut finding = Finding::new(VulnType::Idor, message, location, code);
        finding.severity = severity;
        finding.confidence = confidence;
        finding.remediation = remediation;

        vec![finding]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_receives_auth_context_authentication_param() {
        let code = r#"
            @GetMapping("/users/{id}")
            public User getUser(@PathVariable Long id, Authentication auth) {
                return userService.getUser(id, auth);
            }
        "#;
        assert!(IdorCheck::service_receives_auth_context(code));
    }

    #[test]
    fn test_service_receives_auth_context_principal_param() {
        let code = r#"
            @GetMapping("/users/{id}")
            public User getUser(@PathVariable Long id, Principal principal) {
                return userService.getUser(id, principal);
            }
        "#;
        assert!(IdorCheck::service_receives_auth_context(code));
    }

    #[test]
    fn test_service_receives_auth_context_authentication_principal_annotation() {
        let code = r#"
            @GetMapping("/users/{id}")
            public User getUser(@PathVariable Long id, @AuthenticationPrincipal UserDetails user) {
                return userService.getUser(id, user);
            }
        "#;
        assert!(IdorCheck::service_receives_auth_context(code));
    }

    #[test]
    fn test_service_receives_auth_context_security_context_holder() {
        let code = r#"
            @GetMapping("/users/{id}")
            public User getUser(@PathVariable Long id) {
                return userService.getUser(id, SecurityContextHolder.getContext());
            }
        "#;
        assert!(IdorCheck::service_receives_auth_context(code));
    }

    #[test]
    fn test_service_receives_auth_context_authentication_in_signature() {
        let code = r#"
            @GetMapping("/users/{id}")
            public User getUser(@PathVariable Long id, Authentication authentication) {
                return userService.getUser(id, authentication);
            }
        "#;
        assert!(IdorCheck::service_receives_auth_context(code));
    }

    #[test]
    fn test_service_receives_auth_context_authenticated_user_param() {
        let code = r#"
            @GetMapping("/users/{id}")
            public User getUser(@PathVariable Long id, User authenticatedUser) {
                return userService.getUser(id, authenticatedUser);
            }
        "#;
        assert!(IdorCheck::service_receives_auth_context(code));
    }

    #[test]
    fn test_service_receives_auth_context_no_auth_context() {
        let code = r#"
            @GetMapping("/users/{id}")
            public User getUser(@PathVariable Long id) {
                return userService.getUser(id);
            }
        "#;
        assert!(!IdorCheck::service_receives_auth_context(code));
    }

    #[test]
    fn test_service_receives_auth_context_only_id_param() {
        let code = r#"
            @GetMapping("/users/{id}")
            public User getUser(@PathVariable Long userId) {
                return userRepository.findById(userId);
            }
        "#;
        assert!(!IdorCheck::service_receives_auth_context(code));
    }

    // Phase 15: Endpoint sensitivity classification tests
    #[test]
    fn test_is_public_resource_endpoint_products() {
        let code = r#"
            @GetMapping("/products/{id}")
            public Product getProduct(@PathVariable Long id) {
                return productService.findById(id);
            }
        "#;
        assert!(IdorCheck::is_public_resource_endpoint(code));
    }

    #[test]
    fn test_is_public_resource_endpoint_product_singular() {
        let code = r#"
            @GetMapping("/product/{id}")
            public Product getProduct(@PathVariable Long id) {
                return productService.findById(id);
            }
        "#;
        assert!(IdorCheck::is_public_resource_endpoint(code));
    }

    #[test]
    fn test_is_public_resource_endpoint_categories() {
        let code = r#"
            @GetMapping("/categories/{id}")
            public Category getCategory(@PathVariable Long id) {
                return categoryService.findById(id);
            }
        "#;
        assert!(IdorCheck::is_public_resource_endpoint(code));
    }

    #[test]
    fn test_is_public_resource_endpoint_blog() {
        let code = r#"
            @GetMapping("/blog/{slug}")
            public BlogPost getBlogPost(@PathVariable String slug) {
                return blogService.findBySlug(slug);
            }
        "#;
        assert!(IdorCheck::is_public_resource_endpoint(code));
    }

    #[test]
    fn test_is_public_resource_endpoint_with_path_param() {
        let code = r#"
            @GetMapping(path = "/products/{id}")
            public Product getProduct(@PathVariable Long id) {
                return productService.findById(id);
            }
        "#;
        assert!(IdorCheck::is_public_resource_endpoint(code));
    }

    #[test]
    fn test_is_public_resource_endpoint_with_value_param() {
        let code = r#"
            @GetMapping(value = "/catalog/{id}")
            public CatalogItem getItem(@PathVariable Long id) {
                return catalogService.findById(id);
            }
        "#;
        assert!(IdorCheck::is_public_resource_endpoint(code));
    }

    #[test]
    fn test_is_not_public_resource_endpoint_post() {
        // POST on public resource should not be suppressed
        let code = r#"
            @PostMapping("/products/{id}")
            public Product updateProduct(@PathVariable Long id) {
                return productService.update(id);
            }
        "#;
        assert!(!IdorCheck::is_public_resource_endpoint(code));
    }

    #[test]
    fn test_is_not_public_resource_endpoint_put() {
        // PUT on public resource should not be suppressed
        let code = r#"
            @PutMapping("/products/{id}")
            public Product updateProduct(@PathVariable Long id) {
                return productService.update(id);
            }
        "#;
        assert!(!IdorCheck::is_public_resource_endpoint(code));
    }

    #[test]
    fn test_is_not_public_resource_endpoint_delete() {
        // DELETE on public resource should not be suppressed
        let code = r#"
            @DeleteMapping("/products/{id}")
            public void deleteProduct(@PathVariable Long id) {
                productService.delete(id);
            }
        "#;
        assert!(!IdorCheck::is_public_resource_endpoint(code));
    }

    #[test]
    fn test_is_not_public_resource_endpoint_private_orders() {
        // Private resource should not be marked as public
        let code = r#"
            @GetMapping("/orders/{id}")
            public Order getOrder(@PathVariable Long id) {
                return orderService.findById(id);
            }
        "#;
        assert!(!IdorCheck::is_public_resource_endpoint(code));
    }

    #[test]
    fn test_is_not_public_resource_endpoint_private_profile() {
        // Private resource should not be marked as public
        let code = r#"
            @GetMapping("/profile/{id}")
            public UserProfile getProfile(@PathVariable Long id) {
                return profileService.findById(id);
            }
        "#;
        assert!(!IdorCheck::is_public_resource_endpoint(code));
    }

    #[test]
    fn test_is_not_public_resource_endpoint_private_account() {
        // Private resource should not be marked as public
        let code = r#"
            @GetMapping("/account/{id}")
            public Account getAccount(@PathVariable Long id) {
                return accountService.findById(id);
            }
        "#;
        assert!(!IdorCheck::is_public_resource_endpoint(code));
    }

    // Phase 3.3: AOP authorization annotation detection tests
    #[test]
    fn test_has_aop_authorization_annotation_pre_authorize() {
        let code = r#"
            @PreAuthorize("@authService.canAccess(#orderId, authentication)")
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_post_authorize() {
        let code = r#"
            @PostAuthorize("returnObject.userId == authentication.principal.id")
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_secured() {
        let code = r#"
            @Secured("ROLE_ADMIN")
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_roles_allowed() {
        let code = r#"
            @RolesAllowed({"ROLE_USER", "ROLE_ADMIN"})
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_check_ownership() {
        let code = r#"
            @CheckOwnership
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_verify_owner() {
        let code = r#"
            @VerifyOwner
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_requires_ownership() {
        let code = r#"
            @RequiresOwnership
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_authorized() {
        let code = r#"
            @Authorized
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_authorize_access() {
        let code = r#"
            @AuthorizeAccess
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_check_access() {
        let code = r#"
            @CheckAccess
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_require_permission() {
        let code = r#"
            @RequirePermission("order:read")
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_permission_check() {
        let code = r#"
            @PermissionCheck
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_access_control() {
        let code = r#"
            @AccessControl
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_require_owner() {
        let code = r#"
            @RequireOwner
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_ownership_check() {
        let code = r#"
            @OwnershipCheck
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_verify_access() {
        let code = r#"
            @VerifyAccess
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_ensure_ownership() {
        let code = r#"
            @EnsureOwnership
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_validate_access() {
        let code = r#"
            @ValidateAccess
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_auth_check() {
        let code = r#"
            @AuthCheck
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_custom_security_check() {
        let code = r#"
            @SecurityCheck
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_no_annotation() {
        let code = r#"
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(!IdorCheck::has_aop_authorization_annotation(code));
    }

    #[test]
    fn test_has_aop_authorization_annotation_unrelated_annotation() {
        let code = r#"
            @Transactional
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findById(orderId);
            }
        "#;
        assert!(!IdorCheck::has_aop_authorization_annotation(code));
    }
}
