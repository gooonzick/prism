//! Whether a host has this version of `prism`, and putting it there.

use std::path::Path;

use super::runner::{Runner, quote};

/// This build's version: a remote must report the same one.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const RELEASES: &str = "https://github.com/gooonzick/prism/releases/download";

/// What a host's `prism` is, next to this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Health {
    /// The same version as this one.
    Ready,
    /// No `prism` that answers on the host's PATH or in `~/.local/bin`; when one is there but
    /// doesn't run, the last thing it said on stderr.
    Missing(Option<String>),
    /// Another version (the one it reports), and the last thing it said on stderr, if anything.
    OtherVersion(String, Option<String>),
    /// A system no release is built for (its `uname -sm`).
    Unsupported(String),
}

impl Health {
    /// The word scripts read: `ready`, `missing`, `other_version`, `unsupported`.
    pub fn key(&self) -> &'static str {
        match self {
            Health::Ready => "ready",
            Health::Missing(_) => "missing",
            Health::OtherVersion(..) => "other_version",
            Health::Unsupported(_) => "unsupported",
        }
    }

    /// Follows "prism on laptop is …".
    pub fn describe(&self) -> String {
        match self {
            Health::Ready => "ready".into(),
            Health::Missing(None) => "missing".into(),
            Health::Missing(Some(why)) => format!("there but doesn't run ({why})"),
            Health::OtherVersion(v, None) => format!("version {v}, not {VERSION}"),
            Health::OtherVersion(v, Some(why)) => format!("version {v}, not {VERSION} ({why})"),
            Health::Unsupported(_) => "unsupported".into(),
        }
    }

    /// What the host's `prism` said on stderr when asked its version, if anything.
    pub fn why(&self) -> Option<&str> {
        match self {
            Health::Missing(why) | Health::OtherVersion(_, why) => why.as_deref(),
            _ => None,
        }
    }
}

/// A host's system as release archives name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Platform {
    pub os: String,
    pub arch: String,
}

/// The release platform for a `uname -sm` line; `None` when no release is built for it.
pub fn platform(uname: &str) -> Option<Platform> {
    let (os, arch) = match uname.split_whitespace().collect::<Vec<_>>()[..] {
        ["Linux", "x86_64"] => ("linux", "x86_64"),
        ["Linux", "aarch64" | "arm64"] => ("linux", "arm64"),
        ["Darwin", "arm64"] => ("macos", "arm64"),
        _ => return None,
    };
    Some(Platform { os: os.into(), arch: arch.into() })
}

/// What `check` prints when there is no `prism` to ask; not a line `prism` could print.
const NO_PRISM: &str = "no prism here";

/// One round trip: the host's system and its `prism --version`. Its stderr says why a `prism`
/// that is there doesn't run (a missing library, another architecture).
pub fn check(r: &dyn Runner) -> Result<(Health, Platform), String> {
    let script = format!("uname -sm\nPATH=\"$HOME/.local/bin:$PATH\"\nif command -v prism >/dev/null 2>&1; then prism --version; else echo '{NO_PRISM}'; fi\n");
    let out = r.shell(&script, None)?;
    let text = String::from_utf8_lossy(&out.stdout);
    // A remote shell's startup files may print before our commands do: find the lines by what
    // they say, not by where they are. A system that is neither Linux nor Darwin is reported
    // as the first line.
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let uname = lines
        .iter()
        .find(|l| matches!(l.split_whitespace().next(), Some("Linux" | "Darwin")))
        .or(lines.first())
        .map(|l| l.to_string())
        .unwrap_or_default();
    let version = lines.iter().find_map(|l| l.strip_prefix("prism ")).map(|v| v.trim().to_string());
    // Only a `prism` that is there has something to say about itself.
    let why = if lines.contains(&NO_PRISM) { None } else { last_line(&out.stderr) };
    let known = platform(&uname);
    // A host that runs this very version is ready, whatever it is (a binary uploaded by hand).
    let health = match (&version, &known) {
        (Some(v), _) if v == VERSION => Health::Ready,
        (_, None) => Health::Unsupported(uname.clone()),
        (Some(v), _) => Health::OtherVersion(v.clone(), why),
        (None, _) => Health::Missing(why),
    };
    let platform = known.unwrap_or_else(|| {
        let mut words = uname.split_whitespace();
        Platform { os: words.next().unwrap_or_default().to_lowercase(), arch: words.next().unwrap_or_default().to_string() }
    });
    Ok((health, platform))
}

/// The last line of `stderr` that says something, cut to a length a toast can show.
fn last_line(stderr: &str) -> Option<String> {
    let line = stderr.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    const MAX: usize = 160;
    Some(match line.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_string(),
    })
}

