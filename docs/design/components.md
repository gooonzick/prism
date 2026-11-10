# Components

All controls are rune-kit's. Build them with `.build(k)` and give every interactive one a
stable `.key("…")`: tests and snapshots find elements by key.

## Button

Use for an action with a text label. An action that fits an icon alone (rescan, close, collapse)
is an `IconButton`. Navigation between pages is the page tabs, not buttons.

Variants Prism uses: `Contrast`, `Ghost`, `Outline`, `Danger`, `Loading`. rune-kit also has
`Neutral`, `Warning`, `Submit`, `GhostMuted` and `GhostFaint`; Prism never uses them, and
adding one is a bug.

```
Is it the next step of this view (Sync, Install, Create skill, a dialog's confirm)?
 ├── Yes → Contrast. Only one per view; while its job runs, swap it for Loading.
 └── No
      ├── Does it delete, remove or overwrite? → Danger
      ├── Is it in a toolbar or a dialog footer (Add server, Cancel, Close)? → Ghost
      └── Is it a row of actions in an inspector (Copy to…, Edit…, Reveal)? → Outline
```

- A `Contrast` button that isn't the next step right now drops to `Ghost`, as Sync does when
  nothing is unsynced or when the bulk bar owns the next step (`src/ui/installed.rs`).
- In an inspector the actions sit in one row: `Outline` buttons on the left, `Remove…`
  (`Danger`) alone on the right, with a growing spacer between. Destructive never sits next to
  safe.
- Sizes: `ControlSize::Small` in inspectors, inline in rows and in banners; `Regular` in the
  toolbar and for the main action of the skills.sh inspector; `Large` only for the sidebar
  toggle. Dialog buttons keep the default.

```rust
// Correct: inspector actions
let small = |label: &str| Button::new(label).variant(ButtonVariant::Outline).control_size(ControlSize::Small);

// Incorrect: a second dark button beside the toolbar's Sync
Button::new("Edit…").variant(ButtonVariant::Contrast)
```

## IconButton

Toolbar and row icons: `IconButtonVariant::GhostMuted`, always with `.label(…)` (read by
screen readers) and `.help(…)` (the tooltip), and `.shortcut(…)` when one exists. No other
`IconButtonVariant` is used.

## Choosing a toggle

```
What is the user choosing?
 ├── On or off for one thing, applied at once (link a skill into an agent) → Switch
 ├── Several items from a list, applied by a later button               → Checkbox
 ├── Exactly one of a few options with a description each               → RadioItem
 ├── One of many options in a toolbar or form field                     → Select
 └── A filter or a page of the same data (All / Differs / Read-only)     → SegmentedControl
```

- `Switch`: in light theme call `.standard()`; the compact switch disappears on a light pane
  when off (see `placement_row` in `src/ui/inspector.rs`).
- Segment labels carry their count: "Differs 1". A count of 0 stays visible.

## Tags, dots and badges

- `tag(label, color, help, k)` (`src/ui/installed.rs`): an agent chip or a state chip in a list
  row. Pass a status colour to get a dot inside it; always pass `help` saying what it means.
- `dot(color)`: a 7pt status dot, always followed by a word (`in sync`, `differs`) or inside a
  tag. See the dot colours in `type-and-color.md`.
- `Badge` with `BadgeVariant::Subtle`: only "installed" on skills.sh results. A badge says what
  something is; a tag says where it lives or what state it is in. Don't add new badges for state.

## Text and code

- `TextInput` for every text field; `Select` for a fixed list. Field labels sit above the input.
- Show a value a person typed or would type with `code_box(words(…))` or `mono(…)` (see
  `type-and-color.md`). Values are `.selectable(true)`; labels are not.
- `Spinner` next to a muted line of text while something loads ("Reading the repository…").
  Never a spinner alone.
