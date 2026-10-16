#!/usr/bin/env python3
"""Batman: Return of the Joker (USA) - deterministic disassembly generator.

Reads the iNES ROM (the first argument, else ../raw/"Batman - Return of the Joker (USA).nes") and emits
header.s, chr.s, cutscene.s, bank00.s .. bank15.s, rotj.cfg into disasm/. Build with
disasm/assemble.sh, which byte-compares ld65's output against the ROM.

Two-stage pass (details in ../README.md and ../MAP.md):

1. Recursive-descent from the real entry points (bank 15's $FFFA vectors:
   RESET $E2E6, NMI $E1D0, IRQ $E2A9).  Every JSR/JMP/branch target reached is
   marked as a code start; unreached bytes stay `.byte` data.  Cross-window
   JSR/JMP recorded by the descent seed entry points in the switchable banks
   (FME-7 windows), resolved through the runtime window map below.  The
   indirect-jump dispatch tables (the `LDA tbl,y ... JMP (zp)` idiom) are found
   by their byte shape and their handler entries are seeded the same way.  The
   emulator-observed program counters (`observed_code.SEEDS`) are seeded too,
   each attributed to a bank with the corrected FME-7 window map (MAP.md §11).
   The seed discovery is iterated to a fixed point (MAX_DESCENT_PASSES), so
   seeding order cannot matter.  A bank with no known entry falls back to the
   historical linear sweep.

2. Emit: instructions where the descent proves code, grouped `.byte` runs
   everywhere else, plus labels for proven targets and a few `; tbl_*` / I/O
   comments for the data found during RE.

Byte-identity is structural: any byte the emitter is not certain about is a
`.byte`, and a decoded instruction is only emitted when all of its bytes are
unambiguously part of it.

FME-7 runtime window map used for cross-bank seeds (see MAP.md): the FME-7 maps
PRG banks into the four switchable windows and the game re-maps them per scene,
so one window hosts several banks at different times.  The captured evidence
(NMI shadow RAM $E8-$EB = C0 18 1D 1E) gives gameplay $8000=bank 0,
$A000=bank 8, $C000=bank 13, $6000=bank 14; bank 14 also runs at $C000 in
another scene.  Registers 8,9,A,B are the PRG windows (8 KiB), 0-7 CHR 1 KiB
banks, C mirroring, D IRQ control, E/F IRQ counter (standard FME-7; confirmed
by the NMI shadow-write loop at $E259 and the CHR loop at $E204).
"""
import hashlib
import os
import sys

ROM = os.path.normpath(os.path.join(os.path.dirname(__file__), "..", "..", "raw",
                                    "Batman - Return of the Joker (USA).nes"))
if len(sys.argv) > 1:
    ROM = os.path.abspath(sys.argv[1])
OUT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

HEADER_SIZE = 16
PRG_BANKS = 16
BANK_SIZE = 8192
PRG_SIZE = PRG_BANKS * BANK_SIZE
VEC_NAMES = ('nmi_entry', 'reset_entry', 'irq_entry')
MAX_DESCENT_PASSES = 8  # far-call discovery fixed-point bound

# --- opcode table: official NMOS 6502 instructions only ----------------------
TBL = {}  # opcode -> (mnemonic, mode, nbytes)


def _reg(op, mn, mode, n):
    TBL[op] = (mn, mode, n)


