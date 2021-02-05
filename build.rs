fn main() {
    if let Some(true) = version_check::is_max_version("1.46") {
        println!("cargo:rustc-cfg=self_span_hack");
    }
}
