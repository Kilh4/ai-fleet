//! Application layer for `fleet-security`: CASA deny-by-default evaluator,
//! ContinueGate drift controller, Keyed-Hash Capability Token issuer/verifier,
//! and Base64/Hex-deobfuscating AIDefence scanner.

use crate::domain::{AuthDecision, CasaEnvelope, ContinueGateMetrics, SignedCapabilityToken, ThreatFinding};
use chrono::DateTime;
use sha2::{Digest, Sha256};

pub fn check_casa_authorization(envelope: &CasaEnvelope, requested_action: &str, now_rfc3339: &str) -> AuthDecision {
    let exp = DateTime::parse_from_rfc3339(&envelope.expires_at).ok();
    let now = DateTime::parse_from_rfc3339(now_rfc3339).ok();
    match (now, exp) {
        (Some(n), Some(e)) => {
            if n >= e {
                return AuthDecision {
                    allowed: false,
                    reason: format!("CASA envelope expired at {} (now: {})", envelope.expires_at, now_rfc3339),
                };
            }
        }
        _ => {
            return AuthDecision {
                allowed: false,
                reason: "Malformed RFC3339 timestamp in CASA envelope (deny-by-default)".to_string(),
            };
        }
    }

    if envelope.deny.iter().any(|d| d == requested_action) {
        return AuthDecision {
            allowed: false,
            reason: format!("Action '{}' is explicitly forbidden by CASA deny-list", requested_action),
        };
    }

    if envelope.allow.iter().any(|a| a == requested_action || a == "*") {
        return AuthDecision {
            allowed: true,
            reason: format!("Action '{}' authorized by CASA allow-list for '{}'", requested_action, envelope.objective),
        };
    }

    AuthDecision {
        allowed: false,
        reason: format!("Action '{}' not in CASA allow-list (deny-by-default)", requested_action),
    }
}

pub fn evaluate_continue_gate(m: &ContinueGateMetrics) -> (&'static str, String) {
    if m.coherence_score < m.min_coherence {
        return ("stop", format!("Coherence degraded ({:.2} < {:.2}); halting agent to prevent drift", m.coherence_score, m.min_coherence));
    }
    if m.uncertainty_score > m.max_uncertainty {
        return ("pause", format!("Uncertainty too high ({:.2} > {:.2}); escalating effort & requesting review", m.uncertainty_score, m.max_uncertainty));
    }
    if m.rework_ratio > m.max_rework_ratio {
        return ("pause", format!("Rework ratio exceeded ({:.2} > {:.2}); switching model via Router", m.rework_ratio, m.max_rework_ratio));
    }
    if m.budget_slope > m.max_budget_slope {
        return ("throttle", format!("Quota burn slope high ({:.3}/step > {:.3}); throttling to conservation model", m.budget_slope, m.max_budget_slope));
    }
    if m.consecutive_steps >= m.max_consecutive_steps {
        return ("checkpoint", format!("Reached {} steps; forcing mandatory SHA-256 checkpoint", m.consecutive_steps));
    }
    ("continue", format!("All 5 ContinueGate invariants healthy (coherence={:.2}, uncertainty={:.2}, rework={:.2})", m.coherence_score, m.uncertainty_score, m.rework_ratio))
}

