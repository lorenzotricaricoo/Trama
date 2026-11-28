//! Build script of the core: the Swift runtime path for the optional ScreenCaptureKit
//! fallback, and the verification of the transcription runtime's native libraries.
//!
//! **Native libraries (ADR 0007).**
//! The `sherpa-onnx-sys` build script downloads a prebuilt archive of sherpa-onnx and
//! ONNX Runtime from the sherpa-onnx GitHub release and links it statically, without
//! checking it. This script runs after it (the core depends on `sherpa-onnx-sys`
//! directly, and Cargo runs the build script of a `links` dependency first), finds the
//! archive in its cache and compares its SHA-256 with the value pinned below. A
//! mismatch stops the build before anything is linked.
//!
//! See `docs/architecture/transcription.md` ("Native libraries").

use std::env;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The sherpa-onnx version pinned in `Cargo.toml`; the archive names contain it.
const SHERPA_ONNX_VERSION: &str = "2.0.3";

/// SHA-256 of the static-library archives of sherpa-onnx v2.0.3, as published on the
/// GitHub release (`gh api repos/k2-fsa/sherpa-onnx/releases/tags/v2.0.3`, field
/// `digest` of each asset). One per target that `sherpa-onnx-sys` supports statically.
const PINNED_ARCHIVES: &[(&str, &str)] = &[
    (
        "sherpa-onnx-v2.0.3-osx-arm64-static-lib.tar.bz2",
        "c86468ad8f79b679550f0865cc1a67747f58b8cc8780623ed7d8367fd4b541a3",
    ),
    (
        "sherpa-onnx-v2.0.3-osx-x64-static-lib.tar.bz2",
        "aa2669f175bc3b496788308e211e1bbb792ca7ba654f79db69bf20bef1d597a0",
    ),
    (
        "sherpa-onnx-v2.0.3-win-x64-static-MT-Release-lib.tar.bz2",
        "2dfaeacf899e8d21584caab54a55933b481b5dccbb3615e92daecc6f0618e781",
    ),
    (
        "sherpa-onnx-v2.0.3-win-arm64-static-MT-Release-lib.tar.bz2",
        "5496543fcb89da9cfadb7115669d55677adc35e5e823e0858de2af826e74768c",
    ),
    (
        "sherpa-onnx-v2.0.3-linux-x64-static-lib.tar.bz2",
        "f927e695d29b8c7ea3fda97eed215204b210fd57f9faf990a93377bac098ad57",
    ),
    (
        "sherpa-onnx-v2.0.3-linux-aarch64-static-lib.tar.bz2",
        "11f9860fd4dc18af397a493ee9bcb88d18c6ba1b235fb3041ec1a3ef6522364b",
    ),
];

fn main() {
    // The optional `screencapturekit` crate links a Swift bridge: this crate's tests and
    // examples need the system Swift runtime on their rpath. An app enabling the feature
    // needs the same link argument.
    if env::var_os("CARGO_FEATURE_SCREENCAPTUREKIT").is_some() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    verify_sherpa_onnx();
}

/// Checks the sherpa-onnx archive against its pinned SHA-256; panics on a mismatch.
fn verify_sherpa_onnx() {
    println!("cargo:rerun-if-changed=build.rs");
    for var in [
        "DOCS_RS",
        "SHERPA_ONNX_LIB_DIR",
        "SHERPA_ONNX_ARCHIVE_DIR",
        "CARGO_TARGET_DIR",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    // docs.rs builds without native libraries; so does sherpa-onnx-sys.
    if env::var_os("DOCS_RS").is_some() {
        return;
    }
    if let Some(dir) = env::var_os("SHERPA_ONNX_LIB_DIR") {
        println!(
            "cargo:warning=using the sherpa-onnx libraries in SHERPA_ONNX_LIB_DIR ({}) without verifying them",
            Path::new(&dir).display()
        );
        return;
    }

    let os = env::var("CARGO_CFG_TARGET_OS").expect("set by Cargo");
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("set by Cargo");
    let Some(name) = archive_name(&os, &arch) else {
        panic!(
            "no pinned sherpa-onnx archive for {os}/{arch}: add its SHA-256 to crates/core/build.rs"
        );
    };
    let expected = PINNED_ARCHIVES
        .iter()
        .find(|(pinned, _)| *pinned == name)
        .map(|(_, sha256)| *sha256)
        .unwrap_or_else(|| panic!("{name} is not pinned in crates/core/build.rs"));

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("set by Cargo"));
    let cache = target_dir(&out_dir).join("sherpa-onnx-prebuilt");
    let archive = cache.join(&name);
    println!("cargo:rerun-if-changed={}", archive.display());

    let actual = match sha256(&archive) {
        Ok(actual) => actual,
        Err(err) => panic!(
            "cannot verify the sherpa-onnx libraries: {} ({err}). Delete {} and build again.",
            archive.display(),
            cache.display()
        ),
    };
    if actual != expected {
        panic!(
            "{} does not match its pinned SHA-256 (expected {expected}, found {actual}). \
             Delete {} and build again; if it still fails, the upstream archive changed.",
            archive.display(),
            cache.display()
        );
    }
}

/// The archive `sherpa-onnx-sys` downloads for a target, with static linking (its
/// default, and what the core uses).
fn archive_name(os: &str, arch: &str) -> Option<String> {
    let platform = match (os, arch) {
        ("macos", "aarch64") => "osx-arm64-static",
        ("macos", "x86_64") => "osx-x64-static",
        ("windows", "x86_64") => "win-x64-static-MT-Release",
        ("windows", "aarch64") => "win-arm64-static-MT-Release",
        ("linux", "x86_64") => "linux-x64-static",
        ("linux", "aarch64") => "linux-aarch64-static",
        _ => return None,
    };
    Some(format!(
        "sherpa-onnx-v{SHERPA_ONNX_VERSION}-{platform}-lib.tar.bz2"
    ))
}

/// The target folder, found the same way as the `sherpa-onnx-sys` build script does.
fn target_dir(out_dir: &Path) -> PathBuf {
    if let Some(dir) = env::var_os("CARGO_TARGET_DIR") {
        return PathBuf::from(dir);
    }
    out_dir
        .ancestors()
        .find(|path| path.file_name() == Some(OsStr::new("target")))
        .map(Path::to_path_buf)
        .unwrap_or_else(|| out_dir.to_path_buf())
}

fn sha256(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
