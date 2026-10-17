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
pub mod path_platform;
pub mod patrol_walker;
pub mod pickup;
pub mod proximity_bomb;
pub mod room_exit;
pub mod row_base_reset;
pub mod row_table_pair_patch;
pub mod row_table_tail_clear;
pub mod row_table_tail_set;
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

/// A handler family's own state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Brain {
    /// A type the port does not run yet.
    #[default]
    Unported,
    /// Types `$04` and `$06`.
    BreakableBlock(breakable_block::State),
    /// Type `$02`.
    RoomExit(room_exit::State),
    /// Type `$08`.
    HarmlessBurst(harmless_burst::State),
    /// Type `$0A`.
    HoverDiver(hover_diver::State),
    /// Type `$0C`.
    Stomper(stomper::State),
    /// Type `$0E`.
    StageEndTrigger(stage_end_trigger::State),
    /// Type `$12`.
    ScenerySpawner(scenery_spawner::State),
    /// Type `$14`.
    FallingScenery(falling_scenery::State),
    /// Type `$16`.
    MirroredScroller(mirrored_scroller::State),
    /// Type `$18`.
    HurtfulBurst(hurtful_burst::State),
    /// Type `$1C`.
    CellPainter(cell_painter::State),
    /// Type `$1A`.
    TopScreenRoomExit(top_screen_room_exit::State),
    /// Type `$10`.
    Pickup(pickup::State),
    /// Type `$20`.
    HoverShot(hover_shot::State),
    /// Type `$1E`.
    HoverShooter(hover_shooter::State),
    /// Type `$96`.
    Stage6Lobber(stage6_lobber::State),
    /// Type `$B0`.
    FallingEnemySpawner(falling_enemy_spawner::State),
    /// Type `$B2`.
    RowBaseReset(row_base_reset::State),
    /// Type `$B6`.
    FinalBossPart(final_boss_part::State),
    /// Type `$B4`.
    FinalBoss(final_boss::State),
    /// Type `$B8`.
    FinalBossDefeat(final_boss_defeat::State),
    /// Type `$BA`.
    FinalBossShot(final_boss_shot::State),
    /// Type `$BE`.
    FinalBossFallingPart(final_boss_falling_part::State),
    /// Type `$BC`.
    FinalBossFallingShot(final_boss_falling_shot::State),
    /// Type `$24`.
    EnemyShot(enemy_shot::State),
    /// Type `$36`.
    DriftingBomber(drifting_bomber::State),
    /// Type `$66`.
    VerticalBobber(vertical_bobber::State),
    /// Type `$68`.
    BobberSpawner(bobber_spawner::State),
    /// Type `$26`.
    PatrolWalker(patrol_walker::State),
    /// Type `$32`.
    NextAreaGate(next_area_gate::State),
    /// Type `$28`.
    WaypointPlatform(waypoint_platform::State),
    /// Type `$34`.
    BossStatus(boss_status::State),
    /// Type `$6C`.
    Spellcaster(spellcaster::State),
    /// Type `$6E`.
    TwoStageDrifter(two_stage_drifter::State),
    /// Type `$70`.
    TwoStageDrifterContinuation(two_stage_drifter_continuation::State),
    /// Type `$54`.
    FlightSteer(flight_steer::State),
    /// Type `$76`.
    RowTablePairPatch(row_table_pair_patch::State),
    /// Type `$78`.
    MarkerTrigger(marker_trigger::State),
    /// Type `$7A`.
    FlashMarker(flash_marker::State),
    /// Type `$7C`.
    BlastWalker(blast_walker::State),
    /// Type `$74`.
    DivingDropperPart(diving_dropper_part::State),
    /// Type `$6A`.
    LungingEnemy(lunging_enemy::State),
    /// Second-class type `$04`.
    VerticalShuttlePlatform(vertical_shuttle_platform::State),
    /// Second-class type `$02`.
    SidewaysPlatform(sideways_platform::State),
    /// Second-class type `$08`.
    SinkingPlatform(sinking_platform::State),
    /// Second-class type `$0A`.
    PathPlatform(path_platform::State),
    /// Second-class type `$0C`.
    CameraFloor(camera_floor::State),
    /// Second-class type `$06`.
    CrumblingBlock(crumbling_block::State),
    /// Second-class type `$0E`.
    FallingFloorPiece(falling_floor_piece::State),
    /// Second-class type `$10`.
    FallingPlatform(falling_platform::State),
    /// Type `$22`.
    CrateSoldier(crate_soldier::State),
    /// Type `$C6`.
    OpeningFallWarp(opening_fall_warp::State),
    /// Type `$C2`.
    DriftWaveSpawner(drift_wave_spawner::State),
    /// Type `$C8`.
    RowTableTailClear(row_table_tail_clear::State),
    /// Type `$38`.
    AceRanger(ace_ranger::State),
    /// Type `$64`.
    TableBobber(table_bobber::State),
    /// Types `$3A` and `$3C`, the `$38` boss's parts.
    AceRangerShot(ace_ranger_shot::State),
    /// Type `$56`.
    ColumnEraser(column_eraser::State),
    /// Types `$58` and `$5A`, the stage 5 probe/palette pair.
    SpikeProbe(spike_probe::State),
    /// Type `$2E`.
    Dropper(dropper::State),
    /// Type `$30`.
    DropperPayload(dropper_payload::State),
    /// Type `$40`.
    HopThrowerShot(hop_thrower_shot::State),
    /// Type `$48`.
    VolleyThrower(volley_thrower::State),
    /// Type `$4A`.
    VolleyShot(volley_shot::State),
    /// Type `$44`.
    Stage4BossShot(stage4_boss_shot::State),
    /// Type `$5C`.
    DriftingDecoration(drifting_decoration::State),
    /// Type `$52`.
    Explosion(explosion::State),
    /// Type `$50`.
    BossDefeatedScript(boss_defeated_script::State),
    /// Type `$5E`.
    SwingingHazard(swinging_hazard::State),
    /// Type `$2A`.
    DivingBomber(diving_bomber::State),
    /// Type `$60`.
    ProximityBomb(proximity_bomb::State),
    /// Type `$62`.
    FallingDebris(falling_debris::State),
    /// Type `$72`.
    DivingDropper(diving_dropper::State),
    /// Type `$3E`.
    HopThrower(hop_thrower::State),
    /// Type `$86`.
    FloorDebris(floor_debris::State),
    /// Type `$88`.
    LedgeWalker(ledge_walker::State),
    /// Type `$84`.
    CollapsingFloorController(collapsing_floor_controller::State),
    /// Type `$42`.
    Stage4Boss(stage4_boss::State),
    /// Type `$4C`.
    ShellSpawner(shell_spawner::State),
    /// Type `$4E`.
    BossShell(boss_shell::State),
    /// Type `$2C`.
    BomberBomb(bomber_bomb::State),
    /// Type `$8A`.
    Spitter(spitter::State),
    /// Type `$8C`.
    SpitShot(spit_shot::State),
    /// Type `$80`.
    SpinnerSpawner(spinner_spawner::State),
    /// Type `$82`.
    NextAreaCue(next_area_cue::State),
    /// Type `$8E`.
    Stage3Boss(stage3_boss::State),
    /// Type `$7E`.
    Spinner(spinner::State),
    /// Type `$94`.
    Stage6LobberSpawner(stage6_lobber_spawner::State),
    /// Type `$98`.
    Stage6FixedSprite(stage6_fixed_sprite::State),
    /// Type `$9A`.
    Stage6LobbedPiece(stage6_lobbed_piece::State),
    /// Type `$A2`.
    FallingHazard(falling_hazard::State),
    /// Type `$AA`.
    Stage6BossDeathPart(stage6_boss_death_part::State),
    /// Type `$A4`.
    HazardDropper(hazard_dropper::State),
    /// Type `$A8`.
    Stage6BossPart(stage6_boss_part::State),
    /// Type `$A6`.
    Stage6Boss(stage6_boss::State),
    /// Type `$AE`.
    FallingEnemy(falling_enemy::State),
    /// Type `$C0`.
    FinalBossBouncer(final_boss_bouncer::State),
    /// Type `$9C`.
    RowTableTailSet(row_table_tail_set::State),
    /// Type `$A0`.
    FallingHazardTrigger(falling_hazard_trigger::State),
    /// Type `$9E`.
    HoverPlummeter(hover_plummeter::State),
    /// Type `$AC`.
    Turret(turret::State),
    /// Type `$C4`.
    DriftShot(drift_shot::State),
    /// Type `$46`.
    Stage4BossSlam(stage4_boss_slam::State),
    /// Type `$90`.
    Stage3BossShot(stage3_boss_shot::State),
    /// Type `$92`.
    ShotImpactBurst(shot_impact_burst::State),
}

