#![allow(clippy::single_match)]

use bindgen::callbacks::ParseCallbacks;
use color_eyre::{
    eyre::{ensure, Result, WrapErr},
    Help,
};
use convert_case::{Case, Casing};
use std::{
    env,
    path::{Path, PathBuf},
};

fn main() -> Result<()> {
    color_eyre::install()?;
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-env-changed=SEEK_SDK_PATH");

    let bindings = {
        let mut builder = bindgen::Builder::default()
            .header("wrapper.h")
            .allowlist_type("seekcamera.*")
            .allowlist_type("seekframe.*")
            .allowlist_function("seekcamera.*")
            .allowlist_function("seekframe.*")
            .prepend_enum_name(false)
            .newtype_enum("seekcamera_error_t")
            .newtype_enum("seekcamera_manager_event_t")
            .newtype_enum("seekcamera_color_palette_t")
            .newtype_enum("seekcamera_frame_format_t")
            .newtype_enum("seekcamera_agc_mode_t")
            .newtype_enum("seekcamera_app_resources_region_t")
            .newtype_enum("seekcamera_filter_t")
            .newtype_enum("seekcamera_filter_state_t")
            .newtype_enum("seekcamera_io_type_t")
            .newtype_enum("seekcamera_shutter_mode_t")
            .newtype_enum("seekcamera_temperature_unit_t")
            .newtype_enum("flat_scene_correction_id_t")
            .new_type_alias("seekcamera_serial_number_t")
            .parse_callbacks(Box::new(MyParseCallbacks))
            .parse_callbacks(Box::new(bindgen::CargoCallbacks))
            .clang_args(env::var("EXTRA_CLANG_CFLAGS")?.split_ascii_whitespace())
            .derive_debug(true)
            .impl_debug(true);

        if let Ok(v) = env::var("NIX_CFLAGS_COMPILE") {
            builder = builder.clang_args(v.split_ascii_whitespace());
        }

        if let Ok(v) = env::var("EXTRA_CLANG_CFLAGS") {
            builder = builder.clang_args(v.split_ascii_whitespace());
        }

        // We will allow users to specify the SDK path if they don't want to have it system installed.
        // For example, if on mac, you can use this to get around the fact that the seek sdk only
        // works on windows and linux.
        let sdk_path = env::var("SEEK_SDK_PATH").ok().map(PathBuf::from);
        if let Some(sdk_path) = sdk_path.as_ref() {
            let info = sdk_info(sdk_path)?;

            println!("cargo:rustc-link-search={}", info.lib_path.display());
            let include_arg = format!("-I{}", info.header_path.display());
            builder = builder.clang_arg(include_arg);
        }

        println!("cargo:rustc-link-lib=seekcamera");

        builder
            .generate()
            .wrap_err("Failed to generate bindings")
            .suggestion(
                "Be sure that the Seek camera sdk is installed or the path is specified with \
                 SEEK_SDK_PATH",
            )
            .with_note(|| format!("SEEK_SDK_PATH={sdk_path:?}"))?
    };

    let out_path = PathBuf::from(env::var("OUT_DIR")?);
    bindings.write_to_file(out_path.join("bindings.rs"))?;
    Ok(())
}

#[derive(Debug)]
struct MyParseCallbacks;
impl ParseCallbacks for MyParseCallbacks {
    /// Renames enum variants such that they are PascalCase and strips unecessary prefix.
    fn enum_variant_name(
        &self,
        enum_name: Option<&str>,
        original_variant_name: &str,
        _variant_value: bindgen::callbacks::EnumVariantValue,
    ) -> Option<String> {
        // Handle edge cases
        match original_variant_name {
            "SEEKCAMERA_SUCCESS" => return Some(String::from("Success")),
            _ => (),
        }

        let Some(enum_name) = enum_name else {
            return None;
        };
        // For some reason, bindgen prefixes the name with the "enum" keyword 🤨
        let enum_name = enum_name.strip_prefix("enum seekcamera_")?;

        let strip_variant_prefix = match enum_name {
            "error_t" => "SEEKCAMERA_ERROR_",
            "manager_event_t" => "SEEKCAMERA_MANAGER_EVENT_",
            "color_palette_t" => "SEEKCAMERA_COLOR_PALETTE_",
            "frame_format_t" => "SEEKCAMERA_FRAME_FORMAT_",
            "agc_mode_t" => "SEEKCAMERA_AGC_MODE_",
            "app_resources_region_t" => "SEEKCAMERA_APP_RESOURCES_REGION_",
            "filter_t" => "SEEKCAMERA_FILTER_",
            "filter_state_t" => "SEEKCAMERA_FILTER_STATE_",
            "io_type_t" => "SEEKCAMERA_IO_TYPE_",
            "shutter_mode_t" => "SEEKCAMERA_SHUTTER_MODE_",
            "temperature_unit_t" => "SEEKCAMERA_TEMPERATURE_UNIT_",
            "flat_scene_correction_id_t" => "SEEKCAMERA_FLAT_SCENE_CORRECTION_ID_",
            _ => return None,
        };
        let add_variant_prefix = match enum_name {
            "app_resources_region_t" => "R",
            "flat_scene_correction_id_t" => "Id",
            _ => "",
        };
        original_variant_name
            .strip_prefix(strip_variant_prefix)
            .map(|s| s.to_case(Case::Pascal))
            .map(|s| format!("{add_variant_prefix}{s}"))
    }

    fn item_name(&self, original_item_name: &str) -> Option<String> {
        // Edge cases
        match original_item_name {
            "seekcamera_t" => return None,
            _ => (),
        }

        if let Some(s) = original_item_name.strip_prefix("seekcamera_") {
            return Some(s.to_owned());
        }
        Some(original_item_name.to_owned())
    }
}

struct SdkInfo {
    header_path: PathBuf,
    lib_path: PathBuf,
}
fn sdk_info(path: &Path) -> Result<SdkInfo> {
    let sdk_path = path
        .canonicalize()
        .wrap_err("Failed to canonicalize `SEEK_SDK_PATH`. Does the folder exist?")
        .note(format!("SEEK_SDK_PATH={}", path.display()))?;

    let target = env::var("TARGET").unwrap();
    let target = match target.as_str() {
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu",
        "x86_64-unknown-linux-gnu" => "x86_64-linux-gnu",
        _ => panic!("Unsupported target architecture: {target}"),
    };

    let sdk_path = sdk_path.join(target);

    let header_path = sdk_path.join("include");
    ensure!(header_path.exists(), "Header path was {} but did not exist", header_path.display());

    let lib_path = sdk_path.join("lib");
    ensure!(lib_path.exists(), "Lib path was {} but did not exist", lib_path.display());

    Ok(SdkInfo { header_path, lib_path })
}
