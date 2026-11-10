//! The built `prism` binary, as another machine runs it: `prism hosts check` reads exactly
//! this line. Also makes `cargo test` build the binary, which the `Here` runner's test runs.

use std::process::Command;

#[test]
fn the_binary_reports_its_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_prism")).arg("--version").output().expect("the binary runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), format!("prism {}\n", env!("CARGO_PKG_VERSION")));
}
