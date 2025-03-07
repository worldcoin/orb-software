use std::fmt;
use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DeviceType {
    Xavier,
    Orin,
}

impl fmt::Display for DeviceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceType::Xavier => write!(f, "xavier"),
            DeviceType::Orin => write!(f, "orin"),
        }
    }
}
