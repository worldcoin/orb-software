use color_eyre::{
    eyre::{eyre, ContextCompat, WrapErr},
    Result,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{any::type_name, borrow::Cow, str::Split};
use zenoh::{
    query::{Query, ReplyError},
    sample::Sample,
};

/// Deserializes a space-delimited query payload into a typed tuple.
///
/// This is intended for lightweight command-style payloads such as
/// `"1 banana true"`. Each token is parsed independently:
///
/// - valid JSON tokens like `1`, `true`, or `"banana"` are deserialized as JSON
/// - bare tokens like `banana` are also accepted for string-like targets
///
/// This is best paired with [`ZociQueryExt::args`].
pub trait ZociArg: Sized {
    fn deserialize(str: &str) -> Result<Self>;
}

/// ZOCI helpers for [`zenoh::query::Query`].
///
/// ZOCI stands for `Zenoh Orb Command Interface`.
///
/// The extension exposes two request formats:
///
/// - [`json`](ZociQueryExt::json) for structured JSON payloads
/// - [`args`](ZociQueryExt::args) for raw space-delimited argument strings
///
/// Replies are sent as JSON through [`res`](ZociQueryExt::res) and
/// [`res_err`](ZociQueryExt::res_err).
#[allow(async_fn_in_trait)]
pub trait ZociQueryExt {
    /// Deserializes the query payload as JSON.
    ///
    /// This is the query-side counterpart to `Sender::command` and
    /// `Zenorb::command`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let req: StatusRequest = query.json()?;
    /// ```
    fn json<A>(&self) -> Result<A>
    where
        A: DeserializeOwned;

    /// Deserializes the query payload as a space-delimited argument list.
    ///
    /// This is the query-side counterpart to `Sender::command_raw` and
    /// `Zenorb::command_raw`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let args: (String, bool) = query.args()?;
    /// ```
    fn args<A>(&self) -> Result<A>
    where
        A: ZociArg;

    fn payload_str<'de>(&'de self) -> Result<Cow<'de, str>>;

    /// Replies with a JSON-serialized success payload on the query's key expression.
    ///
    /// This avoids repeating `query.key_expr().clone()` at call sites.
    async fn res<A, E>(&self, value: Result<A, E>) -> Result<()>
    where
        A: Serialize,
        E: Serialize;

    /// Replies with a JSON-serialized success payload on the query's key expression.
    ///
    /// This avoids repeating `query.key_expr().clone()` at call sites.
    async fn res_ok<A>(&self, value: A) -> Result<()>
    where
        A: Serialize;

    /// Replies with a JSON-serialized error payload.
    ///
    /// The payload can later be decoded by callers through [`ReplyExt::json`].
    async fn res_err<A>(&self, value: A) -> Result<()>
    where
        A: Serialize;
}

/// Convenience helpers for decoding a ZOCI reply into typed JSON values.
///
/// This is intended for the inner `Result<Sample, ReplyError>` returned by
/// `reply.into_result()`, or by `command(...)` / `command_raw(...)` after the
/// outer transport `Result` has been unwrapped.
pub trait ReplyExt {
    /// Deserializes a reply payload into typed success and error variants.
    ///
    /// - `Ok(Sample)` is decoded as `A`
    /// - `Err(ReplyError)` is decoded as `B`
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let reply = sender.command("blue/status", &req).await?;
    /// let reply: Result<StatusResponse, StatusError> = reply.json()?;
    /// ```
    fn json<A, B>(self) -> Result<std::result::Result<A, B>>
    where
        A: DeserializeOwned,
        B: DeserializeOwned;
}

impl ZociQueryExt for Query {
    fn json<A>(&self) -> Result<A>
    where
        A: DeserializeOwned,
    {
        let payload = self.payload().wrap_err("could not read payload")?;
        let bytes = payload.to_bytes();
        let val: A = serde_json::from_slice(&bytes)?;

        Ok(val)
    }

    fn args<'de, A>(&self) -> Result<A>
    where
        A: ZociArg,
    {
        let payload = self.payload().wrap_err("could not read payload")?;
        let str = payload.try_to_string()?;
        let val: A = A::deserialize(&str)?;

        Ok(val)
    }

    fn payload_str(&self) -> Result<Cow<'_, str>> {
        let payload = self.payload().wrap_err("could not read payload")?;
        let str = payload.try_to_string()?;

