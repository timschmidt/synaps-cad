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
    /// Frame times for the last N frames (milliseconds)
    pub frame_times: Vec<f32>,
    /// CPU usage percentage (estimated)
    pub cpu_usage: f32,
    /// Memory usage in MB
    pub memory_usage: f32,
    /// Whether to show performance overlay
    pub show_overlay: bool,
    /// Frame count for averaging
    frame_count: usize,
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self {
            frame_times: Vec::with_capacity(60), // Store 1 second at 60 FPS
            cpu_usage: 0.0,
            memory_usage: 0.0,
            show_overlay: false,
            frame_count: 0,
        }
    }
}

impl PerformanceMonitor {
    pub fn record_frame_time(&mut self, frame_time: f32) {
        self.frame_times.push(frame_time);
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }
        self.frame_count += 1;

        // Update system stats every 60 frames (~1 second)
        if self.frame_count >= 60 {
            self.update_system_stats();
            self.frame_count = 0;
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn update_system_stats(&mut self) {
        // Note: Cross-platform CPU usage monitoring would require additional dependencies
        // For now, we'll estimate based on frame times as a proxy
        let avg_frame_time = if self.frame_times.is_empty() {
            16.67 // 60 FPS target
        } else {
            self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32
        };

        // Rough estimation: high frame times suggest high CPU usage
        // This is a simplified metric for debugging purposes
        self.cpu_usage = (avg_frame_time / 16.67 * 100.0).min(100.0);

        // Memory usage would also need platform-specific implementation
        // For debugging, we'll track allocation pressure indirectly
        self.memory_usage = 0.0; // Placeholder
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
}

/// Async file-picker result receiver (avoids blocking the main thread).
#[derive(Resource, Default)]
pub struct FilePickerState {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<Vec<std::path::PathBuf>>>>,
}

/// State for image hover preview in chat.
#[derive(Resource, Default)]
pub struct ImagePreviewState {
    /// (`base64_data` key, decoded texture handle)
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
    pub(crate) timestamp: std::time::Instant,
}

impl AppErrors {
    #[allow(dead_code)]
    pub fn push(&mut self, message: impl Into<String>) {
        self.errors.push(AppError {
            message: message.into(),
            timestamp: std::time::Instant::now(),
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
