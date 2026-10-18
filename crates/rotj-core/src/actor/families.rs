//! Handler families: one module per handler, and the arms that dispatch to them.

/// A family's phases (`$7DD0`) as an enum whose discriminants are the bytes the game stores,
/// with `byte`, `from_byte` and a test that the two round-trip exactly on those bytes.
macro_rules! phases {
    ($(#[$meta:meta])* $name:ident {
        $($(#[$variant_meta:meta])* $variant:ident = $byte:literal),+ $(,)?
    }) => {
        $(#[$meta])*
        #[repr(u8)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($(#[$variant_meta])* $variant = $byte),+
        }

        impl $name {
            /// The byte `$7DD0` holds.
            #[must_use]
            pub const fn byte(self) -> u8 {
                self as u8
            }

            /// The phase a `$7DD0` byte holds, if it is one.
            #[must_use]
            pub const fn from_byte(byte: u8) -> Option<$name> {
                match byte {
                    $($byte => Some($name::$variant),)+
                    _ => None,
                }
            }
        }

        #[cfg(test)]
        #[test]
        fn phase_bytes_round_trip() {
            for byte in 0..=u8::MAX {
                let declared = [$($byte),+].contains(&byte);
                assert_eq!($name::from_byte(byte).map($name::byte), declared.then_some(byte));
            }
        }
    };
}

pub mod ace_ranger;
pub mod ace_ranger_shot;
pub mod blast_walker;
pub mod bobber_spawner;
pub mod bomber_bomb;
pub mod boss_defeated_script;
pub mod boss_shell;
pub mod boss_status;
pub mod breakable_block;
pub mod camera_floor;
pub mod cell_painter;
pub mod collapsing_floor_controller;
pub mod column_eraser;
pub mod crate_soldier;
pub mod crumbling_block;
pub mod diving_bomber;
pub mod diving_dropper;
pub mod diving_dropper_part;
pub mod drift_shot;
pub mod drift_wave_spawner;
pub mod drifting_bomber;
pub mod drifting_decoration;
pub mod dropper;
pub mod dropper_payload;
pub mod enemy_shot;
pub mod explosion;
pub mod falling_debris;
pub mod falling_enemy;
pub mod falling_enemy_spawner;
pub mod falling_floor_piece;
pub mod falling_hazard;
pub mod falling_hazard_trigger;
pub mod falling_platform;
pub mod falling_scenery;
pub mod final_boss;
pub mod final_boss_bouncer;
pub mod final_boss_defeat;
pub mod final_boss_falling_part;
pub mod final_boss_falling_shot;
pub mod final_boss_part;
pub mod final_boss_shot;
pub mod flash_marker;
pub mod flight_steer;
pub mod floor_debris;
pub mod harmless_burst;
pub mod hazard_dropper;
pub mod hop_thrower;
pub mod hop_thrower_shot;
pub mod hover_diver;
pub mod hover_plummeter;
pub mod hover_shooter;
pub mod hover_shot;
pub mod hurtful_burst;
pub mod ledge_walker;
pub mod lunging_enemy;
pub mod marker_trigger;
pub mod mirrored_scroller;
pub mod next_area_cue;
pub mod next_area_gate;
pub mod opening_fall_warp;
pub mod page_column_reset;
pub mod page_table_pair_patch;
pub mod page_table_tail_clear;
pub mod page_table_tail_set;
pub mod path_platform;
pub mod patrol_walker;
pub mod pickup;
pub mod proximity_bomb;
pub mod room_exit;
pub mod scenery_spawner;
pub mod shell_spawner;
pub mod shot_impact_burst;
pub mod sideways_platform;
pub mod sinking_platform;
pub mod spellcaster;
pub mod spike_probe;
pub mod spinner;
pub mod spinner_spawner;
pub mod spit_shot;
pub mod spitter;
pub mod stage3_boss;
pub mod stage3_boss_shot;
pub mod stage4_boss;
pub mod stage4_boss_shot;
pub mod stage4_boss_slam;
pub mod stage6_boss;
pub mod stage6_boss_death_part;
pub mod stage6_boss_part;
pub mod stage6_fixed_sprite;
pub mod stage6_lobbed_piece;
pub mod stage6_lobber;
pub mod stage6_lobber_spawner;
pub mod stage_end_trigger;
pub mod stomper;
pub mod swinging_hazard;
pub mod table_bobber;
pub mod top_screen_room_exit;
pub mod turret;
pub mod two_stage_drifter;
pub mod two_stage_drifter_continuation;
pub mod vertical_bobber;
pub mod vertical_shuttle_platform;
pub mod volley_shot;
pub mod volley_thrower;
pub mod waypoint_platform;

use crate::actor::engine::Work;
use crate::actor::slots::{Second, Slot};
use crate::actor::world::Cx;

/// The families' behaviour tables, imported from the PRG.
pub struct Tables {
    /// [`ace_ranger`]'s.
    pub ace_ranger: ace_ranger::Tables,
    /// [`ace_ranger_shot`]'s.
    pub ace_ranger_shot: ace_ranger_shot::Tables,
    /// [`boss_defeated_script`]'s.
    pub boss_defeated_script: boss_defeated_script::Tables,
    /// [`collapsing_floor_controller`]'s.
    pub collapsing_floor_controller: collapsing_floor_controller::Tables,
    /// [`crate_soldier`]'s.
    pub crate_soldier: crate_soldier::Tables,
    /// [`drift_shot`]'s.
    pub drift_shot: drift_shot::Tables,
    /// [`drift_wave_spawner`]'s.
    pub drift_wave_spawner: drift_wave_spawner::Tables,
    /// [`dropper`]'s.
    pub dropper: dropper::Tables,
    /// [`enemy_shot`]'s.
    pub enemy_shot: enemy_shot::Tables,
    /// [`final_boss`]'s.
    pub final_boss: final_boss::Tables,
    /// [`final_boss_defeat`]'s.
    pub final_boss_defeat: final_boss_defeat::Tables,
    /// [`final_boss_part`]'s.
    pub final_boss_part: final_boss_part::Tables,
    /// [`flight_steer`]'s.
    pub flight_steer: flight_steer::Tables,
    /// [`harmless_burst`]'s.
    pub harmless_burst: harmless_burst::Tables,
    /// [`hazard_dropper`]'s.
    pub hazard_dropper: hazard_dropper::Tables,
    /// [`hop_thrower_shot`]'s.
    pub hop_thrower_shot: hop_thrower_shot::Tables,
    /// [`ledge_walker`]'s.
    pub ledge_walker: ledge_walker::Tables,
    /// [`lunging_enemy`]'s.
    pub lunging_enemy: lunging_enemy::Tables,
    /// [`path_platform`]'s.
    pub path_platform: path_platform::Tables,
    /// [`patrol_walker`]'s.
    pub patrol_walker: patrol_walker::Tables,
    /// [`scenery_spawner`]'s.
    pub scenery_spawner: scenery_spawner::Tables,
    /// [`shell_spawner`]'s.
    pub shell_spawner: shell_spawner::Tables,
    /// [`spike_probe`]'s.
    pub spike_probe: spike_probe::Tables,
    /// [`spinner`]'s.
    pub spinner: spinner::Tables,
    /// [`spit_shot`]'s.
    pub spit_shot: spit_shot::Tables,
    /// [`spitter`]'s.
    pub spitter: spitter::Tables,
    /// [`stage3_boss`]'s.
    pub stage3_boss: stage3_boss::Tables,
    /// [`stage3_boss_shot`]'s.
    pub stage3_boss_shot: stage3_boss_shot::Tables,
    /// [`stage4_boss`]'s.
    pub stage4_boss: stage4_boss::Tables,
    /// [`stage4_boss_shot`]'s.
    pub stage4_boss_shot: stage4_boss_shot::Tables,
    /// [`stage6_boss`]'s.
    pub stage6_boss: stage6_boss::Tables,
    /// [`stage6_boss_death_part`]'s.
    pub stage6_boss_death_part: stage6_boss_death_part::Tables,
    /// [`stomper`]'s.
    pub stomper: stomper::Tables,
    /// [`swinging_hazard`]'s.
    pub swinging_hazard: swinging_hazard::Tables,
    /// [`table_bobber`]'s.
    pub table_bobber: table_bobber::Tables,
    /// [`volley_shot`]'s.
    pub volley_shot: volley_shot::Tables,
    /// [`volley_thrower`]'s.
    pub volley_thrower: volley_thrower::Tables,
    /// [`waypoint_platform`]'s.
    pub waypoint_platform: waypoint_platform::Tables,
}

impl Tables {
    /// Reads every family's tables.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            ace_ranger: ace_ranger::Tables::import(prg),
            ace_ranger_shot: ace_ranger_shot::Tables::import(prg),
            boss_defeated_script: boss_defeated_script::Tables::import(prg),
            collapsing_floor_controller: collapsing_floor_controller::Tables::import(prg),
            crate_soldier: crate_soldier::Tables::import(prg),
            drift_shot: drift_shot::Tables::import(prg),
            drift_wave_spawner: drift_wave_spawner::Tables::import(prg),
            dropper: dropper::Tables::import(prg),
            enemy_shot: enemy_shot::Tables::import(prg),
            final_boss: final_boss::Tables::import(prg),
            final_boss_defeat: final_boss_defeat::Tables::import(prg),
            final_boss_part: final_boss_part::Tables::import(prg),
            flight_steer: flight_steer::Tables::import(prg),
            harmless_burst: harmless_burst::Tables::import(prg),
            hazard_dropper: hazard_dropper::Tables::import(prg),
            hop_thrower_shot: hop_thrower_shot::Tables::import(prg),
            ledge_walker: ledge_walker::Tables::import(prg),
            lunging_enemy: lunging_enemy::Tables::import(prg),
            path_platform: path_platform::Tables::import(prg),
            patrol_walker: patrol_walker::Tables::import(prg),
            scenery_spawner: scenery_spawner::Tables::import(prg),
            shell_spawner: shell_spawner::Tables::import(prg),
            spike_probe: spike_probe::Tables::import(prg),
            spinner: spinner::Tables::import(prg),
            spit_shot: spit_shot::Tables::import(prg),
            spitter: spitter::Tables::import(prg),
            stage3_boss: stage3_boss::Tables::import(prg),
            stage3_boss_shot: stage3_boss_shot::Tables::import(prg),
            stage4_boss: stage4_boss::Tables::import(prg),
            stage4_boss_shot: stage4_boss_shot::Tables::import(prg),
            stage6_boss: stage6_boss::Tables::import(prg),
            stage6_boss_death_part: stage6_boss_death_part::Tables::import(prg),
            stomper: stomper::Tables::import(prg),
            swinging_hazard: swinging_hazard::Tables::import(prg),
            table_bobber: table_bobber::Tables::import(prg),
            volley_shot: volley_shot::Tables::import(prg),
            volley_thrower: volley_thrower::Tables::import(prg),
            waypoint_platform: waypoint_platform::Tables::import(prg),
        }
    }
}

