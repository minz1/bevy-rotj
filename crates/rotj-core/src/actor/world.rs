//! What actors read and write outside their own slot.

use super::button::Buttons;
use super::oam::{OamBuilder, attr};
use super::scroll::Scroll;
use super::slots::Slots;

/// The live spawn schedule (`$7410`) and its page -> row table (`$0440`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// Cells: bit 7 pending, bits 7-6 = `01` a scroll lock, bits 0-5 a record id.
    pub cells: [u8; 256],
    /// Each page's base column in the schedule and the map (`$0440-$045F`).
    pub page_columns: [u8; 32],
}

impl Default for Schedule {
    fn default() -> Schedule {
        Schedule {
            cells: [0; 256],
            page_columns: [0; 32],
        }
    }
}

/// The visible area's size from the camera, in 1/16 px (`256 * 16` wide on the NES).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct View {
    /// Width.
    pub width: i32,
    /// Height.
    pub height: i32,
}

pub use crate::collision::Level;

/// The selected weapon (`$0476`), which picks the projectiles' handler (bank 5 `$801B`). The
/// manual's names were never matched to RAM; each is named for what it does.
#[repr(u8)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Weapon {
    /// The default single shot (likely the Cross Bow).
    #[default]
    Bolt = 0,
    /// Turns toward a target (the manual's homing Batarang).
    Homing = 1,
    /// Flies on, pushing its speed toward a cruise, and splits at a wall or when spent.
    Splitter = 2,
    /// Two parts that weave as they fly.
    Wave = 3,
    /// Three parts at once (likely the Shield Star).
    ThreeWay = 4,
    /// The power-up's weapon.
    Super = 5,
    /// Not a weapon: the power-up's closing pieces, flown by the same loop.
    PowerUpPieces = 6,
}

impl Weapon {
    const ALL: [Weapon; 7] = [
        Weapon::Bolt,
        Weapon::Homing,
        Weapon::Splitter,
        Weapon::Wave,
        Weapon::ThreeWay,
        Weapon::Super,
        Weapon::PowerUpPieces,
    ];

    /// The RAM value, and the index into the game's per-weapon tables.
    #[must_use]
    pub const fn byte(self) -> u8 {
        self as u8
    }

    /// The weapon a RAM value holds, if it is one.
    #[must_use]
    pub fn from_byte(byte: u8) -> Option<Weapon> {
        Weapon::ALL.get(usize::from(byte)).copied()
    }
}

/// What the player is doing (`$046E`), which picks his handler (bank 4 `$96C3`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Motion {
    /// On the ground, still.
    #[default]
    Standing = 0,
    /// Pad down.
    Crouching = 1,
    /// On the ground with a direction held.
    Walking = 2,
    /// Airborne with x control.
    Jumping = 3,
    /// Down+A from a crouch: the slide, which ends in a crouch or turns into a jump.
    Sliding = 4,
    /// Off a ledge or dropped.
    Falling = 5,
    /// Pad up: the weapons aim up.
    AimingUp = 6,
    /// The death, and the freeze the level's end (`$D274`) sets when a boss is beaten.
    Dying = 7,
    /// Held through the power-up, hidden on odd frames.
    PoweringUp = 8,
    /// Lowered into the boss room while the tally counts.
    BossEntry = 9,
    /// Held by a script (the boss beaten, room `$DA0F`): no handler runs.
    Held = 10,
    /// Steered through the air by the pad (actor `$54`, room `$D9FF`), only shooting.
    Flying = 11,
}

impl Motion {
    const ALL: [Motion; 12] = [
        Motion::Standing,
        Motion::Crouching,
        Motion::Walking,
        Motion::Jumping,
        Motion::Sliding,
        Motion::Falling,
        Motion::AimingUp,
        Motion::Dying,
        Motion::PoweringUp,
        Motion::BossEntry,
        Motion::Held,
        Motion::Flying,
    ];

    /// The RAM value, and the index into the game's 12-entry per-motion tables.
    #[must_use]
    pub const fn byte(self) -> u8 {
        self as u8
    }

    /// The motion a RAM value holds, if it is one.
    #[must_use]
    pub fn from_byte(byte: u8) -> Option<Motion> {
        Motion::ALL.get(usize::from(byte)).copied()
    }

    /// Standing, crouching or walking: on his feet and not aiming.
    #[must_use]
    pub const fn on_foot(self) -> bool {
        matches!(self, Motion::Standing | Motion::Crouching | Motion::Walking)
    }
}

/// Sets every 4-colour set's first entry (the colour pixel 0 mirrors) of a palette shadow.
pub fn set_first_colours(palette: &mut [u8; 32], colour: u8) {
    for first in palette.iter_mut().step_by(4) {
        *first = colour;
    }
}

/// The room's orientation byte (`$DD`, room record byte 5).
pub mod orientation {
    /// The room is two pages wide or tall.
    pub const TWO_PAGES: u8 = 0x80;
    /// The room scrolls vertically.
    pub const VERTICAL: u8 = 0x40;

    /// Two pages across, scrolling sideways (`$DD` bits 7-6 = `10`).
    #[must_use]
    pub const fn horizontal_two_page(orientation: u8) -> bool {
        orientation & (TWO_PAGES | VERTICAL) == TWO_PAGES
    }

