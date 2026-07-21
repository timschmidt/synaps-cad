#![cfg_attr(target_arch = "wasm32", allow(clippy::future_not_send))]

use bevy::prelude::*;
use std::sync::{Mutex, mpsc};

use super::code_editor::ScadCode;
use super::compilation::PartLabel;

pub struct AiChatPlugin;

const DEFAULT_SYSTEM_PROMPT: &str = "\
You are an AI assistant for a 3D CAD application (SynapsCAD). \
The user is working with OpenSCAD code. Help them modify their 3D models.\n\
\n\
## Code Output\n\
\n\
For **targeted edits** (changing a parameter, adding/modifying a module, small fixes),\n\
use search-replace blocks — one per logical change:\n\
\n\
<<<REPLACE\n\
<exact text to find — must appear exactly once in the current code>\n\
===\n\
<replacement text>\n\
>>>\n\
\n\
Rules:\n\
- The search text must match the current code **exactly** (whitespace, newlines, indentation).\n\
- Use enough context (e.g. the full function body) to make the match unique.\n\
- You can include multiple <<<REPLACE blocks in one response.\n\
- Leave the replacement empty to delete the matched text.\n\
\n\
For **large rewrites** or when writing code **from scratch**, wrap the full code in a synapscad block:\n\
\\`\\`\\`synapscad\n\
<complete code here>\n\
\\`\\`\\`\n\
\n\
Always use the `$view` system: define your geometry in a module and select it with an \
`if ($view == \"name\")` conditional. Start with a single view called \"main\":\n\
```\n\
$view = \"main\";\n\
module view_main() { /* all geometry here */ }\n\
if ($view == \"main\") view_main();\n\
```\n\
Only add additional views (e.g. \"assembly\", \"part_a\") when the user explicitly asks for them. \
If you create multiple parts, create views for each part.\n\
\n\n\
## General Guidelines\n\
Be concise and helpful.\n\
Always verify your results after making changes with the given 3D context \
information (orthographic views, bounding boxes, part counts). \
If something is unclear, ask clarifying questions before making changes. \
If something looks wrong in the rendered views, suggest corrections.\n\
In verification rounds, carefully compare the rendered views against the user's request. \
\n\
## Part Colors\n\
Use `color()` to give each part a realistic, semantically meaningful color. \
For example: green for plants/leaves, brown for wood/soil, red for flowers, \
gray for metal/concrete, blue for water, white for snow, orange for flames. \
Always pick colors that match the real-world material or object being modeled. \
Example: `color(\"green\") cylinder(h = 20, r = 3);` for a plant stem.\n\
\n\
## Physical Realism\n\
When generating 3D models, consider real-world physics and functionality. \
Objects should be structurally plausible and functionally correct:\n\
- A pipe must be a hollow cylinder (`difference()` of two cylinders), not a solid rod.\n\
- A cup needs an interior cavity so it can hold liquid.\n\
- A wheel should have an axle hole.\n\
- Load-bearing structures (bridges, shelves) need appropriate thickness and supports.\n\
- Moving parts (hinges, gears) need clearance gaps between components.\n\
- Materials that are connected (e.g., a wooden tabletop on metal legs) do not \"flow\" together; \
ensure they have distinct boundaries and do not simply merge into a single shape.\n\
- For objects with multiple parts, ensure each individual part is physically sound and \
\"fits\" correctly with others (proper tolerances, no unintended intersections, alignment).\n\
Think about what the object does in the real world and ensure the geometry reflects that.";

/// Supported AI provider adapters.
pub const ADAPTER_NAMES: &[&str] = &[
    "Anthropic",
    "OpenAI",
    "Gemini",
    "Groq",
    "Ollama",
    "DeepSeek",
    "Cohere",
    "Fireworks",
    "Together",
    "Xai",
    "Zai",
];

/// Returns the environment variable name used for the API key of the given adapter.
/// Returns `None` for adapters that don't need an API key (e.g. Ollama).
pub fn env_var_for_adapter(adapter: &str) -> Option<&'static str> {
    match adapter {
        "Anthropic" => Some("ANTHROPIC_API_KEY"),
        "OpenAI" => Some("OPENAI_API_KEY"),
        "Gemini" => Some("GEMINI_API_KEY"),
        "Groq" => Some("GROQ_API_KEY"),
        "DeepSeek" => Some("DEEPSEEK_API_KEY"),
        "Cohere" => Some("COHERE_API_KEY"),
        "Fireworks" => Some("FIREWORKS_API_KEY"),
        "Together" => Some("TOGETHER_API_KEY"),
        "Xai" => Some("XAI_API_KEY"),
        "Zai" => Some("ZAI_API_KEY"),
        _ => None,
    }
}

/// Returns the default placeholder URL for the given adapter.
/// These match the genai library's built-in default endpoints.
pub fn default_placeholder_url(adapter: &str) -> &'static str {
    match adapter {
        "Anthropic" => "https://api.anthropic.com/v1/",
        "OpenAI" => "https://api.openai.com/v1/",
        "Gemini" => "https://generativelanguage.googleapis.com/v1beta/",
        "Groq" => "https://api.groq.com/openai/v1/",
        "Ollama" => "http://localhost:11434/",
        "DeepSeek" => "https://api.deepseek.com/v1/",
        "Cohere" => "https://api.cohere.com/v1/",
        "Fireworks" => "https://api.fireworks.ai/inference/v1/",
        "Together" => "https://api.together.xyz/v1/",
        "Xai" => "https://api.x.ai/v1/",
        "Zai" => "https://api.z.ai/api/paas/v4/",
        _ => "",
    }
}

/// Normalizes a custom endpoint URL for the selected adapter.
///
/// Rules:
/// - trim whitespace
/// - ensure trailing slash
/// - if only host/root is provided, inherit the adapter's default base path
///   (e.g. `http://localhost:1234/` + Anthropic -> `http://localhost:1234/v1/`)
pub fn normalize_custom_url(adapter: &str, raw_url: &str) -> String {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut normalized = trimmed.to_string();
    if !normalized.ends_with('/') {
        normalized.push('/');
    }

    let Ok(mut parsed) = url::Url::parse(&normalized) else {
        return normalized;
    };

    if parsed.path() == "/"
        && let Ok(default_url) = url::Url::parse(default_placeholder_url(adapter))
    {
        let default_path = default_url.path();
        if !default_path.is_empty() && default_path != "/" {
            parsed.set_path(default_path);
            let mut with_default_path = parsed.to_string();
            if !with_default_path.ends_with('/') {
                with_default_path.push('/');
            }
            return with_default_path;
        }
    }

    normalized
}

#[derive(Resource)]
pub struct AiConfig {
    pub adapter_name: String,
    pub model_name: String,
    /// Per-provider API keys (`adapter_name` → key).
    pub api_keys: std::collections::HashMap<String, String>,
    /// Per-provider last-used model (`adapter_name` → `model_name`).
    pub model_per_provider: std::collections::HashMap<String, String>,
    /// Per-provider custom endpoint URLs (`adapter_name` → URL).
    /// Empty string means "use default endpoint".
    pub custom_urls: std::collections::HashMap<String, String>,
    pub system_prompt: String,
    pub temperature: f64,
    /// Maximum automatic verification rounds (`u32::MAX` = unlimited).
    pub max_verification_rounds: u32,
    pub extended_thinking: bool,
    /// Whether compiled model renders are included as automatic AI context.
    pub send_images: bool,
}

impl AiConfig {
    /// Get the API key for the currently selected adapter.
    pub fn api_key(&self) -> &str {
        self.api_keys
            .get(&self.adapter_name)
            .map_or("", String::as_str)
    }

    /// Get a mutable reference to the API key for the currently selected adapter.
    pub fn api_key_mut(&mut self) -> &mut String {
        self.api_keys.entry(self.adapter_name.clone()).or_default()
    }

    /// Get the custom endpoint URL for the currently selected adapter.
    pub fn custom_url(&self) -> &str {
        self.custom_urls
            .get(&self.adapter_name)
            .map_or("", String::as_str)
    }

    /// Get a mutable reference to the custom endpoint URL for the currently selected adapter.
    pub fn custom_url_mut(&mut self) -> &mut String {
        self.custom_urls
            .entry(self.adapter_name.clone())
            .or_default()
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        let mut custom_urls = std::collections::HashMap::new();
        #[cfg(not(target_arch = "wasm32"))]
        let ollama_host = std::env::var("OLLAMA_HOST").unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        let ollama_host = option_env!("OLLAMA_HOST").unwrap_or_default().to_string();
        if !ollama_host.is_empty() {
            let mut host = ollama_host;
            if !host.ends_with('/') {
                host.push('/');
            }
            custom_urls.insert("Ollama".into(), host);
        }
        Self {
            adapter_name: "Anthropic".into(),
            model_name: "claude-3-5-sonnet-latest".into(),
            api_keys: std::collections::HashMap::new(),
            model_per_provider: std::collections::HashMap::new(),
            custom_urls,
            system_prompt: DEFAULT_SYSTEM_PROMPT.into(),
            temperature: 0.1,
            max_verification_rounds: 2,
            extended_thinking: false,
            send_images: true,
        }
    }
}

fn model_views_for_request<'a>(
    model_views: &'a super::compilation::ModelViews,
    current_code: &str,
    fn_value: u32,
    send_images: bool,
) -> (
    &'a [super::compilation::ModelView],
    &'a [super::compilation::AlternateModelView],
) {
    if send_images && model_views.matches_source(current_code, fn_value) {
        (&model_views.views, &model_views.other_views)
    } else {
        (&[], &[])
    }
}

/// Dynamically fetched model names for the selected adapter.
#[derive(Resource, Default)]
pub struct AvailableModels {
    pub models: Vec<String>,
    pub loading: bool,
    pub last_adapter: String,
    pub last_api_key: String,
    pub last_custom_url: String,
    pub error: Option<String>,
    /// Set to true when the persisted model is no longer available.
    pub needs_configuration: bool,
    /// Saved model name to restore after model list is fetched.
    pub pending_model: Option<String>,
    /// Set by UI on focus-lost to trigger a model reload.
    pub force_reload: bool,
    #[allow(clippy::type_complexity)]
    pub receiver: Option<Mutex<mpsc::Receiver<Result<Vec<String>, String>>>>,
}

