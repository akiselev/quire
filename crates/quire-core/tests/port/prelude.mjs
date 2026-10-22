// Recording stubs for translating VS Code editor tests into quire fixtures.
// Loaded with `node --import ./prelude.mjs --experimental-transform-types <test>.ts`.
// Every editor/model call becomes a fixture step; anything unknown throws `Unsupported`, and the
// test is recorded as not translatable with that reason.

class Unsupported extends Error {}
globalThis.__Unsupported = Unsupported;

function unsupported(name) {
  const f = function () {};
  return new Proxy(f, {
    get(_t, p) {
      if (p === Symbol.toPrimitive || p === 'toString' || p === Symbol.toStringTag || p === 'then' || p === 'constructor') {
        return undefined;
      }
      throw new Unsupported(`${name}.${String(p)}`);
    },
    apply() { throw new Unsupported(`${name}()`); },
    construct() { throw new Unsupported(`new ${name}`); },
  });
}
globalThis.__unsupported = unsupported;

// ---- current test recording ----
let cur = null; // { name, suite, text, editorOpts, modelOpts, languageId, steps, features }
const results = [];
globalThis.__results = results;
const langConfigs = new Map();

function step(s) {
  if (!cur) throw new Unsupported('editor call outside a test');
  if (!cur.editorStarted) throw new Unsupported('step before editor');
  cur.steps.push(s);
}

class Position {
  constructor(lineNumber, column) { this.lineNumber = lineNumber; this.column = column; }
  equals(o) { return o && o.lineNumber === this.lineNumber && o.column === this.column; }
}
class Range {
  constructor(sl, sc, el, ec) {
    if (sl > el || (sl === el && sc > ec)) { [sl, sc, el, ec] = [el, ec, sl, sc]; }
    this.startLineNumber = sl; this.startColumn = sc; this.endLineNumber = el; this.endColumn = ec;
  }
}
class Selection extends Range {
  constructor(a, b, c, d) {
    super(a, b, c, d);
    this.selectionStartLineNumber = a; this.selectionStartColumn = b;
    this.positionLineNumber = c; this.positionColumn = d;
  }
  static fromPositions(s, e = s) { return new Selection(s.lineNumber, s.column, e.lineNumber, e.column); }
  isEmpty() { return this.selectionStartLineNumber === this.positionLineNumber && this.selectionStartColumn === this.positionColumn; }
}
globalThis.Position = Position;
globalThis.Range = Range;
globalThis.Selection = Selection;

function selArgs(s) {
  if (s instanceof Selection) return `${s.selectionStartLineNumber} ${s.selectionStartColumn} ${s.positionLineNumber} ${s.positionColumn}`;
  if (s instanceof Range) return `${s.startLineNumber} ${s.startColumn} ${s.endLineNumber} ${s.endColumn}`;
  if (s && s.selectionStartLineNumber !== undefined) return `${s.selectionStartLineNumber} ${s.selectionStartColumn} ${s.positionLineNumber} ${s.positionColumn}`;
  if (s && s.startLineNumber !== undefined) return `${s.startLineNumber} ${s.startColumn} ${s.endLineNumber} ${s.endColumn}`;
  throw new Unsupported('selection value');
}
function sortSels(list) {
  const key = (s) => {
    const [a, b, c, d] = selArgs(s).split(' ').map(Number);
    const [sl, sc] = (a < c || (a === c && b <= d)) ? [a, b] : [c, d];
    return sl * 1e6 + sc;
  };
  return [...list].sort((x, y) => key(x) - key(y));
}

// ---- markers for values read from the model/view ----
const MARK = Symbol('mark');
function marker(kind, arg) {
  const m = { [MARK]: true, kind, arg };
  return new Proxy(m, {
    get(t, p) {
      if (p === MARK || p === 'kind' || p === 'arg') return t[p];
      if (p === Symbol.toPrimitive || p === 'then' || p === Symbol.toStringTag) return undefined;
      if (p === 'toString') return () => { throw new Unsupported(`${kind} used as a string`); };
      throw new Unsupported(`${kind}.${String(p)}`);
    },
  });
}
const isMarker = (v) => v && typeof v === 'object' && v[MARK] === true;