    /// Two pages tall, scrolling vertically (`$DD` bits 7-6 = `11`).
    #[must_use]
    pub const fn vertical_two_page(orientation: u8) -> bool {
        orientation & (TWO_PAGES | VERTICAL) == TWO_PAGES | VERTICAL
    }
}

/// The bits of the player's pose (`$047F`).
pub mod pose {
    /// A turn is pending.
    pub const TURNING: u8 = 0x80;
    /// The shot pose.
    pub const SHOT: u8 = 0x40;
    /// The next frame resumes the motion saved in [`SAVED_MOTION`].
    pub const RESUME: u8 = 0x20;
    /// The landing animation plays (set on touching down).
    pub const LANDING: u8 = 0x10;
    /// The saved motion's byte, with [`RESUME`].
    pub const SAVED_MOTION: u8 = 0x0F;
    /// With [`LANDING`] and without [`RESUME`]: the long landing (after a jump, bank 4
    /// `$96E0`) rather than the short one (`$96DD`); it shares the saved motion's bits.
    pub const LONG_LANDING: u8 = 0x01;
}

/// The bits of the stage-script byte (`$0481`).
pub mod stage_script {
    /// The player's movement is frozen.
    pub const FROZEN: u8 = 0x80;
    /// The projection is skipped: the player is not drawn.
    pub const HIDDEN: u8 = 0x40;
    /// With [`FROZEN`]: the pads are kept rather than cleared.
    pub const KEEP_PADS: u8 = 0x20;
}

/// The bits of the busy byte (`$AF`).
pub mod busy {
    /// A scripted sequence holds the game; the pause tests it.
    pub const SCRIPTED: u8 = 0x80;
}

/// The bits of the attack byte (`$B8`).
pub mod attack {
    /// The weapon charges (every weapon but the bolt).
    pub const CHARGEABLE: u8 = 0x80;
    /// A charge is building (B held since the press).
    pub const CHARGING: u8 = 0x40;
    /// The charged attack is out.
    pub const CHARGED: u8 = 0x20;
}

/// The bits of the power-up byte (`$0472`).
pub mod power_up {
    /// The power-up is on: invulnerable, the meter drains, weapon pickups are ignored.
    pub const ON: u8 = 0x80;
}

/// The bits of the slide byte (`$0473`).
pub mod sliding {
    /// The player is sliding.
    pub const ON: u8 = 0x80;
}

/// The bits of the hurt byte (`$047A`).
pub mod hurt {
    /// The hurt lockout: controls locked while the timers run.
    pub const STUNNED: u8 = 0x80;
    /// The invulnerability after the hurt (what the contacts and the damage tile test).
    pub const INVULNERABLE: u8 = 0x40;
}

/// The player as actors see him.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Player {
    /// Level x, 1/16 px (`$0460:$0461`).
    pub x: u16,
    /// Level y, 1/16 px (`$0462:$0463`).
    pub y: u16,
    /// Screen x (`$B0`).
    pub screen_x: u8,
    /// Screen y (`$B1`).
    pub screen_y: u8,
    /// Energy (`$0478`).
    pub energy: u8,
    /// The power-up byte (`$0472`): bit 7 the power-up (the Super weapon) is on.
    pub power_up: u8,
    /// The slide byte (`$0473`): bit 7 sliding; the contacts read it as "the player hits actors".
    pub sliding: u8,
    /// The hurt byte (`$047A`): bit 7 the hurt lockout, bit 6 the invulnerability after it.
    pub hurt: u8,
    /// Contact reach below him (`$047B`).
    pub reach_y: u8,
    /// X velocity (`$046C:$046D`).
    pub vx: i16,
    /// What he is doing (`$046E`).
    pub motion: Motion,
    /// Batman's sprite attribute byte (`$046F`): bit 6 faces left; the palette bits beside
    /// it, which his palette changes set.
    pub facing: u8,
    /// Frames of hurt invulnerability (`$0479`).
    pub hurt_timer: u8,
    /// Player pose/action flags (`$047F`): bit 7 a turn is pending, bit 6 the shot
    /// pose, bit 5 resume a saved motion, bits 0-3 the saved motion.
    pub pose: u8,
    /// Y velocity, 1/16 px a frame (`$0469:$046A`).
    pub vy: i16,
    /// The stage-script byte (`$0481`): bit 7 freezes the player and clears the pads
    /// unless bit 5 keeps them, bit 6 skips the projection (docs/ACTORS.md §7a).
    pub stage_script: u8,
    /// The death counter (`$0480`), counting motion 7's flash and burst; cleared by
    /// the death (`$D274`).
    pub death_timer: u8,
    /// Walk animation state (`$0466`).
    pub anim: u8,
    /// The platform's carry x velocity, 1/16 px (`$0467:$0468`).
    pub carrier: i16,
    /// Masks the pad's walk bits (`$0470`).
    pub walk_gate: Buttons,
    /// The walk ramp timer (`$047C`).
    pub walk_timer: u8,
    /// The per-pose step (`$047D`); bits 7-6 are the air state's landing flags.
    pub pose_step: u8,
    /// The per-pose frame counter (`$047E`), cleared with the air state.
    pub pose_timer: u8,
    /// The movement's damage flag (`$A8`), zeroed each frame and set from the probe's
    /// `$9F` bit 7.
    pub hit: u8,
    /// `$B9`: 0x80 set by the movement's tail, cleared by the draw; the `$CAA9` and
    /// `$CC6D` class handlers skip when bit 7 is set.
    pub moved: u8,
    /// The draw's hit-flash counter (`$0464`).
    pub flash_timer: u8,
    /// The death/room-change flags (`$0465`): bit 0 the burst pose, bit 7 hides the draw.
    pub death_flags: u8,
    /// The attack (shoot) frame counter (`$046B`).
    pub attack_timer: u8,
    /// The B-held charge counter (`$B7`); at `$30` the charge is ready.
    pub charge_timer: u8,
    /// The boss room entry's step (`$0488`).
    pub entry_step: u8,
    /// The entry step's frame counter (`$0489`).
    pub entry_timer: u8,
    /// The tally the entry counts up to `$B2:$B3` (`$048A:$048B`, little-endian).
    pub tally: u16,
    /// The tally's cycling last digits (`$048F`).
    pub tally_digit: u8,
    /// The sprite bank `$E4` the entry keeps (`$0487`).
    pub entry_chr: u8,
}

