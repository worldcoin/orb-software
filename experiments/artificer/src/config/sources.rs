//! The different supported artifact sources

use serde::{Deserialize, Serialize};

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Hash, Clone)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "source")]
pub enum Source {
    Github(Github),
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Hash, Clone)]
pub struct Github {
    pub repo: String,
    pub tag: String,
    pub artifact: String,
}
