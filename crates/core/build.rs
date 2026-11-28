fn main() {
    // The optional `screencapturekit` crate links a Swift bridge: this crate's tests and
    // examples need the system Swift runtime on their rpath. An app enabling the feature
    // needs the same link argument.
    if std::env::var_os("CARGO_FEATURE_SCREENCAPTUREKIT").is_some() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
