#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum CommandId {
    Ping = 1,
    Put = 2,
    Get = 3,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Ping,
    Put(PutRequest),
    Get(GetRequest),
}

impl Request {
    pub fn id(&self) -> CommandId {
        match self {
            Request::Ping => CommandId::Ping,
            Request::Put(_) => CommandId::Put,
            Request::Get(_) => CommandId::Get,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Ping,
    Put(PutResponse),
    Get(GetResponse),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PutRequest {
    pub key: String,
    pub val: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PutResponse {
    pub prev_val: Option<Vec<u8>>, // returns the previously stored value, if there was any
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetRequest {
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetResponse {
    pub val: Option<Vec<u8>>,
}

// If Uuid::parse_str() returns an InvalidLength error, there may be an extra
// newline in your uuid.txt file. You can remove it by running
// `truncate -s 36 uuid.txt`.
pub const UUID: &str = include_str!("../../uuid.txt");