        Ok(str)
    }

    async fn res<A, E>(&self, value: Result<A, E>) -> Result<()>
    where
        A: Serialize,
        E: Serialize,
    {
        match value {
            Ok(value) => {
                let payload = serde_json::to_vec(&value)?;

                self.reply(self.key_expr().clone(), payload)
                    .await
                    .map_err(|e| eyre!("{e}"))?;
            }

            Err(value) => {
                let payload = serde_json::to_vec(&value)?;
                self.reply_err(payload).await.map_err(|e| eyre!("{e}"))?;
            }
        }

        Ok(())
    }

    async fn res_ok<A>(&self, value: A) -> Result<()>
    where
        A: Serialize,
    {
        let payload = serde_json::to_vec(&value)?;

        self.reply(self.key_expr().clone(), payload)
            .await
            .map_err(|e| eyre!("{e}"))?;

        Ok(())
    }

    async fn res_err<A>(&self, value: A) -> Result<()>
    where
        A: Serialize,
    {
        let payload = serde_json::to_vec(&value)?;

        self.reply_err(payload).await.map_err(|e| eyre!("{e}"))?;

        Ok(())
    }
}

impl ReplyExt for std::result::Result<Sample, ReplyError> {
    fn json<A, B>(self) -> Result<std::result::Result<A, B>>
    where
        A: DeserializeOwned,
        B: DeserializeOwned,
    {
        match self {
            Ok(reply) => {
                let bytes = reply.payload().to_bytes();
                let value = serde_json::from_slice(&bytes).wrap_err_with(|| {
                    format!("could not deserialize ok reply as {}", type_name::<A>())
                })?;

                Ok(Ok(value))
            }
            Err(reply_err) => {
                let bytes = reply_err.payload().to_bytes();
                let value = serde_json::from_slice(&bytes).wrap_err_with(|| {
                    format!("could not deserialize err reply as {}", type_name::<B>())
                })?;

                Ok(Err(value))
            }
        }
    }
}

struct Args<'a> {
    input: &'a str,
    split: Split<'a, char>,
}

impl<'a> Args<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            split: input.split(' '),
        }
    }

    fn next<A: DeserializeOwned>(&mut self) -> Result<A> {
        let a = self
            .split
            .next()
            .wrap_err_with(|| format!("could not deserialize {}", self.input))?;

        let a = match serde_json::from_str(a) {
            Ok(a) => a,
            Err(json_error) => {
                serde_json::from_value(serde_json::Value::String(a.to_owned()))
                    .map_err(|_| json_error)?
            }
        };

        Ok(a)
    }
}

fn failure<A>(input: &str) -> impl FnOnce() -> String + '_ {
    move || format!("could not deserialize {input:?} as {}", type_name::<A>())
}

impl<A, B> ZociArg for (A, B)
where
    A: DeserializeOwned,
    B: DeserializeOwned,
{
    fn deserialize(str: &str) -> Result<Self> {
        let mut args = Args::new(str);
        (|| -> Result<Self> { Ok((args.next()?, args.next()?)) })()
            .wrap_err_with(failure::<Self>(str))
    }
}

impl<A, B, C> ZociArg for (A, B, C)
where
    A: DeserializeOwned,
    B: DeserializeOwned,
    C: DeserializeOwned,
{
    fn deserialize(str: &str) -> Result<Self> {
        let mut args = Args::new(str);
        (|| -> Result<Self> { Ok((args.next()?, args.next()?, args.next()?)) })()
            .wrap_err_with(failure::<Self>(str))
    }
}

impl<A, B, C, D> ZociArg for (A, B, C, D)
where
    A: DeserializeOwned,
    B: DeserializeOwned,
    C: DeserializeOwned,
    D: DeserializeOwned,
{
    fn deserialize(str: &str) -> Result<Self> {
        let mut args = Args::new(str);
        (|| -> Result<Self> {
            Ok((args.next()?, args.next()?, args.next()?, args.next()?))
        })()
        .wrap_err_with(failure::<Self>(str))
    }
}

impl<A, B, C, D, E> ZociArg for (A, B, C, D, E)
where
    A: DeserializeOwned,
    B: DeserializeOwned,
    C: DeserializeOwned,
    D: DeserializeOwned,
    E: DeserializeOwned,
{
    fn deserialize(str: &str) -> Result<Self> {
        let mut args = Args::new(str);
        (|| -> Result<Self> {
            Ok((
                args.next()?,
                args.next()?,
                args.next()?,
                args.next()?,
                args.next()?,
            ))
        })()
        .wrap_err_with(failure::<Self>(str))
    }
}

