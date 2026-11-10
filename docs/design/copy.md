# Copy

Prism talks like a careful colleague: short, plain, specific about files. It names what will
happen to which file before it happens, and never says more than one sentence where a phrase
will do.

## Rules

- **Sentence case everywhere**: "Add server", "Show values", "Install Command Line Tool…" is the
  one exception (a macOS menu item, which uses title case).
- **`…` (one character, not three dots) on a button that opens a dialog or needs another step**:
  "Copy to…", "Edit…", "Remove…", "Merge…", "Add project…". A button that acts at once has
  none: "Sync 3 skills", "Install", "Use link".
- **Buttons name the action and its object or count**: "Sync 3 skills", "Remove 3", "Review
  changes". Never "OK", "Submit", "Confirm", "Yes".
- **Status words are a closed set**; reuse one, never coin a new one ("off", "error", "ok" don't
  exist). Lowercase, after a dot, no verb:
  - skill rows (`sync_status`, `src/ui/installed.rs`): "synced", "N to link", "not in .agents",
    "broken links", "broken in .agents";
  - skill placements (`Placement::label`, `src/scan.rs`): "linked", "its own copy",
    "links elsewhere", "broken link", "not linked";
  - MCP rows (`status`, `src/ui/mcp.rs`): "in sync", "differs", "read-only";
  - tags: "global", "disabled"; in an agent's detail line: "off here", "managed by <app>".

  A new state needs a new word added to the function that owns that set, and to this list.
- **Tooltips (`.help`) say what pressing does, or what a mark means**: "Move this copy to the
  trash", "Switched off here for Cursor". Never repeat the label.
- **Notes and confirmations say what happens to files**: "Links are deleted; folders move to the
  trash." Name the folder with `~` (`tilde()`), never a full home path.
- **Empty states say what to do next**: "Click a server to see its values and where each agent
  keeps it." Never "No data" or "Nothing here".
- **Errors say what failed and where, and nothing from inside the file**: "line 4, column 12:
  expected a comma" (`src/mcp/formats/json.rs`), "Codex can't express env TOKEN". A line from a
  config can hold a secret; errors go through `safe_error()` and never quote source text.
- **No "please", no exclamation marks, no "successfully"**. A toast title states the result:
  "Installed pdf", "Undid: sync (3 changes)"; a failure names what failed: "Couldn't add
  figma", "Update failed". Its detail line says why.
- **Join two clauses with a semicolon, not an em dash**: "The copies differ; new copies get the
  one you pick."
- **Agent names as each tool writes them**: Claude Code, Claude Desktop, Codex, Cursor, OpenCode,
  Gemini, Windsurf, Kiro; short forms only in tags ("Claude", "Desktop"). The shared folder is
  `.agents`, always with the dot.

```rust
// Correct
Button::new("Remove…").variant(ButtonVariant::Danger)
    .on_press(move |s: &mut App| s.confirm_mcp_remove(&name))

// Incorrect: no ellipsis though it opens a dialog, title case, and a generic verb
Button::new("Delete Server").variant(ButtonVariant::Danger)
```

## Secrets

Env values, header values, tokens in args and in URLs print as `***` in every label, tooltip,
toast, error and log, in the window and the CLI alike. Only Show values (window) and
`--show-secrets` (CLI) reveal them, and only for the item they were pressed on.
