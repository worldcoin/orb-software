//! An in-memory stub implementation of [`crate::BackendT`]

use std::sync::Arc;

use dashmap::DashMap;
use eyre::{ensure, WrapErr as _};
use orb_secure_storage_proto::{
    CommandId, GetRequest, GetResponse, PutRequest, PutResponse, StorageDomain,
    VersionRequest, VersionResponse,
};
use rustix::process::Uid;

use crate::{BackendT, SessionT};

/// An in-memory stub implementation of [`crate::BackendT`].
pub struct InMemoryBackend;

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub struct StateKey {
    pub euid: Uid,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub contents: Vec<u8>,
}

#[derive(Default, Debug)]
pub struct StateInner {
    pub map: DashMap<StateKey, Entry>,
    pub version: String,
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryContext(pub Arc<StateInner>);

/// It happens to be the same type as the context.
pub type InMemorySession = InMemoryContext;

impl BackendT for InMemoryBackend {
    type Context = InMemoryContext;

    type Session = InMemoryContext;

    fn open_session(
        ctx: &mut Self::Context,
        _euid: Uid,
        _domain: StorageDomain, // TODO: Support multiple domains
    ) -> eyre::Result<Self::Session> {
        Ok(ctx.clone())
    }
}

impl SessionT for InMemoryContext {
    fn invoke(
        &mut self,
        command: CommandId,
        serialized_request: &[u8],
        response_buf: &mut [u8],
    ) -> eyre::Result<usize> {
        let euid = rustix::process::geteuid();
        let serialized_response = match command {
            CommandId::Put => {
                let PutRequest { key, val } =
                    serde_json::from_slice(serialized_request)
                        .wrap_err("failed to deserialize `PutRequest`")?;
                let previous_entry = self
                    .0
                    .map
                    .insert(StateKey { euid, name: key }, Entry { contents: val });
                let prev_val = previous_entry.map(
                    |Entry {
                         contents: previous_contents,
                     }| previous_contents,
                );

                let response = PutResponse { prev_val };

                serde_json::to_vec(&response).expect("infallible")
            }

            CommandId::Get => {
                let GetRequest { key } = serde_json::from_slice(serialized_request)
                    .wrap_err("failed to deserialize `GetRequest`")?;
                let current_entry = self
                    .0
                    .map
                    .get(&StateKey { euid, name: key })
                    .map(|e| e.to_owned());
                let val = current_entry.map(|Entry { contents }| contents);

                let response = GetResponse { val };

                serde_json::to_vec(&response).expect("infallible")
            }

            CommandId::Version => {
                let VersionRequest = serde_json::from_slice(serialized_request)
                    .wrap_err("failed to deserialize `GetRequest`")?;

                let version = self.0.version.clone();
                let response = VersionResponse(version);

                serde_json::to_vec(&response).expect("infallible")
            }
        };

        ensure!(
            serialized_response.len() <= response_buf.len(),
            "response size was bigger than output buffer"
        );
        let response_buf = &mut response_buf[0..serialized_response.len()];
        response_buf.copy_from_slice(&serialized_response);

        Ok(serialized_response.len())
    }
}

#[cfg(test)]
mod test {
    use crate::Client;

    use super::*;

    #[test]
    fn test_default_version() {
        color_eyre::install().ok();
        let mut ctx = InMemoryContext::default();
        let mut client =
            Client::<InMemoryBackend>::new(&mut ctx, StorageDomain::WifiProfiles)
                .unwrap();

        assert_eq!(client.version().unwrap(), String::new());
    }

    #[test]
    fn test_instantiated_version() {
        color_eyre::install().ok();
        let version = String::from("yeet");
        let mut ctx = InMemoryContext(Arc::new(StateInner {
            version: version.clone(),
            ..Default::default()
        }));
        let mut client =
            Client::<InMemoryBackend>::new(&mut ctx, StorageDomain::WifiProfiles)
                .unwrap();

        assert_eq!(client.version().unwrap(), version);
    }

    #[test]
    fn empty_state_has_no_contents() {
        color_eyre::install().ok();
        let mut ctx = InMemoryContext::default();
        let mut client =
            Client::<InMemoryBackend>::new(&mut ctx, StorageDomain::WifiProfiles)
                .unwrap();

        assert!(client.get("foobar").unwrap().is_none());
    }

    #[test]
    fn single_item_is_readable() {
        color_eyre::install().ok();
        let initial_contents = [("uwu", "🙀".as_bytes())];
        let mut ctx = make_state(initial_contents.iter());
        let mut client =
            Client::<InMemoryBackend>::new(&mut ctx, StorageDomain::WifiProfiles)
                .unwrap();

        assert_eq!(
            client.get("uwu").unwrap().as_deref(),
            Some(initial_contents[0].1)
        );
        assert!(client.get("umu").unwrap().is_none());
        assert_eq!(
            client.get("uwu").unwrap().as_deref(),
            Some(initial_contents[0].1)
        );
    }

    #[test]
    fn multiple_items_are_readable() {
        color_eyre::install().ok();
        let initial_contents = [
            ("a", [1, 2].as_slice()),
            ("b", [].as_slice()),
            ("be", [6, 7].as_slice()),
        ];
        let mut ctx = make_state(initial_contents.iter());
        let mut client =
            Client::<InMemoryBackend>::new(&mut ctx, StorageDomain::WifiProfiles)
                .unwrap();

        for (k, v) in initial_contents {
            assert_eq!(client.get(k).unwrap().as_deref(), Some(v));
        }
        assert!(client.get("notpresent").unwrap().is_none());
    }

    #[test]
    fn read_write_read_to_same_key() {
        let initial_contents = [("a", "yippee".as_bytes())];
        let mut ctx = make_state(initial_contents.iter());
        let mut client =
            Client::<InMemoryBackend>::new(&mut ctx, StorageDomain::WifiProfiles)
                .unwrap();

        //read
        assert_eq!(
            client.get(initial_contents[0].0).unwrap().as_deref(),
            Some(initial_contents[0].1)
        );
        //write
        let new_content = "babback".as_bytes();
        assert_eq!(
            client
                .put(initial_contents[0].0, new_content)
                .unwrap()
                .as_deref(),
            Some(initial_contents[0].1)
        );
        //read
        assert_eq!(
            client.get(initial_contents[0].0).unwrap().as_deref(),
            Some(new_content)
        );
    }

    fn make_state<'a>(
        contents: impl Iterator<Item = &'a (&'static str, &'static [u8])>,
    ) -> InMemoryContext {
        let euid = rustix::process::geteuid();
        let map = contents
            .into_iter()
            .map(|(k, v)| {
                (
                    StateKey {
                        euid,
                        name: k.to_string(),
                    },
                    Entry {
                        contents: v.to_vec(),
                    },
                )
            })
            .collect();

        InMemoryContext(Arc::new(StateInner {
            map,
            ..Default::default()
        }))
    }
}
