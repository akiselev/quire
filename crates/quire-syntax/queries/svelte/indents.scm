; Svelte indents. quire query, MIT OR Apache-2.0.
; The block list follows tree-sitter-svelte's queries/indents.scm (MIT, Copyright (c) 2024
; Amaan Qureshi).
[
  (element)
  (script_element)
  (style_element)
  (if_statement)
  (each_statement)
  (await_statement)
  (key_statement)
  (snippet_statement)
] @indent

[
  (end_tag)
  (if_end)
  (each_end)
  (await_end)
  (key_end)
  (snippet_end)
  (else_if_start)
  (else_start)
  (then_start)
  (catch_start)
  ">"
  "/>"
] @outdent
