; Svelte injections. quire query, MIT OR Apache-2.0.
; When several patterns inject into the same content node, the FIRST pattern wins, so the
; lang-specific patterns come before the defaults.
; "svelte.script" is resolved by quire-syntax to the language of the component's <script>:
; TypeScript if any <script lang="ts">, JavaScript otherwise.

; <script lang="ts"> (quoted or unquoted)
((script_element
  (start_tag
    (attribute
      (attribute_name) @_attr
      (quoted_attribute_value (attribute_value) @_lang)))
  (raw_text) @injection.content)
 (#eq? @_attr "lang")
 (#any-of? @_lang "ts" "typescript")
 (#set! injection.language "typescript"))
((script_element
  (start_tag
    (attribute
      (attribute_name) @_attr
      (attribute_value) @_lang))
  (raw_text) @injection.content)
 (#eq? @_attr "lang")
 (#any-of? @_lang "ts" "typescript")
 (#set! injection.language "typescript"))

; <script>
((script_element
  (raw_text) @injection.content)
 (#set! injection.language "javascript"))

; <style lang="scss"> etc. are not CSS: inject the named language (unknown names are skipped)
((style_element
  (start_tag
    (attribute
      (attribute_name) @_attr
      (quoted_attribute_value (attribute_value) @injection.language)))
  (raw_text) @injection.content)
 (#eq? @_attr "lang")
 (#not-eq? @injection.language "css"))

; <style>
((style_element
  (raw_text) @injection.content)
 (#set! injection.language "css"))

; {expression}, {#if ...}, {@html ...}, attribute={...}, ...
((svelte_raw_text) @injection.content
 (#set! injection.language "svelte.script"))
