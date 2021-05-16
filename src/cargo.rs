use write;
use io_err;
use std::process::Command;
use std::env;
use std::fmt::Write;
use std::fs::File;
use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::io;
use std::path::{Path, PathBuf};
use cargo_repository_hash;

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

pub struct CargoConfig {
    manifest_dir: PathBuf,
    dot_cargo_dir: PathBuf,
}

impl CargoConfig {
    pub fn new() -> Self {
        let manifest_dir = get_cargo_manifest_dir();
        CargoConfig {
            dot_cargo_dir: manifest_dir.join(".cargo"),
            manifest_dir: manifest_dir,
        }
    }

    pub fn default_forked_index_repository_path(&self) -> PathBuf {
        self.dot_cargo_dir.join("cargo-lts-local-registry-fork")
    }

    // checkout must exist
    fn repo_path_as_url(repo_path: &Path) -> io::Result<String> {
        let repo_path = fs::canonicalize(repo_path)?;
        Ok(format!("file://{}", repo_path.display()).replace(' ', "%20"))
    }

    pub fn set_index_source_override(&self, repo_path: &Path) -> io::Result<()> {
        let config_path = self.dot_cargo_dir.join("config");

        let mut config_toml = if config_path.exists() {
            Self::filtered_config_toml(&config_path)?
        } else {
            String::new()
        };

        let repo_url = Self::repo_path_as_url(repo_path)?;

        write!(&mut config_toml, "# delete this to restore to the default registry
    [source.crates-io]
    replace-with = 'lts-repo-local-fork'

    [source.lts-repo-local-fork] # `cargo lts` modified copy of the crates.io registry
    registry = '{}'
    ", repo_url).unwrap();

        write(&config_path, config_toml.as_bytes())
    }

    pub fn unset_index_source_override(&self) -> io::Result<()> {
        let config_path = self.dot_cargo_dir.join("config");

        if config_path.exists() {
            let config_toml = Self::filtered_config_toml(&config_path)?;
            if config_toml.trim_left().is_empty() {
                fs::remove_file(config_path)?;
            } else {
                write(&config_path, config_toml.as_bytes())?;
            }
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

    pub fn cargo_update_from_current_index(&self) -> io::Result<()> {
        let res = Command::new("cargo")
            .current_dir(&self.manifest_dir)
            .arg("update")
            .status()?;

        if !res.success() {
            return io_err("Cargo update of forked index failed");
        }

        Ok(())
    }

    pub fn cargo_private_crates_io_git_repo_path() -> Option<PathBuf> {
        let cargo_home = match get_cargo_home() {
            Some(p) => p,
            None => return None,
        };
        assert!(cargo_home.is_absolute());
        let path = cargo_home.join("registry").join("index").join("github.com-1ecc6299db9ec823");
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    pub fn cargo_private_custom_git_repo_path(repo_path: &Path) -> Option<PathBuf> {
        let cargo_home = match get_cargo_home() {
            Some(p) => p,
            None => return None,
        };
        assert!(cargo_home.is_absolute());
        let url = match Self::repo_path_as_url(repo_path) {
            Ok(p) => p,
            Err(_) => return None,
        };
        let hash = cargo_repository_hash::short_hash(&url);
        Some(cargo_home.join("registry").join("index").join(format!("-{}", hash)))
    }
}


#[allow(deprecated)]
fn get_cargo_home() -> Option<PathBuf> {
    env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| env::home_dir().map(|d| d.join(".cargo")))
}
