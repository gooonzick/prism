# Prism design rules

Prism is a quiet macOS utility: a sidebar, a list, and an inspector, drawn with rune-kit. Most
of the screen is grey text on a light or dark surface; colour appears only as small status dots
and the one dark (`Contrast`) button that is the next step. Hierarchy comes from weight, size and
dividers, never from colour or boxes around everything.

Read the topic file before touching that part of the UI. Every name below exists in the code;
anything not listed here does not exist in this app and is a bug if you add it.

## The rules that apply everywhere

1. **Tokens only.** Colours come from `k.c.*`, type from `k.ty.*` or the helpers in
   `src/ui/mod.rs`, radii from `k.radius.*`. Never `Color::rgb`, never a hex value, never a new
   `TextStyle::new` outside `src/ui/mod.rs`. A raw value breaks the dark theme the first time.
2. **One `Contrast` button per view.** It is the next step. Everything else is `Ghost`,
   `Outline` or `Danger`. Two dark buttons means the screen has no next step.
3. **Status is a 7pt dot plus a word**, never colour alone and never a coloured background:
   `dot(k.c.success.icon)` + "in sync". Colour-only status is invisible to colour-blind users.
4. **Text has five levels and no more** (see `docs/design/type-and-color.md`). A sixth size or
   weight is a bug.
5. **Secrets never render unmasked by default.** Values go through `masked()`, `masked_args()`
   or `masked_url()` until the user presses Show values.
6. **Both themes.** Check every change with `--theme light` and `--theme dark` snapshots
   (see "Checking a change" below) before calling it done.

## Topic files

| File | Read it when you… |
| --- | --- |
| [docs/design/type-and-color.md](docs/design/type-and-color.md) | pick a font, a text colour, a status colour, a border or a fill |
| [docs/design/components.md](docs/design/components.md) | add a button, toggle, checkbox, tag, badge, input or segmented control |
| [docs/design/layout.md](docs/design/layout.md) | build a list row, an inspector, a dialog, an empty state or a toast |
| [docs/design/copy.md](docs/design/copy.md) | write a label, a tooltip, a note, an error or a confirmation |

## Where things live

- `src/ui/mod.rs`: the shared helpers: `inspector_title`, `section`, `prop`, `code_box`,
  `words`, `mono`, `skill_md`, `heading`, `dot`, `empty`, `inspector_body`, `row_gap`.
  Use these before writing a new element; add a new helper here, not in a page file, when two
  pages need it.
- `src/ui/installed.rs`: `tag()` and `placement_color()`.
- Page files (`installed.rs`, `browse.rs`, `mcp.rs`) and dialog files (`dialogs.rs`,
  `mcp_dialogs.rs`) compose the helpers; they do not define new styles.

## Checking a change

Snapshots render the real window headless at 2x:

```bash
cargo build && target/debug/prism --snap /tmp/p.png --size 1280x900 --scene mcp --theme dark
```

Scenes: `detail`, `sync`, `remove`, `bulk`, `bulk-remove`, `browse`, `notice`, `updates`,
`new-skill`, `mcp`, `mcp-add` (see `SCENES` in `src/main.rs`).
`scripts/demo-home.sh` regenerates the README screenshots from a throwaway home; run it when a
change is visible in them.