impl Player {
    /// Is he facing left (`$046F` bit 6)?
    #[must_use]
    pub fn faces_left(&self) -> bool {
        self.facing & attr::FLIP_X != 0
    }
}

/// Everything a handler reads and does not own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct World {
    /// Camera x, 1/16 px (`$CC:$CD`).
    pub camera_x: u16,
    /// Camera y, 1/16 px (`$CE:$CF`).
    pub camera_y: u16,
    /// The visible area.
    pub view: View,
    /// Scroll orientation flags (`$DD`).
    pub orientation: u8,
    /// This frame's random bytes (`$F9`, `$FA`).
    pub rng: [u8; 2],
    /// The odd-frame flag (`$F7` bit 7).
    pub frame_odd: bool,
    /// The NMI frame counter (`$F1`).
    pub frame: u8,
    /// The area (`$0400`).
    pub area: u8,
    /// The room within the area (`$0402`).
    pub room: u8,
    /// Joypad 1 held buttons (`$F6`): bit 0 right, 1 left, 2 down, 3 up.
    pub pad: Buttons,
    /// Joypad 1 buttons newly pressed this frame, the edges of [`World::pad`] (`$F5`).
    pub pressed: Buttons,
    /// The actor spawn distance from the camera (`$D7`), set outside the actors;
    /// type `$12` reads it.
    pub spawn_distance: u8,
}

/// The game state handlers read and change outside the actor slots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effects {
    /// Sound requests (`$42`, `$43`).
    pub sound: [u8; 2],
    /// Music control (`$40`): `$FF` stops, `$FC` fades.
    pub music: u8,
    /// Song request (`$41`).
    pub song: u8,
    /// The player, as earlier handlers this frame left him.
    pub player: Player,
    /// The selected weapon (`$0476`).
    pub weapon: Weapon,
    /// Lives left after this one (`$0482`).
    pub lives: u8,
    /// Second meter (`$0477`).
    pub meter: u8,
    /// The weapon kept across the level end and the power-up (`$0483`).
    pub stored_weapon: Weapon,
    /// Screen transition selector (`$BA`).
    pub transition: u8,
    /// The transition's parameter (`$BB`).
    pub transition_arg: u8,
    /// `$AF` bit 7: a scripted sequence (death, boss entry, level end, super attack)
    /// holds the game; it blocks the pause.
    pub busy: u8,
    /// The player's attack flags (`$B8`): bit 7 the weapon charges, bit 6 a charge is
    /// building, bit 5 the charged attack is out.
    pub attack: u8,
    /// The boss's hit points (`$A9:$AA`), set by bank 13 `$BE12`.
    pub boss_hp: u16,
    /// The boss's hit pool (`$B2:$B3`), drained by the boss contact and counted up
    /// beside the hit points by the entry.
    pub boss_pool: [u8; 2],
    /// The scroll-lock flags (`$C2`): bits lock the camera's left/right/up/down steps,
    /// `$FF` = none (bank 13 `$B927`).
    pub scroll_lock: u8,
    /// Bit 7 hides the HUD (`$0404`).
    pub hud: u8,
    /// `$0475`, written 0 by type `$50`; nothing in the port reads it.
    pub misc_flag: u8,
    /// The pending banner cutscene (`$0406`), set by type `$54` (`$A0`) or `$50` (`$0E`).
    pub cutscene_id: u8,
    /// The vertical scroll (`$D5`): the raster split's top band, and type `$58` reads
    /// it as its probe offset.
    pub scroll_y: u8,
    /// The stage-end script (`$0408-$040C`); type `$0E` clears its phase.
    pub stage_end: super::stage::StageEnd,
    /// The stage arguments (`$040E`, `$040F`): a sprite id and the boss id (also the
    /// player motion kept across the power-up).
    pub stage_args: [u8; 2],
    /// The palette the NMI uploads (`$04E0-$04FF`).
    pub palette: [u8; 32],
    /// `$B5`, bit 7 set when a handler changes the palette, cleared once the palette is queued.
    pub palette_dirty: u8,
    /// Which of the four CHR shadows animate, by bit (`$0401`); type `$84` clears it.
    pub animated: u8,
    /// The player's projectiles (the `$7BD0` slots).
    pub projectiles: [Projectile; 16],
    /// The area name's metasprite the pause builds (`$7FD0-$7FE1`).
    pub pause_name: [u8; 18],
    /// `$7FF1`, zeroed with the pause's name.
    pub pause_aux: u8,
    /// The level's terrain, which collision reads and level edits write.
    pub level: Level,
    /// The nametable updates queued for the NMI.
    pub vram: Vram,
}

