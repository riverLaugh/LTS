extern crate lts;

use lts::LTS;
use std::env;
use std::fs::File;
use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::exit;
use std::process::Output;

static RUST_RELEASE_DATES: [(&'static str, &'static str); 71] = [
    ("2015-05-15", "1.0.0"),
    ("2015-06-25", "1.1.0"),
    ("2015-08-07", "1.2.0"),
    ("2015-09-17", "1.3.0"),
    ("2015-10-29", "1.4.0"),
    ("2015-12-10", "1.5.0"),
    ("2016-01-21", "1.6.0"),
    ("2016-03-03", "1.7.0"),
    ("2016-04-14", "1.8.0"),
    ("2016-05-26", "1.9.0"),
    ("2016-07-07", "1.10.0"),
    ("2016-08-18", "1.11.0"),
    ("2016-09-29", "1.12.0"),
    ("2016-10-20", "1.12.1"),
    ("2016-11-10", "1.13.0"),
    ("2016-12-22", "1.14.0"),
    ("2017-02-02", "1.15.0"),
    ("2017-02-09", "1.15.1"),
    ("2017-03-16", "1.16.0"),
    ("2017-04-27", "1.17.0"),
    ("2017-06-08", "1.18.0"),
    ("2017-07-20", "1.19.0"),
    ("2017-08-31", "1.20.0"),
    ("2017-10-12", "1.21.0"),
    ("2017-11-22", "1.22.0"),
    ("2017-11-22", "1.22.1"),
    ("2018-01-04", "1.23.0"),
    ("2018-02-15", "1.24.0"),
    ("2018-03-01", "1.24.1"),
    ("2018-03-29", "1.25.0"),
    ("2018-05-10", "1.26.0"),
    ("2018-05-29", "1.26.1"),
    ("2018-06-05", "1.26.2"),
    ("2018-06-21", "1.27.0"),
    ("2018-07-10", "1.27.1"),
    ("2018-07-20", "1.27.2"),
    ("2018-08-02", "1.28.0"),
    ("2018-09-13", "1.29.0"),
    ("2018-09-25", "1.29.1"),
    ("2018-10-11", "1.29.2"),
    ("2018-10-25", "1.30.0"),
    ("2018-11-08", "1.30.1"),
    ("2018-12-06", "1.31.0"),
    ("2018-12-20", "1.31.1"),
    ("2019-01-17", "1.32.0"),
    ("2019-02-28", "1.33.0"),
    ("2019-04-11", "1.34.0"),
    ("2019-04-25", "1.34.1"),
    ("2019-05-14", "1.34.2"),
    ("2019-05-23", "1.35.0"),
    ("2019-07-04", "1.36.0"),
    ("2019-08-15", "1.37.0"),
    ("2019-09-20", "1.38.0"),
    ("2019-11-07", "1.39.0"),
    ("2019-12-19", "1.40.0"),
    ("2020-01-30", "1.41.0"),
    ("2020-02-27", "1.41.1"),
    ("2020-03-12", "1.42.0"),
    ("2020-04-23", "1.43.0"),
    ("2020-05-07", "1.43.1"),
    ("2020-06-04", "1.44.0"),
    ("2020-06-18", "1.44.1"),
    ("2020-07-16", "1.45.0"),
    ("2020-07-30", "1.45.1"),
    ("2020-08-03", "1.45.2"),
    ("2020-08-27", "1.46.0"),
    ("2020-10-08", "1.47.0"),
    ("2020-11-19", "1.48.0"),
    ("2020-12-31", "1.49.0"),
    ("2021-02-11", "1.50.0"),
    ("2021-03-25", "1.51.0"),
];

fn check(output: Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        println!("{}", stdout);
        println!("{}", String::from_utf8_lossy(&output.stderr));
        exit(1);
    }
    stdout.trim().to_string()
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