impl<A, B, C, D, E, F> ZociArg for (A, B, C, D, E, F)
where
    A: DeserializeOwned,
    B: DeserializeOwned,
    C: DeserializeOwned,
    D: DeserializeOwned,
    E: DeserializeOwned,
    F: DeserializeOwned,
{
    fn deserialize(str: &str) -> Result<Self> {
        let mut args = Args::new(str);
        (|| -> Result<Self> {
            Ok((
                args.next()?,
                args.next()?,
                args.next()?,
                args.next()?,
                args.next()?,
                args.next()?,
            ))
        })()
        .wrap_err_with(failure::<Self>(str))
    }
}

#[cfg(test)]
mod tests {
    use super::{Result, ZociArg};
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Count(u64);

    #[derive(Debug, Deserialize, PartialEq)]
    struct Label(String);

    #[derive(Debug, Deserialize, PartialEq)]
    struct Enabled(bool);

    fn deserialize<A>(input: &str) -> Result<A>
    where
        A: ZociArg,
    {
        A::deserialize(input)
    }

    #[test]
    fn deserialize_parses_primitive_values() {
        // Arrange
        let input = r#"1 "banana" true"#;

        // Act
        let actual: (u64, String, bool) = deserialize(input).unwrap();

        // Assert
        assert_eq!(actual, (1, "banana".to_string(), true));
    }

    #[test]
    fn deserialize_parses_custom_types() {
        // Arrange
        let input = r#"1 "banana" true"#;

        // Act
        let actual: (Count, Label, Enabled) = deserialize(input).unwrap();

        // Assert
        assert_eq!(
            actual,
            (Count(1), Label("banana".to_string()), Enabled(true))
        );
    }

    #[test]
    fn deserialize_parses_bare_string_values() {
        // Arrange
        let input = "one two";

        // Act
        let actual: (String, String) = deserialize(input).unwrap();

        // Assert
        assert_eq!(actual, ("one".to_string(), "two".to_string()));
    }

    #[test]
    fn deserialize_parses_bare_string_newtypes() {
        // Arrange
        let input = "one two";

        // Act
        let actual: (Label, Label) = deserialize(input).unwrap();

        // Assert
        assert_eq!(actual, (Label("one".to_string()), Label("two".to_string())));
    }

    #[test]
    fn deserialize_parses_mixed_bare_string_values() {
        // Arrange
        let input = "1 banana true";

        // Act
        let actual: (u64, String, bool) = deserialize(input).unwrap();

        // Assert
        assert_eq!(actual, (1, "banana".to_string(), true));
    }

    #[test]
    fn deserialize_parses_mixed_six_values() {
        // Arrange
        let input = r#"1 "banana" true 4 "apple" false"#;

        // Act
        let actual: (u64, String, bool, Count, Label, Enabled) =
            deserialize(input).unwrap();

        // Assert
        assert_eq!(
            actual,
            (
                1,
                "banana".to_string(),
                true,
                Count(4),
                Label("apple".to_string()),
                Enabled(false),
            )
        );
    }

    #[test]
    fn deserialize_includes_delimited_input_and_tuple_type_when_argument_is_missing() {
        // Arrange
        let input = r#"1"#;

        // Act
        let actual: Result<(u64, String)> = deserialize(input);
        let error = actual.unwrap_err();

        // Assert
        assert!(error
            .to_string()
            .contains(r#"could not deserialize "1" as (u64, alloc::string::String)"#));
    }

    #[test]
    fn deserialize_includes_delimited_input_and_tuple_type_when_argument_is_not_valid_json(
    ) {
        // Arrange
        let input = "banana apple";

        // Act
        let actual: Result<(u64, String)> = deserialize(input);
        let error = actual.unwrap_err();

        // Assert
        assert!(error.to_string().contains(
            r#"could not deserialize "banana apple" as (u64, alloc::string::String)"#
        ));
    }

    #[test]
    fn deserialize_includes_full_primitive_tuple_type_in_errors() {
        // Arrange
        let input = r#"1 "banana""#;

        // Act
        let actual: Result<(u64, String, bool)> = deserialize(input);
        let error = actual.unwrap_err();

        // Assert
        assert!(error.to_string().contains(
            r#"could not deserialize "1 \"banana\"" as (u64, alloc::string::String, bool)"#
        ));
    }
}
