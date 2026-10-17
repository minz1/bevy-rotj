# The second-class families as their own enum (1.1.x, after the families declared once)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: the oracle's
second-slot check among the users; the docs that change). The follow-up `docs/MODERNIZE.md` and the families-once review name.

## Goal

`Brain` holds both the 97 main families and the 8 second-class ones (slots 24-31,
`$CE7F`'s table), so the main dispatch needs an arm that panics on a second-class variant
in a main slot, and `step_second` has a `_ => {}` arm that would silently skip a new
second-class family, or a main one put in a second-class slot. The two are different
tables in the game (`$CF72`, `$CEC3`) with different handler signatures. Behaviour
unchanged.

## Design

- `families!`'s `second` block generates `pub enum SecondBrain { Unported (default), ..}`
  and `SecondBrain::new(kind)` (today's `Brain::new_second`), with the same generated
  docs and `#[deny(unreachable_patterns)]`. `Brain` keeps only `Unported` and the main
  families; the main dispatch loses its panic arm.
- `Second::brain: SecondBrain` (`slots.rs`); its users: `Slots::lift`, `spawn::place`,
  `collision::crumble`, `actor.rs`'s unported check and `oracle.rs`'s
  `assert_second_phases` (`SecondBrain::Unported`). `SecondBrain` derives what `Brain`
  does, `Unported` the default; its variants' docs read "Kind `0x06`." as `Brain`'s do.
- `step_second` matches `SecondBrain` exhaustively: `Unported => {}` explicitly (today's
  `_` arm, which only ever matched `Unported` there), each family's arm as today.
- `Brain::new_second` goes. The name `SecondBrain` follows `Second`, `step_second` and the
  slots' "second-class" wording.

## Tests

- A temporary gate as for the families declared once: every kind 0-255 through
  `SecondBrain::new` gives the variant `Brain::new_second` gave (by name), then deleted.
- The Classic suites; the oracle's no-`Unported` check for second-class slots.

## Plan

1. The gate's snapshot on today's code.
2. `SecondBrain` from the macro; the users; `step_second` exhaustive; the `families!` doc
   comment and `docs/MODERNIZE.md` (the `new_second`/`_ => {}` lines and the "Next" item
   this closes).
3. The gate, then checks, the `reviewer` agent, commit `The second-class families have their
   own enum`.
