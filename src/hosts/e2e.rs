//! End to end over a real ssh: the Linux container `scripts/e2e-ssh.sh` starts, reached with
//! the real [`Ssh`] runner (through the ssh_config the script writes, never the user's own)
//! and running the CLI-only Linux build there. Ignored, since it needs that container:
//!
//! ```text
//! ./scripts/e2e-ssh.sh && cargo test -- --ignored e2e_
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::Host;
use super::copy::{copy_skills, this_machine};
use super::runner::{Local, MakeRunner, Runner, Ssh};
use crate::agents::{Paths, Scope};
use crate::cli::{Ctx, run_with};
use crate::store::Settings;

/// The Host alias in the script's ssh_config.
const TARGET: &str = "prism-e2e";
const SECRET: &str = "FIXTURE_E2E_SECRET";

fn cache() -> PathBuf {
    dirs::home_dir().expect("a home folder").join(".cache/prism-e2e")
}

fn ssh(name: &str) -> Ssh {
    let config = cache().join("ssh_config");
    assert!(config.is_file(), "no {}: run ./scripts/e2e-ssh.sh first", config.display());
    Ssh { name: name.into(), target: TARGET.into(), config: Some(config) }
}

/// `ssh -O <op>` on the control socket the runner uses: its own argv, the command swapped
/// for the control operation.
fn control(op: &str) -> std::process::Output {
    let mut argv = ssh("box").argv("true");
    // It ends `TARGET -- true`: drop those three, whatever comes before them.
    assert_eq!(argv[argv.len() - 3..], [TARGET, "--", "true"], "{argv:?}");
    argv.truncate(argv.len() - 3);
    argv.extend(["-O".into(), op.into(), TARGET.into()]);
    Command::new(&argv[0]).args(&argv[1..]).output().expect("ssh runs")
}

/// Runs `script` on the container; it must succeed. Its stdout.
fn remote(script: &str) -> String {
    let out = ssh("box").shell(script, None).expect("the container answers");
    assert_eq!(out.code, 0, "{script}: {}", out.stderr);
    String::from_utf8(out.stdout).unwrap()
}

/// A ctx on `home` whose "this" is `home` (in process) and whose hosts are the container.
fn ctx(home: &Path) -> Ctx {
    let paths = Paths::isolated(home);
    let settings = Settings::load_from(&paths);
    let here = home.to_path_buf();
    let make_runner: MakeRunner = Arc::new(move |h: &Host| match h.name.as_str() {
        "this" => Box::new(Local { name: h.name.clone(), home: here.clone() }) as Box<dyn Runner>,
        _ => Box::new(Ssh { target: h.ssh.clone(), ..ssh(&h.name) }),
    });
    Ctx { paths, settings, stdin: None, path_var: None, make_runner }
}

