# zenorb

`zenorb` is a small wrapper around [Zenoh](https://zenoh.io/) for orb services.

It keeps Zenoh's native types and behavior, but adds a few orb-specific
conventions:

- declared publisher and querier registries
- shared context for subscribers and queryables
- consistent orb/service key expression prefixes
- a small command/reply layer for query-based interactions

## Design Decisions

### 1. Declared Publishers and Queriers

Zenoh performs best when publishers and queriers are declared once and kept for
the lifetime of the process.

Without a wrapper, each service has to either redeclare them repeatedly or build
its own registry and thread that through the application. `Sender` provides that
registry directly. You declare publishers and queriers at startup and retrieve
them later by key expression.

```rust
use zenoh::bytes::Encoding;
use zenorb::Zenorb;

let zenorb = Zenorb::from_cfg(cfg)
    .orb_id(orb_id)
    .with_name("banana")
    .await?;

let sender = zenorb
    .sender()
    .publisher("events")
    .publisher_with("metrics", |p| p.encoding(Encoding::APPLICATION_JSON))
    .querier("apple/status")
    .build()
    .await?;

// Later
sender.publisher("events")?.put(payload).await?;
sender.querier("apple/status")?.get().await?;
```

`Sender` is cheap to clone, so it can be shared freely across the application.

### 2. Shared Context for Receivers

`Receiver` lets you register subscribers and queryables that all receive the
same cloned context.

That is useful for shared state such as database handles, caches, metrics
clients, or test doubles. Handlers return `Result<()>`, and `zenorb` logs
failures with the relevant key expression so each call site does not have to
repeat the same error-handling boilerplate.

```rust
#[derive(Clone)]
struct AppCtx {
    db: Db,
    metrics: Metrics,
}

zenorb
    .receiver(AppCtx { db, metrics })
    .subscriber("apple/events", async |ctx, sample| {
        let event: Event = serde_json::from_slice(&sample.payload().to_bytes())?;

        ctx.db.insert(&event).await?;
        ctx.metrics.record(&event).await?;

        Ok(())
    })
    .run()
    .await?;
```

### 3. Orb and Service Naming

Orb services need stable key expressions so publishers, subscribers, queriers,
and queryables all agree on the same paths.

`Zenorb` carries the orb ID and service name once:

```rust
use zenorb::Zenorb;

let zenorb = Zenorb::from_cfg(cfg)
    .orb_id(orb_id)
    .with_name("banana")
    .await?;
```

From there, `zenorb` applies the prefixes for you:

| Type | Format |
| --- | --- |
| Publisher | `<orb-id>/<service>/<keyexpr>` |
| Queryable | `<orb-id>/<service>/<keyexpr>` |
| Subscriber | `<orb-id>/<keyexpr>` |
| Querier | `<orb-id>/<keyexpr>` |

That means publishers and queryables are service-scoped, while subscribers and
queriers target the full service path you provide.

```rust
// Service "banana" publishes to ea2ea744/banana/events
sender.publisher("events")?.put(payload).await?;

// Service "apple" subscribes to ea2ea744/banana/events
apple
    .receiver(ctx)
    .subscriber("banana/events", async |ctx, sample| {
        Ok(())
    })
    .run()
    .await?;
```

If `Receiver::queryable(...)` is too opinionated for a use case,
`Zenorb::declare_queryable(...)` exposes the underlying Zenoh queryable builder
while keeping the same service-scoped prefix.

### 4. ZOCI - Zenoh Orb Command Interface

Some queries are really commands: one request, one reply, typed success or typed
error.

`zenorb::zoci` defines a small convention for that pattern.

Caller-side helpers:

- `Sender::command(...)` sends a JSON payload through a declared querier
- `Sender::command_raw(...)` sends a raw string payload through a declared querier
- `Zenorb::command(...)` sends a JSON payload with `Zenorb::get(...)`
- `Zenorb::command_raw(...)` sends a raw string payload with `Zenorb::get(...)`

Handler-side helpers:

- `query.json()` decodes a JSON request payload
- `query.args()` decodes a space-delimited argument payload
- `query.res(...)` sends a JSON success reply
- `query.res_err(...)` sends a JSON error reply

Reply-side helper:

- `ReplyExt::json()` decodes a reply into `Result<OkType, ErrType>`

The command API keeps Zenoh's two layers of failure visible:

- outer `Result`: transport or session failure
- inner `Result`: success reply or `reply_err(...)`

#### ZOCI Example

```rust
use serde::{Deserialize, Serialize};
use zenorb::{
    zoci::{ReplyExt, ZociQueryExt},
    Zenorb,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct StatusRequest {
    id: u64,
    label: String,
}

let red = Zenorb::from_cfg(client_cfg.clone())
    .orb_id(orb_id.clone())
    .with_name("red")
    .await?;

let blue = Zenorb::from_cfg(client_cfg)
    .orb_id(orb_id)
    .with_name("blue")
    .await?;

let sender = red
    .sender()
    .querier("blue/status")
    .build()
    .await?;

blue.receiver(())
    .queryable("status", async |_ctx, query| {
        let req: StatusRequest = query.json()?;
        query.res(&req).await?;

        Ok(())
    })
    .run()
    .await?;

let reply = sender
    .command(
        "blue/status",
        &StatusRequest {
            id: 7,
            label: "banana".into(),
        },
    )
    .await?;

let reply: Result<StatusRequest, StatusRequest> = reply.json()?;
```

For lighter payloads, pair `command_raw(...)` with `query.args()`:

```rust
use zenorb::zoci::{ReplyExt, ZociQueryExt};

blue.receiver(())
    .queryable("tuple", async |_ctx, query| {
        let args: (String, String) = query.args()?;
        query.res(&args).await?;

        Ok(())
    })
    .run()
    .await?;

let sender = red
    .sender()
    .querier("blue/tuple")
    .build()
    .await?;

let reply = sender.command_raw("blue/tuple", "one two").await?;
let reply: Result<(String, String), String> = reply.json()?;
```

## Basic Publish/Subscribe Example

The examples above show the command/reply flow. The example below goes back to
the core `zenorb` pattern: declared publishers on one service, subscribers on
another, and JSON payload handling in the subscriber callback.

```rust
use orb_info::OrbId;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;
use zenorb::Zenorb;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    text: String,
}

#[derive(Clone)]
struct AppCtx {
    received: Arc<Mutex<Vec<Message>>>
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let cfg = zenorb::client_cfg(7447);
    let orb_id = OrbId::from_str("ea2ea744")?;

    let banana = Zenorb::from_cfg(cfg.clone())
        .orb_id(orb_id.clone())
        .with_name("banana")
        .await?;

    let apple = Zenorb::from_cfg(cfg)
        .orb_id(orb_id)
        .with_name("apple")
        .await?;

    let sender = banana
        .sender()
        .publisher("notifications")
        .build()
        .await?;

    let ctx = AppCtx {
        received: Arc::new(Mutex::new(vec![])),
    };

    apple
        .receiver(ctx.clone())
        .subscriber("banana/notifications", async |ctx, sample| {
            let msg: Message = serde_json::from_slice(&sample.payload().to_bytes())?;
            ctx.received.lock().await.push(msg);

            Ok(())
        })
        .run()
        .await?;

    sender
        .publisher("notifications")?
        .put(serde_json::to_vec(&Message {
            text: "hello".to_string(),
        })?)
        .await?;

    Ok(())
}
```
