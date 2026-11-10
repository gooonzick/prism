# Type and colour

## The type ladder

Five levels. Each row is the only way to set that role; there is no sixth.

| Level | Role | Use | Colour |
| --- | --- | --- | --- |
| 1 | Pane title | `inspector_title(name, k)` (15 semibold) | `text_base` |
| 2 | Section title | `section("Agents", trailing, k)` (13 semibold) | `text_base` |
| 3 | Value, row name, body | `k.ty.ui` (13); row names `k.ty.ui_medium`; code `k.ty.mono` (12, SF Mono) | `text_base` |
| 4 | Field label, status word, tag text | `k.ty.small` or `k.ty.caption()` (11) | `text_muted` |
| 5 | Path, hint, "missing", explanation under a value | `k.ty.caption()` (11/16) | `text_faint` |

- A label is always quieter than its value: label level 4, value level 3. The old
  "faint label over muted value" made every line look the same; that is the mess this ladder
  replaced.
- Use `caption()` (11 on a 16 line) for anything that can wrap to two lines; `small` is for one
  line next to a control.
- Never use `k.ty.body`, `body_medium`, `text_small` or `text_xsmall`: they are rune-kit's
  older scale and do not line up with the ladder.
- SKILL.md goes through `skill_md(md, k)`: body at 13, headings at 13 semibold at most, so a
  file's `# Title` never outranks the pane's own title.

### Monospace

Commands, args, URLs, paths, env and header keys, config field names (`headers.X-Key`) are
monospace. Everything a person would type is mono; everything Prism says is Inter.

```rust
// Correct: a command that wraps between words, in a box
prop("Command", code_box(words(&line, k), k), k)

// Incorrect: mono() breaks anywhere, so "--stdio" splits after "--", and its muted colour
// makes the value read as a label
prop("Command", mono(line, k), k)
```

`words()` keeps each word whole. Use it for commands and args. Use `mono()` for a single path or
a log, where breaking anywhere is fine.

**One sentence is one `text()`.** Never assemble a sentence from several text elements to set
one word in mono or bold: the pieces wrap separately and break mid-sentence. Put the key on its
own line in mono and the sentence under it, as each Differences box does (`diffs()` in
`src/ui/mcp.rs`), or write the key in the sentence in the sentence's font.

`.wrap()` and `.wrap_anywhere()` act on text only; on a `row()` they do nothing. A row of
elements that should flow onto more lines uses `.flex_wrap()`, as `words()` does.

## Colours

Text: `text_base` > `text_muted` > `text_faint`, as in the ladder. Nothing else colours text,
except the `Danger` button, which colours its own label.

Status colours are for dots and icons only (`.icon` of each):

```
What does the dot say?
 ├── Linked, in sync, done               → k.c.success.icon
 ├── The agent's own copy, or "info"     → k.c.info.icon
 ├── Needs a look: differs, half linked,
 │   switched off here                   → k.c.warning.icon
 ├── Broken link, failure                → k.c.danger.icon
 └── Neither good nor bad (read-only)    → k.c.text_faint
```

`placement_color()` in `src/ui/installed.rs` and `status()` in `src/ui/mcp.rs` are the two
mappings; extend them, never pick a status colour inline.

## Surfaces and lines

| Need | Token |
| --- | --- |
| Selected row | `.fill(k.c.layer_02)` |
| Checked (multi-select) row | `.fill(k.c.layer_01)` plus `.hover(k.c.hover)` |
| Hover on a pressable row | `.hover(k.c.hover)` |
| Code block | `code_box()`: `layer_01` fill, `border_muted` 1pt, `radius.md` |
| Grouping box (one diff field) | `border_muted` 1pt, `radius.md`, no fill |
| Tag outline | `border_weak` 1pt, `radius.sm` (use `tag()`) |
| Between sections | `Divider::horizontal().build(k)` |
| Muted icon | `k.c.icon_muted` |

Radii: `k.radius.md` for rows and boxes, `k.radius.sm` for tags. No others.

Never put a coloured fill behind text (a red banner, a green row). The one bordered notice is the
update banner in `src/ui/inspector.rs`: `border_weak`, an `info` dot and a button.