function expectFrom(actual, expected) {
  if (!isMarker(actual)) return; // a plain JS assertion, nothing to record
  switch (actual.kind) {
    case 'line': step(`expectLine ${actual.arg} ${JSON.stringify(expected)}`); break;
    case 'value': step(`expectText ${JSON.stringify(String(expected).replace(/\r\n/g, '\n'))}`); break;
    case 'lineCount': step(`expectLineCount ${expected}`); break;
    case 'selection': step(`expectPrimarySel ${selArgs(expected)}`); break;
    case 'selections': step(`expectSel ${sortSels(expected).map(selArgs).join('; ')}`); break;
    case 'position': step(`expectHead ${expected.lineNumber} ${expected.column}`); break;
    default: throw new Unsupported(`assert on ${actual.kind}`);
  }
}

const assert = function (v) { if (isMarker(v)) throw new Unsupported('assert(marker)'); };
assert.strictEqual = (a, e) => expectFrom(a, e);
assert.equal = (a, e) => expectFrom(a, e);
assert.deepStrictEqual = (a, e) => {
  if (Array.isArray(a) && a.some(isMarker)) throw new Unsupported('array of markers');
  expectFrom(a, e);
};
assert.deepEqual = assert.deepStrictEqual;
assert.ok = (v) => { if (isMarker(v)) throw new Unsupported('assert.ok(marker)'); };
assert.notStrictEqual = (a) => { if (isMarker(a)) throw new Unsupported('notStrictEqual'); };
globalThis.assert = assert;

// ---- commands ----
const NAV = {
  CursorLeft: 'cursorLeft', CursorRight: 'cursorRight', CursorUp: 'cursorUp', CursorDown: 'cursorDown',
  CursorHome: 'cursorHome', CursorEnd: 'cursorEnd', CursorTop: 'cursorTop', CursorBottom: 'cursorBottom',
  CursorPageUp: 'cursorPageUp', CursorPageDown: 'cursorPageDown',
  SelectAll: 'editor.action.selectAll', ExpandLineSelection: 'expandLineSelection',
  RemoveSecondaryCursors: 'removeSecondaryCursors',
};
const EDIT = {
  DeleteLeft: 'deleteLeft', DeleteRight: 'deleteRight', Undo: 'undo', Redo: 'redo', Tab: 'tab', Outdent: 'outdent',
};
function navCommand(name) {
  return {
    runCoreEditorCommand(_vm, args) {
      if (name === 'MoveTo' || name === 'MoveToSelect') {
        const p = args.position;
        step(`moveTo ${p.lineNumber} ${p.column}${name === 'MoveToSelect' ? ' select' : ''}`);
        return;
      }
      if (name === 'WordSelect') {
        const p = args.position;
        step(`wordSelect ${p.lineNumber} ${p.column}`);
        return;
      }
      const base = name.endsWith('Select') ? name.slice(0, -6) : name;
      if (NAV[base] && (base === name || NAV[base].startsWith('cursor'))) {
        step(`cmd ${NAV[base]}${base !== name ? 'Select' : ''}`);
        return;
      }
      throw new Unsupported(`CoreNavigationCommands.${name}`);
    },
    runEditorCommand(_a, ed, args) { this.runCoreEditorCommand(null, args || {}); },
    __id: name,
  };
}
globalThis.CoreNavigationCommands = new Proxy({}, { get: (_t, p) => navCommand(String(p)) });
globalThis.CoreEditingCommands = new Proxy({}, {
  get: (_t, p) => {
    const name = String(p);
    return {
      __edit: name,
      runEditorCommand() { runEdit(name); },
      runCoreEditorCommand() { runEdit(name); },
    };
  },
});
function runEdit(name) {
  if (EDIT[name]) { step(`cmd ${EDIT[name]}`); return; }
  throw new Unsupported(`CoreEditingCommands.${name}`);
}

