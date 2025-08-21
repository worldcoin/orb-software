#[derive(Debug, Clone)]
pub enum ConnectionState {
    Connected,
    Connecting,
    Registered,
    Searching,
    Disconnecting,
    Enabling,
    Enabled,
    Disabled,
    Failed,
    Locked,
    Unknown(String),
}

impl<T> From<T> for ConnectionState
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        let value = value.into();

        match value.as_str() {
            "connected" => ConnectionState::Connected,
            "connecting" => ConnectionState::Connecting,
            "registered" => ConnectionState::Registered,
            "searching" => ConnectionState::Searching,
            "disconnecting" => ConnectionState::Disconnecting,
            "enabling" => ConnectionState::Enabling,
            "enabled" => ConnectionState::Enabled,
            "disabled" => ConnectionState::Disabled,
            "failed" => ConnectionState::Failed,
            "locked" => ConnectionState::Locked,
            _ => ConnectionState::Unknown(value),
        }
    }
}

impl AsRef<str> for ConnectionState {
    fn as_ref(&self) -> &str {
        match self {
            ConnectionState::Connected => "connected",
            ConnectionState::Connecting => "connecting",
            ConnectionState::Registered => "registered",
            ConnectionState::Searching => "searching",
            ConnectionState::Disconnecting => "disconnecting",
            ConnectionState::Enabling => "enabling",
            ConnectionState::Enabled => "enabled",
            ConnectionState::Disabled => "disabled",
            ConnectionState::Failed => "failed",
            ConnectionState::Locked => "locked",
            ConnectionState::Unknown(v) => v.as_str(),
        }
    }
}

impl ConnectionState {
    pub fn is_online(&self) -> bool {
        matches!(self, ConnectionState::Connected)
    }
}
