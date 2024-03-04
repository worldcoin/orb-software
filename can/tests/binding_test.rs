use can_rs::addr::CanAddr;
use can_rs::stream::FrameStream;
use can_rs::{Error, Frame, Id};

/// Test that you can send_to an arbitrary interface from a non-zero bound socket
/// Based on `setup-vcan.sh`, we have 2 CAN-FD interfaces that we are going to use: vcan0 and vcan3
/// To start the test, run:
/// ```shell
/// candump vcan0
/// ```
/// and in another shell run:
/// ```shell
/// candump vcan3
/// ```
/// We can make sure we receive:
///   vcan0  080  [64]  00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
/// 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
/// 00 00 00 00 00 00 00
///   vcan3  080  [64]  33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33
/// 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33 33
/// 33 33 33 33 33 33 33
#[ignore = "needs vcan interface"]
#[test]
fn binding_test() -> Result<(), Error> {
    let addr0: CanAddr = "vcan0".parse()?;
    let addr3: CanAddr = "vcan3".parse()?;

    let stream = FrameStream::<64>::build().bind(addr0).unwrap();

    let mut frame: Frame<64> = Frame {
        id: Id::Standard(128),
        len: 64,
        flags: 0,
        data: [0x00_u8; 64],
    };

    stream.send(&frame, 0)?;

    frame.data.copy_from_slice([0x33_u8; 64].as_ref());
    stream.send_to(&frame, 0, &addr3).map_err(|e| {
        println!("Error: {:?}", e);
        Error::Io(e)
    })?;

    Ok(())
}
