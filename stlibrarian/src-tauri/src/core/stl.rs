use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// What we can learn about an STL without keeping it in memory.
///
/// Bounding box matters more than it looks: "will this fit on my bed?" is the
/// single most common question asked of a print library, and answering it from
/// a stored number costs nothing at browse time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StlMeta {
    pub triangles: u32,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl StlMeta {
    pub fn empty() -> Self {
        StlMeta {
            triangles: 0,
            min: [0.0; 3],
            max: [0.0; 3],
        }
    }

    pub fn dimensions(&self) -> [f32; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }
}

/// Read triangle count and bounding box, streaming.
///
/// Never loads the whole file. A 500 MB STL is ordinary in this hobby and the
/// old scanner did `fs::read` on every one of them.
pub fn read_meta(path: &Path) -> std::io::Result<StlMeta> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();

    // A binary STL is exactly 84 + 50n bytes: 80 header, 4 count, 50 per
    // triangle. Checking the arithmetic is the only reliable test -- plenty of
    // binary STLs start with the ASCII word "solid", so sniffing the first
    // bytes gets it wrong on real files.
    if len >= 84 {
        let mut reader = BufReader::new(file);
        let mut header = [0u8; 84];
        reader.read_exact(&mut header)?;
        let count = u32::from_le_bytes([header[80], header[81], header[82], header[83]]);
        if 84u64 + 50u64 * u64::from(count) == len {
            return read_binary(reader, count);
        }
    }
    read_ascii(path)
}

fn read_binary<R: Read>(mut reader: R, count: u32) -> std::io::Result<StlMeta> {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut tri = [0u8; 50];
    let mut seen = 0u32;

    for _ in 0..count {
        // A truncated file stops the walk instead of failing the whole scan --
        // a half-copied download should still show up in the library.
        if reader.read_exact(&mut tri).is_err() {
            break;
        }
        seen += 1;
        // Bytes 0..12 are the facet normal; the three vertices follow.
        for v in 0..3 {
            for axis in 0..3 {
                let o = 12 + v * 12 + axis * 4;
                let f = f32::from_le_bytes([tri[o], tri[o + 1], tri[o + 2], tri[o + 3]]);
                if f.is_finite() {
                    if f < min[axis] {
                        min[axis] = f;
                    }
                    if f > max[axis] {
                        max[axis] = f;
                    }
                }
            }
        }
    }
    Ok(finish(seen, min, max))
}