// ---- model / editor / view model ----
function makeModel(text, languageId, modelOpts) {
  const model = {
    __model: true, __text: text, __languageId: languageId, __modelOpts: modelOpts || {},
    getLineContent: (n) => marker('line', n),
    getValue: (eol) => marker('value'),
    getLineCount: () => marker('lineCount'),
    undo: () => step('cmd undo'),
    redo: () => step('cmd redo'),
    pushStackElement: () => step('closeUndoGroup'),
    dispose: () => {},
    getLanguageId: () => languageId,
  };
  return new Proxy(model, { get(t, p) { if (p in t) return t[p]; if (p === 'then') return undefined; throw new Unsupported(`model.${String(p)}`); } });
}
globalThis.createTextModel = (text, languageId, modelOpts) => {
  if (typeof text !== 'string') throw new Unsupported('createTextModel(non-string)');
  return makeModel(text, languageId ?? null, modelOpts);
};
globalThis.instantiateTextModel = (_inst, text, languageId, modelOpts) => globalThis.createTextModel(text, languageId, modelOpts);

function startEditor(arg, opts, cb) {
  let model;
  if (arg && arg.__model) model = arg;
  else if (Array.isArray(arg)) model = makeModel(arg.join('\n'), null, {});
  else if (typeof arg === 'string') model = makeModel(arg, null, {});
  else throw new Unsupported('withTestCodeEditor arg');
  const { editor, viewModel } = makeEditor(model, opts);
  cb(editor, viewModel, model);
}

function makeEditor(model, opts) {
  if (!cur) throw new Unsupported('editor outside a test');
  if (cur.editorStarted) throw new Unsupported('two editors in one test');
  cur.text = model.__text;
  cur.languageId = model.__languageId;
  cur.modelOpts = model.__modelOpts;
  cur.editorOpts = opts || {};
  cur.editorStarted = true;
  const editorTarget = {
    getModel: () => model,
    runCommand: (c, args) => {
      if (c && c.__edit) return runEdit(c.__edit);
      if (c && c.__id) return c.runCoreEditorCommand(null, args || {});
      throw new Unsupported('editor.runCommand(?)');
    },
    setSelections: (sels) => step(`setSelections ${sels.map(selArgs).join('; ')}`),
    setSelection: (s) => step(`setSelections ${selArgs(s)}`),
    setPosition: (p) => step(`moveTo ${p.lineNumber} ${p.column}`),
    getSelection: () => marker('selection'),
    getSelections: () => marker('selections'),
    getPosition: () => marker('position'),
    pushUndoStop: () => step('closeUndoGroup'),
    focus: () => {},
    dispose: () => {},
  };
  const editor = new Proxy(editorTarget, { get(t, p) { if (p in t) return t[p]; if (p === 'then') return undefined; throw new Unsupported(`editor.${String(p)}`); } });
  const viewModel = new Proxy({
    type: (text, source) => {
      if (source === 'keyboard') {
        // VS Code types keyboard input char by char, each its own edit operation.
        for (const ch of Array.from(text)) step(`type ${JSON.stringify(ch)}`);
      } else {
        step(`typeRaw ${JSON.stringify(text)}`);
      }
    },
    paste: (text, pasteOnNewLine, multicursorText) => {
      if (pasteOnNewLine) throw new Unsupported('paste on new line');
      if (multicursorText) throw new Unsupported('multicursor paste text');
      step(`json ${JSON.stringify({ id: 'editor.action.clipboardPasteAction', text, per_cursor: true })}`);
    },
    cut: () => step('cmd editor.action.clipboardCutAction'),
    setSelections: (_src, sels) => step(`setSelections ${sels.map(selArgs).join('; ')}`),
    getSelection: () => marker('selection'),
    getSelections: () => marker('selections'),
    getPosition: () => marker('position'),
    setHasFocus: () => {},
    model,
  }, { get(t, p) { if (p in t) return t[p]; if (p === 'then') return undefined; throw new Unsupported(`viewModel.${String(p)}`); } });
  editorTarget.getViewModel = () => viewModel;
  return { editor, viewModel };
}
globalThis.instantiateTestCodeEditor = (_inst, model, opts) => makeEditor(model, opts).editor;
globalThis.TextModel = { DEFAULT_CREATION_OPTIONS: {} };
globalThis.assertCursor = (_vm, what) => {
  if (what instanceof Position) step(`expectSel ${what.lineNumber} ${what.column} ${what.lineNumber} ${what.column}`);
  else if (Array.isArray(what)) step(`expectSel ${sortSels(what).map(selArgs).join('; ')}`);
  else step(`expectSel ${selArgs(what)}`);
};
globalThis.withTestCodeEditor = (arg, opts, cb) => startEditor(arg, opts, cb);
globalThis.withAsyncTestCodeEditor = () => { throw new Unsupported('async editor'); };