def _fill_table():
    for fam, mn in ((0x00, 'ora'), (0x20, 'and'), (0x40, 'eor'), (0x60, 'adc'),
                    (0xA0, 'lda'), (0xC0, 'cmp'), (0xE0, 'sbc')):
        for off, mode in ((0x01, 'izx'), (0x05, 'zp'), (0x09, 'imm'),
                          (0x0D, 'abs'), (0x11, 'izy'), (0x15, 'zpx'),
                          (0x19, 'aby'), (0x1D, 'abx')):
            _reg(fam + off, mn, mode, 3 if mode in ('abs', 'abx', 'aby') else 2)
    for fam, mn in ((0x00, 'asl'), (0x20, 'rol'), (0x40, 'lsr'), (0x60, 'ror')):
        _reg(fam + 0x06, mn, 'zp', 2)
        _reg(fam + 0x0A, mn, 'acc', 1)
        _reg(fam + 0x0E, mn, 'abs', 3)
        _reg(fam + 0x16, mn, 'zpx', 2)
        _reg(fam + 0x1E, mn, 'abx', 3)
    _reg(0xC6, 'dec', 'zp', 2); _reg(0xD6, 'dec', 'zpx', 2)
    _reg(0xCE, 'dec', 'abs', 3); _reg(0xDE, 'dec', 'abx', 3)
    _reg(0xE6, 'inc', 'zp', 2); _reg(0xF6, 'inc', 'zpx', 2)
    _reg(0xEE, 'inc', 'abs', 3); _reg(0xFE, 'inc', 'abx', 3)
    _reg(0x24, 'bit', 'zp', 2); _reg(0x2C, 'bit', 'abs', 3)
    _reg(0x00, 'brk', 'imp', 1); _reg(0x40, 'rti', 'imp', 1)
    _reg(0x60, 'rts', 'imp', 1); _reg(0x4C, 'jmp', 'abs', 3)
    _reg(0x6C, 'jmp', 'ind', 3); _reg(0x20, 'jsr', 'abs', 3)
    for op, mn in ((0x08, 'php'), (0x28, 'plp'), (0x48, 'pha'), (0x68, 'pla')):
        _reg(op, mn, 'imp', 1)
    for i, mn in enumerate(('bpl', 'bmi', 'bvc', 'bvs', 'bcc', 'bcs', 'bne', 'beq')):
        _reg(0x10 + 0x20 * i, mn, 'rel', 2)
    for off, mode in ((0xC0, 'imm'), (0xC4, 'zp'), (0xCC, 'abs'),
                      (0xE0, 'imm'), (0xE4, 'zp'), (0xEC, 'abs')):
        _reg(off, 'cpy' if off & 0xF0 == 0xC0 else 'cpx', mode,
             3 if mode == 'abs' else 2)
    for op, mn in ((0x18, 'clc'), (0x38, 'sec'), (0x58, 'cli'), (0x78, 'sei'),
                   (0xB8, 'clv'), (0xD8, 'cld'), (0xF8, 'sed'),
                   (0xAA, 'tax'), (0xA8, 'tay'), (0x8A, 'txa'), (0x98, 'tya'),
                   (0xBA, 'tsx'), (0x9A, 'txs'), (0x88, 'dey'), (0xCA, 'dex'),
                   (0xC8, 'iny'), (0xE8, 'inx'), (0xEA, 'nop')):
        _reg(op, mn, 'imp', 1)
    _reg(0x86, 'stx', 'zp', 2); _reg(0x8E, 'stx', 'abs', 3); _reg(0x96, 'stx', 'zpy', 2)
    _reg(0x84, 'sty', 'zp', 2); _reg(0x8C, 'sty', 'abs', 3); _reg(0x94, 'sty', 'zpx', 2)
    for op, mode in ((0x81, 'izx'), (0x85, 'zp'), (0x8D, 'abs'), (0x91, 'izy'),
                     (0x95, 'zpx'), (0x99, 'aby'), (0x9D, 'abx')):
        _reg(op, 'sta', mode, 3 if mode in ('abs', 'abx', 'aby') else 2)
    _reg(0xA2, 'ldx', 'imm', 2); _reg(0xA6, 'ldx', 'zp', 2); _reg(0xAE, 'ldx', 'abs', 3)
    _reg(0xB6, 'ldx', 'zpy', 2); _reg(0xBE, 'ldx', 'aby', 3)
    for off, mode in ((0xA0, 'imm'), (0xA4, 'zp'), (0xAC, 'abs'),
                      (0xB4, 'zpx'), (0xBC, 'abx')):
        _reg(off, 'ldy', mode, 3 if mode in ('abs', 'abx') else 2)


_fill_table()
assert len(TBL) == 151  # official NMOS 6502 full set
for probe in ((0x4C, 'jmp', 'abs'), (0x20, 'jsr', 'abs'), (0xEA, 'nop', 'imp'),
              (0xA9, 'lda', 'imm'), (0x91, 'sta', 'izy'), (0x6C, 'jmp', 'ind'),
              (0x0A, 'asl', 'acc'), (0xD0, 'bne', 'rel'), (0xFD, 'sbc', 'abx'),
              (0x40, 'rti', 'imp'), (0x60, 'rts', 'imp'), (0xE0, 'cpx', 'imm'),
              (0xCC, 'cpy', 'abs'), (0xC9, 'cmp', 'imm'), (0xE9, 'sbc', 'imm'),
              (0xB0, 'bcs', 'rel'), (0x99, 'sta', 'aby'), (0x86, 'stx', 'zp')):
    got = TBL[probe[0]]
    assert (got[0], got[1]) == probe[1:], (got, probe)

MODE_SIZE = {'imp': 1, 'acc': 1, 'imm': 2, 'zp': 2, 'zpx': 2, 'zpy': 2,
             'izx': 2, 'izy': 2, 'abs': 3, 'abx': 3, 'aby': 3, 'ind': 3,
             'rel': 2}

# --- I/O address annotations (readability only; never affects bytes) ----------
IO_NOTE = {
    0x2000: 'PPUCTRL', 0x2001: 'PPUMASK', 0x2002: 'PPUSTATUS',
    0x2003: 'OAMADDR', 0x2004: 'OAMDATA', 0x2005: 'PPUSCROLL',
    0x2006: 'PPUADDR', 0x2007: 'PPUDATA', 0x4014: 'OAMDMA',
    0x4015: 'APUSTATUS', 0x4016: 'JOY1', 0x4017: 'JOY2/APUFRAME',
}

# --- FME-7 default window banks (written by the reset code at $E337) ---------
# cmd $08..$0B place PRG banks into $8000/$A000/$C000/$6000 respectively.
DEFAULT_WINDOWS = {0x8000: 11, 0xA000: 12, 0xC000: 13, 0x6000: 14}

