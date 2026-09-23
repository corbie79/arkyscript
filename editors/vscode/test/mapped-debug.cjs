const vscode = require('vscode');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const {spawnSync} = require('node:child_process');
module.exports = async (root, until) => {
  const main = vscode.Uri.file(path.join(root,'examples/debug/main.luax'));
  const worker = vscode.Uri.file(path.join(root,'examples/debug/worker.luax'));
  const points = [new vscode.SourceBreakpoint(new vscode.Location(main,new vscode.Position(1,0)))];
  const trace=path.join(root,'build/editor-test/vscode-mapped-trace.jsonl');
  fs.writeFileSync(trace,'');
  const events=[];
  let currentSession;
  const tracker=vscode.debug.registerDebugAdapterTrackerFactory('tynex',{createDebugAdapterTracker(session){currentSession=session;return {onDidSendMessage:m=>{events.push(m);fs.appendFileSync(trace,JSON.stringify(m)+"\n");}};}});
  const stopped=async()=> (await until(()=>events.find(e=>e.event==='stopped'),'mapped debug did not stop')).body;
  const frame=async(session,threadId)=> (await session.customRequest('stackTrace',{threadId})).stackFrames[0];
  try {
    vscode.debug.addBreakpoints(points);
    assert.equal(await vscode.debug.startDebugging(vscode.workspace.workspaceFolders[0],'Tynex: mapped modules'),true);
    let stop=await stopped();
    const session=currentSession;
    let top=await frame(session,stop.threadId);
    assert.equal(top.source.path,main.fsPath); assert.equal(top.line,2);
    events.length=0;
    await session.customRequest('stepIn',{threadId:stop.threadId});
    stop=await stopped(); top=await frame(session,stop.threadId);
    assert.equal(top.source.path,worker.fsPath); assert.equal(top.line,2);
    assert.equal((await session.customRequest('evaluate',{frameId:top.id,expression:'score + amount',context:'watch'})).result,'15');
    const conditional=new vscode.SourceBreakpoint(new vscode.Location(worker,new vscode.Position(1,0)),true,'score == 15');
    points.push(conditional); vscode.debug.addBreakpoints([conditional]);
    events.length=0;
    await session.customRequest('next',{threadId:stop.threadId});
    stop=await stopped(); top=await frame(session,stop.threadId);
    assert.equal(top.line,3); assert.equal(top.source.path,worker.fsPath);
    assert.equal((await session.customRequest('evaluate',{frameId:top.id,expression:'total',context:'watch'})).result,'15');
    events.length=0;
    await session.customRequest('continue',{threadId:stop.threadId});
    stop=await stopped(); top=await frame(session,stop.threadId);
    assert.equal(top.line,2); assert.equal(top.source.path,worker.fsPath);
    assert.equal((await session.customRequest('evaluate',{frameId:top.id,expression:'score',context:'watch'})).result,'15');
    events.length=0;
    await session.customRequest('continue',{threadId:stop.threadId});
    await until(()=>events.find(e=>e.event==='terminated'),'mapped session did not terminate');
    vscode.debug.removeBreakpoints(points);
    events.length=0;
    const cli=path.join(root,'target/debug',process.platform==='win32'?'tynex.exe':'tynex');
    const result=spawnSync(cli,['bundle','examples/debug/error.luax','--out','build/debug/error'],{cwd:root,encoding:'utf8'});
    assert.equal(result.status,0,result.stderr);
    assert.equal(await vscode.debug.startDebugging(vscode.workspace.workspaceFolders[0],{
      type:'tynex',request:'launch',name:'Tynex mapped error',
      entry:'examples/debug/error.luax',bundleDirectory:'build/debug/error'
    }),true);
    stop=await stopped();
    const errorSession=currentSession;
    const stack=await errorSession.customRequest('stackTrace',{threadId:stop.threadId});
    assert.ok(stack.stackFrames.some(f=>f.source?.path===worker.fsPath && f.line===6),JSON.stringify(stack));
    events.length=0;
    await errorSession.customRequest('continue',{threadId:stop.threadId});
    await until(()=>events.find(e=>e.event==='terminated'),'error session did not terminate');
    events.length=0;
    const importMain=vscode.Uri.file(path.join(root,'examples/imports/main.luax'));
    const importWorker=vscode.Uri.file(path.join(root,'examples/imports/worker.luax'));
    const importPoints=[6,10].map(line=>new vscode.SourceBreakpoint(new vscode.Location(importMain,new vscode.Position(line-1,0))));
    points.push(...importPoints); vscode.debug.addBreakpoints(importPoints);
    assert.equal(await vscode.debug.startDebugging(vscode.workspace.workspaceFolders[0],{
      type:'tynex',request:'launch',name:'Tynex import and try',
      entry:'examples/imports/main.luax',bundleDirectory:'build/debug/vscode-imports'
    }),true);
    stop=await stopped();
    const importSession=currentSession;
    top=await frame(importSession,stop.threadId);
    assert.equal(top.source.path,importMain.fsPath); assert.equal(top.line,6);
    events.length=0;
    await importSession.customRequest('stepIn',{threadId:stop.threadId});
    stop=await stopped(); top=await frame(importSession,stop.threadId);
    assert.equal(top.source.path,importWorker.fsPath); assert.equal(top.line,4);
    assert.equal((await importSession.customRequest('evaluate',{frameId:top.id,expression:'value',context:'watch'})).result,'21');
    vscode.debug.removeBreakpoints([importPoints[0]]);
    await importSession.customRequest('setBreakpoints',{source:{path:importMain.fsPath},breakpoints:[{line:10}]});
    events.length=0;
    await importSession.customRequest('continue',{threadId:stop.threadId});
    stop=await stopped(); top=await frame(importSession,stop.threadId);
    assert.equal(top.source.path,importMain.fsPath); assert.equal(top.line,10);
    assert.equal((await importSession.customRequest('evaluate',{frameId:top.id,expression:'score',context:'watch'})).result,'42');
    vscode.debug.removeBreakpoints(importPoints);
    await importSession.customRequest('setBreakpoints',{source:{path:importMain.fsPath},breakpoints:[]});
    events.length=0;
    await importSession.customRequest('continue',{threadId:stop.threadId});
    await until(()=>events.find(e=>e.event==='terminated'),'import/try session did not terminate');
    fs.writeFileSync(path.join(root,'build/editor-test/vscode-mapped-result.json'),JSON.stringify({
      editor:vscode.version,generatedLineOffset:2,originalBreakpoint:2,stepInto:{file:worker.fsPath,line:2},
      stepOver:3,watch:15,conditionalBreakpoint:'score == 15',moduleErrorLine:6,normalAndErrorTermination:true,relaunch:true,
      staticImportTryElse:{breakpoint:6,stepInto:4,argument:21,elseLine:10,score:42}
    },null,2));
  } finally { tracker.dispose(); vscode.debug.removeBreakpoints(points); await vscode.debug.stopDebugging(); }
};
