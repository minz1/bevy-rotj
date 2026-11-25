use super::*;
use rotj_core::digest::fnv1a;
use rotj_core::map::Chain;
use rotj_core::render::{Mirroring, Picture, SCREEN_W, mask};
use rotj_core::sim::{Motion, Transition};

/// `rom`'s `Prg`, its levels imported once for every test.
pub(crate) fn prg(rom: &rotj_rom::Rom) -> Prg {
    static LEVELS: std::sync::OnceLock<Levels> = std::sync::OnceLock::new();
    let levels =
        LEVELS.get_or_init(|| Levels::original(&rom.prg, &rom.chr).expect("the test ROM's levels"));
    Prg::new(rom.prg.clone(), levels.clone())
}

/// A committed FNV-1a digest (`tests/data/digests.txt`).
fn digest(name: &str) -> u64 {
    rotj_core::digest::table(include_str!("../../digests.txt"), name)["-"]
}

/// Steps up to `frames` frames holding `pad`, stopping when `until` holds.
pub(crate) fn step_until(
    play: &mut Play,
    frames: usize,
    pad: Buttons,
    until: impl Fn(&Play) -> bool,
) -> bool {
    (0..frames).any(|_| {
        play.step(pad);
        until(play)
    })
}

pub(crate) fn on_title(play: &Play) -> bool {
    play.session().on_title()
}

/// The game at power-on, or `None` (skipped) without the ROM.
fn powered_on(forced: Option<u8>) -> Option<Play> {
    let rom = rotj_rom::for_test()?;
    Some(Play::power_on(prg(rom), rom.chr.clone(), forced))
}

/// The emulator's screenshots crop 8 lines top and bottom (256x224).
const OVERSCAN: usize = 8;

/// The title the port boots into is the emulator's own frame 300 (the attract
/// capture's screenshot), pixel for pixel.
#[test]
fn the_title_draws_the_emulators_frame() {
    let Some(mut play) = powered_on(None) else {
        return;
    };
    assert!(
        step_until(&mut play, 400, Buttons::NONE, on_title),
        "no title"
    );
    for _ in 0..10 {
        play.step(Buttons::NONE);
    }
    let ours = picture(&play);
    let visible = &ours[OVERSCAN * SCREEN_W..(OVERSCAN + 224) * SCREEN_W];
    assert_eq!(
        fnv1a(visible),
        digest("title-300"),
        "the title differs from fceux's frame 300 (regenerate tools/emu/attract for the pixels)"
    );
}

/// The intro plays from power-on: its pictures change, the raster IRQ splits some.
#[test]
fn the_intro_animates() {
    let Some(mut play) = powered_on(None) else {
        return;
    };
    let mut pictures = std::collections::BTreeSet::new();
    let mut split = 0;
    for _ in 0..270 {
        play.step(Buttons::NONE);
        split += usize::from(play.session().raster_chain() == Some(Chain::Intro));
        pictures.insert(picture(&play));
    }
    assert!(pictures.len() > 60, "{} distinct pictures", pictures.len());
    assert!(split > 20, "{split} split frames");
}

/// Start on the title plays the chosen line's flash, the banner and stage 1-1 to its
/// first loop frame, the game's own songs requested on the way: the title's `$01`
/// (`$61AE`) and the stage's `$04` (`$D973`).
#[test]
fn start_on_the_title_reaches_gameplay() {
    let Some(mut play) = powered_on(None) else {
        return;
    };
    let mut songs = Vec::new();
    let mut step = |play: &mut Play, pad: Buttons| {
        let sound = play.step(pad);
        if let Some(song) = sound[1] {
            songs.push(song.byte());
        }
    };
    while !on_title(&play) {
        step(&mut play, Buttons::NONE);
    }
    step(&mut play, Buttons::START);
    for _ in 0..400 {
        if !play.booting() {
            break;
        }
        step(&mut play, Buttons::START);
    }
    assert!(!play.booting(), "the stage never started");
    assert_eq!(play.session().area(), 0);
    assert_eq!(songs, [0x01, 0x04]);
    assert_plays(&mut play, 60, Buttons::RIGHT);
}

/// `ROTJ_LEVEL` forces the title's level where the game reads it (`$D3D8`).
#[test]
fn a_forced_level_starts_there_from_the_title() {
    let Some(mut play) = powered_on(Some(5)) else {
        return;
    };
    assert!(
        step_until(&mut play, 400, Buttons::NONE, on_title),
        "no title"
    );
    play.step(Buttons::START);
    assert!(step_until(&mut play, 400, Buttons::NONE, |play| {
        !play.booting()
    }));
    assert_eq!(play.session().area(), 0x0C, "stage 3-2's area");
}

