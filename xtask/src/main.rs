use clap::{Parser, Subcommand};
use x::cmd::{build, deb, deploy, pre_commit};

#[derive(Parser, Debug)]
pub struct Cli {
    #[command(subcommand)]
    subcmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Build the select crate using `cargo zigbuild --release`. alias: 'b'
    #[command(alias = "b")]
    Build(build::Args),
    /// Build the select crate using `cargo zigbuild --release`, then package it into a `.deb` using
    /// `cargo deb`
    Deb(deb::Args),
    /// Lints and formats code. alias: 'pc'
    #[command(alias = "pc")]
    PreCommit,
    /// Builds a crate, packages it into a `.deb` and deploys it to an Orb. Automatically restarts
    /// any related systemd services.
    #[command(alias = "d")]
    Deploy(deploy::Args),
}

fn main() {
    let cmd = Cli::parse().subcmd;
    match cmd {
        Cmd::Build(args) => build::run(args),
        Cmd::Deb(args) => deb::run(args),
        Cmd::PreCommit => pre_commit::run(),
        Cmd::Deploy(args) => deploy::run(args),
    }
}
