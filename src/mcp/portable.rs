//! What may not work on another machine when a server definition moves there: paths into the
//! sending machine's home or package managers, and environment variables it expects.

use std::path::Path;

use crate::mcp::model::{EnvValue, Part, Server, Transport};

/// Folders whose contents are particular to one machine, besides its home.
const MACHINE_DIRS: [&str; 2] = ["/opt/homebrew", "/usr/local/Cellar"];

/// One heads-up per value that points into `source_home` or a package manager's folder on
/// `source` (the sending machine's name), and per `${VAR}` an env or header value reads.
/// Names fields as plans do (`command`, `args`, `env.TOKEN`); a value shows only when it is
/// such a path.
pub fn warnings(server: &Server, source_home: &Path, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |line: String| {
        if !out.contains(&line) {
            out.push(line);
        }
    };
    let mut path = |field: &str, value: &str| {
        if machine_path(value, source_home) {
            push(format!("{field} points at {value} on {source}; check it exists here"));
        }
    };
    let keyed: Vec<(String, &EnvValue)> = match &server.transport {
        Transport::Stdio { command, args, env, cwd } => {
            path("command", command);
            args.iter().for_each(|a| path("args", a));
            cwd.iter().for_each(|c| path("cwd", c));
            env.iter().map(|(k, v)| (format!("env.{k}"), v)).collect()
        }
        Transport::Http { headers, .. } => headers.iter().map(|(k, v)| (format!("headers.{k}"), v)).collect(),
    };
    for (field, value) in &keyed {
        path(field, &value.written());
    }
    for (field, value) in &keyed {
        for part in &value.0 {
            if let Part::Var { name, .. } = part {
                push(format!("{field} needs ${{{name}}} set here"));
            }
        }
    }
    out
}

/// Whether `value` is a path inside `home` (not the root) or one of [`MACHINE_DIRS`].
fn machine_path(value: &str, home: &Path) -> bool {
    let value = Path::new(value);
    if !value.is_absolute() {
        return false;
    }
    let in_home = home.is_absolute() && home.parent().is_some() && value.starts_with(home);
    in_home || MACHINE_DIRS.iter().any(|d| value.starts_with(d))
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
            &["/Users/me/mcp/server.js", "--port", "/usr/local/Cellar/x/1/bin/y", "/Users/meg/other.js"],
            &[("CONFIG", EnvValue::literal("/Users/me/.config/s.json")), ("API_KEY", EnvValue::literal("FIXTURE_SECRET"))],
            Some("/Users/me/mcp"),
        );
        assert_eq!(
            warnings(&server, Path::new("/Users/me"), "box"),
            [
                "command points at /opt/homebrew/bin/node on box; check it exists here",
                "args points at /Users/me/mcp/server.js on box; check it exists here",
                "args points at /usr/local/Cellar/x/1/bin/y on box; check it exists here",
                "cwd points at /Users/me/mcp on box; check it exists here",
                "env.CONFIG points at /Users/me/.config/s.json on box; check it exists here",
            ]
        );
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
        assert_eq!(warnings(&server, Path::new("/home/me"), "box"), ["headers.Authorization needs ${TOKEN} set here"]);
    }

    #[test]
    fn a_root_or_empty_home_matches_nothing() {
        let server = stdio("/usr/bin/node", &["x"], &[], None);
        assert!(warnings(&server, Path::new("/"), "box").is_empty());
        assert!(warnings(&server, Path::new(""), "box").is_empty());
    }
}
