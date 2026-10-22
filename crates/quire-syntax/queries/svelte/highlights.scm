; Svelte (tree-sitter-svelte-ng 1.0.2). quire query, MIT OR Apache-2.0.
; The block-keyword groups are taken from tree-sitter-svelte's queries/highlights.scm (MIT,
; Copyright (c) 2024 Amaan Qureshi); the HTML part is written for quire, because the bundled
; query inherits nvim-treesitter's html query. See THIRD_PARTY_NOTICES.md.
; Script, style and {expression} contents are highlighted by their injected languages.
; Precedence: when several patterns capture the same node, the last one wins.

; HTML
(tag_name) @tag
((tag_name) @type
 (#match? @type "^[A-Z]"))
((tag_name) @keyword
 (#match? @keyword "^svelte:"))
(erroneous_end_tag_name) @error
(doctype) @constant
"doctype" @keyword
(attribute_name) @attribute
((attribute_name) @keyword
 (#match? @keyword "^(on|bind|class|style|use|transition|in|out|animate|let):"))
(attribute_value) @string
(quoted_attribute_value) @string
(comment) @comment
(entity) @string.special
(raw_text) @none

[
  "<"
  ">"
  "</"
  "/>"
  "<!"
] @punctuation.bracket

"=" @operator

[
  "\""
  "'"
] @string

; Svelte blocks and tags
[
  "as"
  "key"
  "html"
  "snippet"
  "render"
] @keyword

"const" @keyword

[
  "if"
  "else"
  "else if"
  "then"
] @keyword.conditional

"each" @keyword.repeat

"await" @keyword.coroutine

"catch" @keyword.exception

"debug" @keyword.debug

(snippet_name) @function

[
  "{"
  "}"
  "("
  ")"
] @punctuation.bracket

[
  "#"
  ":"
  "/"
  "@"
] @punctuation.special
