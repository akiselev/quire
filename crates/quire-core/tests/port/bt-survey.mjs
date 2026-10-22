// Backtrack-limit survey input (lang.rs module doc, lang::tests::backtrack_survey).
// Usage: node bt-survey.mjs <out.ndjson> <fixtures/monaco>; reads VS Code's built-in
// language-configuration.json files from a local VS Code install and a line corpus from sibling
// checkouts (paths below are this machine's; adjust them).
import fs from 'node:fs';
import path from 'node:path';
const out = process.argv[2];
const pats = new Map(); // src(with inline flags) -> {js: RegExp, origin}
function add(p, origin) {
  if (p == null) return;
  let src, flags = '';
  if (typeof p === 'string') src = p; else { src = p.pattern; flags = p.flags || ''; }
  let re; try { re = new RegExp(src, flags.replace(/[gy]/g, '')); } catch { return; }
  const f = flags.replace(/[^ims]/g, '');
  const q = (f ? `(?${f})` : '') + src;
  if (!pats.has(q)) pats.set(q, { re, origin });
}
function fromConfig(c, origin) {
  const ir = c.indentationRules;
  if (ir) for (const k of ['increaseIndentPattern', 'decreaseIndentPattern', 'indentNextLinePattern', 'unIndentedLinePattern']) add(ir[k], origin);
  for (const r of c.onEnterRules || []) { add(r.beforeText, origin); add(r.afterText, origin); add(r.previousLineText, origin); }
}
function jsonc(s) {
  let o = '', i = 0, inStr = false;
  while (i < s.length) {
    const ch = s[i];
    if (inStr) { o += ch; if (ch === '\\') { o += s[i + 1]; i += 2; continue; } if (ch === '"') inStr = false; i++; continue; }
    if (ch === '"') { inStr = true; o += ch; i++; continue; }
    if (s.startsWith('//', i)) { while (i < s.length && s[i] !== '\n') i++; continue; }
    if (s.startsWith('/*', i)) { i = s.indexOf('*/', i + 2) + 2; continue; }
    o += ch; i++;
  }
  return JSON.parse(o.replace(/,(\s*[}\]])/g, '$1'));
}
// 1. fixtures
const fx = process.argv[3];
for (const f of fs.readdirSync(fx)) {
  if (!f.endsWith('.fixture')) continue;
  for (const line of fs.readFileSync(path.join(fx, f), 'utf8').split('\n')) {
    if (line.startsWith('config: ')) { try { fromConfig(JSON.parse(line.slice(8)), 'fixture:' + f); } catch {} }
  }
}
const nFix = pats.size;
// 2. VS Code built-in extensions
const ext = '/Applications/Visual Studio Code.app/Contents/Resources/app/extensions';
for (const d of fs.readdirSync(ext)) {
  const dir = path.join(ext, d);
  let files = []; try { files = fs.readdirSync(dir).filter(f => /language-configuration.*\.json$/.test(f)); } catch {}
  for (const f of files) { try { fromConfig(jsonc(fs.readFileSync(path.join(dir, f), 'utf8')), 'vscode:' + d); } catch (e) { console.error('skip', d, f, e.message); } }
}
const nVs = pats.size - nFix;
// 3. Svelte (svelte-vscode, from memory) and rust-analyzer's onEnter rules
const voids = '(?:area|base|br|col|embed|hr|img|input|keygen|link|menuitem|meta|param|source|track|wbr)';
fromConfig({
  indentationRules: {
    increaseIndentPattern: `<(?!\\?|${voids}\\b|[^>]*\\/>)([-_\\.A-Za-z0-9]+)(?=\\s|>)\\b[^>]*>(?!.*<\\/\\1>)|<!--(?!.*-->)|\\{[^}"']*$|\\{#(if|each|await|key|snippet)\\b[^}]*\\}\\s*$|\\{:(else|then|catch)\\b[^}]*\\}\\s*$`,
    decreaseIndentPattern: `^\\s*(<\\/(?!html)[-_\\.A-Za-z0-9]+\\b[^>]*>|-->|\\}|\\{\\/(if|each|await|key|snippet)\\}|\\{:(else|then|catch)\\b)`,
  },
  onEnterRules: [
    { beforeText: `<(?!${voids})([_:\\w][_:\\w-.\\d]*)([^/>]*(?!/)>)[^<]*$`, afterText: `^<\\/([_:\\w][_:\\w-.\\d]*)\\s*>`, action: {} },
    { beforeText: `<(?!${voids})(\\w[\\w\\d]*)([^/>]*(?!/)>)[^<]*$`, action: {} },
    { beforeText: `^\\s*\\{#(if|each|await|key|snippet)\\b[^}]*\\}\\s*$`, afterText: `^\\s*\\{\\/(if|each|await|key|snippet)\\}`, action: {} },
  ],
}, 'svelte');
fromConfig({
  onEnterRules: [
    { beforeText: '^\\s*\\/{3}.*$', action: {} }, { beforeText: '^\\s*\\/{2}\\!.*$', action: {} },
    { beforeText: '^\\s*\\/\\*(\\*|\\!)(?!\\/)([^\\*]|\\*(?!\\/))*$', afterText: '^\\s*\\*\\/$', action: {} },
    { beforeText: '^\\s*\\/\\*(\\*|\\!)(?!\\/)([^\\*]|\\*(?!\\/))*$', action: {} },
    { beforeText: '^(\\ \\ )*\\ \\*(\\ ([^\\*]|\\*(?!\\/))*)?$', action: {} },
    { beforeText: '^(\\ \\ )*\\ \\*\\/\\s*$', action: {} },
  ],
}, 'rust-analyzer');
console.error(`patterns: fixtures ${nFix}, vscode ${nVs}, extra ${pats.size - nFix - nVs}`);
// corpus
const corpus = [];
function walk(dir, re, maxFiles) {
  let n = 0; const st = [dir];
  while (st.length && n < maxFiles) {
    const d = st.pop(); let ents = []; try { ents = fs.readdirSync(d, { withFileTypes: true }); } catch { continue; }
    for (const e of ents) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) { if (!/node_modules|target|\.git/.test(e.name)) st.push(p); }
      else if (re.test(e.name) && n < maxFiles) { n++; corpus.push(...fs.readFileSync(p, 'utf8').split(/\r?\n/).slice(0, 400)); }
    }
  }
}
walk(process.env.HOME + '/projects/quire/crates', /\.rs$/, 40);
walk(process.env.HOME + '/projects/svelters/packages', /\.(ts|js)$/, 30);
walk(process.env.HOME + '/projects/svelters/fixtures', /\.svelte$/, 30);
walk('/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.7/lib/python3.7', /\.py$/, 15);
walk('/Library/Developer/CommandLineTools/SDKs/MacOSX10.14.sdk/usr/include', /\.h$/, 30);
walk('/Applications/Visual Studio Code.app/Contents/Resources/app/node_modules/opentype.js', /\.(css|html)$/, 10);
walk(ext, /\.(md|ps1|lua|rb|vb|php|go|cs|java|yaml|json)$/, 0);
let lines = [...new Set(corpus.filter(l => l.length <= 400))];
// Prefixes (beforeText sees the text before the caret) and some long lines.
const extra = [];
for (const l of lines) if (l.length > 8) extra.push(l.slice(0, l.length >> 1));
const longs = [];
for (let i = 0; i + 20 < lines.length && longs.length < 40; i += 97) longs.push(lines.slice(i, i + 20).join(' '));
lines = [...new Set([...lines, ...extra, ...longs])];
console.error(`lines: ${lines.length} (longest ${Math.max(...lines.map(l => l.length))})`);
const w = fs.openSync(out, 'w');
for (const [q, { re, origin }] of pats) {
  const cases = lines.map(l => [l, re.test(l)]);
  fs.writeSync(w, JSON.stringify({ p: q, origin, cases }) + '\n');
}
fs.closeSync(w);
