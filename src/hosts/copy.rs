//! Copies between machines. Both ends are [`Runner`]s (`Here` for this machine), so a copy
//! from a host to this machine, or between two hosts, goes the same way: the target plans it
//! (`skills receive` / `mcp receive`) and, with the plan's id, does it with what the source
//! sends on stdin. Secrets travel only there, never in a command line.

use std::path::Path;

use serde_json::Value;

use super::runner::{Output, Runner, quote};
use crate::agents::Scope;
use crate::mcp::agents::MCP_AGENTS;
use crate::mcp::scan::{McpServer, Status};

/// How plans and toasts name this machine.
pub fn this_machine() -> &'static str {
    if cfg!(target_os = "macos") { "This Mac" } else { "This computer" }
}

/// Copies the skill `name`, whose folder on `from` is `source_dir`, into `dest` on `to`.
/// Without `apply` it returns `to`'s plan; with the plan's id, `from` packs the folder and `to`
/// unpacks it, checked against the hash `from` gave. The result is `to`'s JSON (exit 4's
/// `plan_changed` included), each action line ending " on {to_label}".
#[allow(clippy::too_many_arguments)]
pub fn copy_skill(
    from: &dyn Runner,
    from_label: &str,
    from_scope: &Scope,
    source_dir: &Path,
    name: &str,
    to: &dyn Runner,
    to_label: &str,
    dest: &Scope,
    apply: Option<&str>,
) -> Result<Value, String> {
    let mut args = strings(&["wire", "skill-hash", name, "--json"]);
    args.extend(scope_args(from_scope));
    let hash = answer(from.prism(&args, None)?, from_label)?["hash"].as_u64().ok_or_else(|| format!("{name} has no folder on {from_label}"))?;
    let mut args = strings(&["skills", "receive", name]);
    match dest {
        Scope::Global => args.push("--to-global".into()),
        Scope::Project(dir) => args.extend(["--to-project".into(), dir.to_string_lossy().into_owned()]),
    }
    args.extend(strings(&["--source-hash", &hash.to_string(), "--from", from_label, "--json"]));
    let tar = match apply {
        Some(id) => {
            args.extend(strings(&["--apply", "--plan-id", id]));
            Some(pack(from, from_label, source_dir, name)?)
        }
        None => None,
    };
    // On a conflict the target reads no stdin: the runners ignore a closed pipe.
    let mut plan = answer(to.prism(&args, tar)?, to_label)?;
    on_machine(&mut plan, to_label);
    Ok(plan)
}

