//! `prism setup`: puts `prism` on `PATH` and installs the prism skill, which tells agents how
//! to drive the CLI. The window's "Install Command Line Tool…" menu item does the first half.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::{Ctx, Exit, Out};
use crate::agents::{AGENTS, CANONICAL, Paths, Scope, tilde};
use crate::fsops::Journal;
use crate::store::{self, LastChange};
use crate::{scan, skill, sync};

/// The skill as it ships; `{{version}}` becomes the binary's version.
const SKILL: &str = include_str!("../../assets/prism-skill/SKILL.md");
const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const LABEL: &str = "install prism skill";
/// Where the link goes on macOS when `~/.local/bin` isn't on `PATH`.
const SYSTEM_BIN: &str = "/usr/local/bin";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Installed,
    Updated,
    Unchanged,
    /// Someone else's skill sits where ours goes.
    Skipped,
    Failed,
}

impl Status {
    fn key(self) -> &'static str {
        match self {
            Status::Installed => "installed",
            Status::Updated => "updated",
            Status::Unchanged => "unchanged",
            Status::Skipped => "skipped",
            Status::Failed => "failed",
        }
    }

    pub fn ok(self) -> bool {
        !matches!(self, Status::Skipped | Status::Failed)
    }
}

/// What putting `prism` on `PATH` did.
pub struct CliReport {
    pub status: Status,
    /// Where `prism` is, or would go.
    pub path: Option<PathBuf>,
    pub message: String,
}

/// What installing the skill did.
pub struct SkillReport {
    pub status: Status,
    /// The version in `~/.agents/skills/prism` now, when it is ours.
    pub version: Option<String>,
    /// Links made into agents.
    pub linked: usize,
    pub conflicts: Vec<String>,
    pub message: String,
}

impl SkillReport {
    fn failed(message: String) -> Self {
        SkillReport { status: Status::Failed, version: None, linked: 0, conflicts: vec![], message }
    }
}

impl CliReport {
    fn failed(path: Option<PathBuf>, message: String) -> Self {
        CliReport { status: Status::Failed, path, message }
    }
}

pub fn run(ctx: &Ctx, out: &mut Out) -> Exit {
    let cli = cli_part(&ctx.paths.home, ctx.path_var.as_deref());
    let skill = install_skill(ctx);
    let human = format!("Command line: {}\nSkill: {}", cli.message, skill.message);
    let json = json!({
        "cli": { "status": cli.status.key(), "path": cli.path, "message": cli.message },
        "skill": {
            "status": skill.status.key(),
            "path": skill_dir(&ctx.paths),
            "version": skill.version,
            "linked": skill.linked,
            "conflicts": skill.conflicts,
            "message": skill.message,
        },
    });
    out.emit(human, json);
    if cli.status.ok() && skill.status.ok() { Exit::Ok } else { Exit::Failed }
}

/// The platform's way of getting `prism` on `PATH`: a link on macOS, a check on Linux, where
/// the tarball's installer already put it in `~/.local/bin`.
pub fn cli_part(home: &Path, path_var: Option<&OsStr>) -> CliReport {
    let exe = match std::env::current_exe().and_then(fs::canonicalize) {
        Ok(exe) => exe,
        Err(e) => return CliReport::failed(None, format!("cannot tell where this program is: {e}")),
    };
    if cfg!(target_os = "macos") { link_cli(&exe, home, path_var, Path::new(SYSTEM_BIN)) } else { check_cli(&exe, home, path_var) }
}

/// Links `prism` to `exe` in `~/.local/bin` when that is on `PATH`, else in `system`.
pub fn link_cli(exe: &Path, home: &Path, path_var: Option<&OsStr>, system: &Path) -> CliReport {
    let local = home.join(".local/bin");
    let dirs = path_dirs(home, path_var);
    if dirs.iter().any(|d| same_dir(d, &local)) {
        return place(exe, &local, true);
    }
    let report = place(exe, system, false);
    if report.status != Status::Failed || report.path.as_deref().is_some_and(|p| fs::symlink_metadata(p).is_ok()) {
        return report;
    }
    let link = system.join("prism");
    CliReport::failed(
        Some(link.clone()),
        format!(
            "~/.local/bin is not on PATH and {} is not writable. Run: sudo mkdir -p {} && sudo ln -sf '{}' {}",
            system.display(),
            system.display(),
            exe.display(),
            link.display()
        ),
    )
}

