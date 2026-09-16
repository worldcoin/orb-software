use clap::Command;
use orb_build_info::BuildInfo;
use prelude::applet::Applet;
use std::process::ExitCode;

pub struct OrbBin {
    applets: Vec<Box<dyn Applet>>,
    build_info: &'static BuildInfo,
}

impl OrbBin {
    pub fn new(build_info: &'static BuildInfo) -> Self {
        Self {
            applets: Vec::new(),
            build_info,
        }
    }

    pub fn register<A: Default + Applet + 'static>(mut self) -> Self {
        self.applets.push(Box::new(A::default()));
        self
    }

    pub fn run(self) -> ExitCode {
        let matches = Command::new("orb-bin")
            .multicall(true)
            // Multicall parses argv[0] as the first subcommand, so registering the
            // canonical name here supports both `orb-bin <applet>` and symlink invocation.
            .subcommand(
                Command::new("orb-bin")
                    .about(env!("CARGO_PKG_DESCRIPTION"))
                    .version(self.build_info.version)
                    .arg_required_else_help(true)
                    .subcommand_value_name("APPLET")
                    .subcommand_help_heading("APPLETS")
                    .subcommands(self.subcommands()),
            )
            .subcommand_value_name("APPLET")
            .subcommand_help_heading("APPLETS")
            .subcommands(self.subcommands())
            .get_matches();

        let (cmd, matches) = match matches.subcommand() {
            Some(("orb-bin", matches)) => matches.subcommand(),
            subcommand => subcommand,
        }
        .expect("could not parse subcomand");

        let applet = self
            .applets
            .into_iter()
            .find(|applet| applet.name() == cmd)
            .unwrap_or_else(|| panic!("{cmd} is not a registered applet"));

        let tel_flusher = orb_telemetry::TelemetryConfig::new()
            .with_journald(applet.name())
            .init();

        let exit_code = applet.main(matches);
        tel_flusher.flush_blocking();

        exit_code
    }

    fn subcommands(&self) -> Vec<Command> {
        self.applets
            .iter()
            .map(|applet| Command::new(applet.name()).about(applet.about()))
            .collect()
    }
}
