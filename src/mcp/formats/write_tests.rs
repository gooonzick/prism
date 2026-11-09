//! Writers: each edit changes only its own entry, and what it writes reads back as written.

use std::collections::BTreeMap;
use std::path::Path;

use super::*;
use crate::mcp::model::{Part, Transport};

fn var(name: &str) -> EnvValue {
    EnvValue::var(name)
}

fn lit(text: &str) -> EnvValue {
    EnvValue::literal(text)
}

fn bearer(name: &str) -> EnvValue {
    EnvValue(vec![Part::Text("Bearer ".into()), Part::Var { name: name.into(), default: None }])
}

fn map(pairs: &[(&str, EnvValue)]) -> BTreeMap<String, EnvValue> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn stdio(name: &str, command: &str, args: &[&str], env: &[(&str, EnvValue)]) -> Server {
    let args = args.iter().map(|a| a.to_string()).collect();
    Server { name: name.into(), transport: Transport::Stdio { command: command.into(), args, env: map(env), cwd: None } }
}

fn http(name: &str, url: &str, headers: &[(&str, EnvValue)], sse: bool) -> Server {
    Server { name: name.into(), transport: Transport::Http { url: url.into(), headers: map(headers), sse } }
}

fn servers(flavor: Flavor, text: &str, place: &Place) -> Vec<Server> {
    read(flavor, text, place).unwrap().into_iter().map(|(s, _, _)| s).collect()
}

fn no_extras() -> Extras {
    Extras::new()
}

const CLAUDE_JSON: &str = r#"{
  "numStartups": 412,
  "theme": "dark",
  "tipsHistory": {"new-user-warmup": 7,"memory-command":3},
  "userID": "0000fake0000",
  "mcpServers": {
    "gitlab": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@zereight/mcp-gitlab"],
      "env": {
        "GITLAB_PERSONAL_ACCESS_TOKEN":   "${GITLAB_TOKEN}",
        "GITLAB_API_URL": "https://gitlab.example.com/api/v4"
      }
    },
    "context7": {"type": "http", "url": "https://mcp.context7.com/mcp", "headers": {"CONTEXT7_API_KEY": "fake-ctx7-key"}}
  },
  "projects": {
    "/Users/someone/work/app": {
      "allowedTools": [],
      "history": [{"display": "переименуй функцию ✨", "pastedContents": {}}],
      "mcpServers": {},
      "enabledMcpjsonServers": ["shared"],
      "disabledMcpjsonServers": [],
      "hasTrustDialogAccepted": true
    },
    "/Users/someone/work/café": {
      "allowedTools": ["Bash(git status)"],
      "mcpServers": { "figma": { "type": "sse", "url": "http://127.0.0.1:3845/sse" } },
      "disabledMcpServers": ["gitlab"]
    }
  },
  "oauthAccount": { "emailAddress": "someone@example.com" }
}
"#;

fn gitlab(api_url: &str) -> Server {
    stdio("gitlab", "npx", &["-y", "@zereight/mcp-gitlab"], &[("GITLAB_PERSONAL_ACCESS_TOKEN", var("GITLAB_TOKEN")), ("GITLAB_API_URL", lit(api_url))])
}

#[test]
fn claude_round_trip_touches_only_one_entry() {
    let out = upsert(Flavor::ClaudeCode, CLAUDE_JSON, &Place::Global, &gitlab("https://gitlab.example.org/api/v4"), &no_extras()).unwrap();
    let expected = r#"{
  "numStartups": 412,
  "theme": "dark",
  "tipsHistory": {"new-user-warmup": 7,"memory-command":3},
  "userID": "0000fake0000",
  "mcpServers": {
    "gitlab": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@zereight/mcp-gitlab"],
      "env": {
        "GITLAB_PERSONAL_ACCESS_TOKEN":   "${GITLAB_TOKEN}",
        "GITLAB_API_URL": "https://gitlab.example.org/api/v4"
      }
    },
    "context7": {"type": "http", "url": "https://mcp.context7.com/mcp", "headers": {"CONTEXT7_API_KEY": "fake-ctx7-key"}}
  },
  "projects": {
    "/Users/someone/work/app": {
      "allowedTools": [],
      "history": [{"display": "переименуй функцию ✨", "pastedContents": {}}],
      "mcpServers": {},
      "enabledMcpjsonServers": ["shared"],
      "disabledMcpjsonServers": [],
      "hasTrustDialogAccepted": true
    },
    "/Users/someone/work/café": {
      "allowedTools": ["Bash(git status)"],
      "mcpServers": { "figma": { "type": "sse", "url": "http://127.0.0.1:3845/sse" } },
      "disabledMcpServers": ["gitlab"]
    }
  },
  "oauthAccount": { "emailAddress": "someone@example.com" }
}
"#;
    assert_eq!(out, expected);

    let same = upsert(Flavor::ClaudeCode, CLAUDE_JSON, &Place::Global, &gitlab("https://gitlab.example.com/api/v4"), &no_extras()).unwrap();
    assert_eq!(same, CLAUDE_JSON, "an identical server leaves the file as it was");
}

