# Agent notes for Prism

- Rust desktop app on rune and rune-kit, plus the `prism` CLI. `cargo test` and
  `cargo clippy --all-targets` must stay clean.
- **Before any UI change, read [DESIGN.md](DESIGN.md)** and the topic file it points to. It
  lists the only components, variants, type levels and colours this app uses; anything not in
  it is a bug.
- Check UI changes with a snapshot in both themes:
  `target/debug/prism --snap /tmp/p.png --scene <scene> --theme light|dark`.