fn play() -> Option<Play> {
    start(0)
}

/// The game at `level`'s first loop frame, or `None` (skipped) without the ROM.
fn start(level: usize) -> Option<Play> {
    let rom = rotj_rom::for_test()?;
    Some(Play::start(prg(rom), rom.chr.clone(), level))
}

#[test]
fn paused_follows_the_games_pause() {
    let Some(mut play) = play() else {
        return;
    };
    for _ in 0..30 {
        play.step(Buttons::NONE);
    }
    assert!(!play.paused());
    play.step(Buttons::START);
    for _ in 0..5 {
        play.step(Buttons::NONE);
    }
    assert!(play.paused(), "Start pauses");
    play.step(Buttons::START);
    for _ in 0..5 {
        play.step(Buttons::NONE);
    }
    assert!(!play.paused(), "Start again resumes");
}

/// The marks of the entries visible on the OAM page.
pub(crate) fn visible_marks(play: &Play) -> impl Iterator<Item = Mark> + '_ {
    let session = play.session();
    session
        .oam()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(*session.marks())
        .filter(|&(&bytes, _)| OamEntry::from(bytes).is_visible())
        .map(|(_, mark)| mark)
}

/// The shown sprites `mark` placed, on screen.
pub(crate) fn shown(play: &Play, mark: Mark) -> usize {
    visible_marks(play).filter(|&at| at == mark).count()
}

/// Every marked sprite shown is drawable as UI exactly as the PPU draws it.
fn assert_drawable(play: &Play) {
    let session = play.session();
    if let Some(why) =
        rotj_core::sim::oam::undrawable(session.oam(), session.marks(), session.lines())
    {
        panic!("{why}");
    }
}

/// One step with `pad`, checking the sprites it shows, which the state before it drew.
fn checked_step(play: &mut Play, pad: Buttons) {
    play.step(pad);
    assert_drawable(play);
}

#[test]
fn classic_shows_the_games_oam_and_faithful_parks_the_ui() {
    let Some(mut play) = play() else {
        return;
    };
    for _ in 0..4 {
        play.step(Buttons::NONE);
    }
    let session = play.session();
    assert_eq!(&session.display_oam(false), session.oam());
    let parked = session.display_oam(true);
    for entry in 0..64 {
        let (ours, game) = (
            &parked[entry * 4..entry * 4 + 4],
            &session.oam()[entry * 4..entry * 4 + 4],
        );
        if session.marks()[entry].is_ui() {
            assert_eq!((ours[0], &ours[1..]), (OamEntry::PARKED_Y, &game[1..]));
        } else {
            assert_eq!(ours, game);
        }
    }
    assert!(session.marks().contains(&Mark::Hud));
}

#[test]
fn the_label_carries_over_one_step() {
    let Some(mut play) = play() else {
        return;
    };
    let mut carried = 0;
    for _ in 0..8 {
        let before = marked(play.session().oam(), play.session().marks(), Mark::Label);
        play.step(Buttons::NONE);
        assert_eq!(play.label.entries(), before);
        carried += usize::from(!play.label.carried(play.session().marks()).is_empty());
    }
    assert!(
        carried >= 3,
        "the even frames carry the odd frames' label: {carried}"
    );
}

#[test]
fn the_title_shows_only_its_cursor_as_ui() {
    let Some(mut play) = powered_on(None) else {
        return;
    };
    assert!(
        step_until(&mut play, 400, Buttons::NONE, on_title),
        "no title"
    );
    for (i, pad) in [
        Buttons::RIGHT,
        Buttons::NONE,
        Buttons::LEFT,
        Buttons::NONE,
        Buttons::SELECT,
        Buttons::NONE,
        Buttons::NONE,
        Buttons::NONE,
    ]
    .into_iter()
    .enumerate()
    {
        let title = on_title(&play);
        checked_step(&mut play, pad);
        let visible = visible_marks(&play).count();
        if title {
            assert!(shown(&play, Mark::Title) > 0, "step {i}");
            assert_eq!(shown(&play, Mark::Title), visible, "step {i}");
        }
    }
}