#[test]
fn claude_local_scope_writes_into_the_project_entry() {
    let app = Place::ClaudeLocal("/Users/someone/work/app".into());
    let linear = http("linear", "https://mcp.linear.app/mcp", &[("Authorization", bearer("LINEAR_TOKEN"))], false);
    let out = upsert(Flavor::ClaudeCode, CLAUDE_JSON, &app, &linear, &no_extras()).unwrap();
    let expected = r#"{
  "numStartups": 412,
  "theme": "dark",
  "tipsHistory": {"new-user-warmup": 7,"memory-command":3},
  "userID": "0000fake0000",
  "mcpServers": {
    "gitlab": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@zereight/mcp-gitlab"],
      "env": {
        "GITLAB_PERSONAL_ACCESS_TOKEN":   "${GITLAB_TOKEN}",
        "GITLAB_API_URL": "https://gitlab.example.com/api/v4"
      }
    },
    "context7": {"type": "http", "url": "https://mcp.context7.com/mcp", "headers": {"CONTEXT7_API_KEY": "fake-ctx7-key"}}
  },
  "projects": {
    "/Users/someone/work/app": {
      "allowedTools": [],
      "history": [{"display": "переименуй функцию ✨", "pastedContents": {}}],
      "mcpServers": {
        "linear": {
          "type": "http",
          "url": "https://mcp.linear.app/mcp",
          "headers": {
            "Authorization": "Bearer ${LINEAR_TOKEN}"
          }
        }
      },
      "enabledMcpjsonServers": ["shared"],
      "disabledMcpjsonServers": [],
      "hasTrustDialogAccepted": true
    },
    "/Users/someone/work/café": {
      "allowedTools": ["Bash(git status)"],
      "mcpServers": { "figma": { "type": "sse", "url": "http://127.0.0.1:3845/sse" } },
      "disabledMcpServers": ["gitlab"]
    }
  },
  "oauthAccount": { "emailAddress": "someone@example.com" }
}
"#;
    assert_eq!(out, expected);
    assert_eq!(servers(Flavor::ClaudeCode, &out, &app), vec![linear]);
    assert_eq!(servers(Flavor::ClaudeCode, &out, &Place::Global), servers(Flavor::ClaudeCode, CLAUDE_JSON, &Place::Global));

    // A project Claude Code hasn't seen yet gets its own entry at the end of `projects`.
    let fresh = Place::ClaudeLocal("/Users/someone/work/new".into());
    let small = "{\n  \"projects\": {\n    \"/Users/someone/work/app\": { \"allowedTools\": [] }\n  }\n}\n";
    let out = upsert(Flavor::ClaudeCode, small, &fresh, &stdio("pw", "npx", &["@playwright/mcp"], &[]), &no_extras()).unwrap();
    let expected = r#"{
  "projects": {
    "/Users/someone/work/app": { "allowedTools": [] },
    "/Users/someone/work/new": {
      "mcpServers": {
        "pw": {
          "type": "stdio",
          "command": "npx",
          "args": ["@playwright/mcp"]
        }
      }
    }
  }
}
"#;
    assert_eq!(out, expected);
}

