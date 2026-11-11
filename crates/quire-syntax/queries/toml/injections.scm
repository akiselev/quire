; quire query, MIT OR Apache-2.0.
; The string under a `wgsl.<name>` key is WGSL (sakkan's docs/design/workspace.md section 10):
; `wgsl.fn_name = '''...'''`, or a pair inside a `[wgsl]` / `[x.wgsl]` table. The engine strips the
; string's quotes (`injection.strip-quotes`); escape sequences stay out of the WGSL layer.
(pair
  (dotted_key
    .
    (bare_key) @_k)
  (string) @injection.content
  (#eq? @_k "wgsl")
  (#set! injection.language "wgsl")
  (#set! injection.strip-quotes))

(table
  [
    (bare_key) @_t
    (dotted_key
      (bare_key) @_t .)
  ]
  (pair
    (_)
    (string) @injection.content)
  (#eq? @_t "wgsl")
  (#set! injection.language "wgsl")
  (#set! injection.strip-quotes))
