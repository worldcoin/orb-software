//! An implementation of [`BackendT`] using OP-TEE secure OS.

use eyre::{Result, WrapErr as _};
use optee_teec::{
    Context, Operation, ParamNone, ParamTmpRef, ParamType, ParamValue, Session, Uuid,
};
use orb_secure_storage_proto::CommandId;
use rustix::process::Uid;

use crate::{BackendT, SessionT};

/// Implementation of [`BackendT`] that uses OP-TEE.
pub struct OpteeBackend;

/// Implementation of [`SessionT`] that uses OP-TEE.
pub struct OpteeSession(Session);

impl BackendT for OpteeBackend {
    type Session = OpteeSession;
    type Context = Context;

    fn open_session(ctx: &mut Self::Context, euid: Uid) -> Result<Self::Session> {
        let uuid = Uuid::parse_str(orb_secure_storage_proto::UUID).expect("infallible");
        let mut euid_op = Operation::new(
            0,
            ParamValue::new(euid.as_raw(), 0, ParamType::ValueInput),
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

        Ok(OpteeSession(session))
    }
}

impl SessionT for OpteeSession {
    fn invoke(
        &mut self,
        command_id: CommandId,
        serialized_request: &[u8],
        response_buf: &mut [u8],
    ) -> Result<usize> {
        let prequest = ParamTmpRef::new_input(serialized_request);
        let presponse = ParamTmpRef::new_output(response_buf);
        let mut operation =
            Operation::new(0, prequest, presponse, ParamNone, ParamNone);

        self.0
            .invoke_command(command_id as u32, &mut operation)
            .wrap_err("failed to invoke optee command")?;
        let response_len = operation.parameters().1.updated_size();

        Ok(response_len)
    }
}
