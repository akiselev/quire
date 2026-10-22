; JavaScript and TypeScript indents. quire query, MIT OR Apache-2.0.
; @indent: lines after the node's first line, up to its last, get one more level.
; @outdent: a line that starts with this token gets one level less.
[
  (statement_block)
  (class_body)
  (object)
  (object_pattern)
  (array)
  (array_pattern)
  (switch_body)
  (switch_case)
  (switch_default)
  (arguments)
  (formal_parameters)
  (named_imports)
  (export_clause)
  (parenthesized_expression)
  (template_substitution)
] @indent

[
  "}"
  "]"
  ")"
] @outdent
