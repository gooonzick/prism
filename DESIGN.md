# Prism design rules

Prism's screens are built from one design system, `src/ds/`, the same one Keysmith uses
(`~/Development/ssh-keys-managment`, copied from `287ebb5`). This file states its rules for
people and agents. The code is the source of truth for every number: when this file and
`src/ds/tokens.rs` disagree, the code wins and this file is wrong.

`src/ds/` depends only on `rune` and `rune-kit`, never on `crate::` (the app, the scanner, the
MCP model, `crate::ui`), so the folder can go back into Keysmith or into the next Rune app. It
is Keysmith's copy plus the items listed under "Prism additions" at the end of this file;
`/usr/bin/diff -r ~/Development/ssh-keys-managment/src/ds src/ds` shows nothing else.

## The one rule

**A screen never picks a raw value.** Code in `src/ui/` uses `ds` blocks and the `space::`,
`radius::` and `size::` constants. It contains no `TextStyle::new`, no `k.ty.*`, `k.c.*` or
`k.radius.*`, and no number passed to `gap`, `padding`, `px`/`py`/`pt`/`pb`/`pl`/`pr`,
`mt`/`mb`/`ml`/`mr`/`mx`/`my` or `radius`. The test `ui_lint::no_raw_values_in_ui` fails on any
of these and names the file and line.

Widths of individual fields (`.width(220.0)` on a select) are allowed. A width used in more than
one place, or one a layout decision depends on, goes into `size::`. `src/ui/` keeps no numeric
size constants of its own: the window's minimum (`MIN_WINDOW`) is in `src/main.rs`, everything
else in `size::`.

If a screen needs a value `ds` does not have, add it to `src/ds/tokens.rs` (or a block to `ds`),
never at the call site, and record it under "Prism additions". A number may be tuned when a
screen shows it is wrong; it is tuned in the token module, and this file is updated with it.

## Type roles

One ladder. A screen picks a role, never a size. Functions in `ds::text` (styles in `ds::ty`).

| Role | `ds::text::` | Style | Colour | Used for |
| --- | --- | --- | --- | --- |
| Title | `title` | 15 semibold, line 20, wraps (inside a word too long for a line) | base | The object's name in the inspector, dialog titles |
| Value | `value` | 13 regular, line 20, truncates at the end | base | Values |
| Name | `name` | Value in medium (500), truncates | base | Names in lists and item rows |
| Body | `body` | Value, wraps | base | Sentences: descriptions, confirmation text, notices |
| Code | `code` | 12 mono, line 18, breaks anywhere, selectable | base | URLs, env and header keys, config field names (`headers.X-Key`), skills.sh sources |
| Command | `command` | Code, wraps between words and never inside one that fits, each word selectable | base | Commands and args: `--stdio` never splits after `--` |
| Path | `path` | Code, one line, cut in the middle, selectable | base | File paths |
| Label | `label` | 11 regular, line 16, truncates | muted | Field labels, card labels, list meta lines, counts, status words |
| Hint | `hint` | 11 regular, line 16, wraps | faint | Help under a field, an agent's path and state, empty-state detail |

A label is always quieter than its value. There are no bold section headings and no dividers:
card labels and cards carry the structure. rune-kit's 14pt and its older 12/13 styles are not
used (see the exceptions for where kit typography remains).

Everything a person would type is mono (Code, Command, Path); everything Prism says is Inter.
**One sentence is one text.** Never assemble a sentence from several texts to set one word in
mono or bold: the pieces wrap separately. Put the key on its own line in Code and the sentence
under it, as each Differences row does, or write the key in the sentence's own face.

SKILL.md goes through `ds::markdown`: body text Body, headings Name (a file's `# Title` never
outranks the pane's title), code Code, list markers muted.

## Colour roles

`ds::color`. Text is `text` (base), `text_muted` or `text_faint`, nothing else.

Status colours `ok`, `warn`, `bad`, `info` go on dots and icons only, never on text or fills.
The one exception is a destructive button's label (`ds::danger`). Status is always a dot (or an
icon) plus a word; colour is never the only signal. A screen names a status by its `Tone`, never
by a colour (see "Status tones").

Surfaces: `pane` (a page or inspector), `card` (one step above the pane, with a `hairline`
border), `selected` (the selected row), `checked` (a picked row that is not the selected one;
selected wins), `hover`. Each maps to one rune-kit palette entry, so both themes come from the
kit. Never a coloured fill behind text (a red banner, a green row).

## Spacing, radii, sizes

- Everything is a multiple of 4. `space::S1` 4, `S2` 8, `S3` 12, `S4` 16, `S6` 24. The one
  exception is `space::ROW_GAP` (2) between list rows and sidebar rows.
