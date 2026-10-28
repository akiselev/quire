# Third-party notices

quire is licensed under MIT OR Apache-2.0. Parts of it are translated from, or test data derived
from, the projects below. Each translated file also carries the notice in its header.

## Visual Studio Code / Monaco Editor

Test cases under `fixtures/monaco/` and tests under `crates/*/tests/vscode_*` are translated from
<https://github.com/microsoft/vscode> (`src/vs/editor/test/**`, `src/vs/editor/contrib/**/test/**`).
Algorithms translated from VS Code are marked in their file headers: among them the suggest
scorer, completion model, suggest session and snippet parser and session in
`crates/quire-core/src/completion/` (`src/vs/base/common/filters.ts`,
`src/vs/editor/contrib/suggest/browser/**`, `src/vs/editor/contrib/snippet/browser/**`). The
tests `crates/quire-core/tests/{suggest_filters,suggest_model,suggest_session,snippet_parser,snippet_session}.rs`
are translated from `src/vs/base/test/common/filters.test.ts` and
`src/vs/editor/contrib/{suggest,snippet}/test/**`. The hover and parameter hints models in
`crates/quire-core/src/{hover,parameter_hints}.rs` are translated from
`src/vs/editor/contrib/{hover,parameterHints}/browser/**`, and their tests
`crates/quire-core/tests/{hover,parameter_hints}.rs` from
`src/vs/editor/contrib/{hover,parameterHints}/test/**`.
`crates/egui_quire/keymaps/vscode-default.json` is a subset of VS Code's default editor key
bindings (JSON has no comments, so its notice is this entry).
`crates/egui_quire/src/wrap.rs` and `crates/skelter_quire/src/wrap.rs` translate the line breaking of
VS Code's `monospaceLineBreaksComputer.ts`; skelter_quire reads the same
keymap file.

```
MIT License

Copyright (c) 2015 - present Microsoft Corporation

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## svelters

Two files of `quire-text` are ported from svelters, and each carries a provenance line in its
header:

- `crates/quire-text/src/words/native.rs`, from `crates/svelters-egui/src/native/words.rs` @ `4365d5a9`
  (FX1 word rules);
- `crates/quire-text/src/history/native.rs`, from `crates/svelters-egui/src/native/undo.rs` @ `e17ebbff`
  (FX3 undo models).

svelters' notice, verbatim from its `LICENSE` at `4365d5a9`:

```
MIT License

Copyright (c) 2021-2026 Alexander Kiselev

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Query files

Files under `queries/` keep their own licence header (Helix: MPL-2.0, kept as separate files;
nvim-treesitter: Apache-2.0). See each file.

## tree-sitter grammar queries (quire-syntax)

Query files under `crates/quire-syntax/queries/` are copied or adapted from the grammars'
bundled queries; each file's header names its source and what quire changed. Files without a
source line are quire's own (MIT OR Apache-2.0). No Helix (MPL-2.0) or nvim-treesitter query is
copied.

| quire file | Source |
|---|---|
| `_javascript/highlights.scm` | tree-sitter-javascript 0.25.0 `queries/highlights.scm` (+ quire overrides) |
| `_jsx/highlights.scm` | tree-sitter-javascript 0.25.0 `queries/highlights-jsx.scm` |
| `javascript/highlights.scm` | tree-sitter-javascript 0.25.0 `queries/highlights-params.scm` |
| `_javascript/injections.scm` | pattern from tree-sitter-javascript 0.25.0 `queries/injections.scm` |
| `typescript/highlights.scm` | tree-sitter-typescript 0.23.2 `queries/highlights.scm` (+ quire overrides) |
| `css/highlights.scm` | tree-sitter-css 0.25.0 `queries/highlights.scm` (reordered, + quire overrides) |
| `svelte/highlights.scm` | tree-sitter-htmlx-svelte 0.1.16 `queries/highlights.scm` (+ quire overrides) |
| `svelte/folds.scm` | tree-sitter-htmlx-svelte 0.1.16 `queries/folds.scm` |
| `svelte/injections.scm`, `svelte/indents.scm` | patterns after tree-sitter-htmlx-svelte 0.1.16 queries |
| `toml/highlights.scm` | tree-sitter-toml-ng 0.7.0 `queries/highlights.scm` (+ quire overrides) |

### tree-sitter-javascript, tree-sitter-typescript, tree-sitter-css

<https://github.com/tree-sitter/tree-sitter-javascript>,
<https://github.com/tree-sitter/tree-sitter-typescript>,
<https://github.com/tree-sitter/tree-sitter-css>

```
The MIT License (MIT)

Copyright (c) 2014 Max Brunsfeld (tree-sitter-javascript)
Copyright (c) 2017 Max Brunsfeld (tree-sitter-typescript)
Copyright (c) 2018 Max Brunsfeld (tree-sitter-css)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### tree-sitter-toml-ng

<https://github.com/tree-sitter-grammars/tree-sitter-toml> (crate `tree-sitter-toml-ng` 0.7.0, the
optional `toml` feature of quire-syntax). The published crate declares `license = "MIT"` and its
authors in `Cargo.toml` and `tree-sitter.json` but ships no licence file; the MIT text follows,
with the authors the crate names.

```
MIT License

Copyright (c) Ika <ikatyang@gmail.com> (https://github.com/ikatyang)
Copyright (c) Amaan Qureshi <amaanq12@gmail.com>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### tree-sitter-htmlx-svelte

<https://github.com/themixednuts/tree-sitter-htmlx>

```
MIT License

Copyright (c) 2025 mixednuts

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Test fonts

### JetBrains Mono (skelter_quire tests)

`crates/skelter_quire/tests/fonts/JetBrainsMono-Regular.subset.ttf` is an ASCII subset
(U+0020–U+007E, layout features `calt`, `liga`, `ccmp`, `locl` kept, hinting dropped, made with
fontTools `pyftsubset`) of JetBrains Mono 2.304 Regular, <https://github.com/JetBrains/JetBrainsMono>.
It is used only by tests and is not part of any crate's build. Copyright 2020 The JetBrains Mono
Project Authors, licensed under the SIL Open Font License, Version 1.1. The full licence text is
`crates/skelter_quire/tests/fonts/JetBrainsMono-OFL.txt`. The font declares no Reserved Font Name.