impl Brain {
    /// The state an actor of `kind` starts in.
    #[must_use]
    pub fn new(kind: u8) -> Brain {
        match kind {
            0x04 | 0x06 => Brain::BreakableBlock(breakable_block::State),
            0x02 => Brain::RoomExit(room_exit::State),
            0x08 => Brain::HarmlessBurst(harmless_burst::State),
            0x0A => Brain::HoverDiver(hover_diver::State),
            0x0C => Brain::Stomper(stomper::State),
            0x0E => Brain::StageEndTrigger(stage_end_trigger::State),
            0x14 => Brain::FallingScenery(falling_scenery::State),
            0x16 => Brain::MirroredScroller(mirrored_scroller::State),
            0x18 => Brain::HurtfulBurst(hurtful_burst::State),
            0x1C => Brain::CellPainter(cell_painter::State),
            0x1A => Brain::TopScreenRoomExit(top_screen_room_exit::State),
            0x10 => Brain::Pickup(pickup::State),
            0x12 => Brain::ScenerySpawner(scenery_spawner::State),
            0x20 => Brain::HoverShot(hover_shot::State),
            0x1E => Brain::HoverShooter(hover_shooter::State),
            0x96 => Brain::Stage6Lobber(stage6_lobber::State),
            0xB0 => Brain::FallingEnemySpawner(falling_enemy_spawner::State),
            0xB2 => Brain::RowBaseReset(row_base_reset::State),
            0xB6 => Brain::FinalBossPart(final_boss_part::State),
            0xB4 => Brain::FinalBoss(final_boss::State),
            0xB8 => Brain::FinalBossDefeat(final_boss_defeat::State),
            0xBA => Brain::FinalBossShot(final_boss_shot::State),
            0xBE => Brain::FinalBossFallingPart(final_boss_falling_part::State),
            0xBC => Brain::FinalBossFallingShot(final_boss_falling_shot::State),
            0x24 => Brain::EnemyShot(enemy_shot::State),
            0x36 => Brain::DriftingBomber(drifting_bomber::State),
            0x66 => Brain::VerticalBobber(vertical_bobber::State),
            0x68 => Brain::BobberSpawner(bobber_spawner::State),
            0x26 => Brain::PatrolWalker(patrol_walker::State),
            0x32 => Brain::NextAreaGate(next_area_gate::State),
            0x28 => Brain::WaypointPlatform(waypoint_platform::State),
            0x34 => Brain::BossStatus(boss_status::State),
            0x6C => Brain::Spellcaster(spellcaster::State),
            0x6E => Brain::TwoStageDrifter(two_stage_drifter::State),
            0x70 => Brain::TwoStageDrifterContinuation(two_stage_drifter_continuation::State),
            0x54 => Brain::FlightSteer(flight_steer::State),
            0x76 => Brain::RowTablePairPatch(row_table_pair_patch::State),
            0x78 => Brain::MarkerTrigger(marker_trigger::State),
            0x7A => Brain::FlashMarker(flash_marker::State),
            0x7C => Brain::BlastWalker(blast_walker::State),
            0x6A => Brain::LungingEnemy(lunging_enemy::State),
            0x22 => Brain::CrateSoldier(crate_soldier::State),
            0xC6 => Brain::OpeningFallWarp(opening_fall_warp::State),
            0xC2 => Brain::DriftWaveSpawner(drift_wave_spawner::State),
            0xC8 => Brain::RowTableTailClear(row_table_tail_clear::State),
            0x38 => Brain::AceRanger(ace_ranger::State),
            0x74 => Brain::DivingDropperPart(diving_dropper_part::State),
            0x64 => Brain::TableBobber(table_bobber::State),
            0x3A | 0x3C => Brain::AceRangerShot(ace_ranger_shot::State),
            0x56 => Brain::ColumnEraser(column_eraser::State),
            0x58 | 0x5A => Brain::SpikeProbe(spike_probe::State),
            0x2E => Brain::Dropper(dropper::State),
            0x30 => Brain::DropperPayload(dropper_payload::State),
            0x40 => Brain::HopThrowerShot(hop_thrower_shot::State),
            0x48 => Brain::VolleyThrower(volley_thrower::State),
            0x4A => Brain::VolleyShot(volley_shot::State),
            0x44 => Brain::Stage4BossShot(stage4_boss_shot::State),
            0x5C => Brain::DriftingDecoration(drifting_decoration::State),
            0x52 => Brain::Explosion(explosion::State),
            0x50 => Brain::BossDefeatedScript(boss_defeated_script::State),
            0x5E => Brain::SwingingHazard(swinging_hazard::State),
            0x2A => Brain::DivingBomber(diving_bomber::State),
            0x60 => Brain::ProximityBomb(proximity_bomb::State),
            0x62 => Brain::FallingDebris(falling_debris::State),
            0x72 => Brain::DivingDropper(diving_dropper::State),
            0x3E => Brain::HopThrower(hop_thrower::State),
            0x86 => Brain::FloorDebris(floor_debris::State),
            0x88 => Brain::LedgeWalker(ledge_walker::State),
            0x84 => Brain::CollapsingFloorController(collapsing_floor_controller::State),
            0x42 => Brain::Stage4Boss(stage4_boss::State),
            0x4C => Brain::ShellSpawner(shell_spawner::State),
            0x4E => Brain::BossShell(boss_shell::State),
            0x2C => Brain::BomberBomb(bomber_bomb::State),
            0x8A => Brain::Spitter(spitter::State),
            0x8C => Brain::SpitShot(spit_shot::State),
            0x80 => Brain::SpinnerSpawner(spinner_spawner::State),
            0x82 => Brain::NextAreaCue(next_area_cue::State),
            0x8E => Brain::Stage3Boss(stage3_boss::State),
            0x7E => Brain::Spinner(spinner::State),
            0x94 => Brain::Stage6LobberSpawner(stage6_lobber_spawner::State),
            0x98 => Brain::Stage6FixedSprite(stage6_fixed_sprite::State),
            0x9A => Brain::Stage6LobbedPiece(stage6_lobbed_piece::State),
            0xA2 => Brain::FallingHazard(falling_hazard::State),
            0xAA => Brain::Stage6BossDeathPart(stage6_boss_death_part::State),
            0xA4 => Brain::HazardDropper(hazard_dropper::State),
            0xA8 => Brain::Stage6BossPart(stage6_boss_part::State),
            0xA6 => Brain::Stage6Boss(stage6_boss::State),
            0xAE => Brain::FallingEnemy(falling_enemy::State),
            0xC0 => Brain::FinalBossBouncer(final_boss_bouncer::State),
            0x9C => Brain::RowTableTailSet(row_table_tail_set::State),
            0xA0 => Brain::FallingHazardTrigger(falling_hazard_trigger::State),
            0x9E => Brain::HoverPlummeter(hover_plummeter::State),
            0xAC => Brain::Turret(turret::State),
            0xC4 => Brain::DriftShot(drift_shot::State),
            0x46 => Brain::Stage4BossSlam(stage4_boss_slam::State),
            0x90 => Brain::Stage3BossShot(stage3_boss_shot::State),
            0x92 => Brain::ShotImpactBurst(shot_impact_burst::State),
            _ => Brain::Unported,
        }
    }

