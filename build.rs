use std::env;
use std::process::Command;
use std::str;

fn main() {
    println!("cargo:rerun-if-env-changed=DOCS_RS");

    let compiler = match rustc_minor_version() {
        Some(compiler) => compiler,
        None => return,
    };

    if compiler < 45 {
        println!("cargo:rustc-cfg=no_span_mixed_site");
    }

    if compiler < 47 {
        println!("cargo:rustc-cfg=self_span_hack");
    }

    if compiler >= 75 && env::var_os("DOCS_RS").is_none() {
        println!("cargo:rustc-cfg=native_async_fn_in_trait");
    }
}

fn rustc_minor_version() -> Option<u32> {
    let rustc = env::var_os("RUSTC")?;
    let output = Command::new(rustc).arg("--version").output().ok()?;
    let version = str::from_utf8(&output.stdout).ok()?;
    let mut pieces = version.split('.');
    if pieces.next() != Some("rustc 1") {
        return None;
    }
    pieces.next()?.parse().ok()
}
