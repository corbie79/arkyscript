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
    file(&d, "main.luax", "local lib=require('lib') print(lib)");
    file(
        &d,
        "lib.luax",
        "--!typecheck strict\nlocal function f(x) return x end return f",
    );
    let o = check(&d, &["main.luax", "--mode", "gradual", "--format", "json"]);
    assert!(!o.status.success());
    let json: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(json["diagnostics"][0]["mode"], "strict");
    assert!(
        json["diagnostics"][0]["file"]
            .as_str()
            .unwrap()
            .ends_with("lib.luax")
    );
}
#[test]
fn inferred_exports_cross_multiple_dependency_levels() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "main.luax",
        "--!typecheck strict\nlocal m=require('api') local n:number=m.add(2,3)",
    );
    file(&d, "api.luax", "return require('nested.maths')");
    file(
        &d,
        "nested/maths/init.luax",
        "local function add(a:number,b:number):number return a+b end return {add=add}",
    );
    success(check(&d, &["main.luax"]));
    file(&d, "main.luax", "local m=require('api') m.add('bad',3)");
    failure(check(&d, &["main.luax"]), "T001");
}
#[test]
fn missing_ambiguous_and_cyclic_modules_are_structured_errors() {
    let d = TempDir::new().unwrap();
    file(&d, "main.luax", "local m=require('lib')");
    failure(check(&d, &["main.luax"]), "module 'lib' not found");
    file(&d, "lib.luax", "return require('main')");
    failure(check(&d, &["main.luax"]), "cyclic module dependency");
    file(&d, "lib.lua", "return 1");
    failure(check(&d, &["main.luax"]), "ambiguous module");
}
#[test]
fn whole_project_check_finds_unreachable_errors_and_respects_exclusions() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "tynex.toml",
        "include=['src/**/*.luax']\nexclude=['src/ignored/**']\nmodule_paths=['src']",
    );
    file(&d, "src/main.luax", "print(1)");
    file(&d, "src/bad.luax", "local n:number='bad'");
    file(&d, "src/ignored/bad.luax", "local =");
    success(check(&d, &["src/main.luax"]));
    failure(check(&d, &["--project", "tynex.toml"]), "bad.luax");
    file(&d, "src/bad.luax", "return 1");
    success(check(&d, &["--project", "tynex.toml"]));
    success(check(&d, &[]));
}

#[test]
fn default_project_scan_includes_luax_scripts_and_declarations() {
    let d = TempDir::new().unwrap();
    file(&d, "tynex.toml", "");
    file(&d, "main.luax", "local n:number=1");
    file(&d, "legacy.lua", "return 1");
    file(&d, "retired.tlj", "local =");
    file(&d, "unused.luax", "local n:number='bad'");
    failure(check(&d, &[]), "unused.luax");
    file(&d, "unused.luax", "return 1");
    success(check(&d, &[]));
    file(&d, "legacy.d.luax", "local n:number=1");
    failure(check(&d, &[]), "legacy.d.luax");
    file(&d, "legacy.d.luax", "export: number");
    success(check(&d, &[]));
}
#[test]
fn dependencies_are_checked_even_if_excluded_from_project_roots() {
    let d = TempDir::new().unwrap();
    file(&d, "tynex.toml", "exclude=['hidden/**']");
    file(&d, "main.luax", "local m=require('hidden.bad')");
    file(&d, "hidden/bad.luax", "local n:number='bad'");
    failure(check(&d, &["--project", "tynex.toml"]), "T001");
}
#[test]
fn nested_configs_set_dependency_policy_but_explicit_project_is_consistent() {
    let d = TempDir::new().unwrap();
    file(&d, "tynex.toml", "type_mode='gradual'");
    file(&d, "lib/tynex.toml", "type_mode='strict'");
    file(&d, "main.luax", "local m=require('lib.api')");
    file(
        &d,
        "lib/api.luax",
        "local function f(x) return x end return f",
    );
    failure(check(&d, &["main.luax"]), "T002");
    success(check(&d, &["main.luax", "--project", "tynex.toml"]));
}
#[test]
fn registered_dynamic_modules_are_checked_before_runtime() {
    let d = TempDir::new().unwrap();
    file(&d, "tynex.toml", "dynamic_modules=['plugin']");
    file(
        &d,
        "main.luax",
        "local name='plugin' local m:any=require(name)",
    );
    file(
        &d,
        "plugin.luax",
        "--!typecheck strict\nlocal n:number='bad'",
    );
    failure(check(&d, &["main.luax"]), "T001");
}
#[test]
fn declarations_are_syntax_only_and_invalid_configuration_never_weakens_checking() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "legacy.d.luax",
        "export: {add:function(number,number):number}",
    );
    success(check(&d, &["legacy.d.luax"]));
    for text in [
        "include=[]",
        "module_paths=[]",
        "dynamic_modules=['../bad']",
        "external_modules=['[']",
    ] {
        file(&d, "tynex.toml", text);
        assert!(!check(&d, &["legacy.d.luax"]).status.success());
    }
}
#[test]
fn require_shadowing_and_comments_do_not_discover_fake_modules() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "main.luax",
        "-- require('missing')\nlocal require=function(n:string):number return 1 end local n=require('missing')",
    );
    success(check(&d, &["main.luax"]));
}
#[test]
fn module_roots_are_ordered_and_host_modules_require_explicit_boundaries() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "tynex.toml",
        "module_paths=['first','second']\nexternal_modules=['host.*']",
    );
    file(&d, "first/api.luax", "return 1");
    file(&d, "second/api.luax", "return 's'");
    file(
        &d,
        "main.luax",
        "--!typecheck strict\nlocal n:number=require('api') local host:any=require('host.ui')",
    );
    success(check(&d, &["main.luax"]));
}
#[test]
fn an_imported_require_alias_is_resolved_to_a_fixed_point() {
    let d = TempDir::new().unwrap();
    file(
        &d,
        "main.luax",
        "--!typecheck strict\nlocal load=require('loader') local n:number=load('data')",
    );
    file(&d, "loader.luax", "return require");
    file(&d, "data.luax", "return 42");
    success(check(&d, &["main.luax"]));
}
