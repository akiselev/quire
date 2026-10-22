; Svelte injections (tree-sitter-htmlx-svelte). quire query, MIT OR Apache-2.0.
; Patterns after tree-sitter-htmlx-svelte 0.1.16 queries/injections.scm and
; queries/htmlx/injections.scm (MIT, Copyright (c) 2025 mixednuts). See THIRD_PARTY_NOTICES.md.
; When several patterns inject into the same content node, the FIRST pattern wins, so the
; lang-specific patterns come before the defaults.

; <script lang="ts">
((element
  (start_tag
    (tag_name) @_tag
    (attribute
      (attribute_name) @_attr
      (quoted_attribute_value (attribute_value) @_lang)))
  (raw_text) @injection.content)
 (#eq? @_tag "script")
 (#eq? @_attr "lang")
 (#any-of? @_lang "ts" "typescript")
 (#set! injection.language "typescript"))
((element
  (start_tag
    (tag_name) @_tag
    (attribute
      (attribute_name) @_attr
      (unquoted_attribute_value) @_lang))
  (raw_text) @injection.content)
 (#eq? @_tag "script")
 (#eq? @_attr "lang")
 (#any-of? @_lang "ts" "typescript")
 (#set! injection.language "typescript"))

; <script>
((element
  (start_tag (tag_name) @_tag)
  (raw_text) @injection.content)
 (#eq? @_tag "script")
 (#set! injection.language "javascript"))

; <style lang="scss"> etc.: the named language (unknown names claim the node and are skipped)
((element
  (start_tag
    (tag_name) @_tag
    (attribute
      (attribute_name) @_attr
      (quoted_attribute_value (attribute_value) @injection.language)))
  (raw_text) @injection.content)
 (#eq? @_tag "style")
 (#eq? @_attr "lang"))

; <style>
((element
  (start_tag (tag_name) @_tag)
  (raw_text) @injection.content)
 (#eq? @_tag "style")
 (#set! injection.language "css"))

; Expressions everywhere ({expr}, attributes, block heads, patterns, @tags): the grammar types
; their content by the component's <script lang>.
((ts) @injection.content
 (#set! injection.language "typescript"))
((js) @injection.content
 (#set! injection.language "javascript"))
((snippet_type_parameters) @injection.content
 (#set! injection.language "typescript"))
