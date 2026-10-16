-- FCEUX Lua: what does a write callback actually receive?
--
-- Writes to $8000/$A000 are writes into ROM space (the mapper snoops them), and
-- fceux may pass different arguments than it does for RAM/RAM-mapped IO. Log
-- every argument plus the accumulator for the first few writes.
--
--   DUMP_DIR="$PWD/tools/emu" UPTIME=340 \
--   timeout -k 5 300 fceux --sound 0 --loadlua "$PWD/tools/emu/mapper-probe.lua" \
--     "$PWD/raw/Batman - Return of the Joker (USA).nes"

local DIR = os.getenv("DUMP_DIR") or "tools/emu"
local UPTIME = tonumber(os.getenv("UPTIME") or "340")
os.execute("mkdir -p '" .. DIR .. "'")
pcall(function() emu.speedmode("maximum") end)

local f = assert(io.open(DIR .. "/mapper-probe.txt", "w"))
local logged = 0

local function probe(addr)
    return function(...)
        if logged >= 24 then
            return
        end
        logged = logged + 1
        local n = select("#", ...)
        local parts = {}
        for i = 1, n do
            local v = select(i, ...)
            parts[#parts + 1] = string.format("arg%d=%s", i, tostring(v))
        end
        f:write(
            string.format(
                "frame %d port $%04X nargs=%d %s A=$%02X X=$%02X\n",
                emu.framecount(),
                addr,
                n,
                table.concat(parts, " "),
                memory.getregister("A"),
                memory.getregister("X")
            )
        )
        f:flush()
    end
end

memory.registerwrite(0x8000, probe(0x8000))
memory.registerwrite(0xA000, probe(0xA000))

while emu.framecount() < UPTIME do
    local keys = {}
    if emu.framecount() >= 300 and emu.framecount() <= 306 then
        keys.start = true
    end
    if emu.framecount() >= 320 then
        keys.right = true
    end
    joypad.set(1, keys)
    emu.frameadvance()
end
f:close()
print("probe done")
