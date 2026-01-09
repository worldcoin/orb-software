use crate::sender::Builder;
use color_eyre::Result;

mod receiver;
mod sender;
mod session;

pub use receiver::Receiver;
pub use sender::Sender;
pub use session::Session;
use zenoh::{bytes::Encoding, sample::Sample};

async fn bla() -> Result<()> {
    let session = Session::from_cfg(zenoh::Config::default())
        .env("dev")
        .orb_id("123")
        .for_service("connd")
        .await?;

    let sender = session
        .sender()
        .publisher_with("checkthisout", |p| p.encoding(Encoding::TEXT_PLAIN))
        .querier("othertopic")
        .querier("otherquerier")
        .build()
        .await?;

    session
        .receiver(())
        .subscribe("keyexpr", async |ctx, sample| Ok(()))
        .subscribe("checkthisout", async |ctx, sample| {
            println!("oh wow we have shared dependencies and logging for errors automatically on every zenoh subscriber, so cool!");
            let bytes = sample.payload().to_bytes();
            let str = String::from_utf8_lossy(&bytes);
            println!("i got {str}");

            Ok(())
        })
        .run()
        .await?;

    sender
        .publisher("checkthisout")?
        .put("hello world!!")
        .await
        .unwrap();

    Ok(())
}