/// The nametable update queue (`$0100-$0180`, filled downward) and the edits' state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vram {
    /// The queue pointer (`$0181`): the next free byte, `$7F` when empty.
    pub top: u8,
    /// Updates queued since the queue was last drained, oldest first.
    pub writes: Vec<VramWrite>,
    /// Both nametables' attribute tables as last queued (`$7530-$75AF`).
    pub attributes: [u8; 128],
    /// The nametable address at the camera's top row on vertical levels (`$CA:$CB`).
    pub origin: u16,
    /// Writes made straight to the PPU outside the queue, with rendering off.
    pub direct: Vec<VramWrite>,
}

/// One queued run of PPU writes (`$C0C6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VramWrite {
    /// PPU address.
    pub addr: u16,
    /// The entry's flags byte (`$32`).
    pub flags: u8,
    /// The bytes written from `addr` on.
    pub bytes: Vec<u8>,
}

/// The PPU's nametable RAM (two physical tables) and palette RAM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ppu {
    /// `[0]` is physical table A, `[1]` table B.
    pub nametables: [[u8; 1024]; 2],
    /// Palette RAM, `$3F00-$3F1F`.
    pub palette: [u8; 32],
}

impl Ppu {
    /// A blank PPU.
    #[must_use]
    pub fn new() -> Ppu {
        Ppu {
            nametables: [[0; 1024]; 2],
            palette: [0x0F; 32],
        }
    }

    /// Applies queued writes the way the NMI flushes them (`$C084`): flags bit 2
    /// steps the address by 32, and `mirroring` (`$EC`, the FME-7 command `$C`)
    /// maps the four logical tables onto the two physical ones.
    pub fn apply(&mut self, writes: &[VramWrite], mirroring: u8) {
        for write in writes {
            let step = if write.flags & 4 != 0 { 32 } else { 1 };
            let mut addr = write.addr;
            for &byte in &write.bytes {
                self.poke(addr, byte, mirroring);
                addr = addr.wrapping_add(step);
            }
        }
    }

    fn poke(&mut self, addr: u16, byte: u8, mirroring: u8) {
        let addr = addr & 0x3FFF;
        if addr >= PALETTE_ADDRESS {
            let mut at = usize::from(addr & 0x1F);
            if at & 0x13 == 0x10 {
                at &= 0x0F;
            }
            self.palette[at] = byte;
        } else if addr >= 0x2000 {
            let table = usize::from((addr >> 10) & 3);
            let physical = match mirroring & 3 {
                0 => table & 1,
                1 => table >> 1,
                2 => 0,
                _ => 1,
            };
            self.nametables[physical][usize::from(addr & 0x3FF)] = byte;
        }
    }
}

impl Default for Ppu {
    fn default() -> Self {
        Ppu::new()
    }
}

impl Default for Vram {
    fn default() -> Self {
        Vram {
            top: 0x7F,
            writes: Vec::new(),
            attributes: [0; 128],
            origin: 0,
            direct: Vec::new(),
        }
    }
}

/// The PPU address of the palette.
const PALETTE_ADDRESS: u16 = 0x3F00;

impl Vram {
    /// Queues `bytes` for the palette (`$3F00`).
    pub fn push_palette(&mut self, bytes: &[u8]) {
        self.push(PALETTE_ADDRESS, 0, bytes);
    }

    /// Queues `bytes` for PPU `addr` (`$C0C6`, data from memory).
    pub fn push(&mut self, addr: u16, flags: u8, bytes: &[u8]) {
        let len = u8::try_from(bytes.len()).expect("a queue entry fits a byte count");
        self.top = self.top.wrapping_sub(len).wrapping_sub(4);
        self.writes.push(VramWrite {
            addr,
            flags,
            bytes: bytes.to_vec(),
        });
    }
}

/// One of the player's projectiles, as the actor contact scan sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Projectile {
    /// Type, 0 when free (`$7BD0`).
    pub kind: u8,
    /// Screen x (`$7B80`).
    pub screen_x: u8,
    /// Screen y (`$7B90`).
    pub screen_y: u8,
    /// Flags (`$7F30`): bit 7 spent, bit 5 deflected.
    pub flags: u8,
    /// Damage it deals (`$7F20`).
    pub damage: u8,
    /// X velocity (`$7D00:$7D10`).
    pub vx: i16,
    /// Y velocity (`$7D20:$7EB0`).
    pub vy: i16,
    /// A per-slot state byte (`$7C80`); bit 7 marks the shot spent.
    pub state: u8,
    /// A per-slot countdown (`$7CB0`): the wall-bounce timer and a spent shot's or
    /// explosion's frames.
    pub timer: u8,
    /// The draw's attribute (`$7C20`), read into `Work::attr`.
    pub attr: u8,
    /// A per-slot parameter (`$7F40`): the homing target slot, a bounce direction or
    /// an orbit index, by weapon.
    pub param: u8,
    /// A charged attack's part or spread index (`$7F50`).
    pub part: u8,
    /// A charged attack's frame counter (`$7F60`).
    pub part_timer: u8,
    /// The projectile's level-x high byte (`$7BB0`, also slots 16-23's `$7BA0`).
    pub level_x_hi: u8,
    /// `$7BC0`, the projectile's level-x low byte.
    pub level_x_lo: u8,
    /// `$7C60`, the projectile's level-y high byte.
    pub level_y_hi: u8,
    /// `$7C70`, the projectile's level-y low byte.
    pub level_y_lo: u8,
}

