// NOTE / REMINDER: Setting env vars in tests will clobber env vars in other tests. This means that
// each test *must* use a unique prefix for its environment variables to ensure they don't clobber
// other tests (and potentially cause non-deterministic error successes/failures depending on
// concurrent execution order).

use std::{path::Path, time::Duration};

use clap::Parser as _;
use figment::Jail;
use orb_update_agent_core::{file_location::LocalOrRemote, Slot};

use crate::settings::{Backend, Settings};

const CFG_FILE_CONTENTS_TRUTHY: &str = r#"
    versions = "/config/versions"
    verify_manifest_signature_against = "stage"
    clientkey = "/config/clientkey"
    workspace = "/config/workspace"
    downloads = "/config/downloads"
    id = "/config/id"
    update_location = "/config/update_location"
    nodbus = true
    skip_version_asserts = true
    noupdate = true
    download_delay = 3000
    recovery = true
"#;

const CFG_FILE_CONTENTS_FALSY: &str = r#"
    versions = "/config/versions"
    verify_manifest_signature_against = "stage"
    clientkey = "/config/clientkey"
    workspace = "/config/workspace"
    downloads = "/config/downloads"
    id = "/config/id"
    update_location = "/config/update_location"
    nodbus = false
    skip_version_asserts = false
    noupdate = false
    download_delay = 3000
    recovery = false
"#;

fn make_args(args: &str) -> Result<crate::Args, clap::Error> {
    crate::Args::try_parse_from(str::split_ascii_whitespace(args))
}

/// Sets environment variables in order to override the default config
///
/// boolean values are set to the value given by `set_bools_to`.
fn set_env(jail: &mut Jail, set_bools_to: bool) {
    let bool_str = if set_bools_to { "true" } else { "false" };
    jail.set_env("update_agent_clientkey", "/env/clientkey");
    jail.set_env("update_agent_workspace", "/env/workspace");
    jail.set_env("update_agent_downloads", "/env/downloads");
    jail.set_env("update_agent_id", "/env/id");
    jail.set_env("update_agent_verify_manifest_signature_against", "prod");
    jail.set_env("update_agent_update_location", "/env/update_location");
    jail.set_env("update_agent_versions", "/env/versions");
    jail.set_env("update_agent_nodbus", bool_str);
    jail.set_env("update_agent_skip_version_asserts", bool_str);
    jail.set_env("update_agent_noupdate", bool_str);
    jail.set_env("update_agent_recovery", bool_str);
    jail.set_env("update_agent_download_delay", "4000");
}

#[test]
fn test_cli_args_override_config_file_and_env_vars() {
    const CLI_ARGS: &str = r#"
    update_agent
        --clientkey /args/clientkey
        --workspace /args/workspace
        --downloads /args/downloads
        --id /args/id
        --verify-manifest-signature-against prod
        --update-location /args/update_location
        --versions /args/versions
        --nodbus
        --skip-version-asserts
        --noupdate
        --download-delay 5000
        --recovery
    "#;

    Jail::expect_with(|jail| {
        jail.create_file("config.toml", CFG_FILE_CONTENTS_FALSY)?;
        set_env(jail, false);
        // change the binary settings to something different from the cli args.
        let args = make_args(CLI_ARGS).unwrap();
        let current_slot = Slot::A;
        let crate::Settings {
            versions,
            verify_manifest_signature_against,
            clientkey,
            active_slot,
            workspace,
            downloads,
            id,
            update_location,
            nodbus,
            skip_version_asserts,
            noupdate,
            recovery,
            download_delay,
            token,
        } = Settings::get(&args, "config.toml", "update_agent_", current_slot)?;
        assert_eq!(active_slot, current_slot);
        assert_eq!(clientkey.as_os_str(), args.clientkey.unwrap().as_str());
        assert_eq!(workspace.as_os_str(), args.workspace.unwrap().as_str());
        assert_eq!(downloads.as_os_str(), args.downloads.unwrap().as_str());
        assert_eq!(id, args.id.unwrap());
        assert_eq!(
            verify_manifest_signature_against,
            args.verify_manifest_signature_against.unwrap()
        );
        assert_eq!(
            update_location,
            LocalOrRemote::parse(&args.update_location.unwrap()).unwrap()
        );
        assert_eq!(versions.as_os_str(), args.versions.unwrap().as_str());
        assert_eq!(nodbus, args.nodbus);
        assert_eq!(skip_version_asserts, args.skip_version_asserts);
        assert_eq!(noupdate, args.noupdate);
        assert_eq!(recovery, args.recovery);
        assert_eq!(
            download_delay.as_millis(),
            args.download_delay.map(u128::from).unwrap()
        );
        assert_eq!(token, args.token);
        Ok(())
    })
}

