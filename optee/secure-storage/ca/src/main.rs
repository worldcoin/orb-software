#![forbid(unsafe_code)]

use optee_teec::{Context, Operation, ParamType, Session, Uuid};
use optee_teec::{ParamNone, ParamValue};
use orb_secure_storage_proto::{CommandId, UUID};

fn hello_world(session: &mut Session) -> optee_teec::Result<()> {
    let mut ping_op = Operation::new(0, ParamNone, ParamNone, ParamNone, ParamNone);
    session.invoke_command(CommandId::Ping as u32, &mut ping_op)?;

    let mut echo_op = Operation::new(
        0,
        ParamValue::new(67, 0, ParamType::ValueInput),
        ParamNone,
        ParamNone,
        ParamNone,
    );
    session.invoke_command(CommandId::Echo as u32, &mut echo_op)?;

    Ok(())
}

fn main() -> optee_teec::Result<()> {
    let mut ctx = Context::new()?;
    let uuid = Uuid::parse_str(UUID)?;
    let mut session = ctx.open_session(uuid)?;

    println!("running hello world CA");
    hello_world(&mut session)?;
    println!("Exiting hello world CA");

    Ok(())
}
