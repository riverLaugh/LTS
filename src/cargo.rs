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

    pub fn get_local_repo_copy_dir(&self) -> PathBuf {
        self.dot_cargo_dir.join("cargo-lts-local-registry-fork")
    }

    pub fn set_index_source_override(&self, repo_path: &Path) -> io::Result<()> {
        let config_path = self.dot_cargo_dir.join("config");

        let mut config_toml = if config_path.exists() {
            Self::filtered_config_toml(&config_path)?
        } else {
            String::new()
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

    pub fn delete_source_override(&self) -> io::Result<()> {
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
