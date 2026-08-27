fn main() {
    let host = std::env::var("HOST").expect("cargo always sets HOST for build scripts");
    println!("cargo:rustc-env=HOST_TRIPLE={host}");
}
