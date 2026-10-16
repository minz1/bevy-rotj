-- Probe: does memory.registerexec fire, and does getregister work?
-- Dev-only. Writes to DUMP_DIR/probe-exec.txt, then exits, so nothing is lost
-- to buffering. Also hooks the NMI entry ($E1D0, which runs every frame) to
-- separate "the API is wrong" from "those addresses are never executed".
--
--   DUMP_DIR="$PWD/tools/emu/probe" timeout -k 5 180 fceux --sound 0 \
--     --loadlua "$PWD/tools/emu/probe-exec.lua" "raw/....nes"

local DIR = os.getenv("DUMP_DIR") or "tools/emu/probe"
os.execute("mkdir -p '" .. DIR .. "'")
local out = assert(io.open(DIR .. "/probe-exec.txt", "w"))

pcall(function() emu.speedmode("maximum") end)

local hooks = {
    { addr = 0xE1D0, name = "nmi_entry" },
    { addr = 0xE1A5, name = "arm_split" },
    { addr = 0xE1C0, name = "set_counter" },
    { addr = 0xE012, name = "handler_0" },
    { addr = 0xE02C, name = "handler_1" },
    { addr = 0xE046, name = "handler_2" },
    { addr = 0xE057, name = "handler_3" },
    { addr = 0xE08F, name = "handler_4" },
    { addr = 0xE0B0, name = "handler_5" },
}
local counts = {}
local samples = {}
local getreg_ok, getreg_err = 0, nil

for _, hook in ipairs(hooks) do
    counts[hook.name] = 0
    memory.registerexec(hook.addr, function()
        counts[hook.name] = counts[hook.name] + 1
        if counts[hook.name] == 1 then
            local ok, a = pcall(function()
                return memory.getregister("A")
            end)
            if ok then
                getreg_ok = getreg_ok + 1
                samples[hook.name] = string.format("A=%s", tostring(a))
            else
                getreg_err = tostring(a)
            end
        end
    end)
end

for _ = 1, 700 do
    emu.frameadvance()
end

out:write("hook,count,first_sample\n")
for _, hook in ipairs(hooks) do
    out:write(string.format("%s,%d,%s\n", hook.name, counts[hook.name], samples[hook.name] or ""))
end
out:write(string.format("getregister_ok=%d err=%s\n", getreg_ok, tostring(getreg_err)))
out:close()
emu.exit()
