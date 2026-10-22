; JavaScript and TypeScript injections. quire query.
; Tagged templates: css`...`, styled.div`...`: the tag or member name names the language (unknown
; names are skipped); the fragments of one template are parsed together.
; Pattern from tree-sitter-javascript 0.25.0 queries/injections.scm (MIT, Copyright (c) 2014
; Max Brunsfeld). See THIRD_PARTY_NOTICES.md.
(call_expression
  function: [
    (identifier) @injection.language
    (member_expression
      property: (property_identifier) @injection.language)
  ]
  arguments: (template_string (string_fragment) @injection.content)
  (#set! injection.combined))
