; Source: tree-sitter-typescript 0.23.2, queries/highlights.scm (MIT, Copyright (c) 2017 Max Brunsfeld).
; https://github.com/tree-sitter/tree-sitter-typescript. See THIRD_PARTY_NOTICES.md.
; The JavaScript base comes first so that these TypeScript patterns win (last pattern wins).
; Changed for quire: the overrides at the end.
; inherits: _javascript

; Types

(type_identifier) @type
(predefined_type) @type.builtin

((identifier) @type
 (#match? @type "^[A-Z]"))

(type_arguments
  "<" @punctuation.bracket
  ">" @punctuation.bracket)

; Variables

(required_parameter (identifier) @variable.parameter)
(optional_parameter (identifier) @variable.parameter)

; Keywords

[ "abstract"
  "declare"
  "enum"
  "export"
  "implements"
  "interface"
  "keyof"
  "namespace"
  "private"
  "protected"
  "public"
  "type"
  "readonly"
  "override"
  "satisfies"
] @keyword

; quire overrides
(type_parameters
  "<" @punctuation.bracket
  ">" @punctuation.bracket)
