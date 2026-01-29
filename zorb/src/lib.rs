use rkyv::{bytecheck, Archive, CheckBytes, Deserialize, Serialize};

pub type Handler = fn(&[u8]) -> color_eyre::Result<String>;

#[macro_export]
macro_rules! register_rkyv_types {
    ($($ty:path),* $(,)?) => {{
        let mut m: std::collections::HashMap<&'static str, $crate::Handler> = std::collections::HashMap::new();
        $(
            fn wrapper(bytes: &[u8]) -> color_eyre::Result<String> {
                let archived: &rkyv::Archived<$ty> =
                    rkyv::check_archived_root::<$ty>(bytes).map_err(|e| color_eyre::eyre::eyre!("{e}"))?;

                let owned: $ty = rkyv::Deserialize::<$ty, rkyv::Infallible>::deserialize(
                    archived,
                    &mut rkyv::Infallible,
                ).map_err(|e|color_eyre::eyre::eyre!("{e}"))?;

                Ok(format!("{owned:?}"))
            }

            m.insert(stringify!($ty), wrapper as $crate::Handler);
        )*
        m
    }};
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[archive_attr(derive(CheckBytes, Debug, PartialEq))]
pub enum Example {
    Foo,
    Bar,
}
