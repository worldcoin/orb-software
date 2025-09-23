use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct TagEntry {
    pub tag: String,
    pub hash: Vec<u8>,
    pub signature: Vec<u8>,
    pub cert: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HashMetadata {
    pub hash: Vec<u8>,
    pub pinned_at: String,
    pub size_bytes: Option<i64>,
    pub notes: Option<String>,
}
