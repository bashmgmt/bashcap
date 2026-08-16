# bashcap — working in this crate

A thin user of `bash-interop`: the instrument (bash injected into every
shell), the rig impl, the reading, and the `bashcap` binary. The words a client
vendors are `assets/bashcap.bash`; `__fixtures/vendor/joining.bash` is the
vendored client half, asserted same-bytes against `rig::JOINING_BASH` in
`tests/cli.rs`. Reference: `KB/bashcap.md`.

```bash
cargo test --lib -- --test-threads=1
cargo test --test cli -- --test-threads=1
cargo clippy --all-targets -- -D warnings   # silent, and stays silent
```

Style follows the parent workspace's CLAUDE.md.
