# bashcap

Record what a bash shell held at a given point, and read it back later.

Mark the point in the script:

```bash
greet() {
    declare who=$1
    declare greeting="hello $who"
    BASHCAP -BCV:who -BCV:greeting -BCS:before-the-echo
    echo "$greeting"
}
```

Run it under capture, then render what came out:

```
bashcap run --into capture.jsonl -- bash greet.bash
bashcap show capture.jsonl
```

Each capture is one JSON object per line, holding the shell that sent it and
what that shell saw:

```json
{"stack": [{"site": {"function": "greet"}, "source": {"file": "greet.bash"}, "lineno": 4},
           {"site": "script", "source": {"file": "greet.bash"}, "lineno": 7}],
 "vars": {"who":      {"attrs": "", "value": {"scalar": "world"}},
          "greeting": {"attrs": "", "value": {"scalar": "hello world"}}},
 "notes": ["before-the-echo"]}
```

Values keep their shape. An array comes back an array, an associative array
comes back with its keys, and `attrs` carries what `declare -p` would have
printed. Each subshell is recorded under its own pid and nesting depth, so
state from a subshell stays separate from its parent's.

`-BCV:` names a variable to capture, `-BCS:` leaves a note. `WITH_BASHCAP`
takes the same flags and then runs a command, for wrapping a call rather than
marking a point. `--trace-calls` records the shell's own function calls
alongside the marked ones.

A call site makes bashcap a dependency of the script that says it. Outside a
session the word is a command that does not exist, loudly, rather than a
capture that silently goes nowhere. A script that must also run without the
tool defines the word itself, in one line:

```bash
declare -F BASHCAP >/dev/null || BASHCAP() { :; }
```

## Reaching a session

`run` provisions `BASH_ENV`, and every non-interactive bash in the tree joins
as it starts. Under `--reach by-hand` the provisioned file only defines the
words, and a script joins where it says `BASHCAP_INIT "$BASHCAP_SESSION"`;
this is the one to use when the program starts shells whose startup you do
not control.

A script can also start the tool itself as a coprocess and keep the capture.
`bashcap serve --help` prints that recipe in full.

`make demo`, or `dev-bin/bashcap-demo.bash`, runs the whole thing over a small
fixture.

Built on [bash-interop](https://github.com/bashmgmt/bash-interop). Reference:
[`docs/`](docs/README.md).

Licensed under the MIT licence.
