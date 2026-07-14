use base64::Engine;
use image::ImageEncoder;

use super::types::{MeshData, ViewImage};

pub mod colors;
pub mod fonts;

const VIEW_SIZE: u32 = 256;
const BG_COLOR: [u8; 3] = [50, 50, 60];

struct ProjectedTri {
    verts: [(f32, f32, f32); 3],
    normal: [f32; 3],
    color: [f32; 3],
}

/// Default palette for parts without explicit color (matches `PART_PALETTE` in compilation.rs).
const VIEW_PART_PALETTE: &[[f32; 3]] = &[
    [0.40, 0.70, 1.00],
    [1.00, 0.60, 0.40],
    [0.50, 0.85, 0.50],
    [0.95, 0.75, 0.30],
    [0.70, 0.50, 0.90],
    [0.30, 0.85, 0.85],
    [0.95, 0.45, 0.60],
    [0.60, 0.80, 0.30],
    [0.85, 0.55, 0.80],
    [0.45, 0.65, 0.85],
    [0.90, 0.65, 0.55],
    [0.55, 0.75, 0.65],
];

#[must_use]
/// Renders the standard orthographic and isometric previews for `parts`.
pub fn render_orthographic_views(parts: &[MeshData]) -> Vec<ViewImage> {
    render_orthographic_views_sized(parts, VIEW_SIZE)
}

/// Render orthographic + isometric views at the given pixel size.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn render_orthographic_views_sized(parts: &[MeshData], size: u32) -> Vec<ViewImage> {
    let mut all_pos = Vec::new();
    let mut all_norm = Vec::new();
    let mut all_idx = Vec::new();
    let mut tri_colors = Vec::new();
    for (part_idx, part) in parts.iter().enumerate() {
        let offset = all_pos.len() as u32;
        all_pos.extend_from_slice(&part.positions);
        all_norm.extend_from_slice(&part.normals);
        all_idx.extend(part.indices.iter().map(|i| i + offset));
        let color = part
            .color
            .unwrap_or(VIEW_PART_PALETTE[part_idx % VIEW_PART_PALETTE.len()]);
        let num_tris = part.indices.len() / 3;
        tri_colors.extend(std::iter::repeat_n(color, num_tris));
    }

    if all_pos.is_empty() {
        return Vec::new();
    }

    // Each tuple maps model axes to screen X, screen Y, and depth.
    let views = [
        ("Front", [0, 1, 2], [1.0_f32, 1.0, 1.0]),
        ("Right", [2, 1, 0], [-1.0_f32, 1.0, 1.0]),
        ("Top", [0, 2, 1], [1.0_f32, -1.0, 1.0]),
        ("Bottom", [0, 2, 1], [1.0_f32, 1.0, -1.0]),
    ];

    let mut result: Vec<ViewImage> = views
        .iter()
        .map(|(label, axes, flips)| {
            let base64_png = render_single_view(
                &all_pos,
                &all_norm,
                &all_idx,
                &tri_colors,
                *axes,
                *flips,
                size,
            );
            ViewImage {
                label: (*label).to_string(),
                base64_png,
            }
        })
        .collect();

    let iso_png = render_iso_view(&all_pos, &all_norm, &all_idx, &tri_colors, size);
    result.push(ViewImage {
        label: "Iso".to_string(),
        base64_png: iso_png,
    });

    result
}

