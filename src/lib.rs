//! Use a copy of the crates-io index that has only crates which are compatible with your compiler.
//!
//! To use it from the command-line, run:
//!
//! ```sh
//! cargo install lts
//! cargo lts
//! ```
//!
//! It will ensure the current project uses crates-io index with crates compatible with the currently default rustc version.
//!
//! This documentation is for a library interface of `cargo-lts`.
//! The library interface makes a shallow git clone of crates-io repository frozen at a specific point in time.

use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
#[cfg(unix)]
use std::os::unix::fs::symlink;

mod minidate;
pub use minidate::Date;

const SNAPSHOT_BRANCHES: &'static [&'static str; 21] = &[
    "snapshot-2018-09-26",
    "snapshot-2019-10-17",
    "snapshot-2020-03-25",
    "snapshot-2020-08-04",
    "snapshot-2020-11-20",
    "snapshot-2021-05-05",
    "snapshot-2021-07-02",
    "snapshot-2021-09-24",
    "snapshot-2021-12-21",
    "snapshot-2022-03-02",
    "snapshot-2022-07-06",
    "snapshot-2022-08-31",
    "snapshot-2022-12-19",
    "snapshot-2023-01-12",
    "snapshot-2023-04-03",
    "snapshot-2023-06-30",
    "snapshot-2023-12-03",
    "snapshot-2024-03-11",
    "snapshot-2024-05-18",
    "snapshot-2024-09-08",
    "snapshot-2024-11-27",
];

mod cargo_repository_hash;

/// Main library handle for `CARGO_HOME`
pub struct LTS {
    git_dir: PathBuf,
    home: PathBuf,
}

/// Branch/fork of the index. Call `LTS::cut_branch_at` to get it.
pub struct Branch {
    /// Name of the branch created
    pub name: String,
    /// Hash of its latest commit
    pub head: String,
}

impl LTS {
    /// Optionally specify custom registry `.git` directory
    pub fn new(dir: Option<PathBuf>) -> Self {
        let home = get_cargo_home();
        let git_dir = dir.unwrap_or_else(|| home.join("registry/index/github.com-1ecc6299db9ec823/.git"));
        LTS {
            git_dir: git_dir,
            home: home,
        }
    }

    /// Ensure an old snapshot is available.
    ///
    /// Without calling this any attempt to use older revision may fail.
    pub fn fetch(&self, for_date: Date) -> io::Result<()> {
        let needed_snapshot = SNAPSHOT_BRANCHES.iter().find(|s| {
            let snap_date = Date::from_str(&s[9..]).unwrap();
            snap_date >= for_date
        });
        if let Some(branch_name) = needed_snapshot {
            if !self.git(&["rev-parse", branch_name, "--"]).is_ok() {
                self.git(&["fetch", "https://github.com/rust-lang/crates.io-index", &format!("{s}:{s}", s = branch_name)])?;
            }
        }
        Ok(())
    }

    /// Create a new branch in the local Cargo registry clone.
    ///
    /// Branch will contain only commits up to given date YYYY-MM-DD
    pub fn cut_branch_at(&self, cutoff_date: Date) -> io::Result<Branch> {
        let cutoff = cutoff_date.to_string();
        let last_commit_hash = self.git(&["log", "--all", "-1", "--format=%H", "--until", &cutoff])?;
        let treeish = format!("{}^{{tree}}", last_commit_hash);
        let msg = format!("Registry at {}", cutoff);
        // create a new commit that is a snapshot of that commit
        let new_head = self.git(&["commit-tree", &treeish, "-m", &msg])?;

        let fork_name = format!("lts-repo-at-{}", &last_commit_hash[0..10]);

        // git requires exposing a commit as a ref in order to clone it
        if self.git(&["branch", &fork_name, &new_head]).is_err() {
            let refname = format!("refs/heads/{}", fork_name);
            self.git(&["update-ref", &refname, &new_head])?;
        }

        Ok(Branch {
            name: fork_name,
            head: new_head,
        })
    }

    /// Create a new, shallow local git checkout from the branch
    ///
    /// Returns `file://` URL of the new repo
    pub fn clone_to(&self, branch: &Branch, fork_destination_dir: &Path, bare: bool) -> io::Result<String> {
        let _ = fs::remove_dir_all(&fork_destination_dir); // just in case

        let mut cmd = Command::new("git");
        cmd.args(&["clone", "--single-branch", "--branch", &branch.name, "--reference", &self.git_dir]);
        if bare {
            cmd.arg("--bare");
        }
        cmd.arg(&self.git_dir).arg(&fork_destination_dir);
        check(cmd.output()?)?;

        let tmp;
        let fork_git_dir = if bare {
            fork_destination_dir
        } else {
            tmp = fork_destination_dir.join(".git");
            &tmp
        };

        // do fixups, so that cargo can find proper dir
        git(&fork_git_dir, &["update-ref", "HEAD", &branch.head])?;
        if bare {
            git(&fork_git_dir, &["branch", "master", &branch.head])?;
        } else {
            git(&fork_git_dir, &["checkout", "-b", "master", &branch.head])?;
        }

        let fork_repo_abs = fs::canonicalize(&fork_git_dir)?;
        let fork_url = format!("file://{}", fork_repo_abs.display());

        self.make_cache_shared(&fork_url);

        Ok(fork_url)
    }

    /// Registry git location, FYI
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    fn git(&self, args: &[&str]) -> io::Result<String> {
        git(&self.git_dir, args)
    }


    // Because the crate files are actually the same, it makes sense to share them
    fn make_cache_shared(&self, url: &str) {
        let hash = cargo_repository_hash::short_hash(url);
        let fork_cache_dir = self.home.join(format!("registry/cache/-{}", hash));
        let git_cache_dir = self.home.join("registry/cache/github.com-1ecc6299db9ec823");
        #[cfg(unix)]
        let _ = symlink(git_cache_dir, fork_cache_dir);
        let fork_src_dir = self.home.join(format!("registry/src/-{}", hash));
        let git_src_dir = self.home.join("registry/src/github.com-1ecc6299db9ec823");
        #[cfg(unix)]
        let _ = symlink(git_src_dir, fork_src_dir);
    }
}

fn git(git_dir: &Path, args: &[&str]) -> io::Result<String> {
    // println!("{:?}",args);
    let out = Command::new("git")
        .env("GIT_AUTHOR_NAME", "LTS")
        .env("GIT_COMMITTER_NAME", "LTS")
        .env("GIT_AUTHOR_EMAIL", "lts@lib.rs")
        .env("GIT_COMMITTER_EMAIL", "lts@lib.rs")
        .env("GIT_ASKPASS", "true")
        .arg("--git-dir")
        .arg(git_dir)
        .args(args)
        .output()?;
    check(out)
}

fn check(out: Output) -> io::Result<String> {
    if !out.status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, String::from_utf8_lossy(&out.stderr).into_owned()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[allow(deprecated)]
fn get_cargo_home() -> PathBuf {
    env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| env::home_dir().map(|d| d.join(".cargo"))).expect("$CARGO_HOME not set")
}

