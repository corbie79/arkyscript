use std::{
    fs,
    process::{Command, Output},
};
use tempfile::TempDir;

fn check(dir: &TempDir, name: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tynex"))
        .current_dir(dir.path())
        .args(["check", name])
        .args(args)
        .output()
        .unwrap()
}
fn file(dir: &TempDir, name: &str, source: &str) {
    let path = dir.path().join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}
#[test]
fn cli_overrides_directive_and_last_matching_glob_overrides_project_default() {
    let dir = TempDir::new().unwrap();
    file(
        &dir,
        "tynex.toml",
        "type_mode = 'strict'\n[[overrides]]\nfiles=['**/*.arky']\nmode='strict'\n[[overrides]]\nfiles=['new/**/*.arky']\nmode='gradual'\n",
    );
    file(&dir, "new/main.arky", "local function f(x) return x end");
    assert!(check(&dir, "new/main.arky", &[]).status.success());
    file(
        &dir,
        "new/main.arky",
        "--!typecheck strict\nlocal function f(x) return x end",
    );
    assert!(!check(&dir, "new/main.arky", &[]).status.success());
    assert!(
        check(&dir, "new/main.arky", &["--mode", "gradual"])
            .status
            .success()
    );
}
#[test]
fn nearest_config_and_explicit_project_are_deterministic() {
    let dir = TempDir::new().unwrap();
    file(&dir, "tynex.toml", "type_mode = 'strict'\n");
    file(&dir, "nested/tynex.toml", "type_mode = 'gradual'\n");
    file(&dir, "nested/main.arky", "local function f(x) return x end");
    assert!(check(&dir, "nested/main.arky", &[]).status.success());
    assert!(
        !check(&dir, "nested/main.arky", &["--project", "tynex.toml"])
            .status
            .success()
    );
}
#[test]
fn malformed_configuration_does_not_fall_back_to_a_looser_mode() {
    let dir = TempDir::new().unwrap();
    file(&dir, "main.arky", "print(1)");
    for config in [
        "type_mode = 'typo'",
        "type_mdoe = 'strict'",
        "[[overrides]]\nfiles=['[']\nmode='strict'",
    ] {
        file(&dir, "tynex.toml", config);
        assert!(!check(&dir, "main.arky", &[]).status.success());
    }
}
#[test]
fn comments_in_long_strings_are_not_mode_directives() {
    let dir = TempDir::new().unwrap();
    file(
        &dir,
        "main.arky",
        "local s=[=[\n--!typecheck strict\n]=]\nlocal function f(x) return x end",
    );
    assert!(check(&dir, "main.arky", &[]).status.success());
}
#[test]
fn json_type_diagnostics_have_code_mode_and_original_location() {
    let dir = TempDir::new().unwrap();
    file(&dir, "main.arky", "-- hello\nlocal n: number = 'bad'");
    let output = check(&dir, "main.arky", &["--format", "json"]);
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["diagnostics"][0]["code"], "T001");
    assert_eq!(value["diagnostics"][0]["line"], 2);
    assert_eq!(value["diagnostics"][0]["mode"], "gradual");
}
#[test]
fn retired_teal_and_invalid_legacy_selection_are_explicit_errors() {
    let dir = TempDir::new().unwrap();
    file(&dir, "main.tl", "print(1)");
    file(&dir, "main.arky", "print(1)");
    let output = check(&dir, "main.tl", &[]);
    assert!(String::from_utf8_lossy(&output.stderr).contains("Teal .tl support has been retired"));
    assert!(
        !check(&dir, "main.arky", &["--mode", "legacy"])
            .status
            .success()
    );
}

#[test]
fn arky_is_the_canonical_typed_extension_and_retired_extensions_are_not_aliases() {
    let dir = TempDir::new().unwrap();
    let source = "local value: number = 42";
    file(&dir, "main.arky", source);
    assert!(check(&dir, "main.arky", &[]).status.success());
    for retired in ["main.tlj", "main.luax"] {
        file(&dir, retired, source);
        let output = check(&dir, retired, &[]);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("expected a .lua or .arky source file")
        );
    }
}

#[test]
fn bundled_class_maps_and_runtime_manifest_share_helper_offsets() {
    let dir = TempDir::new().unwrap();
    let source =
        "class Item\n function fail(): ()\n  error('method failure')\n end\nend\nItem {}:fail()\n";
    file(&dir, "main.arky", source);
    let result = Command::new(env!("CARGO_BIN_EXE_tynex"))
        .current_dir(dir.path())
        .args(["bundle", "main.arky", "--out", "bundle"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let directory = dir.path().join("bundle");
    let map: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("debug-map.json")).unwrap()).unwrap();
    let positions = map["files"][0]["lines"].as_array().unwrap();
    assert!(positions[..3].iter().all(serde_json::Value::is_null));
    assert_eq!(positions[3]["line"], 1);
    assert_eq!(positions[5]["line"], 3);
    let generated = fs::read_to_string(directory.join("module-0.lua")).unwrap();
    assert_eq!(
        generated.lines().nth(5).unwrap(),
        "  error('method failure')"
    );
    let manifest = fs::read_to_string(directory.join("manifest.lua")).unwrap();
    assert!(manifest.contains("[4]=1,"));
    assert!(manifest.contains("[6]=3,"));
    assert!(!manifest.contains("[3]=1,"));
}