/// The projectile arrays, 16 wide each.
pub const PROJECTILE_ARRAYS: [u16; 19] = [
    0x7BD0, 0x7B80, 0x7B90, 0x7F30, 0x7F20, 0x7D00, 0x7D10, 0x7D20, 0x7EB0, 0x7C80, 0x7CB0, 0x7C20,
    0x7F40, 0x7F50, 0x7F60, 0x7BB0, 0x7BC0, 0x7C60, 0x7C70,
];

impl Projectile {
    /// The projectile's byte in `array` (one of [`PROJECTILE_ARRAYS`]).
    #[must_use]
    pub fn byte(&self, array: u16) -> u8 {
        let across = self.vx.to_be_bytes();
        let down = self.vy.to_be_bytes();
        match array {
            0x7BD0 => self.kind,
            0x7B80 => self.screen_x,
            0x7B90 => self.screen_y,
            0x7F30 => self.flags,
            0x7F20 => self.damage,
            0x7D00 => across[0],
            0x7D10 => across[1],
            0x7D20 => down[0],
            0x7EB0 => down[1],
            0x7C80 => self.state,
            0x7CB0 => self.timer,
            0x7C20 => self.attr,
            0x7F40 => self.param,
            0x7F50 => self.part,
            0x7F60 => self.part_timer,
            0x7BB0 => self.level_x_hi,
            0x7BC0 => self.level_x_lo,
            0x7C60 => self.level_y_hi,
            0x7C70 => self.level_y_lo,
            other => panic!("${other:04X} is not a weapon array"),
        }
    }

    /// Writes the weapon's byte in `array`.
    pub fn set_byte(&mut self, array: u16, value: u8) {
        let across = self.vx.to_be_bytes();
        let down = self.vy.to_be_bytes();
        match array {
            0x7BD0 => self.kind = value,
            0x7B80 => self.screen_x = value,
            0x7B90 => self.screen_y = value,
            0x7F30 => self.flags = value,
            0x7F20 => self.damage = value,
            0x7D00 => self.vx = i16::from_be_bytes([value, across[1]]),
            0x7D10 => self.vx = i16::from_be_bytes([across[0], value]),
            0x7D20 => self.vy = i16::from_be_bytes([value, down[1]]),
            0x7EB0 => self.vy = i16::from_be_bytes([down[0], value]),
            0x7C80 => self.state = value,
            0x7CB0 => self.timer = value,
            0x7C20 => self.attr = value,
            0x7F40 => self.param = value,
            0x7F50 => self.part = value,
            0x7F60 => self.part_timer = value,
            0x7BB0 => self.level_x_hi = value,
            0x7BC0 => self.level_x_lo = value,
            0x7C60 => self.level_y_hi = value,
            0x7C70 => self.level_y_lo = value,
            other => panic!("${other:04X} is not a weapon array"),
        }
    }
}

/// The game's random bytes (`$F9/$FA`), advanced once per frame by the app.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rng(pub [u8; 2]);

impl Rng {
    /// One step of the game's generator (bank 14 `$C21C`).
    pub fn next_frame(&mut self) {
        let [hi, lo] = self.0;
        let feedback = (hi ^ lo) & 2 != 0;
        self.0 = [
            (u8::from(feedback) << 7) | (hi >> 1),
            ((hi & 1) << 7) | (lo >> 1),
        ];
    }
}

/// What a handler may touch besides its own slot.
pub struct Cx<'a> {
    /// The PRG image and its tables.
    pub prg: &'a crate::prg::Prg,
    /// Read-only state.
    pub world: &'a World,
    /// The other slots; the running actor's own slot holds only its `kind` meanwhile.
    pub slots: &'a mut Slots,
    /// The live schedule (re-armed on despawn).
    pub schedule: &'a mut Schedule,
    /// Effects outside the slots.
    pub effects: &'a mut Effects,
    /// The camera's scroll state; a handler that shakes the screen writes `scroll_x` (`$FD`).
    pub scroll: &'a mut Scroll,
    /// The OAM page being built.
    pub oam: &'a mut OamBuilder,
}