#[test]
fn claude_transport_change_replaces_transport_fields_and_keeps_the_rest() {
    let text = r#"{
  "mcpServers": {
    "figma": { "type": "stdio", "command": "npx", "args": ["-y", "figma-developer-mcp"], "env": { "FIGMA_API_KEY": "fake" }, "timeout": 30 }
  }
}
"#;
    let figma = http("figma", "http://127.0.0.1:3845/sse", &[], true);
    let out = upsert(Flavor::ClaudeCode, text, &Place::Global, &figma, &no_extras()).unwrap();
    let expected = r#"{
  "mcpServers": {
    "figma": {
      "type": "sse",
      "timeout": 30,
      "url": "http://127.0.0.1:3845/sse"
    }
  }
}
"#;
    assert_eq!(out, expected);
    let (got, extras, _) = read(Flavor::ClaudeCode, &out, &Place::Global).unwrap().remove(0);
    assert_eq!(got, figma);
    assert_eq!(extras["timeout"], 30);
}

const OPENCODE: &str = r#"{
  // OpenCode config, hand-written
  "$schema": "https://opencode.ai/config.json",
  "theme": "tokyonight",
  "mcp": {
    /* GitLab via npx */
    "gitlab": {
      "type": "local",
      "command": ["npx", "-y", "@zereight/mcp-gitlab"],
      "environment": { "GITLAB_TOKEN": "{env:GITLAB_TOKEN}", },
      "enabled": false, // off for now
    },
    "context7": {
      "type": "remote",
      "url": "https://mcp.context7.com/mcp",
      "headers": { "Authorization": "Bearer {env:CTX7}" },
    },
  },
  "agent": { "build": { "model": "anthropic/claude-sonnet-4" }, },
}
"#;

#[test]
fn opencode_round_trip_keeps_comments_and_trailing_commas() {
    let context7 = http("context7", "https://mcp.context7.com/v2/mcp", &[("Authorization", bearer("CTX7"))], false);
    let out = upsert(Flavor::OpenCode, OPENCODE, &Place::Global, &context7, &no_extras()).unwrap();
    let figma = stdio("figma", "npx", &["-y", "figma-developer-mcp", "--stdio"], &[("FIGMA_API_KEY", var("FIGMA_API_KEY"))]);
    let out = upsert(Flavor::OpenCode, &out, &Place::Global, &figma, &no_extras()).unwrap();
    let expected = r#"{
  // OpenCode config, hand-written
  "$schema": "https://opencode.ai/config.json",
  "theme": "tokyonight",
  "mcp": {
    /* GitLab via npx */
    "gitlab": {
      "type": "local",
      "command": ["npx", "-y", "@zereight/mcp-gitlab"],
      "environment": { "GITLAB_TOKEN": "{env:GITLAB_TOKEN}", },
      "enabled": false, // off for now
    },
    "context7": {
      "type": "remote",
      "url": "https://mcp.context7.com/v2/mcp",
      "headers": { "Authorization": "Bearer {env:CTX7}" },
    },
    "figma": {
      "type": "local",
      "command": ["npx", "-y", "figma-developer-mcp", "--stdio"],
      "environment": {
        "FIGMA_API_KEY": "{env:FIGMA_API_KEY}",
      },
    },
  },
  "agent": { "build": { "model": "anthropic/claude-sonnet-4" }, },
}
"#;
    assert_eq!(out, expected);
    let got = read(Flavor::OpenCode, &out, &Place::Global).unwrap();
    assert!(!got.iter().find(|(s, _, _)| s.name == "gitlab").unwrap().2, "gitlab stays switched off");

    let gitlab = stdio("gitlab", "npx", &["-y", "@zereight/mcp-gitlab"], &[("GITLAB_TOKEN", var("GITLAB_TOKEN"))]);
    assert_eq!(upsert(Flavor::OpenCode, OPENCODE, &Place::Global, &gitlab, &no_extras()).unwrap(), OPENCODE);
}

