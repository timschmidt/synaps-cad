//! Build-time application constants that are not exposed as user settings.

/// GitHub repository URL for issue reporting.
pub const GITHUB_REPO_URL: &str = "https://github.com/timschmidt/synaps-cad";

/// Maximum texture side length for egui (GPU limit).
pub const MAX_TEXTURE_SIDE: u32 = 2048;

/// Maximum image size in bytes for AI API requests (most APIs cap at 5 MB).
pub const MAX_IMAGE_BYTES: usize = 4_500_000;