/// An image attached to a chat message, stored as base64 PNG/JPEG.
#[derive(Clone, Debug)]
pub struct ChatImage {
    pub filename: String,
    pub mime_type: String,
    pub base64_data: String,
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// Optional reasoning/thinking content from the model.
    pub thinking: Option<String>,
    /// Images attached to this message.
    pub images: Vec<ChatImage>,
    /// True for messages generated automatically (e.g. verification rounds), not typed by user.
    pub auto_generated: bool,
    /// True for error messages (compilation errors, internal errors).
    pub is_error: bool,
}

#[derive(Debug)]
pub enum AiStreamChunk {
    /// Incremental text content chunk.
    Chunk(String),
    /// Incremental reasoning/thinking chunk.
    ReasoningChunk(String),
    /// Stream finished — final content and reasoning are built from chunks.
    Done {
        content: String,
        reasoning: Option<String>,
    },
    Error(String),
}

/// Maximum number of automatic verify-and-fix rounds per user request.
/// Predefined choices for the UI dropdown.
pub const VERIFICATION_ROUND_CHOICES: &[u32] = &[1, 2, 5, 10, 15, 20, 50, 100, u32::MAX];

const VERIFICATION_PROMPT: &str = "\
These are the rendered orthographic views AFTER your code change was compiled. \
Compare them carefully against the user's original request. \
If the result does NOT match what was asked for, provide corrected code in a synapscad code block. \
If it looks correct, briefly confirm what you see — do NOT repeat the code.\n\
\n\
## Verification Checklist\n\
When verifying or reviewing a model, go through the following checklist:\n\
\n\
### Phase 1: Mechanical Connections (Fasteners & Tolerances)\n\
- [ ] Hole Alignment: Do center axes of all through-holes on overlapping parts perfectly align?\n\
- [ ] Thread & Clearance Tolerances: Is the clearance hole slightly larger than the bolt diameter (e.g. 3.2mm hole for M3 screw)? For tapped holes, does the diameter match the minor diameter?\n\
- [ ] Fastener Completeness: Does every bolt through an unthreaded hole have a nut? Are washers present where needed?\n\
- [ ] Length & Protrusion: Are bolts long enough to engage but short enough not to penetrate unintended components?\n\
- [ ] Interference Detection: Do any solid parts intersect each other (bolt heads clipping into walls, etc.)?\n\
\n\
### Phase 2: Fluid Dynamics & Containment\n\
- [ ] Watertight Integrity: Is the fluid chamber completely enclosed (no non-manifold edges, holes, or gaps)?\n\
- [ ] Gasket/Seal Verification: Where parts join to contain fluid, is there a seal or gasket face?\n\
- [ ] Internal Flow Paths: Is there an unobstructed path from inlet to outlet? Are screws or components blocking the channel?\n\
- [ ] Surface Normals: Are all normals on the fluid boundary facing the correct direction?\n\
\n\
### Phase 3: Physics & Simulation Constraints\n\
- [ ] Density & Mass: Is every part assigned a material density? Is the Center of Mass logical?\n\
- [ ] Buoyancy: If an object should float, is its overall density less than the fluid density?\n\
- [ ] Joint Constraints: Are rigidly connected parts grouped with fixed joints? Do moving parts have correct degrees of freedom and limits?\n\
- [ ] Gravity & Orientation: Is the gravity vector correct relative to fluid reservoirs?\n\
\n\
### Phase 4: Metadata & BOM\n\
- [ ] BOM Consistency: Does the number of generated screws, nuts, and parts match the intended BOM?\n\
- [ ] Naming Conventions: Are parts named logically (e.g. M3_Bolt_12mm, Fluid_Reservoir_Bottom) rather than generic defaults?";

#[derive(Resource)]
pub struct ChatState {
    pub messages: Vec<ChatMessage>,
    pub input_buffer: String,
    pub input_history: Vec<(String, Vec<ChatImage>)>,
    pub history_index: Option<usize>,
    /// Saved draft text when the user starts cycling through history.
    pub history_draft: Option<String>,
    pub is_streaming: bool,
    pub stream_receiver: Option<Mutex<mpsc::Receiver<AiStreamChunk>>>,
    /// Images queued to attach to the next sent message.
    pub pending_images: Vec<ChatImage>,
    /// When the AI produces code that triggers compilation, this is set to
    /// `WaitingForCompilation`. After compilation completes and views update,
    /// it transitions to `ReadyToVerify` and a verification round fires.
    pub verification: VerificationState,
    /// Index into `messages` where the current session starts.
    /// Messages before this index are displayed but not sent to the AI.
    pub session_start: usize,
    /// When streaming started, used to display elapsed time.
    pub streaming_start: Option<web_time::Instant>,
    /// Set to true when the chat should scroll to the bottom (e.g. on send).
    pub scroll_to_bottom: bool,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            input_buffer: String::new(),
            input_history: Vec::new(),
            history_index: None,
            history_draft: None,
            is_streaming: false,
            stream_receiver: None,
            pending_images: Vec::new(),
            verification: VerificationState::Idle,
            session_start: 0,
            streaming_start: None,
            scroll_to_bottom: false,
        }
    }
}

/// Tracks the auto-verification loop state.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub enum VerificationState {
    #[default]
    Idle,
    /// AI produced code; waiting for compilation to finish and views to update.
    WaitingForCompilation,
    /// Compilation done, new views available — trigger verification call.
    ReadyToVerify,
    /// Currently running a verification round (the Nth).
    Verifying(u32),
    /// Compilation failed with an error — send error back to AI to fix.
    ErrorRecovery(String),
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
pub struct TokioRuntime(pub tokio::runtime::Runtime);

#[cfg(target_arch = "wasm32")]
#[derive(Resource, Default)]
pub struct TokioRuntime;

pub const fn ai_networking_available() -> bool {
    true
}

impl Plugin for AiChatPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(target_arch = "wasm32")]
        {
            app.init_resource::<AiConfig>()
                .init_resource::<ChatState>()
                .init_resource::<AvailableModels>()
                .init_resource::<TokioRuntime>()
                .add_systems(
                    Update,
                    (
                        fetch_models_system.run_if(|available: Res<AvailableModels>| {
                            available.loading
                                || available.receiver.is_some()
                                || available.force_reload
                                || available.last_adapter.is_empty()
                        }),
                        ai_send_system,
                        ai_receive_system,
                        ai_verify_system,
                    )
                        .chain(),
                );
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let tokio_rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
            app.init_resource::<AiConfig>()
                .init_resource::<ChatState>()
                .init_resource::<AvailableModels>()
                .insert_resource(TokioRuntime(tokio_rt))
                .add_systems(
                    Update,
                    (
                        fetch_models_system.run_if(
                            // Poll active requests, honor explicit reloads, and
                            // fetch once on startup while `last_adapter` is empty.
                            |available: Res<AvailableModels>| {
                                available.loading
                                    || available.receiver.is_some()
                                    || available.force_reload
                                    || available.last_adapter.is_empty()
                            },
                        ),
                        ai_send_system,
                        ai_receive_system,
                        ai_verify_system,
                    )
                        .chain(),
                );
        }
    }
}

