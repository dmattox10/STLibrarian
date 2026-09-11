//! Sliced output -- the files a printer can actually eat.
//!
//! A model is not printable. The gap between an STL and a print is slicing,
//! and slicing is supports, orientation and profiles: an enormous problem that
//! belongs to PrusaSlicer and Lychee, not here.
//!
//! What a library can do is keep the result. "The Throk body, sliced for the
//! Mars at 50 microns, that we know works" is the artifact a workshop actually
//! reuses, and once it is a first-class thing, printing something again
//! exactly as it went last time stops being an act of memory.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// How much of each end of the file to read looking for metadata.
///
/// Cura writes its summary at the top; PrusaSlicer and Orca append their whole
/// configuration at the bottom. A gcode file runs to tens of megabytes and the
/// middle is nothing but moves.
const WINDOW: usize = 96 * 1024;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SliceMeta {
    pub layer_height: Option<f64>,
    pub print_time_secs: Option<i64>,
    /// Grams, when the slicer said so.
    pub material_grams: Option<f64>,
    pub printer: Option<String>,
}

/// Extensions that mean "a printer could run this".
pub fn is_sliced(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        // FDM
        "gcode" | "gco" | "g" | "bgcode"
        // Resin, various vendors
        | "ctb" | "cbddlp" | "photon" | "pws" | "pwmx" | "pwmo" | "goo" | "sl1" | "sl1s"
    )
}

/// Deliberately not `3mf`: that is a mesh container that some slicers also use
/// as a project format, so treating it as sliced output would hide models.
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn format_of(path: &Path) -> String {
    extension(path)
}

/// True when the format is text we can learn something from.
fn is_text_gcode(path: &Path) -> bool {
    matches!(extension(path).as_str(), "gcode" | "gco" | "g")
}

/// Read what the slicer wrote about its own output.
///
/// Only text gcode is parsed. The resin formats are vendor binaries whose
/// headers are versioned and sometimes obfuscated; guessing at them would
/// produce confident wrong numbers, which is worse than none.
pub fn read_meta(path: &Path) -> std::io::Result<SliceMeta> {
    if !is_text_gcode(path) {
        return Ok(SliceMeta::default());
    }

    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();

    let mut text = String::new();
    let mut head = vec![0u8; WINDOW.min(len as usize)];
    file.read_exact(&mut head)?;
    text.push_str(&String::from_utf8_lossy(&head));

    if len as usize > WINDOW * 2 {
        file.seek(SeekFrom::End(-(WINDOW as i64)))?;
        let mut tail = vec![0u8; WINDOW];
        if file.read_exact(&mut tail).is_ok() {
            text.push('\n');
            text.push_str(&String::from_utf8_lossy(&tail));
        }
    }

    Ok(parse(&text))
}

/// Pull metadata out of slicer comments.
///
/// Every slicer invented its own spelling, so this matches several rather than
/// insisting on one. Anything unrecognised is left as None -- an empty field
/// is honest, a guessed one is not.
pub fn parse(text: &str) -> SliceMeta {
    let mut meta = SliceMeta::default();

    for raw in text.lines() {
        let line = raw.trim();
        if !line.starts_with(';') {
            continue;
        }
        let body = line.trim_start_matches(';').trim();
        let lower = body.to_ascii_lowercase();

        // Cura: ";TIME:8036" -- already seconds.
        if meta.print_time_secs.is_none() {
            if let Some(rest) = lower.strip_prefix("time:") {
                meta.print_time_secs = rest.trim().parse::<i64>().ok();
            }
        }
        // PrusaSlicer / Orca: "estimated printing time (normal mode) = 2h 13m 5s"
        if meta.print_time_secs.is_none() && lower.contains("estimated printing time") {
            if let Some((_, value)) = body.split_once('=') {
                meta.print_time_secs = parse_duration(value);
            }
        }

        if meta.layer_height.is_none() {
            for key in ["layer_height", "layer height"] {
                if let Some(v) = value_after(&lower, key) {
                    meta.layer_height = v.parse::<f64>().ok().filter(|h| *h > 0.0 && *h < 5.0);
                    break;
                }
            }
        }

        if meta.material_grams.is_none() {
            // PrusaSlicer: "filament used [g] = 12.34"; Orca uses the same.
            for key in ["filament used [g]", "total filament used [g]", "filament_used_g"] {
                if let Some(v) = value_after(&lower, key) {
                    meta.material_grams = v
                        .split(',')
                        .filter_map(|part| part.trim().parse::<f64>().ok())
                        .reduce(|a, b| a + b)
                        .filter(|g| *g > 0.0);
                    break;
                }
            }
        }

        if meta.printer.is_none() {
            for key in ["printer_model", "machine_name", "printer_settings_id", "printer_notes"] {
                if let Some(v) = value_after(&lower, key) {
                    let cleaned = v.trim().to_string();
                    if !cleaned.is_empty() && cleaned.len() < 80 {
                        // Recover the original casing from the untouched line.
                        let start = body.len().saturating_sub(cleaned.len());
                        meta.printer = Some(body[start..].trim().to_string());
                    }
                    break;
                }
            }
        }
    }
    meta
}

