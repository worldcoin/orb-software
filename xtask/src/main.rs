use clap::{Parser, Subcommand};
use color_eyre::Result;
use x::cmd::{android, build, deb, deploy, pre_commit, test, test_watch};

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
    /// Builds the whole workspace for `aarch64-linux-android`, skipping
    /// crates marked unsupported via `[package.metadata.orb]
    /// unsupported_targets`.
    AndroidBuild(android::BuildArgs),
    /// Build Android payloads and package each into a signed
    /// `<crate>.apex`
    AndroidApex(android::ApexArgs),
    /// Build the select crate using `cargo zigbuild --release`, then package it into a `.deb` using
    /// `cargo deb`
    Deb(deb::Args),
    /// Lints and formats code. alias: 'pc'
    ///
    #[command(alias = "pc")]
    PreCommit,
    /// Builds a crate, packages it into a `.deb` and deploys it to an Orb. Automatically restarts
    /// any related systemd services.
    #[command(alias = "d")]
    Deploy(deploy::Args),
    /// Run tests for the given crates via `cargo nextest run`. alias: 't'
    #[command(alias = "t")]
    Test(test::Args),
    /// Watch and run tests for the given crates via `bacon` + `cargo nextest run`. alias: 'tw'
    #[command(alias = "tw")]
    TestWatch(test_watch::Args),
    #[command(subcommand)]
    Optee(orb_x_optee::Subcommands),
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let cmd = Cli::parse().subcmd;

    match cmd {
        Cmd::Build(args) => build::run(args),
        Cmd::AndroidBuild(args) => android::run_build(args),
        Cmd::AndroidApex(args) => android::run_apex(args),
        Cmd::Deb(args) => deb::run(args),
        Cmd::PreCommit => pre_commit::run(),
        Cmd::Deploy(args) => deploy::run(args),
        Cmd::Test(args) => test::run(args),
        Cmd::TestWatch(args) => test_watch::run(args),
        Cmd::Optee(args) => {
            tracing_subscriber::fmt::init();
            args.run()
        }
    }
}
