use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use std::sync::{Mutex, mpsc};

#[cfg(not(target_arch = "wasm32"))]
use crate::export::ALL_FORMATS;
use crate::plugins::ai_chat::{
    ADAPTER_NAMES, AiConfig, AvailableModels, ChatState, TokioRuntime, VERIFICATION_ROUND_CHOICES,
    ai_networking_available, default_placeholder_url, env_var_for_adapter, normalize_custom_url,
};
use crate::plugins::code_editor::{ScadCode, detect_views, set_active_view};
use crate::plugins::compilation::{CompilationState, LastCompiledParts, ModelViews};
use crate::plugins::ui::chat::render_chat_content;
use crate::plugins::ui::editor::render_code_editor;
use crate::plugins::ui::resources::{
    AppErrors, ExportState, ImagePreviewState, OccupiedScreenSpace, PickedImage, SettingsDialogOpen,
};
use crate::plugins::ui::utils::show_image_preview;
#[cfg(not(target_arch = "wasm32"))]
use crate::plugins::ui::utils::{clipboard_image_as_chat_image, copy_chat_image_to_clipboard};

#[cfg(not(target_arch = "wasm32"))]
fn env_var_value(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

#[cfg(target_arch = "wasm32")]
const fn env_var_value(_name: &str) -> Option<String> {
    None
}

#[allow(clippy::too_many_arguments)]
pub fn ui_layout_system(
    mut contexts: EguiContexts,
    mut scad_code: ResMut<ScadCode>,
    mut chat_state: ResMut<ChatState>,
    mut occupied: ResMut<OccupiedScreenSpace>,
    mut ai_config: ResMut<AiConfig>,
    mut available_models: ResMut<AvailableModels>,
    mut compilation_state: ResMut<CompilationState>,
    mut file_picker: ResMut<crate::plugins::ui::resources::FilePickerState>,
    runtime: Res<TokioRuntime>,
    mut preview_state: ResMut<ImagePreviewState>,
    mut app_errors: ResMut<AppErrors>,
    mut settings_open: ResMut<SettingsDialogOpen>,
    last_parts: Res<LastCompiledParts>,
    mut export_state: ResMut<ExportState>,
    model_views: Res<ModelViews>,
    mut cached_view_textures: Local<Vec<(String, egui::TextureHandle)>>,
) {
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    if model_views.is_changed() {
        use base64::Engine;
        let mut new_textures = Vec::new();
        for (label, base64_png) in &model_views.views {
            if base64_png.is_empty() {
                continue;
            }
            if let Ok(png_bytes) = base64::engine::general_purpose::STANDARD.decode(base64_png)
                && let Ok(dyn_img) = image::load_from_memory(&png_bytes)
            {
                let rgba = dyn_img.to_rgba8();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [rgba.width() as usize, rgba.height() as usize],
                    rgba.as_raw(),
                );
                new_textures.push((
                    label.clone(),
                    ctx.load_texture(
                        format!("view_cycle_{label}"),
                        color_image,
                        egui::TextureOptions::LINEAR,
                    ),
                ));
            }
        }
        *cached_view_textures = new_textures;
    }

    let panel_id = egui::Id::new("side_panel");
    // Capture the user-selected width before child content can affect layout.
    let panel_width_before =
        egui::containers::panel::PanelState::load(ctx, panel_id).map_or(400.0, |s| s.rect.width());
    let response = egui::SidePanel::left(panel_id)
        .default_width(400.0)
        .min_width(300.0)
        .max_width(600.0)
        .resizable(true)
        .show(ctx, |ui| {
            let max_w = ui.available_width();
            ui.set_min_width(max_w);
            ui.set_max_width(max_w);

            render_error_banner(ui, &mut app_errors);
            ui.add_space(4.0);
            ui.separator();

            render_ai_assistant_header(
                ui,
                &mut chat_state,
                &mut ai_config,
                &mut available_models,
                &mut settings_open,
            );
            ui.checkbox(&mut ai_config.send_images, "Send images")
                .on_hover_text(
                    "Send rendered model views to the AI. Explicitly attached images are always sent.",
                );
            ui.separator();

            let no_model_selected = !ai_networking_available()
                || (ai_config.model_name.is_empty() && ai_config.custom_url().is_empty());
            render_chat_input(
                ui,
                &mut chat_state,
                &mut file_picker,
                &runtime,
                no_model_selected,
            );
            render_pending_attachments(ui, &mut chat_state, &mut preview_state);

            let total_remaining = ui.available_height();
            let chat_height = (total_remaining * 0.45).max(50.0);
            render_chat_messages(
                ui,
                &mut chat_state,
                chat_height,
                &cached_view_textures,
                &mut preview_state,
            );

            ui.add_space(4.0);
            ui.separator();

            render_code_header(
                ui,
                &mut scad_code,
                &mut chat_state,
                &mut compilation_state,
                &last_parts,
                &mut export_state,
                &runtime,
            );
            render_view_selector(ui, &mut scad_code);
            ui.separator();

            render_code_editor(ui, &mut scad_code);
        });

    occupied.left = response.response.rect.width();

    // Prevent content from auto-expanding the panel. egui's SidePanel stores
    // inner_response.response.rect which can be wider than the allocated panel
    // if content overflows. Clamp back to the pre-render width unless the user
    // is actively resizing the panel via drag handle.
    let resize_id = panel_id.with("__resize");
    let is_resizing = ctx.is_being_dragged(resize_id);
    if let Some(state) = egui::containers::panel::PanelState::load(ctx, panel_id)
        && state.rect.width() > panel_width_before + 0.5
        && !is_resizing
    {
        let clamped_rect = egui::Rect::from_min_size(
            state.rect.min,
            egui::vec2(panel_width_before, state.rect.height()),
        );
        ctx.data_mut(|d| {
            d.insert_persisted(
                panel_id,
                egui::containers::panel::PanelState { rect: clamped_rect },
            );
        });
        occupied.left = panel_width_before;
    }
    if available_models.needs_configuration && available_models.last_adapter.is_empty() {
        settings_open.0 = true;
    }
    if settings_open.0 && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        settings_open.0 = false;
    }

    render_settings_dialog(
        ctx,
        &mut settings_open,
        &mut ai_config,
        &mut available_models,
    );
}

