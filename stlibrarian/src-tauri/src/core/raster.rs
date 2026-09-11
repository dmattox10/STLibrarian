//! A software rasteriser for model thumbnails.
//!
//! Previews used to be rendered by whichever client happened to open the
//! library: the front end pulled the mesh, drew it with WebGL and uploaded the
//! PNG back. That leaves a headless NAS with no previews at all until someone
//! visits it in a browser that has WebGL -- the server can index a library and
//! still have nothing to show for it.
//!
//! This draws them server-side instead. No GPU, no window, no client. It
//! streams the mesh a triangle at a time, so peak memory is the framebuffer
//! rather than the model, and a 3M triangle file costs the same as a cube.

use crate::core::stl::{for_each_triangle, read_meta};
use std::path::Path;

/// Rendered at this multiple of the requested size, then box-filtered down.
/// Cheap antialiasing, and without it every silhouette is a staircase.
const SUPERSAMPLE: u32 = 2;

/// Matches the interactive viewer's material and lighting closely enough that a
/// card and the detail view look like the same object.
const BASE_COLOR: [f32; 3] = [0.77, 0.82, 0.85];
const AMBIENT: f32 = 0.34;
const RIM_COLOR: [f32; 3] = [0.37, 0.51, 0.67];

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= f32::EPSILON {
        return [0.0, 0.0, 1.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// An orthonormal camera basis for a three-quarter view.
///
/// Print STLs are authored Z-up, so world up is Z here rather than Y.
struct View {
    right: [f32; 3],
    up: [f32; 3],
    dir: [f32; 3],
}

fn three_quarter_view(azimuth: f32, elevation: f32) -> View {
    let dir = normalize([
        azimuth.sin() * elevation.cos(),
        azimuth.cos() * elevation.cos(),
        elevation.sin(),
    ]);
    let right = normalize(cross([0.0, 0.0, 1.0], dir));
    let up = cross(dir, right);
    View { right, up, dir }
}

/// Render `path` to a PNG of `size` x `size`, transparent where the model is not.
pub fn render(path: &Path, size: u32) -> std::io::Result<Vec<u8>> {
    let size = size.clamp(32, 2048);
    let meta = read_meta(path)?;
    if meta.triangles == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "model has no triangles",
        ));
    }

    let big = size * SUPERSAMPLE;
    let view = three_quarter_view(0.6, 0.45);

    let center = [
        (meta.min[0] + meta.max[0]) * 0.5,
        (meta.min[1] + meta.max[1]) * 0.5,
        (meta.min[2] + meta.max[2]) * 0.5,
    ];

    // Fit by projecting the eight bounding-box corners; the silhouette of a
    // rotated box is what actually has to fit, not its axis-aligned extent.
    let mut half = 0f32;
    for i in 0..8 {
        let corner = [
            if i & 1 == 0 { meta.min[0] } else { meta.max[0] },
            if i & 2 == 0 { meta.min[1] } else { meta.max[1] },
            if i & 4 == 0 { meta.min[2] } else { meta.max[2] },
        ];
        let p = sub(corner, center);
        half = half.max(dot(p, view.right).abs()).max(dot(p, view.up).abs());
    }
    if half <= f32::EPSILON {
        half = 1.0;
    }
    let scale = (big as f32) / (half * 2.0 * 1.12); // a little breathing room

    let mut color = vec![0f32; (big * big * 3) as usize];
    let mut alpha = vec![0f32; (big * big) as usize];
    let mut depth = vec![f32::INFINITY; (big * big) as usize];

    let half_px = big as f32 * 0.5;
    let key = normalize([0.45, -0.65, 0.62]);
    let rim = normalize([-0.6, 0.5, -0.35]);

    for_each_triangle(path, |tri| {
        // To screen space: x right, y up (flipped for raster order), z toward camera.
        let mut sx = [0f32; 3];
        let mut sy = [0f32; 3];
        let mut sz = [0f32; 3];
        for i in 0..3 {
            let p = sub(tri[i], center);
            sx[i] = dot(p, view.right) * scale + half_px;
            sy[i] = half_px - dot(p, view.up) * scale;
            sz[i] = dot(p, view.dir);
        }

        // Winding in screen space tells us which way the facet faces; the
        // normals stored in an STL are too often wrong to trust.
        let area = (sx[1] - sx[0]) * (sy[2] - sy[0]) - (sx[2] - sx[0]) * (sy[1] - sy[0]);
        if area.abs() < 1e-9 {
            return;
        }

        let normal = normalize(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0])));
        // Face the light whichever way the triangle was wound.
        let facing = if dot(normal, view.dir) < 0.0 {
            [-normal[0], -normal[1], -normal[2]]
        } else {
            normal
        };
        let lambert = dot(facing, key).max(0.0);
        let rim_term = dot(facing, rim).max(0.0) * 0.35;
        let shade = (AMBIENT + lambert * 0.78).min(1.0);
        let lit = [
            (BASE_COLOR[0] * shade + RIM_COLOR[0] * rim_term).min(1.0),
            (BASE_COLOR[1] * shade + RIM_COLOR[1] * rim_term).min(1.0),
            (BASE_COLOR[2] * shade + RIM_COLOR[2] * rim_term).min(1.0),
        ];

        let min_x = sx[0].min(sx[1]).min(sx[2]).floor().max(0.0) as i64;
        let max_x = sx[0].max(sx[1]).max(sx[2]).ceil().min(big as f32 - 1.0) as i64;
        let min_y = sy[0].min(sy[1]).min(sy[2]).floor().max(0.0) as i64;
        let max_y = sy[0].max(sy[1]).max(sy[2]).ceil().min(big as f32 - 1.0) as i64;
        if max_x < min_x || max_y < min_y {
            return;
        }

        let inv_area = 1.0 / area;
        for py in min_y..=max_y {
            for px in min_x..=max_x {
                let x = px as f32 + 0.5;
                let y = py as f32 + 0.5;
                // Barycentric coverage via edge functions.
                let w0 = ((sx[1] - sx[0]) * (y - sy[0]) - (sy[1] - sy[0]) * (x - sx[0])) * inv_area;
                let w1 = ((sx[2] - sx[1]) * (y - sy[1]) - (sy[2] - sy[1]) * (x - sx[1])) * inv_area;
                let w2 = ((sx[0] - sx[2]) * (y - sy[2]) - (sy[0] - sy[2]) * (x - sx[2])) * inv_area;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = sz[0] * w1 + sz[1] * w2 + sz[2] * w0;
                let idx = (py as u32 * big + px as u32) as usize;
                // Larger z is nearer the camera along `dir`.
                if depth[idx] != f32::INFINITY && z <= depth[idx] {
                    continue;
                }
                depth[idx] = z;
                color[idx * 3] = lit[0];
                color[idx * 3 + 1] = lit[1];
                color[idx * 3 + 2] = lit[2];
                alpha[idx] = 1.0;
            }
        }
    })?;

    // Box-filter down to the requested size.
    let mut out = vec![0u8; (size * size * 4) as usize];
    let s = SUPERSAMPLE as usize;
    for y in 0..size as usize {
        for x in 0..size as usize {
            let (mut r, mut g, mut b, mut a) = (0f32, 0f32, 0f32, 0f32);
            for dy in 0..s {
                for dx in 0..s {
                    let idx = ((y * s + dy) * big as usize) + (x * s + dx);
                    r += color[idx * 3];
                    g += color[idx * 3 + 1];
                    b += color[idx * 3 + 2];
                    a += alpha[idx];
                }
            }
            let n = (s * s) as f32;
            let o = (y * size as usize + x) * 4;
            out[o] = (r / n * 255.0).round().clamp(0.0, 255.0) as u8;
            out[o + 1] = (g / n * 255.0).round().clamp(0.0, 255.0) as u8;
            out[o + 2] = (b / n * 255.0).round().clamp(0.0, 255.0) as u8;
            out[o + 3] = (a / n * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }

    encode_png(&out, size)
}

fn encode_png(rgba: &[u8], size: u32) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn cube_stl(side: f32) -> Vec<u8> {
        let s = side;
        let p = [
            [0.0, 0.0, 0.0], [s, 0.0, 0.0], [s, s, 0.0], [0.0, s, 0.0],
            [0.0, 0.0, s], [s, 0.0, s], [s, s, s], [0.0, s, s],
        ];
        let faces = [
            (0, 1, 2), (0, 2, 3), (4, 6, 5), (4, 7, 6),
            (0, 5, 1), (0, 4, 5), (1, 6, 2), (1, 5, 6),
            (2, 7, 3), (2, 6, 7), (3, 4, 0), (3, 7, 4),
        ];
        let mut out = vec![0u8; 80];
        out.extend_from_slice(&(faces.len() as u32).to_le_bytes());
        for (a, b, c) in faces {
            out.extend_from_slice(&[0u8; 12]);
            for v in [p[a], p[b], p[c]] {
                for coord in v {
                    out.extend_from_slice(&coord.to_le_bytes());
                }
            }
            out.extend_from_slice(&[0u8; 2]);
        }
        out
    }

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("stlib_raster_{}_{}", std::process::id(), name));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p
    }

    /// Decode just enough PNG header to check the dimensions.
    fn png_size(bytes: &[u8]) -> (u32, u32) {
        assert_eq!(&bytes[0..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        assert_eq!(&bytes[12..16], b"IHDR");
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        (w, h)
    }

    #[test]
    fn renders_a_png_of_the_requested_size() {
        let p = tmp("cube.stl", &cube_stl(20.0));
        let png = render(&p, 256).unwrap();
        assert_eq!(png_size(&png), (256, 256));
        std::fs::remove_file(p).ok();
    }

    /// The point of the whole module: it must work with no GPU and no window,
    /// which is what a test process has.
    #[test]
    fn the_render_is_not_blank() {
        let p = tmp("cube2.stl", &cube_stl(20.0));
        let png = render(&p, 128).unwrap();
        // A blank frame compresses to almost nothing; a shaded cube does not.
        assert!(png.len() > 400, "suspiciously small output: {} bytes", png.len());
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn two_renders_of_the_same_model_agree() {
        let p = tmp("cube3.stl", &cube_stl(20.0));
        assert_eq!(render(&p, 96).unwrap(), render(&p, 96).unwrap());
        std::fs::remove_file(p).ok();
    }

    /// Position must not change the picture -- the camera frames the bounding
    /// box, so a part modelled far from the origin renders identically.
    #[test]
    fn a_model_far_from_the_origin_renders_the_same() {
        let near = tmp("near.stl", &cube_stl(20.0));
        let mut shifted = cube_stl(20.0);
        // Nudge every vertex by +1000 on x.
        for t in 0..12usize {
            for v in 0..3usize {
                let o = 84 + t * 50 + 12 + v * 12;
                let x = f32::from_le_bytes([shifted[o], shifted[o + 1], shifted[o + 2], shifted[o + 3]]);
                shifted[o..o + 4].copy_from_slice(&(x + 1000.0).to_le_bytes());
            }
        }
        let far = tmp("far.stl", &shifted);
        assert_eq!(render(&near, 96).unwrap(), render(&far, 96).unwrap());
        std::fs::remove_file(near).ok();
        std::fs::remove_file(far).ok();
    }

    #[test]
    fn an_empty_model_is_an_error_not_a_blank_image() {
        let p = tmp("empty.stl", &{
            let mut v = vec![0u8; 80];
            v.extend_from_slice(&0u32.to_le_bytes());
            v
        });
        assert!(render(&p, 64).is_err());
        std::fs::remove_file(p).ok();
    }
}
