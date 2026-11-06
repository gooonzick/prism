//! `SKILL.md`: YAML frontmatter with `name` and `description`, then the instructions.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Manifest {
    pub name: Option<String>,
    pub description: Option<String>,
    /// The Markdown after the frontmatter.
    pub body: String,
}

/// The skill's manifest file in `dir`, if it has one (`SKILL.md`, or `skill.md`).
pub fn manifest_path(dir: &Path) -> Option<PathBuf> {
    ["SKILL.md", "skill.md", "Skill.md"].iter().map(|f| dir.join(f)).find(|p| p.is_file())
}

pub fn read(dir: &Path) -> Option<Manifest> {
    let src = std::fs::read_to_string(manifest_path(dir)?).ok()?;
    Some(parse(&src))
}

/// Reads the frontmatter's top-level `name` and `description`, including YAML's folded (`>`)
/// and literal (`|`) block scalars and quoted strings. Everything else is ignored.
pub fn parse(src: &str) -> Manifest {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut lines = src.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return Manifest { body: src.to_string(), ..Manifest::default() };
    }
    let mut front = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        front.push(line);
    }
    if !closed {
        return Manifest { body: src.to_string(), ..Manifest::default() };
    }
    let body = lines.collect::<Vec<_>>().join("\n").trim_start_matches('\n').to_string();
    Manifest { name: field(&front, "name"), description: field(&front, "description"), body }
}

fn field(front: &[&str], key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    let i = front.iter().position(|l| l.starts_with(&prefix))?;
    let value = front[i][prefix.len()..].trim();
    let block = value.chars().next().filter(|c| *c == '>' || *c == '|');
    if value.is_empty() || block.is_some() {
        let continuation: Vec<&str> = front[i + 1..]
            .iter()
            .take_while(|l| l.is_empty() || l.starts_with(' ') || l.starts_with('\t'))
            .map(|l| l.trim())
            .collect();
        let sep = if block == Some('|') { "\n" } else { " " };
        let joined = continuation.join(sep).trim().to_string();
        return (!joined.is_empty()).then_some(joined);
    }
    Some(unquote(value))
}

fn unquote(v: &str) -> String {
    let b = v.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' && b[b.len() - 1] == b'"') {
        return v[1..v.len() - 1].replace("\\\"", "\"").replace("\\n", "\n");
    }
    if b.len() >= 2 && (b[0] == b'\'' && b[b.len() - 1] == b'\'') {
        return v[1..v.len() - 1].replace("''", "'");
    }
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plain_frontmatter_and_body() {
        let m = parse("---\nname: stop-slop\ndescription: Remove AI writing patterns.\n---\n\n# Stop slop\nBody");
        assert_eq!(m.name.as_deref(), Some("stop-slop"));
        assert_eq!(m.description.as_deref(), Some("Remove AI writing patterns."));
        assert_eq!(m.body, "# Stop slop\nBody");
    }

    #[test]
    fn reads_quoted_and_folded_descriptions() {
        let m = parse("---\nname: 'it''s'\ndescription: >\n  Use when\n  testing.\nlicense: MIT\n---\n");
        assert_eq!(m.name.as_deref(), Some("it's"));
        assert_eq!(m.description.as_deref(), Some("Use when testing."));
        let m = parse("---\ndescription: \"Say \\\"hi\\\"\"\n---\n");
        assert_eq!(m.description.as_deref(), Some("Say \"hi\""));
    }

    #[test]
    fn a_file_without_frontmatter_is_all_body() {
        let m = parse("# Just text");
        assert_eq!(m.name, None);
        assert_eq!(m.body, "# Just text");
    }
}
