use can_rs::filter::Filter;
use can_rs::stream::FrameStream;
use can_rs::{Error, Frame, Id, CANFD_DATA_LEN, CAN_DATA_LEN, MTU};
use core::time;
use std::{sync::mpsc, thread};

use crate::{can_address, canfd_address, ID};

#[test]
fn build_frame_stream() -> Result<(), Error> {
    FrameStream::<CAN_DATA_LEN>::build()
        .nonblocking(true)
        .filters(vec![])
        .bind(can_address())?;
    Ok(())
}

#[test]
fn build_stream_upgrades() -> Result<(), Error> {
    let stream_upgrade = FrameStream::<CANFD_DATA_LEN>::build().bind(canfd_address())?;
    assert_eq!(MTU::CANFD, stream_upgrade.mtu()?);

    let stream_default = FrameStream::<CAN_DATA_LEN>::build().bind(can_address())?;
    assert_eq!(MTU::CAN, stream_default.mtu()?);

    let stream_force_upgrade_fail =
        FrameStream::<CANFD_DATA_LEN>::build().bind(can_address())?;
    assert_eq!(MTU::CAN, stream_force_upgrade_fail.mtu()?);

    Ok(())
}

#[test]
fn send_and_receive_check_identical_can_frame() -> Result<(), Error> {
    let id = ID.with(|id| *id);
    let (tx, rx) = mpsc::channel();
    let receiving_thread = thread::spawn(move || -> Result<(), Error> {
        let stream = FrameStream::<CAN_DATA_LEN>::build()
            .filters(vec![Filter {
                id: Id::Standard(id),
                mask: 0xFFFF,
            }])
            .bind(can_address())?;

        let mut frame = Frame::empty();
        let size = stream.recv(&mut frame, 0)?;
        tx.send((frame, size)).unwrap();
        Ok(())
    });

    // Give the thread a fighting chance to spin up
    thread::sleep(std::time::Duration::from_millis(1));

    let frame = Frame {
        id: Id::Standard(id),
        flags: 0,
        len: CAN_DATA_LEN as u8,
        data: [15u8; CAN_DATA_LEN],
    };

    let stream = FrameStream::<CAN_DATA_LEN>::build().bind(can_address())?;
    let size = stream.send(&frame, 0)?;

    thread::sleep(std::time::Duration::from_millis(1));

    let (recv_frame, recv_size) =
        rx.recv_timeout(time::Duration::from_millis(1)).unwrap();
    assert_eq!(frame, recv_frame);
    assert_eq!(frame.len, CAN_DATA_LEN as u8);
    assert_eq!(frame.id, Id::Standard(id));
    assert_eq!(size, recv_size);

    receiving_thread.join().unwrap()?;
    Ok(())
}

#[test]
fn send_and_receive_check_identical_canfd_frame() -> Result<(), Error> {
    let id = ID.with(|id| *id);
    let (tx, rx) = mpsc::channel();
    let receiving_thread = thread::spawn(move || -> Result<(), Error> {
        let stream = FrameStream::<CANFD_DATA_LEN>::build()
            .filters(vec![Filter {
                id: Id::Standard(id),
                mask: 0xFFFF,
            }])
            .bind(canfd_address())?;

        let mut frame = Frame::empty();
        let size = stream.recv(&mut frame, 0)?;
        tx.send((frame, size)).unwrap();
        Ok(())
    });

    // Give the thread a fighting chance to spin up
    thread::sleep(std::time::Duration::from_millis(1));

    let frame = Frame {
        id: Id::Standard(id),
        flags: 0,
        len: CANFD_DATA_LEN as u8,
        data: [16u8; CANFD_DATA_LEN],
    };
    let stream = FrameStream::<CANFD_DATA_LEN>::build().bind(canfd_address())?;
    let size = stream.send(&frame, 0)?;

    thread::sleep(std::time::Duration::from_millis(1));

    let (recv_frame, recv_size) =
        rx.recv_timeout(time::Duration::from_millis(1)).unwrap();
    assert_eq!(frame, recv_frame);
    assert_eq!(frame.len, CANFD_DATA_LEN as u8);
    assert_eq!(frame.id, Id::Standard(id));
    assert_eq!(size, recv_size);

    receiving_thread.join().unwrap()?;
    Ok(())
}

#[test]
#[should_panic(expected = "CanFilterOverflow")]
fn set_too_many_sockets() {
    let filters = vec![
        Filter {
            id: Id::Standard(0),
            mask: 0
        };
        513
    ];
    FrameStream::<CAN_DATA_LEN>::build()
        .nonblocking(true)
        .filters(filters)
        .bind(can_address())
        .unwrap();
}
