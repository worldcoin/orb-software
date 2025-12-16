#![no_std]
#![no_main]

mod trace;

extern crate alloc;

include!(concat!(env!("OUT_DIR"), "/user_ta_header.rs"));

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use anyhow::{bail, Context, Result};
use core::{fmt::Write as _, write};
use optee_utee::object::PersistentObject;
use optee_utee::{
    property::PropertyKey as _, ta_close_session, ta_create, ta_destroy,
    ta_invoke_command, ta_open_session,
};
use optee_utee::{
    DataFlag, Error as TeeError, ErrorKind, GenericObject, LoginType,
    ObjectStorageConstants, Parameters, Result as TeeResult, Whence,
};
use orb_secure_storage_proto::{
    BufferTooSmallErr, CommandId, GetRequest, GetResponse, PutRequest, PutResponse,
    RequestT, ResponseT,
};
use uuid::Uuid;

// The fact that the optee macros don't themselves unambiguously reference box should
// probably be fixed upstream
use alloc::boxed::Box;

#[derive(Default)]
struct Ctx {
    client_info: ClientInfo,
    // Buffers used to reduce copies
    buf1: Vec<u8>,
    sbuf1: String,
}

fn make_prefixed_key(out: &mut String, client_info: &ClientInfo, user_key: &str) {
    // keys cannot live in shared memory, so this also serve the role of copying
    out.clear();
    write!(
        out,
        "v=1,euid={:#x}/{user_key}",
        client_info.effective_user_id
    )
    .expect("infallible");
}

impl Ctx {
    fn handle_get(&mut self, request: GetRequest) -> TeeResult<GetResponse> {
        debug!("{:?}", request);
        let GetRequest { key } = request;
        make_prefixed_key(&mut self.sbuf1, &self.client_info, &key);
        let mut obj = PersistentObject::open(
            ObjectStorageConstants::Private,
            self.sbuf1.as_bytes(),
            DataFlag::ACCESS_READ,
        )?;

        read_obj(&mut obj, &mut self.buf1, &self.sbuf1)?;

        Ok(GetResponse {
            val: self.buf1.clone(),
        })
    }

    fn handle_put(&mut self, request: PutRequest) -> TeeResult<PutResponse> {
        debug!("{:?}", request);
        let PutRequest { key, val } = request;
        make_prefixed_key(&mut self.sbuf1, &self.client_info, &key);
        let obj_result = PersistentObject::open(
            ObjectStorageConstants::Private,
            self.sbuf1.as_bytes(),
            DataFlag::ACCESS_WRITE | DataFlag::ACCESS_READ,
        );
        let mut obj = match obj_result {
            Ok(obj) => obj,
            Err(err) if err.kind() == ErrorKind::ItemNotFound => {
                PersistentObject::create(
                    ObjectStorageConstants::Private,
                    self.sbuf1.as_bytes(),
                    DataFlag::ACCESS_WRITE | DataFlag::ACCESS_READ,
                    None,
                    &[],
                )?
            }
            Err(err) => return Err(err),
        };
        debug!("opened successfully");

        read_obj(&mut obj, &mut self.buf1, &self.sbuf1)?;
        let prev_val = self.buf1.clone();

        obj.seek(0, Whence::DataSeekSet)?; // truncate does not change seek position
        obj.truncate(0)?;
        obj.write(&val)?;

        Ok(PutResponse { prev_val })
    }
}

fn request_from_params<T: RequestT>(params: &mut Parameters) -> TeeResult<T> {
    let mut prequest = unsafe { params.0.as_memref() }?;

    serde_json::from_slice(prequest.buffer()).map_err(|err| {
        error!("failed to deserialize request buffer: {:?}", err);

        TeeError::new(ErrorKind::BadFormat)
    })
}

fn response_to_params<T: ResponseT>(
    response: T,
    params: &mut Parameters,
) -> TeeResult<()> {
    let mut presponse = unsafe { params.1.as_memref() }?;
    let nbytes = response
        .serialize(presponse.buffer())
        .map_err(|BufferTooSmallErr {}| TeeError::new(ErrorKind::ShortBuffer))?;
    presponse.set_updated_size(nbytes);

    Ok(())
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
fn close_session(_ctx: &mut Ctx) {
    trace!("TA close session");
}

#[ta_destroy]
fn destroy() {
    trace!("TA destroy");
}

#[ta_invoke_command]
fn invoke_command(
    ctx: &mut Ctx,
    cmd_id: u32,
    params: &mut Parameters,
) -> TeeResult<()> {
    let cmd_id: CommandId = cmd_id.try_into().map_err(|err| {
        error!("unknown command: {:?}", err);
        TeeError::new(ErrorKind::BadFormat)
    })?;

    match cmd_id {
        CommandId::Put => {
            response_to_params(ctx.handle_put(request_from_params(params)?)?, params)
        }
        CommandId::Get => {
            response_to_params(ctx.handle_get(request_from_params(params)?)?, params)
        }
    }
}

fn read_obj(
    obj: &mut PersistentObject,
    buf: &mut Vec<u8>,
    prefixed_key: &str,
) -> TeeResult<()> {
    let nbytes = obj.info()?.data_size();
    buf.clear();
    buf.resize(nbytes, 0);
    obj.seek(0, Whence::DataSeekSet)?;
    if obj.read(buf)? != u32::try_from(nbytes).expect("overflow") {
        error!(
            "error: premature end of stream while reading {}",
            prefixed_key
        );
        return Err(TeeError::new(ErrorKind::BadState));
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