fn go(ctx: &mut Ctx, args: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(args, ctx, &mut out, &mut err);
    (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

/// Runs `args`, which must exit 0 and print JSON; the JSON, and both streams as printed.
fn ok(ctx: &mut Ctx, args: &[&str]) -> (Value, String) {
    let (code, out, err) = go(ctx, args);
    assert_eq!(code, 0, "prism {}: {out}{err}", args.join(" "));
    (serde_json::from_str(&out).unwrap_or_else(|e| panic!("prism {}: {e}: {out}", args.join(" "))), format!("{out}{err}"))
}

fn skill(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir.join(name)).unwrap();
    std::fs::write(dir.join(name).join("SKILL.md"), format!("---\nname: {name}\ndescription: {name} for the e2e test\n---\n\n# {name}\n")).unwrap();
}

#[test]
#[ignore = "needs the container from ./scripts/e2e-ssh.sh"]
fn e2e_copy_skill_and_server_over_ssh() {
    // A fresh connection: no master left from an earlier run.
    let _ = control("exit");
    // The first call starts a ControlPersist master in the background; it must answer at once,
    // not when that master exits a minute later (it would, were the master to hold our pipes).
    let started = Instant::now();
    let home = remote("rm -rf ~/.agents ~/.claude ~/.claude.json ~/.config ~/src && mkdir -p ~/.claude && printf %s \"$HOME\"");
    assert!(started.elapsed() < Duration::from_secs(10), "the first call took {:?}", started.elapsed());
    assert!(control("check").status.success(), "a master now runs in the background");

    let here = tempfile::tempdir().unwrap();
    let a = here.path();
    std::fs::create_dir_all(a.join(".claude")).unwrap();
    let mut c = ctx(a);

    // The host: saved, checked, and given this build by upload.
    ok(&mut c, &["hosts", "add", "box", "--ssh", TARGET, "--json"]);
    let (check, _) = ok(&mut c, &["hosts", "check", "box", "--json"]);
    assert_eq!((check["health"].as_str(), check["os"].as_str()), (Some("ready"), Some("linux")), "{check}");
    let binary = cache().join("prism-linux");
    let (installed, _) = ok(&mut c, &["hosts", "install", "box", "--binary", binary.to_str().unwrap(), "--json"]);
    assert_eq!(installed["installed"], true);

    // Two skills, one with a link inside, go across as one change.
    let skills = a.join(".agents/skills");
    skill(&skills, "emil-animations");
    skill(&skills, "emil-typography");
    std::os::unix::fs::symlink("SKILL.md", skills.join("emil-animations/README.md")).unwrap();
    let copy = ["skills", "copy", "emil-animations", "emil-typography", "--to-host", "box", "--to-global", "--json"];
    let (plan, _) = ok(&mut c, &copy);
    assert_eq!(plan["actions"].as_array().map(Vec::len), Some(2), "{plan}");
    assert!(plan["actions"][0].as_str().unwrap().ends_with(" on box"), "{plan}");
    assert_eq!(remote("ls ~/.agents/skills 2>/dev/null || true"), "", "a plan changes nothing");
    let id = plan["plan_id"].as_str().unwrap().to_string();
    let (done, _) = ok(&mut c, &[&copy[..], &["--apply", "--plan-id", &id]].concat());
    assert_eq!(done["applied"], true, "{done}");
    let there = remote("cd ~ && cat .agents/skills/emil-typography/SKILL.md >/dev/null && readlink .agents/skills/emil-animations/README.md .claude/skills/emil-animations .claude/skills/emil-typography");
    assert_eq!(there, "SKILL.md\n../../.agents/skills/emil-animations\n../../.agents/skills/emil-typography\n");

    // A server with a secret: it travels on stdin, lands there whole, and is never printed.
    std::fs::write(a.join(".claude.json"), format!(r#"{{"mcpServers":{{"mobbin":{{"command":"npx","args":["-y","mobbin-mcp"],"env":{{"MOBBIN_API_KEY":"{SECRET}"}}}}}}}}"#)).unwrap();
    let copy = ["mcp", "copy", "mobbin", "--to-host", "box", "--to", "claude", "--json"];
    let (plan, printed) = ok(&mut c, &copy);
    assert_eq!(plan["actions"], serde_json::json!(["Add mobbin to Claude Code on box"]));
    let id = plan["plan_id"].as_str().unwrap().to_string();
    let (done, printed_too) = ok(&mut c, &[&copy[..], &["--apply", "--plan-id", &id]].concat());
    assert_eq!(done["applied"], true, "{done}");
    let config: Value = serde_json::from_str(&remote("cat ~/.claude.json")).unwrap();
    assert_eq!(config["mcpServers"]["mobbin"]["env"]["MOBBIN_API_KEY"], SECRET);
    let (listed, listed_text) = ok(&mut c, &["--host", "box", "mcp", "list", "--json"]);
    assert!(listed.to_string().contains("mobbin"), "{listed}");
    for text in [&printed, &printed_too, &listed_text] {
        assert!(!text.contains(SECRET), "{text}");
    }

    // Undo on the host takes the server back: the copy made ~/.claude.json, so it goes.
    let (undone, _) = ok(&mut c, &["--host", "box", "undo", "--json"]);
    assert_eq!(undone["undone"], format!("copy mobbin from {}", this_machine()));
    assert_eq!(remote("test -e ~/.claude.json && echo there || echo gone"), "gone\n");

    // And back: Linux packs a skill linked in from elsewhere (mktemp -d, cp -RP) with a plain
    // one, and this machine takes both.
    remote("mkdir -p ~/src/notes && cd ~/src/notes && printf -- '---\\nname: from-linux\\ndescription: d\\n---\\n' > SKILL.md && ln -s SKILL.md README.md && ln -s ~/src/notes ~/.agents/skills/from-linux");
    let back = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(back.path().join(".claude")).unwrap();
    let to = Local { name: "this".into(), home: back.path().into() };
    let sent = [("from-linux".to_string(), PathBuf::from(format!("{home}/src/notes"))), ("emil-typography".to_string(), PathBuf::from(format!("{home}/.agents/skills/emil-typography")))];
    let from = ssh("box");
    let plan = copy_skills(&from, "box", &Scope::Global, &sent, &to, this_machine(), &Scope::Global, None).unwrap();
    assert_eq!(plan["actions"].as_array().map(Vec::len), Some(2), "{plan}");
    let id = plan["plan_id"].as_str().unwrap();
    let done = copy_skills(&from, "box", &Scope::Global, &sent, &to, this_machine(), &Scope::Global, Some(id)).unwrap();
    assert_eq!(done["applied"], true, "{done}");
    let got = back.path().join(".agents/skills");
    assert!(std::fs::read_to_string(got.join("from-linux/SKILL.md")).unwrap().contains("name: from-linux"));
    assert_eq!(std::fs::read_link(got.join("from-linux/README.md")).unwrap(), Path::new("SKILL.md"));
    assert!(got.join("emil-typography/SKILL.md").is_file());
    assert!(std::fs::symlink_metadata(back.path().join(".claude/skills/from-linux")).unwrap().file_type().is_symlink());

    let _ = control("exit");
}
