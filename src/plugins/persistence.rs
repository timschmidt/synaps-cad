use bevy::prelude::*;
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use super::ai_chat::{AiConfig, ChatImage, ChatMessage, ChatState, normalize_custom_url};
use super::code_editor::ScadCode;
use super::scene::LabelVisibility;

pub struct PersistencePlugin;

const APP_DIR_NAME: &str = "synaps-cad";
const SESSION_FILE: &str = "session.json";
const MAX_PERSISTED_MESSAGES: usize = 50;

#[derive(Serialize, Deserialize)]
struct SerializableImage {
    filename: String,
    mime_type: String,
    base64_data: String,
}

/// Serializable chat message including attached images.
#[derive(Serialize, Deserialize)]
struct SerializableChatMessage {
    role: String,
    content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thinking: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    images: Vec<SerializableImage>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    auto_generated: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_error: bool,
}

#[derive(Serialize, Deserialize)]
struct PersistentData {
    chat_messages: Vec<SerializableChatMessage>,
    adapter_name: String,
    model_name: String,
    temperature: f64,
    editor_code: String,
    #[serde(default = "default_verification_rounds")]
    max_verification_rounds: u32,
    /// Per-provider API keys (`adapter_name` → key).
    #[serde(default)]
    api_keys: std::collections::HashMap<String, String>,
    /// Per-provider last-used model (`adapter_name` → `model_name`).
    #[serde(default)]
    model_per_provider: std::collections::HashMap<String, String>,
    /// Per-provider custom endpoint URLs.
    #[serde(default)]
    custom_urls: std::collections::HashMap<String, String>,
    /// Legacy: custom Ollama host. Migrated into `custom_urls` on load.
    #[serde(default = "default_ollama_host")]
    ollama_host: String,
    #[serde(default)]
    ui: UiSettings,
    /// Legacy: old multi-part data. Merged into `editor_code` on load.
    #[serde(default)]
    parts: std::collections::HashMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct UiSettings {
    #[serde(default = "default_true")]
    show_labels: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self { show_labels: true }
    }
}

const fn default_true() -> bool {
    true
}

const fn default_verification_rounds() -> u32 {
    2
}

fn default_ollama_host() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".into())
    }

    #[cfg(target_arch = "wasm32")]
    {
        option_env!("OLLAMA_HOST")
            .unwrap_or("http://localhost:11434")
            .into()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_DIR_NAME))
}

#[cfg(not(target_arch = "wasm32"))]
fn session_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(SESSION_FILE))
}

impl Plugin for PersistencePlugin {
    fn build(&self, app: &mut App) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = app;
        }

        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Startup, load_session_system).add_systems(
            Update,
            (auto_save_system, save_on_exit_system, save_on_change_system),
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load_session_system(
    mut ai_config: ResMut<AiConfig>,
    mut chat_state: ResMut<ChatState>,
    mut scad_code: ResMut<ScadCode>,
    mut label_vis: ResMut<LabelVisibility>,
) {
    let Some(path) = session_path() else {
        return;
    };
    let data = match std::fs::read_to_string(&path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            eprintln!(
                "[SynapsCAD] Failed to read session file {}: {error}",
                path.display()
            );
            return;
        }
    };
    let Ok(saved) = serde_json::from_str::<PersistentData>(&data) else {
        eprintln!("[SynapsCAD] Failed to parse session file, starting fresh");
        return;
    };

    ai_config.adapter_name = saved.adapter_name;
    ai_config.model_name = saved.model_name;
    ai_config.temperature = saved.temperature;
    ai_config.max_verification_rounds = saved.max_verification_rounds;

    ai_config.api_keys = saved
        .api_keys
        .into_iter()
        .map(|(k, v)| (k, v.trim().to_string()))
        .collect();

    ai_config.model_per_provider = saved.model_per_provider;

    // Migrate the legacy `ollama_host` field into `custom_urls`.
    let mut custom_urls = saved.custom_urls;
    if !saved.ollama_host.is_empty() && !custom_urls.contains_key("Ollama") {
        let mut host = saved.ollama_host;
        if !host.ends_with('/') {
            host.push('/');
        }
        custom_urls.insert("Ollama".into(), host);
    }
    // Normalize whitespace, trailing slashes, and default paths on host-only URLs.
    for (adapter, url) in &mut custom_urls {
        *url = normalize_custom_url(adapter, url);
    }
    ai_config.custom_urls = custom_urls;

    label_vis.visible = saved.ui.show_labels;

    chat_state.messages = saved
        .chat_messages
        .into_iter()
        .map(|m| ChatMessage {
            role: m.role,
            content: m.content,
            thinking: m.thinking,
            images: m
                .images
                .into_iter()
                .map(|i| ChatImage {
                    filename: i.filename,
                    mime_type: i.mime_type,
                    base64_data: i.base64_data,
                })
                .collect(),
            auto_generated: m.auto_generated,
            is_error: m.is_error,
        })
        .collect();

    // Rebuild input history from restored, user-authored messages.
    chat_state.input_history = chat_state
        .messages
        .iter()
        .filter(|m| m.role == "user" && !m.auto_generated)
        .map(|m| (m.content.clone(), m.images.clone()))
        .collect();

    // Restored messages remain visible but are not sent to the AI.
    chat_state.session_start = chat_state.messages.len();

    // Merge legacy multipart documents into the current single editor buffer.
    if !saved.parts.is_empty() {
        let mut merged = String::new();
        for (name, code) in &saved.parts {
            if !merged.is_empty() {
                merged.push_str("\n\n");
            }
            merged.push_str("// --- ");
            merged.push_str(name);
            merged.push_str(" ---\n");
            merged.push_str(code);
        }
        scad_code.text = merged;
    } else if !saved.editor_code.is_empty() {
        scad_code.text = saved.editor_code;
    }
    scad_code.dirty = true;

    eprintln!("[SynapsCAD] Session restored from {}", path.display());
}

