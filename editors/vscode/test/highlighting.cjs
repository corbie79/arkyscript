const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const tm = require('vscode-textmate');
const onig = require('vscode-oniguruma');
(async () => {
  await onig.loadWASM(fs.readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm')).buffer);
  const registry = new tm.Registry({
    onigLib: Promise.resolve({createOnigScanner: p => new onig.OnigScanner(p), createOnigString: s => new onig.OnigString(s)}),
    loadGrammar: async () => tm.parseRawGrammar(fs.readFileSync(path.join(__dirname, '../syntaxes/tynex.tmLanguage.json'), 'utf8'), 'tynex.json')
  });
  const grammar = await registry.loadGrammar('source.tynex');
  function check(source, cases) {
    let stack = tm.INITIAL;
    const lines = source.split('\n');
    const tokens = lines.map(line => { const result = grammar.tokenizeLine(line, stack); stack = result.ruleStack; return result.tokens; });
    for (const [row, word, scope, absent = false] of cases) {
      const column = lines[row].indexOf(word);
      assert.ok(column >= 0, word);
      const found = tokens[row].find(t => t.startIndex <= column && column < t.endIndex);
      assert.equal(found.scopes.some(s => s.startsWith(scope)), !absent, `${row+1}:${word}: ${found.scopes}`);
    }
  }
  check('local function add(score: number, amount: number?): number\n  return score + amount\nend', [
    [0,'function','keyword'], [0,'add','entity.name.function'], [0,'score','variable.parameter'],
    [0,'number','storage.type'], [1,'return','keyword'], [2,'end','keyword']]);
  check('local number = 2\nobj:number()\nif(true) then\nprint("number -- local")\n-- number: string\nlocal s: string = "한글😀"', [
    [0,'number','storage.type',true], [1,'number','storage.type',true], [1,'number','entity.name.function'],
    [2,'if','keyword'], [3,'number','string'], [4,'number','comment'], [5,'string','storage.type']]);
  check('local f: function(number, {name: string}): (boolean, ...number) = other\nlocal n: number = 1', [
    [0,'function','keyword'], [0,'number','storage.type'], [0,'name','variable.other.property'],
    [0,'string','storage.type'], [0,'boolean','storage.type'], [1,'number','storage.type']]);
  check('local r: {\n  name: string, -- comment\n  values: {number}\n} = {}\nlocal x = 1', [
    [1,'name','variable.other.property'], [1,'string','storage.type'], [1,'comment','comment'],
    [2,'number','storage.type'], [4,'local','keyword']]);
  check('--[=[\nlocal x: number\n]=]\nlocal s = [==[number\n-- text]==]\nlocal x: number = 1', [
    [1,'number','comment'], [3,'number','string'], [4,'text','string'], [5,'number','storage.type']]);
  check('local function f(): number return 1 end', [[0,'return','keyword'],[0,'end','keyword']]);
  check('local x: number? =\nlocal next: string = "ok"', [[0,'number','storage.type'],[1,'string','storage.type']]);
  check('import api: any from "api"\ntry\n  print(api.n)\nexcept err then\n  print(err)\nelse\n  print("ok")\nend\ndeclare double: function(number): number', [
    [0,'import','keyword'], [0,'any','storage.type'], [0,'from','keyword'], [0,'"api"','string'],
    [1,'try','keyword'], [3,'except','keyword'], [3,'then','keyword'], [5,'else','keyword'],
    [8,'declare','keyword'], [8,'function','keyword'], [8,'number','storage.type']]);
  check('-- import try except declare\nlocal s="try except import"', [[0,'try','comment'],[1,'try','string']]);
  check('trait Named\n  label: function(Self): string\n  function describe(): string\n    return self:label()\n  end\nend\nclass Player is Named, api.Serializable\n  name: string\n  score: number = 0\n  peer: api.Player?\n  active = true\n  function identity(): Self\n    return self\n  end\nend', [
    [0,'trait','keyword'], [0,'Named','entity.name.type'], [1,'label','entity.name.function'],
    [1,'Self','entity.name.type'], [1,'string','storage.type'], [2,'describe','entity.name.function'],
    [3,'self','variable.language'], [3,'label','entity.name.function'], [6,'class','keyword'],
    [6,'Player','entity.name.type'], [6,'is','keyword'], [6,'Named','entity.name.type'],
    [6,'api.Serializable','entity.name.type'], [7,'string','storage.type'], [8,'number','storage.type'],
    [9,'api.Player','entity.name.type'], [9,'?','keyword.operator.type'], [10,'true','constant.language'],
    [11,'Self','entity.name.type'], [12,'self','variable.language']]);
  check('local player: Player = Player {name = "Mina"}\nlocal maybe: api.Player? = nil\nlocal values: {Player} = {player}\nlocal find: function({[string]: Player}): Player? = lookup', [
    [0,'Player','entity.name.type'], [0,'Mina','string'], [1,'api.Player','entity.name.type'],
    [1,'nil','constant.language'], [2,'Player','entity.name.type'], [3,'string','storage.type'],
    [3,'Player','entity.name.type']]);
  check('obj:Player()\nobj:Named {}\nobj:label "name"\n-- class Player is Named\nlocal text = "trait Named Self"\nlocal function f(value: api.Player?): Self return value end', [
    [0,'Player','entity.name.function'], [0,'Player','entity.name.type',true],
    [1,'Named','entity.name.type',true], [2,'label','entity.name.type',true],
    [3,'Player','comment'], [4,'Named','string'], [5,'api.Player','entity.name.type'],
    [5,'Self','entity.name.type'], [5,'return','keyword'], [5,'value end','entity.name.type',true]]);
  registry.dispose();
  console.log('TextMate highlighting: 12 fixtures passed');
})().catch(e => { console.error(e); process.exitCode = 1; });
