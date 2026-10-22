// Usage: node port.mjs <vscode-src-root> <relative test path> <out fixture> <vscode sha>
// Rewrites the test's imports to globals (unknown names become `Unsupported` proxies), runs it
// under the recording prelude, and writes a fixture plus a JSON summary next to it.
import { readFileSync, writeFileSync, mkdtempSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, dirname, basename } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

const [root, rel, out, sha] = process.argv.slice(2);
const src = readFileSync(join(root, rel), 'utf8');
const here = dirname(fileURLToPath(import.meta.url));

// Collect imported names and drop import lines.
const known = new Set(['assert', 'Position', 'Range', 'Selection', 'CoreNavigationCommands', 'CoreEditingCommands',
  'withTestCodeEditor', 'withAsyncTestCodeEditor', 'createTextModel', 'instantiateTextModel', 'createCodeEditorServices',
  'DisposableStore', 'ILanguageConfigurationService', 'ILanguageService', 'ensureNoDisposablesAreLeakedInTestSuite',
  'EndOfLinePreference', 'instantiateTestCodeEditor', 'TextModel', 'assertCursor', 'testCommand']);
const names = new Set();
const body = src.replace(/^import\s+([\s\S]*?)\s+from\s+'[^']+';\s*$/gm, (_m, spec) => {
  spec = spec.replace(/^type\s+/, '');
  const star = spec.match(/\*\s+as\s+(\w+)/);
  if (star) names.add(star[1]);
  const braces = spec.match(/\{([\s\S]*)\}/);
  if (braces) {
    for (let part of braces[1].split(',')) {
      part = part.trim().replace(/^type\s+/, '');
      if (!part) continue;
      const m = part.match(/(?:\w+\s+as\s+)?(\w+)$/);
      if (m) names.add(m[1]);
    }
  }
  const def = spec.match(/^(\w+)\s*(,|$)/);
  if (def) names.add(def[1]);
  return '';
}).replace(/^import\s+'[^']+';\s*$/gm, '').replace(/@I\w+\s+(?=\w+\s*:)/g, '').replace(/^function assertCursor\(/m, 'function __upstream_assertCursor(')
  .replace(/^(\s*)function testApplyEditsFails\(/m, '$1const testApplyEditsFails = globalThis.__testApplyEditsFails;\n$1function __upstream_testApplyEditsFails(');
let header = '';
for (const n of names) {
  if (!known.has(n)) header += `const ${n} = globalThis.__resolve(${JSON.stringify(n)});\n`;
}
const tmp = mkdtempSync(join(tmpdir(), 'quire-port-'));
const tsPath = join(tmp, basename(rel));
writeFileSync(tsPath, header + body + `\nprocess.on('exit', () => { require_fs().writeFileSync(${JSON.stringify(join(tmp, 'out.json'))}, JSON.stringify(globalThis.__results)); });\nfunction require_fs() { return process.getBuiltinModule('node:fs'); }\n`);
try {
  execFileSync(process.execPath, ['--import', join(here, 'prelude.mjs'), '--experimental-transform-types', '--no-warnings', tsPath], { stdio: ['ignore', 'inherit', 'inherit'] });
} catch (e) {
  console.error('run failed', e.message);
}
const results = JSON.parse(readFileSync(join(tmp, 'out.json'), 'utf8'));

// ---- config mapping ----
const PLAIN = { autoClosingPairs: [], surroundingPairs: [], brackets: [], lineComment: null, blockComment: null };
function mapConfig(t) {
  const cfg = { ...PLAIN };
  const features = [];
  const lc = t.languageId ? t.languageConfig : null;
  if (t.languageId && !lc) features.push(`language ${t.languageId} without configuration`);
  if (lc) {
    for (const [k, v] of Object.entries(lc)) {
      if (k === 'autoClosingPairs') {
        cfg.autoClosingPairs = v.map((p) => Array.isArray(p) ? [p[0], p[1]] : [p.open, p.close]);
        if (v.some((p) => p.notIn)) features.push('autoClosingPairs.notIn (needs tokens)');
        if (v.some((p) => p.beforeText)) features.push('autoClosingPairs.beforeText');
      } else if (k === 'surroundingPairs') {
        cfg.surroundingPairs = v.map((p) => Array.isArray(p) ? [p[0], p[1]] : [p.open, p.close]);
      } else if (k === 'brackets') {
        cfg.brackets = v.map((p) => [p[0], p[1]]);
      } else if (k === 'comments') {
        cfg.lineComment = v.lineComment ?? null;
        cfg.blockComment = v.blockComment ? [v.blockComment[0], v.blockComment[1]] : null;
      } else if (k === 'autoCloseBefore') {
        cfg.autoCloseBefore = v;
      } else {
        features.push(`language ${k}`);
      }
    }
    if (!lc.surroundingPairs) cfg.surroundingPairs = cfg.autoClosingPairs;
  }
  const mo = t.modelOpts || {};
  for (const [k, v] of Object.entries(mo)) {
    if (k === 'tabSize') cfg.tabSize = v;
    else if (k === 'indentSize') cfg.indentSize = v;
    else if (k === 'insertSpaces') cfg.insertSpaces = v;
    else if (k === 'trimAutoWhitespace') { if (v) features.push('trimAutoWhitespace'); }
    else if (k === 'detectIndentation' || k === 'defaultEOL' || k === 'isForSimpleWidget') {}
    else features.push(`model option ${k}`);
  }
  if (mo.tabSize !== undefined && mo.indentSize === undefined) cfg.indentSize = mo.tabSize;
  const eo = t.editorOpts || {};
  for (const [k, v] of Object.entries(eo)) {
    switch (k) {
      case 'tabSize': cfg.tabSize = v; break;
      case 'indentSize': cfg.indentSize = v; break;
      case 'insertSpaces': cfg.insertSpaces = v; break;
      case 'wordSeparators': cfg.wordSeparators = v; break;
      case 'autoClosingBrackets': cfg.autoClosingBrackets = v; break;
      case 'autoClosingQuotes': cfg.autoClosingQuotes = v; break;
      case 'autoClosingOvertype': cfg.autoClosingOvertype = v; break;
      case 'autoClosingDelete': if (v === 'never') cfg.autoClosingDelete = false; else if (v === 'always') features.push('autoClosingDelete always'); break;
      case 'autoSurround': if (v === 'never') cfg.autoSurround = false; else if (v !== 'languageDefined') features.push(`autoSurround ${v}`); break;
      case 'autoIndent': if (v === 'none') cfg.autoIndent = false; else if (v === 'keep') { cfg.brackets = []; } break;
      case 'useTabStops': cfg.useTabStops = v; break;
      case 'emptySelectionClipboard': cfg.emptySelectionClipboard = v; break;
      case 'trimAutoWhitespace': if (v) features.push('trimAutoWhitespace'); break;
      case 'readOnly': cfg.readOnly = v; break;
      case 'multiCursorPaste': if (v !== 'spread') features.push(`multiCursorPaste ${v}`); break;
      case 'stickyTabStops': if (v) features.push('stickyTabStops'); break;
      case 'serviceCollection': break;
      case 'wordWrap': case 'wordWrapColumn': case 'wrappingIndent': case 'fontInfo': case 'lineHeight':
      case 'cursorSurroundingLines': case 'cursorSurroundingLinesStyle': case 'scrollBeyondLastLine':
        features.push(`view option ${k}`); break;
      default: features.push(`editor option ${k}`);
    }
  }
  return { cfg, features };
}

// Suites that test VS Code types or helpers quire does not have, as a whole.
const SUITE_NA = {
  'range.test.ts': "VS Code's line/column Range type; quire ranges are byte ranges (std::ops::Range<usize>) on char boundaries",
  'cursorColumns.test.ts': 'internal visible-column helpers (quire-core commands::model); their behaviour is covered through cursor.test.ts',
  'model.line.test.ts': "VS Code's tokenization registry; quire tokens come from quire-syntax",
  'editStack.test.ts': "VS Code's TextChange serialisation; quire's history stores ChangeSets",
};
if (SUITE_NA[basename(rel)]) for (const t of results) { t.unsupported = SUITE_NA[basename(rel)]; t.skip = false; }
const esc = (s) => s.replace(/\n/g, ' ');
let fx = `# Translated from Visual Studio Code, ${rel}\n# (microsoft/vscode @ ${sha}) by tests/port/port.mjs.\n#\n# Copyright (c) Microsoft Corporation. All rights reserved.\n# Licensed under the MIT License (see THIRD_PARTY_NOTICES.md for the full text).\n\n`;
const summary = [];
for (const t of results) {
  const full = `${t.suite} > ${t.name}`;
  if (t.skip) { summary.push({ name: full, na: 'skipped upstream (test.skip)' }); continue; }
  if (t.unsupported) { summary.push({ name: full, na: t.unsupported }); continue; }
  const { cfg, features } = mapConfig(t);
  fx += `=== ${esc(full)}\n`;
  if (features.length) fx += `# features: ${features.join('; ')}\n`;
  fx += `config: ${JSON.stringify(cfg)}\n`;
  fx += `text: ${JSON.stringify(t.text)}\n`;
  for (const s of t.steps) fx += `${s}\n`;
  fx += '\n';
  summary.push({ name: full, fixture: basename(out), features });
}
writeFileSync(out, fx);
writeFileSync(out.replace(/\.fixture$/, '.summary.json'), JSON.stringify(summary, null, 1));
console.log(`${results.length} tests, ${summary.filter((s) => s.fixture).length} translated`);
