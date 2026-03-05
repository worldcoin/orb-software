use std::{
    env,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=include/sm_types.h");

    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .derive_debug(true)
        .impl_debug(true);

    let local_include = PathBuf::from("include");
    if local_include.exists() {
        builder = builder.clang_arg(format!("-I{}", local_include.display()));
    }

    let lib = pkg_config::probe_library("plug-and-trust")
        .expect("Failed to find plug-and-trust via pkg-config.");
    for path in lib.include_paths {
        builder = add_include_tree(builder, &path);
    }

    let bindings = builder
        .generate()
        .expect("Failed to generate plug-and-trust bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Failed to write bindings");
}

fn add_include_tree(mut builder: bindgen::Builder, path: &Path) -> bindgen::Builder {
    builder = builder.clang_arg(format!("-I{}", path.display()));

    for dir in ["inc", "default"] {
        let candidate = path.join(dir);
        if candidate.exists() {
            builder = builder.clang_arg(format!("-I{}", candidate.display()));
        }
    }

    let nested = path.join("plug-and-trust");
    if nested.exists() {
        builder = builder.clang_arg(format!("-I{}", nested.display()));
        for dir in ["inc", "default"] {
            let candidate = nested.join(dir);
            if candidate.exists() {
                builder = builder.clang_arg(format!("-I{}", candidate.display()));
            }
        }
    }

    builder
}
