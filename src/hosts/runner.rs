//! Running `prism` (or a shell script) on a machine: over ssh for a saved host, in process for tests.
// The window and the CLI pick these up in later steps.
#![allow(dead_code)]

use super::Host;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

/// What a finished command left behind. `stdout` stays bytes: tar streams travel through it.
#[derive(Debug)]
pub struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

pub trait Runner: Send + Sync {
    /// Runs `prism <args>` on the machine. `Err` only when the machine can't be reached.
    fn prism(&self, args: &[String], stdin: Option<Vec<u8>>) -> Result<Output, String>;
    /// Runs a shell script on the machine. `Err` only when the machine can't be reached.
    fn shell(&self, script: &str, stdin: Option<Vec<u8>>) -> Result<Output, String>;
}

/// How the CLI and the window get a runner for a host; tests hand out `Local` ones.
pub type MakeRunner = Arc<dyn Fn(&Host) -> Box<dyn Runner> + Send + Sync>;

pub fn runner_for(host: &Host) -> Box<dyn Runner> {
    Box::new(Ssh { name: host.name.clone(), target: host.ssh.clone() })
}

/// POSIX single-quoting: safe to paste into `sh` as one word.
pub fn quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

fn prism_command(args: &[String]) -> String {
    let mut cmd = String::from("PATH=\"$HOME/.local/bin:$PATH\" prism");
    for a in args {
        cmd.push(' ');
        cmd.push_str(&quote(a));
    }
    cmd
}

/// Spawns, feeds stdin, closes it and reads both streams to the end.
fn spawn(mut cmd: Command, stdin: Option<Vec<u8>>) -> std::io::Result<(i32, Vec<u8>, String)> {
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let writer = stdin.map(|data| {
        let mut pipe = child.stdin.take().expect("stdin is piped");
        // A thread, so a command that fills its output before reading input can't deadlock us.
        std::thread::spawn(move || {
            let _ = pipe.write_all(&data);
        })
    });
    let mut stderr_pipe = child.stderr.take().expect("stderr is piped");
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });
    let mut stdout = Vec::new();
    child.stdout.take().expect("stdout is piped").read_to_end(&mut stdout)?;
    let status = child.wait()?;
    if let Some(w) = writer {
        let _ = w.join();
    }
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok((status.code().unwrap_or(-1), stdout, String::from_utf8_lossy(&stderr).into_owned()))
}

fn finish(res: std::io::Result<(i32, Vec<u8>, String)>) -> Result<Output, (i32, String)> {
    match res {
        Ok((code, stdout, stderr)) => Ok(Output { code, stdout, stderr }),
        Err(e) => Err((-1, e.to_string())),
    }
}

/// A machine reached with `ssh`. Connections are shared through a control socket so that
/// the many small commands of one screen cost one handshake.
pub struct Ssh {
    pub name: String,
    pub target: String,
}

impl Ssh {
    pub fn argv(&self, remote_command: &str) -> Vec<String> {
        let control = control_dir_path().join("%C");
        let mut v: Vec<String> = ["ssh", "-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ControlMaster=auto"].map(String::from).into();
        v.push("-o".into());
        v.push(format!("ControlPath={}", control.display()));
        v.extend(["-o", "ControlPersist=60"].map(String::from));
        v.push(self.target.clone());
        v.push("--".into());
        v.push(remote_command.into());
        v
    }

    fn run(&self, remote_command: &str, stdin: Option<Vec<u8>>) -> Result<Output, String> {
        let unreachable = |why: &str| {
            let last = why.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("no answer");
            format!("Can't reach {}: {last}", self.name)
        };
        // macOS limits a socket path to 104 bytes, so the sockets live under /tmp.
        ensure_control_dir().map_err(|e| unreachable(&e))?;
        let argv = self.argv(remote_command);
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        match finish(spawn(cmd, stdin)) {
            Ok(out) if out.code == 255 => Err(unreachable(&out.stderr)),
            Ok(out) => Ok(out),
            Err((_, why)) => Err(unreachable(&why)),
        }
    }
}

impl Runner for Ssh {
    fn prism(&self, args: &[String], stdin: Option<Vec<u8>>) -> Result<Output, String> {
        self.run(&prism_command(args), stdin)
    }
    fn shell(&self, script: &str, stdin: Option<Vec<u8>>) -> Result<Output, String> {
        self.run(script, stdin)
    }
}

fn control_dir_path() -> PathBuf {
    // SAFETY: getuid has no preconditions and cannot fail.
    PathBuf::from(format!("/tmp/prism-ssh-{}", unsafe { libc::getuid() }))
}

/// Creates the control directory (0700) or checks that the one there is ours and private.
fn ensure_control_dir() -> Result<(), String> {
    let dir = control_dir_path();
    if let Err(e) = std::fs::DirBuilder::new().mode(0o700).create(&dir)
        && e.kind() != std::io::ErrorKind::AlreadyExists
    {
        return Err(format!("can't create {}: {e}", dir.display()));
    }
    // symlink_metadata: a symlink planted at the path is not our directory.
    let meta = std::fs::symlink_metadata(&dir).map_err(|e| format!("can't inspect {}: {e}", dir.display()))?;
    // SAFETY: as above.
    let uid = unsafe { libc::getuid() };
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o777 != 0o700 {
        return Err(format!("{} is not a private directory of this user", dir.display()));
    }
    Ok(())
}

