#![allow(deprecated)] // supporting old versions

#[macro_use]
extern crate serde_derive;
extern crate semver;
extern crate serde;
extern crate serde_json;

use semver::VersionReq;
use semver::Version as SemVer;
use std::fmt::Write;
use std::io::BufRead;
use std::io::BufReader;
use fs::File;
use std::io;
use std::fs;
use std::env;
use std::process::Command;
use std::path::{Path, PathBuf};

const CRATES_IO_INDEX_URL: &str = "https://github.com/rust-lang/crates.io-index";

/// These crates are too old to work with the current compiler (pre-1.0 Rust or pre-NLL bugs)
const DEFAULT_YANKED: &'static [(&'static str, &'static str)] = &[
    ("backtrace", "<0.2.3"),
    ("blake2-rfc", "<0.2.17"),
    ("cfg-if", "<0.1.9"),
    ("conduit-mime-types", "<0.7.3"),
    ("debug_unreachable", "<0.1.1"),
    ("encoding", "<0.2.30"),
    ("error", "<0.1.9"),
    ("gcc", "<0.3.35"),
    ("getopts", "<0.2.18"),
    ("gif", "<0.6.0"),
    ("hyper", "<0.1.13"),
    ("itertools", "<0.3.25"),
    ("lazy_static", "<0.1.16"),
    ("libc", "^0.1"),
    ("log", "<0.3.6"),
    ("log", "<0.4.8,0.4"),
    ("memchr", "<0.1.8"),
    ("mime", "<0.1.3"),
    ("mio", "<0.3.7"),
    ("mio", "<0.6.6,0.6"),
    ("native-tls", "<0.1.5"),
    ("nix", "=0.5.0"),
    ("num", "<0.1.39"),
    ("num-bigint", "<0.1.44"),
    ("num-rational", "<0.1.42"),
    ("num_cpus", "<0.2.13,0.2"),
    ("parking_lot_core", "<0.1.4"),
    ("pest_derive", "<1.0.8"),
    ("phantom", "0.*"),
    ("pkg-config", "<0.3.9"),
    ("plugin", "<0.2.6"),
    ("podio", "<0.1.4"),
    ("rand", "<0.3.15"),
    ("rand_isaac", "=0.1.0"),
    ("route-recognizer", "<0.1.12"),
    ("rustc-serialize", "<0.3.22"),
    ("semver", "<0.1.20"),
    ("solicit", "<0.4.3"),
    ("tempdir", "<0.3.6"),
    ("term", "<0.4.6,0.4"),
    ("tokio-io", "<0.1.13"),
    ("tokio-reactor", "<0.1.3"),
    ("twox-hash", "<1.2.0"),
    ("typeable", "<0.1.2"),
    ("typemap", "<0.3.3"),
    ("unsafe-any", "<0.3.0"),
    ("url", "<0.2.38"),
    ("url", "<1.6.1,1"),
    ("void", "<0.0.5"),
    ("void", "=1.0.0"),
    ("winapi", "<0.1.23"),
    ("winapi", "<0.2.5,0.2"),
];

/// See [the README for the CLI version](https://lib.rs/crates/lts).
pub fn cli_run() -> io::Result<()> {
    let manifest_dir = get_cargo_manifest_dir();
    let dot_cargo_dir = manifest_dir.join(".cargo");

    match parse_args() {
        Op::Exit => return Ok(()),
        Op::Fail => std::process::exit(1),
        Op::Setup => {
            setup_if_needed(&dot_cargo_dir)?;
        },
        Op::Prefetch => {
            fetch_registry(&dot_cargo_dir)?
        },
        Op::Update => {
            fetch_registry(&dot_cargo_dir)?;
            cargo_update_from_forked_index(&manifest_dir)?;
        },
        Op::Reset => delete_local_fork(&dot_cargo_dir)?,
        Op::Yank(specs) => {
            if specs.is_empty() {
                eprintln!("Nothing to change");
                std::process::exit(1);
            }
            let local_repo_copy_dir = setup_if_needed(&dot_cargo_dir)?;
            set_yanked_state(&local_repo_copy_dir, &specs, true)?
        }
    }

    Ok(())
}

