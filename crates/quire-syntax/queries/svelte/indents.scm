; Svelte indents (tree-sitter-htmlx-svelte). quire query, MIT OR Apache-2.0.
; @indent: lines after the node's first line, up to its last, get one more level.
; @outdent: a line that starts with this node or token gets one level less.
; The block lists follow tree-sitter-htmlx-svelte 0.1.16 queries/indents.scm (MIT, Copyright (c)
; 2025 mixednuts).
[
  (element)
  (if_block)
  (each_block)
  (await_block)
  (key_block)
  (snippet_block)
] @indent

[
  (end_tag)
  (block_end)
  (else_if_clause)
  (else_clause)
  (await_branch)
  (orphan_branch)
  ">"
  "/>"
] @outdent