#[test]
fn the_password_entry_shows_nine_six_or_none() {
    let Some(mut play) = powered_on(None) else {
        return;
    };
    assert!(
        step_until(&mut play, 400, Buttons::NONE, on_title),
        "no title"
    );
    for pad in [Buttons::RIGHT, Buttons::NONE, Buttons::A, Buttons::NONE] {
        checked_step(&mut play, pad);
    }
    let mut counts = std::collections::BTreeSet::new();
    let mut entered = false;
    for step in 0..1500 {
        let count = shown(&play, Mark::Password);
        counts.insert(count);
        let pad = if count == 9 && !entered && step % 2 == 0 {
            entered = step > 300;
            if entered { Buttons::A } else { Buttons::DOWN }
        } else {
            Buttons::NONE
        };
        checked_step(&mut play, pad);
    }
    assert!(counts.is_subset(&[0, 6, 9].into()), "{counts:?}");
    assert!(counts.contains(&9) && counts.contains(&6), "{counts:?}");
}

#[test]
fn the_pause_card_is_all_ui() {
    let Some(mut play) = play() else {
        return;
    };
    for _ in 0..30 {
        checked_step(&mut play, Buttons::NONE);
    }
    assert!(shown(&play, Mark::Hud) > 0);
    checked_step(&mut play, Buttons::START);
    let mut paused = 0;
    for _ in 0..30 {
        let was = play.paused();
        checked_step(&mut play, Buttons::NONE);
        if was {
            let visible = visible_marks(&play).count();
            assert_eq!(shown(&play, Mark::Pause), visible);
            paused += 1;
        }
    }
    assert!(paused > 20, "{paused} paused steps");
    checked_step(&mut play, Buttons::START);
}

/// Area `$1C`'s IRQ reads the scroll the loop moves, so its runs come from the lines after it.
#[test]
fn a_split_frame_is_drawn_from_the_lines_after_its_loop() {
    let Some(rom) = rotj_rom::for_test() else {
        return;
    };
    let mut play = Play::start(prg(rom), rom.chr.clone(), 11);
    for frame in 0..60 {
        let pad = if frame % 4 < 2 {
            Buttons::B | Buttons::RIGHT
        } else {
            Buttons::RIGHT
        };
        play.step(pad);
        let session = play.session();
        let modern = session.snapshot().modern.as_ref().expect("drawn Modern");
        let lines = session.lines();
        let mut covered = 0;
        for piece in modern.view.pieces.iter().flatten().copied() {
            assert_eq!(piece.line(), covered, "frame {frame}: contiguous pieces");
            covered = piece.end();
            let line = lines[piece.line()];
            match piece {
                rotj_core::sim::modern::Piece::Run(run) => {
                    assert_eq!(run.top, line, "frame {frame}: run matches lines after loop");
                }
                rotj_core::sim::modern::Piece::Map { left, .. } => {
                    let period = if line.mirroring == rotj_core::render::Mirroring::SideBySide {
                        512
                    } else {
                        256
                    };
                    let scroll_x = i32::from(
                        (((line.v >> 10) & 1) << 8)
                            | ((line.v & 0x1F) << 3)
                            | u16::from(line.fine_x),
                    );
                    assert_eq!(
                        left.rem_euclid(period),
                        scroll_x.rem_euclid(period),
                        "frame {frame}: map piece aligns with IRQ scroll after the loop"
                    );
                }
            }
        }
        assert_eq!(covered, lines.len(), "frame {frame}: full screen covered");
    }
}

/// Arms `$BA`/`$BB` and runs the script that follows to its end.
pub(crate) fn transition(play: &mut Play, kind: Transition, arg: u8) {
    play.session_mut().request_transition(kind, arg);
    play.step(Buttons::NONE);
    for _ in 0..1200 {
        if !play.session().in_script() {
            break;
        }
        play.step(Buttons::NONE);
    }
    assert!(!play.session().in_script(), "{kind:?} never ended");
}

/// The area's last room into its boss room (`$0404` bit 7, `$BB3D`) and Batman's entry.
pub(crate) fn enter_the_boss_room(play: &mut Play, area: u8) {
    play.session_mut().arm_boss_entry();
    play.step(Buttons::NONE);
    for _ in 0..1200 {
        if !play.session().in_script() {
            break;
        }
        play.step(Buttons::NONE);
    }
    assert!(!play.session().in_script(), "boss entry never ended");
    assert_eq!(play.session().area(), area);
    for _ in 0..200 {
        if play.session().player_motion() != Motion::BossEntry {
            break;
        }
        play.step(Buttons::NONE);
    }
    assert_ne!(
        play.session().player_motion(),
        Motion::BossEntry,
        "Batman never entered area {area:#04X}'s boss room"
    );
}