fn render_error_banner(ui: &mut egui::Ui, app_errors: &mut AppErrors) {
    app_errors
        .errors
        .retain(|e| e.timestamp.elapsed().as_secs() < 30);
    for err in &app_errors.errors {
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(80, 20, 20))
            .corner_radius(egui::CornerRadius::same(4))
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("⚠").color(egui::Color32::YELLOW));
                    ui.label(
                        egui::RichText::new(&err.message)
                            .color(egui::Color32::WHITE)
                            .small(),
                    );
                });
                #[cfg(not(target_arch = "wasm32"))]
                if ui.small_button("🔗 Report issue on GitHub").clicked() {
                    let _ =
                        open::that(format!("{}/issues/new", crate::app_config::GITHUB_REPO_URL));
                }
            });
        ui.add_space(2.0);
    }
    if !app_errors.errors.is_empty() {
        ui.separator();
    }
}

fn render_ai_assistant_header(
    ui: &mut egui::Ui,
    chat_state: &mut ChatState,
    ai_config: &mut AiConfig,
    available_models: &mut AvailableModels,
    settings_open: &mut SettingsDialogOpen,
) {
    let is_narrow = ui.available_width() < 420.0;

    ui.horizontal(|ui| {
        ui.heading("AI");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("🗑").on_hover_text("Clear chat history").clicked() {
                chat_state.session_start = chat_state.messages.len();
                chat_state.history_index = None;
                chat_state.pending_images.clear();
            }
            let selected_label = if ai_config.max_verification_rounds == u32::MAX {
                "∞".into()
            } else {
                ai_config.max_verification_rounds.to_string()
            };
            egui::ComboBox::from_id_salt("verify_rounds_main")
                .selected_text(selected_label)
                .width(32.0)
                .show_ui(ui, |ui| {
                    for &n in VERIFICATION_ROUND_CHOICES {
                        let label = if n == u32::MAX {
                            "∞".into()
                        } else {
                            n.to_string()
                        };
                        ui.selectable_value(&mut ai_config.max_verification_rounds, n, label);
                    }
                })
                .response
                .on_hover_text("Verification rounds");
            if !is_narrow {
                ui.label(
                    egui::RichText::new("Verify")
                        .small()
                        .color(egui::Color32::from_rgb(160, 160, 180)),
                );
            }
            ui.checkbox(&mut ai_config.extended_thinking, "")
                .on_hover_text("Extended Thinking");
            if !is_narrow {
                ui.label(
                    egui::RichText::new("extended")
                        .small()
                        .color(egui::Color32::from_rgb(160, 160, 180)),
                )
                .on_hover_text("Extended Thinking");
            }
            if ui
                .button(if available_models.needs_configuration {
                    "⚙ ⚠"
                } else {
                    "⚙"
                })
                .on_hover_text("AI Settings")
                .clicked()
            {
                settings_open.0 = !settings_open.0;
            }
            let mut current_adapter = ai_config.adapter_name.clone();
            let combo_w = if is_narrow { 70.0 } else { 80.0 };
            if ai_config.model_name.is_empty()
                && ai_config.custom_url().is_empty()
                && !available_models.loading
            {
                ui.colored_label(egui::Color32::from_rgb(255, 180, 50), "⚠ Select model in ⚙");
            }
            if egui::ComboBox::from_id_salt("provider_select_main")
                .selected_text(&current_adapter)
                .width(combo_w)
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    for &adapter in ADAPTER_NAMES {
                        let configured = env_var_for_adapter(adapter).is_none()
                            || ai_config
                                .custom_urls
                                .get(adapter)
                                .is_some_and(|url| !url.is_empty())
                            || env_var_for_adapter(adapter)
                                .and_then(env_var_value)
                                .is_some_and(|v| !v.is_empty())
                            || ai_config
                                .api_keys
                                .get(adapter)
                                .is_some_and(|k| !k.is_empty());
                        ui.add_enabled_ui(configured, |ui| {
                            if ui
                                .selectable_value(
                                    &mut current_adapter,
                                    adapter.to_string(),
                                    adapter,
                                )
                                .clicked()
                            {
                                changed = true;
                            }
                        });
                    }
                    changed
                })
                .inner
                .unwrap_or(false)
                && current_adapter != ai_config.adapter_name
            {
                if !ai_config.model_name.is_empty() {
                    ai_config
                        .model_per_provider
                        .insert(ai_config.adapter_name.clone(), ai_config.model_name.clone());
                }
                ai_config.adapter_name = current_adapter;
                ai_config.model_name = ai_config
                    .model_per_provider
                    .get(&ai_config.adapter_name)
                    .cloned()
                    .unwrap_or_default();
                available_models.models.clear();
                available_models.error = None;
                available_models.force_reload = true;
            }
        });
    });
}

