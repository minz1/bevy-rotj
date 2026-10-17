# The frame's steps named (1.1.x step B2, slice 1)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: `ScriptFrame`
with the ROM's resume points; one `start_script`; `parts` everywhere; the copies' real
reasons; the corrected argument for deferring slice 2, with the roadmap reordered; the
fall-through rules listed with their checks). Step B2 of
`docs/specs/2026-10-16-modernisation-roadmap-design.md` ("the loop as a schedule"), first
slice, from a read-only survey of `Game`, `Game::frame`, `Cx`, the app's `Play` and the
oracle's lockstep.

## What the survey found

- `Game::frame` (`actor.rs`) is one function of about 85 lines with three concerns: the
  script front matter, the pause start, and the loop body (HUD, second-class slots, the
  unported report to the `Outside` hook, the player's move, the scroll, the player's draw,
  the second-class draw, the weapons, the actors, the frame's end). "Step the script, then
  store it" is written three times (a running script, a newly dispatched one, the pause).
- `Cx` is built by hand at four sites in `actor.rs` (the weapons call in `frame`,
  `draw_player`, `step_actors`, `step_second`) and one test helper, besides `Game::parts`.
- The lockstep compares at frame boundaries plus the one `Outside` hook; the app passes a
  no-op hook. The per-routine oracle tests call steps (`draw_hud`, `step_actors`,
  `step_second`...) on `&mut Game`, so the steps stay plain functions a schedule can wrap.

## Design

- `Game::frame` keeps its signature and reads as its phases:
  ```
  if self.oam.hung { return; }
  self.scroll.ppu_ctrl &= 0xFB;
  match self.script_frame(prg, chr) {
      ScriptFrame::Stepped => return,
      ScriptFrame::AtPauseCheck if self.pause_requested() => {
          self.start_script(Script::Pause(Pause::default()), prg, chr);
          return;
      }
      ScriptFrame::AtPauseCheck | ScriptFrame::PastPauseCheck => {}
  }
  self.loop_body(prg, outside);
  ```
  `enum ScriptFrame { Stepped, AtPauseCheck, PastPauseCheck }` names where the frame
  resumes. `script_frame` is today's `match self.script.take()`, with the `$D4D3` dispatch
  as `dispatch_transition(prg) -> Option<Script>`; `start_script(script, prg, chr)` steps a
  script and stores it (all three sites); `pause_requested` is the Start/`busy` test;
  `loop_body` one step a line in today's order, the unported report as
  `report_unported_second`, the player's move as `move_player(prg)` (the probe copy, the
  move, and the `stage_script & $A0 == $80` pad clear with its address) beside
  `draw_player`.
- The fall-through rules `script_frame` keeps: (a) `take()` leaves `script` empty after a
  finished script, so `$BA` is not dispatched in the frame a script finishes; (b) the
  speeds are zeroed only when no script runs; (c) `PowerUp::finish` runs, then the pause
  check; (d) a finished pause sets `gate = $FF` and a finished boot, both skip the pause
  check, while room change, next area and stage end resume at it; (e) the ending never
  finishes; (f) `stage_end` returning false resumes at the pause check. The whole-game
  chain and the playtest tapes exercise each; "Start held on the frame a pause ends" is
  `start_pauses_the_game_and_start_again_resumes_it` (Start held four frames across each
  resume, in lockstep).
- Every `Cx` comes from `Game::parts`: in `step_actors` and `step_second` inside each
  iteration, after the slot take and `load_aliases`, with `game.work.slot` set before and
  `cx` dropped before `store_aliases`; writes and their order unchanged.
- The copies: `draw_player`'s `pressed`/`pad` stay (`step_draw` writes them while `Cx`
  holds `world` shared); the `probe` copy stays (`parts` borrows all of `Game` and `probe`
  sits outside `Cx`); `step_move`'s `pad` parameter goes (it reads `cx.world.pad`, which
  does not change during the call). Moving the pad bytes out of `World` belongs to B4
  (input as actions), which reshapes them.
- Slice 2 (the pieces as `bevy_ecs` resources and the loop body as a chained schedule)
  waits for B3's spike, which decides the actor walk's shape: an exclusive system over slot
  entities, or over a `Slots` resource. That shape fixes `Cx` and the systems' signatures;
  built now, slice 2 would give every step `ResMut` parameters and move the oracle harness
  from `&mut Game` to a `bevy_ecs::World`, then have its actor half rewritten after B3. The
  cost moves rather than disappears: B3's spike brings in `bevy_ecs` (a `Game` resource and
  one exclusive walk are enough) and records the clean-build and chain-test times B2 asked
  for. The roadmap's Phase B order is updated to match (B3's spike before B2's schedule).

## Tests

- The Classic suites, the per-routine oracle tests, a 1-seed sweep and a 300-branch
  exploration; the tapes unchanged.

## Plan

1. Split `frame`; `start_script`, `dispatch_transition`, `move_player`; `parts` at the
   hand-built sites; `step_move` without `pad`; the roadmap's Phase B order.
2. Checks, the `reviewer` agent, commit `The frame's steps are named`.
