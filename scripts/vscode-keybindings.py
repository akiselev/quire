#!/usr/bin/env python3
"""Extract VS Code's default editor keybindings from its source and audit quire's keymap.

Usage:
    scripts/vscode-keybindings.py [--src DIR]

Without --src the VS Code sources at SHA are fetched with curl into a new temporary directory.
With --src DIR, DIR must already hold the files under their repository paths (src/vs/...).

For every command id in crates/egui_quire/keymaps/vscode-default.json the script reads the
keybinding rules VS Code declares (kbOpts / KeybindingsRegistry rules: primary, secondary, mac,
linux, win, kbExpr, precondition), resolves them per OS (VS Code's bindToCurrentPlatform: a
platform object replaces both primary and secondary), and writes:

  crates/egui_quire/keymaps/vscode-extracted.json   the table tests/keymap_audit.rs checks
  docs/design/KEYMAP-AUDIT.md                        the audit report

Run with python3 -I (the sources are untrusted data; the script only reads them as text).
"""

import json
import os
import re
import subprocess
import sys
import tempfile

SHA = "bdce63db587665a91d1f1237b807d43471f0d9bc"
SOURCES = [
    "src/vs/editor/browser/coreCommands.ts",
    "src/vs/editor/browser/editorExtensions.ts",
    "src/vs/editor/contrib/clipboard/browser/clipboard.ts",
    "src/vs/editor/contrib/comment/browser/comment.ts",
    "src/vs/editor/contrib/find/browser/findController.ts",
    "src/vs/editor/contrib/find/browser/findModel.ts",
    "src/vs/editor/contrib/folding/browser/folding.ts",
    "src/vs/editor/contrib/lineSelection/browser/lineSelection.ts",
    "src/vs/editor/contrib/linesOperations/browser/linesOperations.ts",
    "src/vs/editor/contrib/multicursor/browser/multicursor.ts",
    "src/vs/editor/contrib/wordOperations/browser/wordOperations.ts",
    "src/vs/workbench/contrib/files/browser/fileCommands.ts",
    "src/vs/workbench/contrib/files/browser/fileConstants.ts",
    "src/vs/workbench/contrib/codeEditor/browser/toggleWordWrap.ts",
]

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KEYMAP = os.path.join(ROOT, "crates/egui_quire/keymaps/vscode-default.json")
TABLE = os.path.join(ROOT, "crates/egui_quire/keymaps/vscode-extracted.json")
REPORT = os.path.join(ROOT, "docs/design/KEYMAP-AUDIT.md")
OSES = ["win", "linux", "mac"]

# Ids in quire's keymap that are not VS Code keybindings, with the reason.
QUIRE_ONLY = {
    "type": "VS Code inserts Enter / Shift+Enter through the text area's input event, not a "
    "keybinding; quire's widget binds them to `type` with a newline.",
}

# Differences the first audit (widget3, 2026-10-22) found and fixed, per command and OS.
FIXED = {
    "closeFindWidget": "all: added secondary `shift+escape`",
    "removeSecondaryCursors": "all: added secondary `shift+escape`",
    "cursorColumnSelectLeft": "linux: removed `ctrl+shift+alt+left` (VS Code: `linux: { primary: 0 }`)",
    "cursorColumnSelectRight": "linux: removed `ctrl+shift+alt+right` (VS Code: `linux: { primary: 0 }`)",
    "cursorColumnSelectPageUp": "linux: removed `ctrl+shift+alt+pageup` (VS Code: `linux: { primary: 0 }`)",
    "cursorColumnSelectPageDown": "linux: removed `ctrl+shift+alt+pagedown` (VS Code: `linux: { primary: 0 }`)",
    "cursorLeft": "mac: added secondary `ctrl+b`",
    "cursorRight": "mac: added secondary `ctrl+f`",
    "cursorUp": "mac: added secondary `ctrl+p`",
    "cursorDown": "mac: added secondary `ctrl+n`",
    "cursorUpSelect": "win: added secondary `ctrl+shift+up`",
    "cursorDownSelect": "win: added secondary `ctrl+shift+down`",
    "deleteLeft": "mac: added secondaries `ctrl+h`, `ctrl+backspace`",
    "deleteRight": "mac: added secondaries `ctrl+d`, `ctrl+delete`",
    "deleteAllRight": "mac: added secondary `cmd+delete`",
    "editor.action.insertCursorAbove": "linux: added secondary `ctrl+shift+up`",
    "editor.action.insertCursorBelow": "linux: added secondary `ctrl+shift+down`",
    "editor.action.nextMatchFindAction": "mac: added secondary `f3`",
    "editor.action.previousMatchFindAction": "mac: added secondary `shift+f3`",
    "redo": "mac: removed `ctrl+shift+z` (VS Code's mac object has no secondary)",
}

