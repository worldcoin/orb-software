use std::{fmt::Display, time::SystemTime};

use serde_json::Value;

/// ANSI color codes for terminal output
const RESET: &str = "\x1b[0m";
const BLUE: &str = "\x1b[34m";
const ROSE: &str = "\x1b[38;5;204m";

/// Returns the current timestamp colored in yellow
pub fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    // Format as HH:MM:SS.mmm
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{ROSE}{hours:02}:{minutes:02}:{seconds:02}.{millis:03}{RESET}")
}

/// Colorizes a key expression in blue
pub fn key_expr(key_expr: impl Display) -> String {
    format!("{BLUE}{}{RESET}", key_expr)
}

/// ANSI color codes for JSON syntax highlighting
const JSON_KEY: &str = "\x1b[36m"; // Cyan for keys
const JSON_STRING: &str = "\x1b[32m"; // Green for string values
const JSON_NUMBER: &str = "\x1b[33m"; // Yellow for numbers
const JSON_BOOL: &str = "\x1b[35m"; // Magenta for booleans
const JSON_NULL: &str = "\x1b[90m"; // Gray for null

/// Colorizes JSON output with syntax highlighting (compact, no newlines)
pub fn json(json_str: &str) -> String {
    match serde_json::from_str::<Value>(json_str) {
        Ok(val) => value(&val),
        Err(_) => json_str.to_string(), // Return as-is if not valid JSON
    }
}

pub fn value(val: &Value) -> String {
    match val {
        Value::Null => format!("{JSON_NULL}null{RESET}"),
        Value::Bool(b) => format!("{JSON_BOOL}{b}{RESET}"),
        Value::Number(n) => format!("{JSON_NUMBER}{n}{RESET}"),
        Value::String(s) => format!("{JSON_STRING}\"{s}\"{RESET}"),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(obj) => {
            let items: Vec<String> = obj
                .iter()
                .map(|(k, v)| format!("{JSON_KEY}\"{k}\"{RESET}: {}", value(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}
