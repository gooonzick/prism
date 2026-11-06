//! skills.sh: search its index over HTTP, install through its CLI (`skills add`).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::agents::{Paths, Scope};

const API: &str = "https://skills.sh";

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RemoteSkill {
    /// `owner/repo/skill`.
    pub id: String,
    /// `owner/repo`: what `skills add` takes.
    #[serde(default)]
    pub source: String,
    #[serde(rename = "skillId", default)]
    pub skill_id: String,
    pub name: String,
    #[serde(default)]
    pub installs: u64,
}

impl RemoteSkill {
    pub fn url(&self) -> String {
        format!("{API}/{}", self.id)
    }

    /// The `--skill` value: the skill's id within its repo.
    pub fn skill_arg(&self) -> &str {
        if self.skill_id.is_empty() { &self.name } else { &self.skill_id }
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    skills: Vec<RemoteSkill>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).user_agent(concat!("skills-explorer/", env!("CARGO_PKG_VERSION"))).build()
}

/// Searches skills.sh. An empty query returns nothing.
pub fn search(query: &str) -> Result<Vec<RemoteSkill>, String> {
    let q = query.trim();
    if q.len() < 2 {
        return Ok(Vec::new());
    }
    let resp: SearchResponse = agent()
        .get(&format!("{API}/api/search"))
        .query("q", q)
        .query("limit", "50")
        .call()
        .map_err(|e| format!("search failed: {e}"))?
        .into_json()
        .map_err(|e| format!("unexpected answer from skills.sh: {e}"))?;
    Ok(resp.skills)
}

/// The skill's `SKILL.md` from GitHub, trying the layouts repos commonly use.
pub fn fetch_manifest(skill: &RemoteSkill) -> Result<String, String> {
    if skill.source.split('/').count() != 2 {
        return Err("only GitHub sources have a preview".into());
    }
    let id = skill.skill_arg();
    let candidates = [
        format!("skills/{id}/SKILL.md"),
        format!("{id}/SKILL.md"),
        format!(".claude/skills/{id}/SKILL.md"),
        format!(".agents/skills/{id}/SKILL.md"),
        format!("plugins/{id}/SKILL.md"),
        "SKILL.md".to_string(),
    ];
    let http = agent();
    for path in candidates {
        let url = format!("https://raw.githubusercontent.com/{}/HEAD/{path}", skill.source);
        if let Ok(resp) = http.get(&url).call()
            && let Ok(text) = resp.into_string() {
                let m = crate::skill::parse(&text);
                // A repo's root SKILL.md only counts when it is this skill.
                if path != "SKILL.md" || m.name.as_deref() == Some(id) {
                    return Ok(text);
                }
            }
    }
    Err("SKILL.md not found in the usual places; open it on skills.sh".into())
}

/// How to run the `skills` CLI: the first of pnpx, npx and bunx found on `PATH`.
pub fn runner() -> Option<(PathBuf, Vec<String>)> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    // A Finder-launched app gets a short PATH; look where Node usually lives too.
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    if let Some(home) = dirs::home_dir() {
        dirs.extend([".bun/bin", ".volta/bin", ".local/bin"].map(|d| home.join(d)));
    }
    let find = |exe: &str| dirs.iter().map(|d| d.join(exe)).find(|p| p.is_file());
    if let Some(p) = find("pnpx") {
        return Some((p, vec!["--silent".into(), "skills".into()]));
    }
    if let Some(p) = find("npx") {
        return Some((p, vec!["--yes".into(), "skills".into()]));
    }
    find("bunx").map(|p| (p, vec!["skills".into()]))
}

/// The `skills add` arguments that put `source` (one skill of it, or all) into `scope`'s
/// `.agents/skills`. Codex's folder in the CLI's own table is `.agents/skills`, globally
/// and per project, so naming it installs into the canonical store and nowhere else; the
/// app then links the skill to the agents the user picked.
pub fn add_args(source: &str, skill: Option<&str>, scope: &Scope) -> Vec<String> {
    let mut args = vec!["add".to_string(), source.to_string(), "--skill".into(), skill.unwrap_or("*").to_string()];
    args.extend(["--agent".into(), "codex".into(), "--yes".into()]);
    if scope.is_global() {
        args.push("--global".into());
    }
    args
}

