'use strict';
const vscode = require('vscode');
const path = require('node:path');
const fs = require('node:fs/promises');
const execFile = require('node:util').promisify(require('node:child_process').execFile);
const { LanguageClient } = require('vscode-languageclient/node');
let client;
async function activate(context) {
    // Own the launch configuration so the upstream adapter receives the complete
    // generated file list after compilation, including newly added modules.
    const debuggerExtension = () => {
        const extension = vscode.extensions.getExtension('tomblind.local-lua-debugger-vscode');
        if (!extension) throw new Error('Install Local Lua Debugger (tomblind.local-lua-debugger-vscode) for Tynex debugging.');
        return extension;
    };
    context.subscriptions.push(vscode.debug.registerDebugConfigurationProvider('tynex', {
        async resolveDebugConfigurationWithSubstitutedVariables(folder, config) {
            const root = folder?.uri.fsPath;
            if (!root) throw new Error('Open a project folder for Tynex debugging.');
            if (!config.entry) throw new Error('Set entry to the .tlj or .lua entry file.');
            if (vscode.workspace.textDocuments.some(d=>d.isDirty && !d.isUntitled && d.uri.fsPath.startsWith(root+path.sep))) {
                throw new Error('Save project changes before building the Tynex debug bundle.');
            }
            let cli = vscode.workspace.getConfiguration('tynex',folder.uri).get('serverPath','tynex').replaceAll('${workspaceFolder}',root);
            if (!path.isAbsolute(cli) && /[\\/]/.test(cli)) cli = path.resolve(root,cli);
            if (process.platform === 'win32' && path.isAbsolute(cli) && !path.extname(cli)) cli += '.exe';
            const output = path.resolve(root,config.bundleDirectory || 'build/debug/vscode');
            const args = ['bundle',path.resolve(root,config.entry),'--out',output];
            if (config.mode) args.push('--mode',config.mode);
            try { await execFile(cli,args,{cwd:root,maxBuffer:4*1024*1024}); }
            catch(error) { throw new Error(error.stderr || error.message); }
            const maps = JSON.parse(await fs.readFile(path.join(output,'debug-map.json'),'utf8'));
            return {...config, cwd:root, workspacePath:root, extensionPath:debuggerExtension().extensionPath,
                program:{command:cli,communication:'pipe'}, args:['exec-bundle',output,'--debug','--',...(config.scriptArguments || [])],
                scriptFiles:maps.files.map(file=>file.generated), stopOnEntry:config.stopOnEntry || false};
        }
    }));
    context.subscriptions.push(vscode.debug.registerDebugAdapterDescriptorFactory('tynex', {
        createDebugAdapterDescriptor() {
            return new vscode.DebugAdapterExecutable(process.execPath,
                [path.join(debuggerExtension().extensionPath,'extension/debugAdapter.js')],
                {env:{ELECTRON_RUN_AS_NODE:'1'}});
        }
    }));
    const start = async () => {
        await client?.dispose();
        const config = vscode.workspace.getConfiguration('tynex');
        const folder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
        let command = config.get('serverPath', 'tynex');
        if (command.includes('${workspaceFolder}')) {
            if (!folder) throw new Error('tynex.serverPath requires an open workspace folder.');
            command = command.replaceAll('${workspaceFolder}', folder);
        }
        if (folder && !path.isAbsolute(command) && /[\\/]/.test(command)) command = path.resolve(folder, command);
        if (process.platform === 'win32' && path.isAbsolute(command) && !path.extname(command)) command += '.exe';
        const selector = [{ scheme: 'file', language: 'tynex' }];
        if (config.get('checkLua', false)) selector.push({ scheme: 'file', language: 'lua' });
        client = new LanguageClient('tynex', 'Tynex', { command, args: ['lsp'], options: { cwd: folder } }, {
            documentSelector: selector,
        });
        await client.start();
    };
    context.subscriptions.push(vscode.commands.registerCommand('tynex.restart', start));
    // Lua activation alone must not launch an unconfigured server in unrelated projects.
    if (vscode.workspace.getConfiguration('tynex').get('checkLua', false)
        || vscode.workspace.textDocuments.some(d => d.languageId === 'tynex')) await start();
    context.subscriptions.push(vscode.workspace.onDidOpenTextDocument(async doc => {
        if (doc.languageId === 'tynex' && !client) {
            try { await start(); } catch (error) { void vscode.window.showErrorMessage(String(error)); }
        }
    }));
}
async function deactivate() { await client?.dispose(); client = undefined; }
module.exports = { activate, deactivate };