# Runtime windows observed in the captured traces: the game re-maps its windows
# per scene, so one window hosts different banks at different times. The
# captured NMI shadow RAM $E8-$EB = C0 18 1D 1E says gameplay runs
# $8000=bank 0, $A000=bank 8, $C000=bank 13, $6000=bank 14, and bank 14 runs at
# $C000 in another scene. A static far call cannot know which scene it belongs
# to, so each window resolves to *every* bank ever seen there; seeding a
# superset can only add decoded code, never remove any (Descent is monotone in
# its seeds).
OBSERVED_WINDOW_BANKS = {0x8000: (0,), 0xA000: (8,), 0xC000: (13, 14), 0x6000: (14,)}

# `disasm/gen/observed_code.py` (written by tools/emu/observed-code.py) is
# generated, so guard the import: it is absent on a clean checkout.
#
# `OBSERVED_SEEDS` are the emulator-observed program counters, wired in as entry
# seeds. The earlier "seeding observed execution collapses coverage" negative
# result was an artefact of the harvest reading the FME-7 window map backwards
# and using the frame's final window state (MAP.md §11 re-tests it). With the
# corrected map and per-write state they are clean (`check_observed.py`: 0
# mid-instruction PCs) and add code to banks 0D/0E. Seeding banks 02/0A from
# them alone shrank their listings (the auto-tiler and the sound driver fell out
# as data), so they only extend banks that already have an entry. `OBSERVED_WINDOWS` (bank -> naming window) is not the far-call map and is
# only used when its keys are CPU window bases.
try:
    from observed_code import SEEDS as OBSERVED_SEEDS
    from observed_code import WINDOWS as OBSERVED_WINDOWS
except ImportError:  # pragma: no cover
    OBSERVED_SEEDS, OBSERVED_WINDOWS = {}, {}


# --- proven data tables: emitted as labelled, grouped `.byte` blocks ----------
# (bank, offset) -> (byte length, label, note)
TABLES = {
    (0x0F, 0x076): (8, 'tbl_e076', '$E001 state dispatcher: 4 x 16-bit handlers (index $D4*2)'),
    (0x0F, 0x2CA): (28, 'tbl_irq_split', 'IRQ mid-frame split: 16-bit handlers, index $D3*2 (first 8 reached)'),
    (0x00, 0x000): (96, 'tbl_8000_ptr', 'bank 00 pointer table: 48 x 16-bit (window $8000)'),
    (0x0A, 0xBD1): (96, 'tbl_song_ptr', 'sound driver song table: 48 x 16-bit, bank 10 @ $8000'),
    (0x0C, 0x1BEB): (0x4A, 'tbl_9beb_ppulist', 'PPU write list: [hi,lo,count,bytes..]x8, $FF-terminated'),
    (0x02, 0x11B7): (0x4C, 'tbl_91b7_ppulist', 'nametable write list used by the $8F2B level loader, $FF-terminated'),
}


def branch_target(data, pc):
    """Branch target as offset relative to the bank base."""
    off = data[pc + 1] - 256 if data[pc + 1] > 127 else data[pc + 1]
    return pc + 2 + off


def abs32(data, pc):
    return data[pc + 1] | (data[pc + 2] << 8)


def decode(data, pc):
    return TBL.get(data[pc])


# --- runtime window used for label naming ("run" space in rotj.cfg) ----------
# Derived from each bank's own in-window absolute references (self-JMP/JSR
# targets that land on instruction boundaries inside the same bank).  This is
# a *naming* choice only: the FME-7 swap actually places each bank wherever the
# stage tables say.  Banks 11/12/13/14 are the reset defaults ($8000/$A000/
# $C000/$6000); the module banks that get swapped in mostly assume $8000.
REAL_WINDOWS = {
    0x00: 0x8000, 0x01: 0xA000, 0x02: 0x8000, 0x03: 0x8000,
    0x04: 0x8000, 0x05: 0x8000, 0x06: 0x8000, 0x07: 0xA000,
    0x08: 0x8000, 0x09: 0x8000, 0x0A: 0x8000, 0x0B: 0x8000,
    0x0C: 0x8000, 0x0D: 0xA000, 0x0E: 0xC000, 0x0F: 0xE000,
}


def window_banks():
    """CPU window base -> every PRG bank that can be mapped there at runtime.

    The reset defaults, the map recorded from the captured NMI shadow, and (when
    `observed_code.WINDOWS` is a window -> bank map) the generated file are all
    unioned: a static far call cannot tell which scene it is in, so every bank
    ever seen in a window is seeded.
    """
    banks = {w: [] for w in (0x6000, 0x8000, 0xA000, 0xC000)}

    def add(w, b):
        if w in banks and b not in banks[w]:
            banks[w].append(b)

    for w, b in DEFAULT_WINDOWS.items():
        add(w, b)
    for w, bs in OBSERVED_WINDOW_BANKS.items():
        for b in bs:
            add(w, b)
    # The committed observed_code.py instead carries bank -> window (the
    # generator's naming convention, not the runtime map), so only use it when
    # its keys really are CPU window bases.
    if OBSERVED_WINDOWS and all(w >= 0x6000 for w in OBSERVED_WINDOWS):
        for w, b in OBSERVED_WINDOWS.items():
            add(w, b)
    return {w: tuple(v) for w, v in banks.items()}


