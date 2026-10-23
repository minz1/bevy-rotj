# The music driver's commands as enums

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: a patch, not a minor
release; the mid-note gate kept on the raw byte; `FF` an explicit unhandled variant; the
decode/execute contract restated; the instrument note keeps its opcode; the SFX commands their own
enum in the same release; the proof extended first: a long-horizon sweep and a coverage record;
no golden digests and no RAM projection). The second half of the survey loop's #10, after
`docs/specs/2026-10-18-apu-registers-design.md`. Implemented in 1.1.33. No behaviour change, proven by the music test.

## What is there

The driver reads each channel's stream through `Banks::byte` and dispatches on the raw byte in two
layers (`read_event`: notes `00-7F`, instrument notes `80-DF`, `F0` stop, else `command` for
`E0-F9`, `flow_command` for `FA-FF`, `repeat`), returning a `Step` for the 6502's jump targets.
`mid_note` runs the same `command` for opcodes `F5` and up (`$82C4`: `CMP #$F5 / BCC`), with a
`mid_note: bool` that only changes the returned `Step`. `FF` has no handler in the ROM (its table
entries point into data, so the 6502 would run data as code); the port skips it on the event path
and neither advances nor consumes anything mid-note. The SFX engine (`SfxChannel::read_events`) has
its own opcode map: `00-7F` DMC note, `F0` stop, `F1` silence, `F2` hold, `F3` record table,
`F4-F6` fixed duration, `F7` clear, `F8-FA` return, `FB` call, `FC` jump, `FD`/`FE` repeat, the rest
abort the frame.

## The change

- `enum Command`, decoded from the stream: `Note(u8)`, `InstrumentNote(u8)` (the raw opcode, since
  the record is `(op - $C0) * 4` wrapping and the drum's attack frame re-reads it), `PitchDelay`,
  `PeriodLoOnly(bool)`, `Untie` (`E5`), `IdlePitch`, `PeriodMacro { on, .. }`, `Retrigger`,
  `Envelope(u16)`, `PitchMacro(u8)`, `FixedVolume(u8)` (`EC`), `Decay(u8)`, `ReleaseAt(u8)` (`EE`),
  `ClearRelease`, `Stop`, `Rest`, `Hold`, `ReleaseFrames(u8)` (`F3-F5`), `FixedDuration(u8)`,
  `ClearFixedDuration`, `Tuning(u8)`, `Transpose(u8)`, `Return`, `Call(u16)`, `Jump(u16)`,
  `Repeat { counter, count, target }`, and `Unhandled` (`FF`); and `enum SfxCommand` for the SFX
  map, the abort as its own variant.
- `decode` consumes the opcode and its fixed operands, advancing `frame.offset` (`Banks::byte` has
  no side effects and returns 0 out of range, so reading them eagerly is safe); durations (notes,
  instrument notes, `ED`/`F1`/`F2`) stay read by `latch_duration`/`next_duration`, which depend on
  `fixed_duration`. So `Call`'s return is `stream + offset`, the exhausted repeat's `+3` is already
  consumed, and the repeat's count is used only when its counter is 0; the note's
  `lo(offset + 1)` truncation and the commands' non-wrapping offsets stay as they are.
- Execution keeps each path's step: mid-note data commands return `Advance`, mid-note jumps
  `Pitch`, the exhausted mid-note repeat `Advance`; the voice and `ATTACK_DONE` logic of instrument
  notes stays where it is. The mid-note gate stays on the raw byte, a named const (`F3-F5` share a
  variant, so a variant can't say whether it acts mid-note). `mid_note: bool` becomes
  `enum Reached { Event, MidNote }`. `skip_one_byte`'s mid-note branch may fold into `Advance`.
- `docs/MUSIC.md` records `FF`'s missing handler; MODERNIZE §2 and §3 and MUSIC.md §2 say the
  driver's commands are typed.

## Proof

First, before the change:
- `every_song_to_its_loop` (`#[ignore]`d, run with `--ignored` for the release like the TAS test):
  the existing side-by-side sweep at 3600 frames per song; the 480-frame default stays.
- A coverage record from the oracle side through the existing `trace.peeks` (the port needs no
  instrumentation): the event dispatch (`$8288`, A = the opcode), the note entries (`$82DD`,
  `$8A7B`), the mid-note dispatch after the gate, the SFX dispatch and the four repeat-exhausted
  paths, as (path, opcode) pairs, asserted against a committed list so a later cut to the horizon
  shows; the horizon is long enough when the set at 1800 frames equals the set at 3600.
Then the music test (every song, the long sweep, the TAS run) unchanged, the coverage list
unchanged, the Classic suites, a sweep and a 300-branch exploration unchanged; tapes byte-identical.
