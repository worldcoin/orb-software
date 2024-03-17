#![forbid(unsafe_code)]

use http::header::{AGE, CACHE_CONTROL};
use std::time::Duration;

/// Parses the `max-age=<number of seconds>` value from the [`CACHE_CONTROL`] header.
pub fn parse_max_age(cache_control_value: &http::HeaderValue) -> Option<u64> {
    let s = cache_control_value.to_str().ok()?;
    s.split(',').map(str::trim).find_map(|s| {
        s.split_once("max-age=")
            .and_then(|(_front, back)| back.parse::<u64>().ok())
    })
}

// ---- Helpers for parsing Cache-Control header
/// Extracts the age and max-age in seconds from the [`AGE`] and [`CACHE_CONTROL`] headers. Then
/// subtracts them to find the time until the age specified by `max-age` is reached.
pub fn time_until_max_age(headers: &http::header::HeaderMap) -> Option<Duration> {
    let max_age = headers.get(CACHE_CONTROL).and_then(parse_max_age)?;
    let age = headers
        .get(AGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let remaining_age = max_age.saturating_sub(age);
    Some(Duration::from_secs(remaining_age))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::{HeaderMap, HeaderValue};

    #[test]
    fn test_time_until_max_age() {
        fn hm(age: &str, max_age: &str) -> HeaderMap {
            let mut m = HeaderMap::new();
            m.insert(AGE, HeaderValue::from_str(age).unwrap());
            m.insert(
                CACHE_CONTROL,
                HeaderValue::from_str(&format!("max-age={max_age}")).unwrap(),
            );
            m
        }

        let test_cases = [
            (hm("0", "10"), Some(10)),
            (hm("10", "0"), Some(0)),
            (hm("1.0", "10"), Some(10)),
            (hm("0", "10.0"), None),
            (HeaderMap::new(), None),
        ];
        for (i, (input, output)) in test_cases.into_iter().enumerate() {
            let output = output.map(Duration::from_secs);
            assert_eq!(time_until_max_age(&input), output, "{i}th case failed");
        }
    }

    #[test]
    fn test_parse_max_age() {
        fn hs(s: &str) -> HeaderValue {
            HeaderValue::try_from(s).unwrap()
        }

        fn hb(b: &[u8]) -> HeaderValue {
            HeaderValue::from_bytes(b).unwrap()
        }

        let test_cases = [
            (hs("max-age=420"), Some(420)),
            (hs("max-age=420 "), Some(420)),
            (hs(" max-age=420"), Some(420)),
            (hs(" max-age=420 "), Some(420)),
            (hs(", max-age=420"), Some(420)),
            (hs(",max-age=420"), Some(420)),
            (hs(",max-age=420,"), Some(420)),
            (hs(",max-age=420, "), Some(420)),
            (hs(",max-age=420, "), Some(420)),
            (hs("foo,max-age=420,bar"), Some(420)),
            (hs(",foo,max-age=420,bar"), Some(420)),
            (hs(",foo,max-age=420,bar,"), Some(420)),
            (hs(",foo,max-age=420,bar "), Some(420)),
            (hs("foo, max-age=420"), Some(420)),
            (hs("Max-Age=420"), None),
            (hs("max_age=420"), None),
            (hs("max-age=3.20"), None),
            (hs("max-age=-3"), None),
            (hs("max-age=-3"), None),
            (hs("max-age=foo"), None),
            (hb(b"\xFF, max-age=420"), None),
        ];

        for (i, (input, output)) in test_cases.into_iter().enumerate() {
            assert_eq!(parse_max_age(&input), output, "{i}th test case failed");
        }
    }
}
