-- FCEUX Lua: the superset capture tool. Writes PPU/RAM/WRAM dumps, screenshots,
-- per-frame RAM, the APU register trace, and the mapper and PPU register traces
-- (each with the program counter) into one directory, so every claim can be
-- checked against the same run and `tools/emu/check-frame.py` can tell whether a
-- frame is reproducible at all.
--
-- Dev-only tooling; never ships with the port.
--
-- The FME-7's registers are write-only, so the only way to see the band swap is
-- to watch the writes. Commands go to $8000, their parameters to $A000 (the
-- game writes those two addresses exactly; see disasm/MAP.md §1), and the
-- mapper shadow lives in RAM $E0-$EF, where $E0+n is the shadow for command n —
-- so $EE/$EF hold the IRQ counter, i.e. the split scanline.
--
-- Run (absolute paths: fceux's working directory is not the repo). Do not run
-- this while another windowed app is using the same display.
--   DUMP_DIR="$PWD/tools/emu/mapper" INPUT=drive UPTIME=700 \
--   timeout -k 5 600 fceux --sound 0 --loadlua "$PWD/tools/emu/mapper.lua" \
--     "$PWD/raw/Batman - Return of the Joker (USA).nes"

local DIR = os.getenv("DUMP_DIR") or "tools/emu/mapper"
local UPTIME = tonumber(os.getenv("UPTIME") or "700")
local INPUT = os.getenv("INPUT") or "drive"

os.execute("mkdir -p '" .. DIR .. "'")
pcall(function() emu.speedmode("maximum") end)

local writes = assert(io.open(DIR .. "/mapper-writes.csv", "w"))
writes:write("frame,port,value,cmd,pc,chr88_8f,mapper_e0_ef\n")

-- The last command written to $8000, so each $A000 parameter can be attributed.
local last_cmd = -1

local function hex(from, to)
    local parts = {}
    for i = from, to do
        parts[#parts + 1] = string.format("%02X", memory.readbyte(i))
    end
    return table.concat(parts, " ")
end

local function log_write(port, value)
    writes:write(
        string.format(
            "%d,%04X,%02X,%d,%04X,%s,%s\n",
            emu.framecount(),
            port,
            value,
            last_cmd,
            memory.getregister("PC"),
            hex(0x88, 0x8F),
            hex(0xE0, 0xEF)
        )
    )
end

-- fceux's callback is (address, size, value). Without the value, take it from
-- the register the store at PC-3 used. Reading the register address back
-- returns the bus, not what was written.
local function value_of(...)
    local args = { ... }
    if type(args[3]) == "number" then
        return args[3]
    end
    local op = memory.readbyte((memory.getregister("PC") - 3) % 0x10000)
    if op == 0x8E then
        return memory.getregister("X")
    elseif op == 0x8C then
        return memory.getregister("Y")
    end
    return memory.getregister("A")
end

memory.registerwrite(0x8000, function(...)
    local value = value_of(...)
    -- The FME-7's command is the value written, not the address. fceux's Lua is
    -- 5.1, which has no bitwise operators, so mask with arithmetic.
    last_cmd = value % 16
    log_write(0x8000, value)
end)
memory.registerwrite(0xA000, function(...)
    log_write(0xA000, value_of(...))
end)

-- Frames where nothing is written still matter: they show the shadow state.
local frames = assert(io.open(DIR .. "/mapper-frames.csv", "w"))
frames:write("frame,chr88_8f,mapper_e0_ef\n")

-- Watched RAM addresses: `WATCH="9a,9b,9c,9d,9e,9f"` logs every write to those
-- addresses with the program counter, which is how the code that *decides*
-- something is found (e.g. the routine that zeroes the actor's velocity when he
-- walks into an obstacle).
local WATCH = os.getenv("WATCH") or ""
local watch_f = nil
if WATCH ~= "" then
    watch_f = assert(io.open(DIR .. "/ram-writes.csv", "w"))
    watch_f:write("frame,addr,value,pc,bytes_before_pc\n")
    for token in WATCH:gmatch("[^,]+") do
        local addr = tonumber(token, 16)
        memory.registerwrite(addr, function()
            local pc = memory.getregister("PC")
            -- Bytes around the program counter: the store instruction is in
            -- there, so the code can be aligned without guesswork.
            local hex = {}
            for i = pc - 12, pc + 4 do
                hex[#hex + 1] = string.format("%02X", memory.readbyte(i % 0x10000))
            end
            watch_f:write(
                string.format(
                    "%d,%04X,%02X,%04X,%s\n",
                    emu.framecount(),
                    addr,
                    memory.readbyte(addr),
                    pc,
                    table.concat(hex, " ")
                )
            )
        end)
    end
end

-- PPU register writes with their program counter: this is how the scanline
-- split (the parallax band) is found, since the mapper trace proved the game
-- uses no IRQ. A write to $2005 during the visible frame is a split.
local ppu_f = assert(io.open(DIR .. "/ppu-writes.csv", "w"))
ppu_f:write("frame,addr,value,pc,scanlineish\n")
for addr = 0x2000, 0x2007 do
    memory.registerwrite(addr, function(...)
        ppu_f:write(
            string.format(
                "%d,%04X,%02X,%04X,\n",
                emu.framecount(),
                addr,
                value_of(...),
                memory.getregister("PC")
            )
        )
    end)
end

-- The raster program itself: which IRQ handler runs when, and what the split
-- counter is armed to. `$E012/$E02C/$E046/$E057` and `$E08F/$E0B0` are the
-- handler entries (disasm/MAP.md §3); `$E1A5` arms a split (A = handler index)
-- and `$E1C0` sets the counter (A = low, Y = high). Hooking execution of these
-- in the always-mapped fixed bank yields the per-frame band program in order.
local program = assert(io.open(DIR .. "/raster-program.csv", "w"))
program:write("frame,addr,a,x,y,d0,d3,d4,d5,d6,fc,fd\n")

local function state()
    return string.format(
        "%02X,%02X,%02X,%02X,%02X,%02X,%02X",
        memory.readbyte(0xD0),
        memory.readbyte(0xD3),
        memory.readbyte(0xD4),
        memory.readbyte(0xD5),
        memory.readbyte(0xD6),
        memory.readbyte(0xFC),
        memory.readbyte(0xFD)
    )
end

local function log_exec(addr)
    memory.registerexec(addr, function()
        program:write(
            string.format(
                "%d,%04X,%02X,%02X,%02X,%s\n",
                emu.framecount(),
                addr,
                memory.getregister("A"),
                memory.getregister("X"),
                memory.getregister("Y"),
                state()
            )
        )
    end)
end

for _, addr in ipairs({ 0xE2A9, 0xE1D0, 0xE012, 0xE02C, 0xE046, 0xE057, 0xE08F, 0xE0B0, 0xE1A5, 0xE1C0 }) do
    log_exec(addr)
end


-- Full RAM per frame, concatenated (2048 bytes/frame), so the per-band scroll
-- buffers and the IRQ accumulators can be read alongside the write trace.
local ramseq = assert(io.open(DIR .. "/ramseq.bin", "wb"))
-- Screenshots at chosen frames, to verify a band render against the emulator.
local SHOTS = os.getenv("SHOTS") or ""
local shot_at = {}
for tok in SHOTS:gmatch("[^,]+") do
    shot_at[tonumber(tok)] = true
end

local function drive(frame)
    local keys = {}
    if frame >= 300 and frame <= 306 then
        keys.start = true
    end
    if INPUT == "drive" and frame >= 500 then
        keys.right = true
        if frame % 90 < 3 then
            keys.A = true
        end
    end
    return keys
end

while emu.framecount() < UPTIME do
    local frame = emu.framecount()
    frames:write(string.format("%d,%s\n", frame, hex(0xE0, 0xEF)))
    ramseq:write(memory.readbyterange(0x0000, 0x0800))
    if shot_at[frame] then
        gui.savescreenshotas(string.format("%s/shot-%d.png", DIR, frame))
    end
    joypad.set(1, drive(frame))
    emu.frameadvance()
end

writes:close()
frames:close()
program:close()
ppu_f:close()
if watch_f then watch_f:close() end
ramseq:close()

local counts = {}
local total = 0
for line in io.lines(DIR .. "/mapper-writes.csv") do
    local _, port, value = line:match("^(%d+),(%x+),(%x+)")
    if port then
        local key = port .. ":" .. value
        counts[key] = (counts[key] or 0) + 1
        total = total + 1
    end
end
print(string.format("wrote %d writes over %d frames", total, UPTIME))
local shown = 0
for key, n in pairs(counts) do
    if shown < 12 then
        print(string.format("   port:value %s x%d", key, n))
        shown = shown + 1
    end
end