fn render_pending_attachments(
    ui: &mut egui::Ui,
    chat_state: &mut ChatState,
    preview_state: &mut ImagePreviewState,
) {
    if chat_state.pending_images.is_empty() {
        return;
    }
    let max_w = ui.available_width();
    let mut to_remove = None;
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(max_w);
        ui.label(
            egui::RichText::new("Attached:")
                .small()
                .color(egui::Color32::from_rgb(140, 140, 160)),
        );
        for (i, img) in chat_state.pending_images.iter().enumerate() {
            let display_name = truncate_filename(&img.filename, 20);
            let resp = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("{display_name}  x"))
                        .small()
                        .color(egui::Color32::from_rgb(180, 180, 200)),
                )
                .fill(egui::Color32::from_rgb(40, 40, 58))
                .corner_radius(4.0),
            );
            if resp.clicked() {
                to_remove = Some(i);
            }
            let hovered = resp.hovered();
            resp.on_hover_text(&img.filename);
            if hovered {
                show_image_preview(ui, img, preview_state);
            }
        }
    });
    if let Some(idx) = to_remove {
        chat_state.pending_images.remove(idx);
    }
    ui.add_space(2.0);
}

fn truncate_filename(name: &str, max_chars: usize) -> String {
    if name.chars().count() <= max_chars {
        return name.to_string();
    }
    let chars: Vec<char> = name.chars().collect();
    let truncated: String = chars[..max_chars].iter().collect();
    format!("{truncated}…")
}

#[allow(clippy::future_not_send)]
async fn pick_chat_images() -> Vec<PickedImage> {
    let Some(handles) = rfd::AsyncFileDialog::new()
        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
        .pick_files()
        .await
    else {
        return Vec::new();
    };

    let mut images = Vec::with_capacity(handles.len());
    for handle in handles {
        images.push(PickedImage {
            filename: handle.file_name(),
            bytes: handle.read().await,
        });
    }
    images
}