#[test]
fn test_cli_args_override_config_file() {
    const CLI_ARGS: &str = r#"
    update_agent
        --clientkey /args/clientkey
        --workspace /args/workspace
        --downloads /args/downloads
        --id /args/id
        --verify-manifest-signature-against prod
        --update-location /args/update_location
        --versions /args/versions
        --nodbus
        --skip-version-asserts
        --noupdate
        --download-delay 5000
        --recovery
    "#;

    Jail::expect_with(|jail| {
        jail.create_file("config.toml", CFG_FILE_CONTENTS_FALSY)?;
        // change the binary settings to something different from the cli args.
        let args = make_args(CLI_ARGS).unwrap();
        let current_slot = Slot::A;
        let crate::Settings {
            versions,
            verify_manifest_signature_against,
            clientkey,
            active_slot,
            workspace,
            downloads,
            id,
            update_location,
            nodbus,
            skip_version_asserts,
            noupdate,
            recovery,
            download_delay,
            token,
        } = Settings::get(&args, "config.toml", "update_agent_", current_slot)?;
        assert_eq!(active_slot, current_slot);
        assert_eq!(clientkey.as_os_str(), args.clientkey.unwrap().as_str());
        assert_eq!(workspace.as_os_str(), args.workspace.unwrap().as_str());
        assert_eq!(downloads.as_os_str(), args.downloads.unwrap().as_str());
        assert_eq!(id, args.id.unwrap());
        assert_eq!(
            verify_manifest_signature_against,
            args.verify_manifest_signature_against.unwrap()
        );
        assert_eq!(
            update_location,
            LocalOrRemote::parse(&args.update_location.unwrap()).unwrap()
        );
        assert_eq!(versions.as_os_str(), args.versions.unwrap().as_str());
        assert_eq!(nodbus, args.nodbus);
        assert_eq!(skip_version_asserts, args.skip_version_asserts);
        assert_eq!(noupdate, args.noupdate);
        assert_eq!(recovery, args.recovery);
        assert_eq!(
            download_delay.as_millis(),
            args.download_delay.map(u128::from).unwrap()
        );
        assert_eq!(token, args.token);
        Ok(())
    })
}

#[test]
fn test_only_setting_config_file_works() {
    Jail::expect_with(|jail| {
        jail.create_file("config.toml", CFG_FILE_CONTENTS_TRUTHY)?;
        let args = make_args("update-agent").unwrap();
        let crate::Settings {
            versions,
            verify_manifest_signature_against,
            clientkey,
            active_slot,
            workspace,
            downloads,
            id,
            update_location,
            nodbus,
            skip_version_asserts,
            noupdate,
            recovery,
            download_delay,
            token,
        } = Settings::get(&args, "config.toml", "update_agent_", Slot::A)?;
        assert_eq!(active_slot, Slot::A);
        assert_eq!(clientkey, Path::new("/config/clientkey"));
        assert_eq!(workspace, Path::new("/config/workspace"));
        assert_eq!(downloads, Path::new("/config/downloads"));
        assert_eq!(id, "/config/id");
        assert_eq!(verify_manifest_signature_against, Backend::Stage);
        assert_eq!(
            update_location,
            LocalOrRemote::parse("/config/update_location").unwrap()
        );
        assert_eq!(versions, Path::new("/config/versions"));
        assert!(nodbus);
        assert!(skip_version_asserts);
        assert!(noupdate);
        assert!(recovery);
        assert_eq!(download_delay, Duration::from_millis(3000));
        assert!(token.is_none());
        Ok(())
    })
}

