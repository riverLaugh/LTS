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

fn main() -> io::Result<()> {
    let manifest_dir = get_cargo_manifest_dir();
    let dot_cargo_dir = manifest_dir.join(".cargo");

    match parse_args() {
        Op::Exit => return Ok(()),
        Op::Fail => std::process::exit(1),
        Op::Reset => delete_local_fork(&dot_cargo_dir)?,
        Op::Yank(specs) => {
            if specs.is_empty() {
                eprintln!("Nothing to change");
                std::process::exit(1);
            }
            let local_repo_copy_dir = ensure_crates_io_fork_exits_and_is_up_to_date(&dot_cargo_dir)?;

            set_index_source_override(&dot_cargo_dir, &local_repo_copy_dir)?;
            cargo_update_forked_index(&manifest_dir)?;

            for spec in &specs {
                set_yanked_state(&local_repo_copy_dir, spec)?
            }
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

fn parse_yankspecs(args: impl Iterator<Item=String>, yank: bool) -> Vec<YankSpec> {
    args.filter_map(|arg| {
        let pos = match arg.as_bytes().iter().position(|&c| !c.is_ascii_alphanumeric() && c != b'_' && c != b'-') {
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

fn set_yanked_state(from_repo: &Path, spec: &YankSpec) -> io::Result<()> {
    let crate_file = crate_path(from_repo, &spec.crate_name);
    let jsons = fs::read(&crate_file)?;
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
                        println!("{} {} yanked = {}", spec.crate_name, ver.vers, spec.yank);
                    }
                }
            }
        }
        lines_out.extend_from_slice(line);
        lines_out.push(b'\n');
    }
    if modified {
        fs::write(&crate_file, &lines_out)?;
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

fn get_local_repo_copy_dir(dot_cargo_dir: &Path) -> PathBuf {
    dot_cargo_dir.join("cargo-lts-local-registry-fork")
}

fn ensure_crates_io_fork_exits_and_is_up_to_date(dot_cargo_dir: &Path) -> io::Result<PathBuf> {
    let local_repo_copy_dir = get_local_repo_copy_dir(dot_cargo_dir);
    if !local_repo_copy_dir.exists() {
        clone_crates_io_to_local_fork(&local_repo_copy_dir)?;
    } else {
        update_cloned_repo_fork(&local_repo_copy_dir)?;
    }
    Ok(local_repo_copy_dir)
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

    fs::write(&config_path, config_toml.as_bytes())
}

fn update_cloned_repo_fork(repo_path: &Path) -> io::Result<()> {
    println!("Updating index");

    let mut cmd = Command::new("git");
    cmd.current_dir(repo_path);
    cmd.arg("fetch");

    if let Some(crates_io_index_git) = standard_crates_io_index_path() {
        force_update_crates_io_index()?;
        cmd.arg(crates_io_index_git);
    } else {
        cmd.arg("--depth=1");
        cmd.arg(CRATES_IO_INDEX_URL);
    }

    let res = cmd.status()?;
    if !res.success() {
        return io_err("Update of crates.io index failed");
    }

    let mut cmd = Command::new("git");
    cmd.current_dir(repo_path);
    cmd.arg("merge");
    cmd.arg("-Xtheirs");
    cmd.arg("FETCH_HEAD");

    let res = cmd.status()?;
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
    let path = get_cargo_home()?
        .join("registry").join("index").join("github.com-1ecc6299db9ec823");
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
    let dest_tmp = parent_dir.join(".lts-temp-checkout");
    let _ = fs::remove_dir_all(&dest_tmp);


    let mut cmd = Command::new("git");
    cmd.arg("clone");

    if let Some(crates_io_index_git) = standard_crates_io_index_path() {
        println!("Reusing {}", crates_io_index_git.display());
        force_update_crates_io_index()?;
        cmd.arg(crates_io_index_git);
    } else {
        println!("Cargo index copy not found, cloning a fresh one");
        cmd.arg("--depth=1");
        cmd.arg(CRATES_IO_INDEX_URL);
    }

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

fn cargo_update_forked_index(manifest_dir: &Path) -> io::Result<()> {
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
            fs::write(&config_path, config_toml.as_bytes())?;
        }
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
pub struct CrateVersion {
    name: String,
    vers: String,
    deps: Vec<serde_json::Value>,
    features: Option<serde_json::Value>,
    links: Option<String>,
    cksum: String,
    yanked: bool,
}