/// Copies the MCP server `name` from `from_scope` on `from` (`from_agent`'s copy, needed when
/// the copies differ) to `agents` in `to`'s global setup, as `mcp add` would add it there: the
/// fields Prism models, not an agent's other keys. The unmasked definition goes on stdin for
/// the plan and for the apply. Returns `to`'s JSON as [`copy_skill`] does; its warnings name
/// what may not exist on `to`.
#[allow(clippy::too_many_arguments)]
pub fn copy_server(
    from: &dyn Runner,
    from_label: &str,
    name: &str,
    from_agent: Option<usize>,
    from_scope: &Scope,
    to: &dyn Runner,
    to_label: &str,
    agents: &[usize],
    apply: Option<&str>,
) -> Result<Value, String> {
    let mut args = strings(&["wire", "server", name, "--json"]);
    args.extend(scope_args(from_scope));
    // serde's errors quote the input, and the input holds secrets: say only what failed.
    let found: McpServer = serde_json::from_value(answer(from.prism(&args, None)?, from_label)?).map_err(|_| format!("{from_label} sent a definition of {name} Prism can't read"))?;
    let copy = match from_agent {
        Some(a) => found.copies.iter().find(|c| c.agent == a).ok_or_else(|| format!("{} has no copy of {name} on {from_label}", MCP_AGENTS[a].name))?,
        None if found.status == Status::Differs => return Err("copies differ; pick one with --from".into()),
        None => found.copies.first().ok_or_else(|| format!("No MCP server named {name} on {from_label}"))?,
    };
    let definition = serde_json::to_vec(&copy.server).map_err(|_| format!("can't send {name}"))?;
    let home = home_of(from, from_label)?;
    let mut args = strings(&["mcp", "receive", name, "--to"]);
    args.extend(agents.iter().map(|&a| MCP_AGENTS[a].id.to_string()));
    args.extend(strings(&["--from", from_label, "--source-home", &home, "--json"]));
    if let Some(id) = apply {
        args.extend(strings(&["--apply", "--plan-id", id]));
    }
    let mut plan = answer(to.prism(&args, Some(definition))?, to_label)?;
    on_machine(&mut plan, to_label);
    Ok(plan)
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

fn scope_args(scope: &Scope) -> Vec<String> {
    match scope {
        Scope::Global => vec![],
        Scope::Project(dir) => vec!["--project".into(), dir.to_string_lossy().into_owned()],
    }
}

/// The skill's folder as a tar, links kept as links. `COPYFILE_DISABLE`: macOS tar would add
/// `._*` members, which the receiver's hash check refuses.
fn pack(from: &dyn Runner, label: &str, dir: &Path, name: &str) -> Result<Vec<u8>, String> {
    let out = from.shell(&format!("COPYFILE_DISABLE=1 tar -C {} -cf - .", quote(&dir.to_string_lossy())), None)?;
    if out.code != 0 {
        return Err(format!("Can't pack {name} on {label}: {}", why(&out)));
    }
    Ok(out.stdout)
}

/// The home folder on `from`, for the warnings about paths into it.
fn home_of(from: &dyn Runner, label: &str) -> Result<String, String> {
    let out = from.shell("printf '%s\\n' \"$HOME\"", None)?;
    let text = String::from_utf8_lossy(&out.stdout);
    // A shell's startup files may print first: the home is the last line.
    match text.lines().map(str::trim).rfind(|l| !l.is_empty()) {
        Some(home) if out.code == 0 => Ok(home.to_string()),
        _ => Err(format!("Can't find the home folder on {label}: {}", why(&out))),
    }
}

/// `prism`'s JSON answer: `Ok` for a plan, a result or a changed plan (exit 4), `Err` with its
/// error, prefixed with the machine, for anything else.
fn answer(out: Output, label: &str) -> Result<Value, String> {
    let json = parse(&out.stdout);
    if let Some(e) = json.as_ref().and_then(|v| v.get("error")).and_then(Value::as_str) {
        return Err(format!("{label}: {e}"));
    }
    match json {
        Some(v) if out.code == 0 || out.code == 4 => Ok(v),
        _ => Err(format!("{label}: {}", why(&out))),
    }
}

/// The JSON on stdout, past anything a remote shell's startup files printed before it.
fn parse(stdout: &[u8]) -> Option<Value> {
    let text = String::from_utf8_lossy(stdout);
    if let Ok(v) = serde_json::from_str(&text) {
        return Some(v);
    }
    let start = text.match_indices('\n').map(|(i, _)| i + 1).find(|&i| text[i..].starts_with('{'))?;
    serde_json::from_str(&text[start..]).ok()
}

/// The last thing a failed command said.
fn why(out: &Output) -> String {
    out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).map_or_else(|| format!("prism exited with {}", out.code), String::from)
}

/// Every action line names the machine it acts on.
fn on_machine(plan: &mut Value, label: &str) {
    if let Some(actions) = plan.get_mut("actions").and_then(Value::as_array_mut) {
        for line in actions.iter_mut() {
            if let Some(text) = line.as_str() {
                *line = Value::from(format!("{text} on {label}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_survive_a_chatty_shell() {
        assert_eq!(parse(b"Welcome!\n{\"a\": 1}\n"), Some(serde_json::json!({ "a": 1 })));
        assert_eq!(parse(b"no json"), None);
    }

    #[test]
    fn an_error_names_the_machine() {
        let out = Output { code: 1, stdout: b"{\"error\": \"No skill named pdf\"}".to_vec(), stderr: String::new() };
        assert_eq!(answer(out, "laptop").unwrap_err(), "laptop: No skill named pdf");
        let out = Output { code: 2, stdout: vec![], stderr: "bad flag\n".into() };
        assert_eq!(answer(out, "laptop").unwrap_err(), "laptop: bad flag");
    }

    #[test]
    fn action_lines_end_with_the_machine() {
        let mut plan = serde_json::json!({ "actions": ["Add s to Codex"], "warnings": ["w"] });
        on_machine(&mut plan, "laptop");
        assert_eq!(plan, serde_json::json!({ "actions": ["Add s to Codex on laptop"], "warnings": ["w"] }));
    }
}