- Radii nest: `radius::TAG` 4, `CONTROL` 6 (controls and list rows), `CARD` 8, `DIALOG` 12.
- Control heights by place, `ds::Place`: `Inspector` 24 (kit `ControlSize::Regular`); `List` 28
  (lists, filters, segments, status-line buttons) and `Bar` 28 (toolbar, dialog footers), both
  kit `ControlSize::Large`. Every button takes a `Place`; none picks a `ControlSize`.
  `Place::height()` gives the height for tests and layout.
- `size::`: `ROW_1` 32 and `ROW_2` 44 (list rows with one and two lines; sidebar rows too),
  `DOT` 7, `ICON` 16, `PANE_EDGE` 16, `PAIR_LABEL` 120 (a pair row's label column, wider for a
  longer label), `ROW_NAME_MIN` 120 (a list row's name, which its tags give way to),
  `ROW_TRAILING` 104 (a list row's status column), `FIELD_SHORT` 96 (a short field beside a
  wide one), `FIELD_KEY` 170 (the name of a name = value row in a form), `FIELD_MIN` 200 (the
  narrowest a field gets with its buttons beside it), `FOCUS_RING` 3 (room left around fields
  for their focus ring), `DIALOG_CONFIRM` 440, `DIALOG_FORM` 560, `DIALOG_WIDE` 760,
  `DIALOG_PICKS` 250 (the picks column of a Wide dialog), `DIALOG_TOP` 96, `INSPECTOR_MIN` 352,
  `INSPECTOR_MAX` 400, `INSPECTOR_READ_MAX` 640, `LIST_MIN` 276.

## Layout

1. **One content edge per pane**, `size::PANE_EDGE` (16) from the pane's border. Filter,
   segments, status line, row text, footer and cards sit on it. A row's hover and selection
   background reaches `S2` past it; its text does not.
2. **One page skeleton** (`ds::Page`): toolbar → filter → segments → status line → list →
   footer. The count (or the selection line) and every notice (read errors, "Maybe one
   server: …" with Merge…) live in the status line.
3. **One inspector skeleton** (`ds::Inspector`): head → action row → notice → cards. Everything
   below the action row is in a card.
4. **One list row anatomy** (`ds::ListRow`) on every page: pick checkbox, name over meta line,
   tags, the status in the trailing column.
5. **Three dialog widths**: 440 for confirmations (`DialogKind::Confirm`: Remove, Copy to…),
   560 for forms (`DialogKind::Form`: New skill, Add host, Add / Edit MCP server, MCP plans),
   760 for picks on the left that change a plan on the right (`DialogKind::Wide`: Sync, MCP
   Copy / Merge with picks). Every dialog's top sits at `DIALOG_TOP`, so dialogs of different
   heights open in the same place. The explanatory sentence comes first as Body text; fields
   and choices sit in cards; errors go in the frame's error line; `Select`s go in
   `DialogFrame::menus`. Any write shows its plan first (`ds::Plan`).
6. **One primary (contrast) button per view** (`ds::primary`): the toolbar's Sync while
   something is unsynced and nothing is picked (the bulk bar's Sync once rows are picked), Add
   server on MCP, Install / Reinstall in a skills.sh result, the main action in a dialog. A
   primary that is not the next step right now drops to `button`.
7. **Long text**: names truncate at the end, paths in the middle, values in a stacked row wrap,
   titles wrap and break inside a word that does not fit. A pair row's label always shows in
   full: its column is `PAIR_LABEL` or as wide as the label, and the value takes the rest and
   truncates (its tooltip is the whole value). `value_row` keeps a row on one line only when it
   fits the narrowest inspector: label (counted as at least 19 characters, the column) plus
   value at most 44 characters; otherwise the value moves under the label (a stacked row). In a
   list row the trailing status keeps `ROW_TRAILING`; the tags give way first, then the name,
   which truncates below `ROW_NAME_MIN` only when the row cannot fit pick + `ROW_NAME_MIN` +
   trailing. A truncated line is readable in full elsewhere: the inspector shows it, or its
   tooltip does.
8. **Pane widths**: the sidebar is the kit's resizable pane (232 by default); the inspector is
   `INSPECTOR_MIN`..=`INSPECTOR_READ_MAX` (352–640, 380 by default), so SKILL.md can be widened
   to read. It starts collapsed and opens on a selection. The list takes the rest and keeps at
   least `LIST_MIN` (276). When space is tight the inspector is drawn narrower than the width
   the user chose, down to `INSPECTOR_MIN`; the stored width is never changed, so it returns to
   it when the window widens. Below sidebar + `LIST_MIN` + `INSPECTOR_MIN` (276 + 352) the
   inspector hides, and it comes back when there is room again (`ui::inspector_pane`, window
   width from `cx.layout_with("kit-root", …)`). The 860 minimum window with the default sidebar
   shows all three panes: sidebar 232, list 276, inspector 352. A wider sidebar hides the
   inspector first. In a narrow list the toolbar title is at the leading edge and truncates
   (`ds::toolbar_title`).
9. **An inspector's action row never wraps.** It holds at most three buttons plus the overflow
   on the left and one action (usually the destructive one) on the right; the rest goes in
   "…" (`ds::overflow`). The builder measures the row (`ds::button_width`) against the width
   the inspector is drawn at, so check it at `INSPECTOR_MIN`. Menu order is fixed; an item that
   does not apply is disabled, not hidden.
10. **No empty frames**: a card with no rows is not drawn (`Card::build` returns `None`); an
    inspector with nothing selected shows an empty state (`Inspector::empty`) or nothing.

### The shell

`ui::view` builds the window as Keysmith's: `SplitView` with `frame_inset(space::S2)`,
`sidebar_padding(space::S2, space::S2)`, the sidebar toggle at `Place::Bar`, `motion::panes()`,
reduced motion passed through.

- **Sidebar**: kit `Sidebar`. Its first header is the machine in view, the environment switcher
  `hosts::switcher` (`src/ui/hosts.rs`): a `Select` listing `this_machine()` ("This Mac", "This
  computer" on Linux), each saved host, then "Add host…", which opens the Add host dialog
  instead of switching. Then Global and skills.sh (`ROW_1`), the projects (`ROW_2`, with their
  "~/Development · 2 skills" line) and Add project….
- **Toolbar**: leading holds the page tabs (`page_tabs`, a `ds::tabs` keyed `tab-skills` /
  `tab-mcp`) and the page's title (`scope_title`, through `ds::toolbar_title`). Pages live in
  the toolbar, not the sidebar, because the sidebar holds scopes. Trailing: `icon_button`s at
  `Place::Bar`, then at most one text button at `Place::Bar`. New skill and Add server take ⌘N,
  Rescan ⌘R (`ui::on_key`; hints spell them with `key_label`). When the toolbar does not fit
  the list pane (`toolbar_fits`), the title hides, the Skills toolbar's icon actions move into a
  toolbar "…", and the text button shows its icon alone, its label as tooltip and accessible
  name.
- **A host in view**: `s.env_label()` names the machine wherever a machine is named (plans,
  Copy to…, toasts). Nothing else marks a remote environment: no banner, no tint. On skills.sh a
  line says installs land on this machine (`browse_note`). Until a host's state is in,
  `hosts::pending` replaces the Skills and MCP pages: `loading` while connecting or installing
  prism, `empty_action` with Install prism (`primary`, disabled when unsupported) or Retry
  (`button`); the toolbar keeps its tabs and the host's name, and the inspector is empty.
- **Toast**: kit `Toast` from `App::notice` (`Notice { title, detail, error, undo, machine, … }`),
  closable, with Undo when the change can be undone. Never a dialog to report success. A change
  on another machine ends its title with "on <machine>"; the toast's Undo acts there
  (`App::undo_toast`), while ⌘Z acts on the machine in view.

## Motion

`ds::motion`. Durations and curves are named tokens like every other value.

- `enter()` ~180 ms and `exit()` ~120 ms, no bounce: dialogs and menus arrive slower than they
  leave.
- `swap()` ~150 ms: the inspector's content cross-fades when the selection changes (opacity
  only, `Inspector::subject`).
- `press_scale(reduced)`: list rows and pressable card rows shrink to 0.97 under the pointer.
- `panes()`: the sidebar and inspector open and close on one spring, 150–300 ms by width.
- `DIM` (0.75): the opacity of a de-emphasised row: a global server seen from a project.
- `COPY_CONFIRM_SECS` (1.2 s): how long `copy_button` shows its check. Prism has no copy button
  yet; the token stays for Keysmith.
- Keyboard actions done many times a day should be instant, as Keysmith makes ⌘N, Enter and
  the arrow keys. Prism's `ui::on_key` does not do this yet.
- Under reduce motion only opacity changes remain: pass `cx.is_reduced_motion()` into
  `ListRow::reduced`, `pressable` and `SplitView::reduced_motion`.

## Blocks

| Block | Use it for |
| --- | --- |
| `Page` | A list page: filter, segments, count or selection line, notices, then rows or an empty state, then a footer; the rows scroll. |
| `Page::segments` + `segments` | Kit segments that narrow the list, fitted to their labels at `Place::List`; labels carry counts ("Differs 1"), a 0 stays visible. |
| `Page::selection` + `Selection` | The status line's first line on a pickable list: select-all (`check-all`, a dash when partial), the summary, actions; with rows picked, the bulk bar ("3 selected" ×, actions, `danger` pinned right). |
| `Page::footer` | A block under the list that does not scroll: skills.sh "Or install straight from a repository". |
| `ListRow` | A row in a `Page` list: pick checkbox (`pick`, `pick_key`), icon, name, meta line, tags, trailing status (`trailing`); 32 or 44 high. `build_tx` when the press needs ⌘/⇧. |
| `empty` / `empty_action` | What is missing and what to do, in place of a list or an inspector; `empty_action` adds a button (Install prism, Retry). |
| `loading` | A kit `Spinner` and a Label line that wraps; never a spinner alone. |
| `Inspector` | The right pane: title, meta, status, description, action row, notice, cards; `Inspector::empty` when nothing is selected. |
| `Card` | A labelled group of rows on a bordered surface, hairlines between rows, an optional trailing control by the label (`reveal`). |
| `pair_row` / `pair_row_with` | Label left (in full), short value right, one line. |
| `stacked_row` / `stacked_row_with` | Label above a long value that wraps: commands, URLs. |
| `value_row` | A pair row that becomes a stacked row when label and value don't fit one line: env and header values, Differences. |
| `item_row` | A name with a hint under it and a trailing control or label: agents of a skill, copies of a server, scopes in Copy to…. |
| `pressable` | Makes a card row a button (hover, press scale). |
| `field` | A form field: label above the input. |
| `field_pair` | Two fields on one line (Name + Command / URL). |
| `plain_row` | A full-width card row for buttons or compound fields ("Show 6 more agents", "Add variable"). |
| `button` | The ordinary (outlined) button. |
| `primary` | The one contrast button of a view. |
| `danger` | A destructive action; the only coloured label. Pinned right in an action row. |
| `quiet` | A secondary action with no outline ("Show 6 more agents", "Add variable"). |
| `reveal` | A `quiet` eye button, "Show values" / "Hide values": the trailing control of a card whose values are masked. |
| `copy_button` | A button that copies and confirms with a check (not used in Prism yet). |
| `icon_button` | An icon-only button; its label is the accessible name and tooltip. |
| `button_width`, `toolbar_title_width` | What a text button and an untruncated toolbar title take, measured before layout, for a row that folds into "…". |
| `filter` | The search field at the top of a page, with a clear button once there is text. |
| `tabs` | Keyed segments that switch what the window shows (page tabs) or a choice inside a form (Command / URL). |
| `toolbar_title` | The toolbar's title at the leading edge; it truncates before the buttons give way. |
| `switch` | A kit `Switch` that applies at once: linking a skill into an agent. |
| `secret_input` + `SecretInput` | A masked text field with its eye: env and header values, a command or URL holding a secret. |
| `overflow` + `MenuAction` | The "…" button and its menu for actions beyond the visible three. |
| `dot`, `status`, `tag` | A 7pt status dot; dot plus a word; a small outlined label with an optional dot and a tooltip. |
| `Notice` + `Tone` | A dot, a wrapping message, an optional detail and button; in the status line, under an action row, in a card or a dialog. |
| `markdown` | A document (SKILL.md) on the type roles; links open in the browser. |
| `Plan` | What a write will do, as dialog body cards: the changes (grouped or plain lines), "Skipped (n)", "Heads-up", a warn notice on top, a `loading` line while computed, a bad notice when it fails. |
| `DialogFrame` + `DialogKind` | Every dialog: title and close button, a body of cards that scrolls, an error line (`error_key` for a second key), a footer with Cancel and one main action, or Cancel alone (`footer_cancel`). |
| `gallery` | Every block on one screen: `cargo run -- --snap out.png --scene ds --size 1500x760`. |

Kit controls used as the kit draws them, inside card rows: `Checkbox`, `RadioItem` (MCP "Copy
from", Claude Code scope), `TextArea` (New skill's "When to use it"), `TextInput`, `Select`.

## Status tones

Every status is a `Tone` (`ds::Tone`), shown as a dot (`dot`), a dot plus a word (`status`) or a
tag with a dot (`tag`). The tone answers "what does the dot say?":

```
What does the dot say?
 ├── Linked, in sync, done                     → Tone::Ok
 ├── An agent's own copy, or news (an update)  → Tone::Info
 ├── Needs a look: differs, half linked, not in
 │   .agents, links elsewhere, switched off    → Tone::Warn
 ├── Broken link, failure                      → Tone::Bad
 └── Neither good nor bad (read-only, to link) → Tone::Neutral
```

The mappings live in three functions; extend them, never pick a tone inline for a state they
own:

- `installed::placement_tone` (`src/ui/installed.rs`): an agent's copy of a skill, for its tag
  and its dot in the inspector. Linked → Ok, its own copy → Info, a link elsewhere → Warn,
  broken → Bad, none → no dot.
- `installed::sync_status`: a skill row's word, tone and tooltip. "synced" Ok, "N to link"
  Neutral, "not in .agents" Warn, "broken links" and "broken in .agents" Bad.
- `mcp::status` (`src/ui/mcp.rs`): a server's word, tone and tooltip. "in sync" Ok, "differs"
  Warn, "read-only" Neutral.

Tags that only say where something lives ("global", "also global", "installed") have no dot.

## Secrets

Secrets never render unmasked by default. Env values, header values, tokens in args and in URLs
print as `***` in every label, tooltip, toast, error and log, in the window and the CLI alike:
values go through `masked()`, `masked_args()` or `masked_url()` (`src/mcp/model.rs`) before
they reach a screen. Only Show values (`reveal`, window) and `--show-secrets` (CLI) reveal them,
and only for the server they were pressed on. In a form a value is a `secret_input`, masked
until its eye is pressed.

## Words

Prism talks like a careful colleague: short, plain, specific about files. It names what will
happen to which file before it happens, and never says more than one sentence where a phrase
will do.

- **Sentence case everywhere**: "Add server", "Show values". "Install Command Line Tool…" is the
  one exception (a macOS menu item, which uses title case).
- **`…` (one character, not three dots) on a button that opens a dialog or needs another step**:
  "Copy to…", "Edit…", "Remove…", "Merge…", "Add project…". A button that acts at once has
  none: "Sync 3 skills", "Install", "Use link".
- **Buttons name the action and its object or count**: "Sync 3 skills", "Remove 3", "Review
  changes". Never "OK", "Submit", "Confirm", "Yes". While a button's job runs its label says so
  ("Updating…") and it is disabled.
- **Status words are a closed set**; reuse one, never coin a new one ("off", "error", "ok" don't
  exist). Lowercase, after a dot, no verb:
  - skill rows (`sync_status`, `src/ui/installed.rs`): "synced", "N to link", "not in .agents",
    "broken links", "broken in .agents";
  - skill placements (`Placement::label`, `src/scan.rs`): "linked", "its own copy",
    "links elsewhere", "broken link", "not linked";
  - MCP rows (`status`, `src/ui/mcp.rs`): "in sync", "differs", "read-only";
  - tags: "global", "disabled", "update", "also global", "installed"; in an agent's detail
    line: "off here", "managed by <app>".

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
  Gemini CLI, GitHub Copilot, Windsurf, Goose, Kiro, Roo Code; short forms only in tags
  ("Claude", "Gemini"). The shared folder is `.agents`, always with the dot.

```rust
// Correct: an ellipsis because it opens a dialog, sentence case, a specific verb
ds::danger("Remove…", Place::Inspector).on_press(move |s: &mut App| s.confirm_mcp_remove(&name))

// Incorrect: no ellipsis though it opens a dialog, title case, and a generic verb
ds::danger("Delete Server", Place::Inspector)
```

## Relation to rune-kit

`ds` wraps rune-kit's controls (Button, IconButton, TextInput, Select, Checkbox, Menu, Sidebar,
Toolbar, Toast, SplitView, and in Prism Switch, SegmentedControl, RadioItem, TextArea, Spinner,
Markdown) and uses the nearest value the kit offers. Where the kit cannot match a rule, the
difference is a recorded exception below, not a reimplementation.

The dialog frame is the one block `ds` builds on raw Rune, because the kit's dialog is centred,
has radius 6, a smaller title and no footer. It keeps what the kit's dialog provides: focus trap,
Escape and outside press to close, focus return, the close button, the dialog role. `ds::tabs`
is drawn on raw Rune too, because the kit keys its segments `segment<i>`; its raw values mirror
the kit's `SegmentedControl`.

Raw values and kit calls live in `src/ds/`. The lint covers `src/ui/` only.

## Recorded exceptions

From Keysmith, and true in Prism:

- **No 32-point controls.** The kit's heights are 16/20/24/28/36. Toolbar and dialog footer
  buttons use 28 (`Place::Bar`, `ControlSize::Large`), not the 32 first planned.
- **Kit typography stays** in sidebar rows, the toolbar title and toasts. Sidebar rows do take
  `ROW_1`/`ROW_2` heights and the spacing scale.
- **Toolbar edge 12.** `SplitView` pads the toolbar 12 from the main pane's edge and offers no
  setter, so the toolbar's buttons sit 4 outside the page's 16 content edge.
- **Menu items are keyed by index.** rune-kit's `Menu` has no item key setter, so overflow items
  are `<key>-popup-item<i>`. Menu lists keep a stable order and disable an item that does not
  apply rather than hide it, so each index keeps its meaning. An inspector's "…" takes the
  keys only while it is drawn (`skill_menu_shown`, `mcp_menu_shown`), and its open flag is
  cleared when the selection changes or goes.
- **Dialog top in a short window.** The rule "top 16 when the window is too short for
  `DIALOG_TOP`" is not implemented: Rune gives `build` no window size, and the window's minimum
  height is 520. The dialog shrinks instead and its body scrolls; the footer stays visible (the
  Add MCP server form scrolls a little at the default window size).
- **Inspector minimum 352.** The spec's 320 was tuned after a screen showed it was too narrow;
  Prism's action rows are checked at 352 too.

Prism's own:

- **Switches keep the kit's size.** Switches are not buttons, so `Place` does not apply:
  `ds::switch` is the kit's compact switch in dark and the standard one in light, where the
  compact one disappears on a light pane when off.
- **The environment switcher is a bordered field.** `hosts::switcher` is a `Select` with the
  field appearance at the head of the sidebar, not a sidebar row.
- **The install-scope `Select` stays inline (24).** On skills.sh "Install into" and its
  `Select` keep the kit's inline look: a 28 bordered field would be heavy in the toolbar, and
  the kit cannot size an inline `Select`.
- **Narrow toolbars fold.** In a list narrower than the toolbar needs (the 860 window with the
  inspector open), the title hides, New skill, Check skills.sh for updates and Rescan move into
  a toolbar "…" (⌘N and ⌘R keep working), and Sync / Add server show their icon alone, without
  contrast.
- **Open SKILL.md folds only when it has to.** In the skill inspector Open SKILL.md stays in
  the action row while the drawn inspector fits it; only narrower (352) does it move into "…",
  after Reveal.
- **The lock and close icons are 16.** The lock on a read-only MCP copy (`size::ICON`) and the
  remove × of an agent's copy (an `icon_button`, the kit's 16) are full-size icons; `ds` has no
  smaller icon token (they were 12 before).
- **The skills.sh footer placeholder wraps at the minimum window.** "owner/repo, owner/repo@skill
  or a URL" is kept as written; in a 244 list it wraps. The footer's buttons sit beside the
  input when there is room (`size::FIELD_MIN`), under it otherwise.
- **No spinner in buttons.** `ds` has no loading button; a running Update reads "Updating…" and
  is disabled.

### Motion gaps

Rune cannot do these well, so they are left out rather than faked:

- **No selection slide.** The selection jumps to the new row: Rune's matched-element motion
  paints above the window and would cover the destination row.
- **Dialogs grow about their centre**, not from the control that opened them (scale 0.97 to 1).
- **Buttons have no press scale.** Only list rows and pressable card rows shrink under the
  pointer; kit buttons show a press by darkening their fill.

## How to add a screen

1. **Pick a skeleton.** A list of things is a `Page` of `ListRow`s with a `Toolbar` whose one
   `primary` is the next step. Details of one thing are an `Inspector`. A question or a form is
   a `DialogFrame` of the right `DialogKind`; a write shows its `Plan` first.
2. **Fill it with blocks.** Group details in `Card`s of `pair_row`, `stacked_row`, `value_row`,
   `item_row`; group form fields in `Card`s of `field` and `field_pair`. Put state in `Notice`s
   and `status` with a `Tone` from the functions under "Status tones"; put actions in `button`,
   `quiet`, `danger`, with the rest in `overflow`. Mask secrets.
3. **Pick roles, not values.** Text through `ds::text`, spacing from `space::`, sizes from
   `size::`, buttons with a `Place`. Never a raw number, style or colour. Words as under
   "Words".
4. **Check it** (below). If a block is missing, add it to `ds`, to the gallery and to "Prism
   additions" first.

## Checking a change

- `cargo test` (the lint `ui_lint::no_raw_values_in_ui` included) and
  `cargo clippy --all-targets -- -D warnings`.
- Snapshots render the real window headless at 2x:
  `target/debug/prism --snap /tmp/p.png --scene <scene> --theme light|dark`.
  `scripts/snap-scenes.sh <out-dir>` renders every scene in both themes from a demo home into
  `<out-dir>/<scene>-<theme>.png` (`none` is the default view). Scenes (`SCENES` in
  `src/main.rs`): `ds` (the gallery), `detail`, `sync`, `remove`, `bulk`, `bulk-remove`,
  `browse`, `notice`, `found`, `updates`, `new-skill`, `mcp`, `mcp-add`, `mcp-copy`, `copy`,
  `copy-to-host`, `repo`, `env-remote`, `env-unreachable`.
- `scripts/demo-home.sh` regenerates the README screenshots in `docs/` from a throwaway home;
  run it when a change is visible in them.
- `/usr/bin/diff -r ~/Development/ssh-keys-managment/src/ds src/ds`: every hunk is an item
  below.

## Prism additions

Changes to `src/ds/` beyond Keysmith's `287ebb5`, each generic (no Prism type, no `crate::`
import), in the gallery when it is a block, and tested in `ds/tests.rs` when it has logic. A
backport to Keysmith is a copy plus this list.

| File | Addition | Why |
| --- | --- | --- |
| `mod.rs` | `mod doc; mod plan;` and their `pub use` | The two new files below. |
| `tokens.rs` | `size::ROW_TRAILING` (104) | The width of a list row's trailing column, so statuses line up row to row. |
| `tokens.rs` | `color::checked` (kit `layer_01`) | The fill of a picked row that is not selected; selected (`layer_02`) wins. |
| `tokens.rs`, `dialog.rs` | `DialogKind::Wide`, `size::DIALOG_WIDE` (760) | A third dialog width for picks on the left that change a plan on the right; fits the 860 minimum window with `S4` either side. |
| `tokens.rs` | `size::DIALOG_PICKS` (250) | The picks column on the left of a Wide dialog (Sync's "Link into", MCP Copy / Merge); the plan takes the rest. |
| `tokens.rs` | `size::INSPECTOR_READ_MAX` (640) | The widest an inspector that shows a document may be dragged, so SKILL.md can be widened to read; `INSPECTOR_MIN` still applies. |
| `tokens.rs` | `size::FIELD_KEY` (170) | The name field of a name = value row in a form: the MCP form's variable and header names. |
| `tokens.rs` | `size::FIELD_MIN` (200) | The narrowest a text field gets with its buttons beside it on one line; narrower, the buttons go under it: the skills.sh footer's source field. |
| `tokens.rs` | `Place::height()` (24 / 28 / 28) | A control's height for a place, as the kit draws `Place::control()`: `tabs` uses it, and tests assert heights without a literal. |
| `tokens.rs` | `#[allow(dead_code)]` on `motion::COPY_CONFIRM_SECS` | Prism has no copy button, so the token is unused here; it stays for Keysmith. |
| `controls.rs` | `segments(labels, current, label, on_pick)` (`on_pick: Fn + Clone`, passed straight to the kit) | Kit `SegmentedControl`, fitted to its labels at `Place::List` height: Skills and MCP status filters. |
| `controls.rs` | `tabs<S>(k, label, items: &[(key, label)], current, on_pick) -> El` | Segments that switch what the window shows, drawn as the kit's `SegmentedControl` at `Place::Bar` height (radiogroup of radios, `radius::CONTROL`, `space::S3` sides), with a caller key on each segment: the kit keys its segments `segment<i>`, and the page tabs must stay `tab-skills` / `tab-mcp`. Its raw values (0.5 hairline, 2/1 focus ring, 13 line) and direct `k.c` / `k.ty` mirror the kit's control, with a comment saying so. Also serves the MCP form's Command / URL switch (`mcp-add-kind-stdio` / `mcp-add-kind-http`). |
| `controls.rs` | `reveal(shown, place)` | A `quiet` button with an eye: "Show values" / "Hide values", the trailing control of a card whose values are masked. |
| `controls.rs` | `secret_input(edit, on_edit, shown, on_toggle)` → `SecretInput` (`label`, `placeholder`, `invalid`, `on_submit`, `key`, `eye_key`, `build(k)`; `pub(super) SecretInput::unwired` for the gallery) | A masked `TextInput` at full width and its eye `icon_button` ("Show value" / "Hide value"): env and header values, a command or URL with a secret. A builder rather than an `El`, because building needs the kit and the field and eye carry their own keys. |
| `controls.rs` | `switch(k, on)` | Kit `Switch` at the kit's size (Mini, 36×16) in dark, the standard switch in light, where the compact one disappears on a light pane when off: linking a skill into an agent. |
| `controls.rs` | `button_width(k, label, place, icon) -> f32`, `toolbar_title_width(k, title) -> f32` | What a text button and an untruncated `toolbar_title` take, measured before layout with the kit's push-button metrics (padding 12/14, 16 icon + 6 gap, medium UI face): the toolbars fold into "…" and the skill inspector moves Open SKILL.md into "…" when they don't fit. Not blocks, so no gallery entry; a ds test checks them against the laid-out widths. |
| `page.rs` | `Page::segments(El)` | A slot between the filter and the status line for those segments; they shrink in their row (`shrink()`), so narrower than their labels need they share the content width and truncate instead of painting past the content edge. |
| `page.rs` | `Page::selection(El)` and `Selection<S>` (`new`, `label`, `summary`, `picked`, `clear_help`, `action`, `danger`, `build`) | The status line's first line on a pickable list: select-all checkbox (`check-all`, dash when partial) on the rows' checkbox edge, a summary or the bulk bar (`bulk-bar`; "N selected ×" is one pressable `bulk-clear`, role button, label "Clear selection", tooltip from `clear_help`), actions, `danger` pinned right; wraps rather than overflow. |
| `page.rs` | `Page::footer(El)` | A block under the list that does not scroll (with an empty state, the empty state takes the list's room): skills.sh "Or install straight from a repository". |
| `page.rs` | `ListRow::pick`, `ListRow::pick_key` | A leading checkbox and the checked fill for ⌘/⇧-click and checkbox multi-select. |
| `page.rs` | `ListRow::trailing(El)` | A `ROW_TRAILING`-wide, end-aligned column after the tags (`<key>-trailing`) for a status or a short value. |
| `page.rs` | `ListRow::build_tx` | `build` with the press's `Tx`, so a row press sees ⌘/⇧; `build` keeps its signature and behaviour. |
| `page.rs` | `loading(k, text)` | A kit `Spinner` and text in the Label style that wraps, the spinner on the first line; never a spinner alone: searching, reading a repository, installing, connecting. |
| `page.rs` | `empty_action(k, title, detail, action)` | `empty` with a button under the detail: Install prism, Retry. |
| `doc.rs` | `markdown::<S>(k, md)` | Kit `Markdown` on the type roles: body Body (`ty::value`), headings 1–3 Name (`ty::name`, never larger than the pane title), inline and block code Code, list markers muted; links open through `tx.open_url`: SKILL.md. |
| `text.rs` | `text::command(k, s)` | Code style as a flex-wrapped row of one selectable text per word, so a command wraps between words and `--stdio` never splits after `--`. |
| `plan.rs` | `Plan` (`new`, `heading`, `group`, `line`, `note`, `skipped`, `heads_up`, `notice`, `waiting`, `failed`, `build -> Vec<El>`) | What a write will do, as dialog body cards: a changes card labelled as the caller words it, of grouped `item_row`s (subject, lines joined " · " as a hint, `<key>-group-<i>`) or plain Body lines (`<key>-line-<i>`) or muted Label notes (`note`, `<key>-note-<i>`: "Nothing to do.") or a `loading` line (`<key>-waiting`); "Skipped (n)" (warn lines) and "Heads-up" (info lines) cards, left out when empty; a warn notice on top; a failure is one bad notice keyed `<key>-failed` instead of the cards; a heading with no group, line, note or loading line draws no changes card. |
| `dialog.rs` | `DialogFrame::footer_cancel(cancel)` | A footer with Cancel and no main action: Copy to…, where picking a scope is the action. `footer` keeps its signature and behaviour. |
| `dialog.rs` | `DialogFrame::error_key(key)` | A second key on the error line's text, besides the line's `<key>-error`: the MCP form's `mcp-add-error` and Add host's `host-error` stayed when their errors moved into the frame. `error` keeps its behaviour. |
| (rule) | Kit controls `Switch`, `SegmentedControl`, `RadioItem`, `TextArea`, `Spinner` | Allowed in `ds` and its callers, through the wrappers above where there is one (`switch`, `segments`, `loading`); `RadioItem` and `TextArea` as the kit draws them, inside card rows, like `Checkbox`. |
| `gallery.rs` | A picking page under the inspector; a summary selection line ("Selection"); `loading` and `empty_action` ("States"); a fifth column with "Tabs", a `Plan` (grouped, skipped, heads-up), `command`, `markdown`, and "Inspector controls" (`switch` off / on, `reveal` both ways, `secret_input`) | Every new block in the gallery; the `ds` scene snaps at 1500×760 (`scripts/snap-scenes.sh`) to fit it. A Wide dialog is too big for a column; dialog kinds are covered by tests. |
| `tests.rs` | Tests for the additions: `wide_dialog_is_760_and_fits_860`, `cancel_only_footer`, `error_line_keeps_its_key_and_takes_another`, and the `page_additions` and later modules (segments, picking, trailing, selection line, footer, `build_tx`, loading / empty_action, tabs, rows and segments at the smallest width, `button_width`, `command`, `markdown`, `Plan` states, `reveal`, `secret_input`, `switch`) | Each addition with logic has a test; Keysmith's tests are unchanged. |

### Fixes to Keysmith's ds

| File | Fix | Why |
| --- | --- | --- |
| `page.rs` | `Page::build`: the list is `self_stretch()` instead of `w_full()`, so with its `mx(-S2)` it reaches `S2` past the content edge on both sides. | `w_full` plus a negative margin shifted the list left instead of widening it, so rows ended 16pt short of the right content edge, and trailing statuses didn't line up with the filter or the selection line's danger button. Backport note: Keysmith's rows end 16pt short today. |
| `page.rs` | `ListRow`: the name and tags sit in a middle row (`grow`, `min_w(0)`); the name column's floor is its min-content, an invisible `ROW_NAME_MIN` strut (the name and meta lines are pinned over it, so a long meta adds nothing), capped by its full width in the middle (`w_frac(1.0)`, CSS automatic minimum size), instead of `min_w(ROW_NAME_MIN)`. Wide rows lay out as before. | With a pick checkbox and a `ROW_TRAILING` column, a 244-wide list (the 860 window with the inspector open) could not fit pick + `ROW_NAME_MIN` + trailing, so the trailing status was cut at the pane edge. Now tags give way first, then the name truncates below `ROW_NAME_MIN`; the trailing column keeps its width. Backport note: Keysmith's rows have no trailing column, so nothing changes there. |
