use assert_cmd::Command;
use predicates::prelude::*;

fn deriva() -> Command {
    Command::cargo_bin("deriva").unwrap()
}

#[test]
fn help_flag() {
    deriva()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Deriva command-line client"));
}

#[test]
fn unknown_subcommand() {
    deriva().arg("foobar").assert().failure();
}

// --- Integration tests (require running server) ---

#[test]
#[ignore]
fn put_from_stdin() {
    let output = deriva()
        .arg("put")
        .arg("-")
        .write_stdin("test data")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let hex = String::from_utf8(output).unwrap();
    assert_eq!(hex.trim().len(), 64);
}

#[test]
#[ignore]
fn put_and_get_roundtrip() {
    let put_out = deriva()
        .arg("put")
        .arg("-")
        .write_stdin("roundtrip test")
        .output()
        .unwrap();
    let addr = String::from_utf8(put_out.stdout).unwrap();
    let addr = addr.trim();
    deriva()
        .arg("get")
        .arg(addr)
        .assert()
        .success()
        .stdout("roundtrip test");
}

#[test]
#[ignore]
fn recipe_and_resolve() {
    let put_out = deriva()
        .arg("put")
        .arg("-")
        .write_stdin("hello")
        .output()
        .unwrap();
    let leaf_addr = String::from_utf8(put_out.stdout).unwrap();
    let leaf_addr = leaf_addr.trim();

    let recipe_out = deriva()
        .args(["recipe", "uppercase", "1.0.0", "--input", leaf_addr])
        .output()
        .unwrap();
    let recipe_addr = String::from_utf8(recipe_out.stdout).unwrap();
    let recipe_addr = recipe_addr.trim();

    deriva()
        .args(["resolve", recipe_addr])
        .assert()
        .success()
        .stdout(predicate::str::contains("uppercase/1.0.0"))
        .stdout(predicate::str::contains(leaf_addr));
}

#[test]
#[ignore]
fn status_command() {
    deriva()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("recipes:"))
        .stdout(predicate::str::contains("hit rate:"));
}

#[test]
#[ignore]
fn invalidate_command() {
    deriva()
        .args(["invalidate", &"00".repeat(32)])
        .assert()
        .success()
        .stdout(predicate::str::contains("was_cached:"));
}
