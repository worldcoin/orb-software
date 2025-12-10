#![no_std]
#![no_main]

mod trace;

extern crate alloc;

use alloc::format;
use alloc::string::ToString;
use anyhow::{bail, Context, Result};
use optee_utee::{
    property::PropertyKey as _, ta_close_session, ta_create, ta_destroy,
    ta_invoke_command, ta_open_session,
};
use optee_utee::{
    Error as TeeError, ErrorKind, LoginType, Parameters, Result as TeeResult,
};
use orb_secure_storage_proto::{Command, CommandId};
use uuid::Uuid;

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
            .map_err(|_| TeeError::new(ErrorKind::NotImplemented))?;

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
    trace!("TA create");
    Ok(())
}

#[ta_open_session]
fn open_session(params: &mut Parameters) -> TeeResult<()> {
    trace!("TA open session");
    if let Err(err) = open_session_inner(params) {
        error!("error: {}", err);
        return Err(TeeError::new(ErrorKind::Generic));
    }

    Ok(())
}

fn open_session_inner(params: &mut Parameters) -> Result<()> {
    let client_info = authenticate_euid(params)?;
    debug!(
        "uuid: {}, login_type: {}, euid: {}",
        client_info.uuid, client_info.login_type, client_info.effective_user_id,
    );

    Ok(())
}

#[ta_close_session]
fn close_session() {
    trace!("TA close session");
}

#[ta_destroy]
fn destroy() {
    trace!("TA destroy");
}

#[ta_invoke_command]
fn invoke_command(cmd_id: u32, params: &mut Parameters) -> TeeResult<()> {
    let cmd = Command::from_params(cmd_id, params)?;
    trace!("TA invoke command {:?}", cmd);
    match cmd {
        Command::Ping => info!("TA response: pong"),
        Command::Echo(payload) => info!("TA response: {}", payload),
    }

    Ok(())
}

fn uuidv5_from_euserid(euid: u32) -> Uuid {
    /// TEE Client UUID name space
    /// See https://elixir.bootlin.com/linux/v5.15.148/source/drivers/tee/tee_core.c#L32
    const NAMESPACE: Uuid = Uuid::from_fields(
        0x58ac9ca0,
        0x2086,
        0x4683,
        &[0xa1, 0xb8, 0xec, 0x4b, 0xc0, 0x8e, 0x01, 0xb6],
    );

    Uuid::new_v5(&NAMESPACE, format!("uid={:x}", euid).as_bytes())
}

struct ClientInfo {
    uuid: Uuid,
    effective_user_id: u32,
    login_type: LoginType,
}

fn authenticate_euid(session_params: &mut Parameters) -> Result<ClientInfo> {
    let alleged_euid = unsafe { session_params.0.as_value() }
        .context("failed to get params")?
        .a();
    let alleged_uuid = uuidv5_from_euserid(alleged_euid);
    let alleged_uuid_string = alleged_uuid.to_string();

    let identity = optee_utee::property::ClientIdentity
        .get()
        .expect("infallible");
    let login_type = identity.login_type();
    if login_type != LoginType::User {
        bail!("expected login type USER but got {}", login_type);
    }

    // Annoyingly, uuid doesn't implement PartialEq, so we have to compare the
    // strings. We should open a PR.
    let optee_uuid = identity.uuid();
    let actual_uuid_string = optee_uuid.to_string();
    if alleged_uuid_string != actual_uuid_string {
        bail!(
            "client app alleged that its euid was {} which should be uuid {}, \
            but we instead got uuid {}",
            alleged_euid,
            alleged_uuid_string,
            actual_uuid_string,
        );
    }

    Ok(ClientInfo {
        uuid: alleged_uuid,
        effective_user_id: alleged_euid,
        login_type,
    })
}

include!(concat!(env!("OUT_DIR"), "/user_ta_header.rs"));