/// Puts the link at `dir/prism`. A link elsewhere is replaced; a real file is left alone.
fn place(exe: &Path, dir: &Path, create: bool) -> CliReport {
    let at = dir.join("prism");
    let shown = at.display();
    let link = |status: Status| -> CliReport {
        if create && let Err(e) = fs::create_dir_all(dir) {
            return CliReport::failed(Some(at.clone()), format!("cannot create {}: {e}", dir.display()));
        }
        // Next to it first, then over it, so `prism` is never missing halfway.
        let tmp = dir.join(".prism.new");
        let _ = fs::remove_file(&tmp);
        match std::os::unix::fs::symlink(exe, &tmp).and_then(|()| fs::rename(&tmp, &at)) {
            Ok(()) => CliReport { status, path: Some(at.clone()), message: format!("linked {shown} to {}", exe.display()) },
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                CliReport::failed(Some(at.clone()), format!("cannot link {shown}: {e}"))
            }
        }
    };
    match fs::symlink_metadata(&at) {
        Err(_) => link(Status::Installed),
        Ok(m) if m.file_type().is_symlink() => {
            if fs::canonicalize(&at).ok().as_deref() == Some(exe) {
                CliReport { status: Status::Unchanged, path: Some(at.clone()), message: format!("{shown} is already installed") }
            } else {
                link(Status::Updated)
            }
        }
        Ok(_) => CliReport::failed(Some(at.clone()), format!("{shown} is another program, not a link to Prism; left it alone. Remove it and try again")),
    }
}

/// Reports whether the first `prism` on `PATH` is `exe`.
pub fn check_cli(exe: &Path, home: &Path, path_var: Option<&OsStr>) -> CliReport {
    let found = path_dirs(home, path_var).into_iter().map(|d| d.join("prism")).find(|p| p.is_file());
    match found {
        Some(p) if fs::canonicalize(&p).ok().as_deref() == Some(exe) => {
            CliReport { status: Status::Unchanged, message: format!("{} is this program", p.display()), path: Some(p) }
        }
        Some(p) => CliReport::failed(Some(p.clone()), format!("{} comes first on PATH and is another program, not {}", p.display(), exe.display())),
        None => CliReport::failed(
            None,
            format!("prism is not on PATH. Run: mkdir -p ~/.local/bin && ln -sf '{}' ~/.local/bin/prism, and put ~/.local/bin on PATH", exe.display()),
        ),
    }
}

/// `PATH`'s folders, with a leading `~` (some shell configs leave it) expanded.
fn path_dirs(home: &Path, path_var: Option<&OsStr>) -> Vec<PathBuf> {
    let Some(var) = path_var else { return Vec::new() };
    std::env::split_paths(var)
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| match d.strip_prefix("~") {
            Ok(rest) => home.join(rest),
            Err(_) => d,
        })
        .collect()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// `$PATH` as the user's login shell sets it. An app opened from the Finder gets a bare `PATH`
/// without `~/.local/bin`, so the menu item asks the shell. `None` when it doesn't answer in time.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn login_path() -> Option<OsString> {
    use std::io::Read;
    use std::os::unix::ffi::OsStringExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const MARK: &str = "__PRISM_PATH__";
    let shell = std::env::var_os("SHELL").filter(|s| !s.is_empty())?;
    let fish = Path::new(&shell).file_name().is_some_and(|n| n == "fish");
    let print = if fish { format!("printf '\\n{MARK}%s' (string join : $PATH)") } else { format!("printf '\\n{MARK}%s' \"$PATH\"") };
    let mut child = Command::new(&shell).arg("-ilc").arg(print).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while child.try_wait().ok()?.is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut out = Vec::new();
    child.stdout.take()?.read_to_end(&mut out).ok()?;
    // Whatever the shell's startup files print comes before the mark.
    let at = out.windows(MARK.len()).rposition(|w| w == MARK.as_bytes())?;
    Some(OsString::from_vec(out[at + MARK.len()..].to_vec())).filter(|p| !p.is_empty())
}