# Context keys, as written in VS Code's source, and how quire treats them.
# "true": holds whenever the editor widget has keyboard focus in its default state (quire binds
# keys only while it is focused; read-only refusal is the command layer's).
IMPLICIT = {
    "EditorContextKeys.textInputFocus": "editor text has focus",
    "EditorContextKeys.editorTextFocus": "editor text has focus",
    "EditorContextKeys.focus": "editor has focus",
    "EditorContextKeys.writable": "read-only documents are refused by the command layer",
    "EditorContextKeys.tabDoesNotMoveFocus": "quire has no tab-moves-focus mode",
    "CONTEXT_FOLDING_ENABLED": "folding is always enabled",
    "!isComposing": "the widget does not dispatch keys while an IME preedit is active",
    "!(accessibilityModeEnabled && isWindows)": "quire has no screen-reader word-navigation mode",
}
# Context keys quire's `When` models (keymap.rs), as written in keybindings.json.
MODELED = {
    "EditorContextKeys.hasMultipleSelections": "editorHasMultipleSelections",
    "CONTEXT_FIND_WIDGET_VISIBLE": "findWidgetVisible",
}

# Contexts quire does not model, with what quire does instead.
UNMODELED = {
    "CONTEXT_FIND_INPUT_FOCUSED": "find input focused: quire's find bar handles Enter / Shift+Enter "
    "in its own text field",
}

KEYCODES = {
    "LeftArrow": "left", "RightArrow": "right", "UpArrow": "up", "DownArrow": "down",
    "PageUp": "pageup", "PageDown": "pagedown", "Home": "home", "End": "end",
    "Backspace": "backspace", "Delete": "delete", "Enter": "enter", "Tab": "tab",
    "Escape": "escape", "Space": "space", "Insert": "insert",
    "Slash": "/", "Backslash": "\\", "BracketLeft": "[", "BracketRight": "]",
    "Minus": "-", "Equal": "=", "Comma": ",", "Period": ".", "Semicolon": ";", "Quote": "'",
    "Backquote": "`",
}


def keycode_name(code):
    if code in KEYCODES:
        return KEYCODES[code]
    m = re.fullmatch(r"Key([A-Z])", code)
    if m:
        return m.group(1).lower()
    m = re.fullmatch(r"Digit(\d)", code)
    if m:
        return m.group(1)
    m = re.fullmatch(r"F(\d+)", code)
    if m:
        return "f" + m.group(1)
    raise ValueError(f"unknown KeyCode.{code}")


# ---------------------------------------------------------------------------------------------
# Fetching and light-weight TypeScript scanning


def fetch(dest):
    for path in SOURCES:
        out = os.path.join(dest, path)
        os.makedirs(os.path.dirname(out), exist_ok=True)
        url = f"https://raw.githubusercontent.com/microsoft/vscode/{SHA}/{path}"
        subprocess.run(["curl", "-sSfL", url, "-o", out], check=True)


def matching(text, i, open_ch, close_ch):
    """Index of the bracket closing the one at text[i] (skips strings and comments)."""
    depth = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c in "'\"`":
            j = i + 1
            while j < n and text[j] != c:
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            continue
        if text.startswith("//", i):
            i = text.find("\n", i)
            continue
        if text.startswith("/*", i):
            i = text.find("*/", i) + 2
            continue
        if c == open_ch:
            depth += 1
        elif c == close_ch:
            depth -= 1
            if depth == 0:
                return i
        i += 1
    raise ValueError("unbalanced")


