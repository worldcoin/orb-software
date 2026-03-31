use std::{io::ErrorKind, os::unix::net::UnixStream, time::Duration};

use color_eyre::{
    eyre::{Context, WrapErr as _},
    Result,
};

const SOCKET_NAME: &str = "cli_stub.sock";

fn main() -> Result<()> {
    color_eyre::install()?;
    let socket_path = std::env::current_exe()?
        .parent()
        .expect("infallible")
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(SOCKET_NAME);

    eprintln!("connecting to {socket_path:?}");
    let mut stream_tx = UnixStream::connect(&socket_path)
        .wrap_err_with(|| format!("failed to connect to {}", socket_path.display()))?;
    let mut stream_rx = stream_tx.try_clone()?;

    let tx_task = std::thread::spawn(move || {
        std::io::copy(&mut std::io::stdin(), &mut stream_tx)?;
        stream_tx.shutdown(std::net::Shutdown::Both)
    });

    if let Err(err) = std::io::copy(&mut stream_rx, &mut std::io::stdout()) {
        panic!("error while receiving on stream: {:?}", err.kind());
    }

    tx_task
        .join()
        .expect("tx task panicked")
        .wrap_err("tx task error")
}
