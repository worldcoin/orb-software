use serde_json::Value;

const HIDDEN: &str = "<hidden>";
const REDACTION_FAILED: &str = "<redaction-failed>";

const SENSITIVE_KEYS: &[&str] = &[
    "pwd",
    "password",
    "secret",
    "token",
    "key",
    "psk",
    "credential",
];

const COMMANDS_TO_SANITIZE: &[&str] = &["wifi_add"];

pub fn should_sanitize(cmd: &str) -> bool {
    COMMANDS_TO_SANITIZE.contains(&cmd)
}

/// Redacts sensitive fields from JSON args. Caller must check should_sanitize() first.
pub fn redact_args(args: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(args) else {
        return REDACTION_FAILED.to_string();
    };
    redact_sensitive_fields(&mut value);
    serde_json::to_string(&value).unwrap_or_else(|_| REDACTION_FAILED.to_string())
}

/// Redacts sensitive fields from a job document (e.g., "wifi_add {\"pwd\":\"secret\"}").
/// Only redacts for commands in COMMANDS_TO_SANITIZE. Others pass through unchanged.
pub fn redact_job_document(doc: &str) -> String {
    let cmd = doc.split_whitespace().next().unwrap_or("");
    if !should_sanitize(cmd) {
        return doc.to_string();
    }

    let json_start = match (doc.find('{'), doc.find('[')) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => return doc.to_string(),
    };

    let prefix = &doc[..json_start];
    let json_part = &doc[json_start..];

    let Ok(mut value) = serde_json::from_str::<Value>(json_part) else {
        return format!("{prefix}{REDACTION_FAILED}");
    };

    redact_sensitive_fields(&mut value);

    match serde_json::to_string(&value) {
        Ok(json) => format!("{prefix}{json}"),
        Err(_) => format!("{prefix}{REDACTION_FAILED}"),
    }
}

fn redact_sensitive_fields(value: &mut Value) {
    let mut stack = vec![value];
    while let Some(current) = stack.pop() {
        match current {
            Value::Object(map) => {
                for (key, val) in map.iter_mut() {
                    if SENSITIVE_KEYS.iter().any(|&k| key.eq_ignore_ascii_case(k)) {
                        *val = Value::String(HIDDEN.to_string());
                    } else {
                        stack.push(val);
                    }
                }
            }
            Value::Array(arr) => stack.extend(arr.iter_mut()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redacts_pwd_field() {
        let doc = r#"wifi_add {"name":"foo","pwd":"val1"}"#;
        let sanitized = redact_job_document(doc);

        assert!(sanitized.contains("foo"));
        assert!(sanitized.contains(HIDDEN));
        assert!(!sanitized.contains("val1"));
    }

    #[test]
    fn test_redacts_multiple_sensitive_fields() {
        let doc = r#"wifi_add {"user":"admin","password":"val1","token":"val2"}"#;
        let sanitized = redact_job_document(doc);

        assert!(sanitized.contains("admin"));
        assert!(!sanitized.contains("val1"));
        assert!(!sanitized.contains("val2"));
        assert_eq!(sanitized.matches(HIDDEN).count(), 2);
    }

    #[test]
    fn test_non_sanitized_command_unchanged() {
        let doc = r#"other_cmd {"pwd":"val1"}"#;
        let sanitized = redact_job_document(doc);
        assert_eq!(sanitized, doc);
    }

    #[test]
    fn test_nested_sensitive_field() {
        let doc = r#"wifi_add {"config":{"nested":{"pwd":"val1"}}}"#;
        let sanitized = redact_job_document(doc);

        assert!(!sanitized.contains("val1"));
        assert!(sanitized.contains(HIDDEN));
    }

    #[test]
    fn test_array_with_sensitive_fields() {
        let doc = r#"wifi_add [{"pwd":"val1"},{"pwd":"val2"}]"#;
        let sanitized = redact_job_document(doc);

        assert!(!sanitized.contains("val1"));
        assert!(!sanitized.contains("val2"));
        assert_eq!(sanitized.matches(HIDDEN).count(), 2);
    }

    #[test]
    fn test_invalid_json_returns_safe_fallback() {
        let doc = r#"wifi_add {invalid json"#;
        let sanitized = redact_job_document(doc);

        assert!(sanitized.contains("wifi_add"));
        assert!(sanitized.contains(REDACTION_FAILED));
    }

    #[test]
    fn test_case_insensitive_key_matching() {
        let doc = r#"wifi_add {"PWD":"val1","Password":"val2","SECRET":"val3"}"#;
        let sanitized = redact_job_document(doc);

        assert!(!sanitized.contains("val1"));
        assert!(!sanitized.contains("val2"));
        assert!(!sanitized.contains("val3"));
    }

    #[test]
    fn test_empty_json() {
        assert_eq!(redact_job_document("wifi_add {}"), "wifi_add {}");
        assert_eq!(redact_job_document("wifi_add []"), "wifi_add []");
    }

    #[test]
    fn test_wifi_add_without_sensitive_fields() {
        let doc = r#"wifi_add {"ssid":"test","hidden":false}"#;
        let sanitized = redact_job_document(doc);
        assert!(sanitized.contains("test"));
        assert!(!sanitized.contains(HIDDEN));
    }

    #[test]
    fn test_redacts_sensitive_preserves_other_fields() {
        let doc = r#"wifi_add {"a":"visible","b":"wpa2","pwd":"redact_me","c":false}"#;
        let sanitized = redact_job_document(doc);
        assert!(sanitized.contains("visible"));
        assert!(sanitized.contains(HIDDEN));
        assert!(!sanitized.contains("redact_me"));
    }

    #[test]
    fn test_deeply_nested_returns_fallback() {
        let mut json = r#"{"a":"#.repeat(150);
        json.push_str(r#""x""#);
        json.push_str(&"}".repeat(150));

        let sanitized = redact_job_document(&format!("wifi_add {json}"));
        assert!(sanitized.contains(REDACTION_FAILED));
    }
}
