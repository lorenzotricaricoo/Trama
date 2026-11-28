//! Build script of the core: the Swift runtime path for the optional ScreenCaptureKit
//! fallback, and the verification of the transcription runtime's native libraries.
//!
//! **Native libraries (ADR 0007).**
//! The `sherpa-onnx-sys` build script downloads a prebuilt archive of sherpa-onnx and
//! ONNX Runtime from the sherpa-onnx GitHub release, extracts it into a cache in the
//! target folder and links the static libraries it finds there, without checking
//! anything. Once extracted, it never reads the archive again. This script runs after
//! it (the core depends on `sherpa-onnx-sys` directly, and Cargo runs the build script
//! of a `links` dependency first) and before anything is linked: it compares the
//! SHA-256 of the archive and of every extracted library that gets linked with the
//! values pinned below, and stops the build on any mismatch. The guarantee is that
//! nothing unverified gets linked, not that the archive is checked before extraction.
//!
//! The libraries are pinned for macOS on Apple Silicon, the target Trama ships and
//! builds in CI (ADR 0005). On other targets only the archive is checked, with a
//! warning, until their libraries are pinned too.
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

/// SHA-256 of the libraries that `sherpa-onnx-sys` links statically (its
/// `SHERPA_ONNX_STATIC_LIBS`), as extracted from a verified archive into
/// `<target>/sherpa-onnx-prebuilt/<archive stem>/lib`. Keyed by archive stem.
const PINNED_LIBRARIES: &[(&str, &[(&str, &str)])] = &[(
    "sherpa-onnx-v2.0.3-osx-arm64-static-lib",
    &[
        (
            "libsherpa-onnx-c-api.a",
            "4e91fc0cabca0b106b9b20977e56c02abbedbb85da0e20b7be6f329285a48a16",
        ),
        (
            "libsherpa-onnx-core.a",
            "e3db070685a867af736137da0a6dde4789616f0a350db8b90a373a9b3c847c54",
        ),
        (
            "libkaldi-hmm-gmm-core.a",
            "a07ce1bd2712101f41ca434408873878b838ff2e0de99a556f5fac4c58078fd3",
        ),
        (
            "libkaldi-native-io-core.a",
            "31fc39925f09cb8c0a88deaf2b9520ebdff7b811d1248f8aed8d082176e1f0b9",
        ),
        (
            "libkaldi-decoder-core.a",
            "ab058144df6041c11edd47a696a3f19749c4e5f28c82bb0d60392f328111e707",
        ),
        (
            "libsherpa-onnx-kaldifst-core.a",
            "b412bea869588b199ffb8477cb504712b924b7cc5b87b4534f7a07af36327d84",
        ),
        (
            "libsherpa-onnx-fstfar.a",
            "38607b1f5c3d6d9c6b6c8e1cea6517f15a4f42f6ae58a573080dc417dbb0356e",
        ),
        (
            "libsherpa-onnx-fst.a",
            "52036ff3df7fa935724fe7969caa17ca1312c7e0bcb549405e421f4d97898f6c",
        ),
        (
            "libkaldi-native-fbank-core.a",
            "5bce74278e4708f9372c33036cc1643d4cb61e07a536f82ddd907aceda3f608e",
        ),
        (
            "libkissfft-float.a",
            "41e924e70098b1c7c63c187f15ae8dba2bd9718d7d88997ad9b3de14f0697f41",
        ),
        (
            "libonnxruntime.a",
            "589e781d544d85267be5d7e74eae68fddd40ba41bf27e9961f867378170f8802",
        ),
        (
            "libssentencepiece_core.a",
            "433245b5a9df5fe0c3b255772676bc739e5013021335043debd528813187fbcc",
        ),
        (
            "libutf8proc.a",
            "452b2bd7e1a9db1f93019a8c77bf9282841842e96135b129b21727792856eba3",
        ),
    ],
)];

fn main() {
    // The optional `screencapturekit` crate links a Swift bridge: this crate's tests and
    // examples need the system Swift runtime on their rpath. An app enabling the feature
    // needs the same link argument.
    if env::var_os("CARGO_FEATURE_SCREENCAPTUREKIT").is_some() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    verify_sherpa_onnx();
}

/// Checks the sherpa-onnx archive and the libraries extracted from it against their
/// pinned SHA-256; panics on a mismatch.
fn verify_sherpa_onnx() {
    println!("cargo:rerun-if-changed=build.rs");
    for var in [
        "DOCS_RS",
        "SHERPA_ONNX_LIB_DIR",
        "SHERPA_ONNX_ARCHIVE_DIR",
        "CARGO_TARGET_DIR",
        "CI",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    // docs.rs builds without native libraries; so does sherpa-onnx-sys.
    if env::var_os("DOCS_RS").is_some() {
        return;
    }
    if let Some(dir) = env::var_os("SHERPA_ONNX_LIB_DIR") {
        let dir = Path::new(&dir).display();
        // A local escape hatch, never a way around the check in CI.
        if env::var_os("CI").is_some() {
            panic!(
                "SHERPA_ONNX_LIB_DIR ({dir}) would link unverified sherpa-onnx libraries; \
                 it is refused when CI is set"
            );
        }
        println!(
            "cargo:warning=using the sherpa-onnx libraries in SHERPA_ONNX_LIB_DIR ({dir}) without verifying them"
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

    check_file(&archive, expected, &cache);

    // What gets linked is the extracted copy, which the archive check alone does not
    // cover: `sherpa-onnx-sys` reuses an existing extraction without reading the archive.
    let stem = name.trim_end_matches(".tar.bz2");
    let lib_dir = cache.join(stem).join("lib");
    match PINNED_LIBRARIES.iter().find(|(pinned, _)| *pinned == stem) {
        Some((_, libraries)) => {
            for (file, expected) in *libraries {
                let path = lib_dir.join(file);
                println!("cargo:rerun-if-changed={}", path.display());
                check_file(&path, expected, &cache);
            }
        }
        None => println!(
            "cargo:warning=only the sherpa-onnx archive is verified on {os}/{arch}; \
             the extracted libraries in {} are linked without their own pinned SHA-256",
            lib_dir.display()
        ),
    }
}

/// Panics unless `path` exists and has the SHA-256 `expected`.
fn check_file(path: &Path, expected: &str, cache: &Path) {
    let actual = match sha256(path) {
        Ok(actual) => actual,
        Err(err) => panic!(
            "cannot verify the sherpa-onnx libraries: {} ({err}). Delete {} and build again.",
            path.display(),
            cache.display()
        ),
    };
    if actual != expected {
        panic!(
            "{} does not match its pinned SHA-256 (expected {expected}, found {actual}). \
             Delete {} and build again; if it still fails, the upstream archive changed.",
            path.display(),
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
