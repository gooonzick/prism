# Prism on the Shared Design System — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild every Prism screen and dialog on Keysmith's design system (`src/ds/`), with the raw-value lint on.

**Architecture:** Copy Keysmith's `src/ds/` and `src/ui_lint.rs` verbatim, then add the generic blocks Prism needs (each recorded for a later backport), then migrate `src/ui/` file by file onto `ds`, then replace the old design documents. Behaviour does not change except inspector actions moving into "…".

**Tech Stack:** Rust 2024, Rune + rune-kit at git rev `7ead7d63` (source: `~/.cargo/git/checkouts/rune-*/7ead7d6/crates/`). Keysmith: `~/Development/ssh-keys-managment` at `287ebb5`.

**Spec:** `docs/superpowers/specs/2026-11-10-prism-design-system-design.md`. Read it first, and Keysmith's `DESIGN.md`; this plan argues from both.

## Global Constraints

- Branch `design-system`. One commit per task. Never push.
- After every task: `cargo test` and `cargo clippy --all-targets -- -D warnings` pass.
- `src/ds/` never imports `crate::` (anything outside `ds`). Every change to it beyond Keysmith's copy is generic and goes in the "Prism additions" list (kept in `docs/superpowers/ds-additions.md` until Task 11 moves it into `DESIGN.md`): file, what, why.
- Changing an existing `ds` item: tune a token or add a builder method; never change an existing method's behaviour or signature.
- Existing element keys stay (`skill-<name>`, `check-<name>`, `check-all`, `bulk-*`, `status-filter`, `tab-skills`, `tab-mcp`, `mcp-*`, `sync`, `remove`, `copy`, `open-md`, `reveal`, …). If a key must change, update `src/ui_tests.rs` in the same task.
- Copy is unchanged: every label, tooltip, note and status word stays as written today.
- Secrets stay masked by default everywhere they are today.
- rune-kit menu items are `<key>-popup-item<i>`: fixed order, disable rather than hide. The "…" open flag counts in `modal()` only while drawn and is cleared when the selection changes or disappears.
- `Select`s in a dialog go in `DialogFrame::menus`. A scrolling list of fields keeps `size::FOCUS_RING` of room. Toolbar and dialog-footer buttons use `Place::Bar` (28); no 32.
- Snapshots: `scripts/snap-scenes.sh <out-dir>` (Task 1) renders every scene in light and dark from a demo home into `<out-dir>/<scene>-<theme>.png`. Before-snapshots of the old UI are in `/tmp/prism-before/` (same names; `none` is the default view). Every UI task compares its scenes, both themes, against them: nothing missing, nothing overflowing.
- Test harness: `src/ui_tests.rs` (`Harness`, `rt.find(key)`, `click(key)`, `settle()`); `ds` block tests in `src/ds/tests.rs` (`Runtime<()>`).

## Review Focus

1. **Long text in a 352 inspector**: a skill or server name like `vercel-react-best-practices-for-large-apps`, a 200-character command, a long URL. Titles wrap, values wrap or truncate inside their card, the action row stays on one line, nothing is wider than the pane. Tests in Tasks 6 and 7.
2. **Secrets**: a server with `env TOKEN=sk-live-abc` and a header `Authorization: Bearer abc`. `abc` appears nowhere (row meta, inspector cards, Differences, Edit form prefill, plan lines) until Show values is pressed, and only for that server. Tests in Tasks 7 and 10.
3. **Minimum window 860×520 with Sync open**: the Wide dialog (760) fits, its body scrolls, the footer stays visible; the inspector shows at 860 with the default sidebar. Tests in Tasks 3 and 4.
4. **Keyboard after "…"**: open the inspector's "…", move the selection with ↓: the menu closes, the flag clears, ↓ keeps moving. Tests in Tasks 6 and 7.
5. **On a host** (`env-remote`): Open SKILL.md and Reveal are disabled (Reveal disabled in the menu, not removed); Check for updates disabled. Test in Task 6.

## File Structure

