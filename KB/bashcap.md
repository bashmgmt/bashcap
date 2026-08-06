# bashcap — the reference tool

A transparent bash wrapper that writes the full state of a running shell at
every `BASHCAP` call site. It is the reference consumer of the rig, and it is
one subject per file:

| | | |
|---|---|---|
| the instrument | `bashcap/instrument.rs`, `bashcap.bash`, `trace.bash`, `polyfill.bash` | the bash it ships, and the one function that composes what gets injected |
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
bashcap polyfill
```

`show` renders a capture through `Capture`'s `Display`, which is the same
text a library caller gets from `println!("{capture}")`. One rendering, in
one place.

## The client's side

A script opts in by calling `BASHCAP`, and stays runnable without the tool by
sourcing the polyfill:

```bash
if [[ -z "${BASHCAP__IS_RUNNING:-}" ]]; then
    WITH_BASHCAP() { "$@"; }
    BASHCAP() { true; }
fi
```

Under the tool, the real definitions are already in place when the polyfill
runs, so its `if` is false and the stubs are never installed. Outside it, both
names resolve to no-ops, so instrumented call sites are safe to ship.

```bash
BASHCAP [-BCV:<var>]… [-BCS:<note>]…
WITH_BASHCAP [-BCV:<var>]… [-BCS:<note>]… <command> [args…]
```

`-BCV:` captures a variable by name, `-BCS:` attaches a note. Every variable
named `BASHCAP__CTX__*` is captured automatically, which is how ambient
context rides along without being named at each site. `WITH_BASHCAP` is the
CPS form: it snapshots, runs the continuation, and returns *its* status.

## The instrument

`bashcap.bash`, whose snapshot ends:

```bash
    BC_INSTR say __BASHCAP__ \
        frames  "(${__bc_frames[*]@Q})" \
        state   "(${__bc_state[*]@Q})" \
        rematch "(${__bc_rematch[*]@Q})" \
        vars    "(${__bc_declared[*]@Q})" \
        notes   "(${__bc_notes[*]@Q})"
```

Each section is a **nested array literal**, so the message keeps its structure
all the way to Rust; `n_d(2)` decodes `frames`, `n_d(1)` the rest.

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

/// The bash to put in a `Startup`, for any rig that wants what bashcap
/// harvests. One way to compose it; `BASH` and `TRACE` are not public.
pub fn instrument(tracing: Tracing) -> String;

pub const POLYFILL: &str;  // polyfill.bash — a client vendors this

pub struct Snapshot {
    pub frames: Vec<Frame>,
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
  arguments that are there belong to the wrong frames.

`--trace-calls` (or `BashCap::tracing_calls()`) asks for them anyway. It does
**not** put anything on the command line — argv reaches only the top-level
shell — but injects `trace.bash`, which arms `extdebug` one command past
startup from a `DEBUG` trap that removes itself. Returning zero from that
handler is not optional: under `extdebug` a non-zero `DEBUG` handler makes
bash skip the command it fired for.

A subject that traces itself — `shopt -s extdebug` as its own first statement,
or a `bashdb` session — gets them without the flag.

### The stack math

Three steps in `bashcap.bash`, each one idea.

**Is the record trustworthy?** The test is alignment, not `shopt -q`.
`BASH_ARGC[i]` is frame `i`'s width only where the array aligns 1:1 with
`FUNCNAME`; enabling `extdebug` part-way leaves it short, and short means
every width belongs to a different frame. What is not trustworthy is carried
as *absent*, so nothing downstream needs to ask again:

```bash
local -a __bc_argc=()
if (( ${#BASH_ARGC[@]} == ${#FUNCNAME[@]} )); then
    __bc_traced=yes
    __bc_argc=("${BASH_ARGC[@]}")
fi
```

**Where does each frame's group start?** `BASH_ARGV` is one flat stack: frame
0's arguments, then frame 1's, and so on, with each group *reversed* within
itself. Summing the widths ahead of a group gives its offset, which turns
reading a frame into an index rather than a walk — and puts `BASHCAP`'s own
group, at index 0, behind every reported frame by construction rather than by
a correction:

```bash
for (( __bc_i = 0; __bc_i < ${#__bc_argc[@]}; __bc_i++ )); do
    __bc_from[__bc_i]=$__bc_at
    __bc_at=$(( __bc_at + __bc_argc[__bc_i] ))
done
```

**Read the frame.** Counting the width down undoes the reversal, so arguments
come out in the order the call was written. Untraced, `__bc_argc` is empty,
every width reads 0, and the loop does not run — no branch needed.

```bash
for (( __bc_j = __bc_argc[__bc_i]; __bc_j > 0; __bc_j-- )); do
    __bc_frame+=("${BASH_ARGV[__bc_from[__bc_i] + __bc_j - 1]}")
done
```

**The cursor moves by assignment, not by `(( += ))`.** A bash arithmetic
*command* reports success only for a non-zero value, so `(( at += n ))` returns
1 whenever the running total is still 0 — which under the subject's own
`set -e` ends the script part-way through a snapshot. Reaching zero needs
nothing unusual: `BASHCAP` with no flags, and a frame called with no
arguments. `$(( ))` inside an assignment has no such status.
`tests.rs::the_walk_survives_the_subjects_own_shell_options` pins it.

`__fixtures/bashcap_demo/child.bash` traces itself, so one demo run shows both
paths. Costs nothing when untraced; about +70 µs on a three-deep stack when
traced, against a ~611 µs snapshot.

Recognise, then decode — the shape every decoder in the crate takes. Decoding
mirrors the assembly exactly: `flat` reads a section with `QuotedNest::words`,
`nested` with `rows`, and `Declaration::read` parses `declare -aX name=rhs`
back into a name, its attribute letters, and its value.

Note what the snapshot does **not** carry: a timestamp, a pid, or a parent.
Those are provenance: the protocol puts them in front of every message and
every tool gets them on `Line` without asking. See [tree.md](tree.md).

## The tool

```rust
pub struct BashCap   { into: PathBuf }                             // description
pub struct Capturing { pub written: usize, sink: BufWriter<File> } // the session

impl Rig for BashCap {
    type Session = Capturing;

    fn startup(&self) -> Startup { Startup { bash: BASH.into(), ..Default::default() } }
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
let (capturing, status) = run(&BashCap::writing(into), argv)?;
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

Builds the debug binary, emits the polyfill, runs
`__fixtures/bashcap_demo/demo.bash` and renders it with `bashcap show`. The fixture
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
