use std::{env, process::Command};

use time::{format_description::well_known::Rfc3339, OffsetDateTime};

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_owned()
    })
}

fn main() {
    for name in ["GIT_COMMIT_SHA", "GIT_DIRTY", "SOURCE_DATE_EPOCH"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!("cargo:rerun-if-changed=build.rs");
    if std::path::Path::new(".packaged-commit").exists() {
        println!("cargo:rerun-if-changed=.packaged-commit");
    }

    // Resolve Git paths through Git itself: a linked worktree has a .git file.
    for name in ["HEAD", "index", "packed-refs"] {
        if let Some(path) = git(&["rev-parse", "--git-path", name]) {
            if std::path::Path::new(&path).exists() {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
            if std::path::Path::new(&path).exists() {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }
    // Dirty describes tracked changes; unrelated untracked files do not affect it.
    if let Some(files) = git(&["ls-files", "-z"]) {
        for path in files.split('\0').filter(|path| !path.is_empty()) {
            println!("cargo:rerun-if-changed={path}");
        }
    }

    let commit = std::fs::read_to_string(".packaged-commit")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| env::var("GIT_COMMIT_SHA").ok())
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned());
    let dirty = env::var("GIT_DIRTY")
        .ok()
        .or_else(|| {
            git(&["status", "--porcelain", "--untracked-files=no"])
                .map(|status| (!status.is_empty()).to_string())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let timestamp = match env::var("SOURCE_DATE_EPOCH") {
        Ok(value) => OffsetDateTime::from_unix_timestamp(
            value.parse().expect("SOURCE_DATE_EPOCH must be an integer"),
        )
        .expect("SOURCE_DATE_EPOCH must be a supported Unix timestamp"),
        Err(env::VarError::NotPresent) => OffsetDateTime::now_utc(),
        Err(error) => panic!("Invalid SOURCE_DATE_EPOCH: {error}"),
    };
    let timestamp = timestamp
        .format(&Rfc3339)
        .expect("UTC timestamp must format");
    println!("cargo:rustc-env=GIT_COMMIT_SHA={commit}");
    println!("cargo:rustc-env=GIT_DIRTY={dirty}");
    println!("cargo:rustc-env=BUILT_TIME_UTC={timestamp}");
}
