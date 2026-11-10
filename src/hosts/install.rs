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
    /// No `prism` on the host's PATH or in `~/.local/bin`.
    Missing,
    /// Another version (the one it reports).
    OtherVersion(String),
    /// A system no release is built for (its `uname -sm`).
    Unsupported(String),
}

impl Health {
    /// The word scripts read: `ready`, `missing`, `other_version`, `unsupported`.
    pub fn key(&self) -> &'static str {
        match self {
            Health::Ready => "ready",
            Health::Missing => "missing",
            Health::OtherVersion(_) => "other_version",
            Health::Unsupported(_) => "unsupported",
        }
    }

    /// Follows "prism on laptop is …".
    pub fn describe(&self) -> String {
        match self {
            Health::Ready => "ready".into(),
            Health::Missing => "missing".into(),
            Health::OtherVersion(v) => format!("version {v}, not {VERSION}"),
            Health::Unsupported(_) => "unsupported".into(),
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

/// One round trip: the host's system and its `prism --version`.
pub fn check(r: &dyn Runner) -> Result<(Health, Platform), String> {
    let out = r.shell("uname -sm; PATH=\"$HOME/.local/bin:$PATH\" prism --version 2>/dev/null", None)?;
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
    let known = platform(&uname);
    // A host that runs this very version is ready, whatever it is (a binary uploaded by hand).
    let health = match (&version, &known) {
        (Some(v), _) if v == VERSION => Health::Ready,
        (_, None) => Health::Unsupported(uname.clone()),
        (Some(v), _) => Health::OtherVersion(v.clone()),
        (None, _) => Health::Missing,
    };
    let platform = known.unwrap_or_else(|| {
        let mut words = uname.split_whitespace();
        Platform { os: words.next().unwrap_or_default().to_lowercase(), arch: words.next().unwrap_or_default().to_string() }
    });
    Ok((health, platform))
}

/// The CLI-only release archive for a platform.
pub fn cli_archive(version: &str, p: &Platform) -> String {
    format!("prism-cli-{version}-{}-{}.tar.gz", p.os, p.arch)
}

/// Puts `prism` at `~/.local/bin/prism` on the host: the release archive for `p`, checked
/// against the release's `SHA256SUMS`, or `binary` from this machine. The old binary is
/// replaced only once the new one is in place beside it.
pub fn install(r: &dyn Runner, version: &str, p: &Platform, binary: Option<&Path>) -> Result<(), String> {
    let out = match binary {
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
            r.shell(&upload_script(version), Some(bytes))?
        }
        None => {
            if !released(p) {
                return Err(format!("no release is built for {} {}; pass --binary", p.os, p.arch));
            }
            r.shell(&download_script(version, p), None)?
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

/// Reads the binary from stdin; it replaces the old one only if it runs and reports `version`.
fn upload_script(version: &str) -> String {
    let want = quote(&format!("prism {version}"));
    format!(
        r#"set -e
dir="$HOME/.local/bin"
mkdir -p "$dir"
cat > "$dir/prism.new"
chmod 755 "$dir/prism.new"
[ "$("$dir/prism.new" --version 2>/dev/null | head -n 1)" = {want} ] || {{
  rm -f "$dir/prism.new"
  echo "the uploaded file is not prism {version}" >&2
  exit 1
}}
mv -f "$dir/prism.new" "$dir/prism"
"#
    )
}

fn download_script(version: &str, p: &Platform) -> String {
    let archive = cli_archive(version, p);
    let base = format!("{RELEASES}/v{version}/");
    format!(
        r#"set -e
archive={archive}
base={base}
dir="$HOME/.local/bin"
mkdir -p "$dir"
work=$(mktemp -d "${{TMPDIR:-/tmp}}/prism-install.XXXXXX")
trap 'rm -rf "$work"' EXIT
cd "$work"
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
cp prism "$dir/prism.new"
chmod 755 "$dir/prism.new"
mv -f "$dir/prism.new" "$dir/prism"
"#,
        archive = quote(&archive),
        base = quote(&base),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::runner::{Local, Output};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;

    /// Answers the check script with a fixed `uname -sm` and `prism --version`.
    struct Fake {
        uname: &'static str,
        version: Option<String>,
        junk: usize,
    }

    impl Runner for Fake {
        fn prism(&self, _: &[String], _: Option<Vec<u8>>) -> Result<Output, String> {
            panic!("check runs no prism command")
        }
        fn shell(&self, _: &str, _: Option<Vec<u8>>) -> Result<Output, String> {
            let mut stdout = "Welcome to the box\n".repeat(self.junk);
            stdout.push_str(&format!("{}\n", self.uname));
            if let Some(v) = &self.version {
                stdout.push_str(&format!("prism {v}\n"));
            }
            Ok(Output { code: 0, stdout: stdout.into_bytes(), stderr: String::new() })
        }
    }

    #[test]
    fn platform_from_uname() {
        let p = |os: &str, arch: &str| Some(Platform { os: os.into(), arch: arch.into() });
        assert_eq!(platform("Linux x86_64"), p("linux", "x86_64"));
        assert_eq!(platform("Linux aarch64"), p("linux", "arm64"));
        assert_eq!(platform("Darwin arm64"), p("macos", "arm64"));
        assert_eq!(platform("Darwin x86_64"), None);
        let (health, platform) = check(&Fake { uname: "FreeBSD amd64", version: None, junk: 0 }).unwrap();
        assert_eq!(health, Health::Unsupported("FreeBSD amd64".into()));
        assert_eq!(platform, Platform { os: "freebsd".into(), arch: "amd64".into() });
    }

    #[test]
    fn check_reads_the_version() {
        let check = |version: Option<&str>| check(&Fake { uname: "Linux aarch64", version: version.map(String::from), junk: 0 }).unwrap();
        let arm = Platform { os: "linux".into(), arch: "arm64".into() };
        assert_eq!(check(Some(VERSION)), (Health::Ready, arm.clone()));
        assert_eq!(check(Some("0.0.0-other")), (Health::OtherVersion("0.0.0-other".into()), arm.clone()));
        assert_eq!(check(None), (Health::Missing, arm));
    }

    #[test]
    fn check_finds_its_lines_after_shell_noise() {
        let noisy = Fake { uname: "Darwin arm64", version: Some(VERSION.into()), junk: 2 };
        let (health, platform) = check(&noisy).unwrap();
        assert_eq!(health, Health::Ready);
        assert_eq!(platform, Platform { os: "macos".into(), arch: "arm64".into() });
        let missing = Fake { uname: "Linux x86_64", version: None, junk: 2 };
        assert_eq!(check(&missing).unwrap().0, Health::Missing);
    }

    #[test]
    fn cli_archive_names() {
        let p = Platform { os: "linux".into(), arch: "arm64".into() };
        assert_eq!(cli_archive("0.2.0", &p), "prism-cli-0.2.0-linux-arm64.tar.gz");
    }

    #[test]
    fn install_script_never_uses_sudo() {
        let script = download_script("0.2.0", &Platform { os: "linux".into(), arch: "x86_64".into() });
        for s in [script.as_str(), upload_script("0.2.0").as_str()] {
            assert!(!s.contains("sudo"), "{s}");
        }
        assert!(script.contains("https://github.com/gooonzick/prism/releases/download/v0.2.0/"), "{script}");
        let verify = script.find("sha256sum -c").expect("verifies");
        let rename = script.find("mv -f").expect("renames");
        assert!(verify < rename, "{script}");
    }

    /// A host whose `curl` serves files from `served`: `sh` with a temp `HOME` and a fake
    /// `curl` first on `PATH`.
    struct Offline {
        home: PathBuf,
        bin: PathBuf,
    }

    impl Offline {
        fn new(root: &Path, served: &Path) -> Offline {
            let (home, bin) = (root.join("home"), root.join("fakebin"));
            std::fs::create_dir_all(&home).unwrap();
            std::fs::create_dir_all(&bin).unwrap();
            let curl = bin.join("curl");
            // Called as: curl -fsSL -o OUT URL
            std::fs::write(&curl, format!("#!/bin/sh\ncp \"{}/${{4##*/}}\" \"$3\"\n", served.display())).unwrap();
            std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).unwrap();
            Offline { home, bin }
        }
        fn prism(&self) -> PathBuf {
            self.home.join(".local/bin/prism")
        }
    }

    impl Runner for Offline {
        fn prism(&self, _: &[String], _: Option<Vec<u8>>) -> Result<Output, String> {
            panic!("install runs no prism command")
        }
        fn shell(&self, script: &str, stdin: Option<Vec<u8>>) -> Result<Output, String> {
            assert!(stdin.is_none());
            let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap_or_default());
            let out = Command::new("sh").args(["-c", script]).env("HOME", &self.home).env("PATH", path).output().unwrap();
            Ok(Output { code: out.status.code().unwrap_or(-1), stdout: out.stdout, stderr: String::from_utf8_lossy(&out.stderr).into_owned() })
        }
    }

    /// A release folder: the archive for `p` holding a `prism` with `content`, and its sums.
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

    #[test]
    fn install_downloads_checks_and_replaces() {
        let tmp = tempfile::tempdir().unwrap();
        let served = tmp.path().join("served");
        std::fs::create_dir_all(&served).unwrap();
        let p = Platform { os: "linux".into(), arch: "x86_64".into() };
        release(&served, &p, "new prism");
        let host = Offline::new(tmp.path(), &served);
        std::fs::create_dir_all(host.prism().parent().unwrap()).unwrap();
        std::fs::write(host.prism(), "old prism").unwrap();

        install(&host, "9.9.9", &p, None).unwrap();
        assert_eq!(std::fs::read_to_string(host.prism()).unwrap(), "new prism");
        assert_eq!(std::fs::metadata(host.prism()).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(!host.prism().with_file_name("prism.new").exists());
    }

    #[test]
    fn a_bad_checksum_keeps_the_old_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let served = tmp.path().join("served");
        std::fs::create_dir_all(&served).unwrap();
        let p = Platform { os: "linux".into(), arch: "x86_64".into() };
        let archive = release(&served, &p, "new prism");
        std::fs::write(served.join("SHA256SUMS"), format!("{}  {archive}\n", "0".repeat(64))).unwrap();
        let host = Offline::new(tmp.path(), &served);
        std::fs::create_dir_all(host.prism().parent().unwrap()).unwrap();
        std::fs::write(host.prism(), "old prism").unwrap();

        let err = install(&host, "9.9.9", &p, None).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert_eq!(std::fs::read_to_string(host.prism()).unwrap(), "old prism");
        assert!(!host.prism().with_file_name("prism.new").exists());
    }

    #[test]
    fn install_uploads_a_binary_over_stdin() {
        let home = tempfile::tempdir().unwrap();
        let local = Local { name: "box".into(), home: home.path().into() };
        let binary = home.path().join("built-prism");
        std::fs::write(&binary, format!("#!/bin/sh\necho \"prism {VERSION}\"\n")).unwrap();
        // No release platform needed for an upload.
        let p = Platform { os: "freebsd".into(), arch: "amd64".into() };
        install(&local, VERSION, &p, Some(&binary)).unwrap();
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
        std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
        std::fs::write(&installed, "old prism").unwrap();
        let binary = home.path().join("built-prism");
        std::fs::write(&binary, "#!/bin/sh\necho \"prism 0.0.0-other\"\n").unwrap();
        let p = Platform { os: "freebsd".into(), arch: "amd64".into() };
        let err = install(&local, VERSION, &p, Some(&binary)).unwrap_err();
        assert!(err.contains("not prism"), "{err}");
        assert_eq!(std::fs::read_to_string(&installed).unwrap(), "old prism");
        assert!(!installed.with_file_name("prism.new").exists());
    }

    #[test]
    fn install_without_a_release_needs_a_binary() {
        let p = Platform { os: "freebsd".into(), arch: "amd64".into() };
        let err = install(&Fake { uname: "FreeBSD amd64", version: None, junk: 0 }, VERSION, &p, None).unwrap_err();
        assert!(err.contains("--binary"), "{err}");
    }
}
