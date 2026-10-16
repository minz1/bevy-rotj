//! Handler families: one module per handler, and the arms that dispatch to them.

pub mod k02;
pub mod k06;
pub mod k08;
pub mod k0a;
pub mod k0c;
pub mod k0e;
pub mod k10;
pub mod k12;
pub mod k14;
pub mod k16;
pub mod k18;
pub mod k1a;
pub mod k1c;
pub mod k1e;
pub mod k20;
pub mod k22;
pub mod k24;
pub mod k26;
pub mod k28;
pub mod k2a;
pub mod k2c;
pub mod k2e;
pub mod k30;
pub mod k32;
pub mod k34;
pub mod k36;
pub mod k38;
pub mod k3a;
pub mod k3e;
pub mod k40;
pub mod k42;
pub mod k44;
pub mod k46;
pub mod k48;
pub mod k4a;
pub mod k4c;
pub mod k4e;
pub mod k50;
pub mod k52;
pub mod k54;
pub mod k56;
pub mod k58;
pub mod k5c;
pub mod k5e;
pub mod k60;
pub mod k62;
pub mod k64;
pub mod k66;
pub mod k68;
pub mod k6a;
pub mod k6c;
pub mod k6e;
pub mod k70;
pub mod k72;
pub mod k74;
pub mod k76;
pub mod k78;
pub mod k7a;
pub mod k7c;
pub mod k7e;
pub mod k80;
pub mod k82;
pub mod k84;
pub mod k86;
pub mod k88;
pub mod k8a;
pub mod k8c;
pub mod k8e;
pub mod k90;
pub mod k92;
pub mod k94;
pub mod k96;
pub mod k98;
pub mod k9a;
pub mod k9c;
pub mod k9e;
pub mod ka0;
pub mod ka2;
pub mod ka4;
pub mod ka6;
pub mod ka8;
pub mod kaa;
pub mod kac;
pub mod kae;
pub mod kb0;
pub mod kb2;
pub mod kb4;
pub mod kb6;
pub mod kb8;
pub mod kba;
pub mod kbc;
pub mod kbe;
pub mod kc0;
pub mod kc2;
pub mod kc4;
pub mod kc6;
pub mod kc8;
pub mod s02;
pub mod s04;
pub mod s06;
pub mod s08;
pub mod s0a;
pub mod s0c;
pub mod s0e;
pub mod s10;

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
    K06(k06::State),
    /// Type `$02`.
    K02(k02::State),
    /// Type `$08`.
    K08(k08::State),
    /// Type `$0A`.
    K0a(k0a::State),
    /// Type `$0C`.
    K0c(k0c::State),
    /// Type `$0E`.
    K0e(k0e::State),
    /// Type `$12`.
    K12(k12::State),
    /// Type `$14`.
    K14(k14::State),
    /// Type `$16`.
    K16(k16::State),
    /// Type `$18`.
    K18(k18::State),
    /// Type `$1C`.
    K1c(k1c::State),
    /// Type `$1A`.
    K1a(k1a::State),
    /// Type `$10`.
    K10(k10::State),
    /// Type `$20`.
    K20(k20::State),
    /// Type `$1E`.
    K1e(k1e::State),
    /// Type `$96`.
    K96(k96::State),
    /// Type `$B0`.
    Kb0(kb0::State),
    /// Type `$B2`.
    Kb2(kb2::State),
    /// Type `$B6`.
    Kb6(kb6::State),
    /// Type `$B4`.
    Kb4(kb4::State),
    /// Type `$B8`.
    Kb8(kb8::State),
    /// Type `$BA`.
    Kba(kba::State),
    /// Type `$BE`.
    Kbe(kbe::State),
    /// Type `$BC`.
    Kbc(kbc::State),
    /// Type `$24`.
    K24(k24::State),
    /// Type `$36`.
    K36(k36::State),
    /// Type `$66`.
    K66(k66::State),
    /// Type `$68`.
    K68(k68::State),
    /// Type `$26`.
    K26(k26::State),
    /// Type `$32`.
    K32(k32::State),
    /// Type `$28`.
    K28(k28::State),
    /// Type `$34`.
    K34(k34::State),
    /// Type `$6C`.
    K6c(k6c::State),
    /// Type `$6E`.
    K6e(k6e::State),
    /// Type `$70`.
    K70(k70::State),
    /// Type `$54`.
    K54(k54::State),
    /// Type `$76`.
    K76(k76::State),
    /// Type `$78`.
    K78(k78::State),
    /// Type `$7A`.
    K7a(k7a::State),
    /// Type `$7C`.
    K7c(k7c::State),
    /// Type `$74`.
    K74(k74::State),
    /// Type `$6A`.
    K6a(k6a::State),
    /// Second-class type `$04`.
    S04(s04::State),
    /// Second-class type `$02`.
    S02(s02::State),
    /// Second-class type `$08`.
    S08(s08::State),
    /// Second-class type `$0A`.
    S0a(s0a::State),
    /// Second-class type `$0C`.
    S0c(s0c::State),
    /// Second-class type `$06`.
    S06(s06::State),
    /// Second-class type `$0E`.
    S0e(s0e::State),
    /// Second-class type `$10`.
    S10(s10::State),
    /// Type `$22`.
    K22(k22::State),
    /// Type `$C6`.
    Kc6(kc6::State),
    /// Type `$C2`.
    Kc2(kc2::State),
    /// Type `$C8`.
    Kc8(kc8::State),
    /// Type `$38`.
    K38(k38::State),
    /// Type `$64`.
    K64(k64::State),
    /// Types `$3A` and `$3C`, the `$38` boss's parts.
    K3a(k3a::State),
    /// Type `$56`.
    K56(k56::State),
    /// Types `$58` and `$5A`, the stage 5 probe/palette pair.
    K58(k58::State),
    /// Type `$2E`.
    K2e(k2e::State),
    /// Type `$30`.
    K30(k30::State),
    /// Type `$40`.
    K40(k40::State),
    /// Type `$48`.
    K48(k48::State),
    /// Type `$4A`.
    K4a(k4a::State),
    /// Type `$44`.
    K44(k44::State),
    /// Type `$5C`.
    K5c(k5c::State),
    /// Type `$52`.
    K52(k52::State),
    /// Type `$50`.
    K50(k50::State),
    /// Type `$5E`.
    K5e(k5e::State),
    /// Type `$2A`.
    K2a(k2a::State),
    /// Type `$60`.
    K60(k60::State),
    /// Type `$62`.
    K62(k62::State),
    /// Type `$72`.
    K72(k72::State),
    /// Type `$3E`.
    K3e(k3e::State),
    /// Type `$86`.
    K86(k86::State),
    /// Type `$88`.
    K88(k88::State),
    /// Type `$84`.
    K84(k84::State),
    /// Type `$42`.
    K42(k42::State),
    /// Type `$4C`.
    K4c(k4c::State),
    /// Type `$4E`.
    K4e(k4e::State),
    /// Type `$2C`.
    K2c(k2c::State),
    /// Type `$8A`.
    K8a(k8a::State),
    /// Type `$8C`.
    K8c(k8c::State),
    /// Type `$80`.
    K80(k80::State),
    /// Type `$82`.
    K82(k82::State),
    /// Type `$8E`.
    K8e(k8e::State),
    /// Type `$7E`.
    K7e(k7e::State),
    /// Type `$94`.
    K94(k94::State),
    /// Type `$98`.
    K98(k98::State),
    /// Type `$9A`.
    K9a(k9a::State),
    /// Type `$A2`.
    Ka2(ka2::State),
    /// Type `$AA`.
    Kaa(kaa::State),
    /// Type `$A4`.
    Ka4(ka4::State),
    /// Type `$A8`.
    Ka8(ka8::State),
    /// Type `$A6`.
    Ka6(ka6::State),
    /// Type `$AE`.
    Kae(kae::State),
    /// Type `$C0`.
    Kc0(kc0::State),
    /// Type `$9C`.
    K9c(k9c::State),
    /// Type `$A0`.
    Ka0(ka0::State),
    /// Type `$9E`.
    K9e(k9e::State),
    /// Type `$AC`.
    Kac(kac::State),
    /// Type `$C4`.
    Kc4(kc4::State),
    /// Type `$46`.
    K46(k46::State),
    /// Type `$90`.
    K90(k90::State),
    /// Type `$92`.
    K92(k92::State),
}

