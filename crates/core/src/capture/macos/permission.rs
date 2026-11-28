//! Microphone permission through AVFoundation.

use std::sync::mpsc;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

use super::super::PermissionState;

/// Microphone access, without asking.
pub(super) fn microphone() -> PermissionState {
    // SAFETY: `AVMediaTypeAudio` is a framework constant; the method takes it as is.
    let status = unsafe {
        let Some(audio) = AVMediaTypeAudio else {
            return PermissionState::Unknown;
        };
        AVCaptureDevice::authorizationStatusForMediaType(audio)
    };
    match status {
        AVAuthorizationStatus::Authorized => PermissionState::Granted,
        AVAuthorizationStatus::Denied => PermissionState::Denied,
        AVAuthorizationStatus::Restricted => PermissionState::Restricted,
        AVAuthorizationStatus::NotDetermined => PermissionState::NotDetermined,
        _ => PermissionState::Unknown,
    }
}

/// Shows the system prompt if the user has not been asked yet, and waits for the answer.
pub(super) fn request_microphone() -> PermissionState {
    let current = microphone();
    if current != PermissionState::NotDetermined {
        return current;
    }
    let (tx, rx) = mpsc::channel();
    let handler = RcBlock::new(move |granted: Bool| {
        let _ = tx.send(granted.as_bool());
    });
    // SAFETY: the completion handler is a `Send` closure called once on an arbitrary
    // queue; it only sends on a channel.
    unsafe {
        let Some(audio) = AVMediaTypeAudio else {
            return PermissionState::Unknown;
        };
        AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &handler);
    }
    match rx.recv() {
        Ok(true) => PermissionState::Granted,
        Ok(false) => PermissionState::Denied,
        Err(_) => microphone(),
    }
}
