# Layout

The window is a `SplitView`: sidebar (scopes and projects), main pane (toolbar, filters, list),
inspector (the selected item). Every page fills these three; a new page never adds a fourth
column or a second toolbar.

## Environment switcher

The sidebar's first header is the machine in view: a `Select` (`hosts::switcher`,
`src/ui/hosts.rs`) listing `this_machine()` ("This Mac", "This computer" on Linux), each saved
host by name, then "Add host…", which opens the Add host dialog instead of switching. Picking a
host makes it the environment: the scopes, projects, pages and every action are that
machine's, and `s.env_label()` names it wherever a machine is named: plans ("Asking devbox
for the plan…"), the Copy to… dialogs and toasts. Nothing else marks a remote environment;
no banner, no tint. On skills.sh a caption says installs land on this machine (`browse_note`).

Until a host's state is in, `hosts::pending` replaces the main pane of the Skills and MCP
pages (the toolbar keeps its tabs and the host's name; the inspector is empty):

- Connecting: `Spinner` plus "Connecting to devbox…" (`small`, `text_muted`), or "Installing
  prism 0.1.0 on devbox…" while an install runs.
- Needs install: `empty("prism 0.1.0 isn't on devbox", detail)`, the detail saying what
  Install does ("Install puts it in ~/.local/bin on devbox; nothing else there changes.") or
  why it can't (no release for that system: use `prism hosts install --binary`), then
  "Install prism" (`Contrast`, disabled when unsupported).
- Unreachable: `empty("Can't reach devbox", why)`, the why being ssh's last line without the
  "Can't reach devbox: " prefix, then "Retry" (`Ghost`).

Scenes: `env-remote` (a host in view) and `env-unreachable`.

## Toolbar

`Toolbar::new().leading(page_tabs(…)).title(scope_title(…)).trailing(row().gap(6.0)…)`.
Trailing holds `IconButton`s (GhostMuted) and at most one text `Button`, the page's main action
(`Ghost`, or `Contrast` when it is the next step). Every toolbar action has a `cmd+` shortcut.

## List rows

Copy `server_row` in `src/ui/mcp.rs` or `result_row` in `src/ui/browse.rs`:

- `row().px(12.0).py(8.0).gap(12.0).center_y().radius(k.radius.md)`, `role("button")`,
  `.label(name)`, `.focusable(true)`, a `key` with the item's name.
- Left: name (`ui_medium`, `text_base`, `.truncate()`) over one line of detail (`small`,
  `text_muted`, `.truncate()`). Then tags, then the status (`dot` + word, right-aligned, fixed
  width so statuses line up).
- Selected: `fill(layer_02)`; otherwise `hover(k.c.hover)`. A global server seen from a project
  is `.dim(0.6)`.

A truncated line must be readable in full elsewhere: the inspector shows it, or `.help()` does.

## Inspector

Order, top to bottom:

1. Head: `inspector_title` and, under it with `gap(4)`, one caption line in `text_muted`
   (a description, or what the status means: "Copies differ in command and args").
2. Actions row (see Button in `components.md`), or a banner when one applies.
3. `Divider`, then a section: `section("Configuration", trailing, k)` and its `prop`s.
4. `Divider`, the next section, and so on. Every section after the head starts with a divider
   and a `section()` title; no section without a title.

The sections, in order, and where a new one goes:

- Skill (`src/ui/inspector.rs`): head, Folder, actions | Agents | SKILL.md.
- skills.sh result (`src/ui/browse.rs`): head, Source, Popularity, Install | SKILL.md.
- MCP server (`src/ui/mcp.rs`): head, actions | Configuration | Agents | Differences.

What the item *is* comes first, where it *lives* next, what is *wrong* or long (Differences,
SKILL.md) last. A new section goes in that order, never between the head and the actions. A
problem the inspector already shows in a section is not repeated in a second one: extend the
section that owns it.

```rust
// Correct
body = body.child(Divider::horizontal().build(k)).child(section("SKILL.md", None, k)).child(skill_md(md, k));

// Incorrect: a 11pt faint "heading" with no divider; it reads as one more label
body = body.child(heading("SKILL.md", k)).child(Markdown::new(md).build(k));
```

- Spacing: `inspector_body()` (padding 16, gap 16 between blocks); `gap(12)` between props in
  a section, `gap(10)` between agent rows, `gap(4)` between a label and its value.
- A control that changes the whole section (Show values) goes in `section()`'s `trailing`, as a
  small `Ghost` button.
- A list of places (Agents): name in `k.ty.ui` with its dot, the path under it in `caption()`
  `text_faint`, the place ("global", "local") right-aligned in `small` `text_muted`, controls on
  the right.

## Dialogs

`Dialog::new("<name>-dialog", title).width(…).fit().open(true).on_close(…)` with
`dialog_body()` (`src/ui/dialogs.rs`) inside. Widths: 460 for a confirmation or a scope pick, 480–620 for a form,
760 for a dialog that lists changes.

- Top: a note explaining what will happen (`k.ty.ui`, `text_muted`, in `dialogs.rs`; inline
  notes under a heading in `mcp_dialogs.rs` are `small`).
- Bottom: a right-aligned row, `Cancel` (`Ghost`) first, then the confirm (`Contrast`, or
  `Danger` when it deletes). The confirm names the action and the count: "Remove 3", "Add server", never "OK" or "Confirm".
- Any write shows its plan first: the dialog lists each change, skipped items under
  `heading("Skipped (n)")`, warnings under `heading("Heads-up")`.

## Empty, loading and notices

- Nothing selected or nothing found: `empty(title, detail, k)`, where the detail says what to do
  ("Click a server to see its values and where each agent keeps it.").
- Loading: `Spinner` plus a muted line, in place of the content that is loading.
- After an action: one `Toast` from `App::notice` (`Notice { title, detail, error, undo, machine }`),
  closable, offering Undo when the change can be undone. Never a dialog to report success.
  A change on another machine ends its title with "on <machine>" and sets `machine`: the
  toast's Undo acts there (`App::undo_toast`), while ⌘Z acts on the machine in view.