/// The value after `key`, whether the slicer used `=` or `:`.
fn value_after<'a>(lower: &'a str, key: &str) -> Option<&'a str> {
    let at = lower.find(key)?;
    let rest = &lower[at + key.len()..];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').or_else(|| rest.strip_prefix(':'))?;
    Some(rest.trim())
}

/// "2h 13m 5s" -> seconds. Also copes with "13m 5s" and bare "45s".
fn parse_duration(value: &str) -> Option<i64> {
    let mut total = 0i64;
    let mut number = String::new();
    let mut saw_any = false;

    for ch in value.chars() {
        if ch.is_ascii_digit() {
            number.push(ch);
            continue;
        }
        let scale = match ch.to_ascii_lowercase() {
            'd' => 86_400,
            'h' => 3_600,
            'm' => 60,
            's' => 1,
            _ => {
                continue;
            }
        };
        if let Ok(n) = number.parse::<i64>() {
            total += n * scale;
            saw_any = true;
        }
        number.clear();
    }
    if saw_any {
        Some(total)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_what_a_printer_can_run() {
        for good in ["/a/plate.gcode", "/a/PLATE.GCODE", "/a/x.ctb", "/a/x.goo", "/a/x.bgcode"] {
            assert!(is_sliced(Path::new(good)), "{}", good);
        }
        // A 3mf is a mesh container that some slicers also save projects in;
        // treating it as output would hide models from the library.
        for bad in ["/a/model.stl", "/a/model.3mf", "/a/notes.txt", "/a/g"] {
            assert!(!is_sliced(Path::new(bad)), "{}", bad);
        }
    }

    #[test]
    fn reads_cura_output() {
        let meta = parse(
            ";FLAVOR:Marlin\n;TIME:8036\n;Layer height: 0.2\n;MACHINE_NAME:Ender 3\nG28\n",
        );
        assert_eq!(meta.print_time_secs, Some(8036));
        assert_eq!(meta.layer_height, Some(0.2));
        assert_eq!(meta.printer.as_deref(), Some("Ender 3"));
    }

    #[test]
    fn reads_prusaslicer_output() {
        let meta = parse(
            "; estimated printing time (normal mode) = 2h 13m 5s\n\
             ; filament used [g] = 12.34\n\
             ; layer_height = 0.15\n\
             ; printer_model = MK4\n",
        );
        assert_eq!(meta.print_time_secs, Some(2 * 3600 + 13 * 60 + 5));
        assert_eq!(meta.layer_height, Some(0.15));
        assert_eq!(meta.material_grams, Some(12.34));
        assert_eq!(meta.printer.as_deref(), Some("MK4"));
    }

    /// Multi-material slicers report a figure per extruder.
    #[test]
    fn filament_across_several_extruders_is_summed() {
        let meta = parse("; filament used [g] = 10.0, 2.5, 0.5\n");
        assert_eq!(meta.material_grams, Some(13.0));
    }

    #[test]
    fn durations_without_every_unit_still_parse() {
        assert_eq!(parse_duration(" 45s"), Some(45));
        assert_eq!(parse_duration("13m 5s"), Some(785));
        assert_eq!(parse_duration("1d 2h"), Some(93_600));
        assert_eq!(parse_duration("no digits here"), None);
    }

    /// An implausible layer height means the line was not what we thought.
    #[test]
    fn nonsense_values_are_refused_rather_than_stored() {
        assert_eq!(parse("; layer_height = 0\n").layer_height, None);
        assert_eq!(parse("; layer_height = 900\n").layer_height, None);
        assert_eq!(parse("; filament used [g] = 0\n").material_grams, None);
    }

    #[test]
    fn a_file_with_no_comments_yields_nothing_rather_than_guesses() {
        let meta = parse("G28\nG1 X10 Y10 F3000\n");
        assert_eq!(meta, SliceMeta::default());
    }

    #[test]
    fn resin_binaries_are_not_guessed_at() {
        // No parser claims to understand these, so nothing is invented.
        let dir = std::env::temp_dir();
        let p = dir.join(format!("stlib_slice_{}.ctb", std::process::id()));
        std::fs::write(&p, b"\x12\x34\x56\x78 binary vendor header").unwrap();
        assert_eq!(read_meta(&p).unwrap(), SliceMeta::default());
        std::fs::remove_file(p).ok();
    }
}
