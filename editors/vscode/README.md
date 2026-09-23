# Arky for VS Code

Local development extension for Arky scripts, powered by the independent Rust Tynex compiler. Provides `.arky` source and `.d.arky` declaration highlighting, file breakpoints, and compiler diagnostics through `tynex lsp`. Completion, hover, rename and formatting are not implemented.

Build the Rust CLI in the Tynex repository with `cargo build --locked -p tynex-cli`. Install the generated VSIX, open the repository, and open `examples/main.arky`. Repository settings select `target/debug/tynex`; Windows uses `.exe` automatically.

For another project, set `tynex.serverPath` to the absolute executable path (or put `tynex` on PATH). Enable `tynex.checkLua` to also synchronize `.lua` buffers. Use **Arky: Restart Language Server** after changing these settings. Only trusted, local file workspaces are supported. Policy comes from the nearest saved `tynex.toml` and each file's leading directive.

Install **Local Lua Debugger** (`tomblind.local-lua-debugger-vscode`) to use the repository's launch configuration. The LuaJIT runtime must be built separately; neither the compiler nor runtime is bundled in this extension.

Build this extension with Node.js 22 or newer: `npm ci`, then `npm run package`. The VSIX is local and is not published to the Marketplace. `npm run test:editor` runs the repository's VS Code integration suite using a separate profile under `build/editor-test`; install Local Lua Debugger in that profile first. The test profile disables workspace trust prompts only for this automated test process.

## Mapped debugging

Install `tomblind.local-lua-debugger-vscode` alongside this extension. Use a launch
configuration with `type: "tynex"`, `request: "launch"`, and `entry: "src/main.arky"`.
Save project edits before starting. Tynex checks and bundles the module graph,
then passes all generated files and v3 source maps to the unmodified upstream
adapter. `bundleDirectory`, `mode`, `scriptArguments`, and `stopOnEntry` are optional.
The Tynex executable must already be built and configured in `tynex.serverPath`.

`npm test` checks TextMate token scopes, including types, records, function types,
comments, Unicode strings and incomplete edits. `npm run test:editor` runs the
actual VS Code diagnostics and mapped module/debug/error integration tests.