def enclosing_object(text, pos):
    """(start, end) of the innermost object literal containing pos."""
    depth = 0
    i = pos
    while i >= 0:
        c = text[i]
        if c == "}":
            depth += 1
        elif c == "{":
            if depth == 0:
                return i, matching(text, i, "{", "}")
            depth -= 1
        i -= 1
    raise ValueError("no enclosing object")


def top_level_props(obj):
    """Top-level `name: value` pairs of an object literal's text (including braces)."""
    props = {}
    i = 1
    n = len(obj) - 1
    while i < n:
        m = re.compile(r"\s*(?://[^\n]*\n\s*)*([A-Za-z_]\w*)\s*:\s*").match(obj, i)
        if not m:
            # skip to next comma at depth 0
            j = i
            depth = 0
            while j < n and not (obj[j] == "," and depth == 0):
                if obj[j] in "([{":
                    depth += 1
                elif obj[j] in ")]}":
                    depth -= 1
                j += 1
            i = j + 1
            continue
        name = m.group(1)
        j = m.end()
        start = j
        depth = 0
        while j < n:
            c = obj[j]
            if c in "'\"`":
                k = j + 1
                while k < n and obj[k] != c:
                    k += 2 if obj[k] == "\\" else 1
                j = k + 1
                continue
            if c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
            elif c == "," and depth == 0:
                break
            j += 1
        props[name] = obj[start:j].strip()
        i = j + 1
    return props


class Consts:
    """String constants: `const X = 'v'`, `static readonly ID = 'v'` (as Class.ID), object maps
    `export const FIND_IDS = { A: 'v' }` (as FIND_IDS.A), and `X.Y.id` for `const Y = ...({ id }`."""

    def __init__(self, texts):
        self.map = {}
        for text in texts:
            for m in re.finditer(r"\bconst\s+(\w+)\s*=\s*'([^']*)'", text):
                self.map[m.group(1)] = m.group(2)
            for m in re.finditer(r"class\s+(\w+)[^{]*\{\s*(?:public\s+)?static\s+readonly\s+ID\s*=\s*'([^']*)'", text):
                self.map[m.group(1) + ".ID"] = m.group(2)
            for m in re.finditer(r"export\s+const\s+(\w+)\s*=\s*\{", text):
                end = matching(text, m.end() - 1, "{", "}")
                body = text[m.end() - 1:end + 1]
                for p in re.finditer(r"(\w+)\s*:\s*'([^']*)'", body):
                    self.map[m.group(1) + "." + p.group(1)] = p.group(2)
            for m in re.finditer(r"export\s+const\s+(\w+)\s*(?::\s*\w+\s*)?=\s*register\w*\(", text):
                idm = re.compile(r"\bid:\s*'([^']*)'").search(text, m.end())
                if idm:
                    self.map.setdefault("@" + m.group(1), idm.group(1))

    def resolve(self, expr):
        expr = expr.strip()
        m = re.fullmatch(r"'([^']*)'", expr)
        if m:
            return m.group(1)
        m = re.fullmatch(r"\w+\.(\w+)\.id", expr)
        if m:
            return self.map.get("@" + m.group(1))
        return self.map.get(expr)


# ---------------------------------------------------------------------------------------------
# Keybinding expressions


def split_args(s):
    out, depth, cur = [], 0, ""
    for c in s:
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        if c == "," and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += c
    if cur.strip():
        out.append(cur)
    return [a.strip() for a in out]