/// This machine, for copies between "this" and a remote one.
pub struct Here;

impl Runner for Here {
    fn prism(&self, args: &[String], stdin: Option<Vec<u8>>) -> Result<Output, String> {
        let exe = std::env::current_exe().map_err(|e| format!("Can't run prism here: {e}"))?;
        let mut cmd = Command::new(exe);
        cmd.args(args);
        finish(spawn(cmd, stdin)).map_err(|(_, why)| format!("Can't run prism here: {why}"))
    }
    fn shell(&self, script: &str, stdin: Option<Vec<u8>>) -> Result<Output, String> {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        finish(spawn(cmd, stdin)).map_err(|(_, why)| format!("Can't run sh here: {why}"))
    }
}

/// A host that is a folder on this machine: the CLI runs in process against `home`.
#[cfg(test)]
pub struct Local {
    pub name: String,
    pub home: PathBuf,
}

#[cfg(test)]
impl Runner for Local {
    fn prism(&self, args: &[String], stdin: Option<Vec<u8>>) -> Result<Output, String> {
        use crate::agents::Paths;
        use crate::cli::{Ctx, run_with};
        use crate::store::Settings;
        let paths = Paths::isolated(&self.home);
        let settings = Settings::load_from(&paths);
        let stdin: Option<Box<dyn Read>> = stdin.map(|d| Box::new(std::io::Cursor::new(d)) as Box<dyn Read>);
        let mut ctx = Ctx { paths, settings, stdin, path_var: None };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let code = run_with(&args, &mut ctx, &mut out, &mut err);
        Ok(Output { code, stdout: out, stderr: String::from_utf8_lossy(&err).into_owned() })
    }
    fn shell(&self, script: &str, stdin: Option<Vec<u8>>) -> Result<Output, String> {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]).env("HOME", &self.home);
        finish(spawn(cmd, stdin)).map_err(|(_, why)| format!("Can't reach {}: {why}", self.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_round_trips_hostile_args() {
        for arg in ["a b", "it's", "$HOME", "a;rm -rf x", "`x`", "\"q\"", ""] {
            let out = Command::new("sh").args(["-c", &format!("printf %s {}", quote(arg))]).output().unwrap();
            assert_eq!(String::from_utf8(out.stdout).unwrap(), arg, "{arg:?}");
        }
    }

    #[test]
    fn ssh_argv_is_non_interactive() {
        let ssh = Ssh { name: "laptop".into(), target: "george@laptop".into() };
        let uid = unsafe { libc::getuid() };
        let want: Vec<String> = [
            "ssh", "-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ControlMaster=auto", "-o",
            &format!("ControlPath=/tmp/prism-ssh-{uid}/%C"), "-o", "ControlPersist=60", "george@laptop", "--", "true",
        ]
        .map(String::from)
        .into();
        assert_eq!(ssh.argv("true"), want);
    }

    #[test]
    fn secret_never_in_argv() {
        const FIXTURE_SECRET: &str = "FIXTURE_SECRET";
        let ssh = Ssh { name: "laptop".into(), target: "george@laptop".into() };
        let args = ["mcp".to_string(), "add".to_string(), "KEY=-".to_string()];
        let argv = ssh.argv(&prism_command(&args));
        assert!(argv.iter().all(|a| !a.contains(FIXTURE_SECRET)));
        // And the data really goes through stdin of the process.
        let mut cmd = Command::new("cat");
        cmd.arg("-");
        let (code, stdout, _) = spawn(cmd, Some(FIXTURE_SECRET.as_bytes().to_vec())).unwrap();
        assert_eq!((code, stdout), (0, FIXTURE_SECRET.as_bytes().to_vec()));
    }

    #[test]
    fn local_runner_runs_cli() {
        let home = tempfile::tempdir().unwrap();
        let local = Local { name: "box".into(), home: home.path().into() };
        let out = local.prism(&["agents".into(), "--json".into()], None).unwrap();
        assert_eq!(out.code, 0, "{}", out.stderr);
        serde_json::from_slice::<serde_json::Value>(&out.stdout).expect("JSON on stdout");
    }

    #[test]
    fn local_shell_uses_its_home() {
        let home = tempfile::tempdir().unwrap();
        let local = Local { name: "box".into(), home: home.path().into() };
        let out = local.shell("printf %s \"$HOME\"", None).unwrap();
        assert_eq!(out.stdout, home.path().as_os_str().as_encoded_bytes());
    }

    #[test]
    fn an_unreachable_host_is_an_error_naming_it() {
        let ssh = Ssh { name: "ghost".into(), target: "nobody@127.0.0.1:1".into() };
        // ssh to a bad target exits 255; with no ssh at all the spawn fails: both are Err.
        let err = ssh.shell("true", None).unwrap_err();
        assert!(err.starts_with("Can't reach ghost: "), "{err}");
    }
}
