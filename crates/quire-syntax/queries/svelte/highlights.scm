; Source: tree-sitter-htmlx-svelte 0.1.16, queries/highlights.scm (MIT, Copyright (c) 2025
; mixednuts). https://github.com/themixednuts/tree-sitter-htmlx. See THIRD_PARTY_NOTICES.md.
; Changed for quire: the overrides at the end. Precedence: the last pattern on a node wins.

; HTML syntax

(tag_name) @tag

(erroneous_end_tag_name) @tag.error

(doctype) @constant

(attribute_name) @attribute

(attribute_value) @string

(comment) @comment

(entity) @constant.character.escape

(text) @text

(raw_text) @text.literal

[
  "<"
  ">"
  "</"
  "/>"
  "="
] @punctuation.bracket

; HTMLX syntax

((tag_name) @type (#match? @type "^[A-Z]"))

(tag_name
  namespace: (tag_namespace) @keyword
  ":" @punctuation.delimiter
  name: (tag_local_name) @tag)

(attribute_directive) @keyword
":" @punctuation.delimiter
(attribute_identifier) @property
(attribute_modifier) @attribute
(attribute_modifiers "|" @punctuation.delimiter)

(expression) @embedded

(shorthand_attribute content: (_) @variable)

[
  "{"
  "}"
] @punctuation.bracket

"|" @punctuation.delimiter

(tag_comment kind: (line_comment) @comment)
(tag_comment kind: (block_comment) @comment)

; Svelte syntax

[
  "if"
  "each"
  "await"
  "key"
  "snippet"
  "else"
  "html"
  "debug"
  "const"
  "render"
  "attach"
] @keyword.control

(block_keyword) @keyword.control
(declaration_kind) @keyword.control

(block_open) @tag.delimiter
(block_close) @tag.delimiter

(shorthand_kind) @keyword.control
(branch_kind) @keyword.control

(expression_value) @embedded

(if_block expression: (expression) @embedded)
(else_if_clause expression: (expression_value) @embedded)

(each_block expression: (expression) @embedded)
(each_block binding: (pattern) @variable)
(each_block index: (pattern) @variable)
(each_block key: (expression) @embedded)

(await_block expression: (expression) @embedded)
(await_branch (pattern) @variable)
(await_block (pattern) @variable)
(orphan_branch (pattern) @variable)

(key_block expression: (expression) @embedded)

(snippet_block name: (snippet_name) @function)
(snippet_parameters parameter: (pattern) @variable)

(block_sigil) @keyword.control

; quire overrides
; Plain text is not a token; script/style bodies and {expressions} are painted by their
; injected languages.
(text) @none
(raw_text) @none
(js) @none
(ts) @none
(quoted_attribute_value) @string
(erroneous_end_tag_name) @error
