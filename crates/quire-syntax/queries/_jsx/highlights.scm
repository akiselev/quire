; Source: tree-sitter-javascript 0.25.0, queries/highlights-jsx.scm (MIT, Copyright (c) 2014 Max Brunsfeld).
; https://github.com/tree-sitter/tree-sitter-javascript. See THIRD_PARTY_NOTICES.md.
; Precedence: when several patterns capture the same node, the last one wins.

(jsx_opening_element (identifier) @tag (#match? @tag "^[a-z][^.]*$"))
(jsx_closing_element (identifier) @tag (#match? @tag "^[a-z][^.]*$"))
(jsx_self_closing_element (identifier) @tag (#match? @tag "^[a-z][^.]*$"))

(jsx_attribute (property_identifier) @attribute)
(jsx_opening_element (["<" ">"]) @punctuation.bracket)
(jsx_closing_element (["</" ">"]) @punctuation.bracket)
(jsx_self_closing_element (["<" "/>"]) @punctuation.bracket)
