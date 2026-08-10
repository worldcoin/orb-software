use rkyv::{Archive, Deserialize, Serialize};

pub mod color;

pub type Handler = fn(&[u8]) -> color_eyre::Result<String>;

#[macro_export]
macro_rules! register_rkyv_types {
    ($($ty:path),* $(,)?) => {{
        let mut m: std::collections::HashMap<&'static str, $crate::Handler> = std::collections::HashMap::new();
        $({
            fn wrapper(bytes: &[u8]) -> color_eyre::Result<String> {
                let mut aligned = rkyv::util::AlignedVec::<16>::with_capacity(bytes.len());
                aligned.extend_from_slice(bytes);

                let archived: &rkyv::Archived<$ty> =
                    rkyv::access::<rkyv::Archived<$ty>, rkyv::rancor::Error>(&aligned).map_err(|e| color_eyre::eyre::eyre!("{e}"))?;

                let owned: $ty = rkyv::deserialize::<$ty, rkyv::rancor::Error>(archived).map_err(|e|color_eyre::eyre::eyre!("{e}"))?;

                Ok(format!("{owned:?}"))
            }

            m.insert(stringify!($ty), wrapper as $crate::Handler);
        })*
        m
    }};
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[rkyv(derive(Debug, PartialEq))]
pub enum Example {
    Foo,
    Bar,
}

#[cfg(test)]
mod tests {
    use super::Example;

    #[test]
    fn registered_handler_deserializes() {
        let handlers = crate::register_rkyv_types!(Example);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&Example::Foo).unwrap();

        assert_eq!(handlers["Example"](&bytes).unwrap(), "Foo");
    }
}