impl Brain {
    /// The state an actor of `kind` starts in.
    #[must_use]
    pub fn new(kind: u8) -> Brain {
        match kind {
            0x04 | 0x06 => Brain::K06(k06::State),
            0x02 => Brain::K02(k02::State),
            0x08 => Brain::K08(k08::State),
            0x0A => Brain::K0a(k0a::State),
            0x0C => Brain::K0c(k0c::State),
            0x0E => Brain::K0e(k0e::State),
            0x14 => Brain::K14(k14::State),
            0x16 => Brain::K16(k16::State),
            0x18 => Brain::K18(k18::State),
            0x1C => Brain::K1c(k1c::State),
            0x1A => Brain::K1a(k1a::State),
            0x10 => Brain::K10(k10::State),
            0x12 => Brain::K12(k12::State),
            0x20 => Brain::K20(k20::State),
            0x1E => Brain::K1e(k1e::State),
            0x96 => Brain::K96(k96::State),
            0xB0 => Brain::Kb0(kb0::State),
            0xB2 => Brain::Kb2(kb2::State),
            0xB6 => Brain::Kb6(kb6::State),
            0xB4 => Brain::Kb4(kb4::State),
            0xB8 => Brain::Kb8(kb8::State),
            0xBA => Brain::Kba(kba::State),
            0xBE => Brain::Kbe(kbe::State),
            0xBC => Brain::Kbc(kbc::State),
            0x24 => Brain::K24(k24::State),
            0x36 => Brain::K36(k36::State),
            0x66 => Brain::K66(k66::State),
            0x68 => Brain::K68(k68::State),
            0x26 => Brain::K26(k26::State),
            0x32 => Brain::K32(k32::State),
            0x28 => Brain::K28(k28::State),
            0x34 => Brain::K34(k34::State),
            0x6C => Brain::K6c(k6c::State),
            0x6E => Brain::K6e(k6e::State),
            0x70 => Brain::K70(k70::State),
            0x54 => Brain::K54(k54::State),
            0x76 => Brain::K76(k76::State),
            0x78 => Brain::K78(k78::State),
            0x7A => Brain::K7a(k7a::State),
            0x7C => Brain::K7c(k7c::State),
            0x6A => Brain::K6a(k6a::State),
            0x22 => Brain::K22(k22::State),
            0xC6 => Brain::Kc6(kc6::State),
            0xC2 => Brain::Kc2(kc2::State),
            0xC8 => Brain::Kc8(kc8::State),
            0x38 => Brain::K38(k38::State),
            0x74 => Brain::K74(k74::State),
            0x64 => Brain::K64(k64::State),
            0x3A | 0x3C => Brain::K3a(k3a::State),
            0x56 => Brain::K56(k56::State),
            0x58 | 0x5A => Brain::K58(k58::State),
            0x2E => Brain::K2e(k2e::State),
            0x30 => Brain::K30(k30::State),
            0x40 => Brain::K40(k40::State),
            0x48 => Brain::K48(k48::State),
            0x4A => Brain::K4a(k4a::State),
            0x44 => Brain::K44(k44::State),
            0x5C => Brain::K5c(k5c::State),
            0x52 => Brain::K52(k52::State),
            0x50 => Brain::K50(k50::State),
            0x5E => Brain::K5e(k5e::State),
            0x2A => Brain::K2a(k2a::State),
            0x60 => Brain::K60(k60::State),
            0x62 => Brain::K62(k62::State),
            0x72 => Brain::K72(k72::State),
            0x3E => Brain::K3e(k3e::State),
            0x86 => Brain::K86(k86::State),
            0x88 => Brain::K88(k88::State),
            0x84 => Brain::K84(k84::State),
            0x42 => Brain::K42(k42::State),
            0x4C => Brain::K4c(k4c::State),
            0x4E => Brain::K4e(k4e::State),
            0x2C => Brain::K2c(k2c::State),
            0x8A => Brain::K8a(k8a::State),
            0x8C => Brain::K8c(k8c::State),
            0x80 => Brain::K80(k80::State),
            0x82 => Brain::K82(k82::State),
            0x8E => Brain::K8e(k8e::State),
            0x7E => Brain::K7e(k7e::State),
            0x94 => Brain::K94(k94::State),
            0x98 => Brain::K98(k98::State),
            0x9A => Brain::K9a(k9a::State),
            0xA2 => Brain::Ka2(ka2::State),
            0xAA => Brain::Kaa(kaa::State),
            0xA4 => Brain::Ka4(ka4::State),
            0xA8 => Brain::Ka8(ka8::State),
            0xA6 => Brain::Ka6(ka6::State),
            0xAE => Brain::Kae(kae::State),
            0xC0 => Brain::Kc0(kc0::State),
            0x9C => Brain::K9c(k9c::State),
            0xA0 => Brain::Ka0(ka0::State),
            0x9E => Brain::K9e(k9e::State),
            0xAC => Brain::Kac(kac::State),
            0xC4 => Brain::Kc4(kc4::State),
            0x46 => Brain::K46(k46::State),
            0x90 => Brain::K90(k90::State),
            0x92 => Brain::K92(k92::State),
            _ => Brain::Unported,
        }
    }

