# bashcap — the reference tool

A transparent bash wrapper that writes the full state of a running shell at
every `BASHCAP` call site. It is the reference consumer of the rig, and it is
one subject per file:

| | | |
|---|---|---|
| the instrument | `bashcap/instrument.rs`, `assets/bashcap.bash`, `bashcap/effect.bash`, `trace.bash` | the words, their effect, and the one function that composes them |
| the record | `bashcap/snapshot.rs` | what a shell sends back, and the decoder that reads one off the wire |
| the rendering | `bashcap/show.rs` | reading a written capture back, and the one `Display` of one |
| the tool | `bashcap/mod.rs` | a rig whose session is a sink, and the JSON line format it owns |
| the program | `src/bin/bashcap.rs` | `clap` and `main` |

`instrument` and `Capture::of` are the pair another tool reuses: the bash that
produces a snapshot, and the code that reads one back.
`tests/examples/snapshotting.rs` is that reuse — bashcap expressed in the
core, with typed captures for a session, `instrument(Tracing::Calls)` for the
full stack, and no command line in between.

```
bashcap run --into FILE [--verbose] [--trace-calls] [--] <command…>
bashcap show FILE
```

`show` renders a capture through `Capture`'s `Display`, which is the same
text a library caller gets from `println!("{capture}")`. One rendering, in
one place.

## The client's side

A script opts in by calling `BASHCAP`:

```bash
BASHCAP [-BCV:<var>]… [-BCS:<note>]…
WITH_BASHCAP [-BCV:<var>]… [-BCS:<note>]… <command> [args…]
```

`-BCV:` captures a variable by name, `-BCS:` attaches a note. Every variable
named `BASHCAP__CTX__*` is captured automatically, which is how ambient
context rides along without being named at each site. `WITH_BASHCAP` is the
CPS form: it snapshots, runs the continuation, and returns *its* status.

Keeping those call sites runnable without the tool is the client's own, and
`assets/bashcap.bash` is what it vendors to do it — see
[vendoring.md](vendoring.md).

## The instrument

`effect.bash`, whose snapshot ends:

```bash
    local -a __bc_walk=()
    __bc_stack __bc_walk 2
    …
    BC_INSTR say __BASHCAP__ \
        "${__bc_walk[@]}" \
        state   "(${__bc_state[*]@Q})" \
        rematch "(${__bc_rematch[*]@Q})" \
        vars    "(${__bc_declared[*]@Q})" \
        notes   "(${__bc_notes[*]@Q})"
```

The frame walk is not bashcap's: `__bc_stack` is shared with every tool that
reports a stack, and contributes six sections of its own — see
[stack.md](stack.md). Each section here is an array literal, read back with
`parse_array`.

Two details in the bash worth knowing:

```bash
local IFS=' '
```

Function-scoped, because the sections above are joined with `[*]` and a client
that had set `IFS` would otherwise collapse each one into a single word. The
envelope is safe without it — `__bc_send` uses `printf` — but these nested
joins are bashcap's own.

```bash
declare -n __bc_ref="$__bc_name"
__bc_declared+=("${__bc_ref[*]@A}")
unset -n __bc_ref
```

`${ref[*]@A}` through a nameref yields a complete self-describing declaration
for every type — `declare -i n='7'`, `declare -A m=([k]="v")` — attributes
included. Indirect `${!name@A}` cannot be used here: it collapses an array to
its first element.

## The decoder

