# Removing `Outside`

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: `Relift` and the rest
of the bracket mechanism go too; the tests that build or count those events; the assertion in
`loop_body` under `cfg(test)`; the docs; the proof's wording). HANDOFF item 6, approved by the
owner as its own release; MODERNIZE #4.

## What it is

`Game::frame` takes a callback, `outside: &mut impl FnMut(Outside, &mut Game)`, and calls it with
`Outside::SecondClass` when a live second-class slot's type has no ported family
(`SecondBrain::Unported`), so the oracle could run the game's own `$CE7F` update in its place
and the tape record what it changed (`Event::Outside`). Every one of the game's 100 handler types
is ported since 1.0.0, and the chain's tape holds no `Outside` event. The app passes a callback
that does nothing; so does every test but the lockstep.

## The change

- Delete `Outside`, the `outside` parameter of `Game::frame` and `loop_body`,
  `Game::report_unported_second`, the oracle's `Environment::outside` and its two
  implementations, `outside_bracket`, `name`, and the test that an `Outside` of the wrong kind is
  rejected.
- Delete what only the bracket used: `Environment::waited` and `relift` (only an `Outside`
  routine could wait for the NMI itself), the `env.waited()` branch of `run_game`,
  `GameRun::relifted` and its place in the pinned counts, the `on_demand` peeks and the `SECOND`
  bracket addresses.
- Delete `Event::Outside` and `Event::Relift` from the tape. Their wire tags (4 and 1) are
  retired, not reused, so every other event keeps its tag and a tape without them (the chain's
  holds tags 0, 2, 3, 5, 6 and 9 only) is the same bytes; the tape's `VERSION` stays. A tape
  holding a retired tag fails to read, as any unknown tag does, which a test pins; the tests that
  built those events use another.
- `SecondBrain::Unported` stays as the empty slot's brain (MODERNIZE #3 is a separate item); a
  live slot with it does nothing on its frame, as the app has always done.
- What the callback stood for becomes a rule where it fired: in `loop_body`, right after
  `step_second`, `#[cfg(test)]` asserts no live second-class slot is unported, so every lockstep
  and replay checks it and the app never panics on it.
- The docs that describe the bracket (ACTORS.md's lockstep notes, PLAN, MODERNIZE #4, the doc
  links) follow.

## Proof

The Classic suites, a sweep and a 300-branch exploration unchanged. Tapes are per checkout: the
1.1.14 chain tape is copied aside first, replayed unmodified by the new code, and `cmp`-equal to
the one the new lockstep records.