impl Cx<'_> {
    /// The byte at a slot-array address, wherever the game's layout puts it.
    #[must_use]
    pub fn byte(&self, addr: u16) -> u8 {
        super::slots::read(self.slots, &self.effects.projectiles, addr)
    }

    /// Writes the byte at a slot-array address.
    pub fn set_byte(&mut self, addr: u16, value: u8) {
        super::slots::write(self.slots, &mut self.effects.projectiles, addr, value);
    }

    /// Field `field` of slot `slot` (0-31), wherever the game's layout puts it.
    #[must_use]
    pub fn get(&self, slot: usize, field: super::slots::Field) -> u8 {
        super::slots::get(self.slots, &self.effects.projectiles, slot, field)
    }

    /// Writes field `field` of slot `slot` (0-31).
    pub fn set(&mut self, slot: usize, field: super::slots::Field, value: u8) {
        super::slots::set(
            self.slots,
            &mut self.effects.projectiles,
            slot,
            field,
            value,
        );
    }
}

#[cfg(test)]
mod layout {
    use super::{
        Effects, Level, Motion, PROJECTILE_ARRAYS, Player, Projectile, Schedule, View, Vram,
        VramWrite, Weapon, World,
    };
    use crate::actor::button::Buttons;

    fn word(ram: &[u8; 2048], hi: usize) -> u16 {
        u16::from_be_bytes([ram[hi], ram[hi + 1]])
    }

    impl Player {
        pub(crate) fn lift(ram: &[u8; 2048]) -> Player {
            Player {
                x: word(ram, 0x460),
                y: word(ram, 0x462),
                screen_x: ram[0xB0],
                screen_y: ram[0xB1],
                energy: ram[0x478],
                power_up: ram[0x472],
                sliding: ram[0x473],
                hurt: ram[0x47A],
                reach_y: ram[0x47B],
                vx: word(ram, 0x46C).cast_signed(),
                motion: Motion::from_byte(ram[0x46E]).expect("a motion 0-11 at $046E"),
                facing: ram[0x46F],
                hurt_timer: ram[0x479],
                pose: ram[0x47F],
                vy: word(ram, 0x469).cast_signed(),
                stage_script: ram[0x481],
                death_timer: ram[0x480],
                anim: ram[0x466],
                carrier: word(ram, 0x467).cast_signed(),
                walk_gate: Buttons::from_bits(ram[0x470]),
                walk_timer: ram[0x47C],
                pose_step: ram[0x47D],
                pose_timer: ram[0x47E],
                hit: ram[0xA8],
                moved: ram[0xB9],
                flash_timer: ram[0x464],
                death_flags: ram[0x465],
                attack_timer: ram[0x46B],
                charge_timer: ram[0xB7],
                entry_step: ram[0x488],
                entry_timer: ram[0x489],
                tally: u16::from_le_bytes([ram[0x48A], ram[0x48B]]),
                tally_digit: ram[0x48F],
                entry_chr: ram[0x487],
            }
        }

        pub(crate) fn project(&self, ram: &mut [u8; 2048]) {
            ram[0x460..0x462].copy_from_slice(&self.x.to_be_bytes());
            ram[0x462..0x464].copy_from_slice(&self.y.to_be_bytes());
            ram[0xB0] = self.screen_x;
            ram[0xB1] = self.screen_y;
            ram[0x478] = self.energy;
            ram[0x472] = self.power_up;
            ram[0x473] = self.sliding;
            ram[0x47A] = self.hurt;
            ram[0x47B] = self.reach_y;
            ram[0x46C..0x46E].copy_from_slice(&self.vx.to_be_bytes());
            ram[0x46E] = self.motion.byte();
            ram[0x46F] = self.facing;
            ram[0x479] = self.hurt_timer;
            ram[0x47F] = self.pose;
            ram[0x469..0x46B].copy_from_slice(&self.vy.to_be_bytes());
            ram[0x481] = self.stage_script;
            ram[0x480] = self.death_timer;
            ram[0x466] = self.anim;
            ram[0x467..0x469].copy_from_slice(&self.carrier.to_be_bytes());
            ram[0x470] = self.walk_gate.bits();
            ram[0x47C] = self.walk_timer;
            ram[0x47D] = self.pose_step;
            ram[0x47E] = self.pose_timer;
            ram[0xA8] = self.hit;
            ram[0xB9] = self.moved;
            ram[0x464] = self.flash_timer;
            ram[0x465] = self.death_flags;
            ram[0x46B] = self.attack_timer;
            ram[0xB7] = self.charge_timer;
            ram[0x488] = self.entry_step;
            ram[0x489] = self.entry_timer;
            ram[0x48A..0x48C].copy_from_slice(&self.tally.to_le_bytes());
            ram[0x48F] = self.tally_digit;
            ram[0x487] = self.entry_chr;
        }
    }

    impl World {
        /// A world with the camera at `camera_x` and everything else zero.
        pub(crate) fn at(camera_x: u16) -> World {
            World {
                camera_x,
                camera_y: 0,
                view: View {
                    width: 256 * 16,
                    height: 240 * 16,
                },
                orientation: 0,
                rng: [0; 2],
                frame_odd: false,
                frame: 0,
                area: 0,
                room: 0,
                pad: Buttons::NONE,
                pressed: Buttons::NONE,
                spawn_distance: 0,
            }
        }

        pub(crate) fn project(&self, ram: &mut [u8; 2048]) {
            ram[0xCC..0xCE].copy_from_slice(&self.camera_x.to_be_bytes());
            ram[0xCE..0xD0].copy_from_slice(&self.camera_y.to_be_bytes());
            ram[0xDD] = self.orientation;
            ram[0x400] = self.area;
            ram[0x402] = self.room;
            [ram[0xF9], ram[0xFA]] = self.rng;
            ram[0xF7] = (ram[0xF7] & 0x7F) | (u8::from(self.frame_odd) << 7);
            ram[0xF1] = self.frame;
            ram[0xF6] = self.pad.bits();
            ram[0xF5] = self.pressed.bits();
            ram[0xD7] = self.spawn_distance;
        }

