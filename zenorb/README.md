# zenorb

A helper library for using [Zenoh](https://zenoh.io/) with orb-specific conventions. Just a small
wrapper for delcaring publishers, queriers, queryables and subscribers. Tries to use native `zenoh` types
as much as possible.

## Design Decisions

### 1. Publisher & Querier Registry

Zenoh encourages holding declared publishers and queriers across the application lifetime for performance optimizations. Without this, you'd need to either:

1. Declare publishers/queriers on every send (inefficient)
2. Manually manage a hashmap of declared publishers/queriers throughout your codebase (boilerplate)

`Sender` solves this by maintaining an internal registry of declared publishers and queriers. You declare them once at startup, then retrieve them by keyexpr when needed:

```rust
// At startup: declare all publishers and queriers
let sender = session
    .sender()
    .publisher("events")
    .publisher_with("metrics", |p| p.encoding(Encoding::APPLICATION_JSON))
    .querier("other-service/status")
    .build()
    .await?;

// Later: use them by keyexpr
sender.publisher("events")?.put(payload).await?;
sender.querier("other-service/status")?.get().await?;
```

`Sender` is `Clone` (wrapping an `Arc<Registry>`), so it can be passed around to different parts of your application.

The only time you should use Zenoh's publishers directly (without declaring) is when topics are dynamic and not known at startup.

### 2. Context Injection & Automatic Error Logging

`Receiver` accepts a generic `Ctx` type that gets cloned and passed to every handler. This enables:

- **Dependency injection**: Pass shared state, database connections, or test mocks
- **Testability**: Inject test doubles without restructuring your handlers

Additionally, handlers return `Result<()>`, and errors are automatically logged with the keyexpr context. This eliminates repetitive error handling boilerplate:

```rust
// Without zenorb: manual cloning and error handling everywhere
let subscriber = session.declare_subscriber("events").await?;
let db = db.clone();
let metrics = metrics.clone();
task::spawn(async move {
    while let Ok(sample) = subscriber.recv_async().await {
        let db = db.clone();
        let metrics = metrics.clone();
        let result = async move {
            let data: Event = deserialize(&sample)?;
            db.insert(&data).await?;
            metrics.record(&data).await?;
            Ok(())
        };

        if let Err(e) = result.await {
            tracing::error!("Handler failed: {e}");
        }
    }
});

// With zenorb: context injection and automatic error logging
session
    .receiver(Ctx { db, metrics })
    .subscriber("events", async |ctx, sample| {
        let data: Event = deserialize(&sample)?;
        ctx.db.insert(&data).await?;
        ctx.metrics.record(&data).await?;

        Ok(())
    })
```

### 3. Standardized Topic Format

All orb sessions need to namespace their topics by orb ID and a name to avoid collisions. Without this library, every service would need to:

1. Carry around, `orb_id`, and `service_name` values
2. Remember to format topics correctly
3. Risk inconsistent formatting across services

zenorb standardizes this in the `Session`:

```rust
let session = Session::from_cfg(cfg)
    .orb_id(orb_id)
    .with_name("my-service")
    .await?;
```

Topic formats are then applied automatically:

| Type       | Format                      |
| ---------- | --------------------------- |
| Publisher  | `<orb-id>/<name>/<keyexpr>` |
| Subscriber | `<orb-id>/<keyexpr>`        |
| Queryable  | `<orb-id>/<name>/<keyexpr>` |
| Querier    | `<orb-id>/<keyexpr>`        |

Note that subscribers and queriers omit the service name since they typically listen to or query other services. The service name is part of the keyexpr you provide:

```rust
// Service "banana" publishes to: ea2ea744/banana/events
// Service "apple" subscribes to: ea2ea744/banana/events
session
    .receiver(ctx)
    .subscriber("banana/events", handler)  // keyexpr includes source service
    .run()
    .await?;
```

## Example

```rust
use orb_info::{orb_os_release::OrbRelease, OrbId};
use zenoh::bytes::Encoding;

#[derive(Clone)]
struct AppCtx {
    received: Arc<Mutex<Vec<Message>>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = zenorb::client_cfg(7447);
    let orb_id = OrbId::from_str("ea2ea744")?;

    // Create sessions with two different names
    let banana_session = Session::from_cfg(cfg.clone())
        .orb_id(orb_id.clone())
        .with_name("banana")
        .await?;

    let apple_session = Session::from_cfg(cfg)
        .orb_id(orb_id)
        .with_name("apple")
        .await?;

    // Set up the sender with declared publishers
    let sender = banana_session
        .sender()
        .publisher("notifications")
        .build()
        .await?;

    // Set up the receiver with context and handlers
    let ctx = AppCtx { received: Arc::new(Mutex::new(vec![])) };

    apple_session
        .receiver(ctx)
        .subscriber("banana/notifications", async |ctx, sample| {
            let msg: Message = serde_json::from_slice(&sample.payload().to_bytes())?;
            ctx.received.lock().await.push(msg);
            Ok(())
        })
        .run()
        .await?;

    // Publish messages
    sender
        .publisher("notifications")?
        .put(serde_json::to_vec(&Message::new("hello"))?)
        .await?;

    Ok(())
}
```
