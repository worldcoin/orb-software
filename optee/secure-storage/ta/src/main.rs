#![no_std]
#![no_main]

mod trace;

extern crate alloc;

include!(concat!(env!("OUT_DIR"), "/user_ta_header.rs"));

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
use orb_secure_storage_proto::{
    CommandId, GetRequest, GetResponse, PutRequest, PutResponse, Request, Response,
};
use uuid::Uuid;

// The fact that the optee macros don't themselves unambiguously reference box should
// probably be fixed upstream
use alloc::boxed::Box;

#[derive(Default)]
struct Ctx {
    client_info: ClientInfo,
}

trait FromInvoke: Sized {
    fn from_params<I>(id: I, params: &mut Parameters) -> TeeResult<Self>
    where
        I: TryInto<CommandId>,
        I::Error: core::fmt::Debug;
}

impl FromInvoke for Request {
    fn from_params<I>(id: I, params: &mut Parameters) -> TeeResult<Self>
    where
        I: TryInto<CommandId>,
        I::Error: core::fmt::Debug,
    {
        let cmd_id: CommandId = id.try_into().map_err(|err| {
            error!("unknown command: {:?}", err);
            TeeError::new(ErrorKind::BadFormat)
        })?;
        let mut prequest = unsafe { params.0.as_memref() }?;
        let request: Request =
            serde_json::from_slice(prequest.buffer()).map_err(|err| {
                error!("failed to deserialize request buffer: {:?}", err);
                TeeError::new(ErrorKind::BadFormat)
            })?;
        // Sanity check
        if request.id() != cmd_id {
            error!(
                "command id {:?} did not match request payload {:?}",
                cmd_id, request
            );
            return Err(TeeError::new(ErrorKind::BadFormat));
        }

        Ok(request)
    }
}

#[ta_create]
fn create() -> TeeResult<()> {
    info!("TA created");
    Ok(())
}

#[ta_open_session]
fn open_session(params: &mut Parameters, ctx: &mut Ctx) -> TeeResult<()> {
    trace!("TA open session");
    ctx.client_info = match open_session_inner(params) {
        Ok(client_info) => client_info,
        Err(err) => {
            error!("error: {}", err);
            return Err(TeeError::new(ErrorKind::Generic));
        }
    };

    Ok(())
}

fn open_session_inner(params: &mut Parameters) -> Result<ClientInfo> {
    let client_info = validate_euid(params)?;
    debug!(
        "opened session with uuid: {}, login_type: {}, euid: {}",
        client_info.uuid, client_info.login_type, client_info.effective_user_id,
    );

    Ok(client_info)
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
    let request = Request::from_params(cmd_id, params)?;
    trace!("TA invoke command {:?}", request);
    let response = match request {
        Request::Ping => Response::Ping,
        Request::Put(request) => Response::Put(handle_put(request)),
        Request::Get(request) => Response::Get(handle_get(request)),
    };

    let serialized_response = serde_json::to_vec(&response).expect("infallible"); // todo: elide the copy
    let mut presponse = unsafe { params.1.as_memref() }?;
    let nbytes = serialized_response.len();
    let presponse_buf = presponse.buffer();
    if presponse_buf.len() < nbytes {
        return Err(TeeError::new(ErrorKind::ShortBuffer));
    }
    let presponse_buf = &mut presponse_buf[0..nbytes];
    presponse_buf.copy_from_slice(serialized_response.as_slice());
    presponse.set_updated_size(nbytes);

    Ok(())
}

fn handle_get(request: GetRequest) -> GetResponse {
    debug!("{:?}", request);

    GetResponse { val: None } // TODO: unstub
}

fn handle_put(request: PutRequest) -> PutResponse {
    debug!("{:?}", request);

    PutResponse { prev_val: None } // TODO: unstub
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

impl Default for ClientInfo {
    fn default() -> Self {
        Self {
            uuid: Default::default(),
            effective_user_id: Default::default(),
            login_type: LoginType::Public,
        }
    }
}

fn validate_euid(session_params: &mut Parameters) -> Result<ClientInfo> {
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
