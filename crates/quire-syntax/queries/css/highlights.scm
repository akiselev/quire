; Source: tree-sitter-css 0.25.0, queries/highlights.scm (MIT, Copyright (c) 2018 Max Brunsfeld).
; https://github.com/tree-sitter/tree-sitter-css. See THIRD_PARTY_NOTICES.md.
; Changed for quire: the custom-property (--x) patterns moved after the generic ones, because the
; last pattern that captures a node wins; keyword values (plain_value) are constants.

(comment) @comment

(tag_name) @tag
(nesting_selector) @tag
(universal_selector) @tag

"~" @operator
">" @operator
"+" @operator
"-" @operator
"*" @operator
"/" @operator
"=" @operator
"^=" @operator
"|=" @operator
"~=" @operator
"$=" @operator
"*=" @operator

"and" @operator
"or" @operator
"not" @operator
"only" @operator

(attribute_selector (plain_value) @string)


(class_name) @property
(id_name) @property
(namespace_name) @property
(property_name) @property
(feature_name) @property

; quire: keyword values (auto, none, blue, ...)
(plain_value) @constant

; Custom properties (moved below the generic patterns: last pattern wins)
((property_name) @variable
 (#match? @variable "^--"))
((plain_value) @variable
 (#match? @variable "^--"))

(pseudo_element_selector (tag_name) @attribute)
(pseudo_class_selector (class_name) @attribute)
(attribute_name) @attribute

(function_name) @function

"@media" @keyword
"@import" @keyword
"@charset" @keyword
"@namespace" @keyword
"@supports" @keyword
"@keyframes" @keyword
(at_keyword) @keyword
(to) @keyword
(from) @keyword
(important) @keyword

(string_value) @string
(color_value) @string.special

(integer_value) @number
(float_value) @number
(unit) @type

[
  "#"
  ","
  "."
  ":"
  "::"
  ";"
] @punctuation.delimiter

[
  "{"
  ")"
  "("
  "}"
] @punctuation.bracket