/// Runs `skills add` and returns its output, ANSI codes stripped.
pub fn install(source: &str, skill: Option<&str>, scope: &Scope, paths: &Paths) -> Result<String, String> {
    let (exe, mut args) = runner().ok_or("Neither pnpx, npx nor bunx was found. Install Node.js to add skills from skills.sh.")?;
    args.extend(add_args(source, skill, scope));
    let cwd = scope.root(paths);
    let out = Command::new(&exe)
        .args(&args)
        .current_dir(&cwd)
        .env("PATH", path_with(exe.parent()))
        .env("DISABLE_TELEMETRY", "1")
        .output()
        .map_err(|e| format!("could not run {}: {e}", exe.display()))?;
    let text = strip_ansi(&format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    if out.status.success() { Ok(text) } else { Err(text) }
}

/// `PATH` with the runner's folder first, so its `node` is found from a Finder launch.
fn path_with(dir: Option<&Path>) -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = dir.map(Path::to_path_buf).into_iter().collect();
    dirs.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    std::env::join_paths(dirs).unwrap_or_default()
}

/// Parses what a person types as a source: `owner/repo`, `owner/repo@skill`, a GitHub URL,
/// or a skills.sh URL (`https://skills.sh/owner/repo/skill`).
pub fn parse_source(input: &str) -> Option<(String, Option<String>)> {
    let s = input.trim().trim_end_matches('/');
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_prefix("https://skills.sh/").or_else(|| s.strip_prefix("https://www.skills.sh/")) {
        let parts: Vec<&str> = rest.split('/').collect();
        return match parts.as_slice() {
            [o, r] => Some((format!("{o}/{r}"), None)),
            [o, r, skill, ..] => Some((format!("{o}/{r}"), Some(skill.to_string()))),
            _ => None,
        };
    }
    if s.contains("://") || s.starts_with("git@") {
        return Some((s.to_string(), None));
    }
    match s.split_once('@') {
        Some((src, skill)) if !skill.is_empty() => Some((src.to_string(), Some(skill.to_string()))),
        _ => Some((s.to_string(), None)),
    }
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if c != '\r' {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_results() {
        let json = r#"{"query":"react","skills":[{"id":"vercel-labs/agent-skills/react","source":"vercel-labs/agent-skills","skillId":"react","name":"react","installs":12}],"count":1}"#;
        let r: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.skills[0].source, "vercel-labs/agent-skills");
        assert_eq!(r.skills[0].skill_arg(), "react");
        assert_eq!(r.skills[0].url(), "https://skills.sh/vercel-labs/agent-skills/react");
    }

    #[test]
    fn install_args_target_the_canonical_store() {
        assert_eq!(add_args("o/r", Some("x"), &Scope::Global), ["add", "o/r", "--skill", "x", "--agent", "codex", "--yes", "--global"]);
        assert_eq!(add_args("o/r", None, &Scope::Project("/p".into())), ["add", "o/r", "--skill", "*", "--agent", "codex", "--yes"]);
    }

    #[test]
    fn parses_sources_people_paste() {
        let p = |s| parse_source(s);
        assert_eq!(p("o/r"), Some(("o/r".into(), None)));
        assert_eq!(p("o/r@skill"), Some(("o/r".into(), Some("skill".into()))));
        assert_eq!(p("https://skills.sh/o/r/skill"), Some(("o/r".into(), Some("skill".into()))));
        assert_eq!(p("https://github.com/o/r"), Some(("https://github.com/o/r".into(), None)));
        assert_eq!(p("  "), None);
    }

    #[test]
    fn strips_ansi() {
        assert_eq!(strip_ansi("\u{1b}[1mBold\u{1b}[0m\r\n\u{1b}[?25l"), "Bold\n");
    }
}
