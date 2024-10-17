use std::io;

use serde::{de::DeserializeSeed, Deserialize};
use serde_json::Deserializer;

// pub type Error = serde_path_to_error::Error<serde_json::Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to deserialize (seedless)")]
    Deserialize(#[source] serde_path_to_error::Error<serde_json::Error>),
    #[error("failed to deserialize with seed")]
    DeserializeSeed(#[source] serde_json::Error),
}

pub fn deserialize<'de, R, T>(reader: R) -> Result<T, Error>
where
    R: io::Read,
    T: Deserialize<'de>,
{
    let json_deserializer = &mut Deserializer::from_reader(reader);
    serde_path_to_error::deserialize(json_deserializer).map_err(Error::Deserialize)
}

pub fn deserialize_seed<'de, S, R, T>(seed: S, reader: R) -> Result<T, Error>
where
    S: DeserializeSeed<'de, Value = T>,
    R: io::Read,
{
    let json_deserializer = &mut Deserializer::from_reader(reader);
    seed.deserialize(json_deserializer)
        .map_err(Error::DeserializeSeed)
}
