#![forbid(unsafe_code)]

use clap::Parser;
use color_eyre::Result;
use eyre::WrapErr as _;

use orb_secure_storage_ca::{optee::OpteeBackend, Client, StorageDomain};

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    args.run()
}

#[derive(Debug, Parser)]
enum Args {
    Get(GetArgs),
    Put(PutArgs),
    Version(VersionArgs),
}

impl Args {
    fn run(self) -> Result<()> {
        match self {
            Self::Get(args) => args.run(),
            Self::Put(args) => args.run(),
            Self::Version(args) => args.run(),
        }
    }
}

#[derive(Debug, Parser)]
struct GetArgs {
    key: String,
}

impl GetArgs {
    fn run(self) -> Result<()> {
        let mut client = make_client()?;
        let val = client.get(&self.key)?;
        println!("got value: {val:?}");
        Ok(())
    }
}

#[derive(Debug, Parser)]
struct PutArgs {
    key: String,
    value: String,
}

impl PutArgs {
    fn run(self) -> Result<()> {
        let mut client = make_client()?;
        let oldval = client.put(&self.key, self.value.as_bytes())?;
        println!("old value: {oldval:?}");
        Ok(())
    }
}

#[derive(Debug, Parser)]
struct VersionArgs;

impl VersionArgs {
    fn run(self) -> Result<()> {
        let mut client = make_client()?;
        let val = client.version()?;
        println!("{val}");

        Ok(())
    }
}
fn make_client() -> Result<Client<OpteeBackend>> {
    let mut ctx =
        optee_teec::Context::new().wrap_err("failed to create optee context")?;

    Client::new(&mut ctx, StorageDomain::WifiProfiles)
        .wrap_err("failed to create secure storage client")
}
