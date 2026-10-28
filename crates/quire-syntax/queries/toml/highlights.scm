; Source: tree-sitter-toml-ng 0.7.0, queries/highlights.scm (MIT, Copyright (c) Ika and Amaan Qureshi).
; https://github.com/tree-sitter-grammars/tree-sitter-toml. See THIRD_PARTY_NOTICES.md.
; Changed for quire: the pair patterns capture the key, not the whole pair (the last pattern that
; captures a node wins, so upstream's inner (bare_key) @type kept pair keys as types); quoted keys
; of a pair are properties too; escape sequences are string.escape.

; Properties
;-----------

(bare_key) @type

(quoted_key) @string

(pair
  (bare_key) @property)

(pair
  (quoted_key) @property)

(pair
  (dotted_key
    (bare_key) @property))

(pair
  (dotted_key
    (quoted_key) @property))

; Literals
;---------

(boolean) @boolean

(comment) @comment

(string) @string

(escape_sequence) @string.escape

[
  (integer)
  (float)
] @number

[
  (offset_date_time)
  (local_date_time)
  (local_date)
  (local_time)
] @string.special

; Punctuation
;------------

[
  "."
  ","
] @punctuation.delimiter

"=" @operator

[
  "["
  "]"
  "[["
  "]]"
  "{"
  "}"
] @punctuation.bracket
