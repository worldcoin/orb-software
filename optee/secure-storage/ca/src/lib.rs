#![forbid(unsafe_code)]

use eyre::{Result, WrapErr as _};
use optee_teec::{Context, Operation, ParamTmpRef, ParamType, Session, Uuid};
use optee_teec::{ParamNone, ParamValue};
use orb_secure_storage_proto::{GetRequest, PutRequest, RequestT, ResponseT};
use tracing::debug;

pub struct Client {
    _ctx: Context,
    session: Session,
    span: tracing::Span,
}

impl Client {
    pub fn new() -> Result<Self> {
        let uuid = Uuid::parse_str(orb_secure_storage_proto::UUID).expect("infallible");
        let euid = rustix::process::geteuid().as_raw();
        let span = tracing::info_span!(
            "orb-secure-storage-client",
            uuid = uuid.to_string(),
            ?euid
        );
        let span_guard = span.enter();

        let mut ctx = Context::new().wrap_err("failed to create TEE context")?;
        debug!(?euid);
        let mut euid_op = Operation::new(
            0,
            ParamValue::new(euid, 0, ParamType::ValueInput),
            ParamNone,
            ParamNone,
            ParamNone,
        );
        let session = Session::new(
            &mut ctx,
            uuid,
            optee_teec::ConnectionMethods::LoginUser,
            Some(&mut euid_op),
        )?;
        drop(span_guard);

        Ok(Self {
            _ctx: ctx,
            session,
            span,
        })
    }

    pub fn get(&mut self, key: &str) -> Result<Vec<u8>> {
        let _span = self.span.enter();
        let request = GetRequest {
            key: key.to_string(),
        };
        let response = invoke_request(&mut self.session, request)?;

        Ok(response.val)
    }

    pub fn put(&mut self, key: &str, value: &[u8]) -> Result<Vec<u8>> {
        let _span = self.span.enter();
        let request = PutRequest {
            key: key.to_owned(),
            val: value.to_owned(),
        };
        let response = invoke_request(&mut self.session, request)?;

        Ok(response.prev_val)
    }
}

fn invoke_request<R: RequestT>(
    session: &mut Session,
    request: R,
) -> Result<R::Response> {
    let mut buffer = vec![0; R::MAX_RESPONSE_SIZE as usize];
    let serialized_request = serde_json::to_vec(&request).expect("infallible");
    let prequest = ParamTmpRef::new_input(serialized_request.as_slice());
    let presponse = ParamTmpRef::new_output(&mut buffer);

    let mut operation = Operation::new(0, prequest, presponse, ParamNone, ParamNone);

    session
        .invoke_command(request.id() as u32, &mut operation)
        .wrap_err("failed to invoke optee command")?;
    let response_len = operation.parameters().1.updated_size();
    let response_buf = &mut buffer[0..response_len];
    let response = R::Response::deserialize(response_buf)?;

    Ok(response)
}
