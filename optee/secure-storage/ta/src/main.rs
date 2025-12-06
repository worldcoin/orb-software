#![no_std]
#![no_main]

use optee_utee::{
    ta_close_session, ta_create, ta_destroy, ta_invoke_command, ta_open_session,
    trace_println,
};
use optee_utee::{ErrorKind, Parameters, Result};
use orb_secure_storage_proto::Command;

#[ta_create]
fn create() -> Result<()> {
    trace_println!("[+] TA create");
    Ok(())
}

#[ta_open_session]
fn open_session(_params: &mut Parameters) -> Result<()> {
    trace_println!("[+] TA open session");
    Ok(())
}

#[ta_close_session]
fn close_session() {
    trace_println!("[+] TA close session");
}

#[ta_destroy]
fn destroy() {
    trace_println!("[+] TA destroy");
}

#[ta_invoke_command]
fn invoke_command(cmd_id: u32, params: &mut Parameters) -> Result<()> {
    trace_println!("[+] TA invoke command");
    let mut values = unsafe { params.0.as_value()? };
    // match Command::from(cmd_id) {
    //     Command::IncValue => {
    //         values.set_a(values.a() + 100);
    //         Ok(())
    //     }
    //     Command::DecValue => {
    //         values.set_a(values.a() - 100);
    //         Ok(())
    //     }
    //     _ => Err(ErrorKind::BadParameters.into()),
    // }
}

include!(concat!(env!("OUT_DIR"), "/user_ta_header.rs"));