/// The frame `play` composes right now, as NES colour indices.
fn picture(play: &Play) -> Vec<u8> {
    let mut frame = Frame::new();
    play.compose(&mut frame, MAX_SPRITES_PER_LINE, false);
    frame.indices().to_vec()
}

/// Plays `frames` frames holding `pad`: the picture must be neither blank nor frozen.
fn assert_plays(play: &mut Play, frames: usize, pad: Buttons) {
    let before = picture(play);
    for _ in 0..frames {
        play.step(pad);
    }
    assert!(!play.session().in_script(), "a script started");
    let after = picture(play);
    let backdrop = after[0];
    assert!(
        after.iter().any(|&pixel| pixel != backdrop),
        "the frame is blank"
    );
    assert_ne!(before, after, "the frame is stuck");
}

/// Where the room is: `(area, room, $DD)`.
fn room(play: &Play) -> (u8, u8, u8) {
    let session = play.session();
    (session.area(), session.room(), session.orientation())
}

/// Stage 4 as the TAS plays it: 4-1, then 4-2's rooms (the slide into a vertical
/// room, `$BB = $61`; the fade out of it, `$E0`; the slide into the last, `$62`) and
/// the stage 4 boss room.
#[test]
fn stage_4_plays_through_its_rooms_to_the_boss() {
    let Some(mut play) = start(6) else {
        return;
    };
    assert_plays(&mut play, 90, Buttons::RIGHT);
    transition(&mut play, Transition::NextArea, 0);
    assert_eq!(room(&play), (0x12, 0, 0x00));
    assert_plays(&mut play, 60, Buttons::RIGHT);
    for (arg, next) in [
        (0x61, (0x12, 2, 0x40)),
        (0xE0, (0x12, 4, 0x00)),
        (0x62, (0x12, 6, 0x40)),
    ] {
        transition(&mut play, Transition::RoomChange, arg);
        assert_eq!(room(&play), next);
        assert_plays(&mut play, 60, Buttons::RIGHT);
    }
    enter_the_boss_room(&mut play, 0x14);
    assert_plays(&mut play, 60, Buttons::NONE);
}

/// Stage 5, which has no rooms and no boss: 5-1, then the one-screen 5-2.
#[test]
fn stage_5_plays_both_areas() {
    let Some(mut play) = start(8) else {
        return;
    };
    assert_plays(&mut play, 90, Buttons::RIGHT);
    transition(&mut play, Transition::NextArea, 0);
    assert_eq!(room(&play), (0x18, 0, 0x00));
    assert_plays(&mut play, 90, Buttons::RIGHT);
}

/// Stage 6 as the TAS plays it: 6-1's two slides (`$BB = $61`), 6-2 and the boss room.
#[test]
fn stage_6_plays_through_its_rooms_to_the_boss() {
    let Some(mut play) = start(10) else {
        return;
    };
    assert_plays(&mut play, 90, Buttons::RIGHT);
    for next in [(0x1A, 2, 0x00), (0x1A, 4, 0x00)] {
        transition(&mut play, Transition::RoomChange, 0x61);
        assert_eq!(room(&play), next);
        assert_plays(&mut play, 60, Buttons::RIGHT);
    }
    transition(&mut play, Transition::NextArea, 0);
    assert_eq!(room(&play), (0x1C, 0, 0x00));
    assert_plays(&mut play, 60, Buttons::RIGHT);
    enter_the_boss_room(&mut play, 0x1E);
    assert_plays(&mut play, 60, Buttons::NONE);
}

/// Stage 7 as the TAS plays it: 7-1's fade (`$BB = $C1`) and slide (`$61`), then the
/// last boss's room.
#[test]
fn stage_7_plays_through_its_rooms_to_the_last_boss() {
    let Some(mut play) = start(12) else {
        return;
    };
    assert_plays(&mut play, 90, Buttons::RIGHT);
    for (arg, next) in [(0xC1, (0x20, 2, 0x00)), (0x61, (0x20, 4, 0x00))] {
        transition(&mut play, Transition::RoomChange, arg);
        assert_eq!(room(&play), next);
        assert_plays(&mut play, 60, Buttons::RIGHT);
    }
    enter_the_boss_room(&mut play, 0x22);
    assert_plays(&mut play, 60, Buttons::NONE);
}

/// The room start's page, drawn through the queue model, is the captured
/// opening nametable.
#[test]
fn the_first_page_is_the_captured_nametable() {
    let Some(play) = play() else {
        return;
    };
    assert_eq!(
        fnv1a(&play.session().ppu().nametables[0][..960]),
        digest("stage-1-1-page-0"),
        "the first page differs from fceux's (tools/emu/stage, frame 700)"
    );
}

