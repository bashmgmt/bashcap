# bashcap — the reference tool

A transparent bash wrapper that writes the full state of a running shell at
every `BASHCAP` call site. It is the reference consumer of the rig, and it is
three concerns in three places:

| | | |
|---|---|---|
| the instrument | `bashcap/snapshot.rs`, `bashcap.bash`, `polyfill.bash` | the bash that harvests a shell's stack, variables and regex state, and the decoder that reads one back |
| the tool | `bashcap/mod.rs` | a rig whose session is a sink, and the JSON line format it owns |
| the program | `src/bin/bashcap.rs` | `clap` and `main` |

The instrument and its decoder are one subject: they are what another tool
reuses, and `BashCap::bash()` returning `snapshot::BASH` is where the pairing
is stated. `tests/examples/snapshotting.rs` is that reuse — bashcap expressed
in the core, with typed snapshots for a session and no command line in
between.

```
bashcap run --into FILE [--verbose] [--] <bash args…>
bashcap polyfill
```

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
pub const BASH: &str;      // bashcap.bash
pub const POLYFILL: &str;  // polyfill.bash
pub const TAG: &str = "__BASHCAP__";

pub struct Snapshot {
    pub frames: Vec<Frame>,
    pub state: IndexMap<String, String>,
    pub rematch: Vec<String>,
    pub vars: IndexMap<String, Captured>,
    pub notes: Vec<String>,
}

pub struct Frame { pub funcname: String, pub source: String, pub lineno: u32 }
pub struct Captured { pub attrs: String, pub value: Value }
pub enum Value { Scalar(String), Indexed(IndexMap<usize, String>), Assoc(IndexMap<String, String>) }

impl Snapshot {
    /// `None` for a message that is not one of ours.
    pub fn of(line: &Line) -> Option<Result<Self, SnapshotError>>;
}
```

Recognise, then decode — the shape every decoder in the crate takes. Decoding
mirrors the assembly exactly: `flat` reads a section with `QuotedNest::words`,
`nested` with `rows`, and `split_declaration` parses `declare -aX name=rhs`
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

    fn bash(&self) -> String { BASH.to_string() }
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

A failed flush in `end` ends the run rather than being lost in a `Drop`.

The output format belongs to the tool: the core moves arglists and knows
nothing about JSON. bashcap declares its own row, flattening the snapshot
under the provenance it wants:

```rust
#[derive(Serialize)]
struct Row<'a> {
    sent_at: u64,
    heard_at: u64,
    pid: u32,
    seq: u32,
    #[serde(flatten)]
    snapshot: &'a Snapshot,
}
```

Lines are written in **arrival** order, each carrying the shell's own clock
and the run's, so ordering downstream is exact and is `sort`'s job. Writing in
`hear` keeps resident memory independent of run length — see
[measurements.md](measurements.md#memory).

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
`__fixtures/bashcap_demo/demo.bash` and renders what it captured. The fixture
exercises every facility in one file — typed variables, ambient context,
`BASH_REMATCH`, nested frames with argv, the CPS form, a subshell and a child
process — and nothing asserts its line numbers, counts or variable names, so
it is meant to be edited.

Typical output:

```
4 snapshots from 3 shells

[0] pid=8737 subshell=0 shlvl=6
    inner@demo.bash:19 ← outer@demo.bash:17 ← main@demo.bash:22
    note  two frames deep, four typed variables, one missing
    var   greeting [--] scalar  = hello world
    var   items    [a]  indexed = {'0': 'alpha', '1': 'beta gamma'}
    var   conf     [A]  assoc   = {'port': '8080', 'host': 'localhost'}
    var   attempts [i]  scalar  = 3
    regex build-2026-08 | build | 2026 | 08

[2] pid=8739 subshell=1 shlvl=6      from inside a subshell
[3] pid=8741 subshell=0 shlvl=7      child process, own pid and SHLVL
```

Entries 2 and 3 come from a subshell and a child process: the first reaches
the wire by re-joining under its own `$BASHPID`, the second because the
prelude runs there too, through `BASH_ENV`.

## See also

- [wire.md](wire.md#the-prelude) — how a rig's bash reaches every shell
- [rig.md](rig.md) — the trait it implements
- `tests/examples/snapshotting.rs` — its instrument, reused without its CLI
- `src/bashcap/tests.rs` — one run covering every section