def style_windows():
    return dict(REAL_WINDOWS)


# --- indirect-jump dispatch tables -------------------------------------------
# The game dispatches per actor/state through a fixed idiom:
#
#     LDA <index>,x     ; index in RAM ($7DD0/$7D50/$7CC0/$D4/...)
#     ASL; TAY          ; 16-bit table index
#     LDA <table>,y     ; pointer low
#     STA <zp>
#     LDA <table+1>,y   ; pointer high
#     STA <zp+1>
#     JMP (<zp>)
#
# Its last thirteen bytes are the constant shape `B9 lo hi 85 zz B9 lo+1 hi 85
# zz+1 6C zz 00` (two absolute,Y loads of adjacent table bytes, two zero-page
# stores into adjacent addresses and `JMP (zp)` through the first of them).  That
# signature is specific enough to find the dispatch sites in raw bank bytes --
# they often sit in code the descent has not reached yet -- and the table is
# always in the same window as the site, so anchors are read from this bank.
# A `PHA/PHA/RTS` jump-table form does not occur (only one incidental `PHA;RTS`
# pair exists, bank 3 offset $171F, and it is not a dispatch).
DISPATCH_TAIL = 13
DISPATCH_CLEAN = 3  # instructions an entry must decode as, to count as code


def decodes_cleanly(prg, bank, off, count):
    """True if `count` instructions decode from `off` without undefined opcodes."""
    data = prg[bank * BANK_SIZE:(bank + 1) * BANK_SIZE]
    for _ in range(count):
        if off >= len(data):
            return False
        ent = decode(data, off)
        if ent is None:
            return False
        off += ent[2]
    return True


def dispatch_targets(prg, bank, window, resolve):
    """Yield (bank, offset, table_addr) handler entries of `bank`'s dispatch tables.

    A table is followed from its anchor while each 16-bit entry lands in
    $8000-$FFFF and its target decodes as code; the first entry that does not is
    where the table ends (the bytes after a table are the next routine's
    instructions, which never decode as a valid pointer).  An entry inside the
    site's own window is a handler in the same bank; an entry outside it is a
    runtime address resolved through the FME-7 window map like any far call.
    """
    data = prg[bank * BANK_SIZE:(bank + 1) * BANK_SIZE]
    for pc in range(len(data) - DISPATCH_TAIL):
        if data[pc] != 0xB9:
            continue
        lo, hi, zp = data[pc + 1], data[pc + 2], data[pc + 4]
        if (data[pc + 3] != 0x85 or data[pc + 5] != 0xB9
                or data[pc + 6] != (lo + 1) & 0xFF or data[pc + 7] != hi
                or data[pc + 8] != 0x85 or data[pc + 9] != (zp + 1) & 0xFF
                or data[pc + 10] != 0x6C or data[pc + 11] != zp
                or data[pc + 12] != 0x00):
            continue
        tbl = hi << 8 | lo
        toff = tbl - window
        if not 0 <= toff < BANK_SIZE:
            continue
        for k in range(256):
            o = toff + 2 * k
            if o + 1 >= len(data):
                break
            target = data[o] | (data[o + 1] << 8)
            if not 0x8000 <= target <= 0xFFFF:
                break
            if window <= target < window + BANK_SIZE:
                candidates = ((bank, target - window),)
            else:
                candidates = resolve(target)
            if not candidates or not all(
                    decodes_cleanly(prg, b, off, DISPATCH_CLEAN)
                    for b, off in candidates):
                break
            for b, off in candidates:
                yield b, off, tbl


class Descent:
    """Recursive descent over one 8 KiB bank; starts/labels/far calls."""

    def __init__(self, data, base):
        self.data = data
        self.base = base
        self.starts = {}      # off -> (mn, mode, n)
        self.seed_names = {}  # off -> name
        self.jsr_targets = set()
        self.other_targets = set()
        self.far = []         # (off, absolute target)

    def seed(self, off, name=None):
        if name:
            self.seed_names[off] = name
        self._walk(off)

    def _walk(self, seed):
        data = self.data
        base = self.base
        stack = [seed]
        while stack:
            pc = stack.pop()
            while 0 <= pc < len(data):
                if pc in self.starts:
                    break
                ent = decode(data, pc)
                if ent is None:
                    break
                mn, mode, n = ent
                if pc + n > len(data):
                    break
                self.starts[pc] = ent
                if mode == 'rel':
                    tgt = branch_target(data, pc)
                    if 0 <= tgt < len(data):
                        self.other_targets.add(tgt)
                        stack.append(tgt)
                    pc += n
                    continue
                if mn in ('jsr', 'jmp') and mode == 'abs':
                    a = abs32(data, pc)
                    if base <= a < base + len(data):
                        tgt = a - base
                        if mn == 'jsr':
                            self.jsr_targets.add(tgt)
                            stack.append(tgt)
                            pc += n
                            continue
                        self.other_targets.add(tgt)
                        pc = tgt
                        continue
                    self.far.append((pc, a))
                    if mn == 'jmp':
                        break
                    pc += n
                    continue
                if mn == 'jmp' and mode == 'ind':
                    break
                if mn in ('rts', 'rti', 'brk'):
                    break
                pc += n

    def labels(self, extra=None):
        lab = {}
        for off in sorted(self.other_targets | self.jsr_targets):
            lab[off] = ('sub_%04X' % (self.base + off) if off in self.jsr_targets
                        else 'loc_%04X' % (self.base + off))
        lab.update(self.seed_names)
        if extra:
            lab.update(extra)
        return lab


