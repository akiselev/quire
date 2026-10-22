// Recording stubs for translating VS Code editor tests into quire fixtures.
// Loaded with `node --import ./prelude.mjs --experimental-transform-types <test>.ts`.
// Every editor/model call becomes a fixture step; anything unknown throws `Unsupported`, and the
// test is recorded as not translatable with that reason.

class Unsupported extends Error {}
globalThis.__Unsupported = Unsupported;

// Outside a test (suite-level setup) an unsupported value is permissive and only records what
// it was; inside a test any use throws `Unsupported`.
function unsupported(name) {
  const f = function () {};
  return new Proxy(f, {
    get(_t, p) {
      if (!cur && (p === Symbol.toPrimitive || p === 'toString')) return () => name;
      if (p === Symbol.toPrimitive || p === 'toString' || p === Symbol.toStringTag || p === 'then' || p === 'constructor' || p === 'toJSON') {
        return undefined;
      }
      if (!cur) return unsupported(`${name}.${String(p)}`);
      throw new Unsupported(`${name}.${String(p)}`);
    },
    set() { return true; },
    apply() { if (!cur) return unsupported(`${name}()`); throw new Unsupported(`${name}()`); },
    construct() { if (!cur) return unsupported(`new ${name}`); throw new Unsupported(`new ${name}`); },
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
  toString() { const f = (v) => (isMarker(v) ? "?" : v); return `(${f(this.lineNumber)},${f(this.column)})`; }
}
class Range {
  constructor(sl, sc, el, ec) {
    if (sl > el || (sl === el && sc > ec)) { [sl, sc, el, ec] = [el, ec, sl, sc]; }
    this.startLineNumber = sl; this.startColumn = sc; this.endLineNumber = el; this.endColumn = ec;
  }
  getStartPosition() { return new Position(this.startLineNumber, this.startColumn); }
  getEndPosition() { return new Position(this.endLineNumber, this.endColumn); }
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
  if (s && [s.startLineNumber, s.startColumn, s.endLineNumber, s.endColumn].some((v) => Number.isNaN(v))) throw new Unsupported('NaN position');
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
      if (kind === 'selections' && p === 'length') return marker('selectionCount');
      if (kind === 'selections' && p === 'map') {
        return (fn) => {
          if (fn && fn.name === 'fromRange') return marker('selectionRanges');
          throw new Unsupported('selections.map(?)');
        };
      }
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
    case 'serializedStops': step(`expectStops ${JSON.stringify(expected)}`); break;
    case 'findMatches': step(`expectFindAll ${actual.arg} => ${[...expected].map(matchArgs).join('; ')}`); break;
    case 'findNext': step(`expectFindNext ${actual.arg} => ${matchArgs(expected)}`); break;
    case 'findPrev': step(`expectFindPrev ${actual.arg} => ${matchArgs(expected)}`); break;
    case 'valueInRange': step(`expectValueInRange ${actual.arg} ${JSON.stringify(String(expected).replace(/\r\n/g, '\n'))}`); break;
    case 'lines': step(`expectText ${JSON.stringify(expected.join('\n'))}`); break;
    case 'maxColumn': step(`expectMaxColumn ${actual.arg} ${expected}`); break;
    case 'lineCount': step(`expectLineCount ${expected}`); break;
    case 'selection': step(`expectPrimarySel ${selArgs(expected)}`); break;
    case 'selections': step(`expectSel ${sortSels(expected).map(selArgs).join('; ')}`); break;
    case 'position': step(`expectHead ${expected.lineNumber} ${expected.column}`); break;
    case 'selectionCount': step(`expectSelCount ${expected}`); break;
    case 'selectionRanges': {
      const rs = [...expected].map((r) => r.join(' '));
      rs.sort((a, b) => { const x = a.split(' ').map(Number), y = b.split(' ').map(Number); return x[0] - y[0] || x[1] - y[1]; });
      step(`expectRanges ${rs.join('; ')}`);
      break;
    }
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
assert.throws = (fn) => {
  const n = cur ? cur.steps.length : 0;
  try { fn(); } catch (e) { if (e instanceof Unsupported) throw e; }
  const added = cur ? cur.steps.splice(n) : [];
  if (added.length === 1 && added[0].startsWith('applyEdits ')) { step(`expectRefused ${added[0]}`); return; }
  throw new Unsupported('assert.throws');
};
globalThis.assert = assert;

// ---- commands ----
const NAV = {
  CursorLeft: 'cursorLeft', CursorRight: 'cursorRight', CursorUp: 'cursorUp', CursorDown: 'cursorDown',
  CursorHome: 'cursorHome', CursorEnd: 'cursorEnd', CursorTop: 'cursorTop', CursorBottom: 'cursorBottom',
  CursorPageUp: 'cursorPageUp', CursorPageDown: 'cursorPageDown',
  SelectAll: 'editor.action.selectAll', ExpandLineSelection: 'expandLineSelection',
  RemoveSecondaryCursors: 'removeSecondaryCursors',
  CursorColumnSelectLeft: 'cursorColumnSelectLeft', CursorColumnSelectRight: 'cursorColumnSelectRight',
  CursorColumnSelectUp: 'cursorColumnSelectUp', CursorColumnSelectDown: 'cursorColumnSelectDown',
  CursorColumnSelectPageUp: 'cursorColumnSelectPageUp', CursorColumnSelectPageDown: 'cursorColumnSelectPageDown',
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
      if (name === 'ColumnSelect') {
        step(`columnSelect ${args.position.lineNumber} ${args.mouseColumn}${args.doColumnSelect ? ' drag' : ''}`);
        return;
      }
      if (name === 'WordSelect') {
        const p = args.position;
        step(`wordSelect ${p.lineNumber} ${p.column}`);
        return;
      }
      const base = name.endsWith('Select') ? name.slice(0, -6) : name;
      if (NAV[name] && name.startsWith('CursorColumnSelect')) { step(`cmd ${NAV[name]}`); return; }
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
    __case: null,
    __model: true, __text: text, __languageId: languageId, __modelOpts: modelOpts || {},
    getLineContent: (n) => marker('line', n),
    getValue: (eol) => marker('value'),
    getLineCount: () => marker('lineCount'),
    getLinesContent: () => marker('lines'),
    getLineMaxColumn: (n) => marker('maxColumn', n),
    setEOL: (eol) => step(`setEOL ${eol === 1 ? 'crlf' : 'lf'}`),
    getFullModelRange: () => ({ __full: true }),
    applyEdits: (edits, computeUndo) => {
      if (computeUndo) throw new Unsupported('applyEdits undo edits');
      const list = edits.map((e) => {
        if (e.forceMoveMarkers) throw new Unsupported('forceMoveMarkers');
        return [...selArgs(e.range).split(' ').map(Number), e.text ?? ''];
      });
      step(`applyEdits ${JSON.stringify(list)}`);
    },
    getValueInRange: (r, eol) => {
      if (eol !== undefined && eol !== 1 && eol !== 0) throw new Unsupported('getValueInRange CRLF');
      return marker('valueInRange', selArgs(r));
    },
    setValue: (v) => step(`setValue ${JSON.stringify(v)}`),
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
  const m = makeModel(text, languageId ?? null, modelOpts);
  if (cur) startCase(m, {});
  return m;
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

// Starts a fixture case for a model (a model-only test, or the model an editor is made for).
function startCase(model, opts) {
  if (!cur) throw new Unsupported('model outside a test');
  if (cur.editorStarted) {
    // A second model/editor in one test: the first becomes its own fixture case ("… #1").
    cur.parts = cur.parts || [];
    cur.parts.push({ ...cur, parts: undefined, steps: cur.steps });
    cur.steps = [];
  }
  cur.text = model.__text;
  cur.languageId = model.__languageId;
  cur.modelOpts = model.__modelOpts;
  cur.editorOpts = opts || {};
  cur.editorStarted = true;
  model.__case = cur.steps;
}

function makeEditor(model, opts) {
  if (!cur) throw new Unsupported('editor outside a test');
  if (model.__case !== cur.steps) startCase(model, opts);
  else cur.editorOpts = opts || {};
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
    getValue: () => marker('value'),
    trigger: (_src, id, args) => {
      if (id === 'type') { for (const ch of Array.from(args.text)) step(`type ${JSON.stringify(ch)}`); return; }
      if (id === 'paste') { if (args.multicursorText || args.pasteOnNewLine) throw new Unsupported('paste variants'); step(`json ${JSON.stringify({ id: 'editor.action.clipboardPasteAction', text: args.text, per_cursor: true })}`); return; }
      if (id === 'undo' || id === 'redo' || id === 'removeSecondaryCursors' || id === 'cut') { step(`cmd ${id === 'cut' ? 'editor.action.clipboardCutAction' : id}`); return; }
      throw new Unsupported(`editor.trigger(${id})`);
    },
    registerAndInstantiateContribution: (_id, ctor) => {
      if (ctor && ctor.__contribution) return { dispose() {}, getState() { throw new Unsupported('find controller state'); } };
      throw new Unsupported('contribution');
    },
    setHasTextFocus: () => { throw new Unsupported('text focus'); },
    focus: () => {},
    dispose: () => {},
  };
  const editor = new Proxy(editorTarget, { get(t, p) { if (p in t) return t[p]; if (p === 'then') return undefined; throw new Unsupported(`editor.${String(p)}`); } });
  const viewModel = new Proxy({
    startComposition: () => { cur.composing = true; step('imeStart'); },
    endComposition: () => { cur.composing = false; step('imeEnd'); },
    compositionType: (text, prev, next, delta) => {
      if (delta) throw new Unsupported('compositionType positionDelta');
      if (cur.composing) step(`imeReplace ${JSON.stringify(text)} ${prev} ${next}`);
      else step(`json ${JSON.stringify({ id: 'compositionType', text, replace_prev_chars: prev, replace_next_chars: next })}`);
    },
    type: (text, source) => {
      if (cur.composing) { step(`imeType ${JSON.stringify(text)}`); return; }
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

// ---- stubs for imported VS Code classes (resolved by name; anything else is Unsupported) ----
const STUBS = new Map();
function actionClass(id) {
  return class { run(_accessor, _editor, _args) { step(`cmd ${id}`); } };
}
for (const [name, id] of Object.entries({
  InsertCursorAbove: 'editor.action.insertCursorAbove',
  InsertCursorBelow: 'editor.action.insertCursorBelow',
  AddSelectionToNextFindMatchAction: 'editor.action.addSelectionToNextFindMatch',
  DeleteAllLeftAction: 'deleteAllLeft',
  DeleteLinesAction: 'editor.action.deleteLines',
  IndentLinesAction: 'editor.action.indentLines',
  InsertLineAfterAction: 'editor.action.insertLineAfter',
  InsertLineBeforeAction: 'editor.action.insertLineBefore',
})) STUBS.set(name, actionClass(id));
STUBS.set('CommonFindController', { ID: 'findController', __contribution: true });
STUBS.set('MultiCursorSelectionController', { ID: 'multiCursorController', __contribution: true });
STUBS.set('Handler', { Type: 'type', Paste: 'paste', Cut: 'cut', Undo: 'undo', Redo: 'redo' });
// ICommand classes used through testCommand(): constructing one names the action it belongs to.
STUBS.set('CopyLinesCommand', class { constructor(_sel, down) { this.__cmd = down ? 'editor.action.copyLinesDownAction' : 'editor.action.copyLinesUpAction'; } });
STUBS.set('MoveLinesCommand', class { constructor(_sel, down, autoIndent) { this.__cmd = down ? 'editor.action.moveLinesDownAction' : 'editor.action.moveLinesUpAction'; this.__autoIndent = autoIndent; } });
STUBS.set('IndentAction', { None: 0, Indent: 1, IndentOutdent: 2, Outdent: 3 });
STUBS.set('EditorAutoIndentStrategy', { None: 0, Keep: 1, Brackets: 2, Advanced: 3, Full: 4 });
STUBS.set('TestLanguageConfigurationService', class { register() { return { dispose() {} }; } dispose() {} });
STUBS.set('ServiceCollection', class { constructor() {} set() {} });
STUBS.set('InMemoryStorageService', class { dispose() {} });
STUBS.set('IStorageService', Symbol('IStorageService'));
// Word operation commands (wordOperations.ts): `runEditorCommand` records the command id.
for (const base of ['CursorWordLeft', 'CursorWordRight', 'CursorWordStartLeft', 'CursorWordStartRight', 'CursorWordEndLeft', 'CursorWordEndRight']) {
  for (const sel of ['', 'Select']) {
    const id = base[0].toLowerCase() + base.slice(1) + sel;
    STUBS.set(base + sel, class { runEditorCommand() { step(`cmd ${id}`); } runCoreEditorCommand() { step(`cmd ${id}`); } });
  }
}
for (const name of ['DeleteWordLeft', 'DeleteWordRight', 'DeleteWordStartLeft', 'DeleteWordStartRight', 'DeleteWordEndLeft', 'DeleteWordEndRight']) {
  const id = name[0].toLowerCase() + name.slice(1);
  STUBS.set(name, class { runEditorCommand() { step(`cmd ${id}`); } });
}
STUBS.set('isFirefox', false);
// wordTestUtils.ts (VS Code, MIT): pipe positions.
STUBS.set('deserializePipePositions', (text) => {
  let out = ''; let line = 1; let ci = 0; const ps = [];
  for (const ch of text) {
    if (ch === '\n') { out += ch; line++; ci = 0; continue; }
    if (ch === '|') ps.push(new Position(line, ci + 1)); else { out += ch; ci += ch.length; }
  }
  return [out, ps];
});
STUBS.set('serializePipePositions', (_text, stops) => {
  if (isMarker(stops) && stops.kind === 'stops') return marker('serializedStops');
  throw new Unsupported('serializePipePositions(values)');
});
// testRepeatedActionAndExtractPositions: the action, record and stop condition become one
// `repeat` step (the record and stop condition are recognised from their source).
STUBS.set('testRepeatedActionAndExtractPositions', (text, initial, action, record, stop, options = {}) => {
  const model = makeModel(text, null, {});
  const { editor } = makeEditor(model, options);
  step(`moveTo ${initial.lineNumber} ${initial.column}`);
  const n = cur.steps.length;
  action(editor);
  const cmds = cur.steps.splice(n);
  if (cmds.length !== 1 || !cmds[0].startsWith('cmd ')) throw new Unsupported('repeated action shape');
  const rec = record.toString();
  let recKind;
  if (/getPosition\(\)/.test(rec)) recKind = 'position';
  else if (/text\.length - ed\.getValue\(\)\.length \+ 1/.test(rec)) recKind = 'deleted';
  else throw new Unsupported('repeated record');
  const st = stop.toString();
  let until;
  let m;
  if ((m = st.match(/getPosition\(\)!?\.equals\(new Position\((\d+), (\d+)\)\)/))) until = `pos ${m[1]} ${m[2]}`;
  else if (/getValue\(\)\.length === 0/.test(st)) until = 'empty';
  else throw new Unsupported('repeated stop condition');
  step(`repeat ${cmds[0].slice(4)} ${recKind} ${until}`);
  return marker('stops');
});
// textModelSearch.ts
STUBS.set('USUAL_WORD_SEPARATORS', '`~!@#$%^&*()-=+[{]}\\|;:\'",.<>/?');
STUBS.set('EndOfLineSequence', { LF: 0, CRLF: 1 });
class SearchParams { constructor(searchString, isRegex, matchCase, wordSeparators) { Object.assign(this, { searchString, isRegex, matchCase, wordSeparators }); } }
STUBS.set('SearchParams', SearchParams);
class FindMatch { constructor(range, matches) { this.range = range; this.matches = matches; } }
STUBS.set('FindMatch', FindMatch);
function queryJson(p) {
  if (!(p instanceof SearchParams)) throw new Unsupported('search params');
  return JSON.stringify({ searchString: p.searchString, isRegex: p.isRegex, matchCase: p.matchCase, wordSeparators: p.wordSeparators });
}
function posArg(p) {
  if (isMarker(p.lineNumber) || isMarker(p.column)) return 'end';
  return `${p.lineNumber} ${p.column}`;
}
STUBS.set('TextModelSearch', {
  findMatches(_model, params, range, capture, _limit) {
    if (!range || !range.__full) throw new Unsupported('findMatches in a range');
    if (capture) throw new Unsupported('capturing matches');
    return marker('findMatches', queryJson(params));
  },
  findNextMatch(_model, params, pos, capture) {
    if (capture) throw new Unsupported('capturing matches');
    return marker('findNext', `${posArg(pos)} ${queryJson(params)}`);
  },
  findPreviousMatch(_model, params, pos, capture) {
    if (capture) throw new Unsupported('capturing matches');
    return marker('findPrev', `${posArg(pos)} ${queryJson(params)}`);
  },
});
function matchArgs(m) {
  if (m === undefined || m === null) return 'none';
  if (!(m instanceof FindMatch) || m.matches) throw new Unsupported('find match value');
  return selArgs(m.range);
}
// editableTextModelTestUtils.ts: apply edits (all against the original), check, apply the inverse.
STUBS.set('testApplyEditsWithSyncedModels', (original, edits, expected, inputEditsAreInvalid = false) => {
  const model = makeModel(original.join('\n'), null, {});
  startCase(model, {});
  const list = edits.map((e) => {
    if (e.forceMoveMarkers) throw new Unsupported('forceMoveMarkers');
    return [...selArgs(e.range).split(' ').map(Number), e.text ?? ''];
  });
  step(`applyEdits ${JSON.stringify(list)}`);
  step(`expectText ${JSON.stringify(expected.join('\n'))}`);
  step('applyInverse');
  step(`expectText ${JSON.stringify(original.join('\n'))}`);
});
globalThis.__testApplyEditsFails = (original, edits) => {
  const model = makeModel(original.join('\n'), null, {});
  startCase(model, {});
  const list = edits.map((e) => [...selArgs(e.range).split(' ').map(Number), e.text ?? '']);
  step(`expectRefused applyEdits ${JSON.stringify(list)}`);
};
globalThis.__resolve = (name) => STUBS.has(name) ? STUBS.get(name) : unsupported(name);

// testCommand(lines, languageId, selection, commandFactory, expectedLines, expectedSelection)
globalThis.testCommand = (lines, languageId, selection, factory, expectedLines, expectedSelection) => {
  if (languageId) throw new Unsupported('testCommand with a language');
  const model = makeModel(lines.join('\n'), null, {});
  makeEditor(model, {});
  step(`setSelections ${selArgs(selection)}`);
  const c = factory(null, selection);
  if (!c || !c.__cmd) throw new Unsupported('testCommand(other command)');
  if (c.__autoIndent === 4) throw new Unsupported('move lines with indentation rules (autoIndent full)');
  step(`cmd ${c.__cmd}`);
  step(`expectText ${JSON.stringify(expectedLines.join('\n'))}`);
  step(`expectSel ${selArgs(expectedSelection)}`);
};

// ---- services ----
class DisposableStore { add(x) { return x; } dispose() {} clear() {} }
globalThis.DisposableStore = DisposableStore;
const disposable = { dispose() {} };
globalThis.ILanguageConfigurationService = Symbol('ILanguageConfigurationService');
globalThis.ILanguageService = Symbol('ILanguageService');
// RegExps do not survive JSON: language rules keep their source with the flags inline.
function reToString(re) {
  let f = '';
  if (re.flags.includes('i')) f += 'i';
  if (re.flags.includes('m')) f += 'm';
  if (re.flags.includes('s')) f += 's';
  return (f ? `(?${f})` : '') + re.source;
}
function plainRegexps(v) {
  if (v instanceof RegExp) return reToString(v);
  if (Array.isArray(v)) return v.map(plainRegexps);
  if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, plainRegexps(x)]));
  return v;
}
const languageConfigurationService = {
  register(id, conf) { langConfigs.set(id, plainRegexps(conf)); return disposable; },
};
const languageService = { registerLanguage() { return disposable; } };
globalThis.createCodeEditorServices = () => ({
  get(id) {
    if (id === globalThis.ILanguageConfigurationService) return languageConfigurationService;
    if (id === globalThis.ILanguageService) return languageService;
    throw new Unsupported('instantiationService.get(other service)');
  },
  createInstance() { throw new Unsupported('instantiationService.createInstance'); },
  invokeFunction(fn) { return fn({ get() { throw new Unsupported('accessor.get'); } }); },
});
globalThis.ensureNoDisposablesAreLeakedInTestSuite = () => new DisposableStore();
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
  if (cur.parts && !cur.unsupported) {
    const parts = [...cur.parts, { ...cur, parts: undefined }];
    parts.forEach((p, i) => {
      if (p.languageId) p.languageConfig = langConfigs.get(p.languageId) || null;
      results.push({ ...p, name: `${name} #${i + 1}` });
    });
    cur = null;
    return;
  }
  results.push(cur);
  cur = null;
}
globalThis.test = (name, fn) => runTest(name, fn, false);
globalThis.test.skip = (name) => runTest(name, null, true);
globalThis.test.only = globalThis.test;