/// Fetch model names when adapter selection changes.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_models_system(
    mut ai_config: ResMut<AiConfig>,
    mut available: ResMut<AvailableModels>,
    runtime: Res<TokioRuntime>,
    mut redraw: EventWriter<bevy::window::RequestRedraw>,
) {
    if let Some(ref rx_mutex) = available.receiver {
        redraw.send(bevy::window::RequestRedraw);
        let rx = rx_mutex.lock().unwrap();
        if let Ok(result) = rx.try_recv() {
            drop(rx);
            available.loading = false;
            available.receiver = None;
            match result {
                Ok(models) => {
                    available.error = None;
                    // Restore the pending selection only if the provider still
                    // advertises it.
                    if let Some(pending) = available.pending_model.take() {
                        if models.contains(&pending) {
                            ai_config.model_name = pending;
                            available.needs_configuration = false;
                        } else {
                            available.needs_configuration = !pending.is_empty();
                        }
                    } else {
                        // An empty selection is valid; only a stale named model
                        // requires reconfiguration.
                        available.needs_configuration = !ai_config.model_name.is_empty()
                            && !models.contains(&ai_config.model_name);
                    }
                    available.models = models;
                }
                Err(e) => {
                    eprintln!("[SynapsCAD] Failed to fetch models: {e}");
                    available.models.clear();
                    available.error = Some(e);
                    // Authentication and network failures do not invalidate
                    // the saved model selection.
                }
            }
            return;
        }
    }

    // Trigger a new fetch if adapter/API key/custom endpoint changed,
    // or when explicitly requested by the UI (focus-lost reload).
    let current_key = ai_config.api_key().to_string();
    let key_changed = available.last_api_key != current_key;
    let current_url = ai_config.custom_url().to_string();
    let url_changed = available.last_custom_url != current_url;
    let force_reload = available.force_reload;
    available.force_reload = false;
    if (force_reload
        || available.last_adapter != ai_config.adapter_name
        || key_changed
        || url_changed)
        && !available.loading
    {
        // Do not display models returned by a previous provider or endpoint.
        available.models.clear();
        if !ai_config.model_name.is_empty() {
            available.pending_model = Some(ai_config.model_name.clone());
        }
        ai_config.model_name.clear();
        available.last_adapter.clone_from(&ai_config.adapter_name);
        available.last_api_key.clone_from(&current_key);
        available.last_custom_url.clone_from(&current_url);
        available.loading = true;

        let adapter_name = ai_config.adapter_name.clone();
        let api_key = if current_key.is_empty() {
            None
        } else {
            Some(current_key)
        };
        let custom_url = current_url;
        let (tx, rx) = mpsc::channel();
        available.receiver = Some(Mutex::new(rx));

        runtime.0.spawn(async move {
            let result = fetch_model_names(&adapter_name, api_key.as_deref(), &custom_url).await;
            let _ = tx.send(result);
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn fetch_model_names(
    adapter_name: &str,
    api_key: Option<&str>,
    custom_url: &str,
) -> Result<Vec<String>, String> {
    use genai::Client;
    use genai::adapter::AdapterKind;
    use genai::resolver::AuthData;

    let adapter_kind = match adapter_name {
        "OpenAI" => AdapterKind::OpenAI,
        "Anthropic" => AdapterKind::Anthropic,
        "Gemini" => AdapterKind::Gemini,
        "Groq" => AdapterKind::Groq,
        "Ollama" => AdapterKind::Ollama,
        "DeepSeek" => AdapterKind::DeepSeek,
        "Cohere" => AdapterKind::Cohere,
        "Fireworks" => AdapterKind::Fireworks,
        "Together" => AdapterKind::Together,
        "Xai" => AdapterKind::Xai,
        "Zai" => AdapterKind::Zai,
        other => return Err(format!("Unknown adapter: {other}")),
    };

    // Ollama model discovery uses its native tags endpoint.
    if adapter_kind == AdapterKind::Ollama {
        let base = if custom_url.is_empty() {
            default_placeholder_url("Ollama")
        } else {
            custom_url
        };
        let url = format!("{base}api/tags");
        let client = reqwest::Client::new();
        let mut request = client.get(&url);
        if let Some(key) = api_key
            && !key.is_empty()
        {
            request = request.header("Authorization", format!("Bearer {key}"));
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("Failed to fetch Ollama models: {e}"))?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(
                "Unauthorized (401). Please check your API key for this Ollama host.".into(),
            );
        }
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Ollama models: {e}"))?;

        let mut models = Vec::new();
        if let Some(models_value) = body.get("models").and_then(|m| m.as_array()) {
            for model in models_value {
                if let Some(name) = model.get("name").and_then(|n| n.as_str()) {
                    models.push(name.to_string());
                }
            }
        }
        return Ok(models);
    }

    // Custom non-Ollama endpoints use OpenAI-compatible model discovery.
    if !custom_url.is_empty() {
        let url = format!("{custom_url}models");
        let client = reqwest::Client::new();
        let mut request = client.get(&url);
        if let Some(key) = api_key
            && !key.is_empty()
        {
            request = request.header("Authorization", format!("Bearer {key}"));
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("Failed to fetch models from custom endpoint: {e}"))?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err("Unauthorized (401). Please check your API key.".into());
        }
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse models response: {e}"))?;

        let mut models = Vec::new();
        // OpenAI-compatible shape: { "data": [{"id": "model-name"}, ...] }.
        if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
            for model in data {
                if let Some(id) = model.get("id").and_then(|i| i.as_str()) {
                    models.push(id.to_string());
                }
            }
        }
        // Some compatible servers instead return Ollama's `models` shape.
        if models.is_empty()
            && let Some(models_arr) = body.get("models").and_then(|m| m.as_array())
        {
            for model in models_arr {
                if let Some(name) = model.get("name").and_then(|n| n.as_str()) {
                    models.push(name.to_string());
                }
            }
        }
        if models.is_empty() {
            return Err(
                "No models returned from custom endpoint. You can type the model name manually."
                    .into(),
            );
        }
        return Ok(models);
    }

    // The genai client owns discovery for provider-default endpoints.
    let client = api_key.map_or_else(Client::default, |key| {
        let key = key.to_string();
        Client::builder()
            .with_auth_resolver_fn(move |_| Ok(Some(AuthData::Key(key.clone()))))
            .build()
    });

    match client.all_model_names(adapter_kind).await {
        Ok(models) if !models.is_empty() => Ok(models),
        Ok(_) => Err("No models returned. Check your API key.".into()),
        Err(e) => Err(format!("Failed to fetch models: {e}")),
    }
}

/// Fetch model names when adapter selection changes in the browser.
#[cfg(target_arch = "wasm32")]
fn fetch_models_system(
    mut ai_config: ResMut<AiConfig>,
    mut available: ResMut<AvailableModels>,
    mut redraw: EventWriter<bevy::window::RequestRedraw>,
) {
    if let Some(ref rx_mutex) = available.receiver {
        redraw.send(bevy::window::RequestRedraw);
        let rx = rx_mutex.lock().unwrap();
        if let Ok(result) = rx.try_recv() {
            drop(rx);
            available.loading = false;
            available.receiver = None;
            match result {
                Ok(models) => {
                    available.error = None;
                    if let Some(pending) = available.pending_model.take() {
                        if models.contains(&pending) {
                            ai_config.model_name = pending;
                            available.needs_configuration = false;
                        } else {
                            available.needs_configuration = !pending.is_empty();
                        }
                    } else {
                        available.needs_configuration = !ai_config.model_name.is_empty()
                            && !models.contains(&ai_config.model_name);
                    }
                    available.models = models;
                }
                Err(e) => {
                    eprintln!("[SynapsCAD] Failed to fetch models: {e}");
                    available.models.clear();
                    available.error = Some(e);
                }
            }
            return;
        }
    }

    let current_key = ai_config.api_key().to_string();
    let key_changed = available.last_api_key != current_key;
    let current_url = ai_config.custom_url().to_string();
    let url_changed = available.last_custom_url != current_url;
    let force_reload = available.force_reload;
    available.force_reload = false;
    if (force_reload
        || available.last_adapter != ai_config.adapter_name
        || key_changed
        || url_changed)
        && !available.loading
    {
        available.models.clear();
        if !ai_config.model_name.is_empty() {
            available.pending_model = Some(ai_config.model_name.clone());
        }
        ai_config.model_name.clear();
        available.last_adapter.clone_from(&ai_config.adapter_name);
        available.last_api_key.clone_from(&current_key);
        available.last_custom_url.clone_from(&current_url);
        available.loading = true;

        let adapter_name = ai_config.adapter_name.clone();
        let api_key = if current_key.is_empty() {
            None
        } else {
            Some(current_key)
        };
        let custom_url = current_url;
        let (tx, rx) = mpsc::channel();
        available.receiver = Some(Mutex::new(rx));

        wasm_bindgen_futures::spawn_local(async move {
            let result = fetch_model_names(&adapter_name, api_key.as_deref(), &custom_url).await;
            let _ = tx.send(result);
        });
    }
}

#[cfg(target_arch = "wasm32")]
async fn fetch_model_names(
    adapter_name: &str,
    api_key: Option<&str>,
    custom_url: &str,
) -> Result<Vec<String>, String> {
    let base = if custom_url.is_empty() {
        default_placeholder_url(adapter_name)
    } else {
        custom_url
    };

    match adapter_name {
        "Ollama" => fetch_ollama_models(base, api_key).await,
        "Anthropic" if custom_url.is_empty() => fetch_anthropic_models(base, api_key).await,
        "Gemini" if custom_url.is_empty() => fetch_gemini_models(base, api_key).await,
        "Cohere" if custom_url.is_empty() => fetch_cohere_models(base, api_key).await,
        _ => fetch_openai_compatible_models(base, api_key).await,
    }
}

#[cfg(target_arch = "wasm32")]
async fn fetch_json(
    request: reqwest::RequestBuilder,
    label: &str,
) -> Result<serde_json::Value, String> {
    let response = request
        .send()
        .await
        .map_err(|e| format!("{label} request failed. Browser CORS policy may require a proxy or CORS-enabled custom endpoint. Details: {e}"))?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Unauthorized (401). Please check your API key.".into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "{label} request failed with HTTP status {}.",
            response.status()
        ));
    }

    response
        .json()
        .await
        .map_err(|e| format!("Failed to parse {label} response: {e}"))
}

#[cfg(target_arch = "wasm32")]
fn with_bearer_auth(
    mut request: reqwest::RequestBuilder,
    api_key: Option<&str>,
) -> reqwest::RequestBuilder {
    if let Some(key) = api_key
        && !key.is_empty()
    {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    request
}

#[cfg(target_arch = "wasm32")]
fn parse_model_names(body: &serde_json::Value) -> Vec<String> {
    let mut models = Vec::new();

    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        for model in data {
            if let Some(id) = model.get("id").and_then(|i| i.as_str()) {
                models.push(id.to_string());
            }
        }
    }

    if let Some(models_arr) = body.get("models").and_then(|m| m.as_array()) {
        for model in models_arr {
            if let Some(name) = model
                .get("name")
                .or_else(|| model.get("id"))
                .and_then(|n| n.as_str())
            {
                let name = name.strip_prefix("models/").unwrap_or(name);
                models.push(name.to_string());
            }
        }
    }

    models.sort();
    models.dedup();
    models
}

#[cfg(target_arch = "wasm32")]
async fn fetch_openai_compatible_models(
    base: &str,
    api_key: Option<&str>,
) -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();
    let request = with_bearer_auth(client.get(format!("{base}models")), api_key);
    let body = fetch_json(request, "model list").await?;
    let models = parse_model_names(&body);
    if models.is_empty() {
        Err("No models returned. You can type the model name manually.".into())
    } else {
        Ok(models)
    }
}

#[cfg(target_arch = "wasm32")]
async fn fetch_ollama_models(base: &str, api_key: Option<&str>) -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();
    let request = with_bearer_auth(client.get(format!("{base}api/tags")), api_key);
    let body = fetch_json(request, "Ollama model list").await?;
    Ok(parse_model_names(&body))
}

#[cfg(target_arch = "wasm32")]
async fn fetch_anthropic_models(base: &str, api_key: Option<&str>) -> Result<Vec<String>, String> {
    let Some(key) = api_key.filter(|key| !key.is_empty()) else {
        return Err("Enter an Anthropic API key or type the model name manually.".into());
    };
    let client = reqwest::Client::new();
    let request = client
        .get(format!("{base}models"))
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01");
    let body = fetch_json(request, "Anthropic model list").await?;
    let models = parse_model_names(&body);
    if models.is_empty() {
        Err("No Anthropic models returned. You can type the model name manually.".into())
    } else {
        Ok(models)
    }
}

#[cfg(target_arch = "wasm32")]
async fn fetch_gemini_models(base: &str, api_key: Option<&str>) -> Result<Vec<String>, String> {
    let Some(key) = api_key.filter(|key| !key.is_empty()) else {
        return Err("Enter a Gemini API key or type the model name manually.".into());
    };
    let client = reqwest::Client::new();
    let body = fetch_json(
        client.get(format!("{base}models?key={key}")),
        "Gemini model list",
    )
    .await?;
    let mut models = Vec::new();
    if let Some(models_arr) = body.get("models").and_then(|m| m.as_array()) {
        for model in models_arr {
            let supports_generate_content = model
                .get("supportedGenerationMethods")
                .and_then(|m| m.as_array())
                .is_none_or(|methods| {
                    methods
                        .iter()
                        .any(|method| method.as_str() == Some("generateContent"))
                });
            if supports_generate_content
                && let Some(name) = model.get("name").and_then(|n| n.as_str())
            {
                models.push(name.strip_prefix("models/").unwrap_or(name).to_string());
            }
        }
    }
    if models.is_empty() {
        Err("No Gemini models returned. You can type the model name manually.".into())
    } else {
        Ok(models)
    }
}