def far_hazard(mn, mode, data, pc, window):
    """Hazard comment for JSR/JMP that leave this bank window (bytes untouched)."""
    if mn == 'jsr' and mode == 'abs':
        a = abs32(data, pc)
        if not (window[0] <= a <= window[1]):
            return ' ; far call to CPU ${:04X}'.format(a)
    if mn == 'jmp' and mode in ('abs', 'ind'):
        a = abs32(data, pc)
        if mode == 'ind':
            return ' ; indirect jmp through ${:04X}, target unknown at build'.format(a)
        if not (window[0] <= a <= window[1]):
            return ' ; far jump to CPU ${:04X}'.format(a)
    return None


def fmt_operand(data, pc, base, window, labels, mode):
    if mode in ('imp', 'acc'):
        return ''
    if mode == 'imm':
        return '#${:02X}'.format(data[pc + 1])
    if mode == 'zp':
        return '${:02X}'.format(data[pc + 1])
    if mode in ('zpx', 'zpy'):
        return '${:02X}, {}'.format(data[pc + 1], 'x' if mode == 'zpx' else 'y')
    if mode == 'izx':
        return '(${:02X}, x)'.format(data[pc + 1])
    if mode == 'izy':
        return '(${:02X}), y'.format(data[pc + 1])
    if mode == 'rel':
        off = branch_target(data, pc)
        return labels.get(off, '${:04X}'.format(base + off))
    a = abs32(data, pc)
    if mode == 'ind':
        return '(${:04X})'.format(a)  # unambiguous: jmp has one indirect form
    name = labels.get(a - base) if window[0] <= a <= window[1] else None
    arg = name or 'a:${:04X}'.format(a)
    return arg + (', x' if mode == 'abx' else ', y' if mode == 'aby' else '')


def io_comment(mn, mode, data, pc):
    """Short trailing comment for a recognized I/O access (readability only)."""
    if mode != 'abs' or mn not in ('sta', 'stx', 'sty', 'lda', 'ldx', 'ldy'):
        return None
    a = abs32(data, pc)
    if a in IO_NOTE:
        return ' ; {}'.format(IO_NOTE[a])
    if a in (0x8000, 0xA000):
        return ' ; FME-7 {}'.format('command' if a == 0x8000 else 'parameter')
    return None


