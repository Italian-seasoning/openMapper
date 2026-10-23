# Contributing to openMapper

Thank you for helping. A few rules keep the project easy to work on.

## Issues

* Write in English; one problem per issue; use the latest release.
* For a bug say what you did, what you expected and what happened, and attach the map's `global.cfg`
  path and the end of the log if the program stopped. A screenshot helps.
* Security problems are reported privately, see [SECURITY.md](SECURITY.md).

## Pull requests

* One change per pull request, based on the latest `main`.
* `cargo build --release` and `cargo test --release` pass (CI runs them).
* Keep large files out of the repository: link screenshots and videos in the pull request and keep
  test data small.
* Write code that reads like the code around it: its naming, its comment density, its idiom. The
  interface texts come in English and Russian (`tr("English", "Русский")`).
* New code goes into new files where it can: shared files (`src/ui.rs`, `src/editor.rs`,
  `src/app.rs`, `src/main.rs`) only get small hooks. See [docs/ROADMAP.md](docs/ROADMAP.md).
* Commit messages say what changed for the user, in plain words.

## Building

```bash
cargo run --release
```

Linux needs `libgtk-3-dev`, `libxkbcommon-dev` and `libwayland-dev`. The logo and icons are made by
`python3 tools/make-icons.py` (Pillow, fontTools, skia-pathops).