```
src/ds/*                 copied verbatim from Keysmith (Task 1), then extended:
  tokens.rs              + DIALOG_WIDE, ROW_TRAILING, INSPECTOR_READ_MAX, color::checked
  page.rs                + Page::segments/selection/footer, ListRow::pick/trailing/build_tx, loading, empty_action
  controls.rs            + segments, switch, reveal, secret_input
  dialog.rs              + DialogKind::Wide, cancel-only footer
  plan.rs (new)          Plan
  doc.rs (new)           markdown
  text.rs                + command
  gallery.rs, tests.rs   + every addition
src/ui_lint.rs           copied verbatim
src/ui/mod.rs            shell on ds; old helpers deleted in Task 11
src/ui/{installed,inspector,mcp,browse,dialogs,mcp_dialogs,hosts}.rs   migrated
scripts/snap-scenes.sh   all scenes × both themes
docs/superpowers/ds-additions.md   running list, folded into DESIGN.md in Task 11
DESIGN.md, AGENTS.md     rewritten; docs/design/ deleted (Task 11)
```

---

### Task 1: Import the design system verbatim

**Files:** Create `src/ds/*` (copy), `src/ui_lint.rs` (copy), `scripts/snap-scenes.sh`, `docs/superpowers/ds-additions.md`; modify `src/main.rs`, `src/app/mod.rs`, `src/ui/mod.rs`.

**Interfaces — Produces:** `crate::ds::*` exactly as Keysmith's; `App::gallery: bool`; scene `ds`.

- [ ] **Step 1:** `cp -R ~/Development/ssh-keys-managment/src/ds src/ && cp ~/Development/ssh-keys-managment/src/ui_lint.rs src/`. Check `diff -r ~/Development/ssh-keys-managment/src/ds src/ds` prints nothing.
- [ ] **Step 2:** In `src/main.rs`: `mod ds;` and `#[cfg(test)] mod ui_lint;`; scene `("ds", |s| s.gallery = true)` added to `SCENES`. In `App`: `pub gallery: bool` (doc: "Show the design system's gallery instead of the app (`--scene ds`)"), `false` in `App::new`. In `ui::view`, first thing after `Kit`: `if s.gallery { let g = crate::ds::gallery::<App>(&k, &mut layer); return rune_kit::root(g, layer); }`.
- [ ] **Step 3:** Mark `no_raw_values_in_ui` `#[ignore = "on once every screen is on ds (Task 11)"]` — the only edit to `ui_lint.rs`, reverted in Task 11. `the_pattern` stays on.
- [ ] **Step 4:** `scripts/snap-scenes.sh <out>`: runs `scripts/demo-home.sh "$out/readme"`, takes the demo home from its last line ("… demo home in <path>"), then for every scene in `SCENES` plus `none` (no `--scene`) and `ds`, for `light` and `dark`, runs `prism --snap "$out/<scene>-<theme>.png" --size 1180x760 [--scene <scene>] --theme <theme>` with `HOME=<demo>` and `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `XDG_CONFIG_HOME` unset (as `demo-home.sh` does). Scene names are read from `src/main.rs` with `grep -o '("[a-z-]*", |s|'`.
- [ ] **Step 5:** `docs/superpowers/ds-additions.md` with a heading "Prism additions to `src/ds/`" and the line "Keysmith base: `287ebb5`." Each later task appends to it.
- [ ] **Step 6:** `cargo test` and clippy pass (if clippy reports dead code in `ds`, put `#[allow(dead_code)]` on `mod ds;` in `main.rs`, not inside `ds`; Task 11 removes it). `scripts/snap-scenes.sh /tmp/prism-t1`; `ds-light.png` and `ds-dark.png` show Keysmith's gallery; other scenes match `/tmp/prism-before/`.
- [ ] **Step 7:** Commit `feat(ds): import the design system from Keysmith`.

### Task 2: Page additions

**Files:** Modify `src/ds/{tokens,page,controls,gallery,tests}.rs`, `docs/superpowers/ds-additions.md`.

**Interfaces — Produces:**

