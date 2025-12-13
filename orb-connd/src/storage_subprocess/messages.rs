use serde::{Deserialize, Serialize};

// TODO: Switch to rkyv instead of cbor

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Put { key: String, val: Vec<u8> },
    Get { key: String },
}

pub enum Response {
    Put(Result<(), PutErr>),
    Get(Result<Vec<u8>, GetErr>),
}

#[derive(Debug, thiserror::Error)]
pub enum PutErr {
    #[error("{0}")]
    Generic(String),
}

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum GetErr {
    #[error("{0}")]
    Generic(String),
}
