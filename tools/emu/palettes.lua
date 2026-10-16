-- FCEUX Lua: trace palette-RAM writes at level start, with program counters.
--
-- LEVEL=<n> forces the level index $20 at CPU $D3D8 (the game's own selector,
-- `LEV_AREA` / `probe.lua`), so the level loads through the game's own table.
-- Every $2006/$2007 write is followed; when the PPU address register lands on
-- $3F00-$3F1F the $2007 writes are logged with their PC (frame,index,value,pc).
-- Also logs each nested JSR: the executor that pushes PPU write-lists lives in
-- bank 14; the PC at the $2007 store tells us which code path wrote palette RAM.
--
-- Run (absolute paths):
--   DUMP_DIR="$PWD/tools/emu/palettes-trace/lvl-0" LEVEL=0 INPUT=idle \
--     RAMSEQ=off UPTIME=1400 \
--     timeout -k 3 600 fceux --sound 0 --loadlua "$PWD/tools/emu/palettes-trace/trace.lua" \
--       "$PWD/raw/Batman - Return of the Joker (USA).nes"
local DIR = os.getenv("DUMP_DIR") or "tools/emu/palettes-trace"
local LEVEL = os.getenv("LEVEL") or "0"
local UPTIME = tonumber(os.getenv("UPTIME") or "1400")
os.execute("mkdir -p '" .. DIR .. "'")
pcall(function() emu.speedmode("maximum") end)

local function byte(a) return memory.readbyte(a) end

local csv = assert(io.open(DIR .. "/palettetrace.csv", "w"))
csv:write("frame,kind,index,value,pc\n")

-- PPU address register latch: first write sets the high byte, second the low.
local addr = 0
local hi = true
memory.registerwrite(0x2006, function(...)
    local args = { ... }
    local v = args[3]
    if type(v) ~= "number" then v = memory.getregister("A") end
    if hi then
        addr = (v % 0x40) * 0x100
        hi = false
        if v % 0x40 == 0x3F then
            csv:write(string.format("%d,addr,%04X,%02X,%04X\n", emu.framecount(), addr, v, memory.getregister("PC")))
            csv:flush()
        end
    else
        addr = addr + v
        hi = true
    end
end)

memory.registerwrite(0x2007, function(...)
    local args = { ... }
    local v = args[3]
    if type(v) ~= "number" then v = memory.getregister("A") end
    if addr >= 0x3F00 and addr < 0x3F20 then
        csv:write(string.format("%d,data,%02X,%02X,%04X\n",
            emu.framecount(), addr - 0x3F00, v, memory.getregister("PC")))
        csv:flush()
    end
    addr = (addr + 1) % 0x4000
end)

-- Force the level index at the game's own selector, before `LDX $20` reads it.
memory.registerexec(0xD3D8, function()
    if LEVEL ~= "off" then memory.writebyte(0x20, tonumber(LEVEL) % 256) end
    csv:write(string.format("%d,d3d8,00,%02X,%04X\n",
        emu.framecount(), byte(0x20), memory.getregister("PC")))
    csv:flush()
end)

-- Log the palette builders' arguments: far_C367 (X=$33, Y=$34 selectors),
-- far_C30A (A=brightness $32, X=mask $30), sub_C35A.
for _, addr in ipairs({ 0xC35A, 0xC367, 0xC30A }) do
    memory.registerexec(addr, function()
        local sp = memory.getregister("S")
        local ret = memory.readbyte(0x100 + sp + 1) + memory.readbyte(0x100 + sp + 2) * 256
        csv:write(string.format("%d,exec_%04X,20=%02X,0400=%02X,%02X,%02X,ret=%04X\n",
            emu.framecount(), addr, byte(0x20), byte(0x0400),
            memory.getregister("X"), memory.getregister("Y"), ret))
        csv:flush()
    end)
end

-- The palette-record pointer at the moment far_C367 is called from bank 13
-- `$B44D`: $27/$28 point at the per-(area,stage) record.
memory.registerexec(0xB44D, function()
    csv:write(string.format("%d,b44d,rec=%02X%02X,x=%02X,y=%02X,mapper=%02X%02X%02X%02X\n",
        emu.framecount(), byte(0x28), byte(0x27),
        memory.getregister("X"), memory.getregister("Y"),
        byte(0xE8), byte(0xE9), byte(0xEA), byte(0xEB)))
    csv:flush()
end)

for frame = 0, UPTIME do
    local keys = {}
    if frame >= 300 and frame <= 306 then keys.start = true end
    joypad.set(1, keys)
    emu.frameadvance()
end
csv:close()
emu.print("palette trace complete: " .. DIR)
emu.exit()