#[test]
fn opencode_writes_into_the_servers_layout_the_file_uses() {
    let text = r#"{
  "mcp": {
    "timeout": 10000,
    "servers": {
      "pw": { "type": "local", "command": ["npx", "@playwright/mcp"], "disabled": true }
    }
  }
}
"#;
    let ctx = http("context7", "https://mcp.context7.com/mcp", &[], false);
    let out = upsert(Flavor::OpenCode, text, &Place::Global, &ctx, &no_extras()).unwrap();
    let expected = r#"{
  "mcp": {
    "timeout": 10000,
    "servers": {
      "pw": { "type": "local", "command": ["npx", "@playwright/mcp"], "disabled": true },
      "context7": {
        "type": "remote",
        "url": "https://mcp.context7.com/mcp"
      }
    }
  }
}
"#;
    assert_eq!(out, expected);
    let on = set_disabled(Flavor::OpenCode, &out, Path::new("/w"), &Place::Project("/w".into()), "pw", false).unwrap();
    assert!(on.contains(r#""pw": { "type": "local", "command": ["npx", "@playwright/mcp"], "disabled": false }"#), "{on}");
}

const CODEX: &str = r#"# Codex config — hand-tuned
model = "gpt-5-codex"
approval_policy = "on-request"

[mcp_servers.gitlab]
command = "npx"
args = ["-y", "@zereight/mcp-gitlab"]   # pinned later
env_vars = ["GITLAB_TOKEN"]
startup_timeout_sec = 20

[mcp_servers.gitlab.env]
GITLAB_API_URL = "https://gitlab.example.com/api/v4"

# Context7 over HTTP
[mcp_servers.context7]
url = "https://mcp.context7.com/mcp"
bearer_token_env_var = "CTX7_TOKEN"

[mcp_servers.figma]
command = "npx"
args = ["-y", "figma-developer-mcp", "--stdio"]
env = { FIGMA_API_KEY = "fake-figma-key", LOG = "warn" }

[profiles.fast]
model = "gpt-5-mini"   # cheaper
"#;

fn codex_figma(key: &str) -> Server {
    stdio("figma", "npx", &["-y", "figma-developer-mcp", "--stdio"], &[("FIGMA_API_KEY", lit(key)), ("LOG", lit("warn"))])
}

fn codex_gitlab(api_url: &str) -> Server {
    stdio("gitlab", "npx", &["-y", "@zereight/mcp-gitlab"], &[("GITLAB_TOKEN", var("GITLAB_TOKEN")), ("GITLAB_API_URL", lit(api_url))])
}

#[test]
fn codex_round_trip_keeps_comments_and_other_tables() {
    let out = upsert(Flavor::Codex, CODEX, &Place::Global, &codex_gitlab("https://gitlab.example.org/api/v4"), &no_extras()).unwrap();
    let out = upsert(Flavor::Codex, &out, &Place::Global, &codex_figma("fake-figma-key-2"), &no_extras()).unwrap();
    let brave = stdio("brave", "npx", &["-y", "@brave/brave-search-mcp-server"], &[("BRAVE_API_KEY", var("BRAVE_API_KEY"))]);
    let out = upsert(Flavor::Codex, &out, &Place::Global, &brave, &no_extras()).unwrap();
    let expected = r#"# Codex config — hand-tuned
model = "gpt-5-codex"
approval_policy = "on-request"

[mcp_servers.gitlab]
command = "npx"
args = ["-y", "@zereight/mcp-gitlab"]   # pinned later
env_vars = ["GITLAB_TOKEN"]
startup_timeout_sec = 20

[mcp_servers.gitlab.env]
GITLAB_API_URL = "https://gitlab.example.org/api/v4"

# Context7 over HTTP
[mcp_servers.context7]
url = "https://mcp.context7.com/mcp"
bearer_token_env_var = "CTX7_TOKEN"

[mcp_servers.figma]
command = "npx"
args = ["-y", "figma-developer-mcp", "--stdio"]
env = { FIGMA_API_KEY = "fake-figma-key-2", LOG = "warn" }

[mcp_servers.brave]
command = "npx"
args = ["-y", "@brave/brave-search-mcp-server"]
env_vars = ["BRAVE_API_KEY"]

[profiles.fast]
model = "gpt-5-mini"   # cheaper
"#;
    assert_eq!(out, expected);

    for same in [codex_gitlab("https://gitlab.example.com/api/v4"), codex_figma("fake-figma-key")] {
        assert_eq!(upsert(Flavor::Codex, CODEX, &Place::Global, &same, &no_extras()).unwrap(), CODEX, "{}", same.name);
    }
}

#[test]
fn codex_encodes_references_in_its_own_keys() {
    let remote = http("remote", "https://example.com/mcp", &[("Authorization", bearer("API_TOKEN")), ("X-Team", var("TEAM_ID")), ("X-Static", lit("fake-static"))], false);
    let out = upsert(Flavor::Codex, "", &Place::Global, &remote, &no_extras()).unwrap();
    let expected = r#"[mcp_servers.remote]
url = "https://example.com/mcp"
bearer_token_env_var = "API_TOKEN"
http_headers = { X-Static = "fake-static" }
env_http_headers = { X-Team = "TEAM_ID" }
"#;
    assert_eq!(out, expected);
    assert_eq!(servers(Flavor::Codex, &out, &Place::Global), vec![remote]);

    let renamed_var = stdio("x", "x", &[], &[("TOKEN", var("OTHER"))]);
    assert_eq!(upsert(Flavor::Codex, "", &Place::Global, &renamed_var, &no_extras()), Err("Codex can't express env TOKEN".into()));
    let mixed = http("x", "https://x.example.com", &[("X-Key", EnvValue(vec![Part::Text("k-".into()), Part::Var { name: "K".into(), default: None }]))], false);
    assert_eq!(upsert(Flavor::Codex, "", &Place::Global, &mixed, &no_extras()), Err("Codex can't express header X-Key".into()));
}

#[test]
fn cursor_upsert_into_empty_file_creates_document() {
    let ctx = http("context7", "https://mcp.context7.com/mcp", &[("CONTEXT7_API_KEY", var("CONTEXT7_API_KEY"))], false);
    let out = upsert(Flavor::Cursor, "", &Place::Global, &ctx, &no_extras()).unwrap();
    let expected = r#"{
  "mcpServers": {
    "context7": {
      "url": "https://mcp.context7.com/mcp",
      "headers": {
        "CONTEXT7_API_KEY": "${env:CONTEXT7_API_KEY}"
      }
    }
  }
}
"#;
    assert_eq!(out, expected);
}

