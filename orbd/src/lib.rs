#[allow(unused_imports)]
use std::future::Future;

#[allow(unused_imports)]
use std::pin::Pin;

#[allow(unused_imports)]
use color_eyre::eyre::Result;
#[allow(unused_imports)]
use tokio::signal::unix::{self, SignalKind};
#[allow(unused_imports)]
use tokio::sync::watch;
#[allow(unused_imports)]
use tokio::task::JoinSet;

use tracing::{info, warn};

type BoxComponentFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

pub struct Component {
    name: &'static str,
    run: Box<dyn FnOnce(ComponentContext) -> BoxComponentFuture + Send + 'static>,
}

impl Component {
    pub fn new<F, Fut>(name: &'static str, run: F) -> Self
    where
        F: FnOnce(ComponentContext) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self {
            name,
            run: Box::new(move |ctx| Box::pin(run(ctx))),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    async fn run(self, ctx: ComponentContext) {
        (self.run)(ctx).await
    }
}

#[derive(Clone)]
pub struct ComponentContext {
    pub shutdown: Shutdown,
}

#[derive(Clone)]
pub struct Shutdown {
    rx: watch::Receiver<bool>,
}

impl Shutdown {
    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    pub async fn cancelled(&mut self) {
        if self.is_cancelled() {
            return;
        }

        loop {
            if self.rx.changed().await.is_err() || self.is_cancelled() {
                return;
            }
        }
    }
}

#[derive(Default)]
pub struct Program {
    components: Vec<Component>,
}

impl Program {
    pub fn new() -> Self {
        Self {
            components: Vec::new(),
        }
    }

    pub fn component(&mut self, component: Component) {
        self.components.push(component);
    }

    pub async fn run(self) -> Result<()> {
        let (shudown_tx, shutdown_rx) = watch::channel(false);

        let mut components = JoinSet::new();

        for component in self.components {
            let name = component.name();

            let ctx = ComponentContext {
                shutdown: Shutdown {
                    rx: shutdown_rx.clone(),
                },
            };

            info!(component = name, "starting component");

            components.spawn(async move {
                component.run(ctx).await;
                name
            });
        }

        let mut sigterm = unix::signal(SignalKind::terminate())?;
        let mut sigint = unix::signal(SignalKind::interrupt())?;

        loop {
            tokio::select! {
            _ = sigterm.recv() => {
                warn!("recieved SIGTERM");
                let _ = shudown_tx.send(true);
                break;
            },

            _ = sigint.recv() => {
                warn!("received SIGINT");
                let _ = shudown_tx.send(true);
                break;
            }

            Some(result) = components.join_next() => {
                match result {
                    Ok(name) => info!(component=name, "component finished"),

                    Err(err) => warn!("component task panicked: {err}"),
                }
            }

            }
        }

        while let Some(result) = components.join_next().await {
            match result {
                Ok(name) => info!(component = name, "component stopped"),
                Err(err) => warn!("component task panicked during shutdown: {err}"),
            }
        }

        Ok(())
    }
}