#[cfg(target_arch = "wasm32")]
async fn fetch_cohere_models(base: &str, api_key: Option<&str>) -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();
    let request = with_bearer_auth(client.get(format!("{base}models")), api_key);
    let body = fetch_json(request, "Cohere model list").await?;
    let models = parse_model_names(&body);
    if models.is_empty() {
        Err("No Cohere models returned. You can type the model name manually.".into())
    } else {
        Ok(models)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn ai_send_system(
    mut chat_state: ResMut<ChatState>,
    runtime: Res<TokioRuntime>,
    scad_code: Res<ScadCode>,
    ai_config: Res<AiConfig>,
    model_views: Res<super::compilation::ModelViews>,
    part_query: Query<&PartLabel>,
) {
    if !chat_state.is_streaming || chat_state.stream_receiver.is_some() {
        return;
    }

    let messages: Vec<ChatMessage> = chat_state.messages[chat_state.session_start..].to_vec();
    let part_context = build_part_context(&part_query);

    let current_code = scad_code.text.clone();
    let (active_view_name, _) = super::code_editor::detect_views(&current_code);
    let adapter_name = ai_config.adapter_name.clone();
    let model_name = ai_config.model_name.clone();
    let current_key = ai_config.api_key().to_string();
    let api_key = if current_key.is_empty() {
        None
    } else {
        Some(current_key)
    };
    let custom_url = ai_config.custom_url().to_string();
    let system_prompt = ai_config.system_prompt.clone();
    let temperature = ai_config.temperature;
    let extended_thinking = ai_config.extended_thinking;
    let (views, other_views) = model_views_for_request(
        &model_views,
        &current_code,
        scad_code.fn_value,
        ai_config.send_images,
    );
    let views = views.to_vec();
    let other_views = other_views.to_vec();

    let (tx, rx) = mpsc::channel();
    chat_state.stream_receiver = Some(Mutex::new(rx));

    if cfg!(debug_assertions) {
        eprintln!("[DEBUG] === AI Chat Request ===");
        eprintln!("[DEBUG] Provider: {}", ai_config.adapter_name);
        eprintln!("[DEBUG] Model: {model_name}");
        if !custom_url.is_empty() {
            eprintln!("[DEBUG] Custom URL: {custom_url}");
        }
        eprintln!("[DEBUG] Temperature: {temperature}");
        eprintln!("[DEBUG] Extended thinking: {extended_thinking}");
        eprintln!("[DEBUG] System prompt: {} chars", system_prompt.len());
        eprintln!("[DEBUG] Messages: {}", messages.len());
        eprintln!("[DEBUG] Views: {}", views.len());
    }

    runtime.0.spawn(async move {
        let result = run_ai_stream(
            messages,
            current_code,
            active_view_name,
            &adapter_name,
            &model_name,
            api_key.as_deref(),
            &custom_url,
            &system_prompt,
            temperature,
            extended_thinking,
            &views,
            &other_views,
            part_context,
            tx.clone(),
        )
        .await;
        if let Err(e) = result {
            if cfg!(debug_assertions) {
                eprintln!("[DEBUG] AI error: {e}");
            }
            let _ = tx.send(AiStreamChunk::Error(format!("AI error: {e}")));
        }
    });
}

#[allow(clippy::too_many_arguments, clippy::cognitive_complexity)]
#[cfg(not(target_arch = "wasm32"))]
async fn run_ai_stream(
    messages: Vec<ChatMessage>,
    current_code: String,
    active_view_name: Option<String>,
    adapter_name: &str,
    model_name: &str,
    api_key: Option<&str>,
    custom_url: &str,
    base_system_prompt: &str,
    temperature: f64,
    extended_thinking: bool,
    views: &[(String, String)],
    other_views: &[(String, Vec<(String, String)>)],
    part_context: String,
    tx: mpsc::Sender<AiStreamChunk>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use bevy::tasks::futures_lite::StreamExt;
    use genai::adapter::AdapterKind;
    use genai::chat::{
        ChatMessage as GenaiMessage, ChatOptions, ChatRequest, ChatStreamEvent, ContentPart,
        MessageContent, ReasoningEffort,
    };
    use genai::resolver::{AuthData, Endpoint};
    use genai::{Client, ServiceTarget};

    // Override model-name inference with the selected provider so genai uses
    // the corresponding request format.
    let user_adapter_kind = match adapter_name {
        "OpenAI" => Some(AdapterKind::OpenAI),
        "Anthropic" => Some(AdapterKind::Anthropic),
        "Gemini" => Some(AdapterKind::Gemini),
        "Groq" => Some(AdapterKind::Groq),
        "Ollama" => Some(AdapterKind::Ollama),
        "DeepSeek" => Some(AdapterKind::DeepSeek),
        "Cohere" => Some(AdapterKind::Cohere),
        "Fireworks" => Some(AdapterKind::Fireworks),
        "Together" => Some(AdapterKind::Together),
        "Xai" => Some(AdapterKind::Xai),
        "Zai" => Some(AdapterKind::Zai),
        _ => None,
    };

    let has_custom_url = !custom_url.is_empty();
    let needs_resolver = has_custom_url || user_adapter_kind.is_some();
    let client = if api_key.is_some() || needs_resolver {
        let key = api_key.unwrap_or("").to_string();
        let custom_url_clone = custom_url.to_string();
        let mut builder = Client::builder();
        if !key.is_empty() {
            builder = builder.with_auth_resolver_fn(move |_| Ok(Some(AuthData::Key(key.clone()))));
        }
        if needs_resolver {
            builder = builder.with_service_target_resolver_fn(
                move |mut service_target: ServiceTarget| {
                    if let Some(adapter_kind) = user_adapter_kind {
                        service_target.model.adapter_kind = adapter_kind;
                    }
                    if !custom_url_clone.is_empty() {
                        service_target.endpoint = Endpoint::from_owned(custom_url_clone.clone());
                    }
                    Ok(service_target)
                },
            );
        }
        builder.build()
    } else {
        Client::default()
    };

    let mut system_prompt =
        format!("{base_system_prompt}\n\nCurrent OpenSCAD code:\n```\n{current_code}\n```\n");
    if !part_context.is_empty() {
        system_prompt.push('\n');
        system_prompt.push_str(&part_context);
    }

    if cfg!(debug_assertions) {
        eprintln!(
            "[DEBUG] --- Full system prompt ({} chars) ---",
            system_prompt.len()
        );
        eprintln!("{system_prompt}");
        eprintln!("[DEBUG] --- Chat messages ({} total) ---", messages.len());
        for (i, msg) in messages.iter().enumerate() {
            let preview: String = msg.content.chars().take(200).collect();
            let error_tag = if msg.is_error { " [ERROR-UI-ONLY]" } else { "" };
            eprintln!(
                "[DEBUG]   [{i}] {} (auto={}){error_tag}: {preview}",
                msg.role, msg.auto_generated
            );
        }
        eprintln!("[DEBUG] Views: {}", views.len());
        eprintln!("[DEBUG] ---");
    }

    let mut chat_req = ChatRequest::default().with_system(system_prompt);

    for msg in &messages {
        // UI error messages are not conversation turns.
        if msg.is_error {
            continue;
        }
        match msg.role.as_str() {
            "user" => {
                if msg.images.is_empty() {
                    chat_req = chat_req.append_message(GenaiMessage::user(&msg.content));
                } else {
                    let mut parts = vec![ContentPart::from_text(&msg.content)];
                    for img in &msg.images {
                        parts.push(ContentPart::from_text(format!("{}:", img.filename)));
                        parts.push(ContentPart::from_binary_base64(
                            &img.mime_type,
                            img.base64_data.as_str(),
                            Some(img.filename.clone()),
                        ));
                    }
                    chat_req = chat_req
                        .append_message(GenaiMessage::user(MessageContent::from_parts(parts)));
                }
            }
            "assistant" => {
                chat_req = chat_req.append_message(GenaiMessage::assistant(&msg.content));
            }
            _ => {}
        }
    }

    // Attach the active model views after the conversational history.
    if !views.is_empty() {
        let view_intro = active_view_name.as_ref().map_or_else(
            || "Current 3D model (active view) rendered from five orthographic/isometric views:".to_string(),
            |name| format!("The user is CURRENTLY SEEING the following $view \"{name}\" in their viewport. Here are five orthographic/isometric views of it:"),
        );
        let mut parts = vec![ContentPart::from_text(view_intro)];
        for (label, base64_png) in views {
            if !base64_png.is_empty() {
                let descriptive_label = match label.as_str() {
                    "Front" => "Front view (Looking from +Y towards origin)",
                    "Right" => "Right view (Looking from +X towards origin)",
                    "Top" => "Top view (Looking from +Z towards origin)",
                    "Bottom" => "Bottom view (Looking from -Z towards origin)",
                    "Iso" => "Isometric view (3/4 perspective)",
                    _ => label,
                };
                parts.push(ContentPart::from_text(format!("{descriptive_label}:")));
                parts.push(ContentPart::from_binary_base64(
                    "image/png",
                    base64_png.as_str(),
                    Some(format!("{label}_view.png")),
                ));
            }
        }
        let view_msg = GenaiMessage::user(MessageContent::from_parts(parts));
        chat_req = chat_req.append_message(view_msg);

        // Debug builds retain attached views under `var/tmp` for inspection.
        if cfg!(debug_assertions) {
            use base64::Engine;
            let tmp_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("var/tmp");
            if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
                eprintln!("[DEBUG] Failed to create {}: {e}", tmp_dir.display());
            }
            for (label, base64_png) in views {
                if !base64_png.is_empty() {
                    match base64::engine::general_purpose::STANDARD.decode(base64_png) {
                        Ok(bytes) => {
                            let path = tmp_dir.join(format!("{label}_view.png"));
                            match std::fs::write(&path, &bytes) {
                                Ok(()) => eprintln!("[DEBUG] Saved view image: {}", path.display()),
                                Err(e) => {
                                    eprintln!("[DEBUG] Failed to write {}: {e}", path.display());
                                }
                            }
                        }
                        Err(e) => eprintln!("[DEBUG] Failed to decode {label} base64: {e}"),
                    }
                }
            }
        }
    }

    // Attach lower-resolution previews of inactive `$view` branches.
    if !other_views.is_empty() {
        for (view_name, view_images) in other_views {
            let mut parts = vec![ContentPart::from_text(format!(
                "Rendered views for $view \"{view_name}\":"
            ))];
            for (label, base64_png) in view_images {
                if !base64_png.is_empty() {
                    parts.push(ContentPart::from_text(format!("{label}:")));
                    parts.push(ContentPart::from_binary_base64(
                        "image/png",
                        base64_png.as_str(),
                        Some(format!("{view_name}_{label}_view.png")),
                    ));
                }
            }
            let view_msg = GenaiMessage::user(MessageContent::from_parts(parts));
            chat_req = chat_req.append_message(view_msg);
        }
    }

    // Providers that reject assistant-final histories receive a minimal user
    // continuation after filtering UI-only errors.
    let last_non_error_msg = messages.iter().rev().find(|m| !m.is_error);
    let ends_with_user = !views.is_empty()
        || !other_views.is_empty()
        || last_non_error_msg.is_some_and(|m| m.role == "user");
    if !ends_with_user {
        chat_req = chat_req.append_message(GenaiMessage::user(
            "Please respond to the conversation above.",
        ));
    }

    // Claude's API requires temperature=1 when extended thinking is enabled.
    let is_claude = model_name.contains("claude");
    let effective_temperature = if extended_thinking && is_claude {
        1.0
    } else {
        temperature
    };

    let mut chat_options = ChatOptions::default()
        .with_temperature(effective_temperature)
        .with_capture_content(true)
        .with_capture_reasoning_content(true);

    if extended_thinking {
        chat_options = chat_options.with_reasoning_effort(ReasoningEffort::High);
    }

    // genai 0.6.0-beta.3 does not forward Ollama authentication, so inject the
    // selected bearer token explicitly.
    if user_adapter_kind == Some(AdapterKind::Ollama)
        && let Some(key) = api_key
        && !key.is_empty()
    {
        let headers = genai::Headers::from(("Authorization", format!("Bearer {key}")));
        chat_options.extra_headers = Some(headers);
    }

    let stream_response = match client
        .exec_chat_stream(model_name, chat_req, Some(&chat_options))
        .await
    {
        Ok(response) => response,
        Err(e) => {
            let err_msg = format!("API request failed: {e}");
            if cfg!(debug_assertions) {
                eprintln!("[DEBUG] Stream init error: {err_msg}");
            }
            let _ = tx.send(AiStreamChunk::Error(err_msg));
            return Ok(());
        }
    };

    let mut stream = std::pin::pin!(stream_response.stream);
    let mut full_content = String::new();
    let mut full_reasoning: Option<String> = None;

    while let Some(event) = stream.next().await {
        match event {
            Ok(ChatStreamEvent::Chunk(chunk)) => {
                full_content.push_str(&chunk.content);
                let _ = tx.send(AiStreamChunk::Chunk(chunk.content));
            }
            Ok(ChatStreamEvent::ReasoningChunk(chunk)) => {
                full_reasoning
                    .get_or_insert_with(String::new)
                    .push_str(&chunk.content);
                let _ = tx.send(AiStreamChunk::ReasoningChunk(chunk.content));
            }
            Ok(ChatStreamEvent::End(_)) => {
                break;
            }
            Ok(_) => {} // Start, ThoughtSignatureChunk, ToolCallChunk
            Err(e) => {
                let err_msg = format!("Stream error: {e}");
                if cfg!(debug_assertions) {
                    eprintln!("[DEBUG] {err_msg}");
                }
                let _ = tx.send(AiStreamChunk::Error(err_msg));
                return Ok(());
            }
        }
    }

    // Some servers represent failures such as context exhaustion as an empty
    // successful response.
    if full_content.is_empty() {
        let warning = "The AI returned an empty response. This may indicate:\n\
            • The request exceeded the model's context limit\n\
            • An error on the inference server\n\
            • Network or timeout issues\n\n\
            Check your inference server's logs for details.";
        let _ = tx.send(AiStreamChunk::Error(warning.to_string()));
        return Ok(());
    }

    if cfg!(debug_assertions) {
        let preview: String = full_content.chars().take(500).collect();
        eprintln!(
            "[DEBUG] AI response ({} chars): {preview}",
            full_content.len()
        );
        if let Some(ref r) = full_reasoning {
            eprintln!("[DEBUG] AI reasoning ({} chars)", r.len());
        }
    }

    let _ = tx.send(AiStreamChunk::Done {
        content: full_content,
        reasoning: full_reasoning,
    });

    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn ai_send_system(
    mut chat_state: ResMut<ChatState>,
    scad_code: Res<ScadCode>,
    ai_config: Res<AiConfig>,
    model_views: Res<super::compilation::ModelViews>,
    part_query: Query<&PartLabel>,
) {
    if !chat_state.is_streaming || chat_state.stream_receiver.is_some() {
        return;
    }

    let messages: Vec<ChatMessage> = chat_state.messages[chat_state.session_start..].to_vec();
    let part_context = build_part_context(&part_query);

    let current_code = scad_code.text.clone();
    let (active_view_name, _) = super::code_editor::detect_views(&current_code);
    let adapter_name = ai_config.adapter_name.clone();
    let model_name = ai_config.model_name.clone();
    let current_key = ai_config.api_key().to_string();
    let api_key = if current_key.is_empty() {
        None
    } else {
        Some(current_key)
    };
    let custom_url = ai_config.custom_url().to_string();
    let system_prompt = ai_config.system_prompt.clone();
    let temperature = ai_config.temperature;
    let extended_thinking = ai_config.extended_thinking;
    let (views, other_views) = model_views_for_request(
        &model_views,
        &current_code,
        scad_code.fn_value,
        ai_config.send_images,
    );
    let views = views.to_vec();
    let other_views = other_views.to_vec();

    let (tx, rx) = mpsc::channel();
    chat_state.stream_receiver = Some(Mutex::new(rx));

    if cfg!(debug_assertions) {
        eprintln!("[DEBUG] === Browser AI Chat Request ===");
        eprintln!("[DEBUG] Provider: {}", ai_config.adapter_name);
        eprintln!("[DEBUG] Model: {model_name}");
        if !custom_url.is_empty() {
            eprintln!("[DEBUG] Custom URL: {custom_url}");
        }
        eprintln!("[DEBUG] Temperature: {temperature}");
        eprintln!("[DEBUG] Extended thinking: {extended_thinking}");
        eprintln!("[DEBUG] System prompt: {} chars", system_prompt.len());
        eprintln!("[DEBUG] Messages: {}", messages.len());
        eprintln!("[DEBUG] Views: {}", views.len());
    }

    wasm_bindgen_futures::spawn_local(async move {
        match run_ai_request_wasm(
            messages,
            current_code,
            active_view_name,
            &adapter_name,
            &model_name,
            api_key.as_deref(),
            &custom_url,
            &system_prompt,
            temperature,
            extended_thinking,
            &views,
            &other_views,
            part_context,
        )
        .await
        {
            Ok((content, reasoning)) => {
                let _ = tx.send(AiStreamChunk::Done { content, reasoning });
            }
            Err(e) => {
                if cfg!(debug_assertions) {
                    eprintln!("[DEBUG] AI error: {e}");
                }
                let _ = tx.send(AiStreamChunk::Error(format!("AI error: {e}")));
            }
        }
    });
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug)]
enum BrowserPart {
    Text(String),
    Image {
        mime_type: String,
        base64_data: String,
    },
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug)]
struct BrowserMessage {
    role: String,
    parts: Vec<BrowserPart>,
}

