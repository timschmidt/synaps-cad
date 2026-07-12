// Bevy systems require owned parameters (Query, Res, ResMut, etc.)
#![allow(clippy::needless_pass_by_value)]
// The browser build intentionally compiles some native-facing resources and
// shared AI data structures without enabling their native systems.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::window::PresentMode;
use bevy::winit::WinitSettings;
use bevy_egui::EguiPlugin;

mod app_config;
mod export;
mod plugins;

pub use synaps_cad::compiler;

fn main() {
    let primary_window = Window {
        title: format!("SynapsCAD v{}", env!("CARGO_PKG_VERSION")),
        resolution: (1600.0, 900.0).into(),
        // Use VSync to limit frame rate and reduce CPU usage when idle
        present_mode: PresentMode::Fifo,
        #[cfg(target_arch = "wasm32")]
        canvas: Some("#synaps-cad".into()),
        #[cfg(target_arch = "wasm32")]
        fit_canvas_to_parent: true,
        #[cfg(target_arch = "wasm32")]
        prevent_default_event_handling: false,
        ..default()
    };

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(primary_window),
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(WgpuSettings::default()),
                synchronous_pipeline_compilation: false,
            }),
    );

    #[cfg(not(target_arch = "wasm32"))]
    app.insert_resource(WinitSettings::desktop_app());

    #[cfg(target_arch = "wasm32")]
    app.insert_resource(WinitSettings::game());

    app.add_plugins(EguiPlugin)
        .add_plugins(plugins::SynapScadPlugins)
        .run();
}
