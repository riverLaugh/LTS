#![allow(deprecated)] // supporting old versions

#[macro_use]
extern crate serde_derive;
extern crate semver;
extern crate serde;
extern crate serde_json;

use regfork::YankSpec;
use cargo::CargoConfig;
use semver::VersionReq;
use std::io;
use std::fs;
use std::env;
use std::process::Command;
use std::path::Path;

mod cargo;
mod cargo_repository_hash;

mod regfork;
use regfork::ForkedRegistryIndex;

/// See [the README for the CLI version](https://lib.rs/crates/lts).
pub fn cli_run() -> io::Result<()> {
    let cargo_config = CargoConfig::new();

    match parse_args() {
        Op::Exit => return Ok(()),
        Op::Fail => std::process::exit(1),
        Op::Setup => {
            setup_if_needed(&cargo_config)?;
        },
        Op::Prefetch => {
            fetch_registry(&cargo_config)?
        },
        Op::Update => {
            fetch_registry(&cargo_config)?;
            cargo_config.cargo_update_from_current_index()?;
        },
        Op::Reset => delete_local_fork(&cargo_config)?,
        Op::Yank(specs) => {
            if specs.is_empty() {
                eprintln!("Nothing to change");
                std::process::exit(1);
            }
            let fork = setup_if_needed(&cargo_config)?;
            fork.set_yanked_state(&specs, true)?
        }
    }

    Ok(())
}

enum Op {
    Reset,
    Prefetch,
    Setup,
    Update,
    Yank(Vec<YankSpec>),
    Exit,
    Fail,
}

fn parse_args() -> Op {
    let mut args = env::args().skip(1);
    let cmd = match args.by_ref().filter(|arg| arg != "lts").next() {
        Some(cmd) => cmd,
        None => {
            print_help();
            return Op::Fail;
        },
    };

    match cmd.as_str() {
        "setup" => Op::Setup,
        "prefetch" => Op::Prefetch,
        "update" => Op::Update,
        "yank" => {
            Op::Yank(parse_yankspecs(args, true))
        },
        "unyank" => {
            Op::Yank(parse_yankspecs(args, false))
        },
        "reset" | "unset" => {
            Op::Reset
        },
        "-h" | "--help" => {
            print_help();
            Op::Exit
        },
        "--version" | "-V" => {
            print_version();
            Op::Exit
        },
        wat => {
            eprintln!("Unknown arg: {}", wat);
            Op::Fail
        }
    }
}

fn print_version() {
    println!("lts {} https://lib.rs/lts", env!("CARGO_PKG_VERSION"));
}

fn print_help() {
    print_version();
println!(r#"Locally patch crates.io registry for a Cargo project

Remove any crate from the registry:
    cargo lts yank "SPEC"

SPEC is crate's name followed by a semver range without a space in between,
e.g. "pkg-config<=0.3.6", "semver>=0.11", or "openssl=0.0.1", or "file*".
SPEC must be in quotes. Run `cargo update` to apply changes.

Bring back yanked crate:
    cargo lts unyank "SPEC"

Pull new crate versions from the crates.io registry:
    cargo lts update

When using a patched registry `cargo update` doesn't fetch from crates.io.

Reset back to normal crates.io registry:
    cargo lts reset
"#
);
}

fn parse_yankspecs<I>(args: I, yank: bool) -> Vec<YankSpec> where I: Iterator<Item=String> {
    args.filter_map(|arg| {
        let pos = match arg.as_bytes().iter().position(|&c| !(c as char).is_alphanumeric() && c != b'_' && c != b'-') {
            Some(p) => p,
            None => {
                eprintln!("Spec '{arg}' doesn't contain semver version. It should be like '{arg}<=0.0.1'", arg = arg);
                return None;
            },
        };
        let (crate_name, range) = arg.split_at(pos);
        if crate_name.is_empty() {
            eprintln!("'{arg}' was interpreted as a semver range, but is missing crate name like 'cratename{arg}'", arg = arg);
            return None;
        }

        let range = match VersionReq::parse(range) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Semver range '{}' for '{}' doesn't parse: {}", range, crate_name, e);
                return None;
            }
        };
        Some(YankSpec {
            crate_name: crate_name.into(),
            range,
            yank,
        })
    }).collect()
}

fn setup_if_needed(cargo: &CargoConfig) -> io::Result<ForkedRegistryIndex> {
    let fork = ForkedRegistryIndex::new(cargo.get_local_repo_copy_dir());
    fork.init()?;
    cargo.set_index_source_override(&fork.git_dir())?;
    Ok(fork)
}

fn io_err(s: &str) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Other, s))
}


fn force_update_crates_io_index() -> io::Result<()> {
    let _ = Command::new("cargo")
        .arg("install") // install always uses crates.io index, even if there's a local override
        .arg("libc") // safe trusted crate that can't actually be installed (that's good)
        .arg("--vers").arg("99.99.99-index-update-hack") // just to be sure
        .output()?; // don't print anything
    Ok(())
}

fn delete_local_fork(cargo: &CargoConfig) -> io::Result<()> {
    let f = ForkedRegistryIndex::new(cargo.get_local_repo_copy_dir());
    f.deinit()?;
    cargo.delete_source_override()
}

fn fetch_registry(cargo: &CargoConfig) -> io::Result<()> {
    let local_repo_copy_dir = cargo.get_local_repo_copy_dir();
    if local_repo_copy_dir.exists() {
        let f = ForkedRegistryIndex::new(local_repo_copy_dir);
        f.update_cloned_repo_fork()?;
    } else {
        force_update_crates_io_index()?;
    }
    Ok(())
}

fn read(path: &Path) -> io::Result<Vec<u8>> {
    use io::Read;
    let mut f = fs::File::open(path)?;
    let mut out = Vec::new();
    f.read_to_end(&mut out)?;
    Ok(out)
}

fn write(path: &Path, data: &[u8]) -> io::Result<()> {
    use io::Write;
    let mut f = fs::File::create(path)?;
    f.write_all(data)
}
