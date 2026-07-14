/// Triangle mesh data ready for Bevy rendering.
#[derive(Debug)]
pub struct MeshData {
    /// Vertex positions in Bevy's Y-up coordinate system.
    pub positions: Vec<[f32; 3]>,
    /// Per-vertex normals parallel to [`Self::positions`].
    pub normals: Vec<[f32; 3]>,
    /// Triangle-list indices into [`Self::positions`].
    pub indices: Vec<u32>,
    /// Optional color set via `color()` in the `OpenSCAD` code.
    pub color: Option<[f32; 3]>,
}

/// A rendered orthographic view encoded as base64 PNG.
#[derive(Debug)]
pub struct ViewImage {
    /// Human-readable camera direction such as `front` or `isometric`.
    pub label: String,
    /// PNG bytes encoded without a data-URL prefix.
    pub base64_png: String,
}

/// Result of compiling one `OpenSCAD` source buffer.
#[derive(Debug)]
pub enum CompilationResult {
    /// Compilation completed with zero or more renderable parts.
    Success {
        /// Independent top-level meshes in source order.
        parts: Vec<MeshData>,
        /// Orthographic previews rendered from the completed parts.
        views: Vec<ViewImage>,
        /// Recoverable evaluator or mesh-conversion diagnostics.
        warnings: Vec<String>,
    },
    /// Parsing could not produce a source file.
    Error(String),
    /// The supplied cancellation flag was raised during compilation.
    Canceled,
}
