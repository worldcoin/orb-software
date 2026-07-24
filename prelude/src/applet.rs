use clap::ArgMatches;
use std::process::ExitCode;

pub trait Applet {
    /// The name of the Applet. Will also be used as both command name and syslog identifier for journald.
    fn name(&self) -> &'static str;
    /// Small description of applet.
    fn about(&self) -> &'static str;
    /// Entrypoint for the Applet.
    fn main(&self, args: &ArgMatches) -> ExitCode;
}