// ---- services ----
class DisposableStore { add(x) { return x; } dispose() {} clear() {} }
globalThis.DisposableStore = DisposableStore;
const disposable = { dispose() {} };
globalThis.ILanguageConfigurationService = Symbol('ILanguageConfigurationService');
globalThis.ILanguageService = Symbol('ILanguageService');
const languageConfigurationService = {
  register(id, conf) { langConfigs.set(id, conf); return disposable; },
};
const languageService = { registerLanguage() { return disposable; } };
globalThis.createCodeEditorServices = () => ({
  get(id) {
    if (id === globalThis.ILanguageConfigurationService) return languageConfigurationService;
    if (id === globalThis.ILanguageService) return languageService;
    throw new Unsupported('instantiationService.get(other service)');
  },
  createInstance() { throw new Unsupported('instantiationService.createInstance'); },
  invokeFunction() { throw new Unsupported('instantiationService.invokeFunction'); },
});
globalThis.ensureNoDisposablesAreLeakedInTestSuite = () => {};
globalThis.EndOfLinePreference = { TextDefined: 0, LF: 1, CRLF: 2 };

// ---- mocha-style suite/test ----
const suiteStack = [];
globalThis.suite = (name, fn) => {
  suiteStack.push({ name, setups: [], teardowns: [] });
  try { fn(); } finally { suiteStack.pop(); }
};
globalThis.suite.skip = () => {};
globalThis.setup = (fn) => suiteStack[suiteStack.length - 1].setups.push(fn);
globalThis.teardown = (fn) => suiteStack[suiteStack.length - 1].teardowns.push(fn);
globalThis.suiteSetup = () => {};
globalThis.suiteTeardown = () => {};
function runTest(name, fn, skip) {
  const suiteName = suiteStack.map((s) => s.name).join(' > ');
  cur = { name, suite: suiteName, steps: [], editorStarted: false, unsupported: null, skip: !!skip };
  if (skip) { results.push(cur); cur = null; return; }
  try {
    for (const s of suiteStack) for (const f of s.setups) f();
    const r = fn();
    if (r && typeof r.then === 'function') throw new Unsupported('async test');
    if (!cur.editorStarted) throw new Unsupported('no editor in test');
  } catch (e) {
    if (e instanceof Unsupported) cur.unsupported = e.message;
    else cur.unsupported = `error: ${e && e.message}`;
  }
  if (cur.languageId) cur.languageConfig = langConfigs.get(cur.languageId) || null;
  results.push(cur);
  cur = null;
}
globalThis.test = (name, fn) => runTest(name, fn, false);
globalThis.test.skip = (name) => runTest(name, null, true);
globalThis.test.only = globalThis.test;
