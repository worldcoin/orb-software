use std::io::Read;
use update_agent_can;
use update_agent_can::{CANAddr, CANFDFrame, CANSocket, Error, Protocol, Type, MTU};

#[test]
fn open_virtual_device() {
    assert!(
        CANSocket::new(Type::RAW, Protocol::RAW).is_ok(),
        "failed to get fd for raw CAN socket"
    );
}

#[test]
fn parse_interface_address() {
    let addr: Result<CANAddr, Error> = "vcan0".parse();
    assert!(addr.is_ok(), "failed to parse");

    let addr = addr.unwrap();
    assert_eq!("vcan0", addr.name)
}

#[test]
/// On Linux, with `can-utils` installed and `cansend` available,
/// run:
/// ```shell
/// candump vcan0
/// ```
/// before running the test. After running the test, you should see:
/// ```text
/// vcan0  0F0   [8]  10 20 30 40 50 60 70 80
/// ```
fn bind_virtual_device() {
    let mut vcan: CANSocket =
        CANSocket::new(Type::RAW, Protocol::RAW).expect("failed to get fd for raw CAN socket");
    let addr: CANAddr = "vcan0".parse().expect("failed to parse vcan0 name");
    assert!(
        vcan.bind(&addr).is_ok(),
        "failed to bind to vcan0 interface"
    );
}

#[test]
fn clone_virtual_device() {
    let mut vcan: CANSocket =
        CANSocket::new(Type::RAW, Protocol::RAW).expect("failed to get fd for raw CAN socket");
    let mut stream = vcan
        .bind(&("vcan0".parse().expect("failed to parse vcan0 name")))
        .expect("failed to bind to vcan0");

    assert!(stream.try_clone().is_ok(), "failed to clone socket fd");
}

#[test]
fn mtu_virtual_device() {
    let mut vcan: CANSocket =
        CANSocket::new(Type::RAW, Protocol::RAW).expect("failed to get fd for raw CAN socket");
    // Expect that the MTU fails on an unbound CAN socket.
    // If this assertion fails, grab your iodine tablets...
    assert!(
        vcan.mtu().is_err(),
        "should have failed: impossible to get an MTU on an unbound socket!"
    );

    // Prove that we can get an MTU from a bound CAN socket
    let addr: CANAddr = "vcan0".parse().expect("failed to parse vcan0 name");
    let mtu = vcan.mtu_from_addr(&addr);
    assert!(mtu.is_ok(), "received MTU result from address");
    let mtu = mtu.unwrap();
    assert_eq!(mtu, MTU::CANFD);

    // Now prove the above assumption holds on our bound socket
    vcan.bind(&addr).expect("binding failed for vcan0");
    assert!(
        vcan.mtu().is_ok(),
        "mtu on bound socket failed, likely an issue in binding!"
    );
    assert_eq!(MTU::CANFD, vcan.mtu().unwrap());
}

#[test]
fn mtu_standard_virtual_device() {
    let mut vcan_standard: CANSocket =
        CANSocket::new(Type::RAW, Protocol::RAW).expect("failed to get fd for raw CAN socket");
    vcan_standard
        .bind(&("vcan1".parse().expect("failed to parse vcan1 name")))
        .expect("binding failed for vcan1");
    assert!(
        vcan_standard.mtu().is_ok(),
        "mtu on bound socket failed, likely an issue in binding"
    );
    assert_eq!(MTU::CAN, vcan_standard.mtu().unwrap());
}