    /// The state a second-class actor of `kind` starts in (the `$CEC3` table).
    #[must_use]
    pub fn new_second(kind: u8) -> Brain {
        match kind {
            0x04 => Brain::VerticalShuttlePlatform(vertical_shuttle_platform::State),
            0x02 => Brain::SidewaysPlatform(sideways_platform::State),
            0x08 => Brain::SinkingPlatform(sinking_platform::State),
            0x0A => Brain::PathPlatform(path_platform::State),
            0x0C => Brain::CameraFloor(camera_floor::State),
            0x06 => Brain::CrumblingBlock(crumbling_block::State),
            0x0E => Brain::FallingFloorPiece(falling_floor_piece::State),
            0x10 => Brain::FallingPlatform(falling_platform::State),
            _ => Brain::Unported,
        }
    }
}

/// Runs one second-class slot's handler (the `$CE7F` dispatch).
pub(crate) fn step_second(index: usize, second: &mut Second, work: &mut Work, cx: &mut Cx<'_>) {
    match &mut second.brain {
        Brain::VerticalShuttlePlatform(state) => state.step(index, &mut second.body, cx),
        Brain::SidewaysPlatform(state) => state.step(index, &mut second.body, cx),
        Brain::SinkingPlatform(state) => state.step(index, &mut second.body, work, cx),
        Brain::PathPlatform(state) => state.step(index, &mut second.kind, &mut second.body, cx),
        Brain::CameraFloor(state) => state.step(&mut second.body, cx),
        Brain::CrumblingBlock(state) => state.step(&mut second.kind, &mut second.body, work, cx),
        Brain::FallingFloorPiece(state) => state.step(&mut second.kind, &mut second.body, work, cx),
        Brain::FallingPlatform(state) => state.step(index, &mut second.kind, &mut second.body, cx),
        _ => {}
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

/// Matches a main slot's brain to its family's `step`.
macro_rules! main_families {
    ($slot:ident, $work:ident, $cx:ident; $($family:ident),* $(,)?) => {
        match &mut $slot.brain {
            Brain::Unported => {
                crate::actor::engine::unported(&mut $slot.kind, &$slot.body, $work, $cx);
            }
            Brain::VerticalShuttlePlatform(_) | Brain::SidewaysPlatform(_) | Brain::SinkingPlatform(_) | Brain::PathPlatform(_) | Brain::CameraFloor(_) | Brain::CrumblingBlock(_) | Brain::FallingFloorPiece(_) | Brain::FallingPlatform(_) => {
                panic!("a second-class family in a main slot")
            }
            $(Brain::$family(state) => state.step(&mut $slot.kind, &mut $slot.body, $work, $cx),)*
        }
    };
}

/// Calls `$callback!` with its arguments and every main family's variant, the one list
/// of the families a main slot can hold.
macro_rules! with_main_families {
    ($callback:ident!($($args:tt)*)) => {
        $callback!(
            $($args)*;
            BreakableBlock,
            RoomExit,
            HarmlessBurst,
            HoverDiver,
            Stomper,
            StageEndTrigger,
            FallingScenery,
            MirroredScroller,
            HurtfulBurst,
            CellPainter,
            TopScreenRoomExit,
            Pickup,
            ScenerySpawner,
            HoverShot,
            HoverShooter,
            Stage6Lobber,
            FallingEnemySpawner,
            RowBaseReset,
            FinalBossPart,
            FinalBoss,
            FinalBossDefeat,
            FinalBossShot,
            FinalBossFallingPart,
            FinalBossFallingShot,
            EnemyShot,
            DriftingBomber,
            VerticalBobber,
            BobberSpawner,
            PatrolWalker,
            NextAreaGate,
            WaypointPlatform,
            BossStatus,
            Spellcaster,
            TwoStageDrifter,
            TwoStageDrifterContinuation,
            FlightSteer,
            RowTablePairPatch,
            MarkerTrigger,
            FlashMarker,
            BlastWalker,
            LungingEnemy,
            CrateSoldier,
            OpeningFallWarp,
            DriftWaveSpawner,
            RowTableTailClear,
            AceRanger,
            DivingDropperPart,
            TableBobber,
            AceRangerShot,
            ColumnEraser,
            SpikeProbe,
            Dropper,
            DropperPayload,
            HopThrowerShot,
            VolleyThrower,
            VolleyShot,
            Stage4BossShot,
            DriftingDecoration,
            Explosion,
            BossDefeatedScript,
            SwingingHazard,
            DivingBomber,
            ProximityBomb,
            FallingDebris,
            DivingDropper,
            HopThrower,
            FloorDebris,
            LedgeWalker,
            CollapsingFloorController,
            Stage4Boss,
            ShellSpawner,
            BossShell,
            BomberBomb,
            Spitter,
            SpitShot,
            SpinnerSpawner,
            NextAreaCue,
            Stage3Boss,
            Spinner,
            Stage6LobberSpawner,
            Stage6FixedSprite,
            Stage6LobbedPiece,
            FallingHazard,
            Stage6BossDeathPart,
            HazardDropper,
            Stage6BossPart,
            Stage6Boss,
            FallingEnemy,
            FinalBossBouncer,
            RowTableTailSet,
            FallingHazardTrigger,
            HoverPlummeter,
            Turret,
            DriftShot,
            Stage4BossSlam,
            Stage3BossShot,
            ShotImpactBurst,
        )
    };
}

/// Runs one slot's handler (the `$CF72` dispatch).
pub(crate) fn step(slot: &mut Slot, work: &mut Work, cx: &mut Cx<'_>) {
    slot.brain.follow(slot.kind);
    with_main_families!(main_families!(slot, work, cx));
}

/// A family's state lifted from its slot's arrays.
#[cfg(test)]
pub(crate) fn lift(kind: u8, _wram: &[u8; 8192], _slot: usize) -> Brain {
    Brain::new(kind)
}