#[test]
fn new_entries_take_the_files_indent_and_the_given_extras() {
    let text = "{\n\t\"mcpServers\": {\n\t\t\"a\": { \"command\": \"a\" }\n\t}\n}\n";
    let extras = serde_json::json!({ "envFile": ".env" }).as_object().unwrap().clone();
    let out = upsert(Flavor::Cursor, text, &Place::Global, &stdio("b", "b", &["--x"], &[]), &extras).unwrap();
    let expected = "{\n\t\"mcpServers\": {\n\t\t\"a\": { \"command\": \"a\" },\n\t\t\"b\": {\n\t\t\t\"command\": \"b\",\n\t\t\t\"args\": [\"--x\"],\n\t\t\t\"envFile\": \".env\"\n\t\t}\n\t}\n}\n";
    assert_eq!(out, expected);
    let (_, got_extras, _) = read(Flavor::Cursor, &out, &Place::Global).unwrap().remove(1);
    assert_eq!(got_extras, extras);
}

#[test]
fn remove_last_server_leaves_empty_object() {
    let text = "{\n  \"mcpServers\": {\n    \"figma\": { \"command\": \"npx\", \"args\": [\"figma-developer-mcp\"] }\n  },\n  \"other\": 1\n}\n";
    let out = remove(Flavor::Cursor, text, &Place::Global, "figma").unwrap();
    assert_eq!(out, "{\n  \"mcpServers\": {},\n  \"other\": 1\n}\n");
    assert_eq!(remove(Flavor::Cursor, &out, &Place::Global, "figma").unwrap(), out, "removing what isn't there changes nothing");

    let out = remove(Flavor::Codex, CODEX, &Place::Global, "gitlab").unwrap();
    assert!(!out.contains("gitlab") && out.contains("# Context7 over HTTP\n[mcp_servers.context7]"), "{out}");
}

#[test]
fn desktop_rejects_env_reference() {
    let s = stdio("fs", "npx", &["-y", "@modelcontextprotocol/server-filesystem"], &[("TOKEN", var("FAKE_SECRET_NAME"))]);
    assert_eq!(upsert(Flavor::ClaudeDesktop, "", &Place::Global, &s, &no_extras()), Err("Claude Desktop can't use environment references".into()));
    let remote = http("r", "https://r.example.com/mcp", &[], false);
    assert!(upsert(Flavor::ClaudeDesktop, "", &Place::Global, &remote, &no_extras()).unwrap_err().starts_with("Claude Desktop can't express"));
    let kiro_default = stdio("k", "k", &[], &[("K", EnvValue(vec![Part::Var { name: "K".into(), default: Some("fake-default".into()) }]))]);
    assert_eq!(upsert(Flavor::Kiro, "", &Place::Global, &kiro_default, &no_extras()), Err("Kiro can't express env K".into()));
}

