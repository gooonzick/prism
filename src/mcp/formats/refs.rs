//! Environment-variable references in env and header values, in each agent's own syntax.

use crate::mcp::model::{EnvValue, Part};

/// How an agent writes a reference to an environment variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    /// Every value is literal (Claude Desktop, Codex `env`).
    None,
    /// `${VAR}` and `${VAR:-default}` (Claude Code).
    Shell,
    /// `${VAR}` only (Kiro).
    Braced,
    /// `$VAR` and `${VAR}` (Gemini CLI).
    Dollar,
    /// `${env:VAR}` (Cursor, Windsurf).
    EnvColon,
    /// `{env:VAR}` (OpenCode).
    Brace,
}

fn is_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn name_len(s: &str) -> usize {
    let b = s.as_bytes();
    if b.first().is_some_and(|&c| is_name_start(c)) { b.iter().take_while(|&&c| c.is_ascii_alphanumeric() || c == b'_').count() } else { 0 }
}

/// A reference at the start of `s`: the part and how many bytes it took.
fn reference(s: &str, syntax: Syntax) -> Option<(Part, usize)> {
    let var = |name: &str, default: Option<&str>| Part::Var { name: name.into(), default: default.map(Into::into) };
    // `open` NAME, then `}` or (Shell only) `:-default}`.
    let braced = |open: &str, defaults: bool| -> Option<(Part, usize)> {
        let rest = s.strip_prefix(open)?;
        let n = name_len(rest);
        if n == 0 {
            return None;
        }
        let (name, after) = rest.split_at(n);
        if after.starts_with('}') {
            return Some((var(name, None), open.len() + n + 1));
        }
        let default = after.strip_prefix(":-").filter(|_| defaults)?;
        let end = default.find('}')?;
        Some((var(name, Some(&default[..end])), open.len() + n + 2 + end + 1))
    };
    match syntax {
        Syntax::None => None,
        Syntax::Shell => braced("${", true),
        Syntax::Braced => braced("${", false),
        Syntax::EnvColon => braced("${env:", false),
        Syntax::Brace => braced("{env:", false),
        Syntax::Dollar => braced("${", false).or_else(|| {
            let n = name_len(s.strip_prefix('$')?);
            (n > 0).then(|| (var(&s[1..=n], None), n + 1))
        }),
    }
}

/// `text` split into literal text and references.
pub fn decode(text: &str, syntax: Syntax) -> EnvValue {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut i = 0;
    while i < text.len() {
        if let Some((part, len)) = reference(&text[i..], syntax) {
            if !literal.is_empty() {
                parts.push(Part::Text(std::mem::take(&mut literal)));
            }
            parts.push(part);
            i += len;
        } else {
            let c = text[i..].chars().next().unwrap_or_default();
            literal.push(c);
            i += c.len_utf8();
        }
    }
    if !literal.is_empty() || parts.is_empty() {
        parts.push(Part::Text(literal));
    }
    EnvValue(parts)
}

/// `value` in `syntax`, or `None` when the syntax can't say it: a reference where the agent has
/// none, a default where only `${VAR:-x}` takes one, or literal text that would read back as a
/// reference. Whatever comes back decodes to `value`.
pub fn encode(value: &EnvValue, syntax: Syntax) -> Option<String> {
    let mut out = String::new();
    for part in &value.0 {
        match (part, syntax) {
            (Part::Text(t), _) => out.push_str(t),
            (Part::Var { .. }, Syntax::None) => return None,
            (Part::Var { name, default: Some(d) }, Syntax::Shell) => out.push_str(&format!("${{{name}:-{d}}}")),
            (Part::Var { default: Some(_), .. }, _) => return None,
            (Part::Var { name, .. }, Syntax::Shell | Syntax::Braced | Syntax::Dollar) => out.push_str(&format!("${{{name}}}")),
            (Part::Var { name, .. }, Syntax::EnvColon) => out.push_str(&format!("${{env:{name}}}")),
            (Part::Var { name, .. }, Syntax::Brace) => out.push_str(&format!("{{env:{name}}}")),
        }
    }
    (decode(&out, syntax) == normalized(value)).then_some(out)
}

/// `value` as `decode` would give it back: adjacent texts joined, empty ones dropped.
fn normalized(value: &EnvValue) -> EnvValue {
    let mut parts: Vec<Part> = Vec::new();
    for part in &value.0 {
        match (part, parts.last_mut()) {
            (Part::Text(t), _) if t.is_empty() => {}
            (Part::Text(t), Some(Part::Text(last))) => last.push_str(t),
            _ => parts.push(part.clone()),
        }
    }
    if parts.is_empty() {
        parts.push(Part::Text(String::new()));
    }
    EnvValue(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> Part {
        Part::Text(t.into())
    }

    fn var(name: &str) -> Part {
        Part::Var { name: name.into(), default: None }
    }

    #[test]
    fn each_syntax_reads_its_own_references_only() {
        assert_eq!(decode("${A}", Syntax::Shell).0, vec![var("A")]);
        assert_eq!(decode("${A:-x}", Syntax::Shell).0, vec![Part::Var { name: "A".into(), default: Some("x".into()) }]);
        assert_eq!(decode("${A:-x}", Syntax::Braced).0, vec![text("${A:-x}")]);
        assert_eq!(decode("$A and ${B}", Syntax::Dollar).0, vec![var("A"), text(" and "), var("B")]);
        assert_eq!(decode("$A", Syntax::Shell).0, vec![text("$A")]);
        assert_eq!(decode("Bearer ${env:T}", Syntax::EnvColon).0, vec![text("Bearer "), var("T")]);
        assert_eq!(decode("${T}", Syntax::EnvColon).0, vec![text("${T}")]);
        assert_eq!(decode("{env:T}{file:~/k}", Syntax::Brace).0, vec![var("T"), text("{file:~/k}")]);
        assert_eq!(decode("${A}", Syntax::None).0, vec![text("${A}")]);
        assert_eq!(decode("", Syntax::Shell).0, vec![text("")]);
        assert_eq!(decode("ünï$", Syntax::Dollar).0, vec![text("ünï$")]);
    }

    #[test]
    fn encode_is_the_inverse_of_decode() {
        let bearer = EnvValue(vec![text("Bearer "), var("T")]);
        assert_eq!(encode(&bearer, Syntax::Shell).as_deref(), Some("Bearer ${T}"));
        assert_eq!(encode(&bearer, Syntax::EnvColon).as_deref(), Some("Bearer ${env:T}"));
        assert_eq!(encode(&bearer, Syntax::Brace).as_deref(), Some("Bearer {env:T}"));
        assert_eq!(encode(&bearer, Syntax::Dollar).as_deref(), Some("Bearer ${T}"));
        assert_eq!(encode(&bearer, Syntax::None), None);
        let with_default = EnvValue(vec![Part::Var { name: "A".into(), default: Some("x".into()) }]);
        assert_eq!(encode(&with_default, Syntax::Shell).as_deref(), Some("${A:-x}"));
        assert_eq!(encode(&with_default, Syntax::Braced), None);
        assert_eq!(encode(&EnvValue::literal("${A}"), Syntax::None).as_deref(), Some("${A}"));
        assert_eq!(encode(&EnvValue::literal("${A}"), Syntax::Shell), None, "would read back as a reference");
        assert_eq!(encode(&EnvValue(vec![text("a"), text("b")]), Syntax::Shell).as_deref(), Some("ab"));
    }
}