struct YankSpec {
    crate_name: String,
    range: VersionReq,
    yank: bool,
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

fn set_yanked_state(from_repo_checkout_dir: &Path, specs: &[YankSpec], verbose: bool) -> io::Result<()> {
    let mut any_modified = false;
    for spec in specs {
        let crate_file = crate_path(from_repo_checkout_dir, &spec.crate_name);
        let jsons = read(&crate_file)?;
        let mut lines_out = Vec::with_capacity(jsons.len());
        let mut modified = false;
        for line1 in jsons.split(|&c| c == b'\n') {
            if line1.is_empty() {
                continue;
            }
            let tmp;
            let mut line = line1;
            if let Ok(mut ver) = serde_json::from_slice::<CrateVersion>(line) {
                if ver.yanked != spec.yank {
                    if let Ok(semver) = SemVer::parse(&ver.vers) {
                        if spec.range.matches(&semver) {
                            ver.yanked = spec.yank;
                            tmp = serde_json::to_vec(&ver).unwrap();
                            line = &tmp;
                            modified = true;
                            if verbose {
                                println!("{} {} yanked = {}", spec.crate_name, ver.vers, spec.yank);
                            }
                        }
                    }
                }
            }
            lines_out.extend_from_slice(line);
            lines_out.push(b'\n');
        }
        if modified {
            any_modified = true;
            write(&crate_file, &lines_out)?;
            git_add(from_repo_checkout_dir, &crate_file)?;
        }
    }
    if any_modified {
        git_commit(from_repo_checkout_dir)?;
    }
    Ok(())
}

fn git_add(checkout: &Path, file_path: &Path) -> io::Result<()> {
    let res = Command::new("git")
        .current_dir(checkout)
        .arg("add")
        .arg("--")
        .arg(file_path)
        .status()?;
    if !res.success() {
        return io_err("Failed to run git add");
    }
    Ok(())
}

fn git_commit(checkout: &Path) -> io::Result<()> {
    let res = Command::new("git")
        .current_dir(checkout)
        .env("GIT_AUTHOR_NAME", "LTS")
        .env("GIT_COMMITTER_NAME", "LTS")
        .env("GIT_AUTHOR_EMAIL", "lts@lib.rs")
        .env("GIT_COMMITTER_EMAIL", "lts@lib.rs")
        .arg("commit")
        .arg("--quiet")
        .arg("-m")
        .arg("cargo lts changes")
        .status()?;
    if !res.success() {
        return io_err("Failed to commit changes");
    }
    Ok(())
}

fn get_cargo_manifest_dir() -> PathBuf {
    if let Some(dir) = env::var_os("CARGO_MANIFEST_DIR") {
        return PathBuf::from(dir);
    }
    let mut root_dir = env::current_dir().expect("cwd");
    {
        let tmp = root_dir.clone();
        let mut tmp = tmp.as_path();
        while let Some(new_tmp) = tmp.parent() {
            if new_tmp.join("Cargo.toml").exists() {
                root_dir = new_tmp.to_owned();
            }
            tmp = new_tmp;
        }
    }
    root_dir
}

fn setup_if_needed(dot_cargo_dir: &Path) -> io::Result<PathBuf> {
    let local_repo_copy_dir = ensure_crates_io_fork_exits_and_is_up_to_date(&dot_cargo_dir)?;
    set_index_source_override(&dot_cargo_dir, &local_repo_copy_dir)?;
    Ok(local_repo_copy_dir)
}

fn get_local_repo_copy_dir(dot_cargo_dir: &Path) -> PathBuf {
    dot_cargo_dir.join("cargo-lts-local-registry-fork")
}

fn ensure_crates_io_fork_exits_and_is_up_to_date(dot_cargo_dir: &Path) -> io::Result<PathBuf> {
    let local_repo_copy_dir = get_local_repo_copy_dir(dot_cargo_dir);
    if !local_repo_copy_dir.exists() {
        clone_crates_io_to_local_fork(&local_repo_copy_dir)?;
        set_default_yanks(&local_repo_copy_dir)?;
    } else {
        update_cloned_repo_fork(&local_repo_copy_dir)?;
    }
    Ok(local_repo_copy_dir)
}

fn set_default_yanks(local_repo_copy_dir: &Path) -> io::Result<()> {
    let yanks: Vec<_> = DEFAULT_YANKED.iter().map(|&(crate_name, range)| {
        YankSpec {
            crate_name: crate_name.to_string(),
            range: VersionReq::parse(range).unwrap(),
            yank: true,
        }
    }).collect();
    set_yanked_state(local_repo_copy_dir, &yanks, false)
}

fn set_index_source_override(dot_cargo_dir: &Path, local_repo_copy_dir: &Path) -> io::Result<()> {
    let config_path = dot_cargo_dir.join("config");

    let mut config_toml = if config_path.exists() {
        filtered_config_toml(&config_path)?
    } else {
        String::new()
    };

    let git_dir_path = local_repo_copy_dir.join(".git");
    let repo_path = if git_dir_path.exists() {
        &git_dir_path
    } else {
        local_repo_copy_dir
    };

    assert!(repo_path.is_absolute());
    let repo_url = format!("file://{}", repo_path.display()).replace(' ', "%20");

    write!(&mut config_toml, "# delete this to restore to the default registry
[source.crates-io]
replace-with = 'lts-repo-local-fork'

[source.lts-repo-local-fork] # `cargo lts` modified copy of the crates.io registry
registry = '{}'
", repo_url).unwrap();

    write(&config_path, config_toml.as_bytes())
}

fn fetch_crates_io_into_repo(repo_path: &Path) -> io::Result<()> {
    // can't reuse local on-disk index, because Cargo doesn't always update HEAD
    let res = Command::new("git")
      .current_dir(repo_path)
      .env("GIT_ASKPASS", "true")
      .arg("fetch")
      .arg(CRATES_IO_INDEX_URL)
      .status()?;
    if !res.success() {
        return io_err("Update of crates.io index failed");
    }
    Ok(())
}

fn update_cloned_repo_fork(repo_path: &Path) -> io::Result<()> {
    println!("Updating index");
    fetch_crates_io_into_repo(repo_path)?;

    let res = Command::new("git")
        .current_dir(repo_path)
        .arg("merge")
        .arg("-Xtheirs")
        .arg("--allow-unrelated-histories")
        .arg("-m")
        .arg("cargo lts update")
        .arg("FETCH_HEAD")
        .status()?;
    if !res.success() {
        return io_err("Merge of crates.io index failed");
    }

    Ok(())
}


#[allow(deprecated)]
fn get_cargo_home() -> Option<PathBuf> {
    env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| env::home_dir().map(|d| d.join(".cargo")))
}

fn standard_crates_io_index_path() -> Option<PathBuf> {
    let cargo_home = match get_cargo_home() {
        Some(p) => p,
        None => return None,
    };
    let path = cargo_home.join("registry").join("index").join("github.com-1ecc6299db9ec823");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn remove_git_origin(git_repo_path: &Path) -> io::Result<()> {
    let res = Command::new("git")
    .current_dir(git_repo_path)
    .arg("remote")
    .arg("rm")
    .arg("origin")
    .status()?;
    if !res.success() {
        return io_err("failed to remove origin from git checkout");
    }
    Ok(())
}

fn io_err(s: &str) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Other, s))
}

