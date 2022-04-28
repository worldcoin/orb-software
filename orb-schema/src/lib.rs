include!(concat!(env!("OUT_DIR"), "/supervisor.rs"));

#[cfg(test)]
mod tests {
    use prost::{
        DecodeError,
        Message as _,
    };

    use super::Container;

    #[test]
    fn container_roundtrip_works() -> Result<(), DecodeError> {
        let msg = b"hello world";
        let expected_container = Container {
            payload: (&msg[..]).into(),
        };
        let encoded_container = expected_container.encode_to_vec();
        let decoded_container = Container::decode(&*encoded_container)?;
        assert_eq!(expected_container, decoded_container);
        Ok(())
    }
}
