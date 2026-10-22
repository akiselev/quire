; Bracket pairs. quire query.
; @open and @close are matched within one match of a pattern.

("{" @open "}" @close)
("(" @open ")" @close)
(start_tag "<" @open ">" @close)
(end_tag "</" @open ">" @close)
(self_closing_tag "<" @open "/>" @close)