/// The CLI-only release archive for a platform.
pub fn cli_archive(version: &str, p: &Platform) -> String {
    format!("prism-cli-{version}-{}-{}.tar.gz", p.os, p.arch)
}

/// Puts `prism` at `~/.local/bin/prism` on the host: the release archive for `p`, checked
/// against the release's `SHA256SUMS`, or `binary` from this machine. Either must run there
/// and report `version` first; the old binary is replaced only once the new one is in place
/// beside it. A file there that doesn't say it is prism stays unless `force`.
pub fn install(r: &dyn Runner, version: &str, p: &Platform, binary: Option<&Path>, force: bool) -> Result<(), String> {
    let out = match binary {
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
            r.shell(&upload_script(version, force), Some(bytes))?
        }
        None => {
            if !released(p) {
                return Err(format!("no release is built for {} {}; pass --binary", p.os, p.arch));
            }
            r.shell(&download_script(version, p, force), None)?
        }
    };
    if out.code != 0 {
        let why = out.stderr.lines().rev().map(str::trim).find(|l| !l.is_empty());
        return Err(why.map_or_else(|| format!("the install script exited with {}", out.code), String::from));
    }
    Ok(())
}

/// Whether releases carry an archive for `p`.
fn released(p: &Platform) -> bool {
    [("linux", "x86_64"), ("linux", "arm64"), ("macos", "arm64")].contains(&(p.os.as_str(), p.arch.as_str()))
}

/// The start of both install scripts: `$dir`, a trap that leaves no `prism.new` behind, and
/// (unless `force`) a refusal to replace a `prism` there that is another program.
fn preamble(force: bool) -> String {
    let mut s = String::from(
        r#"set -e
dir="$HOME/.local/bin"
mkdir -p "$dir"
work=$(mktemp -d "${TMPDIR:-/tmp}/prism-install.XXXXXX")
trap 'rm -rf "$work"; rm -f "$dir/prism.new"' EXIT
cd "$work"
"#,
    );
    if !force {
        s.push_str(
            r#"if [ -e "$dir/prism" ] || [ -L "$dir/prism" ]; then
  case "$("$dir/prism" --version 2>/dev/null | head -n 1)" in
    "prism "*) ;;
    *) echo "~/.local/bin/prism is another program; move it away, or replace it with prism hosts install --force" >&2; exit 1 ;;
  esac
fi
"#,
        );
    }
    s
}

/// Runs `./prism` (in `$work`) and stops unless it reports `version`, saying what it printed.
fn verify(version: &str, what: &str) -> String {
    let want = quote(&format!("prism {version}"));
    format!(
        r#"chmod 755 prism
said=$(./prism --version 2>err) || {{
  why=$(tail -n 1 err)
  echo "the {what} prism doesn't run there${{why:+: $why}}" >&2
  exit 1
}}
[ "$(printf '%s\n' "$said" | head -n 1)" = {want} ] || {{
  echo "the {what} file is not prism {version}" >&2
  exit 1
}}
cp prism "$dir/prism.new"
chmod 755 "$dir/prism.new"
mv -f "$dir/prism.new" "$dir/prism"
"#
    )
}

/// Reads the binary from stdin; it replaces the old one only if it runs and reports `version`.
fn upload_script(version: &str, force: bool) -> String {
    format!("{}cat > prism\n{}", preamble(force), verify(version, "uploaded"))
}

