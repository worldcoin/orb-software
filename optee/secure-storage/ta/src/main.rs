#![no_std]
#![no_main]

use optee_utee::{
    ta_close_session, ta_create, ta_destroy, ta_invoke_command, ta_open_session,
    trace_println,
};
use optee_utee::{Error as OpteeError, ErrorKind, Parameters, Result as TeeResult};
use orb_secure_storage_proto::{Command, CommandId};

trait FromInvoke: Sized {
    fn from_params(
        id: impl TryInto<CommandId>,
        params: &mut Parameters,
    ) -> TeeResult<Self>;
}

impl FromInvoke for Command {
    fn from_params(
        id: impl TryInto<CommandId>,
        params: &mut Parameters,
    ) -> TeeResult<Self> {
        let cmd_id: CommandId = id
            .try_into()
            .map_err(|_| OpteeError::new(ErrorKind::NotImplemented))?;

        Ok(match cmd_id {
            CommandId::Ping => Command::Ping,
            CommandId::Echo => {
                let value = unsafe { params.0.as_value() }?;
                Command::Echo(value.a())
            }
        })
    }
}

#[ta_create]
fn create() -> TeeResult<()> {
    trace_println!("[+] TA create");
    Ok(())
}

#[ta_open_session]
fn open_session(_params: &mut Parameters) -> TeeResult<()> {
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
fn invoke_command(cmd_id: u32, params: &mut Parameters) -> TeeResult<()> {
    let cmd = Command::from_params(cmd_id, params)?;
    trace_println!("[+] TA invoke command {:?}", cmd);
    match cmd {
        Command::Ping => trace_println!("[+] TA response: pong"),
        Command::Echo(payload) => trace_println!("[+] TA response: {}", payload),
    }

    Ok(())
}

include!(concat!(env!("OUT_DIR"), "/user_ta_header.rs"));
