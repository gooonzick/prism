# Prism on the shared design system

Date: 2026-11-10. Status: draft for George's approval.

## Why

George wants his desktop apps (Rust, Rune, rune-kit) to read as products by one author. The
design language was settled and built in Keysmith (`~/Development/ssh-keys-managment`, `main` at
`287ebb5`): a self-contained module `src/ds/`, its rules in `DESIGN.md`, and a lint test
`no_raw_values_in_ui` that keeps screens off raw values. The language is approved; Prism changes
to match it, the system does not change to match Prism.

Prism today has its own rules (`DESIGN.md`, `docs/design/{type-and-color,layout,components,copy}.md`)
and helpers in `src/ui/mod.rs`. Some contradict the system: hierarchy by weight and dividers
instead of grouped cards, a five-level type ladder instead of roles, five dialog widths, small
controls in inspectors, about 260 raw values across `src/ui/`. Where they conflict the system
wins. Prism rules the system has no equivalent for are kept (masked secrets, the voice of
`copy.md`, the closed set of status words).

## Goals

- Every Prism screen and dialog built only from `ds`; `no_raw_values_in_ui` wired in and green.
- `src/ds/` in Prism byte-identical to Keysmith's, except for additions listed in this spec, each
  written so it can be copied back into Keysmith.
- One `DESIGN.md` for Prism, derived from Keysmith's, replacing the old `DESIGN.md` and
  `docs/design/*`.

## Non-goals

- A shared crate. Extracting the system is the next step, not this one.
- New features, or changes to what Prism does. The one allowed behaviour change is moving
  inspector actions into the "…" menu.
- Changing Keysmith. Backporting the additions is a later step.
- A new palette or typeface.

## Anti-goals

- Anything Prism shows today disappearing, including secrets becoming visible by default.
- Screens that pass the lint by hiding raw values behind local constants in `src/ui/`.
- Additions to `ds` that only make sense in Prism (they belong in `src/ui/`).

## Decisions

George decided:

1. Prism moves onto the Keysmith system; on a conflict the system wins.
2. Rules the system lacks are kept: `masked*` secrets hidden until shown, the voice and words of
   `copy.md`.
3. Work starts from an audit, then this spec, then a plan executed by subagents (task, review,
   fixes, a final review of the branch), on a separate branch, a commit per task, nothing pushed
   without his word.
4. He accepted the audit and Claude's proposals as a whole ("делай"), including the ones below.
   Calls marked *(Claude's call)* were made by Claude after George said "do what's best" and are
   open to his correction while reading.

## How it is made

- Branch `design-system` in Prism.
- Copy Keysmith's `src/ds/` (all of `mod.rs`, `tokens.rs`, `text.rs`, `card.rs`, `controls.rs`,
  `status.rs`, `page.rs`, `inspector.rs`, `dialog.rs`, `gallery.rs`, `tests.rs`) and
  `src/ui_lint.rs` from `287ebb5` unchanged first, as their own commit, so every later difference
  shows in `git diff` against Keysmith.
