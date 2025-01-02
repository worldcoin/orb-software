use color_eyre::eyre;
use derive_more::From;
use tonic::transport;

pub mod client;

#[derive(From, Debug)]
pub enum Err {
    Transport(transport::Error),
    Tonic(tonic::Status),
    Other(eyre::Error),
}
