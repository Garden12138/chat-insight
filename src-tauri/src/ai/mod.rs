pub mod client;
pub mod commands;
pub mod commitments;
pub mod decisions;
pub mod extraction;
#[cfg(test)]
mod extraction_eval;
pub mod features;
pub mod indexer;
pub mod retrieval;
pub mod settings;
pub mod store;

use client::OpenAiCompatClient;
use serde::Serialize;
use settings::{AiSettings, SettingsStore};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

/// Receives incremental output from long-running AI work.
pub trait Sink {
    /// Returns `false` once the user has cancelled.
    fn delta(&self, text: &str) -> bool;
    fn status(&self, text: &str);
    fn cancelled(&self) -> bool;
}

#[cfg(test)]
pub struct NullSink;

#[cfg(test)]
impl Sink for NullSink {
    fn delta(&self, _text: &str) -> bool {
        true
    }
    fn status(&self, _text: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStreamEvent {
    pub request_id: String,
    pub delta: String,
    pub status: Option<String>,
}

pub struct StreamSink {
    app: AppHandle,
    request_id: String,
    cancel: Arc<AtomicBool>,
    streams: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl StreamSink {
    fn emit(&self, delta: &str, status: Option<&str>) {
        let _ = self.app.emit(
            "ai-stream",
            AiStreamEvent {
                request_id: self.request_id.clone(),
                delta: delta.to_string(),
                status: status.map(str::to_string),
            },
        );
    }
}

impl Sink for StreamSink {
    fn delta(&self, text: &str) -> bool {
        if self.cancelled() {
            return false;
        }
        if !text.is_empty() {
            self.emit(text, None);
        }
        true
    }

    fn status(&self, text: &str) {
        self.emit("", Some(text));
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

impl Drop for StreamSink {
    fn drop(&mut self) {
        if let Ok(mut streams) = self.streams.lock() {
            streams.remove(&self.request_id);
        }
    }
}

pub struct AiState {
    pub settings: SettingsStore,
    pub index_running: Arc<AtomicBool>,
    pub index_cancel: Arc<AtomicBool>,
    pub scan_running: Arc<AtomicBool>,
    streams: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl AiState {
    pub fn new(settings: SettingsStore) -> Self {
        Self {
            settings,
            index_running: Arc::new(AtomicBool::new(false)),
            index_cancel: Arc::new(AtomicBool::new(false)),
            scan_running: Arc::new(AtomicBool::new(false)),
            streams: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn client(&self) -> (AiSettings, OpenAiCompatClient) {
        let settings = self.settings.load();
        let client = OpenAiCompatClient::new(&settings, self.settings.api_key());
        (settings, client)
    }

    pub fn stream(&self, app: &AppHandle, request_id: &str) -> StreamSink {
        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut streams) = self.streams.lock() {
            streams.insert(request_id.to_string(), Arc::clone(&cancel));
        }
        StreamSink {
            app: app.clone(),
            request_id: request_id.to_string(),
            cancel,
            streams: Arc::clone(&self.streams),
        }
    }

    pub fn cancel_stream(&self, request_id: &str) {
        if let Ok(streams) = self.streams.lock() {
            if let Some(flag) = streams.get(request_id) {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }
}
