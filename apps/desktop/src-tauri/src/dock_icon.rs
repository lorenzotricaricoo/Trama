//! The Dock icon follows the accent while Trama is open. Finder, Launchpad and the Dock
//! of a closed app show the bundle icon, which stays Lime: changing it on disk would break
//! the app's signature.
//!
//! Debug builds carry a "DEV" badge on every icon, so they are not mistaken for the
//! release: the bundle icon through `tauri.debug.conf.json`, the Dock icon here. A debug
//! build sets the badged image for Lime too, since the bundle icon of `tauri dev` is not
//! the badged one.

use tauri::{AppHandle, Runtime};

/// An accent and its Dock icon, rendered by `scripts/app-icon.py`.
#[cfg(target_os = "macos")]
type Icons = &'static [(&'static str, &'static [u8])];

/// The Dock icon of every accent but Lime, which is the bundle icon itself.
#[cfg(all(target_os = "macos", any(not(debug_assertions), test)))]
const RELEASE_ICONS: Icons = &[
    ("blue", include_bytes!("../icons/dock/blue.png")),
    ("purple", include_bytes!("../icons/dock/purple.png")),
    ("pink", include_bytes!("../icons/dock/pink.png")),
    ("red", include_bytes!("../icons/dock/red.png")),
    ("orange", include_bytes!("../icons/dock/orange.png")),
    ("yellow", include_bytes!("../icons/dock/yellow.png")),
    ("green", include_bytes!("../icons/dock/green.png")),
    ("graphite", include_bytes!("../icons/dock/graphite.png")),
];

/// The badged Dock icon of every accent, Lime included.
#[cfg(all(target_os = "macos", debug_assertions))]
const DEBUG_ICONS: Icons = &[
    ("lime", include_bytes!("../icons/debug/dock/lime.png")),
    ("blue", include_bytes!("../icons/debug/dock/blue.png")),
    ("purple", include_bytes!("../icons/debug/dock/purple.png")),
    ("pink", include_bytes!("../icons/debug/dock/pink.png")),
    ("red", include_bytes!("../icons/debug/dock/red.png")),
    ("orange", include_bytes!("../icons/debug/dock/orange.png")),
    ("yellow", include_bytes!("../icons/debug/dock/yellow.png")),
    ("green", include_bytes!("../icons/debug/dock/green.png")),
    ("graphite", include_bytes!("../icons/debug/dock/graphite.png")),
];

#[cfg(all(target_os = "macos", not(debug_assertions)))]
const ICONS: Icons = RELEASE_ICONS;
#[cfg(all(target_os = "macos", debug_assertions))]
const ICONS: Icons = DEBUG_ICONS;

/// The image for an accent in `icons`, or `None` for the bundle icon.
#[cfg(target_os = "macos")]
fn find(icons: Icons, accent: &str) -> Option<&'static [u8]> {
    icons
        .iter()
        .find(|(name, _)| *name == accent)
        .map(|(_, png)| *png)
}

/// Shows the Dock icon of `accent`.
pub fn show<R: Runtime>(app: &AppHandle<R>, accent: &str) {
    #[cfg(target_os = "macos")]
    {
        let png = find(ICONS, accent);
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

    fn assert_readable(icons: Icons) {
        for (name, png) in icons {
            let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png));
            assert!(image.is_some(), "AppKit cannot read the {name} icon");
        }
    }

    #[test]
    fn every_accent_but_lime_has_its_dock_icon() {
        for accent in trama_core::config::ACCENTS {
            assert_eq!(find(RELEASE_ICONS, accent).is_some(), *accent != "lime", "{accent}");
        }
        assert_eq!(RELEASE_ICONS.len(), trama_core::config::ACCENTS.len() - 1);
        assert_readable(RELEASE_ICONS);
    }

    #[test]
    fn debug_builds_badge_every_accent() {
        for accent in trama_core::config::ACCENTS {
            assert!(find(DEBUG_ICONS, accent).is_some(), "{accent}");
        }
        assert_eq!(DEBUG_ICONS.len(), trama_core::config::ACCENTS.len());
        assert_readable(DEBUG_ICONS);
        // Tests build with debug assertions, so the app shows the badged icons.
        assert_eq!(ICONS.as_ptr(), DEBUG_ICONS.as_ptr());
    }
}