#[allow(clippy::too_many_arguments, clippy::cognitive_complexity)]
#[cfg(target_arch = "wasm32")]
async fn run_ai_request_wasm(
    messages: Vec<ChatMessage>,
    current_code: String,
    active_view_name: Option<String>,
    adapter_name: &str,
    model_name: &str,
    api_key: Option<&str>,
    custom_url: &str,
    base_system_prompt: &str,
    temperature: f64,
    extended_thinking: bool,
    views: &[(String, String)],
    other_views: &[(String, Vec<(String, String)>)],
    part_context: String,
) -> Result<(String, Option<String>), String> {
    let mut system_prompt =
        format!("{base_system_prompt}\n\nCurrent OpenSCAD code:\n```\n{current_code}\n```\n");
    if !part_context.is_empty() {
        system_prompt.push('\n');
        system_prompt.push_str(&part_context);
    }

    let request_messages = build_browser_messages(messages, active_view_name, views, other_views);

    let effective_temperature = if extended_thinking && model_name.contains("claude") {
        1.0
    } else {
        temperature
    };

    let base = if custom_url.is_empty() {
        default_placeholder_url(adapter_name)
    } else {
        custom_url
    };

    let result = match adapter_name {
        "Anthropic" => {
            request_anthropic(
                base,
                model_name,
                api_key,
                &system_prompt,
                &request_messages,
                effective_temperature,
            )
            .await
        }
        "Gemini" => {
            request_gemini(
                base,
                model_name,
                api_key,
                &system_prompt,
                &request_messages,
                effective_temperature,
            )
            .await
        }
        "Ollama" => {
            request_ollama(
                base,
                model_name,
                api_key,
                &system_prompt,
                &request_messages,
                effective_temperature,
            )
            .await
        }
        "Cohere" if custom_url.is_empty() => {
            request_cohere(
                base,
                model_name,
                api_key,
                &system_prompt,
                &request_messages,
                effective_temperature,
            )
            .await
        }
        _ => {
            request_openai_compatible(
                base,
                model_name,
                api_key,
                &system_prompt,
                &request_messages,
                effective_temperature,
            )
            .await
        }
    }?;

    if result.0.trim().is_empty() {
        Err("The AI returned an empty response.".into())
    } else {
        Ok(result)
    }
}