fn clone_crates_io_to_local_fork(dest: &Path) -> io::Result<()> {
    assert!(dest.is_absolute());

    let parent_dir = dest.parent().unwrap();
    let _ = fs::create_dir_all(parent_dir); // ensure parent dir exists (.cargo)

    // clone to a temp dir to avoid leaving broken checkout if interrupted
    let dest_tmp = parent_dir.join(".cargo-lts-making-local-fork");
    let _ = fs::remove_dir_all(&dest_tmp);


    let mut cmd = Command::new("git");
    cmd.env("GIT_ASKPASS", "true");
    cmd.arg("clone");


    let reusing_crates_io = if let Some(crates_io_index_git) = standard_crates_io_index_path() {
        cmd.arg(crates_io_index_git);
        true
    } else {
        cmd.arg("--depth=1");
        cmd.arg(CRATES_IO_INDEX_URL);
        false
    };

    assert!(!dest_tmp.exists());
    cmd.arg(&dest_tmp);

    let res = cmd.status()?;
    if !res.success() {
        let _ = fs::remove_dir_all(&dest_tmp);
        return io_err("Clone of crates.io index failed");
    }

    println!("Cloned crates.io index");

    // we don't want crates-io to update this, because that'd cause conflicts
    remove_git_origin(&dest_tmp)?;

    fs::rename(&dest_tmp, dest)?;

    // local crates.io copy could have been old
    // but fetch with it as a reference should be faster
    // (not using git's alternatives feature, because it breaks when crates.io squashes)
    if reusing_crates_io {
        fetch_crates_io_into_repo(dest)?;
        let res = Command::new("git")
            .current_dir(dest)
            .arg("reset")
            .arg("--hard")
            .arg("FETCH_HEAD")
            .status()?;
        if !res.success() {
            return io_err("Failed to update forked index to latest crates.io version");
        }
    }
    Ok(())
}

