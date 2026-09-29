const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const tm = require('vscode-textmate');
const onig = require('vscode-oniguruma');
const manifest = require('../package.json');
const language = manifest.contributes.languages.find(language => language.id === 'tynex');
for (const filename of ['main.arky', 'api.d.arky']) {
  assert.ok(language.extensions.some(extension => filename.endsWith(extension)), `${filename} must activate Arky`);
}
assert.deepEqual(language.extensions, ['.arky']);
assert.deepEqual(language.aliases, ['Arky', 'arky']);
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
  check('impl Named for engine.Sprite\nend\ndeclare class Sprite extends Node\nreadonly id: number\nend', [
    [0,'impl','keyword'], [0,'for','keyword'], [2,'declare','keyword'],
    [2,'class','keyword'], [2,'extends','keyword'], [3,'readonly','keyword']]);
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
  check('local score: number? = player?.score\nlocal value = items?[key()]\ncallback?(value)\nplayer?:update(value)\nplayer?:configure {}\nplayer?:label "name"\nlocal mixed = player?.children[index()]?:find("name").score\nlocal ordinary = player:update(value)', [
    [0,'number','storage.type'], [0,'number?','meta.type.annotation'],
    [0,'?.','keyword.operator.optional'], [0,'player','meta.type.annotation',true],
    [1,'?[','keyword.operator.optional'], [1,'key','entity.name.function'],
    [2,'callback','entity.name.function'], [2,'?(','keyword.operator.optional'],
    [3,'?:','keyword.operator.optional'], [3,'update','entity.name.function'],
    [3,'update','entity.name.type',true], [4,'configure','entity.name.type',true],
    [5,'label','entity.name.type',true], [5,'name','string'],
    [6,'find','entity.name.function'], [7,'update','entity.name.function']]);
  check('local callback: (function(number): number)? = nil\nlocal returns: function(number): number? = other\nlocal grouped: (Player)? = nil\n-- player?.field callback?() player?:method()\nlocal text = "player?.field callback?() player?:method()"', [
    [0,'function','keyword'], [0,'number','storage.type'], [0,'?','keyword.operator.type'],
    [0,'?','keyword.operator.optional',true], [0,'nil','constant.language'],
    [1,'?','keyword.operator.type'], [2,'Player','entity.name.type'], [2,'?','keyword.operator.type'],
    [3,'?.','comment'], [3,'?.','keyword.operator.optional',true],
    [4,'?.','string'], [4,'?.','keyword.operator.optional',true]]);
  check('local function factory(): (function(number): number)? return nil end\nlocal function read(): (number)? return nil end', [
    [0,'factory','entity.name.function'], [0,'number','storage.type'], [0,'?','keyword.operator.type'],
    [0,'?','keyword.operator.optional',true], [0,'return','keyword'], [0,'nil','constant.language'],
    [0,'nil','storage.type',true], [0,'end','keyword'],
    [1,'read','entity.name.function'], [1,'number','storage.type'], [1,'?','keyword.operator.type'],
    [1,'?','keyword.operator.optional',true], [1,'return','keyword'], [1,'nil','constant.language'],
    [1,'nil','storage.type',true], [1,'end','keyword']]);
  check('local async function fetch(n: number): number\n  local result = await work(n)\n  return result\nend\nlocal pending: Task<number, string> = fetch(42)\nlocal cb: async function(number): number = fetch\nlocal grouped: Task<Task<number>>? = nil\n-- async await Task<number>\nlocal text = "async await Task<number>"', [
    [0,'async','keyword'], [0,'fetch','entity.name.function'], [0,'n:','variable.parameter'],
    [0,'number','storage.type'], [1,'await','keyword'], [1,'work','entity.name.function'],
    [4,'Task','storage.type'], [4,'number','storage.type'], [4,'string','storage.type'],
    [4,'fetch','meta.type.annotation',true], [5,'async','keyword'], [5,'number','storage.type'],
    [6,'?','keyword.operator.type'], [6,'nil','constant.language'], [7,'await','comment'], [8,'await','string']]);
  check('trait Fetcher\n  fetch: async function(Self): number\nend\nclass Client is Fetcher\n  async function fetch(): number\n    return await work()\n  end\nend\nlocal incomplete: Task<\n number\n> = result\nlocal next: string = "ok"', [
    [1,'fetch','entity.name.function'], [1,'async','keyword'], [1,'Self','entity.name.type'],
    [4,'async','keyword'], [4,'fetch','entity.name.function'], [5,'await','keyword'],
    [8,'Task','storage.type'], [9,'number','storage.type'], [11,'string','storage.type']]);
  check('local pending: Task<Socket> = net.connect("localhost", 8765)\nlocal async function exchange(socket: Socket): string?\n  local count: number = await socket:write("hello")\n  return await socket:read(4096)\nend\nlocal optional: Socket? = nil\nlocal Socket = "Socket"\n-- Socket Task<Socket>', [
    [0,'Task','storage.type'], [0,'Socket','storage.type'], [0,'net','meta.type.annotation',true],
    [1,'Socket','storage.type'], [1,'string','storage.type'], [2,'await','keyword'],
    [2,'write','entity.name.function'], [3,'read','entity.name.function'],
    [5,'Socket','storage.type'], [5,'?','keyword.operator.type'],
    [6,'Socket','storage.type',true], [7,'Socket','comment']]);
  registry.dispose();
  console.log('TextMate highlighting: 19 fixtures passed');
})().catch(e => { console.error(e); process.exitCode = 1; });