fn render_chat_input(
    ui: &mut egui::Ui,
    chat_state: &mut ChatState,
    file_picker: &mut crate::plugins::ui::resources::FilePickerState,
    runtime: &TokioRuntime,
    no_model: bool,
) {
    ui.horizontal_wrapped(|ui| {
        let mut send_clicked = false;
        let mut enter_pressed = false;
        let mut attach_clicked = false;

        let _input_resp = ui
            .horizontal_top(|ui| {
                let text_width = ui.available_width() - 68.0;
                let resp = egui::ScrollArea::vertical()
                    .max_height(100.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut chat_state.input_buffer)
                                .hint_text("Ask the AI assistant...")
                                .desired_width(text_width)
                                .desired_rows(3)
                                .lock_focus(true),
                        )
                    })
                    .inner;
                ui.vertical(|ui| {
                    if chat_state.is_streaming {
                        if ui.button("⏹").clicked() {
                            chat_state.is_streaming = false;
                            chat_state.streaming_start = None;
                            chat_state.stream_receiver = None;
                            chat_state.verification =
                                crate::plugins::ai_chat::VerificationState::Idle;
                        }
                    } else {
                        let send_btn = ui
                            .add_enabled(!no_model, egui::Button::new("⬆"))
                            .on_disabled_hover_text("Configure a model in ⚙ AI Settings first");
                        send_clicked = send_btn.clicked();
                        enter_pressed = !no_model
                            && resp.has_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.shift);
                    }
                    attach_clicked = ui.button("📎").clicked();
                });
                resp
            })
            .inner;

        if attach_clicked && file_picker.receiver.is_none() {
            let (tx, rx) = mpsc::channel();
            file_picker.receiver = Some(Mutex::new(rx));

            #[cfg(not(target_arch = "wasm32"))]
            runtime.0.spawn(async move {
                let _ = tx.send(pick_chat_images().await);
            });

            #[cfg(target_arch = "wasm32")]
            wasm_bindgen_futures::spawn_local(async move {
                let _ = tx.send(pick_chat_images().await);
            });
        }

        #[cfg(target_arch = "wasm32")]
        {
            let _ = runtime;
        }

        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) && !chat_state.input_history.is_empty() {
            if chat_state.history_index.is_none() {
                chat_state.history_draft = Some(chat_state.input_buffer.clone());
                let idx = chat_state.input_history.len() - 1;
                chat_state.history_index = Some(idx);
                let (text, images) = chat_state.input_history[idx].clone();
                chat_state.input_buffer = text;
                chat_state.pending_images = images;
            } else if let Some(i) = chat_state.history_index
                && i > 0
            {
                let idx = i - 1;
                chat_state.history_index = Some(idx);
                let (text, images) = chat_state.input_history[idx].clone();
                chat_state.input_buffer = text;
                chat_state.pending_images = images;
            }
        } else if ui.input(|i| i.key_pressed(egui::Key::ArrowDown))
            && chat_state.history_index.is_some()
        {
            let len = chat_state.input_history.len();
            if let Some(i) = chat_state.history_index {
                if i + 1 < len {
                    let idx = i + 1;
                    chat_state.history_index = Some(idx);
                    let (text, images) = chat_state.input_history[idx].clone();
                    chat_state.input_buffer = text;
                    chat_state.pending_images = images;
                } else {
                    // Moving past the newest history entry restores the draft.
                    if let Some(draft) = chat_state.history_draft.take() {
                        chat_state.input_buffer = draft;
                    }
                    chat_state.pending_images.clear();
                    chat_state.history_index = None;
                }
            }
        } else if ui.input(|i| i.key_pressed(egui::Key::Escape))
            && chat_state.history_index.is_some()
        {
            if let Some(draft) = chat_state.history_draft.take() {
                chat_state.input_buffer = draft;
            }
            chat_state.history_index = None;
        }

        #[cfg(not(target_arch = "wasm32"))]
        if ui.input(|i| i.key_pressed(egui::Key::V) && i.modifiers.command)
            && let Some(img) = clipboard_image_as_chat_image()
        {
            chat_state.pending_images.push(img);
        }
        if enter_pressed {
            chat_state.input_buffer = chat_state.input_buffer.trim_end_matches('\n').to_string();
        }

        if (send_clicked || enter_pressed) && !chat_state.input_buffer.trim().is_empty() {
            let user_msg = chat_state.input_buffer.trim().to_string();
            let images = chat_state.pending_images.clone();
            chat_state
                .input_history
                .push((user_msg.clone(), images.clone()));
            chat_state.history_index = None;
            chat_state.history_draft = None;
            chat_state
                .messages
                .push(crate::plugins::ai_chat::ChatMessage {
                    role: "user".into(),
                    content: user_msg,
                    thinking: None,
                    images,
                    auto_generated: false,
                    is_error: false,
                });
            chat_state.input_buffer.clear();
            chat_state.is_streaming = true;
            chat_state.streaming_start = Some(web_time::Instant::now());
            chat_state.scroll_to_bottom = true;
        }
    });
}