fn force_update_crates_io_index() -> io::Result<()> {
    let _ = Command::new("cargo")
        .arg("install") // install always uses crates.io index, even if there's a local override
        .arg("libc") // safe trusted crate that can't actually be installed (that's good)
        .arg("--vers").arg("99.99.99-index-update-hack") // just to be sure
        .output()?; // don't print anything
    Ok(())
}

fn cargo_update_from_forked_index(manifest_dir: &Path) -> io::Result<()> {
    let res = Command::new("cargo")
        .current_dir(manifest_dir)
        .arg("update")
        .status()?;

    if !res.success() {
        return io_err("Cargo update of forked index failed");
    }

    Ok(())
}


fn filtered_config_toml(config_path: &Path) -> io::Result<String> {
    let mut config_toml = String::new();

    let f = BufReader::new(File::open(config_path)?);
    let mut skipping = false;
    for line in f.lines() {
        let line = line.unwrap();
        let has_our_comment = line.starts_with("# delete this to restore to the default registry");
        if line.starts_with('[') || has_our_comment {
            skipping = has_our_comment
                || line.starts_with("[source.crates-io]")
                || line.starts_with("[source.lts-repo-");
        }

        if !skipping {
            config_toml.push_str(&line);
            config_toml.push('\n');
        }
    }

    Ok(config_toml)
}

fn delete_local_fork(dot_cargo_dir: &Path) -> io::Result<()> {
    let local_repo_copy_dir = get_local_repo_copy_dir(dot_cargo_dir);
    let _ = fs::remove_dir_all(&local_repo_copy_dir);

    let config_path = dot_cargo_dir.join("config");

    if config_path.exists() {
        let config_toml = filtered_config_toml(&config_path)?;
        if config_toml.trim_left().is_empty() {
            fs::remove_file(config_path)?;
        } else {
            write(&config_path, config_toml.as_bytes())?;
        }
    }

    Ok(())
}

fn fetch_registry(dot_cargo_dir: &Path) -> io::Result<()> {
    let local_repo_copy_dir = get_local_repo_copy_dir(dot_cargo_dir);
    if local_repo_copy_dir.exists() {
        update_cloned_repo_fork(&local_repo_copy_dir)?;
    } else {
        force_update_crates_io_index()?;
    }
    Ok(())
}

fn crate_path(index_root: &Path, crate_name: &str) -> PathBuf {
    let mut new_path = PathBuf::from(index_root);

    match crate_name.len() {
        0 => {},
        1 => new_path.push("1"),
        2 => new_path.push("2"),
        3 => {
            new_path.push("3");
            new_path.push(&crate_name[0..1]);
        }
        _ => {
            new_path.push(&crate_name[0..2]);
            new_path.push(&crate_name[2..4]);
        }
    };

    new_path.push(crate_name);
    new_path
}

/// A single version of a crate published to the index
#[derive(Serialize, Deserialize, Clone, Debug)]
struct CrateVersion {
    name: String,
    vers: String,
    deps: Vec<serde_json::Value>,
    features: Option<serde_json::Value>,
    links: Option<String>,
    cksum: String,
    yanked: bool,
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
