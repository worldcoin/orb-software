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
    combinator::{fail, map},
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
#[derive(Clone, Copy, Eq, PartialEq, Debug, Default)]
pub enum Auth {
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
    use proptest::prelude::*;

    fn auth_str() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("WEP"),
            Just("WPA"),
            Just("SAE"),
            Just("nopass"),
        ]
        .prop_map(|t| format!("T:{t};"))
    }

    fn ssid_str() -> impl Strategy<Value = String> {
        "[A-Za-z0-9_]{1,16}".prop_map(|s| format!("S:{s};"))
    }

    fn psk_str() -> impl Strategy<Value = String> {
        "[A-Za-z0-9_]{1,16}".prop_map(|s| format!("P:{s};"))
    }

    fn hidden_str() -> impl Strategy<Value = String> {
        any::<bool>().prop_map(|b| format!("H:{};", if b { "true" } else { "false" }))
    }

    fn parts_strategy() -> impl Strategy<Value = Vec<String>> {
        (auth_str(), ssid_str(), psk_str(), hidden_str())
            .prop_map(|(t, s, p, h)| vec![t, s, p, h])
    }

    fn parts_without_ssid_strategy() -> impl Strategy<Value = Vec<String>> {
        (auth_str(), psk_str(), hidden_str()).prop_map(|(t, p, h)| vec![t, p, h])
    }

    fn parts_with_empty_ssid_strategy() -> impl Strategy<Value = Vec<String>> {
        (auth_str(), psk_str(), hidden_str()).prop_map(|(t, p, h)| {
            vec![t, "S:;".to_string(), p, h]
        })
    }

    fn parts_with_empty_psk_strategy() -> impl Strategy<Value = Vec<String>> {
        (auth_str(), ssid_str(), hidden_str()).prop_map(|(t, s, h)| {
            vec![t, s, "P:;".to_string(), h]
        })
    }

    fn parts_without_psk_strategy() -> impl Strategy<Value = Vec<String>> {
        (auth_str(), ssid_str(), hidden_str()).prop_map(|(t, s, h)| vec![t, s, h])
    }

    fn parts_without_auth_strategy() -> impl Strategy<Value = Vec<String>> {
        (ssid_str(), psk_str(), hidden_str()).prop_map(|(s, p, h)| vec![s, p, h])
    }

    fn parts_without_hidden_strategy() -> impl Strategy<Value = Vec<String>> {
        (auth_str(), ssid_str(), psk_str()).prop_map(|(t, s, p)| vec![t, s, p])
    }

    fn parts_with_empty_hidden_strategy() -> impl Strategy<Value = Vec<String>> {
        (auth_str(), ssid_str(), psk_str()).prop_map(|(t, s, p)| {
            vec![t, s, p, "H:;".to_string()]
        })
    }

    fn parts_with_duplicates_strategy() -> impl Strategy<Value = (Vec<String>, Vec<String>)> {
        (
            auth_str(),
            ssid_str(),
            psk_str(),
            hidden_str(),
            auth_str(),
            ssid_str(),
            psk_str(),
            hidden_str(),
        )
            .prop_map(|(t1, s1, p1, h1, t2, s2, p2, h2)| {
                let first = vec![t1, s1, p1, h1];
                let dupes = vec![t2, s2, p2, h2];
                (first, dupes)
            })
    }

    fn duplicated_parts_shuffled_strategy() -> impl Strategy<Value = (Vec<String>, Vec<String>)> {
        parts_with_duplicates_strategy().prop_flat_map(|(first, dupes)| {
            let first_shuffled = Just(first).prop_shuffle();
            let dupes_shuffled = Just(dupes).prop_shuffle();
            (first_shuffled, dupes_shuffled).prop_map(|(first, dupes)| {
                let expected = first.clone();
                let parts = first.into_iter().chain(dupes.into_iter()).collect();
                (expected, parts)
            })
        })
    }

    fn shuffled_parts_strategy() -> impl Strategy<Value = (Vec<String>, Vec<String>)> {
        parts_strategy().prop_flat_map(|parts| {
            let original = parts.clone();
            Just(parts)
                .prop_shuffle()
                .prop_map(move |shuffled| (original.clone(), shuffled))
        })
    }

    fn build_mecard(parts: &[String]) -> String {
        let mut s = String::from("WIFI:");
        for p in parts {
            s.push_str(p);
        }
        s.push(';');
        s
    }

    proptest! {
        #[test]
        fn prop_field_order_invariance((parts, shuffled) in shuffled_parts_strategy()) {
            let expected = Credentials::parse(&build_mecard(&parts)).unwrap();
            let got = Credentials::parse(&build_mecard(&shuffled)).unwrap();
            prop_assert_eq!(got, expected);
        }

        #[test]
        fn prop_trailing_garbage_is_ignored(
            parts in parts_strategy(),
            garbage in proptest::string::string_regex("(?s).{0,32}").unwrap(),
        ) {
            let base = build_mecard(&parts);
            let expected = Credentials::parse(&base).unwrap();
            let got = Credentials::parse(&format!("{base}{garbage}")).unwrap();
            prop_assert_eq!(got, expected);
        }

        #[test]
        fn prop_missing_ssid_always_errors(parts in parts_without_ssid_strategy()) {
            let input = build_mecard(&parts);
            prop_assert!(Credentials::parse(&input).is_err());
        }

        #[test]
        fn prop_empty_ssid_always_errors(parts in parts_with_empty_ssid_strategy()) {
            let input = build_mecard(&parts);
            prop_assert!(Credentials::parse(&input).is_err());
        }

        #[test]
        fn prop_empty_psk_forces_nopass(parts in parts_with_empty_psk_strategy()) {
            let input = build_mecard(&parts);
            let credentials = Credentials::parse(&input).unwrap();
            prop_assert!(credentials.psk.is_none());
            prop_assert_eq!(credentials.auth, Auth::Nopass);
        }

        #[test]
        fn prop_missing_psk_defaults_to_nopass(parts in parts_without_psk_strategy()) {
            let input = build_mecard(&parts);
            let credentials = Credentials::parse(&input).unwrap();
            prop_assert!(credentials.psk.is_none());
            prop_assert_eq!(credentials.auth, Auth::Nopass);
        }

        #[test]
        fn prop_missing_auth_defaults_to_nopass(parts in parts_without_auth_strategy()) {
            let input = build_mecard(&parts);
            let credentials = Credentials::parse(&input).unwrap();
            prop_assert!(credentials.psk.is_some());
            prop_assert_eq!(credentials.auth, Auth::Nopass);
        }

        #[test]
        fn prop_missing_hidden_defaults_to_false(parts in parts_without_hidden_strategy()) {
            let input = build_mecard(&parts);
            let credentials = Credentials::parse(&input).unwrap();
            prop_assert!(!credentials.hidden);
        }

        #[test]
        fn prop_empty_hidden_defaults_to_false(parts in parts_with_empty_hidden_strategy()) {
            let input = build_mecard(&parts);
            let credentials = Credentials::parse(&input).unwrap();
            prop_assert!(!credentials.hidden);
        }

        #[test]
        fn prop_duplicates_first_wins(
            (expected_parts, parts) in duplicated_parts_shuffled_strategy(),
        ) {
            let expected = Credentials::parse(&build_mecard(&expected_parts)).unwrap();
            let got = Credentials::parse(&build_mecard(&parts)).unwrap();
            prop_assert_eq!(got, expected);
        }
    }

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
    fn test_simple_permissive_semicolon() {
        // single semicolon in the end
        let input = "WIFI:T:WPA;S:mynetwork;P:mypass;";
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
    fn test_empty_tags() {
        let input = r"WIFI:T:;S:hotspotname;P:;H:;;";
        let credentials = Credentials::parse(input).unwrap();
        assert_eq!(credentials.ssid, "hotspotname");
        assert_eq!(credentials.psk, None);
        assert_eq!(credentials.auth, Auth::Nopass);
        assert!(!credentials.hidden);
    }
}
