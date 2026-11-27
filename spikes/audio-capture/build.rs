fn main() {
    // The screencapturekit crate links a Swift bridge: the binary needs the
    // system Swift runtime on its rpath.
    if std::env::var_os("CARGO_FEATURE_SCK").is_some() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
