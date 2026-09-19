use std::{fs, process::Command};

use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn init_lint_and_format_workflow() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("demo");

    cargo_bin_cmd!("verbsmith")
        .args(["init", workspace.to_str().unwrap(), "--name", "demo"])
        .assert()
        .success();
    assert!(workspace.join("verbsmith.toml").is_file());
    assert!(workspace.join("requests/example.http").is_file());

    cargo_bin_cmd!("verbsmith")
        .args(["lint", workspace.to_str().unwrap()])
        .assert()
        .success();
    cargo_bin_cmd!("verbsmith")
        .args(["fmt", workspace.to_str().unwrap()])
        .assert()
        .success();
    cargo_bin_cmd!("verbsmith")
        .args(["fmt", workspace.to_str().unwrap(), "--check"])
        .assert()
        .success();
}

#[test]
fn curl_import_is_parseable() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("demo");
    cargo_bin_cmd!("verbsmith")
        .args(["init", workspace.to_str().unwrap()])
        .assert()
        .success();
    let output = workspace.join("requests/imported.http");
    cargo_bin_cmd!("verbsmith")
        .current_dir(&workspace)
        .args([
            "import",
            "curl",
            "curl -X POST -H 'Content-Type: application/json' -d '{\"ok\":true}' https://example.com",
            "--output",
            output.to_str().unwrap(),
        ])
        .assert()
        .success();
    let contents = fs::read_to_string(output).unwrap();
    assert!(contents.contains("POST https://example.com"));
    cargo_bin_cmd!("verbsmith")
        .current_dir(&workspace)
        .args(["lint"])
        .assert()
        .success();
}

#[test]
fn non_tty_without_a_command_prints_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_verbsmith"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("local-first terminal workspace"));
}