        pub(crate) fn lift(ram: &[u8; 2048]) -> World {
            let camera_x = word(ram, 0xCC);
            let camera_y = word(ram, 0xCE);
            World {
                camera_x,
                camera_y,
                view: View {
                    width: 256 * 16,
                    height: 240 * 16,
                },
                orientation: ram[0xDD],
                rng: [ram[0xF9], ram[0xFA]],
                frame_odd: ram[0xF7] & 0x80 != 0,
                frame: ram[0xF1],
                area: ram[0x400],
                room: ram[0x402],
                pad: Buttons::from_bits(ram[0xF6]),
                pressed: Buttons::from_bits(ram[0xF5]),
                spawn_distance: ram[0xD7],
            }
        }
    }

    impl Effects {
        pub(crate) fn lift(ram: &[u8; 2048], wram: &[u8; 8192]) -> Effects {
            let projectiles = std::array::from_fn(|i| {
                let mut projectile = Projectile::default();
                for array in PROJECTILE_ARRAYS {
                    projectile.set_byte(array, wram[usize::from(array - 0x6000) + i]);
                }
                projectile
            });
            Effects {
                sound: [ram[0x42], ram[0x43]],
                music: ram[0x40],
                song: ram[0x41],
                player: Player::lift(ram),
                weapon: Weapon::from_byte(ram[0x476]).expect("a weapon 0-6 at $0476"),
                lives: ram[0x482],
                meter: ram[0x477],
                stored_weapon: Weapon::from_byte(ram[0x483]).expect("a weapon 0-6 at $0483"),
                transition: ram[0xBA],
                transition_arg: ram[0xBB],
                busy: ram[0xAF],
                attack: ram[0xB8],
                boss_hp: word(ram, 0xA9),
                boss_pool: [ram[0xB2], ram[0xB3]],
                scroll_lock: ram[0xC2],
                hud: ram[0x404],
                cutscene_id: ram[0x406],
                misc_flag: ram[0x475],
                scroll_y: ram[0xD5],
                stage_end: super::super::stage::StageEnd {
                    phase: ram[0x408],
                    timer: ram[0x409],
                    x: ram[0x40A],
                    y: ram[0x40B],
                    fade: ram[0x40C],
                },
                stage_args: [ram[0x40E], ram[0x40F]],
                palette: ram[0x4E0..0x500].try_into().expect("32 bytes"),
                palette_dirty: ram[0xB5],
                animated: ram[0x401],
                projectiles,
                pause_name: wram[0x1FD0..0x1FE2].try_into().expect("18 bytes"),
                pause_aux: wram[0x1FF1],
                level: Level {
                    map: crate::levelmap::Map::from_bytes(
                        wram[..0x1000].try_into().expect("the map"),
                    ),
                    corners: std::array::from_fn(|id| {
                        [0x1000, 0x10D0, 0x11A0, 0x1270].map(|table| wram[table + id])
                    }),
                    classes: wram[0x1340..0x1410].try_into().expect("the classes"),
                    attributes: wram[0x1600..0x1A00].try_into().expect("the attributes"),
                    unread: Level::pack_unread(&wram[0x1000..0x2000]),
                },
                vram: Vram {
                    top: ram[0x181],
                    writes: Vram::lift_writes(ram),
                    attributes: wram[0x1530..0x15B0].try_into().expect("the attributes"),
                    origin: word(ram, 0xCA),
                    direct: Vec::new(),
                },
            }
        }

        pub(crate) fn project(&self, ram: &mut [u8; 2048], wram: &mut [u8; 8192]) {
            [ram[0x42], ram[0x43]] = self.sound;
            ram[0x40] = self.music;
            ram[0x41] = self.song;
            self.player.project(ram);
            ram[0x476] = self.weapon.byte();
            ram[0x482] = self.lives;
            ram[0x477] = self.meter;
            ram[0x483] = self.stored_weapon.byte();
            ram[0xBA] = self.transition;
            ram[0xBB] = self.transition_arg;
            ram[0xAF] = self.busy;
            ram[0xB8] = self.attack;
            ram[0xA9..0xAB].copy_from_slice(&self.boss_hp.to_be_bytes());
            [ram[0xB2], ram[0xB3]] = self.boss_pool;
            ram[0xC2] = self.scroll_lock;
            ram[0x404] = self.hud;
            ram[0x406] = self.cutscene_id;
            ram[0x475] = self.misc_flag;
            ram[0xD5] = self.scroll_y;
            let end = &self.stage_end;
            ram[0x408..0x40D].copy_from_slice(&[end.phase, end.timer, end.x, end.y, end.fade]);
            [ram[0x40E], ram[0x40F]] = self.stage_args;
            ram[0x4E0..0x500].copy_from_slice(&self.palette);
            ram[0xB5] = self.palette_dirty;
            ram[0x401] = self.animated;
            wram[..0x1000].copy_from_slice(self.level.map.as_bytes());
            for (id, corners) in self.level.corners.iter().enumerate() {
                for (table, &corner) in [0x1000, 0x10D0, 0x11A0, 0x1270].iter().zip(corners) {
                    wram[table + id] = corner;
                }
            }
            wram[0x1340..0x1410].copy_from_slice(&self.level.classes);
            wram[0x1600..0x1A00].copy_from_slice(&self.level.attributes);
            self.level.unpack_unread(&mut wram[0x1000..]);
            wram[0x1530..0x15B0].copy_from_slice(&self.vram.attributes);
            wram[0x1FD0..0x1FE2].copy_from_slice(&self.pause_name);
            wram[0x1FF1] = self.pause_aux;
            self.vram.project(ram);
            for (i, projectile) in self.projectiles.iter().enumerate() {
                for array in PROJECTILE_ARRAYS {
                    wram[usize::from(array - 0x6000) + i] = projectile.byte(array);
                }
            }
        }
    }

