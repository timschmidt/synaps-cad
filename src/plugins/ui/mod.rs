use bevy::prelude::*;
use bevy_egui::{EguiInputSet, EguiPreUpdateSet};

pub mod chat;
pub mod editor;
pub mod layout;
pub mod resources;
pub mod systems;
pub mod theme;
pub mod utils;
pub mod viewport;

pub use resources::{AppErrors, OccupiedScreenSpace};
pub use systems::*;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OccupiedScreenSpace>()
            .init_resource::<resources::FilePickerState>()
            .init_resource::<resources::ImagePreviewState>()
            .init_resource::<AppErrors>()
            .init_resource::<resources::PerformanceMonitor>()
            .init_resource::<resources::SettingsDialogOpen>()
            .init_resource::<resources::CheatsheetOpen>()
            .init_resource::<resources::ExportState>()
            .init_resource::<resources::SplashScreen>()
            .add_systems(Startup, theme::setup_egui_theme)
            .add_systems(
                PreUpdate,
                systems::fix_clipboard_paste_events
                    .after(EguiInputSet::WriteEguiEvents)
                    .before(EguiPreUpdateSet::BeginPass),
            )
            .add_systems(
                Update,
                (
                    set_window_icon,
                    splash_screen_system,
                    ui_layout_system,
                    viewport_toolbar_system,
                    cheatsheet_system,
                    draw_part_labels,
                    draw_axis_indicator,
                    performance_monitor_system,
                ),
            );

        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(
            Update,
            (
                poll_file_picker_system,
                poll_export_system,
                file_drop_system,
            ),
        );
    }
}
