use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::anychar,
    combinator::{map, opt, verify},
    multi::fold_many0,
    sequence::pair,
    IResult,
};

// Parses a set of fields with the following requirements:
// 1. A field is parsed no more than once.
// 2. Fields are parsed in arbitrary order.
// 3. Each field is optional.
macro_rules! parse_fields {
    ($input:ident; $($parse:path => $opt:ident,)+) => {
        $(let mut $opt = None;)*
        loop {
            // Skip empty fields
            $input = crate::service::mecard::skip_empty_fields($input);

            $(
                if $opt.is_none() {
                    if let Ok((next_input, parsed)) = $parse($input) {
                        $opt = Some(parsed);
                        $input = next_input;
                        continue;
                    }
                }
            )+
            break;
        }
    };
}

pub(crate) use parse_fields;

pub fn skip_empty_fields(input: &str) -> &str {
    let mut current = input;
    // Keep skipping single semicolons that aren't followed by a field name
    while current.starts_with(';') {
        let rest = &current[1..];
        // If it's followed by a field name (contains ':'), return the rest (without the semicolon)
        if rest.contains(':')
            && let Some(field_name) = rest.split(':').next()
            && field_name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_')
        {
            return rest;
        }

        // If it's an empty string, stop
        if rest.is_empty() {
            break;
        }

        current = rest;
    }

    current
}

pub fn parse_string(input: &str) -> IResult<&str, String> {
    const SPECIAL_CHARS: &[char] = &['\\', ';', ',', '"', ':'];
    let non_special = verify(anychar, |c| SPECIAL_CHARS.iter().all(|s| c != s));
    let special = pair(
        tag("\\"),
        verify(anychar, |c| SPECIAL_CHARS.iter().any(|s| c == s)),
    );
    let unescaped = alt((non_special, map(special, |(_, c)| c)));
    let (input, quote) = opt(tag("\""))(input)?;
    let (input, string) = fold_many0(unescaped, String::new, |mut acc, item| {
        acc.push(item);
        acc
    })(input)?;

    if quote.is_some() {
        let (input, _) = tag("\"")(input)?;
        Ok((input, string))
    } else if string.chars().count() >= 63
        && string.chars().all(|c| c.is_ascii_hexdigit())
    {
        // The value is in hex string format.
        let string = string.as_bytes().chunks(2).fold(
            String::with_capacity(string.len() / 2),
            |mut acc, pair| {
                // The following sequence of unwraps can't fail because of the
                // condition above.
                let string = str::from_utf8(pair).unwrap();
                let octet = u8::from_str_radix(string, 16).unwrap();
                let chr = char::from_u32(octet.into()).unwrap();
                acc.push(chr);
                acc
            },
        );
        Ok((input, string))
    } else {
        Ok((input, string))
    }
}

pub fn parse_field<
    'input,
    'name,
    T,
    F: FnOnce(&'input str) -> IResult<&'input str, T>,
>(
    input: &'input str,
    name: &'name str,
    f: F,
) -> IResult<&'input str, T> {
    let (input, _) = tag(name)(input)?;
    let (input, _) = tag(":")(input)?;
    let (input, value) = f(input)?;
    let (input, _) = tag(";")(input)?;
    Ok((input, value))
}

pub fn parse_bool(input: &str) -> IResult<&str, bool> {
    let true_val = map(tag("true"), |_| true);
    let false_val = map(alt((tag("false"), tag(""))), |_| false);
    alt((true_val, false_val))(input)
}