```rust
/// Whether the subject's shells record what each call was passed.
pub enum Tracing { Off, Calls }

/// The bash a rig hands the subject, for any rig that wants what bashcap
/// harvests. One way to compose it; `BASH` and `TRACE` are not public.
pub fn instrument(tracing: Tracing) -> String;

pub struct Snapshot {
    pub stack: Stack,
    pub state: IndexMap<String, String>,
    pub rematch: Vec<String>,
    pub vars: IndexMap<String, Captured>,
    pub notes: Vec<String>,
}

pub struct Frame {
    pub funcname: String,
    pub source: String,
    pub lineno: u32,

    /// The call's arguments, when the shell was recording them. `None` is
    /// "not recorded", never "called with none".
    pub args: Option<Vec<String>>,
}

pub struct Captured { pub attrs: String, pub value: Value }
pub enum Value { Scalar(String), Indexed(IndexMap<usize, String>), Assoc(IndexMap<String, String>) }

/// One snapshot under the provenance the wire gave it — the output format,
/// one per line, and what `show` reads back.
pub struct Capture {
    pub sent_at: u64, pub heard_at: u64, pub pid: u32, pub seq: u32,
    pub snapshot: Snapshot,
}

impl Capture {
    /// `None` for a message that is not one of ours; `Some(Err)` for one that
    /// is and will not decode.
    pub fn of(line: &Line) -> Option<Result<Self, Failure>>;
}

/// Every capture in a file `BashCap` wrote: one JSON object per line. The one
/// way to read one back, used by `bashcap show` and by the tests alike.
pub fn captures(text: &str) -> Result<Vec<Capture>, Failure>;
```

The word a snapshot message begins with is `__BASHCAP__`, and `Capture::of` is
the only thing that reads it — which is what lets several tools share one wire
while a decode failure stays visible.

## Call arguments

Bash records them only under `extdebug`, and **bashcap never turns it on**.
Three reasons, each sufficient:

- From `BASH_ENV` — the only injection point there is — `shopt -s extdebug`
  means *start the debugger*. Bash warns on the subject's stderr, disables
  debugging mode, and records nothing; where `bashdb` is installed it would
  attach a debugger to the subject.
- It implies `errtrace` and `functrace`, so a subject's own `ERR` and `DEBUG`
  traps become inherited by subshells and functions. That is a change in the
  subject's behaviour.
- Turning it on part-way leaves `BASH_ARGC` shorter than `FUNCNAME`, so the
  arguments that are there belong to the wrong frames — which the reader
  detects and carries as absent, but which is nothing to rely on.

`--trace-calls` (or `BashCap::tracing_calls()`) asks for them anyway. It does
**not** put anything on the command line — argv reaches only the top-level
shell — but injects `trace.bash`, which arms `extdebug` one command past
startup from a `DEBUG` trap that removes itself. Returning zero from that
handler is not optional: under `extdebug` a non-zero `DEBUG` handler makes
bash skip the command it fired for.

A subject that traces itself — `shopt -s extdebug` as its own first statement,
or a `bashdb` session — gets them without the flag.

### The stack math

`effect.bash` does none. It calls `__bc_stack`, which ships bash's five arrays
as they are, and every index — which frames are the instrument's, which line a
frame is executing, where a call's arguments sit in the flat stack and which
way round they are — is undone in Rust. [stack.md](stack.md) is the whole of
it, including why alignment rather than `shopt -q` decides whether a record is
trustworthy.

