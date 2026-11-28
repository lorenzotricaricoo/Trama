//! The Dock icon follows the accent while Trama is open. Finder, Launchpad and the Dock
//! of a closed app show the bundle icon, which stays Lime: changing it on disk would break
//! the app's signature.

use tauri::{AppHandle, Runtime};

/// The Dock icon of every accent but Lime, rendered by `scripts/app-icon.py`. Lime is the
/// bundle icon itself.
#[cfg(target_os = "macos")]
const ICONS: &[(&str, &[u8])] = &[
    ("blue", include_bytes!("../icons/dock/blue.png")),
    ("purple", include_bytes!("../icons/dock/purple.png")),
    ("pink", include_bytes!("../icons/dock/pink.png")),
    ("red", include_bytes!("../icons/dock/red.png")),
    ("orange", include_bytes!("../icons/dock/orange.png")),
    ("yellow", include_bytes!("../icons/dock/yellow.png")),
    ("green", include_bytes!("../icons/dock/green.png")),
    ("graphite", include_bytes!("../icons/dock/graphite.png")),
];

/// The image for an accent, or `None` for the bundle icon.
#[cfg(target_os = "macos")]
fn icon(accent: &str) -> Option<&'static [u8]> {
    ICONS
        .iter()
        .find(|(name, _)| *name == accent)
        .map(|(_, png)| *png)
}

/// Shows the Dock icon of `accent`.
pub fn show<R: Runtime>(app: &AppHandle<R>, accent: &str) {
    #[cfg(target_os = "macos")]
    {
        let png = icon(accent);
        if let Err(err) = app.run_on_main_thread(move || set(png)) {
            eprintln!("trama: cannot change the Dock icon: {err}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, accent);
}

#[cfg(target_os = "macos")]
fn set(png: Option<&'static [u8]>) {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("trama: the Dock icon can only change on the main thread");
        return;
    };
    let image =
        png.and_then(|bytes| NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes)));
    if png.is_some() && image.is_none() {
        eprintln!("trama: cannot read the Dock icon");
        return;
    }
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: called on the main thread (checked above) with a valid image that AppKit
    // retains, or with none, which brings back the bundle icon.
    #[allow(unsafe_code)]
    unsafe {
        app.setApplicationIconImage(image.as_deref());
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use objc2::AllocAnyThread;
    use objc2_app_kit::NSImage;
    use objc2_foundation::NSData;

    use super::*;

    #[test]
    fn every_accent_but_lime_has_its_dock_icon() {
        for accent in trama_core::config::ACCENTS {
            assert_eq!(icon(accent).is_some(), *accent != "lime", "{accent}");
        }
        assert_eq!(ICONS.len(), trama_core::config::ACCENTS.len() - 1);
        for (name, png) in ICONS {
            let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png));
            assert!(image.is_some(), "AppKit cannot read the {name} icon");
        }
    }
}
