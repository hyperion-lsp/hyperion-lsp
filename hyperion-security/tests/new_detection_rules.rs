// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_ssrf_new_url_openstream() {
    let source = r#"
import java.net.URL;

public class SsrfController {
    public void fetch(@RequestParam String url) throws Exception {
        new URL(url).openStream();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SsrfController.java"));

    assert!(findings.iter().any(|f| f.vuln_type == VulnType::Ssrf));
}

#[test]
fn test_xxe_xml_input_factory() {
    let source = r#"
import javax.xml.stream.XMLInputFactory;
import javax.xml.stream.XMLStreamReader;
import java.io.StringReader;

public class XmlHandler {
    public void parse(@RequestParam String xml) throws Exception {
        var xif = XMLInputFactory.newInstance();
        XMLStreamReader reader = xif.createXMLStreamReader(new StringReader(xml));
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("XmlHandler.java"));

    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::XxeInjection));
}

#[test]
fn test_unsafe_deserialization_object_input_stream() {
    let source = r#"
import java.io.ObjectInputStream;
import java.io.ByteArrayInputStream;
import java.util.Base64;

public class DeserController {
    public void parse(@RequestParam String token) throws Exception {
        byte[] bytes = Base64.getDecoder().decode(token);
        ObjectInputStream ois = new ObjectInputStream(new ByteArrayInputStream(bytes));
        Object obj = ois.readObject();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("DeserController.java"));

    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::UnsafeDeserialization));
}

#[test]
fn test_zip_slip_detection() {
    let source = r#"
import java.util.zip.ZipFile;
import java.util.zip.ZipEntry;
import java.io.File;
import java.io.InputStream;
import java.nio.file.Files;

public class ZipHandler {
    public void extract(ZipFile zip, File dest) throws Exception {
        ZipEntry entry = zip.entries().nextElement();
        File out = new File(dest, entry.getName());
        InputStream in = zip.getInputStream(entry);
        Files.copy(in, out.toPath());
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ZipHandler.java"));

    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::PathTraversal));
}

#[test]
fn zip_slip_is_not_suppressed_by_unrelated_normalization() {
    let source = r#"
import java.util.zip.ZipFile;
import java.util.zip.ZipEntry;
import java.io.File;
import java.nio.file.Files;

class ZipHandler {
    void extract(ZipFile zip, File dest) throws Exception {
        ZipEntry entry = zip.entries().nextElement();
        File out = new File(dest, entry.getName());
        String unrelated = java.nio.file.Path.of(".").normalize().toString();
        if (unrelated.startsWith(".")) return;
        Files.copy(zip.getInputStream(entry), out.toPath());
    }
}
"#;
    let findings = SecurityScanner::new(ScanConfig::default())
        .scan_source(source, Path::new("ZipHandler.java"));
    assert!(findings
        .iter()
        .any(|finding| finding.vuln_type == VulnType::PathTraversal));
}

#[test]
fn zip_slip_does_not_trust_guard_before_path_reassignment() {
    let source = r#"
import java.util.zip.ZipFile;
import java.util.zip.ZipEntry;
import java.nio.file.Files;
import java.nio.file.Path;

class ZipHandler {
    void extract(ZipFile zip, Path root) throws Exception {
        ZipEntry entry = zip.entries().nextElement();
        Path target = root.resolve(entry.getName()).normalize();
        if (!target.startsWith(root)) { throw new SecurityException(); }
        target = root.resolve("../outside").resolve(entry.getName());
        Files.copy(zip.getInputStream(entry), target);
    }
}
"#;
    let findings = SecurityScanner::new(ScanConfig::default())
        .scan_source(source, Path::new("ZipHandler.java"));
    assert!(findings
        .iter()
        .any(|finding| finding.vuln_type == VulnType::PathTraversal));
}

#[test]
fn zip_entry_path_construction_without_write_is_not_reported() {
    let source = r#"
import java.util.zip.ZipFile;
import java.util.zip.ZipEntry;
import java.io.File;
import java.nio.file.StandardCopyOption;

class ZipHandler {
    File inspect(ZipFile zip, File dest) {
        ZipEntry entry = zip.entries().nextElement();
        File candidate = new File(dest, entry.getName());
        StandardCopyOption option = StandardCopyOption.REPLACE_EXISTING;
        return candidate;
    }
}
"#;
    let findings = SecurityScanner::new(ScanConfig::default())
        .scan_source(source, Path::new("ZipHandler.java"));
    assert!(!findings
        .iter()
        .any(|finding| finding.vuln_type == VulnType::PathTraversal));
}

#[test]
fn test_jwt_jku_misuse() {
    let source = r#"
import com.auth0.jwk.JwkProviderBuilder;
import com.auth0.jwt.JWT;
import java.net.URL;

public class JwtController {
    public void handle(@RequestParam String token) throws Exception {
        var decoded = JWT.decode(token);
        var jku = decoded.getHeaderClaim("jku");
        var provider = new JwkProviderBuilder(new URL(jku.asString())).build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JwtController.java"));

    assert!(findings.iter().any(|f| f.vuln_type == VulnType::JwtMisuse));
}

#[test]
fn test_weak_jwt_secret() {
    let source = r#"
import io.jsonwebtoken.Jwts;
import io.jsonwebtoken.SignatureAlgorithm;
import io.jsonwebtoken.impl.TextCodec;

public class JwtSecret {
    public static final String[] SECRETS = {"victory", "business"};
    public static final String JWT_SECRET = TextCodec.BASE64.encode(SECRETS[0]);

    public String token() {
        return Jwts.builder().signWith(SignatureAlgorithm.HS256, JWT_SECRET).compact();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JwtSecret.java"));

    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::HardcodedSecret));
}

#[test]
fn test_missing_access_control() {
    let source = r#"
public class ProfileController {
    @GetMapping("/profile/{userId}")
    public String profile(@PathVariable String userId) {
        UserProfile profile = new UserProfile(userId);
        return profile.toString();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ProfileController.java"));

    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::MissingAccessControl));
}

#[test]
fn test_stored_xss_heuristic() {
    let source = r#"
import com.fasterxml.jackson.databind.ObjectMapper;
import java.util.*;

public class StoredComments {
    private static final Map<String, List<Comment>> comments = new HashMap<>();

    @GetMapping("/comments")
    @ResponseBody
    public Collection<Comment> getComments() {
        return comments.getOrDefault("user", List.of());
    }

    @PostMapping("/comments")
    @ResponseBody
    public void add(@RequestBody String commentStr) throws Exception {
        ObjectMapper mapper = new ObjectMapper();
        Comment comment = mapper.readValue(commentStr, Comment.class);
        comments.computeIfAbsent("user", k -> new ArrayList<>()).add(comment);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("StoredComments.java"));

    assert!(findings.iter().any(|f| f.vuln_type == VulnType::Xss));
}
