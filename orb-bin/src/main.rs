//! This is a multicall binary that can be used to execute different Applets.
//! Outside of the machinery needed to register Applets, any other changes should simply be to add / remove Applets.

use orb_bin::OrbBin;
use orb_build_info::{make_build_info, BuildInfo};
use orb_monitoring_auth::{
    client::OrbMonitoringAuthClient, server::OrbMonitoringAuthServer,
};
use std::process::ExitCode;

const BUILD_INFO: BuildInfo = make_build_info!();

fn main() -> ExitCode {
    OrbBin::new(&BUILD_INFO)
        .register::<OrbMonitoringAuthClient>()
        .register::<OrbMonitoringAuthServer>()
        .run()
}
