const CRATES_IO_INDEX_URL: &str = "https://github.com/rust-lang/crates.io-index";

use serde_json;
use write;
use read;
use io_err;
use cargo::CargoConfig;
use semver::VersionReq;
use semver::Version as SemVer;
use std::io;
use std::fs;

use std::process::Command;
use std::path::{Path, PathBuf};

pub struct ForkedRegistryIndex {
    git_checkout: PathBuf,
}

pub struct YankSpec {
    pub crate_name: String,
    pub range: VersionReq,
    pub yank: bool,
}

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


impl ForkedRegistryIndex {
    pub fn new(local_repo_copy_dir: PathBuf) -> Self {
        ForkedRegistryIndex {
            git_checkout: local_repo_copy_dir,
        }
    }

    pub fn init(&self) -> io::Result<()> {
        if !self.git_checkout.exists() {
            self.clone_crates_io_to_local_fork()?;
            self.set_default_yanks()?;
        } else {
            self.update_cloned_repo_fork()?;
        }
        Ok(())
    }

    pub fn deinit(&self) -> io::Result<()> {
        let _ = fs::remove_dir_all(&self.git_checkout);
        Ok(())
    }

    pub fn git_dir(&self) -> PathBuf {
        let git_dir_path = self.git_checkout.join(".git");
        if git_dir_path.exists() {
            git_dir_path
        } else {
            self.git_checkout.clone()
        }
    }


    fn git_add(&self, file_path: &Path) -> io::Result<()> {
        let res = Command::new("git")
            .current_dir(&self.git_checkout)
            .arg("add")
            .arg("--")
            .arg(file_path)
            .status()?;
        if !res.success() {
            return io_err("Failed to run git add");
        }
        Ok(())
    }

    fn git_commit(&self) -> io::Result<()> {
        let res = Command::new("git")
            .current_dir(&self.git_checkout)
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

    fn set_default_yanks(&self) -> io::Result<()> {
        let yanks: Vec<_> = DEFAULT_YANKED.iter().map(|&(crate_name, range)| {
            YankSpec {
                crate_name: crate_name.to_string(),
                range: VersionReq::parse(range).unwrap(),
                yank: true,
            }
        }).collect();
        self.set_yanked_state(&yanks, false)
    }


    pub fn set_yanked_state(&self, specs: &[YankSpec], verbose: bool) -> io::Result<()> {
        let mut any_modified = false;
        for spec in specs {
            let crate_file = self.crate_path(&spec.crate_name);
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
                self.git_add(&crate_file)?;
            }
        }
        if any_modified {
            self.git_commit()?;
        }
        Ok(())
    }

    fn crate_path(&self, crate_name: &str) -> PathBuf {
        let mut new_path = self.git_checkout.clone();

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

    fn fetch_crates_io_into_repo(&self) -> io::Result<()> {
        // can't reuse local on-disk index, because Cargo doesn't always update HEAD
        let res = Command::new("git")
          .current_dir(&self.git_checkout)
          .env("GIT_ASKPASS", "true")
          .arg("fetch")
          .arg(CRATES_IO_INDEX_URL)
          .status()?;
        if !res.success() {
            return io_err("Update of crates.io index failed");
        }
        Ok(())
    }

    pub fn update_cloned_repo_fork(&self) -> io::Result<()> {
        println!("Updating index");
        self.fetch_crates_io_into_repo()?;

        let res = Command::new("git")
            .current_dir(&self.git_checkout)
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

    fn clone_crates_io_to_local_fork(&self) -> io::Result<()> {
        let parent_dir = self.git_checkout.parent().unwrap();
        let _ = fs::create_dir_all(parent_dir); // ensure parent dir exists (.cargo)

        // clone to a temp dir to avoid leaving broken checkout if interrupted
        let dest_tmp = parent_dir.join(".cargo-lts-making-local-fork");
        let _ = fs::remove_dir_all(&dest_tmp);

        let mut cmd = Command::new("git");
        cmd.env("GIT_ASKPASS", "true");
        cmd.arg("clone");

        let reusing_crates_io = if let Some(crates_io_index_git) = CargoConfig::cargo_private_crates_io_git_repo_path() {
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
        Self::remove_git_origin(&dest_tmp)?;

        let _ = fs::remove_dir_all(&self.git_checkout);
        fs::rename(&dest_tmp, &self.git_checkout)?;

        // local crates.io copy could have been old
        // but fetch with it as a reference should be faster
        // (not using git's alternatives feature, because it breaks when crates.io squashes)
        if reusing_crates_io {
            self.fetch_crates_io_into_repo()?;
            let res = Command::new("git")
                .current_dir(&self.git_checkout)
                .arg("reset")
                .arg("--hard")
                .arg("FETCH_HEAD")
                .status()?;
            if !res.success() {
                return io_err("Failed to update forked index to latest crates.io version");
            }
        }

        // Cargo is super slow at cloning from one dir (./fork) to another (~/.cargo/regstry),
        // and native git can just hardlink, so do that.
        if let Some(path) = CargoConfig::cargo_private_custom_git_repo_path(&self.git_checkout) {
            if !path.exists() {
                let _ = Command::new("git") // this is optional optimization
                    .arg("clone")
                    .arg("--bare")
                    .arg(&self.git_checkout)
                    .arg(path)
                    .status()?;
            }
        }
        Ok(())
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