fn render_chat_messages(
    ui: &mut egui::Ui,
    chat_state: &mut ChatState,
    chat_height: f32,
    view_textures: &[(String, egui::TextureHandle)],
    preview_state: &mut ImagePreviewState,
) {
    ui.allocate_ui(egui::vec2(ui.available_width(), chat_height), |ui| {
        let verifying = match &chat_state.verification {
            crate::plugins::ai_chat::VerificationState::WaitingForCompilation => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        egui::RichText::new("Compiling... will verify result")
                            .small()
                            .italics()
                            .color(egui::Color32::from_rgb(140, 140, 160)),
                    );
                });
                true
            }
            crate::plugins::ai_chat::VerificationState::Verifying(round) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        egui::RichText::new(format!("Verifying (round {round})..."))
                            .small()
                            .italics()
                            .color(egui::Color32::from_rgb(100, 160, 255)),
                    );
                });
                true
            }
            _ => false,
        };

        let status_height = if chat_state.is_streaming && !verifying {
            let no_resp = !chat_state
                .messages
                .last()
                .is_some_and(|m| m.role == "assistant" && !m.content.is_empty());
            if no_resp && !view_textures.is_empty() {
                64.0
            } else {
                24.0
            }
        } else {
            0.0
        };

        let scroll_height = (chat_height - status_height - ui.spacing().item_spacing.y).max(20.0);

        if chat_state.scroll_to_bottom {
            chat_state.scroll_to_bottom = false;
            let id = egui::Id::new("chat_scroll");
            let mut state = egui::scroll_area::State::default();
            state.offset.y = f32::MAX;
            state.store(ui.ctx(), id);
        }

        egui::ScrollArea::both()
            .id_salt("chat_scroll")
            .max_height(scroll_height)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let visible_messages = &chat_state.messages[chat_state.session_start..];
                let msg_count = visible_messages.len();
                let mut img_to_remove: Option<(usize, usize)> = None;
                for (i, msg) in visible_messages.iter().enumerate() {
                    let msg_idx = chat_state.session_start + i;
                    let is_last = i == msg_count - 1;
                    let is_user = msg.role == "user";
                    let (prefix, color, header_bg) = if is_user {
                        (
                            "You",
                            egui::Color32::from_rgb(140, 180, 255),
                            egui::Color32::from_rgb(50, 70, 120),
                        )
                    } else if msg.is_error {
                        (
                            "⚠",
                            egui::Color32::from_rgb(255, 140, 140),
                            egui::Color32::from_rgb(120, 50, 50),
                        )
                    } else {
                        (
                            "AI",
                            egui::Color32::from_rgb(160, 255, 160),
                            egui::Color32::from_rgb(45, 80, 45),
                        )
                    };

                    let header_text = if is_user {
                        let preview: String = msg.content.chars().take(80).collect();
                        format!(
                            "{prefix}: {preview}{}",
                            if msg.content.len() > 80 { "…" } else { "" }
                        )
                    } else {
                        format!("{prefix}:")
                    };
                    let id = ui.make_persistent_id(format!("chat_msg_{msg_idx}"));
                    let mut state =
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            id,
                            if chat_state.is_streaming {
                                is_last
                            } else {
                                is_last || !is_user || msg.is_error
                            },
                        );
                    if msg.is_error {
                        state.set_open(true);
                    }
                    state
                        .show_header(ui, |ui| {
                            let w = ui.available_width();
                            egui::Frame::new()
                                .fill(header_bg)
                                .corner_radius(egui::CornerRadius::same(3))
                                .inner_margin(egui::Margin::symmetric(4, 2))
                                .show(ui, |ui| {
                                    ui.set_width(w);
                                    ui.label(
                                        egui::RichText::new(&header_text).strong().color(color),
                                    );
                                });
                        })
                        .body(|ui| {
                            if let Some(ref thinking) = msg.thinking {
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    id.with("thinking"),
                                    true,
                                )
                                .show_header(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("💭 Thinking…")
                                            .italics()
                                            .color(egui::Color32::from_rgb(180, 180, 180)),
                                    );
                                })
                                .body(|ui| {
                                    crate::plugins::ui::chat::render_thinking_content(ui, thinking);
                                });
                            }
                            if !is_user || msg.content.chars().count() > 80 {
                                render_chat_content(ui, &msg.content, msg.is_error);
                            }
                            if !msg.images.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    for (img_i, img) in msg.images.iter().enumerate() {
                                        let frame = egui::Frame::new()
                                            .fill(egui::Color32::from_rgb(40, 40, 58))
                                            .corner_radius(egui::CornerRadius::same(3))
                                            .inner_margin(egui::Margin::symmetric(4, 2))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    let label = ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new("📷")
                                                                .small()
                                                                .color(egui::Color32::from_rgb(
                                                                    160, 160, 180,
                                                                )),
                                                        )
                                                        .sense(egui::Sense::click()),
                                                    );
                                                    if is_user && ui.small_button("x").clicked() {
                                                        img_to_remove = Some((msg_idx, img_i));
                                                    }
                                                    label
                                                })
                                            });
                                        if frame.inner.inner.hovered() {
                                            show_image_preview(ui, img, preview_state);
                                        }
                                        #[cfg(not(target_arch = "wasm32"))]
                                        if frame.inner.inner.clicked() {
                                            copy_chat_image_to_clipboard(img);
                                        }
                                    }
                                });
                            }
                        });
                    ui.add_space(2.0);
                }
                if let Some((m_idx, i_idx)) = img_to_remove {
                    chat_state.messages[m_idx].images.remove(i_idx);
                }
            });

        if chat_state.is_streaming && !verifying {
            let remaining = ui.available_height() - status_height;
            if remaining > 0.0 {
                ui.add_space(remaining);
            }

            let elapsed_text = chat_state.streaming_start.map(|t| {
                let s = t.elapsed().as_secs();
                let m = s / 60;
                let s = s % 60;
                format!("{m:02}:{s:02}")
            });
            let no_resp = !chat_state
                .messages
                .last()
                .is_some_and(|m| m.role == "assistant" && !m.content.is_empty());
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            if no_resp && !view_textures.is_empty() {
                let view_idx = (ui.input(|i| i.time) / 1.5) as usize % view_textures.len();
                let (label, texture) = &view_textures[view_idx];
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if let Some(ref elapsed) = elapsed_text {
                        ui.label(
                            egui::RichText::new(elapsed)
                                .monospace()
                                .color(egui::Color32::from_rgb(200, 200, 210)),
                        );
                    }
                    let img_resp = ui.image(egui::load::SizedTexture::new(
                        texture.id(),
                        egui::vec2(58.0, 58.0),
                    ));
                    if img_resp.hovered() {
                        crate::plugins::ui::utils::show_texture_preview(ui, texture);
                    }
                    ui.label(
                        egui::RichText::new(format!("📷 {label}"))
                            .small()
                            .color(egui::Color32::from_rgb(140, 140, 160)),
                    );
                });
            } else {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if let Some(ref elapsed) = elapsed_text {
                        ui.label(
                            egui::RichText::new(elapsed)
                                .monospace()
                                .color(egui::Color32::from_rgb(200, 200, 210)),
                        );
                    }
                });
            }
            ui.ctx().request_repaint();
        }
    });
}

