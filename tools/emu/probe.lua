-- FCEUX Lua capability probe.
--
-- Run: fceux --nogui 1 --sound 0 --loadlua tools/emu/probe.lua "raw/Batman - Return of the Joker (USA).nes"
-- Then: cat tools/emu/probe.txt
--
-- Why: FCEUX's Lua API varies by version (2.2.x vs 2.6.x). The dump scripts
-- must call functions that actually exist instead of guessed names, so this
-- probes the namespaces the dumps need and exits.

local NAMESPACES =
    { "emu", "memory", "ppu", "rom", "savestate", "joypad", "gui", "movie", "debugger", "zapper" }

local lines = {}
local function say(s)
    lines[#lines + 1] = s
end

say("fceux lua probe")
if emu and emu.getversion then
    say("version: " .. tostring(emu.getversion()))
end

for _, ns in ipairs(NAMESPACES) do
    local t = _G[ns]
    if type(t) == "table" then
        local fns = {}
        for k, v in pairs(t) do
            if type(v) == "function" then
                fns[#fns + 1] = k
            end
        end
        table.sort(fns)
        say(ns .. ": " .. table.concat(fns, " "))
    else
        say(ns .. ": (absent)")
    end
end

-- Specific questions the dump scripts depend on.
local function can(expr, fn)
    local ok, err = pcall(fn)
    say(string.format("%-46s %s%s", expr, tostring(ok), ok and "" or (" -- " .. tostring(err))))
end

can("memory.readbyte(0)", function() return memory.readbyte(0) end)
can("memory.readbyterange(0,16)", function() return memory.readbyterange(0, 16) end)
can("memory.registerwrite(0, f)", function() return memory.registerwrite(0, function() end) end)
can("ppu.readbyte(0x2000)", function() return ppu.readbyte(0x2000) end)
can("ppu.readbyterange(0x2000,16)", function() return ppu.readbyterange(0x2000, 16) end)
can("emu.framecount()", function() return emu.framecount() end)
can("emu.frameadvance()", function() return true end) -- probe only: signature check
can("joypad.set(1,{})", function() return true end)
can("os.getenv('ROM')", function() return os.getenv("ROM") end)

-- Write the report where the shell can read it, then stop the emulator.
local path = os.getenv("PROBE_OUT") or "tools/emu/probe.txt"
local f = io.open(path, "w")
if f then
    f:write(table.concat(lines, "\n"), "\n")
    f:close()
    print("probe written to " .. path)
else
    print(table.concat(lines, "\n"))
end
print(table.concat(lines, "\n"))

if emu and emu.exit then
    emu.exit()
else
    os.exit()
end
