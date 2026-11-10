//! Screens are built from the design system only: `src/ui/` holds no type style, colour, radius
//! or spacing of its own. Raw values live in `src/ds/`; DESIGN.md has the rules.

use std::fs;
use std::path::Path;

/// Text that picks a style, a colour or a radius from rune-kit directly.
const FORBIDDEN: [&str; 4] = ["TextStyle::new", "k.ty.", "k.c.", "k.radius."];

/// Spacing and radius methods that must take a token (`space::`, `radius::`, `size::`).
const SPACING: [&str; 15] = ["gap", "padding", "px", "py", "pt", "pb", "pl", "pr", "mt", "mb", "ml", "mr", "mx", "my", "radius"];

/// Whether `line` calls a spacing or radius method with a number:
/// `\.(gap|padding|px|…|radius)\(\s*-?[0-9]`.
fn raw_spacing(line: &str) -> bool {
    line.match_indices('.').any(|(i, _)| {
        let rest = &line[i + 1..];
        SPACING.iter().any(|m| {
            rest.strip_prefix(m).and_then(|r| r.strip_prefix('(')).is_some_and(|r| {
                let r = r.trim_start();
                r.strip_prefix('-').unwrap_or(r).starts_with(|c: char| c.is_ascii_digit())
            })
        })
    })
}

#[test]
fn the_pattern() {
    for bad in [".gap(6.0)", "x.px( 12.0)", ".mx(-3.0)", ".radius(8.0)", "row().padding(16.0).gap(space::S2)"] {
        assert!(raw_spacing(bad), "{bad} is raw");
    }
    for ok in [".gap(space::S2)", ".mx(-size::FOCUS_RING)", ".header_gap(4.0)", ".w(96.0)", ".h(1.0)", ".pad(4.0)"] {
        assert!(!raw_spacing(ok), "{ok} is allowed");
    }
}

#[test]
#[ignore = "on once every screen is on ds (Task 11)"]
fn no_raw_values_in_ui() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
    let mut files: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "rs")).collect();
    files.sort();
    assert!(!files.is_empty(), "no screens in {}", dir.display());
    let mut found = Vec::new();
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        for (n, line) in text.lines().enumerate() {
            if FORBIDDEN.iter().any(|f| line.contains(f)) || raw_spacing(line) {
                found.push(format!("src/ui/{name}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "raw values in src/ui (use ds tokens and blocks, see DESIGN.md):\n{}", found.join("\n"));
}