/// Declares every handler family once: `Brain` and `SecondBrain` (one variant per
/// family, its kinds in its doc), `Brain::new` and `SecondBrain::new` (kind to family),
/// and the main slots' dispatch `step`.
macro_rules! families {
    (
        main {
            $($(#[$main_meta:meta])* $($main_kind:literal)|+ => $main:ident($main_module:ident),)+
        }
        second {
            $($(#[$second_meta:meta])* $($second_kind:literal)|+ => $second:ident($second_module:ident),)+
        }
    ) => {
        /// Each main family's module name and kinds.
        pub const MAIN: &[(&str, &[u8])] = &[$((stringify!($main_module), &[$($main_kind),+]),)+];

        /// Each second-class family's module name and kinds.
        pub const SECOND: &[(&str, &[u8])] = &[$((stringify!($second_module), &[$($second_kind),+]),)+];

        /// A handler family's own state.
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        pub enum Brain {
            /// A type the port does not run yet.
            #[default]
            Unported,
            $(
                #[doc = concat!("Kind `", stringify!($($main_kind)|+), "`.")]
                $(#[$main_meta])*
                $main($main_module::State),
            )+
        }

        /// A second-class handler family's own state (slots 24-31).
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        pub enum SecondBrain {
            /// A type the port does not run yet.
            #[default]
            Unported,
            $(
                #[doc = concat!("Kind `", stringify!($($second_kind)|+), "`.")]
                $(#[$second_meta])*
                $second($second_module::State),
            )+
        }

        impl Brain {
            /// The state an actor of `kind` starts in.
            #[must_use]
            #[deny(unreachable_patterns)]
            pub fn new(kind: u8) -> Brain {
                match kind {
                    $($($main_kind)|+ => Brain::$main($main_module::State),)+
                    _ => Brain::Unported,
                }
            }
        }

        impl SecondBrain {
            /// The state a second-class actor of `kind` starts in (the `$CEC3` table).
            #[must_use]
            #[deny(unreachable_patterns)]
            pub fn new(kind: u8) -> SecondBrain {
                match kind {
                    $($($second_kind)|+ => SecondBrain::$second($second_module::State),)+
                    _ => SecondBrain::Unported,
                }
            }
        }

        /// Runs one slot's handler (the `$CF72` dispatch).
        pub(crate) fn step(slot: &mut Slot, work: &mut Work, cx: &mut Cx<'_>) {
            slot.brain.follow(slot.kind);
            match &mut slot.brain {
                Brain::Unported => {
                    crate::actor::engine::unported(&mut slot.kind, &slot.body, work, cx);
                }
                $(Brain::$main(state) => state.step(&mut slot.kind, &mut slot.body, work, cx),)+
            }
        }
    };
}

families! {
    main {
        0x02 => RoomExit(room_exit),
        0x04 | 0x06 => BreakableBlock(breakable_block),
        0x08 => HarmlessBurst(harmless_burst),
        0x0A => HoverDiver(hover_diver),
        0x0C => Stomper(stomper),
        0x0E => StageEndTrigger(stage_end_trigger),
        0x10 => Pickup(pickup),
        0x12 => ScenerySpawner(scenery_spawner),
        0x14 => FallingScenery(falling_scenery),
        0x16 => MirroredScroller(mirrored_scroller),
        0x18 => HurtfulBurst(hurtful_burst),
        0x1A => TopScreenRoomExit(top_screen_room_exit),
        0x1C => CellPainter(cell_painter),
        0x1E => HoverShooter(hover_shooter),
        0x20 => HoverShot(hover_shot),
        0x22 => CrateSoldier(crate_soldier),
        0x24 => EnemyShot(enemy_shot),
        0x26 => PatrolWalker(patrol_walker),
        0x28 => WaypointPlatform(waypoint_platform),
        0x2A => DivingBomber(diving_bomber),
        0x2C => BomberBomb(bomber_bomb),
        0x2E => Dropper(dropper),
        0x30 => DropperPayload(dropper_payload),
        0x32 => NextAreaGate(next_area_gate),
        0x34 => BossStatus(boss_status),
        0x36 => DriftingBomber(drifting_bomber),
        0x38 => AceRanger(ace_ranger),
        /// The `$38` boss's parts.
        0x3A | 0x3C => AceRangerShot(ace_ranger_shot),
        0x3E => HopThrower(hop_thrower),
        0x40 => HopThrowerShot(hop_thrower_shot),
        0x42 => Stage4Boss(stage4_boss),
        0x44 => Stage4BossShot(stage4_boss_shot),
        0x46 => Stage4BossSlam(stage4_boss_slam),
        0x48 => VolleyThrower(volley_thrower),
        0x4A => VolleyShot(volley_shot),
        0x4C => ShellSpawner(shell_spawner),
        0x4E => BossShell(boss_shell),
        0x50 => BossDefeatedScript(boss_defeated_script),
        0x52 => Explosion(explosion),
        0x54 => FlightSteer(flight_steer),
        0x56 => ColumnEraser(column_eraser),
        /// The stage 5 probe/palette pair.
        0x58 | 0x5A => SpikeProbe(spike_probe),
        0x5C => DriftingDecoration(drifting_decoration),
        0x5E => SwingingHazard(swinging_hazard),
        0x60 => ProximityBomb(proximity_bomb),
        0x62 => FallingDebris(falling_debris),
        0x64 => TableBobber(table_bobber),
        0x66 => VerticalBobber(vertical_bobber),
        0x68 => BobberSpawner(bobber_spawner),
        0x6A => LungingEnemy(lunging_enemy),
        0x6C => Spellcaster(spellcaster),
        0x6E => TwoStageDrifter(two_stage_drifter),
        0x70 => TwoStageDrifterContinuation(two_stage_drifter_continuation),
        0x72 => DivingDropper(diving_dropper),
        0x74 => DivingDropperPart(diving_dropper_part),
        0x76 => PageTablePairPatch(page_table_pair_patch),
        0x78 => MarkerTrigger(marker_trigger),
        0x7A => FlashMarker(flash_marker),
        0x7C => BlastWalker(blast_walker),
        0x7E => Spinner(spinner),
        0x80 => SpinnerSpawner(spinner_spawner),
        0x82 => NextAreaCue(next_area_cue),
        0x84 => CollapsingFloorController(collapsing_floor_controller),
        0x86 => FloorDebris(floor_debris),
        0x88 => LedgeWalker(ledge_walker),
        0x8A => Spitter(spitter),
        0x8C => SpitShot(spit_shot),
        0x8E => Stage3Boss(stage3_boss),
        0x90 => Stage3BossShot(stage3_boss_shot),
        0x92 => ShotImpactBurst(shot_impact_burst),
        0x94 => Stage6LobberSpawner(stage6_lobber_spawner),
        0x96 => Stage6Lobber(stage6_lobber),
        0x98 => Stage6FixedSprite(stage6_fixed_sprite),
        0x9A => Stage6LobbedPiece(stage6_lobbed_piece),
        0x9C => PageTableTailSet(page_table_tail_set),
        0x9E => HoverPlummeter(hover_plummeter),
        0xA0 => FallingHazardTrigger(falling_hazard_trigger),
        0xA2 => FallingHazard(falling_hazard),
        0xA4 => HazardDropper(hazard_dropper),
        0xA6 => Stage6Boss(stage6_boss),
        0xA8 => Stage6BossPart(stage6_boss_part),
        0xAA => Stage6BossDeathPart(stage6_boss_death_part),
        0xAC => Turret(turret),
        0xAE => FallingEnemy(falling_enemy),
        0xB0 => FallingEnemySpawner(falling_enemy_spawner),
        0xB2 => PageColumnReset(page_column_reset),
        0xB4 => FinalBoss(final_boss),
        0xB6 => FinalBossPart(final_boss_part),
        0xB8 => FinalBossDefeat(final_boss_defeat),
        0xBA => FinalBossShot(final_boss_shot),
        0xBC => FinalBossFallingShot(final_boss_falling_shot),
        0xBE => FinalBossFallingPart(final_boss_falling_part),
        0xC0 => FinalBossBouncer(final_boss_bouncer),
        0xC2 => DriftWaveSpawner(drift_wave_spawner),
        0xC4 => DriftShot(drift_shot),
        0xC6 => OpeningFallWarp(opening_fall_warp),
        0xC8 => PageTableTailClear(page_table_tail_clear),
    }
    second {
        0x02 => SidewaysPlatform(sideways_platform),
        0x04 => VerticalShuttlePlatform(vertical_shuttle_platform),
        0x06 => CrumblingBlock(crumbling_block),
        0x08 => SinkingPlatform(sinking_platform),
        0x0A => PathPlatform(path_platform),
        0x0C => CameraFloor(camera_floor),
        0x0E => FallingFloorPiece(falling_floor_piece),
        0x10 => FallingPlatform(falling_platform),
    }
}

/// Runs one second-class slot's handler (the `$CE7F` dispatch).
pub(crate) fn step_second(index: usize, second: &mut Second, work: &mut Work, cx: &mut Cx<'_>) {
    match &mut second.brain {
        SecondBrain::VerticalShuttlePlatform(state) => state.step(index, &mut second.body, cx),
        SecondBrain::SidewaysPlatform(state) => state.step(index, &mut second.body, cx),
        SecondBrain::SinkingPlatform(state) => state.step(index, &mut second.body, work, cx),
        SecondBrain::PathPlatform(state) => {
            state.step(index, &mut second.kind, &mut second.body, cx);
        }
        SecondBrain::CameraFloor(state) => state.step(&mut second.body, cx),
        SecondBrain::CrumblingBlock(state) => {
            state.step(&mut second.kind, &mut second.body, work, cx);
        }
        SecondBrain::FallingFloorPiece(state) => {
            state.step(&mut second.kind, &mut second.body, work, cx);
        }
        SecondBrain::FallingPlatform(state) => {
            state.step(index, &mut second.kind, &mut second.body, cx);
        }
        SecondBrain::Unported => {}
    }
}

impl Brain {
    /// The brain for `kind`, kept when the slot's family has not changed.
    fn follow(&mut self, kind: u8) {
        let brain = Brain::new(kind);
        if std::mem::discriminant(self) != std::mem::discriminant(&brain) {
            *self = brain;
        }
    }
}

/// A family's state lifted from its slot's arrays.
#[cfg(test)]
pub(crate) fn lift(kind: u8, _wram: &[u8; 8192], _slot: usize) -> Brain {
    Brain::new(kind)
}