def emit_bank(data, bank, base, is_fixed, seeds):
    """Emit the code/data lines of one bank."""
    lines = []
    add = lines.append
    window = (base, base + len(data) - 1)
    limit = len(data) - 6 if is_fixed else len(data)

    if seeds:
        d = Descent(data, base)
        for off, name in seeds:
            if 0 <= off < limit:
                d.seed(off, name)
        starts = d.starts
        labels = d.labels()
        if is_fixed:
            end = len(data) - 6
            for i, nm in enumerate(VEC_NAMES):
                a = data[end + 2 * i] | (data[end + 2 * i + 1] << 8)
                if window[0] <= a <= window[1]:
                    labels[a - base] = nm
    else:
        # historical linear sweep (bank with no known entry point)
        starts = {}
        labels = {}
        pc = 0
        while pc < limit:
            ent = decode(data, pc)
            if ent is None:
                pc += 1
                continue
            mn, mode, n = ent
            if mn == 'brk':
                pc += 1
                continue
            if mode == 'rel':
                if pc + 2 >= limit:
                    pc += 1
                    continue
                a = base + branch_target(data, pc)
            elif mode == 'abs':
                if pc + 3 > limit:
                    pc += 1
                    continue
                a = abs32(data, pc)
            else:
                pc += 1 if mode in ('imp', 'acc') else n
                continue
            if window[0] <= a <= window[1] and a - base not in labels:
                labels[a - base] = 'L{:04X}'.format(a)
            pc += 1 if mode in ('imp', 'acc') else n
        pc = 0
        while pc < limit:
            ent = decode(data, pc)
            if ent is None:
                pc += 1
                continue
            starts[pc] = ent
            pc += 1 if ent[1] in ('imp', 'acc') else ent[2]

    for (tb, toff), (tlen, _, _) in TABLES.items():
        if tb == bank:
            for o in range(toff, toff + tlen):
                labels.pop(o, None)
    rels = sorted(set(starts) | set(labels))

    def is_clean_run(off, n):
        if off + n > limit:
            return False
        for k in range(off + 1, off + n):
            if k in starts or k in labels:
                return False
        return True

    pc = 0
    rel_idx = 0
    while pc < limit:
        while rel_idx < len(rels) and rels[rel_idx] < pc:
            rel_idx += 1  # labels swallowed by a data table are dropped
        tab = TABLES.get((bank, pc))
        if tab is not None and pc + tab[0] <= limit:
            length, lab, note = tab
            add('{}:'.format(lab))
            add('; {}'.format(note))
            for k in range(0, length, 8):
                add('.byte ' + ', '.join(
                    '${:02X}'.format(b) for b in data[pc + k:pc + min(k + 8, length)]))
            pc += length
            continue
        if rel_idx < len(rels) and rels[rel_idx] == pc:
            while rel_idx < len(rels) and rels[rel_idx] == pc:
                lab = labels.get(pc)
                if lab:
                    add('{}:'.format(lab))
                rel_idx += 1
        ent = starts.get(pc)
        if ent is not None:
            mn, mode = ent[:2]
            if mn == 'brk':
                run_end = pc + 1
                while (run_end < limit and data[run_end] == 0x00
                       and run_end not in labels and run_end not in starts):
                    run_end += 1
                if run_end - pc >= 2:
                    chunk = data[pc:min(pc + 8, run_end)]
                    add('.byte ' + ', '.join('${:02X}'.format(b) for b in chunk))
                    pc = min(pc + 8, run_end)
                    continue
                add('brk')
                pc += 1
                continue
            n = MODE_SIZE[mode]
            # a relative branch is only emitted through a real label; otherwise
            # the displacement byte is (misdecoded) data -> emit raw bytes.
            if mode == 'rel' and branch_target(data, pc) not in labels:
                if is_clean_run(pc, n):
                    add('.byte ' + ', '.join('${:02X}'.format(data[pc + k])
                                             for k in range(n)))
                    pc += n
                else:
                    add('.byte ${:02X}'.format(data[pc]))
                    pc += 1
                continue
            if not is_clean_run(pc, n):
                add('.byte ${:02X}'.format(data[pc]))
                pc += 1
                continue
            operand = fmt_operand(data, pc, base, window, labels, mode)
            hz = far_hazard(mn, mode, data, pc, window)
            note = io_comment(mn, mode, data, pc)
            text = '{} {}'.format(mn.upper(), operand).rstrip()
            add(text + (hz or note or ''))
            pc += n
            continue
        # data byte (unreached or undecodable)
        run = 1
        while (pc + run < limit and starts.get(pc + run) is None
               and (pc + run) not in labels and data[pc + run] == data[pc]):
            run += 1
        if run >= 3:
            take = min(run, 8)
            add('.byte ' + ', '.join('${:02X}'.format(data[pc]) for _ in range(take))
                + (' ; x{}'.format(run) if run > 8 else ''))
            pc += take
        else:
            add('.byte ${:02X}'.format(data[pc]))
            pc += 1

    if is_fixed and limit == len(data) - 6:
        names = []
        for i, _nm in enumerate(VEC_NAMES):
            v = data[limit + 2 * i] | (data[limit + 2 * i + 1] << 8)
            if v - base == limit and not any(
                    '{}:'.format(VEC_NAMES[i]) == x for x in lines):
                add('{}:'.format(VEC_NAMES[i]))
            names.append(labels.get(v - base, '${:04X}'.format(v)))
        add('.word {}, {}, {}'.format(*names))
    return lines


def write_header_s(header_bytes, prg_banks, chr_banks, mapper):
    with open(os.path.join(OUT, 'header.s'), 'w') as f:
        f.write('; iNES header, byte-exact copy of the source ROM\'s first 16 bytes\n')
        f.write('; mapper %d, PRG 0x%02X x 16 KiB, CHR 0x%02X x 8 KiB, horizontal mirroring\n\n'
                % (mapper, prg_banks, chr_banks))
        f.write('.segment "HEADER"\n.byte '
                + ', '.join('${:02X}'.format(b) for b in header_bytes) + '\n\n')


# The WRAM payload bank 2 $99A9 copies from CHR (docs/SUBLEVELS.md 7d-i): WRAM
# $7E60-$7FFF is CHR $3DE60-$3DFFF and $6000-$7E5F is CHR $3E000-$3FE5F, so
# cutscene.s covers CHR $3DE60-$3FE5F as code assembled at its WRAM addresses.
PAYLOAD_CHR = 0x3DE60
PAYLOAD_TAIL = 0x1A0
PAYLOAD_SEEDS = ((0x0000, 'cutscene_entry'), (0x0135, 'stage_end_entry'))


