//! MECARD format for WiFi credentials.
//!
//! Spec:
//! <https://github.com/zxing/zxing/wiki/Barcode-Contents#wi-fi-network-config-android-ios-11>
use super::mecard::{parse_bool, parse_field, parse_string};
use crate::service::mecard;
use color_eyre::eyre::eyre;
use color_eyre::Result;
use nom::{
    branch::alt,
    bytes::complete::tag,
    combinator::{eof, fail, map},
    IResult,
};
use std::{
    fmt::{self, Debug},
    ops::Deref,
    str,
};

/// WiFi network credentials.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Credentials {
    /// Authentication type.
    pub auth: Auth,
    /// Network SSID.
    pub ssid: String,
    /// Password.
    pub psk: Option<Password>,
    /// Whether the network SSID is hidden.
    pub hidden: bool,
}

/// Authentication type.
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum Auth {
    /// WEP encryption.
    Wep,
    /// WPA encryption.
    Wpa,
    /// Pure WPA3-SAE.
    Sae,
    /// Unencrypted.
    Nopass,
}

impl Default for Auth {
    fn default() -> Self {
        Self::Nopass
    }
}

/// Newtype on `String` to prevent printing in plaintext.
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct Password(pub String);

impl From<&str> for Password {
    fn from(value: &str) -> Self {
        Password(value.to_string())
    }
}

impl Debug for Password {
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

impl Credentials {
    pub fn parse(input: &str) -> Result<Credentials> {
        match Self::parse_internal(input) {
            Err(e) => Err(eyre!("{e}")),
            Ok((_, c)) => Ok(c),
        }
    }

