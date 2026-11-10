//! What may not work on another machine when a server definition moves there: paths into the
//! sending machine's home or package managers, and environment variables it expects.

use std::path::Path;

use crate::mcp::model::{EnvValue, Part, Server, Transport, masked_args, masked_url};

/// Folders whose contents are particular to one machine, besides its home.
const MACHINE_DIRS: [&str; 2] = ["/opt/homebrew", "/usr/local/Cellar"];

/// One heads-up per value that points into `source_home` or a package manager's folder on
/// `source` (the sending machine's name), and per `${VAR}` an env or header value reads, each
/// naming `target`, the machine it goes to. Names fields as plans do (`command`, `args`,
/// `env.TOKEN`). A command, argument or folder shows masked as local output shows it; an env or
/// header value never shows, only where it points.
pub fn warnings(server: &Server, source_home: &Path, source: &str, target: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |line: String| {
        if !out.contains(&line) {
            out.push(line);
        }
    };
    let keyed: Vec<(String, &EnvValue)> = match &server.transport {
        Transport::Stdio { command, args, env, cwd } => {
            let shown = masked_args(args);
            let plain = [("command", command, masked_url(command))].into_iter();
            let plain = plain.chain(args.iter().zip(shown).map(|(a, m)| ("args", a, m)));
            let plain = plain.chain(cwd.iter().map(|c| ("cwd", c, masked_url(c))));
            for (field, value, shown) in plain {
                if machine_place(value, source_home).is_some() {
                    push(format!("{field} points at {shown} on {source}; check it exists on {target}"));
                }
            }
            env.iter().map(|(k, v)| (format!("env.{k}"), v)).collect()
        }
        Transport::Http { headers, .. } => headers.iter().map(|(k, v)| (format!("headers.{k}"), v)).collect(),
    };
    for (field, value) in &keyed {
        // The value may be a secret: say only where it points.
        match machine_place(&value.written(), source_home) {
            Some(None) => push(format!("{field} points into {source}'s home; check it exists on {target}")),
            Some(Some(dir)) => push(format!("{field} points into {dir} on {source}; check it exists on {target}")),
            None => {}
        }
    }
    for (field, value) in &keyed {
        for part in &value.0 {
            if let Part::Var { name, .. } = part {
                push(format!("{field} needs ${{{name}}} set on {target}"));
            }
        }
    }
    out
}

/// Where `value` points if it is a path particular to the sending machine: `Some(None)` inside
/// `home` (not the root), `Some(Some(dir))` inside one of [`MACHINE_DIRS`].
fn machine_place(value: &str, home: &Path) -> Option<Option<&'static str>> {
    let value = Path::new(value);
    if !value.is_absolute() {
        return None;
    }
    if home.is_absolute() && home.parent().is_some() && value.starts_with(home) {
        return Some(None);
    }
    MACHINE_DIRS.iter().find(|d| value.starts_with(d)).map(|d| Some(*d))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn stdio(command: &str, args: &[&str], env: &[(&str, EnvValue)], cwd: Option<&str>) -> Server {
        let env: BTreeMap<String, EnvValue> = env.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        Server { name: "s".into(), transport: Transport::Stdio { command: command.into(), args: args.iter().map(|a| a.to_string()).collect(), env, cwd: cwd.map(String::from) } }
    }

    #[test]
    fn paths_into_the_senders_machine_are_named() {
        let server = stdio(
            "/opt/homebrew/bin/node",
            &["/Users/me/mcp/server.js", "--port", "/usr/local/Cellar/x/1/bin/y", "/Users/meg/other.js", "--token", "/Users/me/FIXTURE_ARG_SECRET"],
            &[
                ("CONFIG", EnvValue::literal("/Users/me/.config/s.json")),
                ("BREW", EnvValue::literal("/opt/homebrew/etc/FIXTURE_BREW_SECRET")),
                ("API_KEY", EnvValue::literal("/Users/me/FIXTURE_ENV_SECRET")),
            ],
            Some("/Users/me/mcp"),
        );
        let got = warnings(&server, Path::new("/Users/me"), "box", "laptop");
        assert_eq!(
            got,
            [
                "command points at /opt/homebrew/bin/node on box; check it exists on laptop",
                "args points at /Users/me/mcp/server.js on box; check it exists on laptop",
                "args points at /usr/local/Cellar/x/1/bin/y on box; check it exists on laptop",
                "args points at *** on box; check it exists on laptop",
                "cwd points at /Users/me/mcp on box; check it exists on laptop",
                "env.API_KEY points into box's home; check it exists on laptop",
                "env.BREW points into /opt/homebrew on box; check it exists on laptop",
                "env.CONFIG points into box's home; check it exists on laptop",
            ]
        );
        for w in &got {
            assert!(!w.contains("FIXTURE"), "a value leaked: {w}");
        }
    }

    #[test]
    fn references_need_their_variable() {
        let server = Server {
            name: "s".into(),
            transport: Transport::Http {
                url: "https://example.com/mcp".into(),
                headers: [("Authorization".to_string(), EnvValue(vec![Part::Text("Bearer ".into()), Part::Var { name: "TOKEN".into(), default: None }]))].into(),
                sse: false,
            },
        };
        assert_eq!(warnings(&server, Path::new("/home/me"), "box", "laptop"), ["headers.Authorization needs ${TOKEN} set on laptop"]);
    }

    #[test]
    fn a_root_or_empty_home_matches_nothing() {
        let server = stdio("/usr/bin/node", &["x"], &[], None);
        assert!(warnings(&server, Path::new("/"), "box", "laptop").is_empty());
        assert!(warnings(&server, Path::new(""), "box", "laptop").is_empty());
    }
}
