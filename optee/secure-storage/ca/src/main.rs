#![forbid(unsafe_code)]

use eyre::{Result, WrapErr as _};
use optee_teec::{Context, Operation, ParamTmpRef, ParamType, Session, Uuid};
use optee_teec::{ParamNone, ParamValue};
use orb_secure_storage_proto::{Request, Response, UUID};
use tracing::{debug, instrument};

fn invoke_request(session: &mut Session, request: Request) -> Result<Response> {
    let mut buffer = vec![0; 1024];
    let serialized_request = serde_json::to_vec(&request).expect("infallible");
    let prequest = ParamTmpRef::new_input(serialized_request.as_slice());
    let presponse = ParamTmpRef::new_output(&mut buffer);

    let mut operation = Operation::new(0, prequest, presponse, ParamNone, ParamNone);

    session
        .invoke_command(request.id() as u32, &mut operation)
        .wrap_err("failed to invoke optee command")?;
    let response_len = operation.parameters().1.updated_size();
    let response_buf = &buffer[0..response_len];

    serde_json::from_slice(response_buf).wrap_err("failed to deserialize response")
}

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let mut ctx = Context::new()?;
    let uuid = Uuid::parse_str(UUID)?;
    let mut session = open_session(&mut ctx, uuid)?;

    let req = Request::Ping;
    let response = invoke_request(&mut session, req)?;

    debug!(?response);

    Ok(())
}

#[instrument(skip_all, fields(uuid=uuid.to_string()))]
fn open_session(ctx: &mut Context, uuid: Uuid) -> Result<Session> {
    let euid = rustix::process::geteuid().as_raw();
    debug!(?euid);
    let mut euid_op = Operation::new(
        0,
        ParamValue::new(euid, 0, ParamType::ValueInput),
        ParamNone,
        ParamNone,
        ParamNone,
    );
    let session = Session::new(
        ctx,
        uuid,
        optee_teec::ConnectionMethods::LoginUser,
        Some(&mut euid_op),
    )?;

    Ok(session)
}
