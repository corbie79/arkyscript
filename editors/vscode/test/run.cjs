const { spawnSync } = require('node:child_process');
const path = require('node:path');
const root = path.resolve(__dirname, '../../..');
const fs = require('node:fs');
const report = path.join(root,'build/editor-test/vscode-result.json');
const mappedReport = path.join(root,'build/editor-test/vscode-mapped-result.json');
const failure = path.join(root,'build/editor-test/vscode-failure.txt');
for(const file of [report,mappedReport,failure]) fs.rmSync(file,{force:true});
const result = spawnSync(process.env.CODE_BIN || 'code', [
    '--new-window', '--wait', '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes',
    '--user-data-dir', path.join(root,'build/editor-test/profile'),
    '--extensions-dir', path.join(root,'build/editor-test/extensions'),
    '--extensionDevelopmentPath', path.join(root,'editors/vscode'),
    '--extensionTestsPath', path.join(__dirname,'suite.cjs'), root
], { stdio: 'inherit', timeout: 120000, env: {...process.env, TYNEX_TEST_ROOT:root} });
if (fs.existsSync(report)) console.log(fs.readFileSync(report,'utf8'));
if (fs.existsSync(mappedReport)) console.log(fs.readFileSync(mappedReport,'utf8'));
if (fs.existsSync(failure)) console.error(fs.readFileSync(failure,'utf8'));
process.exit(result.status === 0 && fs.existsSync(report) && fs.existsSync(mappedReport) && !fs.existsSync(failure) ? 0 : 1);
