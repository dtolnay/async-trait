fn main() {
    // Warning: build.rs is not published to crates.io.

    println!("cargo:rustc-check-cfg=cfg(async_trait_nightly_testing)");
}
