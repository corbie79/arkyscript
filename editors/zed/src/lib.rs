use zed_extension_api::{self as zed, LanguageServerId, Result};
struct Tynex;
impl zed::Extension for Tynex {
    fn new() -> Self {
        Self
    }
    fn dap_request_kind(
        &mut self,
        adapter_name: String,
        config: serde_json::Value,
    ) -> Result<zed::StartDebuggingRequestArgumentsRequest> {
        if adapter_name != "tynex-emmy" {
            return Err("Unknown Arky debug adapter".into());
        }
        request_kind(&config)
    }
    fn get_dap_binary(
        &mut self,
        adapter_name: String,
        config: zed::DebugTaskDefinition,
        user_binary: Option<String>,
        tree: &zed::Worktree,
    ) -> Result<zed::DebugAdapterBinary> {
        if adapter_name != "tynex-emmy" {
            return Err("Unknown Arky debug adapter".into());
        }
        let mut value: serde_json::Value =
            serde_json::from_str(&config.config).map_err(|e| e.to_string())?;
        let suffix = if zed::current_platform().0 == zed::Os::Windows {
            ".exe"
        } else {
            ""
        };
        let root = tree.root_path();
        let binary = zed::settings::LspSettings::for_worktree("tynex", tree)?.binary;
        let command = user_binary
            .or_else(|| binary.and_then(|b| b.path))
            .or_else(|| tree.which("arky"))
            .unwrap_or_else(|| format!("{root}/target/debug/arky{suffix}"));
        let map = value["tynexMap"]
            .as_str()
            .ok_or("Set tynexMap to the bundle's debug-map.json")?
            .to_owned();
        let backend = value["tynexBackend"]
            .as_str()
            .map(String::from)
            .or_else(|| tree.which("emmylua_dap"))
            .unwrap_or_else(|| format!("{root}/build/editors/bin/emmylua_dap{suffix}"));
        let request = request_kind(&value)?;
        if let Some(object) = value.as_object_mut() {
            object.remove("tynexMap");
            object.remove("tynexBackend");
        }
        Ok(zed::DebugAdapterBinary {
            command: Some(command),
            arguments: vec![
                "dap".into(),
                "--map".into(),
                map,
                "--backend".into(),
                backend,
            ],
            envs: tree.shell_env(),
            cwd: Some(root),
            connection: None,
            request_args: zed::StartDebuggingRequestArguments {
                configuration: value.to_string(),
                request,
            },
        })
    }
    fn language_server_command(
        &mut self,
        id: &LanguageServerId,
        tree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = zed::settings::LspSettings::for_worktree(id.as_ref(), tree)?.binary;
        let command = match binary.as_ref().and_then(|b| b.path.clone()).or_else(|| tree.which("arky")) {
            Some(command) => command,
            None if tree.read_text_file("Cargo.toml").is_ok() => format!(
                "{}/target/debug/arky{}", tree.root_path(),
                if zed::current_platform().0 == zed::Os::Windows { ".exe" } else { "" }
            ),
            None => return Err("Set lsp.tynex.binary.path to the Arky executable, or install arky on PATH. Open the project folder to use its settings.".into()),
        };
        Ok(zed::Command {
            command,
            args: vec!["lsp".into()],
            env: tree.shell_env(),
        })
    }
}
fn request_kind(value: &serde_json::Value) -> Result<zed::StartDebuggingRequestArgumentsRequest> {
    match value["request"].as_str() {
        Some("attach") => Ok(zed::StartDebuggingRequestArgumentsRequest::Attach),
        Some("launch") => Ok(zed::StartDebuggingRequestArgumentsRequest::Launch),
        _ => Err("Arky debugging requires launch or attach".into()),
    }
}
zed::register_extension!(Tynex);

#[cfg(test)]
mod tests {
    use super::*;
    use zed::Extension;
    #[test]
    fn zed_can_determine_session_kind_before_launching_adapter() {
        let mut extension = Tynex;
        for request in ["launch", "attach"] {
            assert!(
                extension
                    .dap_request_kind("tynex-emmy".into(), serde_json::json!({"request":request}))
                    .is_ok()
            );
        }
        assert!(
            extension
                .dap_request_kind("unknown".into(), serde_json::json!({"request":"launch"}))
                .is_err()
        );
        assert!(request_kind(&serde_json::json!({"request":"restart"})).is_err());
    }
}
