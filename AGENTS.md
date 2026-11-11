# Agent notes for Prism

- Rust desktop app on rune and rune-kit, plus the `prism` CLI.
- **Before any UI change, read [DESIGN.md](DESIGN.md).** Screens in `src/ui/` are built only
  from the design system in `src/ds/`: its blocks, roles and `space::` / `radius::` / `size::`
  tokens. A value or block it lacks goes into `src/ds/` (and DESIGN.md's "Prism additions"),
  never into a screen.
- Checks before calling a change done:
  - `cargo test` (includes the lint `ui_lint::no_raw_values_in_ui`, which fails on raw values
    in `src/ui/`);
  - `cargo clippy --all-targets -- -D warnings`;
  - for a UI change, `scripts/snap-scenes.sh <out-dir>` and a look at the scenes it touches in
    both themes (`<scene>-light.png`, `<scene>-dark.png`). One scene:
    `target/debug/prism --snap /tmp/p.png --scene <scene> --theme light|dark`.
- `scripts/demo-home.sh` regenerates the README screenshots in `docs/` when a change shows in
  them.