```rust
// tokens.rs
pub mod size { pub const ROW_TRAILING: f32 = 104.0; }      // a list row's trailing column
pub mod color { pub fn checked(k: &Kit) -> Color /* k.c.layer_01 */; }
// controls.rs
pub fn segments<S: 'static>(labels: &[String], current: Option<usize>, label: &str,
                            on_pick: impl Fn(&mut S, usize, &mut Tx) + 'static) -> SegmentedControl; // fit(), Place::List height
// page.rs
impl Page {
    pub fn segments(self, e: El) -> Self;    // between filter and status line
    pub fn selection(self, e: El) -> Self;   // first line of the status line, replaces `count` when set
    pub fn footer(self, e: El) -> Self;      // under the list, does not scroll
}
pub struct Selection<S>; impl<S: 'static> Selection<S> {
    pub fn new(all: bool, partial: bool, enabled: bool, on_toggle_all: impl Fn(&mut S) + 'static) -> Self; // key "check-all"
    pub fn summary(self, s: impl Into<String>) -> Self;        // Label, truncates, when nothing is picked
    pub fn picked(self, n: usize, on_clear: impl Fn(&mut S) + 'static) -> Self; // "N selected" + clear (key "bulk-clear"); bar keyed "bulk-bar"
    pub fn action(self, b: El) -> Self;                        // after summary or picked line
    pub fn danger(self, b: El) -> Self;                        // pinned right
    pub fn build(self, k: &Kit) -> El;
}
impl ListRow {
    pub fn pick<S: 'static>(self, on: bool, label: impl Into<String>, on_toggle: impl Fn(&mut S, &mut Tx) + 'static) -> Self; // leading Checkbox, hidden label, keyed "check-<key minus prefix>" by the caller via pick_key
    pub fn pick_key(self, key: impl Into<String>) -> Self;
    pub fn trailing(self, e: El) -> Self;                      // ROW_TRAILING wide, end-aligned
    pub fn build_tx<S: 'static>(self, k: &Kit, on_press: impl Fn(&mut S, &mut Tx) + 'static) -> El; // like build, passes Tx (modifiers)
}
pub fn loading(k: &Kit, text: impl Into<String>) -> El;        // Spinner + Label, one row
pub fn empty_action(k: &Kit, title: impl Into<String>, detail: impl Into<String>, action: El) -> El; // empty + button under
```

A picked row (`pick(true, …)`) that is not selected fills `color::checked` with hover; selected wins. The select-all checkbox sits on the same x as row checkboxes; partial shows `glyphs::DASH`. `ListRow::build` keeps its signature and behaviour.

- [ ] **Step 1: Failing tests** in `src/ds/tests.rs`:
  - `segments_sit_between_filter_and_status`: a `Page` with filter, segments, count, rows: y(filter) < y(segments) < y(count) < y(first row).
  - `pick_checkbox_lines_up_with_select_all`: x of `check-all` equals x of `check-a` within 0.5.
  - `checked_row_fills_checked_selected_wins`: `ListRow::pick(true)` presented fill is `color::checked`; with `.selected(true)` it is `color::selected`.
  - `trailing_columns_line_up`: two rows with trailing `status` of different word lengths: the trailing elements have equal right edges and equal widths (`ROW_TRAILING`).
  - `selection_bar_keeps_danger_right`: with `picked(3)`, two actions and a danger at width 640: danger's right edge is the content edge; at width 400 nothing overflows the page.
  - `footer_does_not_scroll`: 50 rows and a footer in a 400-high page: footer fully visible at the bottom, list scrolls.
  - `build_tx_sees_modifiers`: pressing with ⌘ held passes `tx.mods().cmd == true` to the handler.
- [ ] **Step 2:** `cargo test ds::` → the new tests FAIL.
- [ ] **Step 3:** Implement the interfaces above.
- [ ] **Step 4:** Add each to `gallery.rs` (a segments row, a picked row, a selection line in both states, a trailing status, a footer, `loading`, `empty_action`).
- [ ] **Step 5:** Tests pass; clippy clean; snapshot `ds` in both themes and look at the new blocks.
- [ ] **Step 6:** Append to `ds-additions.md`: each new item with file and one-line why (spec rows 3–9).
- [ ] **Step 7:** Commit `feat(ds): segments, picking, selection line, trailing column, footer, loading`.

