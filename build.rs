fn main() {
    // Warning: build.rs is not published to crates.io.

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(async_trait_nightly_testing)");
}
