use clap::Parser;
use color_eyre::Result;

#[derive(Debug, Parser)]
pub struct Cmd {
    #[arg()]
    cmd: String,
}

impl Cmd {
    pub async fn run(self) -> Result<()> {
        todo!()
    }
}