fn get_cutoff_date(arg: Option<&str>) -> (String, String) {
    if let Some(arg) = arg {
        let arg_dot = format!("{}.", arg);
        if arg.starts_with("20") && arg.contains('-') {
            for &(date, ver) in RUST_RELEASE_DATES.iter() {
                if date >= &arg {
                    return (arg.to_owned(), ver.to_owned());
                }
            }
            return (arg.to_owned(), "<date>".into());
        }
        if arg.contains('.') {
            for &(date, ver) in RUST_RELEASE_DATES.iter() {
                if ver == arg || ver.starts_with(&arg_dot) {
                    return (date.to_owned(), ver.to_owned());
                }
            }
        }
    }
    let ver_str = check(Command::new("rustc").arg("--version").output().unwrap());
    let arg = ver_str.splitn(3, ' ').skip(1).next().expect("rustc version ???");
    let arg = arg.splitn(2, '-').next().unwrap();
    for &(date, ver) in RUST_RELEASE_DATES.iter() {
        if ver == arg {
            return (date.to_owned(), ver.to_owned());
        }
    }
    println!("Specify Rust version (1.x.y) or ISO date (YYYY-MM-DD) as an argument");
    exit(1);
}


fn main() {
    let arg = env::args().skip(1).filter(|a| a != "lts" && !a.starts_with('-')).next();
    let arg = arg.as_ref().map(|s| s.as_str());
    let prefetch_only = arg == Some("prefetch");

    let lts = LTS::new(env::var_os("CARGO_REGISTRY_GIT_DIR").map(PathBuf::from));

    if !lts.git_dir().exists() {
        println!("{} doesn't exist. Set CARGO_REGISTRY_GIT_DIR to cargo index .git dir", lts.git_dir().display());
        // makes the index as a side effect
        let _ = Command::new("cargo").arg("install").arg("libc").output();
        if !lts.git_dir().exists() {
            exit(1);
        }
    }

    let cargo_local_dir = prepare_cargo_dir();

    if arg.map(|a| a.starts_with("https://")).unwrap_or(false) {
        let url = arg.unwrap();
        set_custom_index_url(&cargo_local_dir, "lts-repo-custom-url", url, "cargo lts override");
        return;
    }

    lts.fetch().unwrap();

    if prefetch_only {
        return;
    }

    let (cutoff, rust_vers) = get_cutoff_date(arg);

    // make a new repo with just that commit
    let branch = lts.cut_branch_at(&cutoff).unwrap();

    let fork_repo_git_dir = cargo_local_dir.join(&branch.name);

    let fork_url = lts.clone_to(&branch, &fork_repo_git_dir, true).unwrap();

    set_custom_index_url(&cargo_local_dir, &branch.name, &fork_url, &format!("{} ({})", cutoff, rust_vers));
}

fn prepare_cargo_dir() -> PathBuf {
    let root = get_cargo_manifest_dir();

    let cargo_local_dir = root.join(".cargo");
    let _ = fs::create_dir(&cargo_local_dir);
    cargo_local_dir
}

fn set_custom_index_url(cargo_local_dir: &Path, fork_name: &str, fork_url: &str, description: &str) {
    let config_path = cargo_local_dir.join("config");
    let mut config_toml = String::new();

    if config_path.exists() {
        let f = BufReader::new(File::open(&config_path).expect("can't read .cargo/config"));
        let mut skipping = false;
        for line in f.lines() {
            let line = line.unwrap();
            if line.starts_with('[') || line.starts_with("# delete this") {
                skipping = line.starts_with("[source.crates-io]")
                    || line.starts_with("# delete this")
                    || line.starts_with("[source.lts-repo-");
            }

            if !skipping {
                config_toml.push_str(&line);
                config_toml.push('\n');
            }
        }
    }

    config_toml.push_str(&format!("# delete this to restore to the default registry
[source.crates-io]
replace-with = '{fork_name}'

[source.{fork_name}] # {description}
registry = '{fork_url}'
", fork_name = fork_name, description = description, fork_url = fork_url));

    let mut out = File::create(&config_path).expect("Writing .cargo/config");
    out.write_all(config_toml.as_bytes()).unwrap();

    println!("Set {} to use registry state from {}", config_path.display(), description);
}
