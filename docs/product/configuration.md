# Configuration Reference

Everything lives in `<repo>/.weave/config.toml`, written by `weave init`.
Every section besides `mode` is optional — an absent section means that
capability is off, never a pending setup step. Feature-gated sections are
read (and matter) only when the corresponding Cargo feature was compiled
in.

```toml
mode = "single"                     # single | multiple

[storage]
home = ""                           # optional central/custom storage path (default: <repo>/.weave/)
relocate_on_network_fs = false      # opt-in; default is warn-and-refuse

[index]
bailout_ratio = 0.10                # full rebuild above this share of changed files
bailout_floor = 100                 # ...but never below this absolute count

[federation]                        # requires feature = federation
linked_repos = []                   # e.g. ["../sibling-repo"]
staleness_policy = "warn"           # warn | strict | ignore

[hub]                               # requires feature = hub; expected to be rare
url = ""                            # unset is a fully supported permanent state
snapshot_retention = 20             # default when unset; sent as a hint header — pruning itself is hub-server behavior

[slm]                               # requires feature = slm
model = "qwen2.5-coder-0.5b-q4_k_m" # never auto-upgraded; weights not bundled
lazy_load = true                    # must stay true: 0MB idle cost until first `weave ask`

[watch]                             # requires feature = watch
enabled = false                     # must be the literal string "true" to activate
debounce_ms = 2000                  # clamped to [100, 60000]
blast_radius_ceiling = 200          # above this, defer to manual `weave index` instead of auto-reindexing

[report]                            # --html/--open need feature = viz
format = "canvas"                   # "html"/"all" also render HTML; anything else (incl. the "canvas" default) doesn't — markdown+canvas are always written regardless
auto_open = false                   # launch the system browser after `weave report --html`

[viz]                               # requires feature = viz
mode = "static"                     # static (file://) | server (loopback-only HTTP)
```

## `[storage]` — central knowledge store

To store graph data outside the repository tree (a central folder or
multi-repo knowledge vault), either set `[storage] home` per repo or export
a global default:

```bash
export WEAVE_HOME=/path/to/central/knowledge
```

When `WEAVE_HOME` is set and no repo-specific `[storage] home` is set,
`weave` isolates each repository's database under a sanitized namespace:
`$WEAVE_HOME/<sanitized-repo-path>/`. Indexing only ever touches the target
repository's own database and swap files — other repositories under the
same central folder are unaffected. With neither set, storage defaults to
`<repo_root>/.weave/`.

`relocate_on_network_fs` governs what happens if that storage path resolves
onto a network filesystem: the default is to warn and refuse (SQLite's WAL
mode needs shared memory a network mount doesn't reliably provide) rather
than silently risk corruption.

## `[index]`

Controls when `weave index --incremental` gives up and falls back to a
full rebuild.

`weave index --incremental` reuses the existing index and reindexes only
changed files, but falls back to a full rebuild once the changed-file
share crosses `bailout_ratio` — and never below the absolute `bailout_floor`
count, so a tiny repo with a big fractional change doesn't trigger an
unnecessary full rebuild.

## `[federation]`

Requires feature `federation`. `linked_repos` is an array of relative paths to other locally-indexed
repos; `weave link <a> <b>` records contract expectations but does not yet
write this array back for you (edit it once, by hand). `staleness_policy`
controls what `weave check-contracts` does with a divergent contract:
`warn` (diagnostic only), `strict` (non-zero exit — the CI gate), `ignore`.

## `[hub]` (feature: `hub`)

Client-only configuration for an optional, self-hosted snapshot service.
`url` unset is a fully supported permanent state — most teams never enable
this feature. `snapshot_retention` is sent as a hint header on push; actual
pruning is hub-server behavior, not something the client enforces.

## `[slm]` (feature: `slm`)

`model` names a registry entry pulled via `weave slm pull <model> --sha256
<digest>` into `$XDG_CACHE_HOME/weave/models/`. `lazy_load` must stay
`true` for the feature-isolation guarantee to hold (0 MB idle RSS until
`weave ask` is actually invoked) — there is currently no supported reason
to set it `false`.

## `[watch]` (feature: `watch`)

`enabled` must be the literal string `"true"` — anything else (including
absent) is off. `debounce_ms` controls how long a burst of file-change
events is coalesced into one reindex attempt. `blast_radius_ceiling` is the
threshold above which a change is deferred behind a visible marker
(`weave status`, and every MCP tool response) instead of auto-reindexed —
tune it down for a small repo where you want to review large changes
manually, or up for a large repo where routine multi-file edits are
expected and safe to auto-sync.

## `[report]` / `[viz]` (feature: `viz`)

`[report] format` gates which artifacts `weave report` writes in addition
to the always-on Markdown + `.canvas` export: `"html"`/`"all"` also render
offline HTML viewer bundles. `[viz] mode` picks between a static
`file://` bundle (default) and a loopback-only (`127.0.0.1`) static file
server for `weave viz`.

## Reading and writing scalar keys

```bash
weave config set storage.home /path/to/central/knowledge/repo-a
weave config get storage.home
```

`weave config set` only writes scalar (string/bool/number) values — array
keys like `[federation] linked_repos` need a direct edit to
`.weave/config.toml`.