#[test]
fn test_env_override_config_file() {
    Jail::expect_with(|jail| {
        jail.create_file("config.toml", CFG_FILE_CONTENTS_FALSY)?;
        let args = make_args("update-agent").unwrap();
        set_env(jail, true);
        let current_slot = Slot::A;
        let crate::Settings {
            versions,
            verify_manifest_signature_against,
            clientkey,
            active_slot,
            workspace,
            downloads,
            id,
            update_location,
            nodbus,
            skip_version_asserts,
            noupdate,
            recovery,
            download_delay,
            token,
        } = Settings::get(&args, "config.toml", "update_agent_", current_slot)?;
        assert_eq!(active_slot, current_slot);
        assert_eq!(clientkey, Path::new("/env/clientkey"));
        assert_eq!(workspace, Path::new("/env/workspace"));
        assert_eq!(downloads, Path::new("/env/downloads"));
        assert_eq!(id, "/env/id");
        assert_eq!(verify_manifest_signature_against, Backend::Prod);
        assert_eq!(
            update_location,
            LocalOrRemote::parse("/env/update_location").unwrap()
        );
        assert_eq!(versions, Path::new("/env/versions"));
        assert!(nodbus);
        assert!(skip_version_asserts);
        assert!(noupdate);
        assert!(recovery);
        assert_eq!(download_delay, Duration::from_millis(4000));
        assert!(token.is_none());
        Ok(())
    })
}

const PROD_CFG_FILE_CONTENTS: &str = r#"
    versions = "/config/versions"
    components = "/config/components"
    verify_manifest_signature_against = "prod"
    cacert = "/config/downloads"
    clientkey = "/config/clientkey"
    update_location = "/config/update_location"
    workspace = "/config/workspace"
    downloads = "/config/downloads"
    download_delay = 36000
    recovery = false
    nodbus = false
    noupdate = false
    skip_version_asserts = false
"#;

const PROD_CLI_ARGS: &str = r#"
update_agent
    --id /args/id
"#;

#[test]
fn production_config() {
    Jail::expect_with(|jail| {
        let cfg_file_contents = PROD_CFG_FILE_CONTENTS;
        jail.create_file("config.toml", cfg_file_contents)?;
        let args = make_args(PROD_CLI_ARGS).unwrap();
        let crate::Settings {
            versions,
            verify_manifest_signature_against,
            clientkey,
            active_slot,
            workspace,
            downloads,
            id,
            update_location,
            nodbus,
            skip_version_asserts,
            noupdate,
            recovery,
            download_delay,
            token,
        } = Settings::get(&args, "config.toml", "update_agent_", Slot::A)?;
        assert_eq!(active_slot, Slot::A);
        assert_eq!(clientkey, Path::new("/config/clientkey"));
        assert_eq!(workspace, Path::new("/config/workspace"));
        assert_eq!(downloads, Path::new("/config/downloads"));
        assert_eq!(id, "/args/id");
        assert_eq!(verify_manifest_signature_against, Backend::Prod);
        assert_eq!(
            update_location,
            LocalOrRemote::parse("/config/update_location").unwrap()
        );
        assert_eq!(versions, Path::new("/config/versions"));
        assert!(!nodbus);
        assert!(!skip_version_asserts);
        assert!(!noupdate);
        assert!(!recovery);
        assert_eq!(download_delay, Duration::from_millis(36000));
        assert!(token.is_none());
        Ok(())
    })
}
