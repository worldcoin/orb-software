#![allow(dead_code)]

use color_eyre::{
    eyre::{ensure, eyre, Context},
    Result,
};
use futures::{future::BoxFuture, StreamExt};
use std::{
    collections::{HashMap, VecDeque},
    future::{ready, Future},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::Notify, task::JoinHandle, time::timeout};
use tracing::error;
use zbus::{
    message::{Flags, Type},
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue},
    Connection, ConnectionBuilder, MatchRule, Message, MessageStream,
};

const TIMEOUT: Duration = Duration::from_secs(5);

type Response = Box<
    dyn FnOnce(Connection, Message) -> BoxFuture<'static, zbus::Result<Message>> + Send,
>;

#[derive(Default)]
struct State {
    calls: Vec<Message>,
    advertisements: Vec<(String, Vec<u8>)>,
    active_advertisements: HashMap<OwnedObjectPath, (String, Vec<u8>)>,
    responses: VecDeque<Response>,
    unregister_responses: VecDeque<Response>,
    pending: usize,
}

#[derive(Default)]
pub struct FakeBluez {
    state: Arc<Mutex<State>>,
    changed: Arc<Notify>,
    task: Option<JoinHandle<Result<()>>>,
}

impl FakeBluez {
    pub async fn start(&mut self, address: &str) -> Result<()> {
        ensure!(self.task.is_none(), "fake BlueZ is already running");

        let connection = timeout(TIMEOUT, ConnectionBuilder::address(address)?.build())
            .await
            .wrap_err("timed out connecting fake BlueZ to D-Bus")??;
        let rule = MatchRule::builder().msg_type(Type::MethodCall).build();
        let stream = MessageStream::for_match_rule(rule, &connection, None).await?;

        timeout(TIMEOUT, connection.request_name("org.bluez"))
            .await
            .wrap_err("timed out acquiring org.bluez")??;

        let state = self.state.clone();
        let changed = self.changed.clone();
        self.task = Some(tokio::spawn(async move {
            let result = serve(connection, stream, state, changed).await;
            if let Err(error) = &result {
                error!(?error, "fake BlueZ stopped unexpectedly");
            }

            result
        }));

        Ok(())
    }

    pub fn mock_next(
        &self,
        response: impl FnOnce(&Message) -> zbus::Result<Message> + Send + 'static,
    ) -> &Self {
        self.queue_response(move |_, call| ready(response(&call)), false)
    }

    fn queue_response<F, Fut>(&self, response: F, unregister: bool) -> &Self
    where
        F: FnOnce(Connection, Message) -> Fut + Send + 'static,
        Fut: Future<Output = zbus::Result<Message>> + Send + 'static,
    {
        let mut state = self.state.lock().unwrap();
        let responses = if unregister {
            &mut state.unregister_responses
        } else {
            &mut state.responses
        };
        responses.push_back(Box::new(move |connection, call| {
            Box::pin(response(connection, call))
        }));
        state.pending += 1;

        self
    }

    pub fn mock_adapter(&self) -> &Self {
        self.mock_adapter_list(true)
    }

    pub fn mock_no_adapter(&self) -> &Self {
        self.mock_adapter_list(false)
    }

    fn mock_adapter_list(&self, present: bool) -> &Self {
        self.mock_next(move |call| {
            let header = call.header();
            assert_eq!(header.path().map(|path| path.as_str()), Some("/"));
            assert_eq!(
                header.interface().map(|name| name.as_str()),
                Some("org.freedesktop.DBus.ObjectManager"),
            );
            assert_eq!(
                header.member().map(|name| name.as_str()),
                Some("GetManagedObjects"),
            );

            let interfaces = HashMap::from([(
                "org.bluez.Adapter1".to_owned(),
                HashMap::<String, OwnedValue>::new(),
            )]);
            let mut objects = HashMap::new();
            if present {
                objects.insert(ObjectPath::try_from("/org/bluez/hci0")?, interfaces);
            }

            Message::method_reply(call)?.build(&objects)
        })
    }

