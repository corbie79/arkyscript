const vscode = require('vscode');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
async function until(predicate, message) {
    const end = Date.now()+20000;
    while(Date.now()<end) { const result = await predicate(); if(result) return result; await new Promise(r=>setTimeout(r,50)); }
    throw new Error(message);
}
async function run() {
    const root = process.env.TYNEX_TEST_ROOT || vscode.workspace.workspaceFolders[0].uri.fsPath;
    const extension = vscode.extensions.getExtension('tynex-local.tynex');
    assert.ok(extension);
    const testFile = path.join(root,'build/editor-test/live.arky');
    fs.writeFileSync(testFile,'local value: number = 1\n');
    const document = await vscode.workspace.openTextDocument(testFile);
    await vscode.window.showTextDocument(document);
    assert.equal(document.languageId,'tynex');
    const declarationFile = path.join(root,'build/editor-test/api.d.arky');
    fs.writeFileSync(declarationFile,'declare value: number\n');
    assert.equal((await vscode.workspace.openTextDocument(declarationFile)).languageId,'tynex');
    await extension.activate();
    const replace = async text => {
        const edit = new vscode.WorkspaceEdit();
        edit.replace(document.uri,new vscode.Range(document.positionAt(0),document.positionAt(document.getText().length)),text);
        await vscode.workspace.applyEdit(edit);
    };
    await replace('local value: number = "wrong"\n');
    await until(()=>vscode.languages.getDiagnostics(document.uri).some(d=>d.source==='tynex'), 'unsaved error not published');
    assert.equal(fs.readFileSync(testFile,'utf8'),'local value: number = 1\n');
    await replace('local value: number = 2\n');
    await until(()=>vscode.languages.getDiagnostics(document.uri).length===0, 'fixed error not cleared');
    const dependency = path.join(root,'build/editor-test/live-dependency.arky');
    fs.writeFileSync(dependency,'return 1');
    await replace("local value: number = require('live-dependency')\n");
    await until(()=>vscode.languages.getDiagnostics(document.uri).length===0,'valid dependency rejected');
    fs.writeFileSync(dependency,'return "wrong"');
    await until(()=>vscode.languages.getDiagnostics(document.uri).some(d=>d.source==='tynex'),'external dependency edit not watched');
    fs.writeFileSync(dependency,'return 2');
    await until(()=>vscode.languages.getDiagnostics(document.uri).length===0,'dependency fix not cleared');
    await replace('local value: number = 2\n');
    await document.save();
    const source = vscode.Uri.file(path.join(root,'examples/main.arky'));
    await vscode.window.showTextDocument(await vscode.workspace.openTextDocument(source));
    const breakpoint = new vscode.SourceBreakpoint(new vscode.Location(source,new vscode.Position(2,0)));
    vscode.debug.addBreakpoints([breakpoint]);
    const events=[];
    const tracker=vscode.debug.registerDebugAdapterTrackerFactory('lua-local',{createDebugAdapterTracker(){return {onDidSendMessage:m=>events.push(m)};}});
    try {
        assert.equal(await vscode.debug.startDebugging(vscode.workspace.workspaceFolders[0], 'Rust frontend on LuaJIT: example'),true);
        const stopped = await until(()=>events.find(e=>e.event==='stopped'), 'breakpoint did not stop');
        const session=vscode.debug.activeDebugSession;
        const stack = await session.customRequest('stackTrace',{threadId:stopped.body.threadId});
        const frame = stack.stackFrames[0];
        assert.equal(frame.line,3);
        assert.equal(path.resolve(frame.source.path),source.fsPath);
        const scopes = await session.customRequest('scopes',{frameId:frame.id});
        const variables=[];
        for(const scope of scopes.scopes.filter(s=>!s.expensive)) variables.push(...(await session.customRequest('variables',{variablesReference:scope.variablesReference})).variables);
        assert.ok(variables.some(v=>v.name==='score' && v.value==='10'));
        assert.ok(variables.some(v=>v.name==='amount' && v.value==='5'));
        const evaluated=await session.customRequest('evaluate',{frameId:frame.id,expression:'score + amount',context:'watch'});
        assert.equal(evaluated.result,'15');
        events.length=0;
        await session.customRequest('next',{threadId:stopped.body.threadId});
        await until(()=>events.find(e=>e.event==='stopped'),'step over did not stop');
        const next=(await session.customRequest('stackTrace',{threadId:stopped.body.threadId})).stackFrames[0];
        assert.equal(next.line,4);
        events.length=0;
        await session.customRequest('continue',{threadId:stopped.body.threadId});
        await until(()=>events.find(e=>e.event==='terminated'),'debug session did not terminate');
        fs.writeFileSync(path.join(root,'build/editor-test/vscode-result.json'),JSON.stringify({editor:vscode.version,diagnostics:'unsaved error/fix and external dependency watcher passed',debug:{file:frame.source.path,line:frame.line,locals:['score=10','amount=5'],watch:evaluated.result,stepOverLine:next.line,termination:true}},null,2));
    } finally { tracker.dispose(); vscode.debug.removeBreakpoints([breakpoint]); await vscode.debug.stopDebugging(); }
    await require("./mapped-debug.cjs")(root, until);
};

exports.run = async () => {
    try { await run(); }
    catch(error) {
        const root = process.env.TYNEX_TEST_ROOT || vscode.workspace.workspaceFolders[0].uri.fsPath;
        fs.writeFileSync(path.join(root,'build/editor-test/vscode-failure.txt'),String(error.stack || error));
        throw error;
    }
};