### Task 3: Inspector and dialog additions

**Files:** Modify `src/ds/{tokens,text,controls,dialog,mod,gallery,tests}.rs`; create `src/ds/plan.rs`, `src/ds/doc.rs`; `docs/superpowers/ds-additions.md`.

**Interfaces — Produces:**

```rust
// tokens.rs
pub mod size { pub const DIALOG_WIDE: f32 = 760.0; pub const INSPECTOR_READ_MAX: f32 = 640.0; }
// dialog.rs
pub enum DialogKind { Confirm, Form, Wide }                   // width() -> DIALOG_WIDE for Wide
impl DialogFrame { pub fn footer_cancel(self, cancel: El) -> Self; } // no main action
// text.rs
pub fn command(k: &Kit, s: impl Into<String>) -> El;          // Code style, wraps between words only, selectable
// doc.rs
pub fn markdown<S: 'static>(k: &Kit, md: impl Into<String>) -> El; // body Body, headings Name, code Code; links open via tx.open_url
// controls.rs
pub fn switch(k: &Kit, on: bool) -> Switch;                   // Place::Inspector; .standard() when !k.dark
pub fn reveal(shown: bool, place: Place) -> Button;           // quiet + eye icon, "Show values" / "Hide values"
pub fn secret_input<S: 'static>(edit: &TextEdit, on_edit: impl Fn(&mut S) -> &mut TextEdit + 'static, shown: bool, on_toggle: impl Fn(&mut S) + 'static) -> El; // masked TextInput + eye icon_button; caller keys it
// plan.rs
pub struct Plan; impl Plan {
    pub fn new(key: impl Into<String>) -> Self;
    pub fn heading(self, s: impl Into<String>) -> Self;       // the changes card's label, as the caller words it
    pub fn group(self, subject: impl Into<String>, lines: Vec<String>) -> Self; // item_row: Name + lines joined " · " as Hint
    pub fn line(self, s: impl Into<String>) -> Self;          // a plain Body row
    pub fn skipped(self, items: Vec<String>) -> Self;         // card "Skipped (n)", omitted when empty
    pub fn heads_up(self, items: Vec<String>) -> Self;        // card "Heads-up", omitted when empty
    pub fn notice(self, n: El) -> Self;                       // on top (Tone::Warn notice)
    pub fn waiting(self, text: impl Into<String>) -> Self;    // loading inside the changes card
    pub fn failed(self, why: impl Into<String>) -> Self;      // Notice Tone::Bad instead of the cards
    pub fn build(self, k: &Kit) -> Vec<El>;                   // cards, for DialogFrame::body
}
```

`Switch` in the light theme uses `.standard()` because the compact switch disappears on a light pane when off (`placement_row` in `src/ui/inspector.rs` today). `markdown` is today's `skill_md` with role styles: headings 1–3 at `ty::name` (no heading larger than the pane title), body `ty::value` wrapping, code `ty::code`.

