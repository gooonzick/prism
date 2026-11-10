//! Saved remote hosts: a name Prism shows and the ssh target it connects to.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Host {
    pub name: String,
    /// What follows `ssh`: `user@host` or an alias from `~/.ssh/config`.
    pub ssh: String,
}

pub fn find<'a>(hosts: &'a [Host], name: &str) -> Option<&'a Host> {
    hosts.iter().find(|h| h.name == name)
}

/// 1-32 characters of letters, digits, `.`, `_` and `-`; "local" and "this" mean this machine.
pub fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 32 || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return Err(format!("{name:?} is not a host name: use 1-32 letters, digits, '.', '_' or '-'"));
    }
    if matches!(name, "local" | "this") {
        return Err(format!("{name:?} is reserved for this machine"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert!(valid_name("dev-box_1.lan").is_ok());
        assert!(valid_name("a b").is_err());
        assert!(valid_name("this").is_err());
        assert!(valid_name(&"x".repeat(33)).is_err());
    }

    #[test]
    fn find_matches_the_exact_name() {
        let hosts = vec![Host { name: "a".into(), ssh: "u@a".into() }];
        assert_eq!(find(&hosts, "a").map(|h| h.ssh.as_str()), Some("u@a"));
        assert!(find(&hosts, "b").is_none());
    }
}