    pub fn mock_adapter_is_powered(&self, powered: bool) -> &Self {
        self.mock_next(move |call| {
            let header = call.header();
            assert_eq!(
                header.path().map(|path| path.as_str()),
                Some("/org/bluez/hci0"),
            );
            assert_eq!(
                header.interface().map(|name| name.as_str()),
                Some("org.freedesktop.DBus.Properties"),
            );
            assert_eq!(header.member().map(|name| name.as_str()), Some("Get"));

            let (interface, property): (String, String) = call.body().deserialize()?;
            assert_eq!(interface, "org.bluez.Adapter1");
            assert_eq!(property, "Powered");

            Message::method_reply(call)?.build(&OwnedValue::from(powered))
        })
    }

    pub fn mock_adapter_set_powered(&self, powered: bool) -> &Self {
        self.mock_adapter_set_powered_reply(powered, None)
    }

    pub fn mock_adapter_set_powered_error(
        &self,
        powered: bool,
        error: &'static str,
    ) -> &Self {
        self.mock_adapter_set_powered_reply(powered, Some(error))
    }

    fn mock_adapter_set_powered_reply(
        &self,
        powered: bool,
        error: Option<&'static str>,
    ) -> &Self {
        self.mock_next(move |call| {
            let header = call.header();
            assert_eq!(
                header.path().map(|path| path.as_str()),
                Some("/org/bluez/hci0"),
            );
            assert_eq!(
                header.interface().map(|name| name.as_str()),
                Some("org.freedesktop.DBus.Properties"),
            );
            assert_eq!(header.member().map(|name| name.as_str()), Some("Set"));

            let (interface, property, value): (String, String, OwnedValue) =
                call.body().deserialize()?;
            assert_eq!(interface, "org.bluez.Adapter1");
            assert_eq!(property, "Powered");
            assert_eq!(bool::try_from(value)?, powered);

            if let Some(error) = error {
                return Message::method_error(call, error)?
                    .build(&"mock power failure");
            }

            Message::method_reply(call)?.build(&())
        })
    }

    pub fn mock_advertisements(&self, count: usize) -> &Self {
        assert!(count > 0);

        for _ in 0..count {
            self.mock_advertisement_call("RegisterAdvertisement", None);
        }

        self
    }

    pub fn mock_unregister_advertisements(&self, count: usize) -> &Self {
        assert!(count > 0);

        for _ in 0..count {
            self.mock_advertisement_call("UnregisterAdvertisement", None);
        }

        self
    }

    pub fn mock_advertisement_error(&self, error: &'static str) -> &Self {
        self.mock_advertisement_call("RegisterAdvertisement", Some(error))
    }

    fn mock_advertisement_call(
        &self,
        method: &'static str,
        error: Option<&'static str>,
    ) -> &Self {
        let state = self.state.clone();
        self.queue_response(
            move |connection, call| async move {
                let header = call.header();
                assert_eq!(header.member().map(|name| name.as_str()), Some(method));
                assert_eq!(
                    header.path().map(|path| path.as_str()),
                    Some("/org/bluez/hci0"),
                );
                assert_eq!(
                    header.interface().map(|name| name.as_str()),
                    Some("org.bluez.LEAdvertisingManager1"),
                );

                match header.member().map(|name| name.as_str()) {
                    Some("RegisterAdvertisement") => {
                        let (path, _options): (
                            OwnedObjectPath,
                            HashMap<String, OwnedValue>,
                        ) = call.body().deserialize()?;
                        let reply = connection
                            .call_method(
                                Some(header.sender().unwrap().as_str()),
                                path.as_str(),
                                Some("org.freedesktop.DBus.Properties"),
                                "Get",
                                &("org.bluez.LEAdvertisement1", "ServiceData"),
                            )
                            .await?;
                        let service_data: OwnedValue = reply.body().deserialize()?;
                        let service_data =
                            HashMap::<String, OwnedValue>::try_from(service_data)?;
                        assert_eq!(service_data.len(), 1);
                        let (service_id, payload) =
                            service_data.into_iter().next().unwrap();
                        let advertisement = (service_id, Vec::<u8>::try_from(payload)?);
                        let mut state = state.lock().unwrap();
                        state.advertisements.push(advertisement.clone());
                        if error.is_none() {
                            assert!(state
                                .active_advertisements
                                .insert(path, advertisement)
                                .is_none());
                        }
                    }
                    Some("UnregisterAdvertisement") => {
                        let (path,): (OwnedObjectPath,) = call.body().deserialize()?;
                        assert!(state
                            .lock()
                            .unwrap()
                            .active_advertisements
                            .remove(&path)
                            .is_some());
                    }
                    method => panic!("unexpected advertisement method: {method:?}"),
                }

                if let Some(error) = error {
                    return Message::method_error(&call, error)?
                        .build(&"mock advertisement failure");
                }

                Message::method_reply(&call)?.build(&())
            },
            method == "UnregisterAdvertisement",
        )
    }