    impl Vram {
        /// The queued entries from `$0181` up to `$017F`, oldest first.
        fn lift_writes(ram: &[u8; 2048]) -> Vec<VramWrite> {
            let mut writes = Vec::new();
            let mut at = usize::from(ram[0x181]) + 1;
            while at + 4 <= 0x80 {
                let entry = &ram[0x100 + at..0x180];
                let count = usize::from(entry[3]);
                let Some(bytes) = entry.get(4..4 + count) else {
                    break;
                };
                writes.push(VramWrite {
                    addr: u16::from_be_bytes([entry[1], entry[2]]),
                    flags: entry[0],
                    bytes: bytes.to_vec(),
                });
                at += 4 + count;
            }
            writes.reverse();
            writes
        }

        fn project(&self, ram: &mut [u8; 2048]) {
            let queued: usize = self.writes.iter().map(|write| write.bytes.len() + 4).sum();
            let mut at = usize::from(self.top) + queued;
            let mut put = |byte: u8| {
                ram[0x100 + (at & 0xFF)] = byte;
                at = at.wrapping_sub(1);
            };
            for write in &self.writes {
                let [hi, lo] = write.addr.to_be_bytes();
                write.bytes.iter().rev().copied().for_each(&mut put);
                let count = u8::try_from(write.bytes.len()).expect("a byte count");
                [count, lo, hi, write.flags].into_iter().for_each(&mut put);
            }
            ram[0x181] = self.top;
            ram[0xCA..0xCC].copy_from_slice(&self.origin.to_be_bytes());
        }
    }

    impl Schedule {
        pub(crate) fn lift(ram: &[u8; 2048], wram: &[u8; 8192]) -> Schedule {
            Schedule {
                cells: wram[0x1410..0x1510].try_into().expect("256 cells"),
                page_columns: ram[0x440..0x460].try_into().expect("32 page columns"),
            }
        }

        pub(crate) fn project(&self, ram: &mut [u8; 2048], wram: &mut [u8; 8192]) {
            wram[0x1410..0x1510].copy_from_slice(&self.cells);
            ram[0x440..0x460].copy_from_slice(&self.page_columns);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Motion, Rng, Weapon};

    #[test]
    fn set_first_colours_sets_every_fourth_entry() {
        let mut palette: [u8; 32] = std::array::from_fn(|i| u8::try_from(i).expect("32"));
        super::set_first_colours(&mut palette, 0x0F);
        for (i, &colour) in palette.iter().enumerate() {
            let expected = if i % 4 == 0 {
                0x0F
            } else {
                u8::try_from(i).expect("32")
            };
            assert_eq!(colour, expected, "entry {i}");
        }
    }

    #[test]
    fn the_pose_bits_are_the_games() {
        use super::pose;
        assert_eq!(
            [
                pose::TURNING,
                pose::SHOT,
                pose::RESUME,
                pose::LANDING,
                pose::SAVED_MOTION,
                pose::LONG_LANDING
            ],
            [0x80, 0x40, 0x20, 0x10, 0x0F, 0x01]
        );
    }

    #[test]
    fn weapon_bytes_round_trip() {
        for byte in 0..=6 {
            let weapon = Weapon::from_byte(byte).expect("a weapon");
            assert_eq!(weapon.byte(), byte);
        }
        assert!((7..=255).all(|byte| Weapon::from_byte(byte).is_none()));
    }

    #[test]
    fn motion_bytes_round_trip() {
        for byte in 0..=11 {
            let motion = Motion::from_byte(byte).expect("a motion");
            assert_eq!(motion.byte(), byte);
        }
        assert!((12..=255).all(|byte| Motion::from_byte(byte).is_none()));
    }

    #[test]
    fn on_foot_is_standing_crouching_or_walking() {
        let on_foot: Vec<u8> = (0..=11)
            .filter_map(Motion::from_byte)
            .filter(|motion| motion.on_foot())
            .map(Motion::byte)
            .collect();
        assert_eq!(on_foot, [0, 1, 2]);
    }

    #[test]
    fn rng_steps_like_c21c() {
        let mut rng = Rng([0x01, 0x02]);
        rng.next_frame();
        assert_eq!(rng.0, [0x80, 0x81]);
        rng.next_frame();
        assert_eq!(rng.0, [0x40, 0x40]);
    }
}
