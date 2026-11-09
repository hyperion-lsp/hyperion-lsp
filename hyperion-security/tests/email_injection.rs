// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_mime_message_subject_injection() {
    let source = r#"
public class EmailService {
    public void sendEmail(HttpServletRequest request) throws Exception {
        String subject = request.getParameter("subject");
        MimeMessage message = new MimeMessage(session);
        message.setSubject(subject);  // Email header injection
        Transport.send(message);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("EmailService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect email header injection in setSubject"
    );
    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::EmailHeaderInjection),
        "Should detect EmailHeaderInjection vuln type"
    );
    assert_eq!(
        findings[0].severity,
        hyperion_security::Severity::High,
        "Email header injection should be High severity"
    );
}

#[test]
fn test_mime_message_from_injection() {
    let source = r#"
public class EmailController {
    public void sendNotification(@RequestParam String fromAddress) throws Exception {
        MimeMessage mimeMessage = new MimeMessage(session);
        mimeMessage.setFrom(fromAddress);
        Transport.send(mimeMessage);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("EmailController.java"));

    assert!(
        !findings.is_empty(),
        "Should detect email header injection in setFrom"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::EmailHeaderInjection));
}

#[test]
fn test_mime_message_recipient_injection() {
    let source = r#"
public class MailService {
    public void send(HttpServletRequest request) throws Exception {
        String to = request.getParameter("to");
        MimeMessage message = new MimeMessage(session);
        message.addRecipient(Message.RecipientType.TO, new InternetAddress(to));
        Transport.send(message);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("MailService.java"));

    assert!(
        !findings.is_empty(),
        "Should detect email header injection in addRecipient"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::EmailHeaderInjection));
}

#[test]
fn test_simple_mail_message_injection() {
    let source = r#"
public class SpringMailService {
    public void sendMail(@PathVariable String subject, @PathVariable String to) {
        SimpleMailMessage mailMessage = new SimpleMailMessage();
        mailMessage.setSubject(subject);
        mailMessage.setTo(to);
        mailSender.send(mailMessage);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SpringMailService.java"));

    assert!(
        !findings.is_empty(),
        "Should detect email header injection in SimpleMailMessage"
    );
    // Should detect both setSubject and setTo
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::EmailHeaderInjection));
}

#[test]
fn test_custom_header_injection() {
    let source = r#"
public class CustomHeaderMailer {
    public void send(HttpServletRequest request) throws Exception {
        String headerValue = request.getParameter("header");
        MimeMessage message = new MimeMessage(session);
        message.addHeader("X-Custom-Header", headerValue);
        Transport.send(message);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CustomHeaderMailer.java"));

    assert!(
        !findings.is_empty(),
        "Should detect email header injection in addHeader"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::EmailHeaderInjection));
}

#[test]
fn test_cc_bcc_injection() {
    let source = r#"
public class BulkMailer {
    public void sendBulk(@RequestParam String cc, @RequestParam String bcc) throws Exception {
        MimeMessage message = new MimeMessage(session);
        message.setCc(cc);
        message.setBcc(bcc);
        Transport.send(message);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("BulkMailer.java"));

    assert!(
        !findings.is_empty(),
        "Should detect email header injection in setCc/setBcc"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::EmailHeaderInjection));
}

#[test]
fn test_safe_email_no_false_positive() {
    // Email with constant values, no user input
    let source = r#"
public class SafeEmailService {
    public void sendNotification() throws Exception {
        MimeMessage message = new MimeMessage(session);
        message.setSubject("System Notification");
        message.setTo("admin@example.com");
        Transport.send(message);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SafeEmailService.java"));

    assert!(
        findings.is_empty(),
        "Should not detect injection for constant email headers"
    );
}

#[test]
fn test_multi_step_email_injection() {
    let source = r#"
public class EmailSender {
    public void send(HttpServletRequest request) throws Exception {
        String userInput = request.getParameter("subject");
        String emailSubject = userInput;
        MimeMessage message = new MimeMessage(session);
        message.setSubject(emailSubject);
        Transport.send(message);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("EmailSender.java"));

    assert!(
        !findings.is_empty(),
        "Should detect email injection through data flow"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::EmailHeaderInjection));

    // Verify data flow tracking
    let finding = &findings[0];
    assert!(
        !finding.dataflow.is_empty(),
        "Should have data flow path tracked"
    );
}
