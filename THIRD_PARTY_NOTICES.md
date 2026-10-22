# Third-party notices

quire is licensed under MIT OR Apache-2.0. Parts of it are translated from, or test data derived
from, the projects below. Each translated file also carries the notice in its header.

## Visual Studio Code / Monaco Editor

Test cases under `fixtures/monaco/` and tests under `crates/*/tests/vscode_*` are translated from
<https://github.com/microsoft/vscode> (`src/vs/editor/test/**`, `src/vs/editor/contrib/**/test/**`).
Algorithms translated from VS Code are marked in their file headers.
`crates/egui_quire/keymaps/vscode-default.json` is a subset of VS Code's default editor key
bindings (JSON has no comments, so its notice is this entry).

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

## Query files

Files under `queries/` keep their own licence header (Helix: MPL-2.0, kept as separate files;
nvim-treesitter: Apache-2.0). See each file.