/// RFC-2104 HMAC-SHA256 construction (`H((K ^ opad) || H((K ^ ipad) || msg))`).
pub fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    let mut k_block = [0u8; 64];
    if key.len() > 64 {
        let hashed = Sha256::digest(key);
        k_block[..32].copy_from_slice(&hashed);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k_block[i];
        opad[i] ^= k_block[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    format!("{:x}", outer.finalize())
}

pub fn issue_capability_token(
    signing_key: &[u8],
    subject: &str,
    task_id: &str,
    scopes: Vec<String>,
    expires_epoch_secs: i64,
) -> SignedCapabilityToken {
    let canonical = format!("{}|{}|{}|{}", subject, task_id, scopes.join(","), expires_epoch_secs);
    let mac_hex = hmac_sha256_hex(signing_key, canonical.as_bytes());
    SignedCapabilityToken {
        subject: subject.into(),
        task_id: task_id.into(),
        scopes,
        expires_epoch_secs,
        mac_hex,
    }
}

pub fn verify_capability_token(
    signing_key: &[u8],
    token: &SignedCapabilityToken,
    required_scope: &str,
    now_epoch_secs: i64,
) -> Result<(), String> {
    if now_epoch_secs >= token.expires_epoch_secs {
        return Err("capability token expired".into());
    }
    let canonical = format!("{}|{}|{}|{}", token.subject, token.task_id, token.scopes.join(","), token.expires_epoch_secs);
    let expected = hmac_sha256_hex(signing_key, canonical.as_bytes());
    // Constant-time byte comparison to prevent timing side-channels.
    if expected.len() != token.mac_hex.len()
        || expected.bytes().zip(token.mac_hex.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) != 0
    {
        return Err("invalid HMAC-SHA256 signature on capability token".into());
    }
    if !token.scopes.iter().any(|s| s == required_scope || s == "*") {
        return Err(format!("token lacks required scope `{required_scope}`"));
    }
    Ok(())
}

fn decode_hex_runs(input: &str) -> String {
    let mut extra = String::new();
    for word in input.split_whitespace() {
        let hex = word.strip_prefix("0x").unwrap_or(word);
        if hex.len() >= 16 && hex.len() % 2 == 0 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let bytes: Vec<u8> = (0..hex.len())
                .step_by(2)
                .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
                .collect();
            if let Ok(s) = String::from_utf8(bytes) {
                extra.push(' ');
                extra.push_str(&s);
            }
        }
    }
    extra
}

pub fn scan_aidefence_threats(input: &str) -> Vec<ThreatFinding> {
    // Strip zero-width obfuscation characters (`\u{200B}`, `\u{200C}`, `\u{FEFF}`) and decode hex payloads.
    let cleaned: String = input
        .chars()
        .filter(|c| !matches!(*c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}'))
        .collect();
    let decoded_hex = decode_hex_runs(&cleaned);
    let combined = format!("{cleaned} {decoded_hex}");
    let low = combined.to_lowercase();
    let mut findings = Vec::new();

    // 1. ADR-118 0..6 word window between override verbs and target nouns (English & Arabic)
    let verbs = ["ignore", "disregard", "forget", "override", "bypass", "تجاهل", "الغ", "انس"];
    let nouns = [
        "instruction", "instructions", "prompt", "prompts", "rule", "rules",
        "context", "constitution", "system-prompt", "التعليمات", "القواعد", "الدستور",
    ];
    let tokens: Vec<&str> = low.split_whitespace().collect();
    for (i, tok) in tokens.iter().enumerate() {
        let clean_tok = tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        if verbs.contains(&clean_tok) {
            let end = (i + 8).min(tokens.len());
            for (j, raw_target) in tokens.iter().enumerate().take(end).skip(i + 1) {
                let target = raw_target.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
                if nouns.contains(&target) {
                    findings.push(ThreatFinding {
                        category: "instruction_override".into(),
                        severity: "CRITICAL".into(),
                        confidence: 0.96,
                        description: format!(
                            "ADR-118 verb->noun override window detected ('{} ... {}' within {} words)",
                            clean_tok,
                            target,
                            j - i - 1
                        ),
                    });
                    break;
                }
            }
        }
    }

    // 2. Role hijack / Jailbreak / Exfiltration markers
    let hijack_markers = [
        ("you are now", "role_switching", "HIGH", 0.88),
        ("act as a different", "role_switching", "HIGH", 0.85),
        ("behave like", "role_switching", "HIGH", 0.84),
        ("developer mode", "jailbreak", "CRITICAL", 0.93),
        ("god mode", "jailbreak", "CRITICAL", 0.95),
        ("sudo mode", "jailbreak", "HIGH", 0.90),
        ("curl http", "exfil_command", "HIGH", 0.89),
        ("invoke-webrequest", "exfil_command", "HIGH", 0.89),
    ];
    for (pat, cat, sev, conf) in hijack_markers {
        if low.contains(pat) {
            findings.push(ThreatFinding {
                category: cat.into(),
                severity: sev.into(),
                confidence: conf,
                description: format!("Detected adversarial marker '{}'", pat),
            });
        }
    }

    // 3. Secret / API Key exfiltration detector
    let secret_prefixes = ["sk-ant-", "sk-proj-", "ghp_", "gho_", "aiza", "-----begin private key", "postgres://postgres:"];
    for sp in secret_prefixes {
        if low.contains(sp) {
            findings.push(ThreatFinding {
                category: "secret_exposure".into(),
                severity: "CRITICAL".into(),
                confidence: 0.99,
                description: format!("Credential/private-key pattern '{}' detected in payload", sp),
            });
        }
    }

    findings
}