def chord(expr, os_):
    """One chord (`KeyMod.CtrlCmd | KeyCode.KeyZ`) as keybindings.json text on os_."""
    mods = set()
    key = None
    for part in expr.split("|"):
        part = part.strip()
        if part == "KeyMod.CtrlCmd":
            mods.add("cmd" if os_ == "mac" else "ctrl")
        elif part == "KeyMod.WinCtrl":
            mods.add("ctrl" if os_ == "mac" else "meta")
        elif part == "KeyMod.Shift":
            mods.add("shift")
        elif part == "KeyMod.Alt":
            mods.add("alt")
        elif part.startswith("KeyCode."):
            key = keycode_name(part[len("KeyCode."):])
        else:
            raise ValueError(f"unknown key part {part!r} in {expr!r}")
    order = ["ctrl", "shift", "alt", "cmd", "meta"]
    return "+".join([m for m in order if m in mods] + [key])


def keybinding(expr, os_):
    expr = expr.strip()
    if expr in ("0", "", "undefined"):
        return None
    m = re.fullmatch(r"(?:KeyChord|KeyMod\.chord)\((.*)\)", expr, re.S)
    if m:
        a, b = split_args(m.group(1))
        return chord(a, os_) + " " + chord(b, os_)
    return chord(expr, os_)


def platform_keys(rule, os_):
    """VS Code's bindToCurrentPlatform: the platform object, if present, replaces the rule."""
    src = rule
    if os_ in rule:
        src = top_level_props(rule[os_])
    keys = []
    k = keybinding(src.get("primary", "0"), os_)
    if k:
        keys.append(k)
    sec = src.get("secondary")
    if sec:
        for e in split_args(sec.strip()[1:-1]):
            k = keybinding(e, os_)
            if k:
                keys.append(k)
    return keys


# ---------------------------------------------------------------------------------------------
# when clauses


def when_atoms(exprs):
    """Flatten an `and` of context expressions into atom strings. Returns None for `or`."""
    atoms = []
    for e in exprs:
        if not e or e in ("undefined", "null"):
            continue
        # An `or` with an implicit alternative (`focus || editorIsOpen`) holds while focused.
        m = re.fullmatch(r"ContextKeyExpr\.or\((.*)\)", e.strip(), re.S)
        if m and any(a in IMPLICIT for a in split_args(m.group(1))):
            atoms.append(next(a for a in split_args(m.group(1)) if a in IMPLICIT))
            continue
        if "ContextKeyExpr.or" in e:
            return None
        e = re.sub(
            r"ContextKeyExpr\.and\(\s*CONTEXT_ACCESSIBILITY_MODE_ENABLED\s*,\s*IsWindowsContext\s*\)\?\.negate\(\)",
            "!(accessibilityModeEnabled && isWindows)",
            e,
        )
        e = re.sub(r"ContextKeyExpr\.not\('(\w+)'\)", r"!\1", e)
        e = re.sub(r"ContextKeyExpr\.and\(", "(", e)
        for a in re.findall(r"!\([^)]*\)|!?[A-Za-z_][\w.]*(?:\.negate\(\))?", e):
            atoms.append(a)
    return atoms


def classify_when(atoms):
    """(quire when or None, modeled?, reason) for a rule's when atoms."""
    if atoms is None:
        return None, False, "`or` context not modeled"
    modeled = []
    for a in atoms:
        if a in IMPLICIT:
            continue
        if a in MODELED:
            modeled.append(MODELED[a])
            continue
        return None, False, UNMODELED.get(a, f"context `{a}` not modeled by quire")
    if len(modeled) > 1:
        return None, False, "more than one modeled context"
    return (modeled[0] if modeled else None), True, ""


# ---------------------------------------------------------------------------------------------
# Extraction