/// Render an isometric view using a rotated projection (looking from top-right-front).
#[allow(
    clippy::similar_names,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn render_iso_view(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    indices: &[u32],
    tri_colors: &[[f32; 3]],
    view_size: u32,
) -> String {
    let size = view_size as usize;
    let margin = 0.1;

    // Rotate 45° around Y and then arctan(1/sqrt(2)) around X.
    let cos_y = std::f32::consts::FRAC_1_SQRT_2;
    let sin_y = std::f32::consts::FRAC_1_SQRT_2;
    let angle_x: f32 = 35.264_f32.to_radians();
    let cos_x = angle_x.cos();
    let sin_x = angle_x.sin();

    let project = |p: &[f32; 3]| -> (f32, f32, f32) {
        let rx = p[0].mul_add(cos_y, p[2] * sin_y);
        let ry = p[1];
        let rz = (-p[0]).mul_add(sin_y, p[2] * cos_y);
        let sx = rx;
        let sy = ry.mul_add(cos_x, -(rz * sin_x));
        let r_z = ry.mul_add(sin_x, rz * cos_x);
        (sx, sy, r_z)
    };

    let projected: Vec<(f32, f32, f32)> = positions.iter().map(project).collect();

    let (mut sx_min, mut sx_max) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut sy_min, mut sy_max) = (f32::INFINITY, f32::NEG_INFINITY);
    for &(sx, sy, _) in &projected {
        sx_min = sx_min.min(sx);
        sx_max = sx_max.max(sx);
        sy_min = sy_min.min(sy);
        sy_max = sy_max.max(sy);
    }

    let range_x = sx_max - sx_min;
    let range_y = sy_max - sy_min;
    if range_x < 1e-6 || range_y < 1e-6 {
        return String::new();
    }

    let usable = 2.0f32.mul_add(-margin, 1.0);
    let scale = (size as f32 * usable) / range_x.max(range_y);
    let cx = f32::midpoint(sx_min, sx_max);
    let cy = f32::midpoint(sy_min, sy_max);
    let half = size as f32 / 2.0;

    let to_pixel = |sx: f32, sy: f32| -> (f32, f32) {
        (
            (sx - cx).mul_add(scale, half),
            (-(sy - cy)).mul_add(scale, half),
        )
    };

    let mut tris: Vec<ProjectedTri> = Vec::with_capacity(indices.len() / 3);
    for (tri_idx, tri) in indices.chunks(3).enumerate() {
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let v0 = projected[i0];
        let v1 = projected[i1];
        let v2 = projected[i2];
        let n = [
            (normals[i0][0] + normals[i1][0] + normals[i2][0]) / 3.0,
            (normals[i0][1] + normals[i1][1] + normals[i2][1]) / 3.0,
            (normals[i0][2] + normals[i1][2] + normals[i2][2]) / 3.0,
        ];
        let color = tri_colors.get(tri_idx).copied().unwrap_or([0.4, 0.7, 1.0]);
        tris.push(ProjectedTri {
            verts: [v0, v1, v2],
            normal: n,
            color,
        });
    }

    let mut pixels = vec![BG_COLOR; size * size];
    let mut depth_buf = vec![f32::NEG_INFINITY; size * size];

    let light_dir = normalize([0.3, 0.5, 1.0]);

    for tri in &tris {
        let p0 = to_pixel(tri.verts[0].0, tri.verts[0].1);
        let p1 = to_pixel(tri.verts[1].0, tri.verts[1].1);
        let p2 = to_pixel(tri.verts[2].0, tri.verts[2].1);

        let min_px = (p0.0.min(p1.0).min(p2.0).floor() as i32).max(0);
        let max_px = (p0.0.max(p1.0).max(p2.0).ceil() as i32).min(size as i32 - 1);
        let min_py = (p0.1.min(p1.1).min(p2.1).floor() as i32).max(0);
        let max_py = (p0.1.max(p1.1).max(p2.1).ceil() as i32).min(size as i32 - 1);

        for py in min_py..=max_py {
            for px in min_px..=max_px {
                let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                let (w0, w1, w2) = barycentric(p0, p1, p2, (fx, fy));
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    let depth = w2.mul_add(
                        tri.verts[2].2,
                        w0.mul_add(tri.verts[0].2, w1 * tri.verts[1].2),
                    );
                    let idx = py as usize * size + px as usize;
                    if depth > depth_buf[idx] {
                        depth_buf[idx] = depth;
                        let ndotl = dot(tri.normal, light_dir).abs();
                        let shade = 0.8f32.mul_add(ndotl, 0.2);
                        let r = (tri.color[0] * 255.0 * shade).clamp(0.0, 255.0) as u8;
                        let g = (tri.color[1] * 255.0 * shade).clamp(0.0, 255.0) as u8;
                        let b = (tri.color[2] * 255.0 * shade).clamp(0.0, 255.0) as u8;
                        pixels[idx] = [r, g, b];
                    }
                }
            }
        }
    }

    let mut img_buf = image::RgbImage::new(view_size, view_size);
    for (i, px) in pixels.iter().enumerate() {
        let x = (i % size) as u32;
        let y = (i / size) as u32;
        img_buf.put_pixel(x, y, image::Rgb(*px));
    }

    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder
        .write_image(
            img_buf.as_raw(),
            view_size,
            view_size,
            image::ExtendedColorType::Rgb8,
        )
        .expect("PNG encoding failed");

    base64::engine::general_purpose::STANDARD.encode(&png_bytes)
}

