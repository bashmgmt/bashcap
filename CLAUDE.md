# bashcap — working in this crate

A thin user of `bash-interop`: the instrument (bash injected into every
shell), the rig impl, the reading, and the `bashcap` binary. Reference: `docs/bashcap.md`.

```bash
cargo test --lib -- --test-threads=1
cargo test --test cli -- --test-threads=1
cargo clippy --all-targets -- -D warnings   # silent, and stays silent
```

Style follows the parent workspace's CLAUDE.md.