def extract(src_dir, wanted):
    texts = {}
    for path in SOURCES:
        with open(os.path.join(src_dir, path), encoding="utf-8") as f:
            texts[path] = f.read()
    consts = Consts(texts.values())
    found = {}
    for path, text in texts.items():
        for m in re.finditer(r"\bid:\s*([^,\n}]+)", text):
            cid = consts.resolve(m.group(1))
            if cid not in wanted:
                continue
            start, end = enclosing_object(text, m.start())
            props = top_level_props(text[start:end + 1])
            line = text.count("\n", 0, m.start()) + 1
            pre = props.get("precondition")
            rules = []
            if "kbOpts" in props:
                kb = props["kbOpts"]
                objs = [kb] if kb.startswith("{") else [
                    a for a in split_args(kb.strip()[1:-1])]
                for o in objs:
                    p = top_level_props(o)
                    rules.append((p, [p.get("kbExpr"), pre]))
            elif "primary" in props:
                rules.append((props, [props.get("when")]))
            else:
                continue
            for p, when_exprs in rules:
                atoms = when_atoms(when_exprs)
                qwhen, ok, reason = classify_when(atoms)
                found.setdefault(cid, []).append({
                    "source": f"{path}:{line}",
                    "when": " && ".join(atoms) if atoms is not None else "|".join(
                        w for w in when_exprs if w),
                    "quire_when": qwhen,
                    "modeled": ok,
                    "reason": reason,
                    **{os_: platform_keys(p, os_) for os_ in OSES},
                })
        # registerColumnSelection(CoreNavigationCommands.X.id, keys): column-selection mode.
        for m in re.finditer(r"registerColumnSelection\(([^,]+),\s*([^)]+)\);", text):
            cid = consts.resolve(m.group(1))
            if cid not in wanted:
                continue
            line = text.count("\n", 0, m.start()) + 1
            found.setdefault(cid, []).append({
                "source": f"{path}:{line}",
                "when": "textInputFocus && editorColumnSelection",
                "quire_when": None,
                "modeled": False,
                "reason": "column selection mode (editor.columnSelection) not modeled by quire",
                **{os_: [keybinding(m.group(2), os_)] for os_ in OSES},
            })
    return found


# ---------------------------------------------------------------------------------------------
# quire's keymap, with keymap.rs's platform rule (`key` unless a platform field overrides it)


MOD_ALIASES = {"control": "ctrl", "option": "alt", "meta": "cmd", "win": "cmd", "super": "cmd"}


def canon(seq):
    out = []
    for c in seq.split():
        parts = c.split("+")
        mods = sorted({MOD_ALIASES.get(m.lower(), m.lower()) for m in parts[:-1]})
        out.append("+".join(mods + [parts[-1].lower()]))
    return " ".join(out)


def ours(raw):
    table = {}
    for e in raw:
        for os_ in OSES:
            k = e.get(os_) or e.get("key")
            if k:
                table.setdefault(e["command"], {o: [] for o in OSES})[os_].append(
                    {"key": k, "when": e.get("when")})
    return table


def expected_for(rules):
    exp = {os_: [] for os_ in OSES}
    for r in rules:
        if not r["modeled"]:
            continue
        for os_ in OSES:
            for k in r[os_]:
                exp[os_].append({"key": k, "when": r["quire_when"]})
    return exp


def same(a, b):
    ca = [(canon(x["key"]), x["when"] or None) for x in a]
    cb = [(canon(x["key"]), x["when"] or None) for x in b]
    primary_ok = (not ca and not cb) or (ca and cb and ca[0] == cb[0])
    return sorted(ca) == sorted(cb) and bool(primary_ok)


def fmt(lst):
    if not lst:
        return "none"
    return ", ".join(
        f"`{x['key']}`" + (f" when `{x['when']}`" if x["when"] else "") for x in lst)