/// Half a minute holding Right, jumping and firing: the loop, its column
/// streaming, the spawns, the actors and the shots all run in the port,
/// through the room change at the end of the first room.
#[test]
fn the_game_runs_holding_right() {
    let Some(mut play) = play() else {
        return;
    };
    for frame in 0..1800 {
        let jump = if frame % 40 < 12 {
            Buttons::A
        } else {
            Buttons::NONE
        };
        let fire = if frame % 8 < 2 {
            Buttons::B
        } else {
            Buttons::NONE
        };
        play.step(Buttons::RIGHT | jump | fire);
    }
    assert_eq!(play.session().room(), 2, "the room change never ran");
    let mut frame = Frame::new();
    play.compose(&mut frame, MAX_SPRITES_PER_LINE, false);
    assert!(frame.rgba().iter().any(|&byte| byte != 0));
}

/// Left standing in stage 1-2, Batman is pushed into the first pit, dies (motion 7)
/// and the area restarts with a life fewer (`$D527`); play goes on.
#[test]
fn standing_in_stage_1_2_loses_a_life_and_restarts() {
    let Some(mut play) = start(1) else {
        return;
    };
    let mut died = false;
    for _ in 0..900 {
        play.step(Buttons::NONE);
        died |= play.session().player_motion() == Motion::Dying;
    }
    let session = play.session();
    assert!(died, "Batman never died");
    assert_eq!((session.lives(), session.area()), (1, 2));
    assert!(!session.in_script());
    assert_ne!(session.player_motion(), Motion::Dying);
}

/// With no lives left the continue screen counts down, GAME OVER follows and
/// the game resets into its boot.
#[test]
fn the_last_life_ends_in_game_over_and_a_reset() {
    let Some(mut play) = start(1) else {
        return;
    };
    assert!(
        step_until(&mut play, 4000, Buttons::NONE, Play::booting),
        "GAME OVER never reset into boot"
    );
    assert!(
        step_until(&mut play, 400, Buttons::NONE, on_title),
        "boot after GAME OVER never reached the title"
    );
}

/// Stage 1-2's gate into the boss room: the card shows, the room loads and Batman enters.
#[test]
fn the_boss_room_starts_and_batman_enters() {
    let Some(mut play) = start(1) else {
        return;
    };
    play.session_mut().arm_boss_entry();
    let mut entered = false;
    for _ in 0..3600 {
        play.step(Buttons::NONE);
        entered |= play.session().player_motion() == Motion::BossEntry;
    }
    assert!(entered, "Batman never ran his entry");
    assert_eq!(play.session().area(), 4);
    assert_ne!(play.session().player_motion(), Motion::BossEntry);
}

/// Beating the stage 1 boss plays the banner cutscene (`$98BD`) with its raster split
/// and loads stage 2-1 (`$0400 = 6`) with its song `$0F`, and play goes on.
#[test]
fn beating_the_stage_1_boss_plays_the_cutscene_and_starts_stage_2() {
    let Some(mut play) = start(1) else {
        return;
    };
    play.session_mut().arm_boss_entry();
    for _ in 0..600 {
        play.step(Buttons::NONE);
    }
    assert_eq!(play.session().area(), 4, "the boss room never loaded");
    play.session_mut().arm_cutscene_next_area(0x0E);
    let mut requests = Vec::new();
    let mut split = false;
    for _ in 0..1200 {
        requests.extend(
            play.step(Buttons::NONE)
                .into_iter()
                .flatten()
                .map(SoundId::byte),
        );
        split |= play.session().raster_chain().is_some();
        if play.session().area() == 6 && !play.session().in_script() {
            break;
        }
    }
    assert_eq!(play.session().area(), 6, "stage 2 never loaded");
    assert!(!play.session().in_script(), "the load never finished");
    assert!(split, "the cutscene's split never ran");
    assert!(requests.contains(&0x0F), "{requests:x?}");
    for _ in 0..120 {
        play.step(Buttons::NONE);
    }
    assert_eq!(play.session().area(), 6);
}