#[cfg(target_arch = "wasm32")]
fn build_browser_messages(
    messages: Vec<ChatMessage>,
    active_view_name: Option<String>,
    views: &[(String, String)],
    other_views: &[(String, Vec<(String, String)>)],
) -> Vec<BrowserMessage> {
    let mut request_messages = Vec::new();

    for msg in messages {
        if msg.is_error {
            continue;
        }
        match msg.role.as_str() {
            "user" => {
                let mut parts = vec![BrowserPart::Text(msg.content)];
                for img in msg.images {
                    parts.push(BrowserPart::Text(format!("{}:", img.filename)));
                    parts.push(BrowserPart::Image {
                        mime_type: img.mime_type,
                        base64_data: img.base64_data,
                    });
                }
                request_messages.push(BrowserMessage {
                    role: "user".into(),
                    parts,
                });
            }
            "assistant" => request_messages.push(BrowserMessage {
                role: "assistant".into(),
                parts: vec![BrowserPart::Text(msg.content)],
            }),
            _ => {}
        }
    }

    if !views.is_empty() {
        let view_intro = active_view_name.as_ref().map_or_else(
            || "Current 3D model (active view) rendered from five orthographic/isometric views:"
                .to_string(),
            |name| {
                format!(
                    "The user is CURRENTLY SEEING the following $view \"{name}\" in their viewport. Here are five orthographic/isometric views of it:"
                )
            },
        );
        let mut parts = vec![BrowserPart::Text(view_intro)];
        for (label, base64_png) in views {
            if !base64_png.is_empty() {
                let descriptive_label = match label.as_str() {
                    "Front" => "Front view (Looking from +Y towards origin)",
                    "Right" => "Right view (Looking from +X towards origin)",
                    "Top" => "Top view (Looking from +Z towards origin)",
                    "Bottom" => "Bottom view (Looking from -Z towards origin)",
                    "Iso" => "Isometric view (3/4 perspective)",
                    _ => label,
                };
                parts.push(BrowserPart::Text(format!("{descriptive_label}:")));
                parts.push(BrowserPart::Image {
                    mime_type: "image/png".into(),
                    base64_data: base64_png.clone(),
                });
            }
        }
        request_messages.push(BrowserMessage {
            role: "user".into(),
            parts,
        });
    }

    for (view_name, view_images) in other_views {
        let mut parts = vec![BrowserPart::Text(format!(
            "Rendered views for $view \"{view_name}\":"
        ))];
        for (label, base64_png) in view_images {
            if !base64_png.is_empty() {
                parts.push(BrowserPart::Text(format!("{label}:")));
                parts.push(BrowserPart::Image {
                    mime_type: "image/png".into(),
                    base64_data: base64_png.clone(),
                });
            }
        }
        request_messages.push(BrowserMessage {
            role: "user".into(),
            parts,
        });
    }

    let ends_with_user = request_messages
        .last()
        .is_some_and(|message| message.role == "user");
    if !ends_with_user {
        request_messages.push(BrowserMessage {
            role: "user".into(),
            parts: vec![BrowserPart::Text(
                "Please respond to the conversation above.".into(),
            )],
        });
    }

    request_messages
}

#[cfg(target_arch = "wasm32")]
async fn post_json(
    request: reqwest::RequestBuilder,
    body: serde_json::Value,
    label: &str,
) -> Result<serde_json::Value, String> {
    fetch_json(request.json(&body), label).await
}

#[cfg(target_arch = "wasm32")]
fn text_from_parts(parts: &[BrowserPart]) -> String {
    let mut text = String::new();
    for part in parts {
        if let BrowserPart::Text(part_text) = part {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(part_text);
        }
    }
    text
}

#[cfg(target_arch = "wasm32")]
fn openai_content_parts(parts: &[BrowserPart]) -> serde_json::Value {
    use serde_json::json;

    if parts
        .iter()
        .all(|part| matches!(part, BrowserPart::Text(_)))
    {
        return serde_json::Value::String(text_from_parts(parts));
    }

    serde_json::Value::Array(
        parts
            .iter()
            .map(|part| match part {
                BrowserPart::Text(text) => json!({ "type": "text", "text": text }),
                BrowserPart::Image {
                    mime_type,
                    base64_data,
                } => json!({
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:{mime_type};base64,{base64_data}")
                    }
                }),
            })
            .collect(),
    )
}

#[cfg(target_arch = "wasm32")]
async fn request_openai_compatible(
    base: &str,
    model_name: &str,
    api_key: Option<&str>,
    system_prompt: &str,
    messages: &[BrowserMessage],
    temperature: f64,
) -> Result<(String, Option<String>), String> {
    use serde_json::json;

    let mut request_messages = vec![json!({
        "role": "system",
        "content": system_prompt,
    })];
    for message in messages {
        request_messages.push(json!({
            "role": if message.role == "assistant" { "assistant" } else { "user" },
            "content": openai_content_parts(&message.parts),
        }));
    }

    let client = reqwest::Client::new();
    let request = with_bearer_auth(client.post(format!("{base}chat/completions")), api_key);
    let body = post_json(
        request,
        json!({
            "model": model_name,
            "messages": request_messages,
            "temperature": temperature,
            "stream": false,
        }),
        "OpenAI-compatible chat",
    )
    .await?;

    let content = body
        .get("choices")
        .and_then(|choices| choices.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| {
            choice
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(|content| content.as_str())
                .or_else(|| choice.get("text").and_then(|text| text.as_str()))
        })
        .unwrap_or_default()
        .to_string();

    Ok((content, None))
}

#[cfg(target_arch = "wasm32")]
fn anthropic_content_parts(parts: &[BrowserPart]) -> Vec<serde_json::Value> {
    use serde_json::json;

    parts
        .iter()
        .map(|part| match part {
            BrowserPart::Text(text) => json!({ "type": "text", "text": text }),
            BrowserPart::Image {
                mime_type,
                base64_data,
            } => json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": mime_type,
                    "data": base64_data,
                }
            }),
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
async fn request_anthropic(
    base: &str,
    model_name: &str,
    api_key: Option<&str>,
    system_prompt: &str,
    messages: &[BrowserMessage],
    temperature: f64,
) -> Result<(String, Option<String>), String> {
    use serde_json::json;

    let Some(key) = api_key.filter(|key| !key.is_empty()) else {
        return Err("Enter an Anthropic API key before sending.".into());
    };

    let request_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|message| {
            json!({
                "role": if message.role == "assistant" { "assistant" } else { "user" },
                "content": anthropic_content_parts(&message.parts),
            })
        })
        .collect();

    let client = reqwest::Client::new();
    let request = client
        .post(format!("{base}messages"))
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01");
    let body = post_json(
        request,
        json!({
            "model": model_name,
            "max_tokens": 4096,
            "temperature": temperature,
            "system": system_prompt,
            "messages": request_messages,
        }),
        "Anthropic chat",
    )
    .await?;

    let mut content = String::new();
    let mut reasoning = String::new();
    if let Some(parts) = body.get("content").and_then(|content| content.as_array()) {
        for part in parts {
            match part.get("type").and_then(|value| value.as_str()) {
                Some("text") => {
                    if let Some(text) = part.get("text").and_then(|text| text.as_str()) {
                        content.push_str(text);
                    }
                }
                Some("thinking") => {
                    if let Some(text) = part.get("thinking").and_then(|text| text.as_str()) {
                        reasoning.push_str(text);
                    }
                }
                _ => {}
            }
        }
    }

    let reasoning = if reasoning.is_empty() {
        None
    } else {
        Some(reasoning)
    };
    Ok((content, reasoning))
}

