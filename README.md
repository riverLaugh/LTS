# LTS for Rust dependencies

Trying to use an old compiler, and all the dependencies are broken? Here's a workaround.

This is a proof of concept of an alternative crates registry that contains crates compatible with old Rust versions.

You can set crates-io registry state per project to contain only crates that are compatible with a selected Rust version.

## Requirements

 * **Rust 1.13** or later (this is so old, that even Debian has it),
 * `git` command in `PATH`.

## Current vs future implementation

The current implementation just rewinds the crates-io registry to a previous commit from a date of release of a specific Rust version, and configures Cargo to use the truncated registry. This means that the registry won't contain any newer crates, even if they'd be compatible.

A future implementation will use a specially-filtered custom registry, allowing use of all crate versions as long as they're compatible.

## Installation

```sh
cargo install -f lts
```

## Usage

`cd` to your project's directory (where `Cargo.toml` is), and run:

```sh
cargo lts
```

It will configure the current project to use registry compatible with the current compiler. On the first run, it may take a few minutes to download the full registry.

You can specify an old Rust version as a compatibility target `cargo lts 1.26.0` or a specific date `cargo lts 2018-01-01`.

And then run:

```sh
cargo update
```

to update `Cargo.lock` from the stale registry. To go back to the future, delete `[source.crates-io] replace-with = …` from `./.cargo/config`.

You can set `CARGO_MANIFEST_DIR` environmental variable to modify other than the current directory. `cargo lts` assumes the index is in `$CARGO_HOME/registry/index/github.com-1ecc6299db9ec823/.git`. You can set `CARGO_REGISTRY_GIT_DIR` to reference another checkout of the index.

`cargo lts prefetch` fetches the registry without altering the local project. Useful to cache registry state in Docker images.
