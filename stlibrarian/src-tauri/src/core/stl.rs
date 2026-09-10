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
}