/// Stage 2-1's room changes as the TAS takes them (`$BB` = `$61`, then `$81`): the
/// slide into the two-nametable vertical room (`$DD = $C0`, `$BA46`), the camera
/// following Batman up it (`$852F`, `$83C4`), and the fade into the third room (`$BA8B`).
#[test]
fn stage_2_1_changes_rooms_and_scrolls_up() {
    let Some(mut play) = start(2) else {
        return;
    };
    let change = |play: &mut Play, side: u8| {
        play.session_mut()
            .request_transition(Transition::RoomChange, side);
        play.step(Buttons::NONE);
        for _ in 0..200 {
            if !play.session().in_script() {
                break;
            }
            play.step(Buttons::NONE);
        }
        assert!(!play.session().in_script(), "the room change never ended");
    };
    change(&mut play, 0x61);
    assert_eq!(
        (play.session().room(), play.session().orientation()),
        (2, 0xC0)
    );
    let camera = play.session().camera().1;
    play.session_mut().nudge_player_y(-0x0400);
    let mut lowest = camera;
    for _ in 0..30 {
        play.step(Buttons::NONE);
        lowest = lowest.min(play.session().camera().1);
    }
    assert!(lowest < camera, "the camera never scrolled up");
    change(&mut play, 0x81);
    assert_eq!(
        (play.session().room(), play.session().orientation()),
        (4, 0)
    );
    for _ in 0..60 {
        play.step(Buttons::RIGHT);
    }
    let mut frame = Frame::new();
    play.compose(&mut frame, MAX_SPRITES_PER_LINE, false);
    let backdrop = frame.indices()[0];
    assert!(
        frame.indices().iter().any(|&pixel| pixel != backdrop),
        "the frame is blank"
    );
}

/// The ending, as the stage 7 boss's last figure (type `$B8`) leaves it: `$BA = 6`
/// in stage 7. It runs its song `$0D`, its scenes and credits, and holds its last
/// screen. Sampled every 100 frames, the picture must show something and keep
/// changing, so a blank or stuck ending fails.
#[test]
fn beating_the_final_boss_plays_the_ending() {
    let Some(mut play) = start(12) else {
        return;
    };
    play.session_mut().request_transition(Transition::Ending, 0);
    let mut requests = Vec::new();
    let mut samples = Vec::new();
    let mut frame = Frame::new();
    for tick in 0..7000 {
        requests.extend(
            play.step(Buttons::NONE)
                .into_iter()
                .flatten()
                .map(SoundId::byte),
        );
        if tick % 100 == 50 {
            play.compose(&mut frame, MAX_SPRITES_PER_LINE, false);
            samples.push(frame.indices().to_vec());
        }
    }
    assert!(
        play.session().ending_over(),
        "the ending never reached its last screen"
    );
    for id in [0x0D, 0x22, 0x23] {
        assert!(
            requests.contains(&id),
            "${id:02X} never requested: {requests:x?}"
        );
    }
    let shown: Vec<&Vec<u8>> = samples
        .iter()
        .filter(|pixels| pixels.iter().any(|&pixel| pixel != pixels[0]))
        .collect();
    assert!(shown.len() >= 60, "{} of 70 samples drawn", shown.len());
    let mut distinct = shown.clone();
    distinct.sort();
    distinct.dedup();
    assert!(distinct.len() >= 45, "{} distinct pictures", distinct.len());
    assert!(
        shown.last() == samples.last().as_ref(),
        "the last screen is blank"
    );
}

/// Start pauses the game and freezes it; Start again resumes.
#[test]
fn start_pauses_and_resumes() {
    let Some(mut play) = play() else {
        return;
    };
    for _ in 0..60 {
        play.step(Buttons::RIGHT);
    }
    play.step(Buttons::START);
    let paused_x = play.session().player_x();
    for _ in 0..30 {
        play.step(Buttons::RIGHT);
    }
    assert!(play.paused());
    assert_eq!(play.session().player_x(), paused_x);
    play.step(Buttons::START);
    play.step(Buttons::RIGHT);
    play.step(Buttons::RIGHT);
    assert!(!play.session().in_script());
    for _ in 0..30 {
        play.step(Buttons::RIGHT);
    }
    assert_ne!(play.session().player_x(), paused_x);
}

/// The stage end's departure (phases 2-4, as its walk-off leaves them), then the
/// next area's load (the banner, area 2 and its song `$04`) with stage 1-2's
/// first page in the nametable, and play goes on.
#[test]
fn the_stage_end_loads_stage_1_2() {
    let Some(mut play) = play() else {
        return;
    };
    for _ in 0..120 {
        play.step(Buttons::NONE);
    }
    play.session_mut().arm_stage_end_depart(0xFF, 0x70);
    let mut requests = Vec::new();
    for _ in 0..800 {
        requests.extend(
            play.step(Buttons::NONE)
                .into_iter()
                .flatten()
                .map(SoundId::byte),
        );
        if play.session().area() == 2 && !play.session().in_script() {
            break;
        }
    }
    assert_eq!(play.session().area(), 2, "the next area never loaded");
    assert!(requests.contains(&0x04), "{requests:x?}");
    let fresh = start(1).expect("the ROM");
    assert_eq!(
        play.session().ppu().nametables[0],
        fresh.session().ppu().nametables[0]
    );
    for _ in 0..120 {
        play.step(Buttons::NONE);
    }
    assert_eq!(play.session().area(), 2);
}

