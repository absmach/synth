// SPDX-License-Identifier: Apache-2.0

//! Import a single-part KiCad symbol/footprint pair out of a ZIP
//! archive — the format SnapEDA and UltraLibrarian both produce when
//! a user chooses "KiCad" as the export target (a `.kicad_sym` file
//! plus a `<name>.pretty/<name>.kicad_mod` footprint, sometimes with
//! a `.step`/`.wrl` 3D model alongside).
//!
//! IMPORTANT: this is original, clean-room code. It reimplements the
//! documented zip-contents layout from scratch — it does NOT derive
//! from `Import-LIB-KiCad-Plugin` (Steffen-W, GPL-3.0), the existing
//! community tool that imports these same SnapEDA/UltraLibrarian
//! zips into KiCad. Only the two vendors' own published import
//! documentation (SnapEDA's guidance: extract the zip as-is and keep
//! the folder structure intact) and generic ZIP/`.kicad_sym` format
//! knowledge are relied upon — same posture as `easyeda.rs`'s
//! relationship to `easyeda2kicad.py` (AGPL-3.0): approach and public
//! file-format facts only, no code or internal structure copied.
//!
//! Both vendors' Terms of Service separately prohibit
//! automated/scripted access to their *sites* (robots, scrapers, bulk
//! API use without a signed agreement — this is about not scraping
//! snapeda.com/ultralibrarian.com, unrelated to the GPL question
//! above), so Synth never talks to either site directly: the user
//! downloads and exports the zip themselves through their own browser
//! session, and this module only reads the file they already have on
//! disk. See `registry/CREDITS.md` for the full licensing posture.
//!
//! Like [`crate::kicad_lib_loader`], we don't parse full KiCad
//! s-expression grammar — we scan for the handful of tokens we
//! actually need (`(symbol "..."`, `(pin ...)`).

use std::io::Read;
use std::path::Path;

use crate::kicad_lib_loader::{first_symbol_name, physical_pins_from_source, PhysicalPin};

/// Everything extracted from one SnapEDA/UltraLibrarian-style export
/// zip: the part's pin inventory (from its `.kicad_sym`) and, when
/// present, the raw text of its `.kicad_mod` footprint file.
#[derive(Debug, Clone)]
pub struct KicadZipImport {
    /// Symbol name as declared inside the `.kicad_sym` file — used as
    /// the default part id.
    pub symbol_name: String,
    pub pins: Vec<PhysicalPin>,
    /// Raw `.kicad_mod` text, if the zip contained exactly one
    /// footprint. `None` when no `.kicad_mod` entry was found — the
    /// caller falls back to a synthesized footprint, same as any
    /// other part with no real footprint.
    pub footprint_text: Option<String>,
    /// Footprint file stem (without `.kicad_mod`), used as the
    /// on-disk module name when writing it into the Tier-2 registry.
    pub footprint_name: Option<String>,
}

