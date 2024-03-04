use std::io::Write;

use can_rs::{isotp::stream::IsotpStream, Error, CAN_DATA_LEN};

use crate::isotp_address;

#[test]
#[ignore = "needs vcan interface"]
fn build_isotp_stream() -> Result<(), Error> {
    IsotpStream::<CAN_DATA_LEN>::build().bind(isotp_address())?;
    Ok(())
}

#[test]
#[ignore = "needs vcan interface"]
fn write_isotp_stream() -> Result<(), Error> {
    let mut stream = IsotpStream::<CAN_DATA_LEN>::build().bind(isotp_address())?;
    let bytes: Vec<u8> = vec![0, 1, 2, 4, 8, 16, 32, 64, 128, 255];
    let got = stream.write(bytes.as_slice())?;
    assert_eq!(got, bytes.len());
    Ok(())
}