    /// The state a second-class actor of `kind` starts in (the `$CEC3` table).
    #[must_use]
    pub fn new_second(kind: u8) -> Brain {
        match kind {
            0x04 => Brain::S04(s04::State),
            0x02 => Brain::S02(s02::State),
            0x08 => Brain::S08(s08::State),
            0x0A => Brain::S0a(s0a::State),
            0x0C => Brain::S0c(s0c::State),
            0x06 => Brain::S06(s06::State),
            0x0E => Brain::S0e(s0e::State),
            0x10 => Brain::S10(s10::State),
            _ => Brain::Unported,
        }
    }
}

/// Runs one second-class slot's handler (the `$CE7F` dispatch).
pub(crate) fn step_second(index: usize, second: &mut Second, work: &mut Work, cx: &mut Cx<'_>) {
    match &mut second.brain {
        Brain::S04(state) => state.step(index, &mut second.body, cx),
        Brain::S02(state) => state.step(index, &mut second.body, cx),
        Brain::S08(state) => state.step(index, &mut second.body, work, cx),
        Brain::S0a(state) => state.step(index, &mut second.kind, &mut second.body, cx),
        Brain::S0c(state) => state.step(&mut second.body, cx),
        Brain::S06(state) => state.step(&mut second.kind, &mut second.body, work, cx),
        Brain::S0e(state) => state.step(&mut second.kind, &mut second.body, work, cx),
        Brain::S10(state) => state.step(index, &mut second.kind, &mut second.body, cx),
        _ => {}
    }
}