def main():
    args = sys.argv[1:]
    if args[:1] == ["--src"]:
        src = args[1]
    else:
        src = tempfile.mkdtemp(prefix="vscode-src.")
        fetch(src)
    with open(KEYMAP, encoding="utf-8") as f:
        raw = json.load(f)
    wanted = sorted({e["command"] for e in raw})
    found = extract(src, set(wanted))
    our = ours(raw)

    commands = {}
    for cid in wanted:
        if cid in QUIRE_ONLY:
            continue
        rules = found.get(cid)
        if not rules:
            sys.exit(f"no VS Code keybinding found for {cid}")
        commands[cid] = {"rules": rules, "expected": expected_for(rules)}

    table = {
        "vscode_sha": SHA,
        "generated_by": "scripts/vscode-keybindings.py",
        "quire_only": QUIRE_ONLY,
        "commands": commands,
    }
    with open(TABLE, "w", encoding="utf-8") as f:
        json.dump(table, f, indent=1, sort_keys=True)
        f.write("\n")

    # Report
    lines = [
        "# Keymap audit: quire vs VS Code defaults",
        "",
        "Generated by `scripts/vscode-keybindings.py` from VS Code "
        f"[`{SHA[:12]}`](https://github.com/microsoft/vscode/tree/{SHA}) "
        "(do not edit by hand). The extracted table is "
        "`crates/egui_quire/keymaps/vscode-extracted.json`; "
        "`crates/egui_quire/tests/keymap_audit.rs` fails when "
        "`keymaps/vscode-default.json` drifts from it.",
        "",
        "Rules: VS Code's `bindToCurrentPlatform` (a `mac`/`linux`/`win` object replaces both "
        "`primary` and `secondary`), `KeyMod.CtrlCmd` is Ctrl (Cmd on macOS), `KeyMod.WinCtrl` is "
        "Ctrl on macOS and Meta elsewhere. A rule's `when` is its `kbExpr` and the command's "
        "`precondition`. Contexts that always hold while the focused editor is in its default "
        "state are implicit; `editorHasMultipleSelections` and `findWidgetVisible` are modeled; a "
        "rule with any other context is N/A (listed with the reason).",
        "",
        "Sources read: " + ", ".join(f"`{x}`" for x in SOURCES) + ".",
        "",
        "Status: **match** (every OS equal, primary first, `when` equal), **fixed** (a match "
        "after this audit changed quire's keymap; the change is listed), **mismatch** "
        "(the drift test fails), **N/A** (no VS Code keybinding, or only unmodeled rules).",
        "",
    ]
    n_match = n_mis = 0
    rows = []
    for cid in wanted:
        if cid in QUIRE_ONLY:
            rows.append((cid, "N/A", QUIRE_ONLY[cid], None))
            continue
        c = commands[cid]
        o = our.get(cid, {os_: [] for os_ in OSES})
        status = "match" if all(same(o[os_], c["expected"][os_]) for os_ in OSES) else "mismatch"
        if status == "match" and cid in FIXED:
            status = "fixed"
            reason = FIXED[cid]
        if status != "mismatch":
            n_match += 1
        else:
            n_mis += 1
        rows.append((cid, status, reason if status == "fixed" else "", (c, o)))
    lines.append(
        f"{len(wanted)} command ids in the keymap: {n_match} match (of which "
        f"{sum(1 for r in rows if r[1] == 'fixed')} fixed in the first audit), {n_mis} mismatch, "
        f"{len(QUIRE_ONLY)} N/A (quire-only).")
    lines.append("")
    for cid, status, reason, data in rows:
        lines.append(f"## `{cid}`: {status}")
        lines.append("")
        if data is None:
            lines.append(reason)
            lines.append("")
            continue
        c, o = data
        if reason:
            lines.append(f"Fixed: {reason}.")
            lines.append("")
        lines.append("| OS | VS Code | quire |")
        lines.append("|---|---|---|")
        for os_ in OSES:
            lines.append(f"| {os_} | {fmt(c['expected'][os_])} | {fmt(o[os_])} |")
        lines.append("")
        for r in c["rules"]:
            src_line = f"- `{r['source']}`, when `{r['when'] or 'always'}`"
            if not r["modeled"]:
                keys = "; ".join(f"{os_}: {', '.join(r[os_]) or 'none'}" for os_ in OSES)
                src_line += f": N/A, {r['reason']} ({keys})"
            lines.append(src_line)
        lines.append("")
    with open(REPORT, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    print(f"{len(wanted)} ids: {n_match} match, {n_mis} mismatch, {len(QUIRE_ONLY)} quire-only")
    for cid, status, _, data in rows:
        if status == "mismatch":
            c, o = data
            for os_ in OSES:
                if not same(o[os_], c["expected"][os_]):
                    print(f"  {cid} [{os_}] vscode={fmt(c['expected'][os_])} ours={fmt(o[os_])}")


if __name__ == "__main__":
    main()
