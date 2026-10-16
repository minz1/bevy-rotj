"""Reaching definitions of the 6502 V flag: which instructions can leave V at a routine's RTS."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generate as g

rom = open(g.ROM, 'rb').read()
prg = rom[g.HEADER_SIZE:g.HEADER_SIZE + g.PRG_SIZE]
VSET = {'adc', 'sbc', 'bit', 'clv', 'plp'}
ENTRY = 'IN'


class Flow:
    def __init__(self, mapping, indirect):
        self.mapping = mapping  # window base -> bank
        self.indirect = indirect  # pc -> [targets]
        self.memo = {}
        self.notes = set()

    def byte(self, addr):
        base = addr & 0xE000
        bank = self.mapping.get(base)
        if bank is None:
            raise KeyError(f'${addr:04X} in unmapped window')
        return prg[bank * 0x2000 + (addr - base)]

    def ins(self, pc):
        op = self.byte(pc)
        mn, mode, n = g.TBL[op]
        arg = self.byte(pc + 1) | (self.byte(pc + 2) << 8) if n == 3 else (self.byte(pc + 1) if n == 2 else None)
        return mn, mode, n, arg

    def label(self, pc):
        return f'{self.mapping[pc & 0xE000]:02X}:{pc:04X}'

    def summary(self, entry, stop=None):
        """Defs reaching every RTS of the routine at `entry` (ENTRY = V passes through)."""
        key = (entry, stop)
        if key in self.memo:
            if self.memo[key] is None:
                self.notes.add(f'recursion at ${entry:04X}')
                return {ENTRY}
            return self.memo[key]
        self.memo[key] = None
        state = {entry: {ENTRY}}
        work = [entry]
        out = set()
        while work:
            pc = work.pop()
            defs = state[pc]
            if stop is not None and pc == stop:
                out |= defs
                continue
            mn, mode, n, arg = self.ins(pc)
            after = {self.label(pc)} if mn in VSET else set(defs)
            succ = []
            if mn == 'rts':
                out |= after
            elif mn in ('rti', 'brk'):
                self.notes.add(f'{mn} at {self.label(pc)}')
            elif mn == 'jmp' and mode == 'abs':
                succ = [arg]
            elif mn == 'jmp':
                targets = self.indirect.get(pc)
                if targets is None:
                    self.notes.add(f'unresolved jmp ({arg:04X}) at {self.label(pc)}')
                succ = targets or []
            elif mn == 'jsr':
                if arg in (0xC229, 0xE1C0):
                    self.notes.add(f'bank switch call at {self.label(pc)}')
                s = self.summary(arg)
                after = (s - {ENTRY}) | (after if ENTRY in s else set())
                succ = [pc + 3]
            elif mode == 'rel':
                off = self.byte(pc + 1)
                succ = [pc + 2, pc + 2 + (off - 256 if off > 127 else off)]
            else:
                if mn == 'pla' or mn == 'txs':
                    self.notes.add(f'{mn} at {self.label(pc)}')
                succ = [pc + n]
            for s in succ:
                old = state.get(s)
                if old is None or not after <= old:
                    state[s] = (old or set()) | after
                    work.append(s)
        self.memo[key] = out
        self.reach = state
        return out


def table(f, lo, hi, count):
    return [f.byte(lo + i) | (f.byte(hi + i) << 8) for i in range(0, 2 * count, 2)] if hi is None else []


def player_flow():
    f = Flow({0x8000: 4, 0xC000: 14, 0xE000: 15}, {})
    words = lambda lo, n: [f.byte(lo + 2 * i) | f.byte(lo + 2 * i + 1) << 8 for i in range(n)]
    f.indirect[0x84CE] = words(0x96C3, 12)
    f.indirect[0x9A72] = words(0x9A75, 6)
    f.indirect[0x9B71] = words(0x9EB5, 6)
    f.indirect[0x9BCB] = words(0x9EC1, 6)
    return f, words
