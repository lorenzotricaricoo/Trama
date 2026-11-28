//! Bridge between the core's model downloads and the frontend: Tauri commands that wrap
//! [`ModelManager`], and the `model` event sent after every change.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use trama_core::models::{ModelEventSink, ModelManager, ModelStatus, ModelStore, PANICKED};

/// Name of the event sent to the frontend for every change of the transcription model.
pub const MODEL_EVENT: &str = "model";

/// Overrides the models folder, for example to share one download between development
/// builds: `TRAMA_MODELS=/tmp/trama-models`.
const MODELS_OVERRIDE_ENV: &str = "TRAMA_MODELS";

/// Name of the models folder inside the app data folder, next to the library.
const MODELS_DIR: &str = "Models";

/// The transcription model, owned by the app for its whole life.
pub struct Models {
    pub transcription: ModelManager,
}

/// A model as the frontend sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPayload {
    /// `missing`, `downloading`, `verifying`, `ready` or `corrupt`.
    pub state: &'static str,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    /// Error code for the interface to translate.
    pub error_code: Option<&'static str>,
    /// Progress of the running download or check, from 0 to 1.
    pub progress: Option<f32>,
}

impl From<&ModelStatus> for ModelPayload {
    fn from(status: &ModelStatus) -> Self {
        Self {
            state: status.state.as_str(),
            downloaded_bytes: status.downloaded_bytes,
            total_bytes: status.total_bytes,
            error_code: status.error_code,
            progress: status.progress,
        }
    }
}

/// The models folder: the override if set, otherwise inside the app data folder
/// (`~/Library/Application Support/<identifier>/Models` on macOS).
pub fn models_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    match std::env::var_os(MODELS_OVERRIDE_ENV) {
        Some(dir) if !dir.is_empty() => Ok(PathBuf::from(dir)),
        _ => Ok(app.path().app_data_dir()?.join(MODELS_DIR)),
    }
}

/// Creates the manager of the transcription model; nothing is downloaded until the user
/// asks.
pub fn start<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Models> {
    let store = ModelStore::new(models_dir(app)?, trama_core::models::transcription_model());
    let app = app.clone();
    let events: Arc<dyn ModelEventSink> = Arc::new(move |status: ModelStatus| {
        if let Err(err) = app.emit(MODEL_EVENT, ModelPayload::from(&status)) {
            eprintln!("trama: cannot send a model event to the frontend: {err}");
        }
    });
    Ok(Models {
        transcription: ModelManager::new(store, events),
    })
}

/// Runs a manager call on a blocking thread: some calls wait (a removal waits for the
/// download to stop) and none must hold up the main thread or the async runtime.
/// Errors reach the frontend as codes to translate; the detail goes to the log (A4, A5).
async fn on_manager(
    app: AppHandle,
    call: impl FnOnce(&ModelManager) -> trama_core::models::Result<ModelStatus> + Send + 'static,
) -> Result<ModelPayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let models = app.state::<Models>();
        call(&models.transcription)
            .map(|status| ModelPayload::from(&status))
            .map_err(|err| {
                eprintln!("trama: model operation failed: {err}");
                err.code().to_owned()
            })
    })
    .await
    .map_err(|err| {
        eprintln!("trama: model operation did not finish: {err}");
        PANICKED.to_owned()
    })?
}

/// State of the transcription model.
#[tauri::command]
pub async fn model_status(app: AppHandle) -> Result<ModelPayload, String> {
    on_manager(app, ModelManager::status).await
}

/// Starts or resumes the download; progress arrives as `model` events.
#[tauri::command]
pub async fn download_model(app: AppHandle) -> Result<ModelPayload, String> {
    on_manager(app, ModelManager::download).await
}

/// Stops the download or the check, keeping what was downloaded.
#[tauri::command]
pub async fn cancel_model_download(app: AppHandle) -> Result<ModelPayload, String> {
    on_manager(app, ModelManager::cancel).await
}

/// Starts hashing the model files again; progress arrives as `model` events.
#[tauri::command]
pub async fn verify_model(app: AppHandle) -> Result<ModelPayload, String> {
    on_manager(app, ModelManager::verify).await
}

/// Deletes the model from disk, stopping a download first.
#[tauri::command]
pub async fn remove_model(app: AppHandle) -> Result<ModelPayload, String> {
    on_manager(app, ModelManager::remove).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use trama_core::models::ModelState;

    #[test]
    fn payload_uses_the_frontend_names() {
        let status = ModelStatus {
            state: ModelState::Downloading,
            downloaded_bytes: 10,
            total_bytes: 40,
            error_code: None,
            progress: Some(0.25),
        };
        let json = serde_json::to_value(ModelPayload::from(&status)).unwrap();
        assert_eq!(json["state"], "downloading");
        assert_eq!(json["downloadedBytes"], 10);
        assert_eq!(json["totalBytes"], 40);
        assert!(json["errorCode"].is_null());
        assert_eq!(json["progress"], 0.25);
    }
}
