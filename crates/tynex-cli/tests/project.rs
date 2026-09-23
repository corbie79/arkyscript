use std::{
    fs,
    process::{Command, Output},
};
use tempfile::TempDir;
fn file(dir: &TempDir, name: &str, source: &str) {
    let p = dir.path().join(name);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, source).unwrap();
}
fn check(dir: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tynex"))
        .current_dir(dir.path())
        .arg("check")
        .args(args)
        .output()
        .unwrap()
}
fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn failure(output: Output, expected: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn imports_apply_each_files_policy_even_when_entry_cli_mode_is_gradual() {
    let d = TempDir::new().unwrap();
    file(&d, "main.arky", "local lib=require('lib') print(lib)");
    file(
        &d,
        "lib.arky",
        "--!typecheck strict\nlocal function f(x) return x end return f",
    );
    let o = check(&d, &["main.arky", "--mode", "gradual", "--format", "json"]);
    assert!(!o.status.success());
    let json: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(json["diagnostics"][0]["mode"], "strict");
    assert!(
        json["diagnostics"][0]["file"]
            .as_str()
            .unwrap()
            .ends_with("lib.arky")
    );
}
#[test]
fn inferred_exports_cross_multiple_dependency_levels() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "main.arky",
        "--!typecheck strict\nlocal m=require('api') local n:number=m.add(2,3)",
    );
    file(&d, "api.arky", "return require('nested.maths')");
    file(
        &d,
        "nested/maths/init.arky",
        "local function add(a:number,b:number):number return a+b end return {add=add}",
    );
    success(check(&d, &["main.arky"]));
    file(&d, "main.arky", "local m=require('api') m.add('bad',3)");
    failure(check(&d, &["main.arky"]), "T001");
}
#[test]
fn missing_ambiguous_and_cyclic_modules_are_structured_errors() {
    let d = TempDir::new().unwrap();
    file(&d, "main.arky", "local m=require('lib')");
    failure(check(&d, &["main.arky"]), "module 'lib' not found");
    file(&d, "lib.arky", "return require('main')");
    failure(check(&d, &["main.arky"]), "cyclic module dependency");
    file(&d, "lib.lua", "return 1");
    failure(check(&d, &["main.arky"]), "ambiguous module");
}
#[test]
fn whole_project_check_finds_unreachable_errors_and_respects_exclusions() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "tynex.toml",
        "include=['src/**/*.arky']\nexclude=['src/ignored/**']\nmodule_paths=['src']",
    );
    file(&d, "src/main.arky", "print(1)");
    file(&d, "src/bad.arky", "local n:number='bad'");
    file(&d, "src/ignored/bad.arky", "local =");
    success(check(&d, &["src/main.arky"]));
    failure(check(&d, &["--project", "tynex.toml"]), "bad.arky");
    file(&d, "src/bad.arky", "return 1");
    success(check(&d, &["--project", "tynex.toml"]));
    success(check(&d, &[]));
}

#[test]
fn default_project_scan_includes_arky_scripts_and_declarations() {
    let d = TempDir::new().unwrap();
    file(&d, "tynex.toml", "");
    file(&d, "main.arky", "local n:number=1");
    file(&d, "legacy.lua", "return 1");
    file(&d, "retired.tlj", "local =");
    file(&d, "retired.luax", "local =");
    file(&d, "unused.arky", "local n:number='bad'");
    failure(check(&d, &[]), "unused.arky");
    file(&d, "unused.arky", "return 1");
    success(check(&d, &[]));
    file(&d, "legacy.d.arky", "local n:number=1");
    failure(check(&d, &[]), "legacy.d.arky");
    file(&d, "legacy.d.arky", "export: number");
    success(check(&d, &[]));
}
#[test]
fn dependencies_are_checked_even_if_excluded_from_project_roots() {
    let d = TempDir::new().unwrap();
    file(&d, "tynex.toml", "exclude=['hidden/**']");
    file(&d, "main.arky", "local m=require('hidden.bad')");
    file(&d, "hidden/bad.arky", "local n:number='bad'");
    failure(check(&d, &["--project", "tynex.toml"]), "T001");
}
#[test]
fn nested_configs_set_dependency_policy_but_explicit_project_is_consistent() {
    let d = TempDir::new().unwrap();
    file(&d, "tynex.toml", "type_mode='gradual'");
    file(&d, "lib/tynex.toml", "type_mode='strict'");
    file(&d, "main.arky", "local m=require('lib.api')");
    file(
        &d,
        "lib/api.arky",
        "local function f(x) return x end return f",
    );
    failure(check(&d, &["main.arky"]), "T002");
    success(check(&d, &["main.arky", "--project", "tynex.toml"]));
}
#[test]
fn registered_dynamic_modules_are_checked_before_runtime() {
    let d = TempDir::new().unwrap();
    file(&d, "tynex.toml", "dynamic_modules=['plugin']");
    file(
        &d,
        "main.arky",
        "local name='plugin' local m:any=require(name)",
    );
    file(
        &d,
        "plugin.arky",
        "--!typecheck strict\nlocal n:number='bad'",
    );
    failure(check(&d, &["main.arky"]), "T001");
}
#[test]
fn declarations_are_syntax_only_and_invalid_configuration_never_weakens_checking() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "legacy.d.arky",
        "export: {add:function(number,number):number}",
    );
    success(check(&d, &["legacy.d.arky"]));
    for text in [
        "include=[]",
        "module_paths=[]",
        "dynamic_modules=['../bad']",
        "external_modules=['[']",
    ] {
        file(&d, "tynex.toml", text);
        assert!(!check(&d, &["legacy.d.arky"]).status.success());
    }
}
#[test]
fn require_shadowing_and_comments_do_not_discover_fake_modules() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "main.arky",
        "-- require('missing')\nlocal require=function(n:string):number return 1 end local n=require('missing')",
    );
    success(check(&d, &["main.arky"]));
}
#[test]
fn module_roots_are_ordered_and_host_modules_require_explicit_boundaries() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "tynex.toml",
        "module_paths=['first','second']\nexternal_modules=['host.*']",
    );
    file(&d, "first/api.arky", "return 1");
    file(&d, "second/api.arky", "return 's'");
    file(
        &d,
        "main.arky",
        "--!typecheck strict\nlocal n:number=require('api') local host:any=require('host.ui')",
    );
    success(check(&d, &["main.arky"]));
}
#[test]
fn an_imported_require_alias_is_resolved_to_a_fixed_point() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "main.arky",
        "--!typecheck strict\nlocal load=require('loader') local n:number=load('data')",
    );
    file(&d, "loader.arky", "return require");
    file(&d, "data.arky", "return 42");
    success(check(&d, &["main.arky"]));
}