/// Parse a SnapEDA/UltraLibrarian-style KiCad export zip from raw
/// bytes (already read from disk by the caller).
///
/// # Errors
/// Returns `Err` when the archive can't be opened, contains no
/// `.kicad_sym` entry, or that entry has no top-level symbol with at
/// least one physical pin. A missing *footprint* is not an error —
/// `footprint_text`/`footprint_name` are `None` and the caller
/// decides how to handle it (synthesized fallback, same as any other
/// footprint-less part).
pub fn parse_kicad_zip(bytes: &[u8]) -> Result<KicadZipImport, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("could not open zip archive: {e}"))?;

    let mut sym_text: Option<String> = None;
    let mut footprint: Option<(String, String)> = None;

    for i in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(i) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let lower = name.to_ascii_lowercase();

        if lower.ends_with(".kicad_sym") && sym_text.is_none() {
            let mut buf = String::new();
            if entry.read_to_string(&mut buf).is_ok() {
                sym_text = Some(buf);
            }
        } else if lower.ends_with(".kicad_mod") && footprint.is_none() {
            let mut buf = String::new();
            if entry.read_to_string(&mut buf).is_ok() {
                let stem = Path::new(&name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&name)
                    .to_string();
                footprint = Some((stem, buf));
            }
        }
    }

    let sym_text = sym_text.ok_or("zip contains no .kicad_sym file")?;
    let symbol_name = first_symbol_name(&sym_text)
        .ok_or("the .kicad_sym file has no top-level (symbol \"...\") block")?;
    let pins = physical_pins_from_source(&sym_text, &symbol_name)
        .ok_or("no physical pins found in the symbol (unsupported/empty symbol?)")?;

    Ok(KicadZipImport {
        symbol_name,
        pins,
        footprint_name: footprint.as_ref().map(|(n, _)| n.clone()),
        footprint_text: footprint.map(|(_, t)| t),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a minimal single-part `.kicad_sym` + `.kicad_mod` zip in
    /// memory, matching the shape SnapEDA/UltraLibrarian document
    /// (symbol at zip root, footprint under a `<lib>.pretty/` dir).
    fn sample_zip() -> Vec<u8> {
        let sym = r#"(kicad_symbol_lib (version 20211014) (generator kicad_symbol_editor)
  (symbol "MyPart"
    (property "Reference" "U" (at 0 0 0))
    (symbol "MyPart_0_1"
    )
    (symbol "MyPart_1_1"
      (pin power_in line (at -5.08 0 0) (length 2.54)
        (name "VDD" (effects (font (size 1.27 1.27))))
        (number "1" (effects (font (size 1.27 1.27))))
      )
      (pin passive line (at 5.08 0 180) (length 2.54)
        (name "IO1" (effects (font (size 1.27 1.27))))
        (number "2" (effects (font (size 1.27 1.27))))
      )
    )
  )
)
"#;
        let modu = r#"(footprint "MyPart" (version 20211014) (generator pcbnew) (layer "F.Cu")
  (pad "1" smd rect (at -1 0) (size 0.6 0.6) (layers "F.Cu" "F.Paste" "F.Mask"))
  (pad "2" smd rect (at 1 0) (size 0.6 0.6) (layers "F.Cu" "F.Paste" "F.Mask"))
)
"#;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            zw.start_file("MyPart.kicad_sym", opts).unwrap();
            zw.write_all(sym.as_bytes()).unwrap();
            zw.start_file("MyPart.pretty/MyPart.kicad_mod", opts)
                .unwrap();
            zw.write_all(modu.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn parses_symbol_name_pins_and_footprint_from_zip() {
        let import = parse_kicad_zip(&sample_zip()).expect("valid sample zip");
        assert_eq!(import.symbol_name, "MyPart");
        assert_eq!(import.pins.len(), 2);
        assert_eq!(import.pins[0].number, "1");
        assert_eq!(import.pins[0].name, "VDD");
        assert_eq!(import.pins[0].electrical_type, "power_in");
        assert_eq!(import.footprint_name.as_deref(), Some("MyPart"));
        assert!(import.footprint_text.unwrap().contains("(pad \"1\""));
    }

    #[test]
    fn rejects_a_zip_with_no_kicad_sym() {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            zw.start_file("readme.txt", opts).unwrap();
            zw.write_all(b"no symbol here").unwrap();
            zw.finish().unwrap();
        }
        let err = parse_kicad_zip(&buf.into_inner()).unwrap_err();
        assert!(err.contains("no .kicad_sym"));
    }

    #[test]
    fn missing_footprint_is_not_an_error() {
        let sym_only = {
            let mut buf = std::io::Cursor::new(Vec::new());
            let mut zw = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            zw.start_file("MyPart.kicad_sym", opts).unwrap();
            zw.write_all(
                br#"(kicad_symbol_lib (version 20211014)
  (symbol "MyPart"
    (symbol "MyPart_1_1"
      (pin input line (at 0 0 0) (length 2.54)
        (name "A" (effects (font (size 1.27 1.27))))
        (number "1" (effects (font (size 1.27 1.27))))
      )
    )
  )
)
"#,
            )
            .unwrap();
            zw.finish().unwrap();
            buf.into_inner()
        };
        let import = parse_kicad_zip(&sym_only).expect("symbol-only zip still parses");
        assert!(import.footprint_text.is_none());
        assert!(import.footprint_name.is_none());
        assert_eq!(import.pins.len(), 1);
    }
}
