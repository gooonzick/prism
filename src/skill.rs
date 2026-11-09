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

/// A value under the frontmatter's `metadata:` map (string keys and values, as the skills
/// format has it), like `metadata.prism-version`.
pub fn metadata(src: &str, key: &str) -> Option<String> {
    let mut lines = src.strip_prefix('\u{feff}').unwrap_or(src).lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return None;
    }
    let front: Vec<&str> = lines.take_while(|l| l.trim_end() != "---").collect();
    let start = front.iter().position(|l| l.trim_end() == "metadata:")?;
    let prefix = format!("{key}:");
    front[start + 1..]
        .iter()
        .take_while(|l| l.is_empty() || l.starts_with(' ') || l.starts_with('\t'))
        .find_map(|l| l.trim().strip_prefix(&prefix).map(|v| unquote(v.trim())))
}

/// Skill names as agents accept them: lowercase letters, digits and single hyphens.
pub fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Give it a name".into());
    }
    if name.len() > 64 {
        return Err("Keep the name under 64 characters".into());
    }
    let ok = name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok || name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Err("Use lowercase letters, digits and single hyphens, like pdf-tools".into());
    }
    Ok(())
}

/// The `SKILL.md` of a new skill. An empty description becomes a prompt to fill in.
pub fn template(name: &str, description: &str) -> String {
    let description = description.trim().replace('\n', " ");
    let description = if description.is_empty() { "Use when …".to_string() } else { description };
    format!("---\nname: {name}\ndescription: {}\n---\n\n# {name}\n\nWhat to do, step by step.\n", yaml_string(&description))
}

/// `s` as a YAML scalar: plain when that is safe, double-quoted otherwise.
fn yaml_string(s: &str) -> String {
    let plain = !s.is_empty()
        && !s.contains(": ")
        && !s.contains(" #")
        && !s.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c))
        && !s.ends_with(':');
    if plain { s.to_string() } else { format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_skill_convention() {
        assert!(valid_name("pdf-tools").is_ok());
        assert!(valid_name("v2").is_ok());
        for bad in ["", "PDF", "pdf tools", "-x", "x-", "a--b"] {
            assert!(valid_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn descriptions_are_quoted_only_when_yaml_needs_it() {
        assert_eq!(yaml_string("Use when testing"), "Use when testing");
        assert_eq!(yaml_string("Use when: testing"), "\"Use when: testing\"");
        assert_eq!(parse(&format!("---\ndescription: {}\n---\n", yaml_string("Say \"hi\": now"))).description.as_deref(), Some("Say \"hi\": now"));
    }

    #[test]
    fn a_new_skill_reads_back() {
        let m = parse(&template("pdf-tools", "  Use when\nPDFs: ok "));
        assert_eq!(m.name.as_deref(), Some("pdf-tools"));
        assert_eq!(m.description.as_deref(), Some("Use when PDFs: ok"));
        assert_eq!(parse(&template("x", "")).description.as_deref(), Some("Use when …"));
    }

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
    fn reads_metadata_values() {
        let src = "---\nname: prism\nmetadata:\n  author: me\n  prism-version: \"1.2.3\"\nlicense: MIT\n---\nversion: 9\n";
        assert_eq!(metadata(src, "prism-version").as_deref(), Some("1.2.3"));
        assert_eq!(metadata(src, "author").as_deref(), Some("me"));
        assert_eq!(metadata(src, "license"), None, "top-level keys are not metadata");
        assert_eq!(metadata("---\nname: x\nprism-version: 1\n---\n", "prism-version"), None);
        assert_eq!(metadata("# no frontmatter\nmetadata:\n  a: b\n", "a"), None);
    }

    #[test]
    fn a_file_without_frontmatter_is_all_body() {
        let m = parse("# Just text");
        assert_eq!(m.name, None);
        assert_eq!(m.body, "# Just text");
    }
}