#[allow(
    clippy::similar_names,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn render_single_view(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    indices: &[u32],
    tri_colors: &[[f32; 3]],
    axes: [usize; 3],
    flips: [f32; 3],
    view_size: u32,
) -> String {
    let size = view_size as usize;
    let margin = 0.1;

    let projected: Vec<(f32, f32, f32)> = positions
        .iter()
        .map(|p| {
            (
                p[axes[0]] * flips[0],
                p[axes[1]] * flips[1],
                p[axes[2]] * flips[2],
            )
        })
        .collect();

    let (mut sx_min, mut sx_max) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut sy_min, mut sy_max) = (f32::INFINITY, f32::NEG_INFINITY);
    for &(sx, sy, _) in &projected {
        sx_min = sx_min.min(sx);
        sx_max = sx_max.max(sx);
        sy_min = sy_min.min(sy);
        sy_max = sy_max.max(sy);
    }

    let range_x = sx_max - sx_min;
    let range_y = sy_max - sy_min;
    if range_x < 1e-6 || range_y < 1e-6 {
        return String::new();
    }

    // One scale preserves aspect ratio while fitting both dimensions.
    let usable = 2.0f32.mul_add(-margin, 1.0);
    let scale = (size as f32 * usable) / range_x.max(range_y);
    let cx = f32::midpoint(sx_min, sx_max);
    let cy = f32::midpoint(sy_min, sy_max);
    let half = size as f32 / 2.0;

    // Image-space Y increases downward.
    let to_pixel = |sx: f32, sy: f32| -> (f32, f32) {
        (
            (sx - cx).mul_add(scale, half),
            (-(sy - cy)).mul_add(scale, half),
        )
    };

    let mut tris: Vec<ProjectedTri> = Vec::with_capacity(indices.len() / 3);
    for (tri_idx, tri) in indices.chunks(3).enumerate() {
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let v0 = projected[i0];
        let v1 = projected[i1];
        let v2 = projected[i2];
        let n = [
            (normals[i0][0] + normals[i1][0] + normals[i2][0]) / 3.0,
            (normals[i0][1] + normals[i1][1] + normals[i2][1]) / 3.0,
            (normals[i0][2] + normals[i1][2] + normals[i2][2]) / 3.0,
        ];
        let color = tri_colors.get(tri_idx).copied().unwrap_or([0.4, 0.7, 1.0]);
        tris.push(ProjectedTri {
            verts: [v0, v1, v2],
            normal: n,
            color,
        });
    }

    let mut pixels = vec![BG_COLOR; size * size];
    let mut depth_buf = vec![f32::NEG_INFINITY; size * size];

    // A slight offset from the camera direction reveals surface curvature.
    let light_dir = normalize([0.3, 0.5, 1.0]);

    for tri in &tris {
        let p0 = to_pixel(tri.verts[0].0, tri.verts[0].1);
        let p1 = to_pixel(tri.verts[1].0, tri.verts[1].1);
        let p2 = to_pixel(tri.verts[2].0, tri.verts[2].1);

        let min_px = (p0.0.min(p1.0).min(p2.0).floor() as i32).max(0);
        let max_px = (p0.0.max(p1.0).max(p2.0).ceil() as i32).min(size as i32 - 1);
        let min_py = (p0.1.min(p1.1).min(p2.1).floor() as i32).max(0);
        let max_py = (p0.1.max(p1.1).max(p2.1).ceil() as i32).min(size as i32 - 1);

        for py in min_py..=max_py {
            for px in min_px..=max_px {
                let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                let (w0, w1, w2) = barycentric(p0, p1, p2, (fx, fy));
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    let depth = w2.mul_add(
                        tri.verts[2].2,
                        w0.mul_add(tri.verts[0].2, w1 * tri.verts[1].2),
                    );
                    let idx = py as usize * size + px as usize;
                    if depth > depth_buf[idx] {
                        depth_buf[idx] = depth;
                        let ndotl = dot(tri.normal, light_dir).abs();
                        let shade = 0.8f32.mul_add(ndotl, 0.2);
                        let r = (tri.color[0] * 255.0 * shade).clamp(0.0, 255.0) as u8;
                        let g = (tri.color[1] * 255.0 * shade).clamp(0.0, 255.0) as u8;
                        let b = (tri.color[2] * 255.0 * shade).clamp(0.0, 255.0) as u8;
                        pixels[idx] = [r, g, b];
                    }
                }
            }
        }
    }

    let mut img_buf = image::RgbImage::new(view_size, view_size);
    for (i, px) in pixels.iter().enumerate() {
        let x = (i % size) as u32;
        let y = (i / size) as u32;
        img_buf.put_pixel(x, y, image::Rgb(*px));
    }

    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder
        .write_image(
            img_buf.as_raw(),
            view_size,
            view_size,
            image::ExtendedColorType::Rgb8,
        )
        .expect("PNG encoding failed");

    base64::engine::general_purpose::STANDARD.encode(&png_bytes)
}

fn barycentric(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p: (f32, f32)) -> (f32, f32, f32) {
    let d = (p1.1 - p2.1).mul_add(p0.0 - p2.0, (p2.0 - p1.0) * (p0.1 - p2.1));
    if d.abs() < 1e-10 {
        return (-1.0, -1.0, -1.0);
    }
    let w0 = (p1.1 - p2.1).mul_add(p.0 - p2.0, (p2.0 - p1.0) * (p.1 - p2.1)) / d;
    let w1 = (p2.1 - p0.1).mul_add(p.0 - p2.0, (p0.0 - p2.0) * (p.1 - p2.1)) / d;
    let w2 = 1.0 - w0 - w1;
    (w0, w1, w2)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[2].mul_add(b[2], a[0].mul_add(b[0], a[1] * b[1]))
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = v[2].mul_add(v[2], v[0].mul_add(v[0], v[1] * v[1])).sqrt();
    if len < 1e-10 {
        return [0.0, 0.0, 1.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}
