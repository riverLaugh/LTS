# LTS for Rust dependencies

Do you need a Long-Term-Support version of Rust? It's not going to happen. BUT here's an alternative: get rid of incompatible dependencies. This tool creates a local fork of the crates.io registry and lets you yank any crates from it.

## How does it work?

It clones the crates.io registry to a local directory, and enables Cargo's source replacement feature in `.cargo/config`. Cargo still thinks it uses the crates.io registry, but fetches it from the local directory. `Cargo.lock` remains compatible with the crates.io registry!

The local fork can be modified at will. Currently yanking and unyanking of arbitrary crates is supported.

## Installation

```sh
cargo install -f lts --vers=0.3.0-alpha.1
```

### Requirements

 * **Rust 1.19** or later (this is so old, that even Debian has it),
 * `git` command in `PATH`.

Tested on macOS and Linux.

## Usage

### Yanking crates

`cd` to your project's directory (where the `Cargo.toml` is), and run:

```sh
cargo lts yank "SPEC"
```

where the `SPEC` is crate's name followed by semver range, without a space in between. Semver range starts with `>=`, `<=` or `=` followed by a version. It must be quoted (because `<` and `>` are shell special characters). For example, to yank `serde` version `1.0.118` and all newer versions of `serde`, run:

```sh
cargo lts yank "serde>=1.0.118"
```

On the first run it will set up the registry fork, which may take a minute. After yanking or unyanking run `cargo update` or `cargo generate-lockfile` to apply the changes to your `Cargo.lock`.

Multiple crates can be yanked at the same time:

```sh
cargo lts yank "backtrace<=0.1.8" "gcc<=0.3.0" "lazy_static<=0.1.0" "libc^0.1.0" "mio<=0.3.7" "mio=0.6.0" "nix=0.5.0" "num<=0.1.25" "pkg-config<=0.3.2" "rand<=0.3.8" "rustc-serialize<=0.3.21" "semver<=0.1.5" "void<=0.0.4" "winapi<=0.1.17"
```

### Updating the registry

```sh
cargo lts update
```

Note that `cargo update` alone won't fetch new creates from the crates.io registry, because it's set up to use a local fork. You need to update the local fork with `cargo lts update`.


### Disabling the registry override

This will delete the fork and set config back to normal:

```sh
cargo lts reset
```

or you can edit `.cargo/config` yourself and remove the `replace-with` line.