#[cfg(target_arch = "wasm32")]
fn gemini_parts(parts: &[BrowserPart]) -> Vec<serde_json::Value> {
    use serde_json::json;

    parts
        .iter()
        .map(|part| match part {
            BrowserPart::Text(text) => json!({ "text": text }),
            BrowserPart::Image {
                mime_type,
                base64_data,
            } => json!({
                "inline_data": {
                    "mime_type": mime_type,
                    "data": base64_data,
                }
            }),
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
async fn request_gemini(
    base: &str,
    model_name: &str,
    api_key: Option<&str>,
    system_prompt: &str,
    messages: &[BrowserMessage],
    temperature: f64,
) -> Result<(String, Option<String>), String> {
    use serde_json::json;

    let model_name = model_name.strip_prefix("models/").unwrap_or(model_name);
    let mut url = format!("{base}models/{model_name}:generateContent");
    if let Some(key) = api_key.filter(|key| !key.is_empty()) {
        url.push_str("?key=");
        url.push_str(key);
    }

    let contents: Vec<serde_json::Value> = messages
        .iter()
        .map(|message| {
            json!({
                "role": if message.role == "assistant" { "model" } else { "user" },
                "parts": gemini_parts(&message.parts),
            })
        })
        .collect();

    let client = reqwest::Client::new();
    let body = post_json(
        client.post(url),
        json!({
            "systemInstruction": {
                "parts": [{ "text": system_prompt }],
            },
            "contents": contents,
            "generationConfig": {
                "temperature": temperature,
            },
        }),
        "Gemini chat",
    )
    .await?;

    let mut content = String::new();
    if let Some(parts) = body
        .get("candidates")
        .and_then(|candidates| candidates.as_array())
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(|parts| parts.as_array())
    {
        for part in parts {
            if let Some(text) = part.get("text").and_then(|text| text.as_str()) {
                content.push_str(text);
            }
        }
    }

    Ok((content, None))
}

#[cfg(target_arch = "wasm32")]
async fn request_ollama(
    base: &str,
    model_name: &str,
    api_key: Option<&str>,
    system_prompt: &str,
    messages: &[BrowserMessage],
    temperature: f64,
) -> Result<(String, Option<String>), String> {
    use serde_json::json;

    let mut request_messages = vec![json!({
        "role": "system",
        "content": system_prompt,
    })];
    for message in messages {
        let images: Vec<String> = message
            .parts
            .iter()
            .filter_map(|part| match part {
                BrowserPart::Image { base64_data, .. } => Some(base64_data.clone()),
                BrowserPart::Text(_) => None,
            })
            .collect();
        let mut message_json = json!({
            "role": if message.role == "assistant" { "assistant" } else { "user" },
            "content": text_from_parts(&message.parts),
        });
        if !images.is_empty() {
            message_json["images"] = serde_json::Value::Array(
                images.into_iter().map(serde_json::Value::String).collect(),
            );
        }
        request_messages.push(message_json);
    }

    let client = reqwest::Client::new();
    let request = with_bearer_auth(client.post(format!("{base}api/chat")), api_key);
    let body = post_json(
        request,
        json!({
            "model": model_name,
            "messages": request_messages,
            "stream": false,
            "options": {
                "temperature": temperature,
            },
        }),
        "Ollama chat",
    )
    .await?;
    let content = body
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .unwrap_or_default()
        .to_string();
    Ok((content, None))
}

#[cfg(target_arch = "wasm32")]
async fn request_cohere(
    base: &str,
    model_name: &str,
    api_key: Option<&str>,
    system_prompt: &str,
    messages: &[BrowserMessage],
    temperature: f64,
) -> Result<(String, Option<String>), String> {
    use serde_json::json;

    let Some(key) = api_key.filter(|key| !key.is_empty()) else {
        return Err("Enter a Cohere API key before sending.".into());
    };

    let request_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|message| {
            json!({
                "role": if message.role == "assistant" { "assistant" } else { "user" },
                "content": text_from_parts(&message.parts),
            })
        })
        .collect();

    let client = reqwest::Client::new();
    let body = post_json(
        client
            .post(format!("{base}chat"))
            .header("Authorization", format!("Bearer {key}")),
        json!({
            "model": model_name,
            "messages": request_messages,
            "preamble": system_prompt,
            "temperature": temperature,
        }),
        "Cohere chat",
    )
    .await?;

    let content = body
        .get("text")
        .and_then(|text| text.as_str())
        .or_else(|| {
            body.get("message")
                .and_then(|message| message.get("content"))
                .and_then(|content| content.as_array())
                .and_then(|content| content.first())
                .and_then(|content| content.get("text"))
                .and_then(|text| text.as_str())
        })
        .unwrap_or_default()
        .to_string();
    Ok((content, None))
}

fn ai_receive_system(
    mut chat_state: ResMut<ChatState>,
    mut scad_code: ResMut<ScadCode>,
    ai_config: Res<AiConfig>,
    mut redraw: EventWriter<bevy::window::RequestRedraw>,
) {
    if !chat_state.is_streaming {
        return;
    }

    // Streaming channel polling requires redraws while the interface is idle.
    redraw.send(bevy::window::RequestRedraw);

    // Drain the channel so one frame applies all currently available output.
    let chunks: Vec<AiStreamChunk> = {
        let Some(ref rx_mutex) = chat_state.stream_receiver else {
            return;
        };
        let rx = rx_mutex.lock().unwrap();
        let mut chunks = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(c) => chunks.push(c),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if chunks.is_empty() {
                        drop(rx);
                        chat_state.is_streaming = false;
                        chat_state.streaming_start = None;
                        chat_state.stream_receiver = None;
                        return;
                    }
                    break;
                }
            }
        }
        chunks
    };

    for chunk in chunks {
        match chunk {
            AiStreamChunk::Chunk(text) => {
                // Maintain one live assistant message during streaming.
                let append = chat_state
                    .messages
                    .last()
                    .is_some_and(|m| m.role == "assistant" && !m.is_error);
                if append {
                    chat_state
                        .messages
                        .last_mut()
                        .unwrap()
                        .content
                        .push_str(&text);
                } else {
                    chat_state.messages.push(ChatMessage {
                        role: "assistant".into(),
                        content: text,
                        thinking: None,
                        images: Vec::new(),
                        auto_generated: false,
                        is_error: false,
                    });
                }
            }
            AiStreamChunk::ReasoningChunk(text) => {
                let append = chat_state
                    .messages
                    .last()
                    .is_some_and(|m| m.role == "assistant" && !m.is_error);
                if append {
                    chat_state
                        .messages
                        .last_mut()
                        .unwrap()
                        .thinking
                        .get_or_insert_with(String::new)
                        .push_str(&text);
                } else {
                    chat_state.messages.push(ChatMessage {
                        role: "assistant".into(),
                        content: String::new(),
                        thinking: Some(text),
                        images: Vec::new(),
                        auto_generated: false,
                        is_error: false,
                    });
                }
            }
            AiStreamChunk::Done { content, reasoning } => {
                // Replace accumulated chunks with the authoritative final text.
                let replace = chat_state
                    .messages
                    .last()
                    .is_some_and(|m| m.role == "assistant" && !m.is_error);
                if replace {
                    let last = chat_state.messages.last_mut().unwrap();
                    last.content.clone_from(&content);
                    last.thinking = reasoning;
                }
                chat_state.is_streaming = false;
                chat_state.streaming_start = None;
                chat_state.stream_receiver = None;

                let code_changed = match extract_code_change(&content) {
                    Some(CodeChange::FullReplace(new_code)) => {
                        scad_code.text = new_code;
                        true
                    }
                    Some(CodeChange::SearchReplace(replacements)) => {
                        match apply_search_replace(&scad_code.text, &replacements) {
                            Ok(new_code) => {
                                scad_code.text = new_code;
                                true
                            }
                            Err(err) => {
                                eprintln!("[SynapsCAD] Search-and-replace failed: {err}");
                                // A complete fenced program remains a valid
                                // fallback when a search block no longer matches.
                                if let Some(full) = extract_openscad_code(&content) {
                                    scad_code.text = full;
                                    true
                                } else {
                                    false
                                }
                            }
                        }
                    }
                    None => false,
                };

                if code_changed {
                    scad_code.dirty = true;

                    let round = match &chat_state.verification {
                        VerificationState::Verifying(n) => *n,
                        _ => 0,
                    };

                    if round < ai_config.max_verification_rounds {
                        chat_state.verification = VerificationState::WaitingForCompilation;
                    } else {
                        chat_state.verification = VerificationState::Idle;
                    }
                } else {
                    chat_state.verification = VerificationState::Idle;
                }
                return;
            }
            AiStreamChunk::Error(err) => {
                // Remove incomplete assistant output before exposing the error.
                if chat_state
                    .messages
                    .last()
                    .is_some_and(|m| m.role == "assistant" && !m.is_error)
                {
                    chat_state.messages.pop();
                }
                // Restore the submitted prompt so it can be retried or edited.
                if let Some(last_user_msg) = chat_state
                    .messages
                    .iter()
                    .rposition(|m| m.role == "user" && !m.auto_generated)
                {
                    let msg = chat_state.messages.remove(last_user_msg);
                    chat_state.input_buffer = msg.content;
                    chat_state.pending_images = msg.images;
                }
                chat_state.messages.push(ChatMessage {
                    role: "assistant".into(),
                    content: err,
                    thinking: None,
                    images: Vec::new(),
                    auto_generated: false,
                    is_error: true,
                });
                chat_state.is_streaming = false;
                chat_state.streaming_start = None;
                chat_state.stream_receiver = None;
                chat_state.verification = VerificationState::Idle;
                return;
            }
        }
    }
}

/// Watches for compilation to finish after AI-produced code, then triggers verification.
fn ai_verify_system(
    mut chat_state: ResMut<ChatState>,
    compilation_state: Res<super::compilation::CompilationState>,
    ai_config: Res<AiConfig>,
) {
    match &chat_state.verification {
        VerificationState::WaitingForCompilation if !compilation_state.is_compiling => {
            chat_state.verification = VerificationState::ReadyToVerify;
        }
        VerificationState::ReadyToVerify => {
            #[allow(clippy::cast_possible_truncation)]
            let round = chat_state
                .messages
                .iter()
                .rev()
                .take_while(|m| m.role != "user" || m.auto_generated)
                .filter(|m| m.role == "user" && m.auto_generated)
                .count() as u32
                + 1;

            let max_label = if ai_config.max_verification_rounds == u32::MAX {
                "∞".to_string()
            } else {
                ai_config.max_verification_rounds.to_string()
            };

            // Add an internal user turn to request visual verification.
            chat_state.messages.push(ChatMessage {
                role: "user".into(),
                content: format!("[Verification round {round}/{max_label}] {VERIFICATION_PROMPT}"),
                thinking: None,
                images: Vec::new(),
                auto_generated: true,
                is_error: false,
            });

            chat_state.is_streaming = true;
            chat_state.streaming_start = Some(web_time::Instant::now());
            chat_state.verification = VerificationState::Verifying(round);
        }
        VerificationState::ErrorRecovery(err) => {
            // Feed compilation failures back into the active verification loop.
            let error_msg = err.clone();

            chat_state.messages.push(ChatMessage {
                role: "user".into(),
                content: format!(
                    "[Error Recovery] The code you provided has a compilation error:\n\n{error_msg}\n\n\
                    Please fix this error and provide corrected code."
                ),
                thinking: None,
                images: Vec::new(),
                auto_generated: true,
                is_error: false,
            });

            chat_state.is_streaming = true;
            chat_state.streaming_start = Some(web_time::Instant::now());
            chat_state.verification = VerificationState::Verifying(0); // Reset round counter
        }
        _ => {}
    }
}

/// Build part context describing the compiled parts (@1, @2, ...) for the AI.
fn build_part_context(part_query: &Query<&PartLabel>) -> String {
    use std::fmt::Write;
    let mut parts: Vec<&PartLabel> = part_query.iter().collect();
    if parts.is_empty() {
        return String::new();
    }
    parts.sort_by_key(|p| p.index);

    let mut ctx = String::from("Compiled parts:\n");
    for part in &parts {
        let [r, g, b] = part.color;
        let _ = writeln!(
            ctx,
            "  {}: color=({:.2}, {:.2}, {:.2})",
            part.label, r, g, b
        );
    }
    ctx.push_str("When the user references @N, it refers to the part listed above.\n");
    ctx
}

/// Result of extracting code from an AI response.
enum CodeChange {
    /// Full replacement — the AI sent a complete `synapscad` code block.
    FullReplace(String),
    /// Search-and-replace pairs — the AI sent `<<<REPLACE` blocks.
    SearchReplace(Vec<(String, String)>),
}

/// Extracts code changes from AI response.
/// First tries `<<<REPLACE` search-and-replace blocks, then falls back to full `synapscad` block.
fn extract_code_change(text: &str) -> Option<CodeChange> {
    let replacements = parse_search_replace(text);
    if !replacements.is_empty() {
        return Some(CodeChange::SearchReplace(replacements));
    }

    extract_openscad_code(text).map(CodeChange::FullReplace)
}

