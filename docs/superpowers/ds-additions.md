# Prism additions to `src/ds/`

Keysmith base: `287ebb5`.

| File | Addition | Why |
| --- | --- | --- |
| `tokens.rs` | `size::ROW_TRAILING` (104) | The width of a list row's trailing column, so statuses line up row to row (spec #6). |
| `tokens.rs` | `color::checked` (kit `layer_01`) | The fill of a picked row that is not selected; selected (`layer_02`) wins (spec #4). |
| `controls.rs` | `segments(labels, current, label, on_pick)` (`on_pick: Fn + Clone`, passed straight to the kit) | Kit `SegmentedControl`, fitted to its labels at `Place::List` height: Skills and MCP status filters (spec #3). |
| `page.rs` | `Page::segments(El)` | A slot between the filter and the status line for those segments (spec #3). |
| `page.rs` | `Page::selection(El)` and `Selection<S>` (`new`, `label`, `summary`, `picked`, `clear_help`, `action`, `danger`, `build`) | The status line's first line on a pickable list: select-all checkbox (`check-all`, dash when partial) on the rows' checkbox edge, a summary or the bulk bar (`bulk-bar`; "N selected ×" is one pressable `bulk-clear`, role button, label "Clear selection", tooltip from `clear_help`), actions, `danger` pinned right; wraps rather than overflow (spec #5). |
| `page.rs` | `Page::footer(El)` | A block under the list that does not scroll: skills.sh "Or install straight from a repository" (spec #7). |
| `page.rs` | `ListRow::pick`, `ListRow::pick_key` | A leading checkbox and the checked fill for ⌘/⇧-click and checkbox multi-select (spec #4). |
| `page.rs` | `ListRow::trailing(El)` | A `ROW_TRAILING`-wide, end-aligned column after the tags (`<key>-trailing`) for a status or a short value (spec #6). |
| `page.rs` | `ListRow::build_tx` | `build` with the press's `Tx`, so a row press sees ⌘/⇧; `build` keeps its signature and behaviour (spec #4). |
| `page.rs` | `loading(k, text)` | A kit `Spinner` and text in the Label style that wraps, the spinner on the first line; never a spinner alone: searching, reading a repository, installing, connecting (spec #8). |
| `page.rs` | `empty_action(k, title, detail, action)` | `empty` with a button under the detail: Install prism, Retry (spec #9). |
| `gallery.rs` | A picking page under the inspector; a summary selection line and `loading` / `empty_action` | Every new block in the gallery. |

## Fixes to Keysmith's ds

| File | Fix | Why |
| --- | --- | --- |
| `page.rs` | `Page::build`: the list is `self_stretch()` instead of `w_full()`, so with its `mx(-S2)` it reaches `S2` past the content edge on both sides. | `w_full` plus a negative margin shifted the list left instead of widening it, so rows ended 16pt short of the right content edge, and trailing statuses didn't line up with the filter or the selection line's danger button. Backport note: Keysmith's rows end 16pt short today. |
