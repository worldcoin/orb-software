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

/// Commands that need their args sanitized before logging.
const COMMANDS_TO_SANITIZE: &[&str] = &["wifi_add"];

/// Returns true if the given command needs its args sanitized.
pub fn should_sanitize_args(cmd: &str) -> bool {
    COMMANDS_TO_SANITIZE.contains(&cmd)
}

/// Redacts sensitive fields from raw args for safe logging.
/// On error, returns REDACTION_FAILED.
#[inline]
pub fn redact_args(s: &str) -> String {
    redact_json_inner(s, false)
}

/// Redacts sensitive fields from a job document for safe logging.
/// On error, preserves the command prefix so we know which command failed.
/// Wraps with panic protection to ensure we never crash the job agent.
#[inline]
pub fn redact_job_document(job_document: &str) -> String {
    match std::panic::catch_unwind(|| redact_json_inner(job_document, true)) {
        Ok(result) => result,
        Err(_) => REDACTION_FAILED.to_string(),
    }
}

fn redact_json_inner(s: &str, preserve_prefix_on_error: bool) -> String {
    let Some(json_start) = find_json_start(s) else {
        return s.to_string();
    };

    let prefix = &s[..json_start];
    let json_part = &s[json_start..];

    let Ok(mut value) = serde_json::from_str::<Value>(json_part) else {
        return if preserve_prefix_on_error {
            format!("{prefix}{REDACTION_FAILED}")
        } else {
            REDACTION_FAILED.to_string()
        };
    };

    redact_sensitive_fields(&mut value);

    match serde_json::to_string(&value) {
        Ok(sanitized_json) => format!("{prefix}{sanitized_json}"),
        Err(_) if preserve_prefix_on_error => format!("{prefix}{REDACTION_FAILED}"),
        Err(_) => REDACTION_FAILED.to_string(),
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
    fn test_command_with_json_args() {
        // Test redact_job_document (full job document with command)
        let doc = r#"some_cmd {"name":"visible","token":"secret123"}"#;
        let sanitized = redact_job_document(doc);
        assert!(sanitized.contains("some_cmd"));
        assert!(sanitized.contains("visible"));
        assert!(sanitized.contains(HIDDEN));
        assert!(!sanitized.contains("secret123"));

        // Test redact_args (just the args portion)
        let args = r#"{"name":"visible","token":"secret123"}"#;
        let sanitized_args = redact_args(args);
        assert!(sanitized_args.contains("visible"));
        assert!(sanitized_args.contains(HIDDEN));
        assert!(!sanitized_args.contains("secret123"));
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
