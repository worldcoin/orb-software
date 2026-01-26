mod receiver;
mod sender;
mod session;

pub use receiver::Receiver;
pub use sender::Sender;
pub use session::Zenorb;
pub use zenoh;

pub fn client_cfg(port: u16) -> zenoh::Config {
    let mut cfg = zenoh::Config::default();
    cfg.insert_json5("mode", r#""client""#).unwrap();
    cfg.insert_json5("connect/endpoints", &format!(r#"["tcp/127.0.0.1:{port}"]"#))
        .unwrap();
    cfg.insert_json5("scouting/multicast/enabled", "false")
        .unwrap();

    cfg
}

pub fn router_cfg(port: u16) -> zenoh::Config {
    let mut cfg = zenoh::Config::default();
    cfg.insert_json5("mode", r#""router""#).unwrap();
    cfg.insert_json5("listen/endpoints", &format!(r#"["tcp/127.0.0.1:{port}"]"#))
        .unwrap();
    cfg.insert_json5("scouting/multicast/enabled", "false")
        .unwrap();

    cfg
}
