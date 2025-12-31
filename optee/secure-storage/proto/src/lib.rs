#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum CommandId {
    Put = 1,
    Get = 2,
}

pub trait ResponseT: Sized {
    type DeserializeErr: core::error::Error + Send + Sync + 'static;
    fn deserialize<B: AsRef<[u8]>>(buf: B) -> Result<Self, Self::DeserializeErr>;
    fn serialize(&self, out_buf: &mut [u8]) -> Result<usize, BufferTooSmallErr>;
}

pub trait RequestT: Sized + serde::Serialize + for<'a> serde::Deserialize<'a> {
    const MAX_RESPONSE_SIZE: u32;
    type Response: ResponseT;
    fn id(&self) -> CommandId;
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Put(PutRequest),
    Get(GetRequest),
}

impl Request {
    pub fn id(&self) -> CommandId {
        match self {
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

#[derive(Debug, thiserror::Error)]
#[error("could not deserialize because the provided buffer was too small")]
pub struct BufferTooSmallErr;

#[derive(Debug, Serialize, Deserialize)]
pub struct PutRequest {
    pub key: String,
    pub val: Vec<u8>,
}

impl RequestT for PutRequest {
    const MAX_RESPONSE_SIZE: u32 = 1024;

    type Response = PutResponse;

    fn id(&self) -> CommandId {
        CommandId::Put
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PutResponse {
    // TODO: Make it an option
    pub prev_val: Vec<u8>, // returns the previously stored value (or an empty vec)
}

impl ResponseT for PutResponse {
    type DeserializeErr = core::convert::Infallible;

    fn deserialize<B: AsRef<[u8]>>(buf: B) -> Result<Self, Self::DeserializeErr> {
        Ok(PutResponse {
            prev_val: buf.as_ref().to_vec(),
        })
    }

    fn serialize(&self, out_buf: &mut [u8]) -> Result<usize, BufferTooSmallErr> {
        let nbytes = self.prev_val.len();
        if out_buf.len() < nbytes {
            return Err(BufferTooSmallErr);
        }
        let out_buf = &mut out_buf[0..nbytes];
        out_buf.copy_from_slice(&self.prev_val);

        Ok(nbytes)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetRequest {
    pub key: String,
}

impl RequestT for GetRequest {
    const MAX_RESPONSE_SIZE: u32 = 1024;

    type Response = GetResponse;

    fn id(&self) -> CommandId {
        CommandId::Get
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetResponse {
    // TODO: Make it an option
    pub val: Vec<u8>,
}

impl ResponseT for GetResponse {
    type DeserializeErr = core::convert::Infallible;

    fn deserialize<B: AsRef<[u8]>>(buf: B) -> Result<Self, Self::DeserializeErr> {
        Ok(GetResponse {
            val: buf.as_ref().to_vec(),
        })
    }

    fn serialize(&self, out_buf: &mut [u8]) -> Result<usize, BufferTooSmallErr> {
        let nbytes = self.val.len();
        if out_buf.len() < nbytes {
            return Err(BufferTooSmallErr);
        }
        let out_buf = &mut out_buf[0..nbytes];
        out_buf.copy_from_slice(&self.val);

        Ok(nbytes)
    }
}

/// Different domains for storage. Each domain maps to a different TA with its storage
/// isolated from other domains.
#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub enum StorageDomain {
    WifiProfiles,
}

impl StorageDomain {
    pub const fn as_uuid(&self) -> &'static str {
        match self {
            // If Uuid::parse_str() returns an InvalidLength error, there may be an extra
            // newline in your uuid.txt file. You can remove it by running
            // `truncate -s 36 uuid.txt`.
            StorageDomain::WifiProfiles => include_str!("../../uuid.txt"),
        }
    }
}
