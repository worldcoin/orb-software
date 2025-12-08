use serde_json::Value;

const HIDDEN: &str = "<hidden>";
const REDACTION_FAILED: &str = "<redaction-failed>";
/// Exact key names to redact (case-insensitive). Add more here as needed.
const SENSITIVE_KEYS: &[&str] = &[
    "pwd",
    "password",
    "secret",
    "token",
    "key",
    "psk",
    "credential",
];

/// Redacts sensitive fields from a job document for safe logging.
#[inline]
pub fn redact_job_document(job_document: &str) -> String {
    // Catch any unexpected panics to ensure we never crash the job agent
    match std::panic::catch_unwind(|| redact_job_document_inner(job_document)) {
        Ok(result) => result,
        Err(_) => REDACTION_FAILED.to_string(),
    }
}

fn redact_job_document_inner(job_document: &str) -> String {
    let json_start = find_json_start(job_document);

    let Some(json_start) = json_start else {
        // No JSON found - safe to return as-is
        return job_document.to_string();
    };

    let command_part = &job_document[..json_start];

    // If we found JSON, we MUST either successfully redact it or return a safe fallback.
    // We never return the original JSON portion as it may contain secrets.
    let json_part = &job_document[json_start..];

    let Ok(mut value) = serde_json::from_str::<Value>(json_part) else {
        // JSON parsing failed - return safe fallback
        return format!("{command_part}{REDACTION_FAILED}");
    };

    redact_sensitive_fields(&mut value);

    match serde_json::to_string(&value) {
        Ok(sanitized_json) => format!("{command_part}{sanitized_json}"),
        Err(_) => format!("{command_part}{REDACTION_FAILED}"),
    }
}

#[inline]
fn find_json_start(s: &str) -> Option<usize> {
    let obj_start = s.find('{');
    let arr_start = s.find('[');

    match (obj_start, arr_start) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn redact_sensitive_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *val = Value::String(HIDDEN.to_string());
                } else {
                    redact_sensitive_fields(val);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_sensitive_fields(item);
            }
        }
        _ => {}
    }
}

#[inline]
fn is_sensitive_key(key: &str) -> bool {
    let key_lower = key.to_lowercase();
    SENSITIVE_KEYS.iter().any(|&k| key_lower == k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redacts_pwd_field() {
        let doc = r#"cmd {"name":"foo","pwd":"val1"}"#;
        let sanitized = redact_job_document(doc);

        assert!(sanitized.contains("foo"));
        assert!(sanitized.contains(HIDDEN));
        assert!(!sanitized.contains("val1"));
    }

    #[test]
    fn test_redacts_multiple_sensitive_fields() {
        let doc = r#"cmd {"user":"admin","password":"val1","token":"val2"}"#;
        let sanitized = redact_job_document(doc);

        assert!(sanitized.contains("admin"));
        assert!(!sanitized.contains("val1"));
        assert!(!sanitized.contains("val2"));
        assert_eq!(sanitized.matches(HIDDEN).count(), 2);
    }

    #[test]
    fn test_no_json_unchanged() {
        let doc = "simple_command";
        let sanitized = redact_job_document(doc);
        assert_eq!(sanitized, doc);
    }

    #[test]
    fn test_nested_sensitive_field() {
        let doc = r#"cmd {"config":{"nested":{"pwd":"val1"}}}"#;
        let sanitized = redact_job_document(doc);

        assert!(!sanitized.contains("val1"));
        assert!(sanitized.contains(HIDDEN));
    }

    #[test]
    fn test_array_with_sensitive_fields() {
        let doc = r#"cmd [{"pwd":"val1"},{"pwd":"val2"}]"#;
        let sanitized = redact_job_document(doc);

        assert!(!sanitized.contains("val1"));
        assert!(!sanitized.contains("val2"));
        assert_eq!(sanitized.matches(HIDDEN).count(), 2);
    }

    #[test]
    fn test_invalid_json_returns_safe_fallback() {
        let doc = r#"cmd {invalid json"#;
        let sanitized = redact_job_document(doc);

        assert!(sanitized.contains("cmd"));
        assert!(sanitized.contains(REDACTION_FAILED));
    }

    #[test]
    fn test_case_insensitive_key_matching() {
        let doc = r#"cmd {"PWD":"val1","Password":"val2","SECRET":"val3"}"#;
        let sanitized = redact_job_document(doc);

        assert!(!sanitized.contains("val1"));
        assert!(!sanitized.contains("val2"));
        assert!(!sanitized.contains("val3"));
    }

    #[test]
    fn test_empty_json() {
        assert_eq!(redact_job_document("cmd {}"), "cmd {}");
        assert_eq!(redact_job_document("cmd []"), "cmd []");
    }

    #[test]
    fn test_json_without_sensitive_fields() {
        let doc = r#"cmd {"name":"test","value":123}"#;
        let sanitized = redact_job_document(doc);
        assert!(sanitized.contains("test"));
        assert!(!sanitized.contains(HIDDEN));
    }

    #[test]
    fn test_deeply_nested_returns_fallback() {
        let mut json = r#"{"a":"#.repeat(150);
        json.push_str(r#""x""#);
        json.push_str(&"}".repeat(150));

        let sanitized = redact_job_document(&format!("cmd {json}"));
        assert!(sanitized.contains(REDACTION_FAILED));
    }
}
