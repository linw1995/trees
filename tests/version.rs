use std::process::Command;

use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[test]
fn build_timestamp_uses_source_epoch_or_head_committer_time() {
    let expected_epoch = match option_env!("SOURCE_DATE_EPOCH") {
        Some(value) => value.parse::<i64>().unwrap(),
        None => {
            let output = Command::new("git")
                .args(["show", "-s", "--format=%ct", "HEAD"])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .output()
                .unwrap();
            if !output.status.success() {
                return;
            }
            String::from_utf8(output.stdout)
                .unwrap()
                .trim()
                .parse::<i64>()
                .unwrap()
        }
    };
    let actual = OffsetDateTime::parse(env!("BUILT_TIME_UTC"), &Rfc3339).unwrap();
    assert_eq!(actual.unix_timestamp(), expected_epoch);
}

#[test]
fn short_version_preserves_the_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_trees"))
        .arg("-V")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!("trees ", env!("CARGO_PKG_VERSION"), "\n")
    );
}

#[test]
fn long_version_uses_embedded_metadata_not_runtime_environment() {
    let output = Command::new(env!("CARGO_BIN_EXE_trees"))
        .arg("--version")
        .env("GIT_COMMIT_SHA", "runtime-value-must-not-be-used")
        .env("GIT_DIRTY", "runtime-value-must-not-be-used")
        .env("SOURCE_DATE_EPOCH", "0")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "trees {}\ncommit: {}\ndirty: {}\nbuild time (UTC): {}\n",
            env!("CARGO_PKG_VERSION"),
            env!("GIT_COMMIT_SHA"),
            env!("GIT_DIRTY"),
            env!("BUILT_TIME_UTC"),
        )
    );
}