#[test]
fn claude_disable_adds_to_project_list_once() {
    let app = Path::new("/Users/someone/work/app");
    let off = set_disabled(Flavor::ClaudeCode, CLAUDE_JSON, app, &Place::Global, "context7", true).unwrap();
    let expected = CLAUDE_JSON.replace("      \"hasTrustDialogAccepted\": true\n", "      \"hasTrustDialogAccepted\": true,\n      \"disabledMcpServers\": [\"context7\"]\n");
    assert_eq!(off, expected);
    assert_eq!(set_disabled(Flavor::ClaudeCode, &off, app, &Place::Global, "context7", true).unwrap(), off, "a second disable changes nothing");
    assert_eq!(disabled_in(&off, app).servers, ["context7"]);
    let on = set_disabled(Flavor::ClaudeCode, &off, app, &Place::Global, "context7", false).unwrap();
    assert_eq!(disabled_in(&on, app), disabled_in(CLAUDE_JSON, app));

    let shared = set_disabled(Flavor::ClaudeCode, CLAUDE_JSON, app, &Place::Project(app.into()), "shared", true).unwrap();
    assert_eq!(shared, CLAUDE_JSON.replace("\"disabledMcpjsonServers\": []", "\"disabledMcpjsonServers\": [\"shared\"]"));
    let cafe = Path::new("/Users/someone/work/café");
    let again = set_disabled(Flavor::ClaudeCode, CLAUDE_JSON, cafe, &Place::ClaudeLocal(cafe.into()), "gitlab", true).unwrap();
    assert_eq!(again, CLAUDE_JSON);

    assert!(set_disabled(Flavor::Cursor, "{}", app, &Place::Global, "x", true).is_err());
    assert!(set_disabled(Flavor::Codex, CODEX, app, &Place::Global, "gitlab", true).is_err(), "a global Codex server isn't the project's to switch off");
    let codex_off = set_disabled(Flavor::Codex, CODEX, app, &Place::Project(app.into()), "gitlab", true).unwrap();
    assert_eq!(codex_off, CODEX.replace("startup_timeout_sec = 20\n", "startup_timeout_sec = 20\nenabled = false\n"));
}

#[test]
fn rename_keeps_position() {
    let out = rename(Flavor::ClaudeCode, CLAUDE_JSON, &Place::Global, "gitlab", "gitlab-work").unwrap();
    assert_eq!(out, CLAUDE_JSON.replace("    \"gitlab\": {", "    \"gitlab-work\": {"));
    assert!(rename(Flavor::ClaudeCode, CLAUDE_JSON, &Place::Global, "gitlab", "context7").is_err(), "never over another server");

    let out = rename(Flavor::Codex, CODEX, &Place::Global, "gitlab", "gitlab-work").unwrap();
    assert_eq!(out, CODEX.replace("[mcp_servers.gitlab]", "[mcp_servers.gitlab-work]").replace("[mcp_servers.gitlab.env]", "[mcp_servers.gitlab-work.env]"));

    let out = rename(Flavor::OpenCode, OPENCODE, &Place::Global, "gitlab", "gl").unwrap();
    assert_eq!(out, OPENCODE.replace("    \"gitlab\": {", "    \"gl\": {"));
}

/// One file per flavour with a server already in it, in that agent's own reference syntax.
fn start(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::ClaudeCode => r#"{ "mcpServers": { "keep": { "command": "k", "env": { "T": "${T:-fake-default}" } } } }"#,
        Flavor::ClaudeDesktop => r#"{ "mcpServers": { "keep": { "command": "k", "env": { "T": "${NOT_A_REF}" } } }, "preferences": {} }"#,
        Flavor::Codex => "[mcp_servers.keep]\ncommand = \"k\"\nenv = { PORT = 3000 }\n",
        Flavor::Cursor => r#"{ "mcpServers": { "keep": { "type": "sse", "url": "https://k.example.com/sse", "headers": { "A": "${env:T}" } } } }"#,
        Flavor::OpenCode => r#"{ "mcp": { "keep": { "type": "local", "command": ["k"], "environment": { "T": "{env:T}" }, "cwd": "/w" } } }"#,
        Flavor::Gemini => r#"{ "mcpServers": { "keep": { "command": "k", "args": [], "env": { "T": "$T" } }, "sse": { "url": "https://s.example.com/sse" } } }"#,
        Flavor::Windsurf => r#"{ "mcpServers": { "keep": { "url": "https://k.example.com/mcp", "headers": { "Authorization": "Bearer ${env:T}" }, "disabled": true } } }"#,
        Flavor::Kiro => r#"{ "mcpServers": { "keep": { "command": "k", "env": { "T": "${T}", "N": 5 }, "autoApprove": [] } } }"#,
    }
}