def write_chr_s(rom):
    chr_off = HEADER_SIZE + PRG_SIZE
    chr_len = len(rom) - chr_off
    rel = os.path.relpath(ROM, OUT)
    after = PAYLOAD_CHR + 0x2000
    with open(os.path.join(OUT, 'chr.s'), 'w') as f:
        f.write('; all %d bytes of CHR-ROM, .incbin straight from the source ROM\n' % chr_len)
        f.write('; (offset %d, length %d) except the WRAM payload at CHR $%05X-$%05X,\n'
                % (chr_off, chr_len, PAYLOAD_CHR, after - 1))
        f.write('; listed in cutscene.s.\n')
        f.write('; FME-7 maps CHR in eight 1 KiB windows (registers $0-$7); the game\n')
        f.write('; swaps them per video band for fake parallax (see MAP.md).\n\n')
        f.write('.segment "CHR"\n')
        f.write('.incbin "%s", %d, %d\n' % (rel, chr_off, PAYLOAD_CHR))
        f.write('.include "cutscene.s"\n')
        f.write('.incbin "%s", %d, %d\n\n' % (rel, chr_off + after, chr_len - after))


def write_cutscene_s(rom):
    chr_ = rom[HEADER_SIZE + PRG_SIZE:]
    tail = chr_[PAYLOAD_CHR:PAYLOAD_CHR + PAYLOAD_TAIL]
    main_ = chr_[PAYLOAD_CHR + PAYLOAD_TAIL:PAYLOAD_CHR + 0x2000]
    image = main_ + tail
    lines = emit_bank(image, None, 0x6000, False, list(PAYLOAD_SEEDS))
    split = lines.index('sub_7E60:')
    with open(os.path.join(OUT, 'cutscene.s'), 'w') as f:
        f.write('; The WRAM payload: CHR $%05X-$%05X, copied to WRAM by bank 2 $99A9\n'
                % (PAYLOAD_CHR, PAYLOAD_CHR + 0x1FFF))
        f.write('; (CHR banks $F8-$FF to $6000-$7E5F, bank $F7 from $260 to $7E60-$7FFF)\n')
        f.write('; and entered at $6000 (the banner cutscene, $98BD) or $6135 (the\n')
        f.write('; stage end, $999C). Included by chr.s; assembled at its WRAM addresses.\n')
        f.write('; A JSR $C0C6 after CLC takes a two-byte inline pointer (listed as CPY).\n\n')
        f.write('.org $7E60\n')
        for line in lines[split:]:
            f.write(line + '\n')
        f.write('\n.org $6000\n')
        for line in lines[:split]:
            f.write(line + '\n')
        f.write('.reloc\n')


