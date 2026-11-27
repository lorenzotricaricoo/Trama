//! Trama core library.
//!
//! Capture, transcription, the curator, the index and storage live here, with no
//! dependency on the user interface (principle A1). The desktop app, and later the CLI
//! and the MCP server, are clients of this library.

pub mod i18n;

/// Version of the core library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