#[test]
fn every_flavour_reads_back_what_it_wrote_and_leaves_identical_servers_alone() {
    let flavors = [Flavor::ClaudeCode, Flavor::ClaudeDesktop, Flavor::Codex, Flavor::Cursor, Flavor::OpenCode, Flavor::Gemini, Flavor::Windsurf, Flavor::Kiro];
    for flavor in flavors {
        let text = start(flavor);
        let before = servers(flavor, text, &Place::Global);
        for existing in &before {
            assert_eq!(upsert(flavor, text, &Place::Global, existing, &no_extras()).unwrap(), text, "{flavor:?} {}", existing.name);
        }

        let refs = flavor != Flavor::ClaudeDesktop;
        let token = if refs { var("GITLAB_TOKEN") } else { lit("fake-token") };
        let mut new = vec![stdio("gitlab", "npx", &["-y", "@zereight/mcp-gitlab"], &[("GITLAB_TOKEN", token), ("READ_ONLY", lit("true"))])];
        if refs {
            new.push(http("remote", "https://r.example.com/mcp", &[("Authorization", bearer("R_TOKEN"))], false));
        }
        if !matches!(flavor, Flavor::ClaudeDesktop | Flavor::Codex | Flavor::OpenCode) {
            new.push(http("events", "https://e.example.com/sse", &[], true));
        }
        let mut out = text.to_string();
        for s in &new {
            out = upsert(flavor, &out, &Place::Global, s, &no_extras()).unwrap_or_else(|e| panic!("{flavor:?} {}: {e}", s.name));
            assert!(servers(flavor, &out, &Place::Global).contains(s), "{flavor:?} {}:\n{out}", s.name);
            assert_eq!(upsert(flavor, &out, &Place::Global, s, &no_extras()).unwrap(), out, "{flavor:?} {} again", s.name);
        }
        let after = servers(flavor, &out, &Place::Global);
        assert!(before.iter().all(|s| after.contains(s)), "{flavor:?}: the other servers stay");

        // Switching transport keeps reading back, and drops what belonged to the old one.
        let flipped = http("gitlab", "https://gitlab.example.com/mcp", &[], false);
        if flavor != Flavor::ClaudeDesktop {
            let out = upsert(flavor, &out, &Place::Global, &flipped, &no_extras()).unwrap();
            assert!(servers(flavor, &out, &Place::Global).contains(&flipped), "{flavor:?} flipped:\n{out}");
            assert!(!out.contains("zereight"), "{flavor:?}: args went with the command");
        }

        let gone = remove(flavor, &out, &Place::Global, "gitlab").unwrap();
        assert!(!servers(flavor, &gone, &Place::Global).iter().any(|s| s.name == "gitlab"), "{flavor:?}");
    }
}

/// Claude Code keys `projects` by the canonical path; a write through a symlinked checkout lands
/// in that entry rather than making a second one.
#[cfg(unix)]
#[test]
fn claude_local_writes_find_the_canonical_project_key() {
    let home = tempfile::tempdir().unwrap();
    let real = home.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = home.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let key = std::fs::canonicalize(&real).unwrap().to_string_lossy().into_owned();
    let text = serde_json::to_string_pretty(&serde_json::json!({ "projects": { &key: { "mcpServers": {} } } })).unwrap();
    let pw = stdio("pw", "npx", &["@playwright/mcp"], &[]);
    let out = upsert(Flavor::ClaudeCode, &text, &Place::ClaudeLocal(link.clone()), &pw, &no_extras()).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["projects"].as_object().unwrap().len(), 1);
    assert_eq!(doc["projects"][&key]["mcpServers"]["pw"]["command"], "npx");
    let off = set_disabled(Flavor::ClaudeCode, &out, &link, &Place::ClaudeLocal(link.clone()), "pw", true).unwrap();
    assert_eq!(disabled_in(&off, &real).servers, ["pw"]);
}
