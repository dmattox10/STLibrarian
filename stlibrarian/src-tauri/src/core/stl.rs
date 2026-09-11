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
    /// Enclosed volume in the STL's own cubic units -- cubic millimetres by
    /// near-universal convention in printing. Multiply by a resin or filament
    /// density and you have grams, which is what a print actually costs.
    ///
    /// Only meaningful for a closed mesh. An open shell still yields a number,
    /// it just is not the volume of anything -- see `watertight`.
    pub volume: f32,
    /// Every edge shared by exactly two triangles.
    ///
    /// A sculpted model is often not closed, and an open mesh makes `volume`
    /// meaningless rather than merely imprecise. Knowing which you have is
    /// also the difference between a model that slices and one that does not.
    pub watertight: bool,
}

impl StlMeta {
    pub fn empty() -> Self {
        StlMeta {
            triangles: 0,
            min: [0.0; 3],
            max: [0.0; 3],
            volume: 0.0,
            watertight: false,
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

/// Bounding box, triangle count and volume, in one streaming pass.
///
/// Never loads the whole file. A 500 MB STL is ordinary in this hobby and the
/// original scanner called `fs::read` on every one of them.
pub fn read_meta(path: &Path) -> std::io::Result<StlMeta> {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut triangles = 0u32;
    let mut six_volume = 0f64;

    // Edge parity, in constant memory.
    //
    // In a closed mesh every edge is shared by exactly two triangles, so
    // XOR-ing a hash of each undirected edge cancels everything and leaves
    // zero. Anything left over is a boundary. The alternative -- a set of nine
    // million edges for a three million triangle model -- is hundreds of
    // megabytes to learn one boolean.
    //
    // Vertices are identified by their exact bit patterns. A well-formed mesh
    // repeats coincident corners exactly; one that does not is not watertight
    // in any useful sense either.
    let mut edge_parity: u64 = 0;
    let vertex_key = |v: [f32; 3]| -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for c in v {
            // Normalise -0.0, which has different bits from 0.0 and would
            // otherwise split a shared corner in two.
            let bits = u64::from((if c == 0.0 { 0.0 } else { c }).to_bits());
            h = (h ^ bits).wrapping_mul(0x1000_0000_01b3);
        }
        h
    };

    for_each_triangle(path, |tri| {
        triangles += 1;

        let keys = [vertex_key(tri[0]), vertex_key(tri[1]), vertex_key(tri[2])];
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            // Undirected: the two triangles sharing an edge traverse it in
            // opposite directions, so the pair has to be ordered.
            let (lo, hi) = if keys[a] < keys[b] {
                (keys[a], keys[b])
            } else {
                (keys[b], keys[a])
            };
            let mut e = lo.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ hi;
            e ^= e >> 29;
            e = e.wrapping_mul(0xbf58_476d_1ce4_e5b9);
            e ^= e >> 32;
            edge_parity ^= e;
        }
        for v in &tri {
            for axis in 0..3 {
                if v[axis].is_finite() {
                    if v[axis] < min[axis] {
                        min[axis] = v[axis];
                    }
                    if v[axis] > max[axis] {
                        max[axis] = v[axis];
                    }
                }
            }
        }
        // Signed volume of the tetrahedron from the origin to this facet. Over
        // a closed mesh the outside contributions cancel and what remains is
        // six times the enclosed volume, wherever the origin happens to sit.
        // Accumulated in f64 because three million f32 additions drift.
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        six_volume += f64::from(a[0])
            * (f64::from(b[1]) * f64::from(c[2]) - f64::from(b[2]) * f64::from(c[1]))
            - f64::from(a[1])
                * (f64::from(b[0]) * f64::from(c[2]) - f64::from(b[2]) * f64::from(c[0]))
            + f64::from(a[2])
                * (f64::from(b[0]) * f64::from(c[1]) - f64::from(b[1]) * f64::from(c[0]));
    })?;

    if triangles == 0 || !min[0].is_finite() {
        return Ok(StlMeta::empty());
    }
    Ok(StlMeta {
        triangles,
        min,
        max,
        volume: (six_volume / 6.0).abs() as f32,
        watertight: edge_parity == 0,
    })
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
/// True for every mesh format the library can read.
pub fn is_supported_mesh(path: &Path) -> bool {
    matches!(extension_of(path).as_str(), "stl" | "obj" | "3mf")
}

pub(crate) fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Wavefront OBJ.
///
/// Unlike STL, an OBJ shares vertices between faces, so the vertex table has
/// to be held while parsing -- that is the format, not a choice. The files are
/// correspondingly smaller than the STL of the same mesh.
fn for_each_triangle_obj<F: FnMut([[f32; 3]; 3])>(
    path: &Path,
    f: &mut F,
) -> std::io::Result<()> {
    let reader = BufReader::new(File::open(path)?);
    let mut verts: Vec<[f32; 3]> = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix("v ") {
            let mut it = rest.split_whitespace();
            let mut xyz = [0f32; 3];
            let mut ok = true;
            for axis in 0..3 {
                match it.next().and_then(|t| t.parse::<f32>().ok()) {
                    Some(v) if v.is_finite() => xyz[axis] = v,
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                verts.push(xyz);
            }
        } else if let Some(rest) = trimmed.strip_prefix("f ") {
            // Each token is v, v/vt, v//vn or v/vt/vn -- only the first index
            // is geometry. Indices are 1-based, and negative counts back from
            // the most recent vertex.
            let idx: Vec<usize> = rest
                .split_whitespace()
                .filter_map(|tok| tok.split('/').next())
                .filter_map(|n| n.parse::<i64>().ok())
                .filter_map(|i| {
                    let resolved = if i > 0 {
                        i - 1
                    } else if i < 0 {
                        verts.len() as i64 + i
                    } else {
                        return None;
                    };
                    usize::try_from(resolved).ok().filter(|&r| r < verts.len())
                })
                .collect();

            // A face may be any convex polygon; fan it from the first corner.
            for k in 1..idx.len().saturating_sub(1) {
                f([verts[idx[0]], verts[idx[k]], verts[idx[k + 1]]]);
            }
        }
    }
    Ok(())
}

/// 3MF: a zip archive whose payload is an XML model part.
///
/// Only the geometry is read. Build transforms are ignored, so a file holding
/// several objects placed around a plate reports them all in their own local
/// coordinates -- fine for a preview and for a triangle count, wrong if you
/// wanted the arrangement. Single-object files, which is nearly all of them
/// outside a slicer's save format, are exact.
fn for_each_triangle_3mf<F: FnMut([[f32; 3]; 3])>(
    path: &Path,
    f: &mut F,
) -> std::io::Result<()> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    // Conventionally 3D/3dmodel.model, but the name is not mandated, so take
    // whatever .model part the archive actually carries.
    let part = archive
        .file_names()
        .find(|n| n.to_ascii_lowercase().ends_with(".model"))
        .map(|n| n.to_string())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "no model part in 3mf")
        })?;

    let entry = archive
        .by_name(&part)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    let mut reader = quick_xml::Reader::from_reader(BufReader::new(entry));
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut verts: Vec<[f32; 3]> = Vec::new();

    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        match event {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(ref e) | quick_xml::events::Event::Empty(ref e) => {
                let name = e.local_name();
                let tag = String::from_utf8_lossy(name.as_ref()).to_string();
                let attr = |key: &str| -> Option<String> {
                    e.attributes().flatten().find_map(|a| {
                        if a.key.local_name().as_ref() == key.as_bytes() {
                            Some(String::from_utf8_lossy(&a.value).to_string())
                        } else {
                            None
                        }
                    })
                };

                match tag.as_str() {
                    // Each object carries its own vertex table.
                    "mesh" => verts.clear(),
                    "vertex" => {
                        let get = |k: &str| attr(k).and_then(|v| v.parse::<f32>().ok());
                        if let (Some(x), Some(y), Some(z)) = (get("x"), get("y"), get("z")) {
                            if x.is_finite() && y.is_finite() && z.is_finite() {
                                verts.push([x, y, z]);
                            }
                        }
                    }
                    "triangle" => {
                        let get = |k: &str| attr(k).and_then(|v| v.parse::<usize>().ok());
                        if let (Some(a), Some(b), Some(c)) = (get("v1"), get("v2"), get("v3")) {
                            if a < verts.len() && b < verts.len() && c < verts.len() {
                                f([verts[a], verts[b], verts[c]]);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

pub(crate) fn for_each_triangle<F: FnMut([[f32; 3]; 3])>(path: &Path, mut f: F) -> std::io::Result<()> {
    match extension_of(path).as_str() {
        "obj" => return for_each_triangle_obj(path, &mut f),
        "3mf" => return for_each_triangle_3mf(path, &mut f),
        _ => {}
    }

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

    // ----------------------------------------------------------- 3mf ----

    fn make_3mf(part_name: &str, xml: &str) -> Vec<u8> {
        use std::io::Write;
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            w.start_file(part_name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(xml.as_bytes()).unwrap();
            w.finish().unwrap();
        }
        buf
    }

    const TETRA_3MF: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
 <resources>
  <object id="1" type="model">
   <mesh>
    <vertices>
     <vertex x="0" y="0" z="0"/>
     <vertex x="2" y="0" z="0"/>
     <vertex x="0" y="3" z="0"/>
     <vertex x="0" y="0" z="4"/>
    </vertices>
    <triangles>
     <triangle v1="0" v2="1" v3="2"/>
     <triangle v1="0" v2="1" v3="3"/>
     <triangle v1="1" v2="2" v3="3"/>
     <triangle v1="0" v2="2" v3="3"/>
    </triangles>
   </mesh>
  </object>
 </resources>
 <build><item objectid="1"/></build>
</model>"#;

    #[test]
    fn a_3mf_is_read_like_any_other_mesh() {
        let p = tmp("model.3mf", &make_3mf("3D/3dmodel.model", TETRA_3MF));
        let meta = read_meta(&p).unwrap();
        assert_eq!(meta.triangles, 4);
        assert_eq!(meta.min, [0.0, 0.0, 0.0]);
        assert_eq!(meta.max, [2.0, 3.0, 4.0]);
        std::fs::remove_file(p).ok();
    }

    /// The part name is conventional, not mandated.
    #[test]
    fn the_model_part_is_found_wherever_it_sits() {
        let p = tmp("odd.3mf", &make_3mf("Objects/object_1.model", TETRA_3MF));
        assert_eq!(read_meta(&p).unwrap().triangles, 4);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn a_3mf_previews_like_the_others() {
        let p = tmp("prev.3mf", &make_3mf("3D/3dmodel.model", TETRA_3MF));
        let mesh = read_preview(&p, 1000).unwrap();
        assert_eq!(mesh.triangles, 4);
        assert_eq!(mesh.positions.len(), 4 * 9);
        std::fs::remove_file(p).ok();
    }

    /// Each object has its own vertex table, so indices must not leak between
    /// them -- getting that wrong builds triangles out of another object's
    /// corners and the mesh comes out as noise.
    #[test]
    fn vertex_tables_do_not_leak_between_objects() {
        let two = r#"<model>
 <resources>
  <object id="1"><mesh>
    <vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
    <triangles><triangle v1="0" v2="1" v3="2"/></triangles>
  </mesh></object>
  <object id="2"><mesh>
    <vertices><vertex x="0" y="0" z="0"/><vertex x="9" y="0" z="0"/><vertex x="0" y="9" z="0"/></vertices>
    <triangles><triangle v1="0" v2="1" v3="2"/></triangles>
  </mesh></object>
 </resources>
</model>"#;
        let p = tmp("two.3mf", &make_3mf("3D/3dmodel.model", two));
        let meta = read_meta(&p).unwrap();
        assert_eq!(meta.triangles, 2);
        assert_eq!(meta.max, [9.0, 9.0, 0.0]);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn a_3mf_with_no_model_part_is_an_error_not_a_panic() {
        let p = tmp("empty.3mf", &make_3mf("doc.txt", "not a model"));
        assert!(read_meta(&p).is_err());
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn something_that_is_not_a_zip_is_an_error() {
        let p = tmp("fake.3mf", b"this is plainly not an archive");
        assert!(read_meta(&p).is_err());
        std::fs::remove_file(p).ok();
    }

    // ----------------------------------------------------------- obj ----

    #[test]
    fn an_obj_is_read_like_any_other_mesh() {
        let obj = b"# a unit-ish tetrahedron
v 0 0 0
v 2 0 0
v 0 3 0
v 0 0 4
f 1 2 3
f 1 2 4
f 2 3 4
f 1 3 4
";
        let p = tmp("mesh.obj", obj);
        let meta = read_meta(&p).unwrap();
        assert_eq!(meta.triangles, 4);
        assert_eq!(meta.min, [0.0, 0.0, 0.0]);
        assert_eq!(meta.max, [2.0, 3.0, 4.0]);
        std::fs::remove_file(p).ok();
    }

    /// OBJ faces carry texture and normal indices the geometry does not need.
    #[test]
    fn face_indices_with_texture_and_normal_parts_are_handled() {
        let obj = b"v 0 0 0
v 1 0 0
v 0 1 0
f 1/1/1 2/2/2 3/3/3
f 1//1 2//2 3//3
";
        let p = tmp("slashes.obj", obj);
        let meta = read_meta(&p).unwrap();
        assert_eq!(meta.triangles, 2);
        assert_eq!(meta.max, [1.0, 1.0, 0.0]);
        std::fs::remove_file(p).ok();
    }

    /// A quad is one face in OBJ and two triangles everywhere else.
    #[test]
    fn polygon_faces_are_fanned_into_triangles() {
        let obj = b"v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
f 1 2 3 4
";
        let p = tmp("quad.obj", obj);
        assert_eq!(read_meta(&p).unwrap().triangles, 2);
        std::fs::remove_file(p).ok();
    }

    /// Negative indices count back from the most recent vertex.
    #[test]
    fn relative_face_indices_resolve() {
        let obj = b"v 0 0 0
v 5 0 0
v 0 5 0
f -3 -2 -1
";
        let p = tmp("relative.obj", obj);
        let meta = read_meta(&p).unwrap();
        assert_eq!(meta.triangles, 1);
        assert_eq!(meta.max, [5.0, 5.0, 0.0]);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn an_obj_out_of_range_index_is_skipped_not_panicked_on() {
        let obj = b"v 0 0 0
v 1 0 0
f 1 2 99
f 1 2 0
";
        let p = tmp("bad.obj", obj);
        assert_eq!(read_meta(&p).unwrap().triangles, 0);
        std::fs::remove_file(p).ok();
    }

    // ------------------------------------------------------- preview ----

    /// A closed box of side s encloses exactly s cubed.
    fn cube(origin: [f32; 3], side: f32) -> Vec<[[f32; 3]; 3]> {
        let o = origin;
        let s = side;
        let p = [
            [o[0], o[1], o[2]],
            [o[0] + s, o[1], o[2]],
            [o[0] + s, o[1] + s, o[2]],
            [o[0], o[1] + s, o[2]],
            [o[0], o[1], o[2] + s],
            [o[0] + s, o[1], o[2] + s],
            [o[0] + s, o[1] + s, o[2] + s],
            [o[0], o[1] + s, o[2] + s],
        ];
        let faces = [
            (0, 1, 2), (0, 2, 3), (4, 6, 5), (4, 7, 6),
            (0, 5, 1), (0, 4, 5), (1, 6, 2), (1, 5, 6),
            (2, 7, 3), (2, 6, 7), (3, 4, 0), (3, 7, 4),
        ];
        faces.iter().map(|&(a, b, c)| [p[a], p[b], p[c]]).collect()
    }

    #[test]
    fn a_closed_cube_is_watertight() {
        let p = tmp("wt_cube.stl", &binary_stl(&cube([0.0, 0.0, 0.0], 20.0)));
        assert!(read_meta(&p).unwrap().watertight);
        std::fs::remove_file(p).ok();
    }

    /// The case that motivated it: an open shell still reports a volume, and
    /// that volume is not the volume of anything.
    #[test]
    fn a_cube_missing_a_face_is_not_watertight() {
        let mut faces = cube([0.0, 0.0, 0.0], 20.0);
        faces.truncate(faces.len() - 2); // drop one square, two triangles
        let p = tmp("wt_open.stl", &binary_stl(&faces));
        let meta = read_meta(&p).unwrap();
        assert!(!meta.watertight, "an open box must not claim to be closed");
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn a_single_triangle_is_not_watertight() {
        let p = tmp(
            "wt_tri.stl",
            &binary_stl(&[[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]),
        );
        assert!(!read_meta(&p).unwrap().watertight);
        std::fs::remove_file(p).ok();
    }

    /// Two separate closed solids in one file are still closed.
    #[test]
    fn two_closed_solids_in_one_file_are_watertight() {
        let mut faces = cube([0.0, 0.0, 0.0], 10.0);
        faces.extend(cube([100.0, 0.0, 0.0], 10.0));
        let p = tmp("wt_two.stl", &binary_stl(&faces));
        assert!(read_meta(&p).unwrap().watertight);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn volume_of_a_cube_is_its_side_cubed() {
        let p = tmp("vol_cube.stl", &binary_stl(&cube([0.0, 0.0, 0.0], 20.0)));
        let meta = read_meta(&p).unwrap();
        assert!(
            (meta.volume - 8000.0).abs() < 1.0,
            "expected 8000 mm3, got {}",
            meta.volume
        );
        std::fs::remove_file(p).ok();
    }

    /// The tetrahedron sum is taken from the origin, so a model sitting far
    /// from it must still report the same volume -- otherwise every part whose
    /// author did not centre it would be priced wrongly.
    #[test]
    fn volume_does_not_depend_on_where_the_model_sits() {
        let here = tmp("vol_here.stl", &binary_stl(&cube([0.0, 0.0, 0.0], 10.0)));
        let far = tmp("vol_far.stl", &binary_stl(&cube([500.0, -300.0, 120.0], 10.0)));
        let a = read_meta(&here).unwrap().volume;
        let b = read_meta(&far).unwrap().volume;
        assert!((a - 1000.0).abs() < 0.5, "got {}", a);
        assert!((a - b).abs() < 0.5, "{} vs {}", a, b);
        std::fs::remove_file(here).ok();
        std::fs::remove_file(far).ok();
    }

    #[test]
    fn volume_scales_with_the_cube_of_size() {
        let small = tmp("vol_s.stl", &binary_stl(&cube([0.0, 0.0, 0.0], 10.0)));
        let big = tmp("vol_b.stl", &binary_stl(&cube([0.0, 0.0, 0.0], 20.0)));
        let ratio = read_meta(&big).unwrap().volume / read_meta(&small).unwrap().volume;
        assert!((ratio - 8.0).abs() < 0.05, "doubling side should be 8x: {}", ratio);
        std::fs::remove_file(small).ok();
        std::fs::remove_file(big).ok();
    }

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