fn render_code_header(
    ui: &mut egui::Ui,
    scad_code: &mut ScadCode,
    chat_state: &mut ChatState,
    compilation_state: &mut CompilationState,
    last_parts: &LastCompiledParts,
    export_state: &mut ExportState,
    runtime: &TokioRuntime,
) {
    ui.horizontal(|ui| {
        ui.heading("Code");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // In RTL layout first item = rightmost, last item = leftmost.
            // Compile/Cancel is last so it remains the leftmost item on narrow
            // panels.

            #[cfg(not(target_arch = "wasm32"))]
            if export_state.receiver.is_some() {
                ui.spinner();
            } else {
                let has_parts = !last_parts.parts.is_empty();
                let export_btn = ui.add_enabled(has_parts, egui::Button::new("💾"));
                let export_btn_rect = export_btn.rect;
                let export_btn_hovered = export_btn.contains_pointer();
                let export_btn_clicked = export_btn.clicked();
                export_btn.on_hover_text("Export model");

                let popup_id = ui.make_persistent_id("export_popup");
                if export_btn_clicked && has_parts {
                    ui.memory_mut(|m| m.toggle_popup(popup_id));
                }
                if ui.memory(|m| m.is_popup_open(popup_id)) {
                    let area = egui::Area::new(popup_id)
                        .order(egui::Order::Foreground)
                        .fixed_pos(export_btn_rect.left_bottom())
                        .show(ui.ctx(), |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.set_min_width(160.0);
                                for &fmt in ALL_FORMATS {
                                    if ui.button(fmt.label()).clicked() {
                                        let ext = fmt.extension();
                                        let (tx, rx) = mpsc::channel();
                                        export_state.receiver = Some(Mutex::new(rx));
                                        export_state.pending_format = Some(fmt);
                                        runtime.0.spawn(async move {
                                            let handle = rfd::AsyncFileDialog::new()
                                                .set_file_name(format!("model.{ext}"))
                                                .add_filter(ext.to_uppercase(), &[ext])
                                                .save_file()
                                                .await;
                                            let _ = tx.send(handle.map(|h| h.path().to_path_buf()));
                                        });
                                        ui.memory_mut(bevy_egui::egui::Memory::close_popup);
                                    }
                                }
                            });
                        });
                    if ui.input(|i| i.pointer.any_click())
                        && !area.response.contains_pointer()
                        && !export_btn_hovered
                    {
                        ui.memory_mut(bevy_egui::egui::Memory::close_popup);
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = (last_parts, export_state, runtime);
            }

            if ui.button("🗑").on_hover_text("Clear code & chat").clicked() {
                scad_code.text.clear();
                scad_code.dirty = true;
                compilation_state.should_zoom = true;
                chat_state.session_start = chat_state.messages.len();
                chat_state.history_index = None;
                chat_state.verification = crate::plugins::ai_chat::VerificationState::Idle;
            }

            let fn_label = format!("$fn {}", scad_code.fn_value);
            egui::ComboBox::from_id_salt("fn_select")
                .selected_text(&fn_label)
                .width(64.0)
                .show_ui(ui, |ui| {
                    for &v in &[8u32, 12, 16, 24, 32, 48, 64, 96, 128, 256] {
                        if ui
                            .selectable_value(&mut scad_code.fn_value, v, v.to_string())
                            .changed()
                        {
                            scad_code.dirty = true;
                        }
                    }
                });

            if compilation_state.is_compiling {
                if ui
                    .button(
                        egui::RichText::new("⏹ Cancel")
                            .color(egui::Color32::from_rgb(255, 100, 100)),
                    )
                    .clicked()
                    && let Some(cancel) = &compilation_state.cancel_signal
                {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            } else if ui.button("Compile").clicked() {
                scad_code.dirty = true;
            }
        });
    });
}

