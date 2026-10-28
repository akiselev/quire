; Bracket pairs. quire query, MIT OR Apache-2.0.
; @open and @close are matched within one match of a pattern.

("[" @open "]" @close)
("[[" @open "]]" @close)
("{" @open "}" @close)
