use serde::{Deserialize, Serialize};

// TODO: Switch to rkyv instead of cbor

#[derive(Debug, Serialize, Deserialize)]
pub(super) enum Request {
    Put { key: String, val: Vec<u8> },
    Get { key: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) enum Response {
    Put(Result<Option<Vec<u8>>, PutErr>),
    Get(Result<Option<Vec<u8>>, GetErr>),
}

#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
pub(super) enum PutErr {
    #[error("{0}")]
    Generic(String),
}

#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
pub(super) enum GetErr {
    #[error("{0}")]
    Generic(String),
}
