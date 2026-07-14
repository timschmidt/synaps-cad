use crate::export::ExportFormat;
use bevy::prelude::*;
use bevy_egui::egui;
use std::sync::{Mutex, mpsc};

#[derive(Resource, Default)]
pub struct OccupiedScreenSpace {
    pub left: f32,
}

/// Performance monitoring data for debug display.
#[derive(Resource)]
pub struct PerformanceMonitor {
    /// Frame times for the last 60 frames, in milliseconds.
    pub frame_times: Vec<f32>,
    /// Whether to show the performance overlay.
    pub show_overlay: bool,
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self {
            frame_times: Vec::with_capacity(60),
            show_overlay: false,
        }
    }
}

impl PerformanceMonitor {
    pub fn record_frame_time(&mut self, frame_time: f32) {
        self.frame_times.push(frame_time);
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn average_frame_time(&self) -> f32 {
        if self.frame_times.is_empty() {
            0.0
        } else {
            self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32
        }
    }

    pub fn current_fps(&self) -> f32 {
        let avg_frame_time = self.average_frame_time();
        if avg_frame_time > 0.0 {
            1000.0 / avg_frame_time
        } else {
            0.0
        }
    }

    /// Returns average frame time as a percentage of a 60 FPS frame budget.
    pub fn frame_budget_usage(&self) -> f32 {
        self.average_frame_time() / (1000.0 / 60.0) * 100.0
    }
}

/// Async file-picker result receiver (avoids blocking the main thread).
#[derive(Resource, Default)]
pub struct FilePickerState {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<Vec<PickedImage>>>>,
}

pub struct PickedImage {
    pub(crate) filename: String,
    pub(crate) bytes: Vec<u8>,
}

/// State for image hover preview in chat.
#[derive(Resource, Default)]
pub struct ImagePreviewState {
    /// Active attachment ID and its decoded texture.
    pub(crate) active: Option<(String, egui::TextureHandle)>,
}

/// Whether the AI settings dialog window is open.
#[derive(Resource, Default)]
pub struct SettingsDialogOpen(pub bool);

#[derive(Resource, Default)]
pub struct CheatsheetOpen(pub bool);

/// Non-fatal errors shown in the UI instead of panicking.
#[derive(Resource, Default)]
pub struct AppErrors {
    pub(crate) errors: Vec<AppError>,
}

pub struct AppError {
    pub(crate) message: String,
    pub(crate) timestamp: web_time::Instant,
}

impl AppErrors {
    #[allow(dead_code)]
    pub fn push(&mut self, message: impl Into<String>) {
        self.errors.push(AppError {
            message: message.into(),
            timestamp: web_time::Instant::now(),
        });
    }
}

/// Async export save-dialog state.
#[derive(Resource, Default)]
pub struct ExportState {
    /// Receives the chosen save path from the async file dialog.
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<Option<std::path::PathBuf>>>>,
    /// The format selected for the pending export.
    pub(crate) pending_format: Option<ExportFormat>,
}

/// Splash screen state — shown on app startup, dismissed by click or timeout.
#[derive(Resource)]
pub struct SplashScreen {
    pub(crate) texture: Option<egui::TextureHandle>,
    pub(crate) timer: f32,
    pub(crate) dismissing: bool,
}

impl Default for SplashScreen {
    fn default() -> Self {
        Self {
            texture: None,
            timer: 1.5,
            dismissing: false,
        }
    }
}
