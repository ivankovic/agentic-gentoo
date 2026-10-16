# deck

What is installed on a Gentoo machine, where each package came from, which carry local patches,
and where each patch stands upstream. A TUI, plus tab-separated output for scripts and agents.

```sh
cargo install --path tools/deck --locked   # → ~/.cargo/bin/deck
deck                                       # the TUI
deck packages [--filter here|compiled|patched|overlay|…]
deck patches
deck mark <category/package/file> <state> [--url URL] [--note TEXT]
deck upstream <category/package/file> <URL>
```

It reads the host's directory, `~/src/agentic-gentoo/hosts/$(hostname)` unless `--host-dir` or
`DECK_HOST_DIR` says otherwise. Nothing is cached: every start reads the system afresh.

## Origin

From Portage's database in `/var/db/pkg`:

| Origin | Meaning | How it is told |
|---|---|---|
| `binhost` | binary package from the binhost | `BINPKGMD5` or `BUILD_ID` recorded |
| `compiled` | built here, the build made machine code | an installed file is ELF, PE/EFI, LLVM bitcode or SPIR-V (firmware and docs aside) |
| `prebuilt` | built here from upstream's binaries | the package name ends in `-bin` |
| `noarch` | built here, no machine code | none of the above: scripts, data, fonts, firmware |

`Eco` names the ecosystem a package vendors from (`rust`, `go`, `python`, `ruby`), read off the
eclasses it inherits.

## Patches

Patch files in the host's `portage/patches/` (user patches, deployed to `/etc/portage/patches`)
and in `overlay/<cat>/<pkg>/files/`. A patch's id is `category/package/file`, so it keeps its
history when the version directory is renamed on a bump. `Applies to` is the installed package
the directory matches the way `eapply_user` matches it (PN, P or PF, with an optional `:slot`);
`(no match)` means it applies to nothing installed. `Deployed` compares the file with the
system's copy.

Upstream state lives in the host's `upstream.toml`: one `[[patch]]` per patch, one
`[[patch.event]]` per change of state, each with a timestamp and an optional URL and note. The
last event is the current state. A patch removed from the repository stays listed, as
`(file gone)`, so its history is kept.

| State | Meaning |
|---|---|
| `unsent` | ours, not offered upstream yet |
| `sent` | offered upstream, no response yet |
| `in-review` | upstream is reviewing it |
| `changes-requested` | upstream asked for changes |
| `accepted` | merged upstream, not in a release yet |
| `released` | in an upstream release; drop it on the next bump |
| `backport` | taken from upstream; drop it on the next bump |
| `rejected` | upstream said no for good; ours to carry |
| `withdrawn` | we took it back |
| `dropped` | no longer carried here |

The first five count as open.

## Keys

| Key | |
|---|---|
| `Tab`, `1`, `2` | packages / patches |
| `j` `k` `d` `u` `g` `G` | move, page, top, bottom (arrows, PgUp/PgDn, Home/End too) |
| `/`, `Esc` | search by name, clear it |
| `f` `F` | next / previous package filter |
| `Enter` | on a patched package: its patches; on a patch: change its state |
| `m` | change a patch's state: pick it, then URL and note (both optional) |
| `U` | set the patch's upstream project |
| `r` | reload |
| `q` | quit |