fn render_view_selector(ui: &mut egui::Ui, scad_code: &mut ScadCode) {
    let (active_view, all_views) = detect_views(&scad_code.text);
    if all_views.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("View:")
                    .small()
                    .color(egui::Color32::from_rgb(160, 160, 180)),
            );
            let current = active_view.unwrap_or_default();
            for view_name in &all_views {
                let is_active = *view_name == current;
                let label = egui::RichText::new(view_name.as_str()).small();
                let label = if is_active {
                    label.strong().color(egui::Color32::from_rgb(100, 160, 255))
                } else {
                    label.color(egui::Color32::from_rgb(160, 160, 180))
                };
                if ui.selectable_label(is_active, label).clicked() && !is_active {
                    set_active_view(&mut scad_code.text, view_name);
                    scad_code.dirty = true;
                }
            }
        });
    }
}

fn render_settings_dialog(
    ctx: &egui::Context,
    settings_open: &mut SettingsDialogOpen,
    ai_config: &mut AiConfig,
    available_models: &mut AvailableModels,
) {
    egui::Window::new("⚙ AI Settings")
        .open(&mut settings_open.0)
        .resizable(true)
        .default_width(360.0)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            if available_models.needs_configuration {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 180, 50),
                    "⚠ Previously configured model is no longer available.",
                );
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                ui.label("Provider:");
                let prev = ai_config.adapter_name.clone();
                egui::ComboBox::from_id_salt("ai_adapter_select")
                    .selected_text(&ai_config.adapter_name)
                    .show_ui(ui, |ui| {
                        for &adapter in ADAPTER_NAMES {
                            ui.selectable_value(
                                &mut ai_config.adapter_name,
                                adapter.to_string(),
                                adapter,
                            );
                        }
                    });
                if ai_config.adapter_name != prev {
                    if !ai_config.model_name.is_empty() {
                        ai_config
                            .model_per_provider
                            .insert(prev, ai_config.model_name.clone());
                    }
                    ai_config.model_name = ai_config
                        .model_per_provider
                        .get(&ai_config.adapter_name)
                        .cloned()
                        .unwrap_or_default();
                    available_models.force_reload = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Model:");
                let has_custom_url = !ai_config.custom_url().is_empty();
                let needs_key =
                    env_var_for_adapter(&ai_config.adapter_name).is_some() && !has_custom_url;
                let has_key = !needs_key
                    || env_var_for_adapter(&ai_config.adapter_name)
                        .and_then(env_var_value)
                        .is_some_and(|v| !v.is_empty())
                    || !ai_config.api_key().is_empty();
                if has_key {
                    let model_label = if available_models.loading {
                        "Loading...".into()
                    } else if available_models.models.is_empty() {
                        "No models available".into()
                    } else {
                        ai_config.model_name.clone()
                    };
                    let prev_model = ai_config.model_name.clone();
                    egui::ComboBox::from_id_salt("ai_model_select")
                        .selected_text(model_label)
                        .show_ui(ui, |ui| {
                            for model in &available_models.models {
                                ui.selectable_value(
                                    &mut ai_config.model_name,
                                    model.clone(),
                                    model.as_str(),
                                );
                            }
                        });
                    if has_custom_url && available_models.models.is_empty() {
                        ui.add_space(4.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut ai_config.model_name)
                                .hint_text("Optional manual model name"),
                        );
                    }
                    if ai_config.model_name != prev_model
                        && available_models.models.contains(&ai_config.model_name)
                    {
                        available_models.needs_configuration = false;
                    }
                } else {
                    ui.colored_label(egui::Color32::from_rgb(255, 180, 50), "⚠ Set API key first");
                }
            });
            if let Some(ref err) = available_models.error {
                egui::ScrollArea::vertical()
                    .id_salt("settings_err_scroll")
                    .max_height(60.0)
                    .show(ui, |ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 100, 100),
                            format!("⚠ {err}"),
                        );
                    });
            }

            ui.horizontal(|ui| {
                ui.label("Endpoint URL:");
                let placeholder = default_placeholder_url(&ai_config.adapter_name);
                let adapter_name = ai_config.adapter_name.clone();
                let url = ai_config.custom_url_mut();
                let res = ui.add(egui::TextEdit::singleline(url).hint_text(placeholder));
                if res.lost_focus()
                    || (res.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    let normalized = normalize_custom_url(&adapter_name, url);
                    let has_endpoint = !normalized.is_empty();
                    let url_changed = *url != normalized;
                    *url = normalized;

                    if res.lost_focus() && (has_endpoint || url_changed) {
                        available_models.force_reload = true;
                        ctx.request_repaint();
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("API Key:");
                let has_custom_url = !ai_config.custom_url().is_empty();
                let env_var = env_var_for_adapter(&ai_config.adapter_name);
                let env_set = env_var
                    .and_then(env_var_value)
                    .is_some_and(|v| !v.is_empty());
                if env_set && ai_config.api_key().is_empty() && !has_custom_url {
                    ui.add_enabled(
                        false,
                        egui::TextEdit::singleline(&mut String::new())
                            .hint_text(format!("Set via {}", env_var.unwrap_or(""))),
                    );
                } else {
                    let visibility_id = egui::Id::new("ai_settings_api_key_visible");
                    let mut show_api_key =
                        ctx.data(|d| d.get_temp::<bool>(visibility_id).unwrap_or(false));
                    let key = ai_config.api_key_mut();
                    let api_key_response = ui.add(
                        egui::TextEdit::singleline(key)
                            .password(!show_api_key)
                            .hint_text(if env_set {
                                "Override env var"
                            } else {
                                "Enter API key"
                            }),
                    );
                    let mut toggled_visibility = false;
                    if ui
                        .button(if show_api_key { "🙈" } else { "👁" })
                        .on_hover_text(if show_api_key {
                            "Hide API key"
                        } else {
                            "Show API key as plain text"
                        })
                        .clicked()
                    {
                        show_api_key = !show_api_key;
                        toggled_visibility = true;
                        ctx.request_repaint();
                    }
                    ctx.data_mut(|d| d.insert_temp(visibility_id, show_api_key));

                    // Normalize persisted keys before triggering discovery.
                    if api_key_response.lost_focus() && !toggled_visibility {
                        let trimmed_key = key.trim().to_string();
                        if trimmed_key != *key {
                            *key = trimmed_key;
                        }

                        available_models.force_reload = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Temperature:");
                ui.add(egui::Slider::new(&mut ai_config.temperature, 0.0..=2.0).step_by(0.1));
            });
            ui.label("System Prompt:");
            egui::ScrollArea::vertical()
                .id_salt("settings_prompt_scroll")
                .max_height(120.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut ai_config.system_prompt.clone())
                            .desired_width(ui.available_width())
                            .desired_rows(4)
                            .interactive(false),
                    );
                });
        });
}
