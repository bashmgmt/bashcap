# bashcap — the reference tool

`src/utilprog/bashcap/`, with its bash in `bash/bashcap/`

A transparent bash wrapper that records the full state of a running shell at
every `BASHCAP` call site. It is the reference consumer of the rig: one bash
file, one decoder, and a command line that is one call.

```
bashcap run --into FILE [--] <bash args…>   capture into FILE, one snapshot per line
bashcap polyfill                             the client-side no-op stubs
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
names cost nothing — which is what makes it safe to leave calls in production
code.

```bash
BASHCAP [-BCV:<var>]… [-BCS:<note>]…
WITH_BASHCAP [-BCV:<var>]… [-BCS:<note>]… <command> [args…]
```

`-BCV:` captures a variable by name, `-BCS:` attaches a note. Every variable
named `BASHCAP__CTX__*` is captured automatically, which is how ambient context
rides along without being named at each site. `WITH_BASHCAP` is the CPS form:
it snapshots, runs the continuation, and returns *its* status.

## The rig

```rust
pub struct BashCap;

impl Rig for BashCap {
    fn setup(&self) -> Setup {
        Setup::new().bash(SRC.read().unwrap_or_else(|error| panic!("{error}")))
    }

    /// bashcap only listens. A script that asks it something gets a status
    /// saying nobody was there to answer.
    fn answer(&mut self, _turn: &Turn) -> Reply {
        Reply::status(127)
    }
}
```

That is all the Rust there is on the bash side. Everything bashcap *does* is in
`bash/bashcap/bashcap.bash`, including `WITH_BASHCAP`, which is a bash idiom
and belongs in bash. The snapshot ends:

```bash
declare -a __bc_rec=(
    __BASHCAP__
    frames  "(${__bc_frames[*]@Q})"
    state   "(${__bc_state[*]@Q})"
    rematch "(${__bc_rematch[*]@Q})"
    vars    "(${__bc_declared[*]@Q})"
    notes   "(${__bc_notes[*]@Q})"
)
BC_INSTR say "${__bc_rec[@]}"
```

Each section is a **nested array literal**, so the message keeps its structure
all the way to Rust instead of being flattened behind sentinels. The last line
is the whole of how it reaches the wire.

Two details in the bash worth knowing:

```bash
declare -- IFS=' '
```

Function-scoped, because the sections above are joined with `[*]` and a client
that had set `IFS` would otherwise collapse each one into a single word. The
envelope is safe without this — `__bc_pack` uses `printf` — but these nested
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
```

```rust
impl FromRecord for Snapshot {
    type Err = SnapshotError;
    fn from_record(record: &Record) -> Option<Result<Self, Self::Err>> {
        Some(Self::decode(record.behind(TAG)?))
    }
}
```

Recognise, then decode — the shape every `FromRecord` takes. Decoding mirrors
the assembly exactly: `flat` reads a section at `Schema::n_d(1)`, `nested` at
`n_d(2)`, and `split_declaration` parses `declare -aX name=rhs` back into a
name, its attribute letters, and its value.

Note what the snapshot does **not** carry: a timestamp, a pid, or a parent.
Those are provenance, they belong to `Stamp` and `Origin`, and every tool gets
them without asking. See [capture.md](capture.md).

## The command line

```rust
let mut bashcap = BashCap::writing(File::create(into)?);
let status = bashcap.run(argv)?;
```

`BashCap` decodes and writes in `heard`, so a snapshot reaches the file as it
arrives rather than when the run ends. Resident memory does not track the
capture:

| snapshots | peak RSS | output |
|---:|---:|---:|
| 200 | 7.7 MB | 0.19 MB |
| 2 000 | 7.8 MB | 1.9 MB |
| 20 000 | 7.5 MB | 18.9 MB |

The core carries no serialisation, so bashcap declares its own output row —
the wrapper owns its format, provenance included:

```rust
#[derive(Serialize)]
struct Row<'a> {
    at: u64,
    pid: u32,
    seq: u32,
    #[serde(flatten)]
    snapshot: &'a Snapshot,
}
```

Lines are written in **arrival** order rather than sorted by stamp. Each
carries its own `at`, so ordering downstream is exact and is `sort`'s job.

Three rules, all the same rule:

- **`--into` is required.** A wrapper must not guess where to write.
- **Nothing goes to stderr** unless `--verbose` is passed. stderr belongs to
  the subject.
- **The first plain word ends bashcap's options.** `bashcap run --into out
  build.bash --into elsewhere` passes `--into elsewhere` to the script, where
  it belongs. A flag bashcap does not know is an error rather than a guess, and
  `--` takes a wrapped command that starts with a dash.

The exit code is the subject's, via `ExitStatus::code()`.

## Playground

```sh
make bashcap-demo [SCRIPT=path/to/your.bash]
```

Builds the debug binary, emits the polyfill, runs
`__fixtures/bashcap_demo/demo.bash` and renders what it captured. The fixture
exercises every facility in one file — typed variables, ambient context,
`BASH_REMATCH`, nested frames with argv, the CPS form, a subshell and a child
process — and no test asserts its line numbers, so it is meant to be edited.

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

Entries 2 and 3 are the parts that were impossible before: a subshell reaches
the wire because it re-joins by name rather than inheriting anything, and a
child process because the prelude re-runs there via `BASH_ENV`.

## See also

- [source.md](source.md) — where a tool's bash lives
- [run.md](run.md#a-rig-is-three-functions) — the trait it implements
- `src/utilprog/bashcap/tests.rs` — one run covering every section