#[test]
/// On Linux, with `can-utils` installed and `cansend` available,
/// run:
/// ```shell
/// cansend vcan0 0FF#8070605040302010
/// ```
/// and watch for the output from the test (you may need to execute
/// the test with `cargo test ... -- --nocapture` to see output)
///
/// You should see:
/// ```text
/// CANFrame {
///     id: 255,
///     len: 8,
///     pad0: 0,
///     pad1: 0,
///     dlc: 0,
///     data: [
///         128, 112, 96, 80, 64, 48, 32, 16, 0, 0, ..., 0
///     ]
/// }
/// ```
fn recv_virtual_device() {
    let mut vcan: CANSocket =
        CANSocket::new(Type::RAW, Protocol::RAW).expect("failed to get fd for raw CAN socket");
    let mut stream = vcan
        .bind(&("vcan0".parse().expect("failed to parse vcan0 name")))
        .expect("failed to bind to vcan0");

    // This maps directly to the values above
    let assert_frame = CANFDFrame {
        id: 255,
        len: 8,
        flags: 0,
        res0: 0,
        res1: 0,
        data: [
            128, 112, 96, 80, 64, 48, 32, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    };

    let mut frame: CANFDFrame = CANFDFrame::new();
    stream
        .recv(&mut frame, 0)
        .expect("encountered error reading frame!");
    assert_eq!(assert_frame, frame);
}

#[test]
fn read_virtual_device() {
    let mut vcan: CANSocket =
        CANSocket::new(Type::RAW, Protocol::RAW).expect("failed to get fd for raw CAN socket");
    let mut stream = vcan
        .bind(&("vcan0".parse().expect("failed to parse vcan0 name")))
        .expect("failed to bind to vcan0");

    let assert_buf: [u8; 64] = [
        128, 112, 96, 80, 64, 48, 32, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0,
    ];
    let mut buf: [u8; 64] = [0u8; 64];
    stream
        .read(&mut buf)
        .expect("encountered error reading frame into buffer!");

    println!("{:?}", buf);

    assert_eq!(assert_buf, buf);
}

#[test]
/// On Linux, with `can-utils` installed and `cansend` available,
/// run:
/// ```shell
/// candump vcan0
/// ```
/// before running the test. After running the test, you should see:
/// ```text
/// vcan0  0F0   [8]  10 20 30 40 50 60 70 80
/// ```
fn send_virtual_device() {
    let mut vcan: CANSocket =
        CANSocket::new(Type::RAW, Protocol::RAW).expect("failed to get fd for raw CAN socket");
    let stream = vcan
        .bind(&("vcan0".parse().expect("failed to parse vcan0 name")))
        .expect("failed to bind to vcan0");

    let mut buf: [u8; 64] = [0u8; 64];
    buf[..8].copy_from_slice(&[
        0x10u8, 0x20u8, 0x30u8, 0x40u8, 0x50u8, 0x60u8, 0x70u8, 0x80u8,
    ]);

    let frame = CANFDFrame {
        id: 128,
        len: 0,
        flags: 0,
        res0: 0,
        res1: 0,
        data: buf,
    };

    let size = stream
        .send(&frame, 0)
        .expect("encountered error writing frame!");
    assert_eq!(std::mem::size_of::<CANFDFrame>(), size);
}

#[test]
fn send_clone_virtual_device() {
    let mut vcan: CANSocket =
        CANSocket::new(Type::RAW, Protocol::RAW).expect("failed to get fd for raw CAN socket");
    let mut stream = vcan
        .bind(&("vcan0".parse().expect("failed to parse vcan0 name")))
        .expect("failed to bind to vcan0");

    let mut stream_clone = stream.try_clone().expect("failed to clone socket fd");

    let mut buf: [u8; 64] = [0u8; 64];
    buf[..8].copy_from_slice(&[
        0x10u8, 0x10u8, 0x20u8, 0x20u8, 0x30u8, 0x30u8, 0x40u8, 0x40u8,
    ]);

    let frame = CANFDFrame {
        id: 128,
        len: 0,
        flags: 0,
        res0: 0,
        res1: 0,
        data: buf,
    };

    let size = stream
        .send(&frame, 0)
        .expect("encountered error writing frame to original stream!");
    assert_eq!(std::mem::size_of::<CANFDFrame>(), size);

    let size = stream_clone
        .send(&frame, 0)
        .expect("encountered error writing frame to cloned stream!");
    assert_eq!(std::mem::size_of::<CANFDFrame>(), size);
}