fn skill_dir(paths: &Paths) -> PathBuf {
    AGENTS[CANONICAL].dir(&Scope::Global, paths).join("prism")
}

pub fn skill_text() -> String {
    SKILL.replace("{{version}}", VERSION)
}

/// `a` is older than `b`. A version that doesn't parse is older than any.
fn older(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Option<Vec<u64>> {
        let core = v.trim().split(['-', '+']).next()?;
        core.split('.').map(|p| p.parse().ok()).collect()
    }
    let (Some(mut a), Some(mut b)) = (parts(a), parts(b)) else { return true };
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a < b
}

/// Writes the skill into `~/.agents/skills/prism` when it is missing or older, then links it into
/// the target agents. Someone else's skill named prism is left alone.
fn install_skill(ctx: &Ctx) -> SkillReport {
    let paths = &ctx.paths;
    let dir = skill_dir(paths);
    let shown = tilde(&dir, paths);
    let existing = skill::manifest_path(&dir);
    let (status, version) = match &existing {
        None => (Status::Installed, VERSION.to_string()),
        Some(file) => {
            let old = match fs::read_to_string(file) {
                Ok(t) => t,
                Err(e) => return SkillReport::failed(format!("cannot read {}: {e}", tilde(file, paths))),
            };
            let theirs = skill::metadata(&old, "prism-version").filter(|_| skill::parse(&old).name.as_deref() == Some("prism"));
            match theirs {
                None => {
                    let message = format!("{shown} is another skill, not the prism skill; left it alone");
                    return SkillReport { status: Status::Skipped, ..SkillReport::failed(message) };
                }
                Some(v) if older(&v, VERSION) => (Status::Updated, VERSION.to_string()),
                Some(v) => (Status::Unchanged, v),
            }
        }
    };
    if status != Status::Unchanged {
        let file = existing.unwrap_or_else(|| dir.join("SKILL.md"));
        let written = fs::create_dir_all(&dir).and_then(|()| crate::fsops::write_atomic(&file, skill_text().as_bytes()));
        if let Err(e) = written {
            return SkillReport::failed(format!("cannot write {}: {e}", tilde(&file, paths)));
        }
    }

    let inv = scan::scan(&Scope::Global, paths);
    let plan = sync::plan_one(&inv, "prism", &ctx.targets(), paths);
    let conflicts: Vec<String> = plan.conflicts.iter().map(|c| c.reason.clone()).collect();
    let mut linked = 0;
    let mut failure = None;
    if !plan.actions.is_empty() {
        store::begin_change(paths);
        let mut journal = Journal::default();
        match sync::apply(&Scope::Global, paths, &plan.actions, &mut journal) {
            Ok(n) => linked = n,
            Err((n, e)) => (linked, failure) = (n, Some(e)),
        }
        if !journal.is_empty() {
            store::save_last_change(paths, Some(&LastChange { label: LABEL.into(), journal }));
        }
    }

    let mut message = match status {
        Status::Installed => format!("installed {shown} ({VERSION})"),
        Status::Updated => format!("updated {shown} to {VERSION}"),
        _ if version == VERSION => format!("{shown} is up to date ({version})"),
        _ => format!("{shown} is newer ({version}); left it"),
    };
    if linked > 0 {
        message.push_str(&format!(", linked into {}", sync::plural(linked, "agent", "agents")));
    }
    if !conflicts.is_empty() {
        message.push_str(&format!("; not linked: {}", conflicts.join("; ")));
    }
    let status = match failure {
        Some(e) => {
            message.push_str(&format!("; linking stopped: {e}"));
            Status::Failed
        }
        None => status,
    };
    SkillReport { status, version: Some(version), linked, conflicts, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe(h: &Path) -> PathBuf {
        let exe = h.join("Prism.app/Contents/MacOS/prism");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, "").unwrap();
        exe
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(older("0.9.0", "0.10.0"));
        assert!(older("0.1", "0.1.1"));
        assert!(!older("0.1.0", "0.1"));
        assert!(!older("1.0.0", "0.9.9"));
        assert!(!older("0.2.0-beta", "0.2.0"));
        assert!(older("garbage", "0.1.0"));
        assert!(older("", "0.1.0"));
    }

    #[test]
    fn the_shipped_skill_is_prisms_and_names_this_version() {
        let text = skill_text();
        let m = skill::parse(&text);
        assert_eq!(m.name.as_deref(), Some("prism"));
        let description = m.description.unwrap();
        assert!(description.len() <= 300, "{} characters", description.len());
        assert_eq!(skill::metadata(&text, "prism-version").as_deref(), Some(VERSION));
        assert!(!text.contains('\u{2014}'), "no em dashes");
    }

    #[test]
    fn macos_links_into_local_bin_when_it_is_on_path() {
        let home = tempfile::tempdir().unwrap();
        let h = &fs::canonicalize(home.path()).unwrap();
        let (exe, system) = (exe(h), h.join("system"));
        let path = OsString::from("/usr/bin:~/.local/bin");

        let r = link_cli(&exe, h, Some(&path), &system);
        assert_eq!(r.status, Status::Installed, "{}", r.message);
        let at = h.join(".local/bin/prism");
        assert_eq!(fs::read_link(&at).unwrap(), exe);
        assert_eq!(link_cli(&exe, h, Some(&path), &system).status, Status::Unchanged);

        // A link to some other build is replaced.
        fs::remove_file(&at).unwrap();
        std::os::unix::fs::symlink(h.join("old/prism"), &at).unwrap();
        let r = link_cli(&exe, h, Some(&path), &system);
        assert_eq!(r.status, Status::Updated, "{}", r.message);
        assert_eq!(fs::read_link(&at).unwrap(), exe);
        assert!(!system.exists());
    }

    #[test]
    fn macos_falls_back_to_the_system_folder_then_to_the_command() {
        let home = tempfile::tempdir().unwrap();
        let h = &fs::canonicalize(home.path()).unwrap();
        let (exe, system) = (exe(h), h.join("system"));
        let path = OsString::from("/usr/bin");

        // Not there (as good as not writable): the user gets the command to run.
        let r = link_cli(&exe, h, Some(&path), &system);
        assert_eq!(r.status, Status::Failed);
        assert!(r.message.contains(&format!("ln -sf '{}' {}", exe.display(), system.join("prism").display())), "{}", r.message);
        assert!(!h.join(".local").exists(), "~/.local/bin is not on PATH, so nothing goes there");

        fs::create_dir(&system).unwrap();
        let r = link_cli(&exe, h, Some(&path), &system);
        assert_eq!(r.status, Status::Installed, "{}", r.message);
        assert_eq!(fs::read_link(system.join("prism")).unwrap(), exe);
    }

    #[test]
    fn linux_checks_which_prism_comes_first() {
        let home = tempfile::tempdir().unwrap();
        let h = &fs::canonicalize(home.path()).unwrap();
        let exe = exe(h);
        let (a, b) = (h.join("a"), h.join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let path = std::env::join_paths([&a, &b]).unwrap();

        let r = check_cli(&exe, h, Some(&path));
        assert_eq!(r.status, Status::Failed);
        assert!(r.message.contains("not on PATH"), "{}", r.message);

        std::os::unix::fs::symlink(&exe, b.join("prism")).unwrap();
        assert_eq!(check_cli(&exe, h, Some(&path)).status, Status::Unchanged);

        fs::write(a.join("prism"), "").unwrap();
        let r = check_cli(&exe, h, Some(&path));
        assert_eq!(r.status, Status::Failed);
        assert_eq!(r.path, Some(a.join("prism")));
    }
}