fn download_script(version: &str, p: &Platform, force: bool) -> String {
    let archive = cli_archive(version, p);
    let base = format!("{RELEASES}/v{version}/");
    format!(
        r#"{preamble}archive={archive}
base={base}
fetch() {{
  if command -v curl >/dev/null 2>&1; then curl -fsSL -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then wget -qO "$2" "$1"
  else echo "neither curl nor wget is installed" >&2; exit 1
  fi
}}
fetch "$base$archive" "$archive"
fetch "${{base}}SHA256SUMS" SHA256SUMS
awk -v f="$archive" '$2 == f || $2 == "*" f' SHA256SUMS > sum
[ -s sum ] || {{ echo "SHA256SUMS has no line for $archive" >&2; exit 1; }}
if command -v sha256sum >/dev/null 2>&1; then sha256sum -c sum >/dev/null
else shasum -a 256 -c sum >/dev/null
fi
tar -xzf "$archive" prism
{verify}"#,
        preamble = preamble(force),
        archive = quote(&archive),
        base = quote(&base),
        verify = verify(version, "downloaded"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::runner::{Local, Output};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;

    /// Answers the check script with a fixed `uname -sm` and `prism --version`, or with no
    /// `prism` at all, or with one that only complains on stderr.
    struct Fake {
        uname: &'static str,
        version: Option<String>,
        junk: usize,
        stderr: &'static str,
    }

    fn fake(uname: &'static str, version: Option<&str>) -> Fake {
        Fake { uname, version: version.map(String::from), junk: 0, stderr: "" }
    }

    impl Runner for Fake {
        fn prism(&self, _: &[String], _: Option<Vec<u8>>) -> Result<Output, String> {
            panic!("check runs no prism command")
        }
        fn shell(&self, _: &str, _: Option<Vec<u8>>) -> Result<Output, String> {
            let mut stdout = "Welcome to the box\n".repeat(self.junk);
            stdout.push_str(&format!("{}\n", self.uname));
            match &self.version {
                Some(v) => stdout.push_str(&format!("prism {v}\n")),
                None if self.stderr.is_empty() => stdout.push_str(&format!("{NO_PRISM}\n")),
                None => {}
            }
            Ok(Output { code: 0, stdout: stdout.into_bytes(), stderr: self.stderr.into() })
        }
    }

    #[test]
    fn platform_from_uname() {
        let p = |os: &str, arch: &str| Some(Platform { os: os.into(), arch: arch.into() });
        assert_eq!(platform("Linux x86_64"), p("linux", "x86_64"));
        assert_eq!(platform("Linux aarch64"), p("linux", "arm64"));
        assert_eq!(platform("Darwin arm64"), p("macos", "arm64"));
        assert_eq!(platform("Darwin x86_64"), None);
        let (health, platform) = check(&fake("FreeBSD amd64", None)).unwrap();
        assert_eq!(health, Health::Unsupported("FreeBSD amd64".into()));
        assert_eq!(platform, Platform { os: "freebsd".into(), arch: "amd64".into() });
    }

    #[test]
    fn check_reads_the_version() {
        let check = |version: Option<&str>| check(&fake("Linux aarch64", version)).unwrap();
        let arm = Platform { os: "linux".into(), arch: "arm64".into() };
        assert_eq!(check(Some(VERSION)), (Health::Ready, arm.clone()));
        assert_eq!(check(Some("0.0.0-other")), (Health::OtherVersion("0.0.0-other".into(), None), arm.clone()));
        assert_eq!(check(None), (Health::Missing(None), arm));
    }

    #[test]
    fn check_says_why_a_prism_there_doesnt_run() {
        let glibc = "prism: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found (required by prism)";
        let broken = Fake { stderr: "bash: warning: setlocale\nprism: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found (required by prism)\n", ..fake("Linux x86_64", None) };
        let (health, _) = check(&broken).unwrap();
        assert_eq!(health, Health::Missing(Some(glibc.into())));
        assert_eq!(health.key(), "missing");
        assert!(health.describe().contains("GLIBC_2.39"), "{}", health.describe());
        // No prism at all: whatever the shell said is not about it.
        let absent = Fake { stderr: "", ..fake("Linux x86_64", None) };
        assert_eq!(check(&absent).unwrap().0, Health::Missing(None));
        let old = Fake { stderr: "warning: old config\n", ..fake("Linux x86_64", Some("0.0.1")) };
        assert_eq!(check(&old).unwrap().0, Health::OtherVersion("0.0.1".into(), Some("warning: old config".into())));
        let long = "x".repeat(400);
        assert_eq!(last_line(&long).unwrap().chars().count(), 161, "cut short, with an ellipsis");
    }

    #[test]
    fn check_finds_its_lines_after_shell_noise() {
        let noisy = Fake { junk: 2, ..fake("Darwin arm64", Some(VERSION)) };
        let (health, platform) = check(&noisy).unwrap();
        assert_eq!(health, Health::Ready);
        assert_eq!(platform, Platform { os: "macos".into(), arch: "arm64".into() });
        let missing = Fake { junk: 2, ..fake("Linux x86_64", None) };
        assert_eq!(check(&missing).unwrap().0, Health::Missing(None));
    }

    #[test]
    fn check_runs_on_a_real_shell() {
        let home = tempfile::tempdir().unwrap();
        let local = Local { name: "box".into(), home: home.path().into() };
        assert_eq!(check(&local).unwrap().0, Health::Missing(None), "no prism there");
        let bin = home.path().join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("prism"), "#!/bin/sh\necho 'libc.so.6: version GLIBC_2.39 not found' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(bin.join("prism"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(check(&local).unwrap().0, Health::Missing(Some("libc.so.6: version GLIBC_2.39 not found".into())));
    }

    #[test]
    fn cli_archive_names() {
        let p = Platform { os: "linux".into(), arch: "arm64".into() };
        assert_eq!(cli_archive("0.2.0", &p), "prism-cli-0.2.0-linux-arm64.tar.gz");
    }

    #[test]
    fn install_script_never_uses_sudo() {
        let script = download_script("0.2.0", &Platform { os: "linux".into(), arch: "x86_64".into() }, false);
        for s in [script.as_str(), upload_script("0.2.0", false).as_str()] {
            assert!(!s.contains("sudo"), "{s}");
        }
        assert!(script.contains("https://github.com/gooonzick/prism/releases/download/v0.2.0/"), "{script}");
        let verify = script.find("sha256sum -c").expect("verifies");
        let runs = script.find("./prism --version").expect("runs what it downloaded");
        let copy = script.find("cp prism").expect("copies");
        let rename = script.find("mv -f").expect("renames");
        assert!(verify < runs && runs < copy && copy < rename, "{script}");
    }

    /// A host whose `curl` serves files from `served`: `sh` with a temp `HOME` and `TMPDIR`,
    /// and a fake `curl` first on `PATH`.
    struct Offline {
        home: PathBuf,
        bin: PathBuf,
        tmp: PathBuf,
    }

    impl Offline {
        fn new(root: &Path, served: &Path) -> Offline {
            let (home, bin, tmp) = (root.join("home"), root.join("fakebin"), root.join("tmp"));
            for d in [&home, &bin, &tmp] {
                std::fs::create_dir_all(d).unwrap();
            }
            let curl = bin.join("curl");
            // Called as: curl -fsSL -o OUT URL
            std::fs::write(&curl, format!("#!/bin/sh\ncp \"{}/${{4##*/}}\" \"$3\"\n", served.display())).unwrap();
            std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).unwrap();
            Offline { home, bin, tmp }
        }
        fn prism(&self) -> PathBuf {
            self.home.join(".local/bin/prism")
        }
        /// A `prism` there already, saying `said` when asked its version.
        fn has(&self, said: &str) {
            executable(&self.prism(), &format!("#!/bin/sh\necho '{said}'\n"));
        }
        /// Nothing of the install is left: no `prism.new`, no work folder.
        fn left_nothing(&self) {
            assert!(!self.prism().with_file_name("prism.new").exists());
            assert_eq!(std::fs::read_dir(&self.tmp).unwrap().count(), 0, "the work folder went");
        }
    }

    impl Runner for Offline {
        fn prism(&self, _: &[String], _: Option<Vec<u8>>) -> Result<Output, String> {
            panic!("install runs no prism command")
        }
        fn shell(&self, script: &str, stdin: Option<Vec<u8>>) -> Result<Output, String> {
            assert!(stdin.is_none());
            let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap_or_default());
            let out = Command::new("sh").args(["-c", script]).env("HOME", &self.home).env("TMPDIR", &self.tmp).env("PATH", path).output().unwrap();
            Ok(Output { code: out.status.code().unwrap_or(-1), stdout: out.stdout, stderr: String::from_utf8_lossy(&out.stderr).into_owned() })
        }
    }

    fn executable(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A release folder: the archive for `p` holding a `prism` that is the script `content`,
    /// and its sums.
    fn release(dir: &Path, p: &Platform, content: &str) -> String {
        let archive = cli_archive("9.9.9", p);
        let stage = dir.join("stage");
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("prism"), content).unwrap();
        let ok = Command::new("tar").arg("-czf").arg(dir.join(&archive)).arg("-C").arg(&stage).arg("prism").status().unwrap();
        assert!(ok.success());
        let sum = Command::new("sh").args(["-c", "sha256sum \"$0\" 2>/dev/null || shasum -a 256 \"$0\"", &archive]).current_dir(dir).output().unwrap();
        let sum = String::from_utf8(sum.stdout).unwrap();
        std::fs::write(dir.join("SHA256SUMS"), &sum).unwrap();
        archive
    }

    const NEW: &str = "#!/bin/sh\necho 'prism 9.9.9'\n";
    const OLD: &str = "#!/bin/sh\necho 'prism 0.0.1'\n";

    /// A host with prism 0.0.1, offered a release of 9.9.9 whose `prism` is `content`.
    fn offline(tmp: &Path, content: &str) -> (Offline, Platform) {
        let served = tmp.join("served");
        std::fs::create_dir_all(&served).unwrap();
        let p = Platform { os: "linux".into(), arch: "x86_64".into() };
        release(&served, &p, content);
        let host = Offline::new(tmp, &served);
        host.has("prism 0.0.1");
        (host, p)
    }

    #[test]
    fn install_downloads_checks_and_replaces() {
        let tmp = tempfile::tempdir().unwrap();
        let (host, p) = offline(tmp.path(), NEW);
        install(&host, "9.9.9", &p, None, false).unwrap();
        assert_eq!(std::fs::read_to_string(host.prism()).unwrap(), NEW);
        assert_eq!(std::fs::metadata(host.prism()).unwrap().permissions().mode() & 0o777, 0o755);
        host.left_nothing();
    }

    #[test]
    fn a_bad_checksum_keeps_the_old_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let (host, p) = offline(tmp.path(), NEW);
        let archive = cli_archive("9.9.9", &p);
        std::fs::write(tmp.path().join("served/SHA256SUMS"), format!("{}  {archive}\n", "0".repeat(64))).unwrap();
        let err = install(&host, "9.9.9", &p, None, false).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert_eq!(std::fs::read_to_string(host.prism()).unwrap(), OLD);
        host.left_nothing();
    }

    #[test]
    fn a_download_that_doesnt_run_there_keeps_the_old_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let glibc = "#!/bin/sh\necho \"prism: version 'GLIBC_2.39' not found\" >&2\nexit 1\n";
        let (host, p) = offline(tmp.path(), glibc);
        let err = install(&host, "9.9.9", &p, None, false).unwrap_err();
        assert_eq!(err, "the downloaded prism doesn't run there: prism: version 'GLIBC_2.39' not found");
        assert_eq!(std::fs::read_to_string(host.prism()).unwrap(), OLD);
        host.left_nothing();
    }

    #[test]
    fn a_download_reporting_another_version_keeps_the_old_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let (host, p) = offline(tmp.path(), "#!/bin/sh\necho 'prism 1.2.3'\n");
        let err = install(&host, "9.9.9", &p, None, false).unwrap_err();
        assert_eq!(err, "the downloaded file is not prism 9.9.9");
        assert_eq!(std::fs::read_to_string(host.prism()).unwrap(), OLD);
        host.left_nothing();
    }

    #[test]
    fn another_program_named_prism_stays_unless_forced() {
        let tmp = tempfile::tempdir().unwrap();
        let (host, p) = offline(tmp.path(), NEW);
        host.has("Prism image tools 4.1");
        let err = install(&host, "9.9.9", &p, None, false).unwrap_err();
        assert!(err.contains("another program") && err.contains("--force"), "{err}");
        assert!(std::fs::read_to_string(host.prism()).unwrap().contains("image tools"), "left alone");
        host.left_nothing();

        install(&host, "9.9.9", &p, None, true).unwrap();
        assert_eq!(std::fs::read_to_string(host.prism()).unwrap(), NEW);
    }

    #[test]
    fn install_uploads_a_binary_over_stdin() {
        let home = tempfile::tempdir().unwrap();
        let local = Local { name: "box".into(), home: home.path().into() };
        let binary = home.path().join("built-prism");
        std::fs::write(&binary, format!("#!/bin/sh\necho \"prism {VERSION}\"\n")).unwrap();
        // No release platform needed for an upload.
        let p = Platform { os: "freebsd".into(), arch: "amd64".into() };
        install(&local, VERSION, &p, Some(&binary), false).unwrap();
        let installed = home.path().join(".local/bin/prism");
        assert!(std::fs::read_to_string(&installed).unwrap().contains(VERSION));
        assert_eq!(std::fs::metadata(&installed).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(!installed.with_file_name("prism.new").exists());
    }

    #[test]
    fn an_upload_reporting_another_version_keeps_the_old_binary() {
        let home = tempfile::tempdir().unwrap();
        let local = Local { name: "box".into(), home: home.path().into() };
        let installed = home.path().join(".local/bin/prism");
        executable(&installed, OLD);
        let binary = home.path().join("built-prism");
        std::fs::write(&binary, "#!/bin/sh\necho \"prism 0.0.0-other\"\n").unwrap();
        let p = Platform { os: "freebsd".into(), arch: "amd64".into() };
        let err = install(&local, VERSION, &p, Some(&binary), false).unwrap_err();
        assert!(err.contains("not prism"), "{err}");
        assert_eq!(std::fs::read_to_string(&installed).unwrap(), OLD);
        assert!(!installed.with_file_name("prism.new").exists());
    }

    #[test]
    fn install_without_a_release_needs_a_binary() {
        let p = Platform { os: "freebsd".into(), arch: "amd64".into() };
        let err = install(&fake("FreeBSD amd64", None), VERSION, &p, None, false).unwrap_err();
        assert!(err.contains("--binary"), "{err}");
    }
}