- [ ] **Step 1: Failing tests:**
  - `wide_dialog_is_760_and_fits_860`: a Wide frame in an 860×520 window: panel width 760; with a 2000-high body the footer is fully visible and the body scrolls.
  - `cancel_only_footer`: `footer_cancel` renders `close`, the cancel button, and no second button.
  - `command_breaks_between_words`: `command("npx -y figma-developer-mcp --stdio")` in a 120-wide column. `command` is a flex-wrapped row of one text element per word (as today's `words()`); assert the element for `--stdio` keeps its natural width (not split, not clipped) and the row is more than one line high.
  - `markdown_headings_are_not_titles`: a `# Big` heading's line height is `ty::name`'s, not `ty::title`'s.
  - `plan_states`: groups → one card with N item rows; empty skipped/heads-up → no extra cards; `failed` → exactly one notice keyed `<key>-failed`, no changes card; `waiting` → a spinner element inside the changes card.
  - `reveal_labels`: `reveal(false, …)` label "Show values", `reveal(true, …)` "Hide values".
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement. `mod.rs` gains `pub mod doc; mod plan; pub use plan::*;` (or `pub use doc::*`, consistent with the rest).
- [ ] **Step 4:** Gallery: a Wide dialog is too big for the gallery column; show a `Plan` (grouped, skipped, heads-up), `command`, `markdown`, `switch` on/off, `reveal`, `secret_input` shown/hidden. The dialog kinds are covered by tests.
- [ ] **Step 5:** Tests pass, clippy clean, `ds` snapshots in both themes checked.
- [ ] **Step 6:** Append spec rows 1, 2, 10–17 to `ds-additions.md`.
- [ ] **Step 7:** Commit `feat(ds): wide dialogs, plan, markdown, command text, reveal, secret input, switch`.

### Task 4: Shell, sidebar, toolbar, host states

**Files:** Modify `src/ui/mod.rs`, `src/ui/hosts.rs` (`switcher`, `pending` only), `src/main.rs` (`min_size`), `src/app/mod.rs` (pane defaults); tests in `src/ds/tests.rs` or `src/ui_tests.rs`.

**Interfaces — Consumes:** Tasks 1–3. **Produces:** `ui::toolbar(s, k, title, trailing: El) -> Toolbar` (tabs + `toolbar_title` in leading) used by every page; `ui::inspector_pane(s, cx) -> Option<PaneState>` as Keysmith's.

- [ ] **Step 1: Failing tests** (`src/ui_tests.rs`):
  - `inspector_shows_at_minimum_window`: window 860×520, a skill selected: `rt.presented("inspector")` is visible and at least `INSPECTOR_MIN` wide; list at least `LIST_MIN`.
  - `inspector_hides_when_sidebar_is_wide`: sidebar width set to 300 in an 860 window: the inspector is not drawn; back at 232 it returns at its stored width.
  - `toolbar_title_follows_tabs`: x(`tab-mcp`) right edge < x of the title, both in the leading area.
  - A `const` check like Keysmith's `pane_widths`: `232.0 + LIST_MIN + INSPECTOR_MIN <= 860.0`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** `min_size: Some((860.0, 520.0))`; `sidebar: PaneState::new(232.0).range(180.0, 420.0)`; `inspector: PaneState::new(380.0).range(size::INSPECTOR_MIN, size::INSPECTOR_READ_MAX).collapsed()`. `SplitView` configured as Keysmith's `src/ui/mod.rs:66-79` (frame inset `S2`, sidebar padding `S2, S2`, toggle `Place::Bar` with `past_lights(S4).edge(S4).gap(S1)`, `motion::panes()`), inspector through `inspector_pane`. Sidebar: rows `ROW_1`, project rows `ROW_2`, `header_gap(S1)`, `header_padding(0.0, S3)`, `row_gap(ROW_GAP)`. Page tabs: `ds::segments` keyed `page-tabs` with segment keys `tab-skills`/`tab-mcp` if the kit allows keyed segments; otherwise keep the hand-built tabs with every raw value replaced by tokens (`Place::Bar` height, `radius::CONTROL`, `space::S3`). Toast unchanged.
- [ ] **Step 4:** `hosts::pending`: connecting → `ds::loading`; needs install → `empty_action` with Install prism (`primary`, `Place::List`, disabled when unsupported, key unchanged); unreachable → `empty_action` with Retry (`button`). Toolbar keeps tabs and the host's name via `ui::toolbar`. `switcher` stays a kit `Select`, sized `Place::List`.
- [ ] **Step 5:** Tests pass, clippy clean. Snapshots: `none`, `detail`, `env-remote`, `env-unreachable` in both themes against `/tmp/prism-before/`; sidebar rows aligned, title after tabs.
- [ ] **Step 6:** Commit `refactor(ui): shell, sidebar and toolbar on ds`.

### Task 5: Skills page

**Files:** Modify `src/ui/installed.rs`; `src/ui_tests.rs` if a key moves.

**Interfaces — Consumes:** `Page`, `Selection`, `ListRow::{pick, pick_key, trailing, build_tx}`, `segments`, `tag`, `status`, `Tone`, `ui::toolbar`. **Produces:** `installed::placement_tone(p: &Placement) -> Option<Tone>` (replaces `placement_color`), `sync_status(..) -> (String, Tone, String)`.

- [ ] **Step 1: Failing tests** (`src/ui_tests.rs`):
  - `skill_rows_are_one_anatomy`: in the project harness, `skill-review` height is `ROW_2` and its name's x equals the filter's x (one content edge).
  - `bulk_bar_replaces_summary`: click `check-review`: `bulk-bar` present, `bulk-sync` present, `bulk-remove` right edge equals the page's content edge; `bulk-clear` empties the selection.
  - Existing tests on ticking, ⌘/⇧-click, Clean up and filters still pass unchanged.
- [ ] **Step 2:** Run → new tests FAIL.
- [ ] **Step 3:** Rebuild `page`, `toolbar`, `filters`, `list_header`, `bulk_bar`, `skill_row` on `ds` as the spec's "Skills" section says. Toolbar Sync: `primary` when `unsynced > 0 && s.checked.is_empty()`, else `button`; `Place::Bar`; icon `SYNC`. Bulk Sync is `primary`, Update/Copy to… `button`, Remove… `danger`, all `Place::List`. "Clean up N broken links" is a `quiet` action in the `Selection`. Tags: `ds::tag(k, short, placement_tone(p), help)`; "update" tag `Tone::Info`; "also global" no tone.
- [ ] **Step 4:** Tests pass, clippy clean, no raw values left in `installed.rs` (`cargo test ui_lint -- --ignored` lists none from this file). Snapshots `none`, `bulk`, `updates`, `notice` both themes vs before.
- [ ] **Step 5:** Commit `refactor(ui): skills page on ds`.

### Task 6: Skill inspector

**Files:** Modify `src/ui/inspector.rs`; app state for the "…" flag if Prism lacks one (`App::menu_open: bool` or as Keysmith names it, cleared in `select`/when the selection disappears, counted in `modal()` only while drawn).

**Interfaces — Consumes:** `Inspector`, `Card`, `item_row`, `plain_row`, `path`, `switch`, `markdown`, `overflow`, `MenuAction`, `Notice`. 

- [ ] **Step 1: Failing tests:**
  - `skill_actions_fit_one_row_at_minimum`: inspector at `INSPECTOR_MIN`, a skill named `vercel-react-best-practices-for-large-apps`: `open-md`, `copy`, the overflow button and `remove` share one y; `remove`'s right edge ≤ the inspector's content edge; the title wraps (height > one line), nothing wider than the pane.
  - `reveal_is_in_the_menu`: open the overflow: item 0 is Reveal and runs `reveal_path` (or, on a host, is disabled).
  - `menu_closes_on_selection_change` (Review Focus 4): open "…", press ↓: the menu is gone and a second ↓ moves the selection again.
  - `host_disables_local_actions` (Review Focus 5): in the `env-remote` setup `open-md` is disabled and the menu's Reveal item is disabled.
  - Existing tests on Use link, Move into .agents, switches, Show more agents pass unchanged.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Rebuild `installed` (inspector), `update_banner`, `actions`, `agents`, `placement_row` per the spec's "Skill inspector". Menu items in this order, stable: Reveal. If the row does not fit at `INSPECTOR_MIN`, move Open SKILL.md into the menu after Reveal (record it in the commit message).
- [ ] **Step 4:** Tests pass, clippy clean, no raw values in `inspector.rs`. Snapshots `detail`, `updates`, `env-remote` both themes vs before.
- [ ] **Step 5:** Commit `refactor(ui): skill inspector on ds`.

### Task 7: MCP page and inspector

**Files:** Modify `src/ui/mcp.rs`.

**Interfaces — Consumes:** `Page`, `ListRow::trailing`, `segments`, `Notice`, `Inspector`, `Card`, `pair_row`, `stacked_row`, `stacked_row_with`, `value_row`, `item_row`, `command`, `code`, `reveal`, `overflow`. **Produces:** `mcp::status(server) -> (&'static str, Tone, String)`.

- [ ] **Step 1: Failing tests:**
  - `secrets_masked_until_shown` (Review Focus 2): a server with env `TOKEN=sk-live-abc` and header `Authorization: Bearer abc`: no presented text in the window contains `abc` (walk `rt` text, or `rt.find_text` if the harness has it; otherwise assert on the strings the view builds via the masking helpers); after `mcp-reveal`, the Environment card shows `sk-live-abc`; selecting another server shows it masked.
  - `long_command_wraps_in_card`: a 200-character command at `INSPECTOR_MIN`: the Command row's width ≤ the card's, height > one line.
  - `merge_notice_in_status_line`: in the `mcp` demo setup the Merge notice's y is above the first row's.
  - `disable_here_in_menu`: in a project, the overflow's item 0 reads "Disable here" (or "Enable here"); in Global it is disabled.
  - `menu_closes_on_selection_change` for the MCP inspector (Review Focus 4).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Rebuild per the spec's "MCP servers" and "MCP server inspector". Keep `mcp-copy`, `mcp-edit`, `mcp-remove`, `mcp-values`, `mcp-copies`, `mcp-diffs` keys on the equivalent elements; the reveal button keeps today's key. Dim: `ListRow::dim(true)` (0.75).
- [ ] **Step 4:** Tests pass, clippy clean, no raw values in `mcp.rs`. Snapshots `mcp`, `mcp-copy` (background) both themes vs before.
- [ ] **Step 5:** Commit `refactor(ui): MCP page and inspector on ds`.

### Task 8: skills.sh page and its inspectors

**Files:** Modify `src/ui/browse.rs`, `src/ui/hosts.rs` (`browse_note` if it styles text).

**Interfaces — Consumes:** `Page::{footer}`, `ListRow::trailing`, `tag`, `loading`, `empty`, `Inspector`, `Card`, `markdown`, `primary`.

- [ ] **Step 1: Failing tests:**
  - `install_count_in_trailing_column`: in the browse setup the install counts of two rows share a right edge.
  - `repo_footer_stays_visible`: with 40 results the footer's input is fully visible.
  - Existing browse/install/repo tests pass.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Rebuild `page`, `result_row`, `details`, `repo_listing` per the spec. "installed" is `tag(k, "installed", None, help)` with today's help text or "Already in this scope". Install/Reinstall and Install N are `primary` `Place::Inspector`.
- [ ] **Step 4:** Tests pass, clippy clean, no raw values in `browse.rs`. Snapshots `browse`, `repo` both themes vs before.
- [ ] **Step 5:** Commit `refactor(ui): skills.sh page on ds`.

### Task 9: Skill dialogs

**Files:** Modify `src/ui/dialogs.rs`.

**Interfaces — Consumes:** `DialogFrame`, `DialogKind::{Confirm, Form, Wide}`, `footer_cancel`, `Plan`, `Card`, `field`, `pressable`, `item_row`, `Notice`, `loading`, `danger`, `primary`, `button`.

- [ ] **Step 1: Failing tests:**
  - `dialog_widths`: open each dialog via its scene setter; panel widths: sync 760, remove 440, copy 440, copy plan 560, new skill 560, found 560.
  - `sync_fits_minimum_window` (Review Focus 3): 860×520 window, Sync open with 10 agents and 20 skills: `sync-dialog` footer fully visible, body scrolls.
  - Existing tests on sync, remove, copy, new skill, found pass (keys `sync-dialog`, `remove-dialog`, `copy-dialog`, `new-skill-dialog`, `found-dialog` and their buttons unchanged).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Rebuild each dialog per the spec's dialog table. Sync's plan uses `Plan::group` with `group_actions`; skipped and heads-up as today; the warning line (`sync-notice`) is `Plan::notice`. Copy's machine headings become card labels; unreachable machine is a `Notice` `Tone::Bad`. Errors go through `DialogFrame::error`.
- [ ] **Step 4:** Tests pass, clippy clean, no raw values in `dialogs.rs`. Snapshots `sync`, `remove`, `bulk-remove`, `copy`, `copy-to-host`, `new-skill`, `found` both themes vs before.
- [ ] **Step 5:** Commit `refactor(ui): skill dialogs on ds`.

### Task 10: MCP and host dialogs

**Files:** Modify `src/ui/mcp_dialogs.rs`, `src/ui/hosts.rs` (`add_host`, `add_project`).

**Interfaces — Consumes:** as Task 9, plus `secret_input`, `segments`, `field_pair`, kit `RadioItem`, `Checkbox`, `TextArea`, `Select` (menus in `DialogFrame::menus`).

- [ ] **Step 1: Failing tests:**
  - `mcp_dialog_widths`: form 560; plan with picks 760; plan without picks 560; add host 560; add project 560.
  - `edit_form_masks_prefilled_secrets` (Review Focus 2): Edit on the secret server: the env value input shows masked text; its eye reveals it.
  - `copy_scope_menu_opens_by_its_select`: open the machine `Select` in MCP Copy: its popup is within 40 points of the select (not in the window corner).
  - Existing MCP form/plan and host tests pass (`mcp-form`, `mcp-plan`, `mcp-add-*`, `mcp-apply`, `mcp-cancel`, `add-host-dialog`, `host-*` keys unchanged).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Rebuild per the spec's dialog table. Agents checkboxes stay in two columns inside their card. The Command/URL switch is `ds::segments` keyed as today's `kind_switch`.
- [ ] **Step 4:** Tests pass, clippy clean, no raw values in either file. Snapshots `mcp-add`, `mcp-copy` both themes vs before.
- [ ] **Step 5:** Commit `refactor(ui): MCP and host dialogs on ds`.

### Task 11: Lint on, old helpers out, documents

**Files:** Modify `src/ui_lint.rs` (remove the `#[ignore]`), `src/ui/mod.rs` (delete `inspector_title`, `section`, `prop`, `code_box`, `words`, `mono`, `skill_md`, `heading`, `dot`, `empty`, `inspector_body`, `row_gap` and any other helper no screen uses), `src/main.rs` (drop `#[allow(dead_code)]` if added); rewrite `DESIGN.md`, `AGENTS.md`; delete `docs/design/`, `docs/superpowers/ds-additions.md` (folded in); regenerate README screenshots.

- [ ] **Step 1:** Remove the `#[ignore]`; `cargo test ui_lint` passes. `diff ~/Development/ssh-keys-managment/src/ui_lint.rs src/ui_lint.rs` prints nothing.
- [ ] **Step 2:** Delete the old helpers; `cargo build` and clippy clean (no dead code).
- [ ] **Step 3:** `DESIGN.md` per the spec's "What replaces the old documents": Keysmith's sections adapted to Prism (app name, scenes, the Prism additions folded into the block and token tables), plus "Secrets", "Words" (all of `copy.md`), "Status tones" (the decision tree mapped to `Tone`, naming `placement_tone` and `mcp::status`), "Prism additions" (from `ds-additions.md`), "Checking a change" (`scripts/snap-scenes.sh`, `scripts/demo-home.sh`). Every name in it exists in the code.
- [ ] **Step 4:** `AGENTS.md`: read `DESIGN.md` before UI work; checks are `cargo test` (lint included), `cargo clippy --all-targets -- -D warnings`, `scripts/snap-scenes.sh` in both themes.
- [ ] **Step 5:** `git rm -r docs/design docs/superpowers/ds-additions.md`; `grep -rn "docs/design" --exclude-dir=target .` finds nothing outside `docs/superpowers/`.
- [ ] **Step 6:** `diff -r ~/Development/ssh-keys-managment/src/ds src/ds`: every hunk is an item in `DESIGN.md`'s "Prism additions"; nothing else.
- [ ] **Step 7:** `scripts/demo-home.sh` (README screenshots); `scripts/snap-scenes.sh /tmp/prism-after`; compare every scene in both themes with `/tmp/prism-before/`: nothing missing, one content edge per pane, cards in inspectors and dialogs.
- [ ] **Step 8:** `cargo test`, clippy pass. Commit `docs: DESIGN.md on the shared system; lint on`.
