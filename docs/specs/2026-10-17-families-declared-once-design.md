# The actor families declared once (1.1.x, after A3)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: the counts;
`literal` kinds; generated kind docs; the error behaviour; kind order; the pinned table a
temporary gate; why this suits the ECS step). The follow-up `docs/MODERNIZE.md` records ("`Brain`'s declaration, `new` and
`lift` still repeat the list; one declaration macro could generate them from it").

## Goal

`actor/families.rs` lists the 97 main families three times (the `Brain` variants with
their kind docs, `Brain::new`'s kind arms, `with_main_families!` for the dispatch) and the
8 second-class families four times (the variants, `new_second`, `step_second`, the
dispatch's panic arm), besides the 105 `pub mod` lines. Every family's `State` is a unit
struct (A3's survey): `Brain` is the family's identity and its dispatch, which one
declaration can generate. Afterwards a family is declared in two places, its `pub mod`
line (rustfmt does not follow macro-expanded modules) and the declaration, plus
`step_second` for a second-class one. Behaviour unchanged.

This is the table the ECS step grows from: spawning (or `follow` when a slot's kind
changes) will insert the family's `State` as a component and the dispatch becomes
per-family systems, so the unit `State` types stay. A payload-free `Family` enum with
free `step` functions would touch all 105 modules and drop those types; a `trait Family
{ const KINDS; fn step }` registry still needs a central list, loses the compile-time
duplicate check and cannot cover the second-class handlers' differing signatures (the
macro can generate such impls later if the ECS step wants them).

## Design

- One `families!` invocation in `families.rs` declares each family once:
  ```
  families! {
      main {
          0x02 => RoomExit(room_exit),
          0x04 | 0x06 => BreakableBlock(breakable_block),
          ...
      }
      second {
          0x02 => SidewaysPlatform(sideways_platform),
          ...
      }
  }
  ```
  generating `enum Brain` (`Unported` the default, then every variant with its docs),
  `Brain::new` (the main arms, `_ => Unported`), `Brain::new_second` (the second arms) and
  the main dispatch `step` (`Unported` to `engine::unported`, the second-class variants to
  the existing panic, each main variant to `state.step(&mut slot.kind, &mut slot.body,
  work, cx)`).
- `step_second` stays hand-written: the eight second-class handlers take different
  arguments (`index`, `kind`, `work`), and a macro over eight arms would hide that.
- `main_families!` and `with_main_families!` go. `follow` and `lift` are unchanged.
- Kinds are `$($kind:literal)|+`; each variant's doc is generated from them (``Kind
  `0x04 | 0x06`.``, ``Second-class kind ...``), with `$(#[$meta:meta])*` kept for the two
  hand-written docs that add something (`AceRangerShot`, `SpikeProbe`). Rows are in kind
  order (the `$CF72` table's; nothing depends on variant order, and multi-kind rows sort by
  their first kind). The generated `new`/`new_second` matches carry
  `#[deny(unreachable_patterns)]`, so a duplicate kind fails to compile at its literal; a
  duplicate variant fails with E0428; a kind in both blocks is legal (two tables). One
  macro rule, no internal rules, so a typo errors at its token. `phases!` stays.

## Tests

- A temporary gate, not committed: a test-only table of today's kinds per variant, run
  against today's code and after the macro (every kind 0-255 through `new` and
  `new_second`), then deleted, since a kept copy would be a third list. What stays: the
  Classic suites and the oracle's checks that no live slot is `Unported`.
- The Classic suites, a 1-seed sweep and a 300-branch exploration; the tapes unchanged.

## Plan

1. Tests first (the pinned table against today's code).
2. `families!`; delete the old lists and macros.
3. `docs/MODERNIZE.md`'s line on this (its `lift` is already `Brain::new`); checks, the
   `reviewer` agent, commit `The actor families are declared once`.
