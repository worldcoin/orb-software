use std::{fmt, ops::Deref};

/// WiFi network credentials.
#[derive(Debug)]
pub struct Credentials {
    /// Network SSID.
    pub ssid: String,
    /// Password.
    pub password: Option<Password>,
    /// Whether the network SSID is hidden.
    pub hidden: bool,
    pub auth_type: AuthType,
}

/// Authentication type.
#[derive(Default, Clone, Copy, Eq, PartialEq, Debug)]
pub enum AuthType {
    /// WEP encryption.
    Wep,
    /// WPA encryption.
    Wpa,
    /// Pure WPA3-SAE.
    Sae,
    /// Unencrypted.
    #[default]
    Nopass,
}

/// Newtype on `String` to prevent printing in plaintext.
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct Password(pub String);

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl Deref for Password {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq<&str> for Password {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}