    /// Parses WiFi credentials encoded in MECARD format.
    pub(crate) fn parse_internal(input: &str) -> IResult<&str, Self> {
        let (mut input, _) = tag("WIFI:")(input)?;

        mecard::parse_fields! { input;
            Auth::parse => auth_type,
            parse_ssid => ssid,
            parse_password => password,
            parse_hidden => hidden,
        }

        let ssid = ssid.filter(|ssid| !ssid.is_empty());
        let (password, auth_type) = password
            .filter(|pwd| !pwd.is_empty())
            .map_or((None, Some(Auth::Nopass)), |pwd| {
                (Some(Password(pwd)), auth_type)
            });

        // ssid is actually not optional.
        if ssid.is_none() {
            let (_, ()) = fail(input)?;
        }

        let (input, _) = tag(";")(input)?;
        let (input, _) = eof(input)?;

        let auth_type = auth_type.unwrap_or_default();
        let ssid = ssid.unwrap_or_default();
        let hidden = hidden.unwrap_or_default();

        Ok((
            input,
            Self {
                auth: auth_type,
                ssid,
                psk: password,
                hidden,
            },
        ))
    }
}

impl Auth {
    pub fn parse(input: &str) -> IResult<&str, Self> {
        parse_field(input, "T", |input| {
            let wep = map(tag("WEP"), |_| Self::Wep);
            let wpa = map(tag("WPA"), |_| Self::Wpa);
            let sae = map(tag("SAE"), |_| Self::Sae);
            let nopass = map(alt((tag("nopass"), tag(""))), |_| Self::Nopass);
            alt((wep, wpa, sae, nopass))(input)
        })
    }
}

pub fn parse_ssid(input: &str) -> IResult<&str, String> {
    parse_field(input, "S", parse_string)
}

pub fn parse_password(input: &str) -> IResult<&str, String> {
    parse_field(input, "P", parse_string)
}

pub fn parse_hidden(input: &str) -> IResult<&str, bool> {
    parse_field(input, "H", parse_bool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple() {
        let input = "WIFI:T:WPA;S:mynetwork;P:mypass;;";
        let credentials = Credentials::parse(input).unwrap();
        assert_eq!(credentials.auth, Auth::Wpa);
        assert_eq!(credentials.ssid, "mynetwork");
        assert_eq!(credentials.psk.unwrap(), "mypass");
        assert!(!credentials.hidden);
    }

    #[test]
    fn test_escaped() {
        let input = r#"WIFI:S:\"foo\;bar\\baz\";;"#;
        let credentials = Credentials::parse(input).unwrap();
        assert_eq!(credentials.auth, Auth::Nopass);
        assert_eq!(credentials.ssid, r#""foo;bar\baz""#);
        assert_eq!(credentials.psk, None);
        assert!(!credentials.hidden);
    }

    #[test]
    fn test_quoted() {
        let input = r#"WIFI:S:"\"foo\;bar\\baz\"";P:"mypass";;"#;
        let credentials = Credentials::parse(input).unwrap();
        assert_eq!(credentials.auth, Auth::Nopass);
        assert_eq!(credentials.ssid, r#""foo;bar\baz""#);
        assert_eq!(credentials.psk.unwrap(), "mypass");
        assert!(!credentials.hidden);
    }

    #[test]
    fn test_unescaped() {
        let input = r#"WIFI:S:"foo;bar\baz";;"#;
        assert!(Credentials::parse(input).is_err());
    }

    #[test]
    fn test_different_order() {
        let input = "WIFI:P:mypass;H:true;S:mynetwork;T:WPA;;";
        let credentials = Credentials::parse(input).unwrap();
        assert_eq!(credentials.auth, Auth::Wpa);
        assert_eq!(credentials.ssid, "mynetwork");
        assert_eq!(credentials.psk.unwrap(), "mypass");
        assert!(credentials.hidden);
    }

    #[test]
    fn test_missing_ssid() {
        let input = "WIFI:P:mypass;T:WPA;H:true;;";
        assert!(Credentials::parse(input).is_err());

        let input = "WIFI:P:mypass;S:;T:WPA;H:true;;";
        assert!(Credentials::parse(input).is_err());
    }

    #[test]
    fn test_duplicates() {
        let input = "WIFI:H:true;P:mypass;T:WPA;S:mynetwork;P:mypass;;";
        assert!(Credentials::parse(input).is_err());
    }

    #[test]
    fn test_trailing_garbage() {
        let input = "WIFI:T:WPA;S:mynetwork;P:mypass;;garbage";
        assert!(Credentials::parse(input).is_err());
    }

    #[test]
    fn test_hex_string() {
        let input = r"WIFI:S:worldcoin;P:5265616461626c6520746578742065786163746c792033322062797465732e21;;";
        let credentials = Credentials::parse(input).unwrap();
        assert_eq!(credentials.auth, Auth::Nopass);
        assert_eq!(credentials.ssid, "worldcoin");
        assert_eq!(
            credentials.psk,
            Some("Readable text exactly 32 bytes.!".into())
        );
        assert!(!credentials.hidden);
    }

    #[test]
    fn test_empty_password() {
        let input = r"WIFI:S:hotspotname;T:nopass;P:;H:false;;";
        let credentials = Credentials::parse(input).unwrap();
        assert_eq!(credentials.ssid, "hotspotname");
        assert_eq!(credentials.psk, None);
        assert_eq!(credentials.auth, Auth::Nopass);

        let input = r"WIFI:S:hotspotname;T:nopass;H:false;;";
        let credentials = Credentials::parse(input).unwrap();
        assert_eq!(credentials.ssid, "hotspotname");
        assert_eq!(credentials.psk, None);
        assert_eq!(credentials.auth, Auth::Nopass);

        let input = r"WIFI:S:hotspotname;T:WPA;P:;;";
        let credentials = Credentials::parse(input).unwrap();
        assert_eq!(credentials.ssid, "hotspotname");
        assert_eq!(credentials.psk, None);
        assert_eq!(credentials.auth, Auth::Nopass);
    }

    #[test]
    fn test_empty_tags() {
        let input = r"WIFI:T:;S:hotspotname;P:;H:;;";
        let credentials = Credentials::parse(input).unwrap();
        assert_eq!(credentials.ssid, "hotspotname");
        assert_eq!(credentials.psk, None);
        assert_eq!(credentials.auth, Auth::Nopass);
        assert!(!credentials.hidden);
    }
}