/// Timer resource to throttle auto-save.
#[derive(Resource)]
#[cfg(not(target_arch = "wasm32"))]
struct AutoSaveTimer(Timer);

#[cfg(not(target_arch = "wasm32"))]
impl Default for AutoSaveTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(5.0, TimerMode::Repeating))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn auto_save_system(
    time: Res<Time>,
    mut timer: Local<AutoSaveTimer>,
    ai_config: Res<AiConfig>,
    chat_state: Res<ChatState>,
    scad_code: Res<ScadCode>,
    label_vis: Res<LabelVisibility>,
) {
    timer.0.tick(time.delta());
    if !timer.0.just_finished() {
        return;
    }

    save_session(&ai_config, &chat_state, &scad_code, &label_vis);
}

#[cfg(not(target_arch = "wasm32"))]
fn save_on_exit_system(
    exit_events: EventReader<AppExit>,
    ai_config: Res<AiConfig>,
    chat_state: Res<ChatState>,
    scad_code: Res<ScadCode>,
    label_vis: Res<LabelVisibility>,
) {
    if !exit_events.is_empty() {
        save_session(&ai_config, &chat_state, &scad_code, &label_vis);
    }
}

/// Save soon after chat or UI settings change (debounced to avoid excessive I/O during streaming).
#[cfg(not(target_arch = "wasm32"))]
fn save_on_change_system(
    time: Res<Time>,
    mut debounce: Local<Option<f32>>,
    label_vis: Res<LabelVisibility>,
    chat_state: Res<ChatState>,
    ai_config: Res<AiConfig>,
    scad_code: Res<ScadCode>,
) {
    let changed = (label_vis.is_changed() && !label_vis.is_added())
        || (chat_state.is_changed() && !chat_state.is_added());
    if changed {
        *debounce = Some(2.0); // save 2 seconds after last change
    }
    if let Some(ref mut remaining) = *debounce {
        *remaining -= time.delta_secs();
        if *remaining <= 0.0 {
            *debounce = None;
            save_session(&ai_config, &chat_state, &scad_code, &label_vis);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn save_session(
    ai_config: &AiConfig,
    chat_state: &ChatState,
    scad_code: &ScadCode,
    label_vis: &LabelVisibility,
) {
    let Some(dir) = config_dir() else {
        return;
    };
    let Some(path) = session_path() else {
        return;
    };

    if std::fs::create_dir_all(&dir).is_err() {
        eprintln!("[SynapsCAD] Failed to create config directory");
        return;
    }

    let data = PersistentData {
        chat_messages: chat_state
            .messages
            .iter()
            .rev()
            .take(MAX_PERSISTED_MESSAGES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|m| SerializableChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                thinking: m.thinking.clone(),
                images: m
                    .images
                    .iter()
                    .map(|i| SerializableImage {
                        filename: i.filename.clone(),
                        mime_type: i.mime_type.clone(),
                        base64_data: i.base64_data.clone(),
                    })
                    .collect(),
                auto_generated: m.auto_generated,
                is_error: m.is_error,
            })
            .collect(),
        adapter_name: ai_config.adapter_name.clone(),
        model_name: ai_config.model_name.clone(),
        temperature: ai_config.temperature,
        editor_code: scad_code.text.clone(),
        max_verification_rounds: ai_config.max_verification_rounds,
        api_keys: ai_config.api_keys.clone(),
        model_per_provider: ai_config.model_per_provider.clone(),
        custom_urls: ai_config.custom_urls.clone(),
        ollama_host: ai_config
            .custom_urls
            .get("Ollama")
            .cloned()
            .unwrap_or_default(),
        ui: UiSettings {
            show_labels: label_vis.visible,
        },
        parts: std::collections::HashMap::new(),
    };

    match serde_json::to_string_pretty(&data) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                eprintln!("[SynapsCAD] Failed to save session: {e}");
            }
        }
        Err(e) => eprintln!("[SynapsCAD] Failed to serialize session: {e}"),
    }
}