`__fixtures/bashcap_demo/child.bash` traces itself, so one demo run shows both
paths. About +45 µs on a six-deep stack when traced, against a ~480 µs
snapshot — see [measurements.md](measurements.md#cost-of-a-snapshot).

Recognise, then decode — the shape every decoder in the crate takes. Decoding
mirrors the assembly exactly: `Columns::of` takes the six the frame walk
contributed, `flat` reads the rest with `parse_array`, and
`Declaration::read` parses `declare -aX name=rhs` back into a name, its
attribute letters, and its value.

Note what the snapshot does **not** carry: a timestamp, a pid, or a parent.
Those are provenance: the protocol puts them in front of every message and
every tool gets them on `Line` without asking. See [tree.md](tree.md).

## The tool

```rust
pub struct BashCap   { into: PathBuf }                             // description
pub struct Capturing { pub written: usize, sink: BufWriter<File> } // the session

impl Rig for BashCap {
    type Session = Capturing;

    fn bash(&self) -> String { instrument(Tracing::Off) }
    fn open(&self) -> Result<Capturing, Failure> { /* creates the file */ }
    fn hear(&self, session: &mut Capturing, said: Line) -> Result<(), Failure> { /* writes */ }
    fn end(&self, session: &mut Capturing, _: ExitStatus) -> Result<(), Failure> {
        session.sink.flush().doing(|| self.writing_to())
    }
}
```

`answer` is inherited: bashcap only listens, so a shell that asks it something
is told the word is unknown. `BashCap { into }` is a description and opens
nothing; the session holds the sink and the tally, and `run` hands it back
alongside the status:

```rust
let (capturing, status) = BashCap::writing(into).run(argv)?;
```

The wrapped command carries its own program, so `bashcap run --into out bash
build.bash` is the ordinary form and `bashcap run --into out make test` also
works: every bash `make` starts reads the same `BASH_ENV`.

A failed flush in `end` ends the run rather than being lost in a `Drop`.

The output format belongs to the tool: the core moves arglists and knows
nothing about JSON. bashcap declares its own row, flattening the snapshot
under the provenance it wants:

`Capture` is that row, and `serde(flatten)` puts the provenance beside the
snapshot's own fields rather than above them.

Lines are written in **arrival** order, each carrying the shell's own clock
and the run's, so ordering downstream is exact and is `sort`'s job. Writing in
`hear` keeps resident memory independent of run length — see
[measurements.md](measurements.md#memory).

An indexed array travels as `[index, value]` pairs, not as an object: a bash
indexed array is sparse, so its indices are data, and JSON can only spell an
object key as a string — which `serde(flatten)` then cannot read back as a
number.

## Rendering

`Display` on `Capture`, `Frame`, `Captured` and `Value`, and nothing else
renders. A value prints as the bash that would declare it, because
`bash::value`'s emitters already do exactly that:

```
[3] pid 488092 seq 0 shlvl 7 subshell 0
    at    child_work@child.bash:12 ('a first argument' 'a second')
    at    main@child.bash:14 ()
    note  child process, own pid and SHLVL
    var   payload [a] ([0]='x' [1]='y' [2]='z')
```

Empty parentheses are a call with no arguments; no parentheses at all is a
shell that was not recording them.

## The program

`src/bin/bashcap.rs` is the whole of it: the two subcommands, and a `capture`
that calls `run`. Four properties of a transparent wrapper:

- **`--into` is required**; there is no default output location.
- **stderr belongs to the subject** unless `--verbose` is passed.
- **The first plain word ends bashcap's options.** `bashcap run --into out
  build.bash --into elsewhere` passes `--into elsewhere` to the script. An
  unknown flag before that point is an error, and `--` takes a wrapped command
  that starts with a dash.
- **The exit code is the subject's**, via `ExitStatus::shell_code()`.

## Playground

```sh
make bashcap-demo [SCRIPT=path/to/your.bash]
```

Builds the debug binary, shows the vendored words and checks them against the
asset, runs `__fixtures/bashcap_demo/demo.bash` once with no tool and once
under `bashcap run`, and renders the capture with `bashcap show`. The fixture
exercises every facility in one file — typed variables, ambient context,
`BASH_REMATCH`, nested frames with argv, the CPS form, a subshell and a child
process — and nothing asserts its line numbers, counts or variable names, so
it is meant to be edited.

Typical output is the block above. come from a subshell and a child process: the first reaches the wire by
re-joining under its own `$BASHPID`, the second because the prelude runs there
too, through `BASH_ENV`.

## See also

- [wire.md](wire.md#the-prelude) — how a rig's bash reaches every shell
- [rig.md](rig.md) — the trait it implements
- `tests/examples/snapshotting.rs` — its instrument, reused without its CLI
- `src/bashcap/tests.rs` — its bash-level tests: one run covering every section
