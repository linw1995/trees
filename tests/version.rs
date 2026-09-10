use std::process::Command;

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