/// Runs one slot's handler (the `$CF72` dispatch).
pub(crate) fn step(slot: &mut Slot, work: &mut Work, cx: &mut Cx<'_>) {
    let brain = Brain::new(slot.kind);
    if std::mem::discriminant(&slot.brain) != std::mem::discriminant(&brain) {
        slot.brain = brain;
    }
    macro_rules! main_families {
        ($($family:ident),* $(,)?) => {
            match &mut slot.brain {
                Brain::Unported => {
                    crate::actor::engine::unported(&mut slot.kind, &slot.body, work, cx);
                }
                Brain::S04(_) | Brain::S02(_) | Brain::S08(_) | Brain::S0a(_) | Brain::S0c(_) | Brain::S06(_) | Brain::S0e(_) | Brain::S10(_) => {
                    panic!("a second-class family in a main slot")
                }
                $(Brain::$family(state) => state.step(&mut slot.kind, &mut slot.body, work, cx),)*
            }
        };
    }
    main_families!(
        K06, K02, K08, K0a, K0c, K0e, K14, K16, K18, K1c, K1a, K10, K12, K20, K1e, K96, Kb0, Kb2,
        Kb6, Kb4, Kb8, Kba, Kbe, Kbc, K24, K36, K66, K68, K26, K32, K28, K34, K6c, K6e, K70, K54,
        K76, K78, K7a, K7c, K6a, K22, Kc6, Kc2, Kc8, K38, K74, K64, K3a, K56, K58, K2e, K30, K40,
        K48, K4a, K44, K5c, K52, K50, K5e, K2a, K60, K62, K72, K3e, K86, K88, K84, K42, K4c, K4e,
        K2c, K8a, K8c, K80, K82, K8e, K7e, K94, K98, K9a, Ka2, Kaa, Ka4, Ka8, Ka6, Kae, Kc0, K9c,
        Ka0, K9e, Kac, Kc4, K46, K90, K92,
    );
}

/// A family's state lifted from its slot's arrays.
#[cfg(test)]
pub(crate) fn lift(kind: u8, _wram: &[u8; 8192], _slot: usize) -> Brain {
    Brain::new(kind)
}