def write_rotj_cfg(windows, chr_len):
    L = []
    a = L.append
    a('# ld65 config for disasm/assemble.sh (see disasm/README.md)')
    a('# HDR + LOAD0..15 + CHR lay the output file out byte-for-byte: 16-byte')
    a('# header, 16 x 8 KiB PRG banks in ROM order, then 256 KiB CHR.')
    a('# PRG0..15 are run-space only ("file ="): they give banks CPU-window')
    a('# labels; bank 15 sits at $E000, the FME-7 fixed window. Others use')
    a('# bank %% 3 rotation over $8000/$A000/$C000 (documented convention).')
    a('MEMORY {')
    a('    HDR: file = %O, start = $0000, size = $0010, fill = yes, fillval = $00;')
    for i in range(PRG_BANKS):
        a('    PRG%d:  file = "",  start = $%04X, size = $2000; # run-space, no file bytes'
          % (i, windows[i]))
    for i in range(PRG_BANKS):
        a('    LOAD%d: file = %%O, start = $%05X, size = $2000, fill = yes, fillval = $00; # PRG bank 0x%02X'
          % (i, HEADER_SIZE + i * BANK_SIZE, i))
    a('    CHR: file = %%O, start = $20010, size = $40000, fill = yes, fillval = $00; # %d KiB CHR'
      % (chr_len // 1024))
    a('}')
    a('')
    a('SEGMENTS {')
    a('    HEADER: load = HDR;')
    for i in range(PRG_BANKS):
        a('    PRG%d:   load = LOAD%d, run = PRG%d;' % (i, i, i))
    a('    CHR:    load = CHR;')
    a('}')
    with open(os.path.join(OUT, 'rotj.cfg'), 'w') as f:
        f.write('\n'.join(L) + '\n')


def collect_entries(prg, observed=None):
    """Derive per-bank entry seeds, iterated to a fixed point.

    Fixed bank 15: reset/NMI/IRQ plus the targets of two literal 16-bit jump
    tables in the fixed bank (the $E076 object dispatch and the $E2CA IRQ
    split dispatch).  Every other bank is seeded from the far-call targets of
    already-seeded banks, resolved through `window_banks()`: an absolute target
    inside a switchable window seeds *every* bank observed there (the FME-7
    places banks per scene, so the target's bank is ambiguous).  `observed`
    (default `observed_code.SEEDS`, the emulator PC harvest; pass `{}` for the
    unobserved baseline) adds those program counters as entry seeds too.

    Seeding order cannot matter.  Each pass re-walks every seeded bank and adds
    whatever far targets it finds, repeating until a pass adds no seed and no
    bank's instruction count grows; the loop is bounded to MAX_DESCENT_PASSES.
    A bank with no seed keeps the linear-sweep fallback listing.
    """
    if observed is None:
        observed = OBSERVED_SEEDS

    def resolve(a, wb):
        if 0xE000 <= a <= 0xFFFF:
            return ((15, a - 0xE000),)
        w = (a // 0x2000) * 0x2000
        return tuple((b, a - w) for b in wb.get(w, ()))

    def clean_entry(bank, off, n=6):
        """True if `n` instructions decode from off without undefined opcodes."""
        return decodes_cleanly(prg, bank, off, n)

    wb = window_banks()
    windows = style_windows()
    seeds = {i: [] for i in range(PRG_BANKS)}
    seen = {i: set() for i in range(PRG_BANKS)}

    def add(bank, off, name):
        if not 0 <= off < BANK_SIZE or off in seen[bank]:
            return False
        seen[bank].add(off)
        seeds[bank].append((off, name))
        return True

    add(15, 0x2E6, 'reset_entry')
    add(15, 0x1D0, 'nmi_entry')
    add(15, 0x2A9, 'irq_entry')
    # $E076: 4-entry pointer table used by the $E001 state dispatcher.
    for off in (0x012, 0x02C, 0x046, 0x057):
        add(15, off, 'sub_%04X' % (0xE000 + off))
    # $E2CA: IRQ split dispatch table; some slots are unused ($FF) or mid-code,
    # so only seed the entries that decode cleanly.
    b15 = prg[15 * BANK_SIZE:16 * BANK_SIZE]
    for i in range(14):
        a = b15[0x2CA + 2 * i] | (b15[0x2CB + 2 * i] << 8)
        for b, off in resolve(a, wb):
            if clean_entry(b, off):
                add(b, off, 'irq_%04X' % a)
    counts = {i: 0 for i in range(PRG_BANKS)}
    for _pass in range(MAX_DESCENT_PASSES):
        settled = True
        for bank in range(PRG_BANKS):
            if not seeds[bank]:
                continue
            # Observed register-write PCs reach only part of a bank, so they
            # extend a bank the static seeds already reach; a bank they alone
            # would seed keeps its fuller sweep listing (MAP.md §11).
            for off, _reason in observed.get(bank, ()):
                add(bank, off, 'obs_%04X' % (windows[bank] + off))
            d = Descent(prg[bank * BANK_SIZE:(bank + 1) * BANK_SIZE],
                        windows[bank])
            for off, name in seeds[bank]:
                d.seed(off, name)
            if len(d.starts) != counts[bank]:
                counts[bank] = len(d.starts)
                settled = False
            for _pc, a in d.far:
                for b, off in resolve(a, wb):
                    if add(b, off, 'far_%04X' % a):
                        settled = False
            # Indirect-jump dispatch tables in this bank's raw bytes: their
            # handler entries are code starts the plain descent cannot see.
            for b, off, tbl in dispatch_targets(prg, bank, windows[bank],
                                                 lambda a: resolve(a, wb)):
                if add(b, off, 'tbl_%04X_%04X' % (tbl, windows[b] + off)):
                    settled = False
        if settled:
            break
    else:
        raise AssertionError(
            'far-call descent did not settle in %d passes' % MAX_DESCENT_PASSES)
    # self-check: the idiom detector must have followed the known dispatch tables
    # (bank 13 $ABA8 -> $AB5A, bank 15 $E1A1 -> $E17C); a silent regression in the
    # byte shape would otherwise only show up as lost coverage.
    assert 0x0B5A in seen[13] and 0x017C in seen[15]
    return seeds


def main():
    rom = open(ROM, 'rb').read()
    header = rom[:HEADER_SIZE]
    prg = rom[HEADER_SIZE:HEADER_SIZE + PRG_SIZE]
    chr_len = len(rom) - (HEADER_SIZE + PRG_SIZE)
    assert len(prg) == PRG_SIZE
    mapper = (header[6] >> 4) | (header[7] & 0xF0)
    windows = style_windows()
    seeds = collect_entries(prg)
    write_header_s(header, header[4], header[5], mapper)
    write_chr_s(rom)
    write_cutscene_s(rom)
    write_rotj_cfg(windows, chr_len)
    for i in range(PRG_BANKS):
        base = windows[i]
        data = prg[i * BANK_SIZE:(i + 1) * BANK_SIZE]
        with open(os.path.join(OUT, 'bank%02X.s' % i), 'w') as f:
            f.write('; Batman: Return of the Joker (USA), PRG bank 0x%02X (8 KiB)\n' % i)
            f.write("; CPU window convention (rotj.cfg 'run'): $%04X-$%04X\n"
                    % (base, base + BANK_SIZE - 1))
            if i == 15:
                f.write('; FME-7 fixed bank: has NMI/RESET/IRQ entries + $FFFA vectors.\n')
            if seeds[i]:
                f.write('; recursive-descent pass: %d entry point(s); unreached bytes are data.\n'
                        % len(seeds[i]))
            else:
                f.write('; no known entry point; linear-sweep fallback listing.\n')
            f.write('\n.segment "PRG%d"\n\n' % i)
            for line in emit_bank(data, i, base, i == 15, seeds[i]):
                f.write(line + '\n')
            f.write('\n')
    print('generated 16 banks; ROM sha1 =', hashlib.sha1(rom).hexdigest())


if __name__ == '__main__':
    main()