/// Parses `<<<REPLACE` / `===` / `>>>` blocks from AI response.
fn parse_search_replace(text: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let mut remaining = text;

    while let Some(start) = remaining.find("<<<REPLACE") {
        let after_marker = &remaining[start + "<<<REPLACE".len()..];
        let after_newline = if let Some(nl) = after_marker.find('\n') {
            &after_marker[nl + 1..]
        } else {
            break;
        };

        let Some(separator) = after_newline.find("\n===\n") else {
            break;
        };

        let old_str = &after_newline[..separator];

        let after_sep = &after_newline[separator + "\n===\n".len()..];

        let Some(end) = after_sep.find("\n>>>") else {
            break;
        };

        let new_str = &after_sep[..end];

        if !old_str.is_empty() {
            results.push((old_str.to_string(), new_str.to_string()));
        }

        remaining = &after_sep[end + "\n>>>".len()..];
    }

    results
}

/// Applies search-and-replace pairs to the current code buffer.
/// Returns the modified code, or None if any replacement failed to find its target.
fn apply_search_replace(code: &str, replacements: &[(String, String)]) -> Result<String, String> {
    let mut result = code.to_string();
    for (i, (old, new)) in replacements.iter().enumerate() {
        let count = result.matches(old.as_str()).count();
        if count == 0 {
            return Err(format!(
                "Search-and-replace #{}: could not find the target text in the code",
                i + 1
            ));
        }
        if count > 1 {
            return Err(format!(
                "Search-and-replace #{}: target text appears {} times (must be unique)",
                i + 1,
                count
            ));
        }
        result = result.replacen(old.as_str(), new.as_str(), 1);
    }
    Ok(result)
}

/// Extracts `OpenSCAD` code from AI response.
/// Supports ` ```synapscad ` and ` ```openscad ` code blocks (ignores any `:suffix`).
fn extract_openscad_code(text: &str) -> Option<String> {
    let markers = ["```synapscad", "```openscad"];

    for marker in &markers {
        if let Some(start) = text.find(marker) {
            let rest = &text[start + marker.len()..];

            // Ignore optional fence metadata such as `:main`.
            let newline = rest.find('\n').unwrap_or(0);
            let code_rest = &rest[newline..];
            let end = code_rest.find("```")?;
            let code = code_rest[..end].trim().to_string();
            if !code.is_empty() {
                return Some(code);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cached_model_views(code: &str, fn_value: u32) -> super::super::compilation::ModelViews {
        super::super::compilation::ModelViews {
            source_hash: Some(super::super::compilation::model_source_hash(code, fn_value)),
            views: vec![("Iso".into(), "active-image".into())],
            other_views: vec![(
                "assembly".into(),
                vec![("Front".into(), "inactive-image".into())],
            )],
        }
    }

    #[test]
    fn request_uses_current_model_views_when_images_are_enabled() {
        let cached = cached_model_views("cube(10);", 16);

        let (views, other_views) = model_views_for_request(&cached, "cube(10);", 16, true);

        assert_eq!(views.len(), 1);
        assert_eq!(other_views.len(), 1);
    }

    #[test]
    fn request_rejects_stale_model_views() {
        let cached = cached_model_views("cube(10);", 16);

        let (source_changed, _) = model_views_for_request(&cached, "sphere(10);", 16, true);
        let (settings_changed, _) = model_views_for_request(&cached, "cube(10);", 32, true);

        assert!(source_changed.is_empty());
        assert!(settings_changed.is_empty());
    }

    #[test]
    fn request_omits_current_model_views_when_images_are_disabled() {
        let cached = cached_model_views("cube(10);", 16);

        let (views, other_views) = model_views_for_request(&cached, "cube(10);", 16, false);

        assert!(views.is_empty());
        assert!(other_views.is_empty());
    }

    #[test]
    fn images_are_enabled_by_default() {
        assert!(AiConfig::default().send_images);
    }

    #[test]
    fn test_parse_search_replace_single() {
        let text = "Here's the change:\n\n<<<REPLACE\ncube(10);\n===\ncube(20);\n>>>\n\nDone!";
        let pairs = parse_search_replace(text);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "cube(10);");
        assert_eq!(pairs[0].1, "cube(20);");
    }

    #[test]
    fn test_parse_search_replace_multiple() {
        let text = "<<<REPLACE\ncube(10);\n===\ncube(20);\n>>>\n\n<<<REPLACE\nsphere(5);\n===\nsphere(10);\n>>>";
        let pairs = parse_search_replace(text);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, "cube(10);");
        assert_eq!(pairs[0].1, "cube(20);");
        assert_eq!(pairs[1].0, "sphere(5);");
        assert_eq!(pairs[1].1, "sphere(10);");
    }

    #[test]
    fn test_parse_search_replace_multiline() {
        let text = "<<<REPLACE\nmodule foo() {\n    cube(10);\n}\n===\nmodule foo() {\n    cube(20);\n    sphere(5);\n}\n>>>";
        let pairs = parse_search_replace(text);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "module foo() {\n    cube(10);\n}");
        assert_eq!(
            pairs[0].1,
            "module foo() {\n    cube(20);\n    sphere(5);\n}"
        );
    }

    #[test]
    fn test_parse_search_replace_empty_new() {
        let text = "<<<REPLACE\ncube(10);\n===\n\n>>>";
        let pairs = parse_search_replace(text);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "cube(10);");
        assert_eq!(pairs[0].1, "");
    }

    #[test]
    fn test_parse_search_replace_none() {
        let text = "Just some text without any replace blocks.";
        let pairs = parse_search_replace(text);
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_apply_search_replace_ok() {
        let code = "cube(10);\nsphere(5);";
        let replacements = vec![("cube(10);".into(), "cube(20);".into())];
        let result = apply_search_replace(code, &replacements).unwrap();
        assert_eq!(result, "cube(20);\nsphere(5);");
    }

    #[test]
    fn test_apply_search_replace_not_found() {
        let code = "cube(10);";
        let replacements = vec![("cylinder(5);".into(), "cylinder(10);".into())];
        assert!(apply_search_replace(code, &replacements).is_err());
    }

    #[test]
    fn test_apply_search_replace_ambiguous() {
        let code = "cube(10);\ncube(10);";
        let replacements = vec![("cube(10);".into(), "cube(20);".into())];
        assert!(apply_search_replace(code, &replacements).is_err());
    }

    #[test]
    fn test_extract_code_change_prefers_replace() {
        let text = "<<<REPLACE\ncube(10);\n===\ncube(20);\n>>>\n\n```synapscad\ncube(99);\n```";
        match extract_code_change(text) {
            Some(CodeChange::SearchReplace(pairs)) => {
                assert_eq!(pairs.len(), 1);
                assert_eq!(pairs[0].1, "cube(20);");
            }
            _ => panic!("Expected SearchReplace"),
        }
    }

    #[test]
    fn test_extract_code_change_full_replace() {
        let text = "Here's the code:\n\n```synapscad\ncube(10);\n```";
        match extract_code_change(text) {
            Some(CodeChange::FullReplace(code)) => {
                assert_eq!(code, "cube(10);");
            }
            _ => panic!("Expected FullReplace"),
        }
    }

    #[test]
    fn test_extract_code_change_full_replace_openscad() {
        let text = "Here's the code:\n\n```openscad\nsphere(5);\n```";
        match extract_code_change(text) {
            Some(CodeChange::FullReplace(code)) => {
                assert_eq!(code, "sphere(5);");
            }
            _ => panic!("Expected FullReplace"),
        }
    }

    #[test]
    fn test_extract_code_change_none() {
        let text = "No code here, just a description.";
        assert!(extract_code_change(text).is_none());
    }

    #[test]
    fn test_extract_openscad_code_synapscad() {
        let text = "Here's the code:\n\n```synapscad\ncube(10);\n```";
        let code = extract_openscad_code(text);
        assert_eq!(code, Some("cube(10);".into()));
    }

    #[test]
    fn test_extract_openscad_code_openscad() {
        let text = "Here's the code:\n\n```openscad\nsphere(5);\n```";
        let code = extract_openscad_code(text);
        assert_eq!(code, Some("sphere(5);".into()));
    }

    #[test]
    fn test_extract_openscad_code_with_suffix() {
        let text = "Code with suffix:\n\n```synapscad:example\ncylinder(r=5, h=10);\n```";
        let code = extract_openscad_code(text);
        assert_eq!(code, Some("cylinder(r=5, h=10);".into()));
    }

    #[test]
    fn test_extract_openscad_code_openscad_with_suffix() {
        let text = "Code with suffix:\n\n```openscad:test\ntranslate([10,0,0]) cube(5);\n```";
        let code = extract_openscad_code(text);
        assert_eq!(code, Some("translate([10,0,0]) cube(5);".into()));
    }

    #[test]
    fn test_extract_openscad_code_prefers_synapscad() {
        let text = "Two blocks:\n\n```openscad\nsphere(5);\n```\n\n```synapscad\ncube(10);\n```";
        let code = extract_openscad_code(text);
        assert_eq!(code, Some("cube(10);".into()));
    }

    #[test]
    fn test_extract_openscad_code_empty() {
        let text = "Empty code:\n\n```synapscad\n\n```";
        let code = extract_openscad_code(text);
        assert_eq!(code, None);
    }

    #[test]
    fn test_normalize_custom_url_keeps_explicit_path() {
        let url = normalize_custom_url("Anthropic", "http://localhost:1234/anthropic/");
        assert_eq!(url, "http://localhost:1234/anthropic/");
    }

    #[test]
    fn test_normalize_custom_url_adds_default_path_for_host_only() {
        let url = normalize_custom_url("Anthropic", "http://localhost:1234");
        assert_eq!(url, "http://localhost:1234/v1/");
    }

    #[test]
    fn test_normalize_custom_url_openai_adds_default_path_for_host_only() {
        let url = normalize_custom_url("OpenAI", "http://localhost:1234");
        assert_eq!(url, "http://localhost:1234/v1/");
    }

    #[test]
    fn test_normalize_custom_url_ollama_stays_root() {
        let url = normalize_custom_url("Ollama", "http://localhost:11434");
        assert_eq!(url, "http://localhost:11434/");
    }
}