- `mod ds;` and `#[cfg(test)] mod ui_lint;` in `src/main.rs`. A scene `ds` in `SCENES` renders the
  gallery, as in Keysmith (`s.gallery = true`, or whatever Prism's shell needs to show it).
- The lint is wired in from the start but may be `#[ignore]`d until the last screen is migrated;
  it is un-ignored and green before the branch is done. *(Claude's call: lets each task stay
  green while screens move one at a time.)*
- Every addition to `ds` is recorded in a section "Prism additions" of `DESIGN.md` with the file
  it lives in, so a later backport is a copy plus that list. Additions are generic (no Prism type,
  no `crate::` import) and appear in the gallery.

## Additions to `ds`

Each is generic and would serve Keysmith unchanged. Names are proposals; the builder may change
the API, not the behaviour.

| # | Addition | What it is | Why Prism needs it |
| --- | --- | --- | --- |
| 1 | `DialogKind::Wide`, `size::DIALOG_WIDE` 760 | A third dialog width, for a dialog whose picks on the left change a plan on the right. | Sync and MCP Copy/Merge with picks show the effect of a tick next to it. *(Claude's call)* |
| 2 | Cancel-only footer on `DialogFrame` | A footer with Cancel and no main action (`footer` taking an optional primary, or a separate method). | Copy to…: picking a scope is the action; there is nothing to confirm. |
| 3 | `Page::segments(El)` and `ds::segments` | A slot between the filter and the status line; `segments` wraps kit `SegmentedControl` at `Place::List` height. Labels carry counts ("Differs 1"). | Skills (All/Unsynced/Broken/Updates) and MCP (All/Differs/Read-only) filters. |
| 4 | Picking in `ListRow` | A leading checkbox (`ListRow::pick(on, on_toggle)`), a checked fill `color::checked` (kit `layer_01`) with hover, independent of the selected fill. | ⌘/⇧-click and checkbox multi-select on Skills. |
| 5 | Selection line in `Page` | The first line of the status line can lead with a select-all checkbox (aligned with the row checkboxes, indeterminate when partial) and end with actions. With items picked it is the bulk bar: "N selected", a clear button, actions, a `danger` pinned right. | Skills: select all, "Clean up 1 broken link", bulk Sync/Update/Copy to…/Remove…. |
| 6 | `ListRow::trailing(El)` | A fixed-width column at the row's end, after the tags, for a `status` or a Label value, so statuses line up (`size::ROW_TRAILING`, today's 104). | Skill and MCP rows end with a status; skills.sh rows with an install count. |
| 7 | `Page::footer(El)` | A block pinned under the list that does not scroll: a Label and a row of controls. | skills.sh: "Or install straight from a repository". |
| 8 | `ds::loading(k, text)` | A kit `Spinner` and a Label line; never a spinner alone. | Searching, reading a repository, asking a host for a plan, connecting. |
| 9 | `empty` with an action | `ds::empty` with an optional button under the detail (a new function or a builder). | A host that needs prism (Install prism) or can't be reached (Retry). |
| 10 | `ds::markdown(k, md)` | kit `Markdown` styled on roles: body text Body, headings Name (never larger than the pane title), inline and block code Code, list markers muted. | SKILL.md in both inspectors. |
| 11 | `ds::text::command(k, s)` | Code style that wraps between words and never inside one, selectable. | Commands and args: `--stdio` must not split after `--`. |
| 12 | `ds::reveal(shown, place)` | A `quiet` button with an eye icon, "Show values" / "Hide values". | The trailing control of a card whose values are masked. |
| 13 | `ds::secret_input` | A `TextInput` that masks its text until its eye `icon_button` is pressed. | Env and header values in the MCP form. |
| 14 | `ds::switch(k, on)` | kit `Switch` at `Place::Inspector`, using the standard (not compact) switch in the light theme, where the compact one disappears on a light pane when off. | Linking a skill into an agent. |
| 15 | `ds::Plan` | What a write will do, as cards: a card labelled with the heading the caller passes (today's text: "18 changes to 6 skills", "3 changes") holding either grouped rows (`item_row`: subject, its lines as a hint) or plain lines (one row each), whichever the caller has. Sync passes groups (`group_actions` builds them today); MCP and host plans pass their flat lines. No plan type, wire format or CLI JSON changes. *(Claude's call)* Then a card "Skipped (n)"; a card "Heads-up"; a `loading` state while it is computed; a `Notice` (`Tone::Bad`) when it fails; an optional `Notice` (`Tone::Warn`) on top. | Sync, Copy to a host, every MCP write. |
| 16 | Kit controls | `Switch`, `SegmentedControl`, `RadioItem`, `TextArea` and `Spinner` join the list of kit controls `ds` allows (through the wrappers above where there is one). `RadioItem` and `TextArea` are used as the kit draws them, inside card rows, like `Checkbox` in Keysmith. | Prism's forms and inspectors. |
| 17 | `size::INSPECTOR_READ_MAX` 640 | A wider maximum for an inspector that shows a document, so the user can widen it to read. `INSPECTOR_MIN` still applies. | Prism lets the inspector grow to 640 for SKILL.md today; capping it at 400 would take that away. *(Claude's call)* |

Existing `ds` code is not frozen, but it changes only the way Keysmith's `DESIGN.md` allows: a
token tuned in `tokens.rs` when a screen shows it is wrong, or a block extended. Prefer adding a
builder method over changing an existing one's behaviour, so Keysmith's screens stay as they are
after a backport (for example, ⌘/⇧-click reaches a `ListRow` through a new method, `on_press`
keeps its signature). Every such change is listed in "Prism additions" with its reason, like an
addition, so the `diff -r` gate stays honest. *(Claude's call)*

Every new token goes in `tokens.rs`, every block in the gallery. `ds/tests.rs` gains a test per
block that has logic (picking, selection line, plan states, reveal).

Kept out of `ds` because they are Prism's own: the masking functions (`masked`, `masked_args`,
`masked_url`, in `crate::mcp::model`), status words and their tones, agent names.

## The shell

As Keysmith's `src/ui/mod.rs`: `SplitView` with `frame_inset(space::S2)`,
`sidebar_padding(space::S2, space::S2)`, the sidebar toggle at `Place::Bar`, `motion::panes()`,
reduced motion passed through. The inspector is drawn through an `inspector_pane` like
Keysmith's: narrower than the stored width down to `INSPECTOR_MIN` when the list would drop
below `LIST_MIN`, hidden below that, the stored width never overwritten. Its range becomes
`INSPECTOR_MIN..=INSPECTOR_READ_MAX` (from 300..=640); it still starts collapsed and opens on a
selection as today.

The window's minimum becomes 860 wide (from 820) and the sidebar's default width 232 (from 248),
both Keysmith's values, so the smallest window shows all three panes: sidebar 232, list 276,
inspector 352. Today's minimum with the new `INSPECTOR_MIN` would hide the inspector, which the
anti-goals forbid. A sidebar the user widens hides the inspector first, as in Keysmith.
*(Claude's call)*

- **Sidebar**: kit `Sidebar`, kit typography (a recorded exception). Rows `ROW_1` (Global,
  skills.sh, Add project…) and `ROW_2` (projects, with their "~/Development · 2 skills" line;
  40 today). Header gap `S1`, row gap `ROW_GAP`. The environment switcher `Select` stays its first
  header.
- **Toolbar**: leading holds the page tabs (Skills / MCP servers) followed by `toolbar_title`
  (truncates). The kit's centred title is not used. Pages stay in the toolbar rather than the
  sidebar, because Prism's sidebar holds scopes. *(Claude's call)* Trailing: `icon_button`s at
  `Place::Bar`, then at most one text button at `Place::Bar`. Page tabs are built from tokens (or
  become `ds::segments` if it can carry the keys `tab-skills`/`tab-mcp` and the radio roles).
- **Toast**: kit `Toast` with Undo, as today (a recorded exception).

## Each screen

Keys stay as they are wherever the element survives; a test whose key changes is updated in
the same task.

### Skills (`installed.rs`)

`Page`: filter "Filter skills"; segments All/Unsynced/Broken/Updates with counts; status line
with the select-all checkbox, "7 skills in ~ · ⌘- or ⇧-click to pick several" and, at its end,
"Clean up 1 broken link"; with skills picked, the bulk bar: "3 selected", clear, Sync
(`primary`), Update (when any picked skill has one), Copy to…, and Remove… (`danger`) on the
right. Rows: `ListRow` with the pick checkbox, name, description as meta, agent tags (`tag` with
the placement tone and its help), the sync status in the trailing column.

Toolbar: tabs, "Global skills", New skill (⌘N), Check skills.sh for updates, Rescan (⌘R), then
"Sync 7 skills": `primary` while something is unsynced and nothing is picked, otherwise `button`
(the bulk bar owns the next step).

### Skill inspector (`inspector.rs`)

`Inspector`: title (wraps), description; action row Open SKILL.md and Copy to… visible, "…" with
Reveal, Remove… (`danger`) right. The builder checks the row fits at `INSPECTOR_MIN` and moves
more into "…" if not; menu order is fixed and an item that doesn't apply (Reveal on a host) is
disabled. Notice under the actions: "A newer version is on skills.sh" (`Tone::Info`) with Update
(`button`, not `primary`: the toolbar's Sync is the view's primary). Cards:

- "Folder": the path (`path`), selectable. *(Claude's call: a one-row card rather than the meta
  line, because the path is long and the meta line truncates.)*
- "Agents": `item_row` per place: name (with the placement dot for an agent's own copy), the
  path and state as hint ("~/.claude/skills · its own copy"), trailing `switch`, or Move into
  .agents / Use link with the remove `icon_button`. "Show 6 more agents" / "Show fewer agents" is a `quiet` button in
  the card's last row (`plain_row`).
- "SKILL.md": `markdown`.

### MCP servers (`mcp.rs`)

`Page`: segments All/Differs/Read-only (Disabled before Read-only in a project, as today); status line with "5 servers for ~" and every notice
(read errors, "Maybe one server: figma, figma-dev (same package figma-developer-mcp)" with
Merge…) as `Notice`s; the Merge notice moves from under the list into the status line. Rows:
name, the masked command or URL as meta, agent tags, status in the trailing column; a global
server seen from a project is `dim` (0.75, from 0.6). Toolbar: tabs, "Global MCP servers",
Rescan, "Add server" (`primary`).

### MCP server inspector (`mcp.rs`)

`Inspector`: title, `status` (differs / in sync / read-only), description ("Copies differ in
headers.CONTEXT7_API_KEY"). Action row: Copy to… and Edit… visible; "…" with Disable here /
Enable here (one item whose label follows the state, disabled in Global); Remove… right.
Read-only servers have no action row, inherited ones no Copy to… and no Remove…, as today.
Cards:

- "Configuration", trailing `reveal`: a hint row "As Claude Code has it" when the copies differ;
  `pair_row` Transport; `stacked_row` Command (`command`) or URL (`code`).
- "Environment" / "Headers": a `value_row` per key (`code` label, masked value), present only
  when there are any. Show values reveals these and the differences of this server only.
- "Agents": `item_row` per copy: agent name, file path as hint, place ("global", "local") as a
  trailing Label.
- "Differences": one `stacked_row` per field, its label the field key in `code`
  (`headers.CONTEXT7_API_KEY`), its value a column of `pair_row`s agent → value (masked), with
  "missing" as a Hint.

### skills.sh (`browse.rs`)

`Page`: filter "Search skills.sh"; empty "Find skills on skills.sh" with its detail; `loading`
while searching; rows with name, source as meta, `tag` "installed" (replaces the `Badge`), the
install count in the trailing column. Footer: Label "Or install straight from a repository",
the input, Show skills, Install all. Toolbar: tabs are absent on this page as today, title
"skills.sh", "Install into" Label and the scope `Select` (its menu in the layer).

### skills.sh result inspector

Title; action row Install / Reinstall (`primary`: the page has no other) and Open on skills.sh;
hint "Installs into ~/.agents/skills and links it into your sync targets."; card "Details" with
Source (`code`) and Popularity; card "SKILL.md" (`markdown`, `loading` while it loads).

### Repository inspector (scene `repo`)

Title (the repository); action row Install 2 skills (`primary`) and Close; `loading` "Reading
the repository…"; a card of kit `Checkbox` rows with label (name, "(installed)") and
description.

### A host that isn't ready (`hosts::pending`)

In place of the page, with tabs and the host's name in the toolbar and an empty inspector:
connecting is `loading` ("Connecting to devbox…", "Installing prism 0.1.0 on devbox…"); needs
install is `empty` with Install prism (`primary`, disabled when unsupported); unreachable is
`empty` with Retry (`button`). Copy unchanged.

### Dialogs

All on `DialogFrame`, top at `DIALOG_TOP`, the explanatory sentence first in the body as Body
text, fields and choices in cards, errors in the frame's error line, footer Cancel plus one main
action (`primary`, or `danger` when it deletes). Selects in a dialog go to `DialogFrame::menus`.

| Dialog | Kind | Body |
| --- | --- | --- |
| Sync | Wide | Left: card "Link into" of kit `Checkbox` rows (agent name, path; "(not found)" agents disabled as today). Right: `Plan`. Footer: Cancel, "Apply 18 changes". |
| Remove (one, several) | Confirm | The sentence; footer Cancel, Remove / "Remove 3" (`danger`). |
| Copy to… | Confirm | The sentence; a card per machine labelled with its name, rows `pressable` `item_row`s (folder icon, scope, "2 skills there" / clashes as hint); `loading` per machine while asking; an unreachable machine is a `Notice` (`Tone::Bad`). Cancel-only footer. |
| Copy to a host (plan) | Form | `Plan`; footer Cancel, the apply button. |
| New skill | Form | The sentence; card with `field` Name and `field` "When to use it" (`TextArea`). Footer Cancel, Create skill. |
| Projects found | Form | The sentence; card of `Checkbox` rows (name, "~/Development · 2 skills"). Footer Cancel, "Add 2 projects". |
| Add host | Form | The sentence; card with `field` Name and `field` ssh target. Footer Cancel, Add host. |
| Add a project on a host | Form | The sentence; card with `field` Folder. Footer Cancel, Add project. |
| Add / Edit MCP server | Form | Card "Server": `field_pair` Name + the Command/URL `segments` (add only); card with `field` "Command and arguments" or URL (masked when prefilled with a likely secret, with its eye); card "Environment" / "Headers": key + `secret_input` + remove per row, `quiet` "Add variable", the `${VAR}` hint; card "Agents" (add only): `Checkbox` rows in two columns, name and file; card "Claude Code scope" (add, in a project): `RadioItem`s with descriptions. Edit shows its sentence first. Footer Cancel, Add server / Review changes. |
| MCP Copy / Merge with picks | Wide | Left: cards "Copy from" (`RadioItem`s, the "copies differ" hint), "Copy to" (`Checkbox`es), the machine and scope `Select`s across machines, "One name for" with `field` Name (Merge). Right: `Plan`. |
| MCP Remove / Disable / Enable, Copy without picks | Form | Picks (card "Remove from") if any, then `Plan`. |

## What replaces the old documents

- `DESIGN.md` is rewritten from Keysmith's: the one rule and the lint, type and colour roles,
  spacing/radii/sizes, layout rules, motion, the block table, the relation to rune-kit, the
  recorded exceptions, how to add a screen. Prism-specific parts are added:
  - **Secrets**: the masking rule from today's `DESIGN.md` and `copy.md` (values pass through
    `masked`, `masked_args`, `masked_url`; Show values reveals one server; `--show-secrets` in
    the CLI).
  - **Words**: `copy.md`'s rules carried over in full (sentence case, "…" on buttons that need
    another step, buttons name action and count, the closed set of status words and where each
    set lives, tooltips, notes that say what happens to files, empty states, errors through
    `safe_error()`, toast titles, semicolons, agent names, `.agents`).
  - **Status tones**: today's dot-colour decision tree mapped onto `Tone`
    (`placement_color()` and `status()` become functions returning a `Tone`).
  - **Prism additions**: the list of additions to `ds` above, each with its file.
  - **Scenes**: the scene list including `ds`, and `scripts/demo-home.sh` for the README
    screenshots.
- `docs/design/` is deleted; nothing in it survives that isn't in the new `DESIGN.md`.
- `AGENTS.md` points to `DESIGN.md` and states the check commands (tests, clippy, the lint,
  snapshots in both themes).
- The README screenshots are regenerated with `scripts/demo-home.sh` at the end.

## Constraints and lessons from Keysmith

- After every task `cargo test` and `cargo clippy --all-targets -- -D warnings` pass.
- Every UI change is checked with snapshots in light and dark
  (`target/debug/prism --snap … --scene … --theme light|dark`, from the demo home).
- rune-kit menu items are keyed `<key>-popup-item<i>`: menu order is fixed, inapplicable items
  are disabled, not hidden.
- The "…" open flag counts in `modal()` only while the menu is drawn, and is cleared when the
  selection changes or disappears.
- `Select`s in a dialog go in `DialogFrame::menus`.
- A scrolling list of fields keeps `size::FOCUS_RING` of room.
- An inspector's action row never wraps; check it at `INSPECTOR_MIN`.
- Titles wrap with `wrap_anywhere`; long values go to `stacked_row` through `value_row`.
- Window width comes from `cx.layout_with("kit-root", …)`.
- No 32-point controls; toolbar and dialog footer buttons are 28 (`Place::Bar`).
- Tests press elements by key; change a key and its test together.

## Left to the builder

- The API of each addition, and how it maps onto rune-kit.
- The order of migration, as long as each task leaves tests and clippy green.
- Exact choices within the scale (4, 8 or 12 between two elements), consistent per block.
- Which inspector actions go into "…" beyond those named, if a row does not fit at
  `INSPECTOR_MIN`.
- Splitting `src/ui/` files differently if a file grows past reading.

## Done when

- Every screen and dialog is built only from `ds`; `no_raw_values_in_ui` runs (not ignored) and
  passes.
- `diff -r` of `src/ds/` against Keysmith shows only the additions listed in `DESIGN.md`.
- `DESIGN.md` matches the code; `docs/design/` is gone.
- The `ds` scene renders every block, the additions included, in light and dark.
- `cargo test` and `cargo clippy --all-targets -- -D warnings` pass.
- Snapshots of every scene in both themes show one content edge per pane, cards in inspectors and
  dialogs, three dialog widths, one row anatomy, and nothing missing compared with the snapshots
  taken before the work.