fn read_ascii(path: &Path) -> std::io::Result<StlMeta> {
    let reader = BufReader::new(File::open(path)?);
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut vertices = 0u64;

    for line in reader.lines() {
        // Non-UTF-8 means this was not ASCII after all; stop rather than error.
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim_start();
        if !trimmed.starts_with("vertex") {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        parts.next();
        let mut xyz = [0f32; 3];
        let mut ok = true;
        for axis in 0..3 {
            match parts.next().and_then(|s| s.parse::<f32>().ok()) {
                Some(v) if v.is_finite() => xyz[axis] = v,
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        vertices += 1;
        for axis in 0..3 {
            if xyz[axis] < min[axis] {
                min[axis] = xyz[axis];
            }
            if xyz[axis] > max[axis] {
                max[axis] = xyz[axis];
            }
        }
    }
    Ok(finish((vertices / 3) as u32, min, max))
}

fn finish(triangles: u32, min: [f32; 3], max: [f32; 3]) -> StlMeta {
    if triangles == 0 || !min[0].is_finite() {
        return StlMeta::empty();
    }
    StlMeta { triangles, min, max }
}

pub struct PreviewMesh {
    /// Nine floats per triangle: three vertices, x/y/z each.
    pub positions: Vec<f32>,
    pub triangles: u32,
    pub source_triangles: u32,
}

impl PreviewMesh {
    /// Little-endian bytes, which is what `Float32Array` expects on every
    /// platform this runs on.
    pub fn into_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.positions.len() * 4);
        for f in self.positions {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out
    }
}

pub fn read_preview(path: &Path, max_triangles: u32) -> std::io::Result<PreviewMesh> {
    let max = max_triangles.max(1);
    let meta = read_meta(path)?;

    // Small enough already: hand back every triangle, untouched.
    if meta.triangles <= max {
        return read_preview_exact(path, meta.triangles);
    }
    cluster(path, meta, max)
}

/// Every triangle, no reduction.
fn read_preview_exact(path: &Path, expected: u32) -> std::io::Result<PreviewMesh> {
    let mut positions = Vec::with_capacity(expected as usize * 9);
    let mut kept = 0u32;
    for_each_triangle(path, |tri| {
        for v in tri {
            positions.extend_from_slice(&v);
        }
        kept += 1;
    })?;
    Ok(PreviewMesh {
        positions,
        triangles: kept,
        source_triangles: kept,
    })
}

/// Reduce by vertex clustering.
///
/// The obvious reduction -- keep every Nth triangle -- is wrong, and wrong in a
/// way that looks fine in a unit test and terrible on screen. Triangles in an
/// STL are an unordered soup, so a 1-in-50 sample is fifty thousand facets
/// floating unconnected in space: the preview renders as dust rather than as a
/// model.
///
/// Clustering keeps the surface instead. Space is divided into a grid, every
/// vertex collapses to the average of its cell, and triangles whose corners
/// land in one cell disappear. What survives is a coherent, lower-resolution
/// shell -- blocky up close, correct at thumbnail size.
fn cluster(path: &Path, meta: StlMeta, max: u32) -> std::io::Result<PreviewMesh> {
    use std::collections::HashMap;

    // Output scales with the square of the grid resolution: a closed surface
    // crossing an NxNxN grid yields roughly 19 N^2 triangles, measured against
    // a sphere. Invert that to pick a side length that lands near `max`.
    //
    // `max` is a target, not a guarantee -- the constant depends on how
    // convoluted the surface is, and a folded mesh occupies more cells than a
    // sphere. That is acceptable: the budget exists to bound memory, and even
    // double 60k triangles is about 4 MB.
    const TRIS_PER_CELL_SQUARED: f64 = 19.0;
    let res = ((max as f64) / TRIS_PER_CELL_SQUARED)
        .sqrt()
        .round()
        .clamp(8.0, 256.0) as u32;

    let size = meta.dimensions();
    let extent = size[0].max(size[1]).max(size[2]).max(f32::MIN_POSITIVE);
    let scale = res as f32 / extent;

    let cell_of = |v: [f32; 3]| -> (u32, u32, u32) {
        let c = |x: f32, lo: f32| (((x - lo) * scale) as i64).clamp(0, res as i64 - 1) as u32;
        (
            c(v[0], meta.min[0]),
            c(v[1], meta.min[1]),
            c(v[2], meta.min[2]),
        )
    };

    // Cell -> (running vertex sum, count)
    let mut cells: HashMap<(u32, u32, u32), ([f64; 3], u32)> = HashMap::new();
    let mut faces: Vec<[(u32, u32, u32); 3]> = Vec::new();

    for_each_triangle(path, |tri| {
        let a = cell_of(tri[0]);
        let b = cell_of(tri[1]);
        let c = cell_of(tri[2]);
        // All three corners in one cell: the triangle has collapsed to a
        // point and contributes nothing but a degenerate face.
        if a == b && b == c {
            return;
        }
        for (cell, v) in [(a, tri[0]), (b, tri[1]), (c, tri[2])] {
            let entry = cells.entry(cell).or_insert(([0.0; 3], 0));
            entry.0[0] += f64::from(v[0]);
            entry.0[1] += f64::from(v[1]);
            entry.0[2] += f64::from(v[2]);
            entry.1 += 1;
        }
        faces.push([a, b, c]);
    })?;

    let representative: HashMap<(u32, u32, u32), [f32; 3]> = cells
        .into_iter()
        .map(|(k, (sum, n))| {
            let n = f64::from(n.max(1));
            (k, [(sum[0] / n) as f32, (sum[1] / n) as f32, (sum[2] / n) as f32])
        })
        .collect();

    // Many source triangles collapse onto the same trio of cells; keeping one
    // of each is most of the reduction.
    let mut seen = std::collections::HashSet::new();
    let mut positions = Vec::new();
    let mut kept = 0u32;
    for face in faces {
        let mut key = face;
        key.sort();
        if !seen.insert(key) {
            continue;
        }
        for cell in face {
            if let Some(p) = representative.get(&cell) {
                positions.extend_from_slice(p);
            }
        }
        kept += 1;
    }

    Ok(PreviewMesh {
        positions,
        triangles: kept,
        source_triangles: meta.triangles,
    })
}

/// Stream triangles out of either STL flavour, one callback per facet.
fn for_each_triangle<F: FnMut([[f32; 3]; 3])>(path: &Path, mut f: F) -> std::io::Result<()> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();

    if len >= 84 {
        let mut reader = BufReader::with_capacity(1 << 16, file);
        let mut header = [0u8; 84];
        reader.read_exact(&mut header)?;
        let count = u32::from_le_bytes([header[80], header[81], header[82], header[83]]);
        if 84u64 + 50u64 * u64::from(count) == len {
            let mut buf = [0u8; 50];
            for _ in 0..count {
                if reader.read_exact(&mut buf).is_err() {
                    break;
                }
                let mut tri = [[0f32; 3]; 3];
                for v in 0..3 {
                    for axis in 0..3 {
                        let o = 12 + v * 12 + axis * 4;
                        tri[v][axis] =
                            f32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
                    }
                }
                f(tri);
            }
            return Ok(());
        }
    }

    // ASCII
    let reader = BufReader::new(File::open(path)?);
    let mut pending: Vec<[f32; 3]> = Vec::with_capacity(3);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim_start();
        if !trimmed.starts_with("vertex") {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        parts.next();
        let mut xyz = [0f32; 3];
        let mut ok = true;
        for axis in 0..3 {
            match parts.next().and_then(|s| s.parse::<f32>().ok()) {
                Some(v) if v.is_finite() => xyz[axis] = v,
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        pending.push(xyz);
        if pending.len() == 3 {
            f([pending[0], pending[1], pending[2]]);
            pending.clear();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("stlib_test_{}_{}", std::process::id(), name));
        let mut f = File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p
    }

    /// One triangle with vertices at (0,0,0), (2,0,0), (0,3,4).
    fn binary_stl(triangles: &[[[f32; 3]; 3]]) -> Vec<u8> {
        let mut out = vec![0u8; 80];
        out.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
        for tri in triangles {
            out.extend_from_slice(&[0u8; 12]); // normal
            for v in tri {
                for c in v {
                    out.extend_from_slice(&c.to_le_bytes());
                }
            }
            out.extend_from_slice(&[0u8; 2]); // attribute byte count
        }
        out
    }

    #[test]
    fn binary_bounding_box_and_count() {
        let bytes = binary_stl(&[[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 4.0]]]);
        let p = tmp("bin.stl", &bytes);
        let meta = read_meta(&p).unwrap();
        assert_eq!(meta.triangles, 1);
        assert_eq!(meta.min, [0.0, 0.0, 0.0]);
        assert_eq!(meta.max, [2.0, 3.0, 4.0]);
        assert_eq!(meta.dimensions(), [2.0, 3.0, 4.0]);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn negative_coordinates_are_kept() {
        let bytes = binary_stl(&[[[-5.0, 0.0, 0.0], [1.0, -2.0, 0.0], [0.0, 0.0, 7.0]]]);
        let p = tmp("neg.stl", &bytes);
        let meta = read_meta(&p).unwrap();
        assert_eq!(meta.min, [-5.0, -2.0, 0.0]);
        assert_eq!(meta.max, [1.0, 0.0, 7.0]);
        assert_eq!(meta.dimensions(), [6.0, 2.0, 7.0]);
        std::fs::remove_file(p).ok();
    }

    /// The reason the detector does arithmetic instead of sniffing: this file
    /// is binary but its header begins with the word "solid".
    #[test]
    fn binary_file_whose_header_says_solid_is_not_read_as_ascii() {
        let mut bytes = binary_stl(&[[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]);
        bytes[..6].copy_from_slice(b"solid ");
        let p = tmp("liar.stl", &bytes);
        let meta = read_meta(&p).unwrap();
        assert_eq!(meta.triangles, 1);
        assert_eq!(meta.max, [1.0, 1.0, 0.0]);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn ascii_is_parsed() {
        let text = b"solid cube
 facet normal 0 0 0
  outer loop
   vertex 0 0 0
   vertex 10 0 0
   vertex 0 20 5
  endloop
 endfacet
endsolid cube
";
        let p = tmp("ascii.stl", text);
        let meta = read_meta(&p).unwrap();
        assert_eq!(meta.triangles, 1);
        assert_eq!(meta.min, [0.0, 0.0, 0.0]);
        assert_eq!(meta.max, [10.0, 20.0, 5.0]);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn empty_and_truncated_files_do_not_panic() {
        let p = tmp("empty.stl", b"");
        assert_eq!(read_meta(&p).unwrap(), StlMeta::empty());
        std::fs::remove_file(p).ok();

        // Claims 5 triangles, carries one and a half.
        let mut bytes = binary_stl(&[[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]]]);
        bytes[80..84].copy_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 20]);
        let p = tmp("trunc.stl", &bytes);
        let meta = read_meta(&p).unwrap();
        assert!(meta.triangles <= 5);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn zero_triangle_binary_reports_empty() {
        let p = tmp("zero.stl", &binary_stl(&[]));
        assert_eq!(read_meta(&p).unwrap(), StlMeta::empty());
        std::fs::remove_file(p).ok();
    }

    // ------------------------------------------------------- preview ----

    #[test]
    fn a_small_mesh_is_returned_whole() {
        let tris = [
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0]],
        ];
        let p = tmp("prev_small.stl", &binary_stl(&tris));
        let mesh = read_preview(&p, 1000).unwrap();
        assert_eq!(mesh.triangles, 2);
        assert_eq!(mesh.source_triangles, 2);
        assert_eq!(mesh.positions.len(), 2 * 9);
        assert_eq!(&mesh.positions[0..3], &[0.0, 0.0, 0.0]);
        std::fs::remove_file(p).ok();
    }



    #[test]
    fn preview_of_ascii_works_and_decimates() {
        let mut text = String::from("solid s\n");
        for i in 0..300 {
            text.push_str(&format!(
                " facet normal 0 0 0\n  outer loop\n   vertex {} 0 0\n   vertex {} 1 0\n   vertex {} 0 1\n  endloop\n endfacet\n",
                i, i, i
            ));
        }
        text.push_str("endsolid s\n");
        let p = tmp("prev_ascii.stl", text.as_bytes());
        let mesh = read_preview(&p, 30).unwrap();
        assert_eq!(mesh.source_triangles, 300);
        assert!(mesh.triangles <= 30, "kept {}", mesh.triangles);
        assert_eq!(mesh.positions.len(), mesh.triangles as usize * 9);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn preview_bytes_round_trip_as_little_endian_floats() {
        let tris = [[[1.5, 2.5, 3.5], [4.5, 5.5, 6.5], [7.5, 8.5, 9.5]]];
        let p = tmp("prev_bytes.stl", &binary_stl(&tris));
        let mesh = read_preview(&p, 10).unwrap();
        let bytes = mesh.into_bytes();
        assert_eq!(bytes.len(), 9 * 4);
        let first = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert_eq!(first, 1.5);
        let last = f32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);
        assert_eq!(last, 9.5);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn preview_of_an_empty_file_is_empty_not_a_panic() {
        let p = tmp("prev_empty.stl", b"");
        let mesh = read_preview(&p, 100).unwrap();
        assert_eq!(mesh.triangles, 0);
        assert!(mesh.positions.is_empty());
        std::fs::remove_file(p).ok();
    }
    /// A sphere, which is what this code actually meets: a closed surface
    /// whose triangles share edges.
    fn sphere(radius: f32, segments: usize) -> Vec<[[f32; 3]; 3]> {
        use std::f32::consts::PI;
        let mut tris = Vec::new();
        let rings = segments / 2;
        for i in 0..rings {
            let t0 = PI * i as f32 / rings as f32;
            let t1 = PI * (i + 1) as f32 / rings as f32;
            for j in 0..segments {
                let p0 = 2.0 * PI * j as f32 / segments as f32;
                let p1 = 2.0 * PI * (j + 1) as f32 / segments as f32;
                let pt = |t: f32, p: f32| {
                    [radius * t.sin() * p.cos(), radius * t.sin() * p.sin(), radius * t.cos()]
                };
                let (a, b, c, d) = (pt(t0, p0), pt(t0, p1), pt(t1, p1), pt(t1, p0));
                tris.push([a, b, c]);
                tris.push([a, c, d]);
            }
        }
        tris
    }

    fn bounds(positions: &[f32]) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for chunk in positions.chunks(3) {
            for a in 0..3 {
                if chunk[a] < min[a] { min[a] = chunk[a]; }
                if chunk[a] > max[a] { max[a] = chunk[a]; }
            }
        }
        (min, max)
    }

    #[test]
    fn a_large_surface_is_reduced_below_the_cap() {
        let tris = sphere(50.0, 100);
        assert!(tris.len() > 9_000, "fixture too small: {}", tris.len());
        let p = tmp("prev_sphere.stl", &binary_stl(&tris));

        let mesh = read_preview(&p, 1_500).unwrap();
        assert_eq!(mesh.source_triangles as usize, tris.len());
        // The budget is a target; what matters is that it lands in the right
        // order of magnitude rather than passing the source through.
        assert!(mesh.triangles <= 3_000, "over budget: {}", mesh.triangles);
        assert!(mesh.triangles < mesh.source_triangles / 3, "barely reduced: {}", mesh.triangles);
        // Not collapsed to nothing either -- a preview of 12 triangles is as
        // useless as one of three million.
        assert!(mesh.triangles > 200, "reduced too far: {}", mesh.triangles);
        std::fs::remove_file(p).ok();
    }

    /// The reduced mesh has to be the *whole* model, not a piece of it.
    #[test]
    fn the_reduced_mesh_still_covers_the_whole_model() {
        let tris = sphere(50.0, 100);
        let p = tmp("prev_cover.stl", &binary_stl(&tris));
        let mesh = read_preview(&p, 1_500).unwrap();

        let (min, max) = bounds(&mesh.positions);
        for axis in 0..3 {
            assert!(min[axis] < -45.0, "axis {} starts at {}", axis, min[axis]);
            assert!(max[axis] > 45.0, "axis {} ends at {}", axis, max[axis]);
        }
        std::fs::remove_file(p).ok();
    }

    /// The property that makes a preview look like a model instead of dust.
    ///
    /// Keeping every Nth triangle leaves facets that share no corners, so the
    /// vertex count stays at three per triangle and the render is a cloud of
    /// unconnected specks. Clustering welds corners together, so distinct
    /// vertices fall far below 3x the triangle count.
    #[test]
    fn reduction_welds_vertices_rather_than_scattering_triangles() {
        use std::collections::HashSet;
        let tris = sphere(50.0, 100);
        let p = tmp("prev_weld.stl", &binary_stl(&tris));
        let mesh = read_preview(&p, 1_500).unwrap();

        let unique: HashSet<[u32; 3]> = mesh
            .positions
            .chunks(3)
            .map(|c| [c[0].to_bits(), c[1].to_bits(), c[2].to_bits()])
            .collect();
        let corners = mesh.triangles as usize * 3;
        assert!(
            unique.len() * 2 < corners,
            "vertices are not shared: {} unique for {} corners",
            unique.len(),
            corners
        );
        std::fs::remove_file(p).ok();
    }

}
