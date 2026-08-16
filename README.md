# bashcap

A transparent bash wrapper: write the full state of every shell at every
`BASHCAP` call site — variables, the call stack with arguments, `BASH_REMATCH`,
notes — as one JSON object per line, then render it back with `bashcap show`.

```
bashcap run   [--reach bash-env|by-hand] --into capture.jsonl [--trace-calls] -- bash build.bash
bashcap serve --at session.d --into capture.jsonl      # started BY a script, via BC_START
bashcap show  capture.jsonl
```

`make demo` (or `dev-bin/bashcap-demo.bash`) walks the whole story. Built on
[`bash-interop`](../bash-interop); the words a client vendors are
`assets/bashcap.bash`, and the reference is `KB/bashcap.md`.