/// A jump makes `Play::step` return effect `$42=$24` (`$84FD`/`$8508`,
/// bank 4) on the take-off frame, and the driver plays it as the documented
/// noise sweep (`docs/re/MUSIC.md` `$24`): the frame's writes include the noise
/// channel. If the request were dropped, the driver would write none of them.
#[test]
fn a_jump_requests_the_jump_effect_and_the_driver_plays_it() {
    let Some(mut play) = play() else {
        return;
    };
    let request = (0..300)
        .map(|frame| {
            play.step(if frame % 40 < 12 {
                Buttons::A
            } else {
                Buttons::NONE
            })
        })
        .find(|sound| sound[2] == SoundId::from_byte(rotj_music::JUMP_SFX))
        .expect("the jump requested $42=$24");
    assert_eq!(request[2].map(SoundId::byte), Some(0x24));
    assert_eq!(request[3], None, "the jump is slot $42");
    assert_eq!(
        play.session().player_motion(),
        Motion::Jumping,
        "the sound is the take-off"
    );

    let mut driver = crate::audio::driver();
    driver.request(request[2].expect("the jump's request").byte());
    let writes: Vec<(u16, u8)> = driver.tick().iter().map(|w| (w.addr, w.value)).collect();
    assert!(
        writes.contains(&(rotj_apu::noise::CTRL, 0x16)),
        "noise volume: {writes:04x?}"
    );
    assert!(
        writes.contains(&(rotj_apu::noise::TIMER, 0x0E)),
        "noise period: {writes:04x?}"
    );
    assert!(
        writes.contains(&(rotj_apu::noise::LENGTH, 0x08)),
        "noise length: {writes:04x?}"
    );
    assert!(
        writes.contains(&(rotj_apu::pulse2::CTRL, 0x10)),
        "the silent pulse voice: {writes:04x?}"
    );
}

/// Holding fire fires the default weapon: `Play::step` returns its effect
/// `$42=$2D` (bank 4 `$9D2D`: `LDA #$2D / STA $42`).
#[test]
fn firing_requests_the_weapon_effect() {
    let Some(mut play) = play() else {
        return;
    };
    let request = (0..600)
        .map(|frame| {
            play.step(if frame % 8 < 2 {
                Buttons::B
            } else {
                Buttons::NONE
            })
        })
        .find(|sound| sound[2] == SoundId::from_byte(0x2D))
        .expect("firing requested $42=$2D");
    assert_eq!(request[2].map(SoundId::byte), Some(0x2D));
    assert_eq!(request[3], None, "the fire effect is slot $42");
}

/// A damage-class tile under Batman makes `Play::step` return the damage
/// effect `$42=$25` (`$82C6`/`$B17F`) and costs energy. Every solid metatile
/// becomes class 3 (`$7340` holds `2 * class`, as in the core's own damage
/// test), so the ground he lands on hurts.
#[test]
fn taking_damage_requests_the_damage_effect() {
    let Some(mut play) = play() else {
        return;
    };
    play.session_mut().make_non_solid_tiles_hazard();
    let energy = play.session().player_energy();
    let request = (0..120)
        .map(|_| play.step(Buttons::NONE))
        .find(|sound| sound[2] == SoundId::from_byte(0x25))
        .expect("the damage requested $42=$25");
    assert_eq!(request[2].map(SoundId::byte), Some(0x25));
    assert_eq!(request[3], None, "the damage effect is slot $42");
    assert!(
        play.session().player_energy() < energy,
        "the hit cost energy ({} -> {})",
        energy,
        play.session().player_energy()
    );
}

