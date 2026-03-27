use std::{io::ErrorKind, os::unix::net::UnixStream};

fn main() -> std::io::Result<()> {
    let socket_path = std::env::current_exe()?
        .parent()
        .expect("infallible")
        .join("socket");

    let temp_dir = tempfile::tempdir()?;
    let socket_symlink_path = temp_dir.path().join("socket");
    std::os::unix::fs::symlink(&socket_path, &socket_symlink_path)?;
    eprintln!("connecting with {socket_symlink_path:?} -> {socket_path:?}");
    let mut stream_tx = UnixStream::connect(socket_symlink_path)?;
    let mut stream_rx = stream_tx.try_clone()?;

    let tx_task = std::thread::spawn(move || {
        std::io::copy(&mut std::io::stdin(), &mut stream_tx)?;
        stream_tx.shutdown(std::net::Shutdown::Both)
    });

    if let Err(err) = std::io::copy(&mut stream_rx, &mut std::io::stdout()) {
        panic!("error while receiving on stream: {:?}", err.kind());
    }

    tx_task.join().expect("tx task panicked")
}
