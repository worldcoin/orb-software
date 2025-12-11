#![forbid(unsafe_code)]

use clap::Parser;
use color_eyre::Result;
use orb_secure_storage_ca::Client;

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
}

impl Args {
    fn run(self) -> Result<()> {
        match self {
            Self::Get(args) => args.run(),
            Self::Put(args) => args.run(),
        }
    }
}

#[derive(Debug, Parser)]
struct GetArgs {
    key: String,
}

impl GetArgs {
    fn run(self) -> Result<()> {
        let mut client = Client::new()?;
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
        let mut client = Client::new()?;
        let oldval = client.put(&self.key, self.value.as_bytes())?;
        println!("old value: {oldval:?}");
        Ok(())
    }
}