/// A fresh directory under the system temp dir for one test.
fn temp_dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rotj-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn the_recorder_writes_each_pad() {
    let dir = temp_dir("recorder-writes");
    let mut recorder = Recorder::open(&dir, 0).expect("recording");
    for pad in [0x81, 0x00, 0x42] {
        recorder.record(Buttons::from_bits(pad));
    }
    drop(recorder);
    let files: Vec<_> = std::fs::read_dir(&dir).expect("dir").flatten().collect();
    assert_eq!(files.len(), 1);
    let name = files[0].file_name().into_string().expect("utf-8");
    assert!(name.ends_with("-level0.pads"), "{name}");
    assert_eq!(
        std::fs::read(files[0].path()).expect("read"),
        [0x81, 0x00, 0x42]
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn a_second_recorder_does_not_clobber() {
    let dir = temp_dir("recorder-clobber");
    let mut first = Recorder::open_at(&dir, 1234, 5).expect("first");
    let mut second = Recorder::open_at(&dir, 1234, 5).expect("second");
    assert!(
        Recorder::open_at(&dir, 1234, 5).is_none(),
        "a third gives up"
    );
    first.record(Buttons::from_bits(1));
    second.record(Buttons::from_bits(2));
    drop((first, second));
    assert_eq!(
        std::fs::read(dir.join("1234-level5.pads")).expect("first"),
        [1]
    );
    assert_eq!(
        std::fs::read(dir.join("1234-2-level5.pads")).expect("second"),
        [2]
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn a_missing_directory_records_nothing() {
    let dir = temp_dir("recorder-missing").join("absent");
    assert!(Recorder::open(&dir, 0).is_none());
}

fn line(l: &rotj_oracle::Line) -> Line {
    Line {
        v: l.v,
        fine_x: l.fine_x,
        ctrl: l.ctrl,
        mask: l.mask,
        chr: l.chr,
        sprite_chr: l.sprite_chr,
        mirroring: Mirroring::from_register(l.mirroring),
    }
}

/// From power-on with no input (the intro, the title, the attract mode's demo and
/// its pages), the app's every frame is the oracle's.
#[test]
#[ignore = "3000 frames of the oracle (release, 5 s); the gates run it in release with --ignored"]
fn the_app_draws_like_the_oracle_from_power_on() {
    let (Some(mut play), Some(mut m)) = (powered_on(None), rotj_oracle::power_on()) else {
        return;
    };
    m.bus.trace.peeks = vec![0xC232, 0xC23F, 0xE1D0];
    let mut waiting = false;
    let mut lagged = |m: &mut rotj_oracle::Machine| {
        let mut lag = false;
        for peek in m.bus.trace.peeked.drain(..) {
            match peek.pc {
                0xC232 => waiting = true,
                0xC23F => waiting = false,
                _ => lag |= !waiting,
            }
        }
        lag
    };
    let (mut drawn, mut jittered, mut unmatched, mut skipped) = (0, 0, 0, 0);
    let (mut ours, mut theirs) = (Frame::new(), Frame::new());
    for tick in 0..3000 {
        play.step(Buttons::NONE);
        let mut log = m.run_frame(0);
        while lagged(&mut m) || log.nmis == 0 {
            skipped += 1;
            log = m.run_frame(0);
        }
        if log.lines.is_empty() {
            continue;
        }
        let mut lines: Vec<Line> = log.lines.iter().map(line).collect();
        let mut compared = [true; 240];
        for (y, mine) in play.session().lines().iter().enumerate() {
            let states = std::iter::once(lines[y]).chain(
                log.splits
                    .iter()
                    .filter(|s| s.0 == y)
                    .flat_map(|s| [line(&s.1), line(&s.2)]),
            );
            let blank = |l: &Line| l.mask & mask::RENDERING == 0;
            match states
                .into_iter()
                .find(|s| s == mine || blank(s) && blank(mine))
            {
                Some(state) => lines[y] = state,
                None => compared[y] = false,
            }
        }
        jittered += usize::from(compared.contains(&false));
        unmatched += compared.iter().filter(|&&c| !c).count();
        if lines.iter().all(|l| l.mask & mask::RENDERING == 0) {
            continue;
        }
        play.compose(&mut ours, MAX_SPRITES_PER_LINE, false);
        theirs.render(
            &Picture {
                lines: &lines,
                vram: &m.bus.ppu.vram,
                palette: &m.bus.ppu.palette,
                oam: &m.bus.ppu.oam,
                chr: &m.bus.cart.chr,
            },
            MAX_SPRITES_PER_LINE,
        );
        for y in (0..240).filter(|&y| compared[y]) {
            let row = y * SCREEN_W..(y + 1) * SCREEN_W;
            assert_eq!(
                ours.indices()[row.clone()],
                theirs.indices()[row],
                "tick {tick} (oracle frame {}): line {y}",
                m.frame
            );
        }
        drawn += 1;
    }
    assert_eq!((drawn, jittered, unmatched, skipped), (2937, 42, 319, 15));
}