    pub fn advertisements(&self) -> Vec<(String, Vec<u8>)> {
        self.state.lock().unwrap().advertisements.clone()
    }

    pub fn active_advertisements(&self) -> Vec<(String, Vec<u8>)> {
        let mut advertisements: Vec<_> = self
            .state
            .lock()
            .unwrap()
            .active_advertisements
            .values()
            .cloned()
            .collect();
        advertisements.sort();

        advertisements
    }

    pub async fn assert_no_calls_for(&self, duration: Duration) {
        let initial = self.state.lock().unwrap().calls.len();
        let received = timeout(duration, async {
            loop {
                let notified = self.changed.notified();
                if self.state.lock().unwrap().calls.len() != initial {
                    break;
                }
                notified.await;
            }
        })
        .await;
        assert!(received.is_err(), "received an unexpected BlueZ call");
    }

    pub fn calls(&self) -> Vec<Message> {
        self.state.lock().unwrap().calls.clone()
    }

    pub async fn wait_all_called(&self, duration: Duration) -> Result<()> {
        timeout(duration, async {
            loop {
                let notified = self.changed.notified();
                if self.state.lock().unwrap().pending == 0 {
                    break;
                }
                notified.await;
            }
        })
        .await
        .wrap_err_with(|| {
            let pending = self.state.lock().unwrap().pending;
            format!("timed out waiting for {pending} mock responses to finish")
        })?;

        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        if let Some(task) = self.task.take() {
            task.abort();
            match task.await {
                Ok(result) => result?,
                Err(error) if error.is_cancelled() => {}
                Err(error) => return Err(error.into()),
            }
        }

        Ok(())
    }
}

impl Drop for FakeBluez {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

async fn serve(
    connection: Connection,
    mut stream: MessageStream,
    state: Arc<Mutex<State>>,
    changed: Arc<Notify>,
) -> Result<()> {
    while let Some(call) = stream.next().await {
        let call = call?;
        let response = {
            let mut state = state.lock().unwrap();
            state.calls.push(call.clone());
            if call.header().member().map(|name| name.as_str())
                == Some("UnregisterAdvertisement")
                && !state.unregister_responses.is_empty()
            {
                state.unregister_responses.pop_front()
            } else {
                state.responses.pop_front()
            }
        };
        changed.notify_waiters();
        let response = response.ok_or_else(|| {
            eyre!(
                "unexpected D-Bus call with no queued response: {:?}",
                call.header()
            )
        })?;
        let reply = timeout(TIMEOUT, response(connection.clone(), call.clone()))
            .await
            .wrap_err("timed out running fake BlueZ response")??;

        if !call
            .primary_header()
            .flags()
            .contains(Flags::NoReplyExpected)
        {
            timeout(TIMEOUT, connection.send(&reply))
                .await
                .wrap_err("timed out sending fake BlueZ reply")??;
        }

        state.lock().unwrap().pending -= 1;
        changed.notify_waiters();
    }

    color_eyre::eyre::bail!("fake BlueZ D-Bus stream closed")
}
