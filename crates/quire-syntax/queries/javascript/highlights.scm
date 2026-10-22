; JavaScript (with JSX). quire query.
; inherits: _javascript,_jsx

; Source: tree-sitter-javascript 0.25.0, queries/highlights-params.scm (MIT, Copyright (c) 2014
; Max Brunsfeld). JavaScript only: TypeScript wraps parameters in required_parameter.
(formal_parameters
  [
    (identifier) @variable.parameter
    (array_pattern
      (identifier) @variable.parameter)
    (object_pattern
      [
        (pair_pattern value: (identifier) @variable.parameter)
        (shorthand_property_identifier_pattern) @variable.parameter
      ])
  ]
)
