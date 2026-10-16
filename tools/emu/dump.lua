-- FCEUX Lua extraction dump for Batman: Return of the Joker.
--
-- NOTE: prefer `mapper.lua`, which writes all of this *and* the mapper/PPU
-- register traces (with program counters) into the same directory, so
-- `tools/emu/check-frame.py` can say whether a frame is even reproducible. Use
-- this script only when you do not need traces.
--
-- Dev-only tooling: never ships with the port. Gives us runtime truth (VRAM,
-- palette RAM, nametables, CHR bank schedule, WRAM collision map, RAM
-- variables, APU register writes) to cross-check against `disasm/MAP.md`.
--
-- Run (paths MUST be absolute: fceux's working directory is not the repo):
--   DUMP_DIR="$PWD/tools/emu/dumps" \
--   timeout -k 3 600 fceux --sound 0 --loadlua tools/emu/dump.lua \
--     "raw/Batman - Return of the Joker (USA).nes"
--
-- Probed API (fceux 2.6.5): ppu.readbyterange, memory.readbyterange,
-- memory.registerwrite, savestate.save, emu.speedmode/frameadvance/framecount.
--
-- Files written into DUMP_DIR:
--   config.txt        what ran
--   ppu-<f>.bin       PPU $0000-$3FFF: CHR windows, nametables, palette RAM
--   ram-<f>.bin       CPU $0000-$07FF (OAM page $02, mapper shadow $E0-$EC, CHR shadow $88-$8F)
--   wram-<f>.bin      $6000-$7FFF (the level/collision tilemap built by the loader)
--   ramseq-<from>.bin full RAM per frame, concatenated, for offline variable hunting
--   leads.csv         curated RAM/scroll values per frame
--   apu-writes.csv    frame,address,value for every $4000-$4017 write
--   inputs.csv        frame,pad: the controller byte set for each frame (A=1 B=2 Select=4
--                     Start=8 Up=16 Down=32 Left=64 Right=128), for replay in rotj-oracle
--   oam-<f>.bin       the page the last $4014 write copied into OAM (Lua cannot read OAM)
--   ram-writes.csv    WATCH=<hex addrs>: every write to those addresses with the PC
--   ram-exec.csv      EXEC=<hex addrs>: every execution of those addresses, with registers
--   state-<f>.fcs     savestate per dump frame (re-enter without replaying input)
--
-- INPUT modes relevant here: `route` plays stage 1-1 from the start (Right held,
-- plus the held-A jumps in ROUTE_JUMPS) until real enemies are drawn as sprites;
-- `seek`/`actorseek` write $0400/$00AC to park the camera on a screen (no
-- gameplay route needed); pair either with WATCH and EXEC to trace the per-slot
-- actor arrays ($7CC0-$7DEF). `route` is what produced tools/emu/route-dense.

local DIR = os.getenv("DUMP_DIR") or "tools/emu/dumps"
local DUMPS = os.getenv("DUMPS") or "300,600,900,1200,1800,3000"
local RAMSEQ = os.getenv("RAMSEQ") or "1250,1600"
local UPTIME = tonumber(os.getenv("UPTIME") or "3100")
-- "from,to": save a screenshot every frame in that range, to pin down which
-- frame our dump state corresponds to (the framebuffer lags the PPU state).
local SHOTS = os.getenv("SHOTS") or ""
local last_dump = tonumber((DUMPS:gsub(".*,", ""))) or 3000
-- INPUT=route: frames at which A is pressed. List consecutive frames to hold A
-- (the jump is variable-height: ~1 frame rises 22 px, ~25 frames rises 54 px,
-- which is what clears the 32 px step at level x 192). Comma separated.
local route_jumps = {}
for tok in (os.getenv("ROUTE_JUMPS") or ""):gmatch("[^,]+") do
    route_jumps[tonumber(tok)] = true
end
-- State for `route2`'s bunny-hop: whether the previous frame was airborne.
local route_air = false

os.execute("mkdir -p '" .. DIR .. "'")
pcall(function() emu.speedmode("maximum") end) -- unthrottled; dump speed matters

local dump_at = {}
for tok in DUMPS:gmatch("[^,]+") do
    dump_at[tonumber(tok)] = true
end
local seq_from, seq_to = RAMSEQ:match("(%d+),(%d+)")
seq_from, seq_to = tonumber(seq_from), tonumber(seq_to)

local function openw(name)
    local f = io.open(DIR .. "/" .. name, "w")
    if not f then
        emu.print("could not open " .. name)
    end
    return f
end

-- Curated leads (docs/RE-NOTES.md cheats + disasm/MAP.md RAM map).
local LEADS = {
    { "sublevel", 0x0401, 1 },
    { "stage", 0x0402, 1 },
    { "jump_009e", 0x009E, 1 },
    { "scroll_00c3", 0x00C3, 1 },
    { "anim_0466", 0x0466, 1 },
    { "walkbehind_046f", 0x046F, 1 },
    { "invin_0477", 0x0477, 1 },
    { "jump_047d", 0x047D, 1 },
    { "lives_0482", 0x0482, 1 },
    { "suit_04f2", 0x04F2, 1 },
    { "suit_04f3", 0x04F3, 1 },
    { "ppuctrl_ab", 0x00AB, 1 },
    { "scrollx_ad", 0x00AD, 1 },
    { "scrolly_ac", 0x00AC, 1 },
    { "ppumask_fe", 0x00FE, 1 },
    { "irq_d0", 0x00D0, 1 },
    { "irq_d3", 0x00D3, 1 },
    { "timer_f1", 0x00F1, 1 },
    { "cam_ca", 0x00CA, 2 },
    { "cam_0460", 0x0460, 2 },
    { "cam_0462", 0x0462, 2 },
    { "obj_7b00", 0x7B00, 2 },
    { "obj_7b20", 0x7B20, 2 },
    { "vel_7e70", 0x7E70, 2 },
    { "state_7d90", 0x7D90, 1 },
    { "type_7db0", 0x7DB0, 1 },
    { "anim_7dd0", 0x7DD0, 1 },
}

local leads_f = openw("leads.csv")
if leads_f then
    local hdr = { "frame" }
    for _, c in ipairs(LEADS) do
        hdr[#hdr + 1] = c[1]
    end
    leads_f:write(table.concat(hdr, ","), "\n")
end

local seq_f
if seq_from and seq_to then
    seq_f = openw(string.format("ramseq-%d.bin", seq_from))
end

local inputs_f = openw("inputs.csv")
if inputs_f then
    inputs_f:write("frame,pad\n")
end
local last_oam = nil

-- INPUT=replay: play a `frame,pad` csv back (one written by this script's own
-- `inputs.csv`, or by the oracle's `routefind`), so fceux reproduces an oracle
-- route exactly. Loaded once; only the input is set, nothing is poked.
local replay_pads = {}
if (os.getenv("INPUT") or "") == "replay" then
    local path = os.getenv("REPLAY")
    if path and path ~= "" then
        local f = io.open(path, "r")
        if f then
            for line in f:lines() do
                local fr, pad = line:match("^(%d+),(%d+)")
                if fr then
                    replay_pads[tonumber(fr)] = tonumber(pad)
                end
            end
            f:close()
        else
            emu.print("could not open REPLAY " .. path)
        end
    end
end
local PAD_KEYS = { "A", "B", "select", "start", "up", "down", "left", "right" }
local function keys_for_pad(pad)
    local keys = {}
    for bit, key in ipairs(PAD_KEYS) do
        if math.floor(pad / 2 ^ (bit - 1)) % 2 == 1 then
            keys[key] = true
        end
    end
    return keys
end

local apu_f = openw("apu-writes.csv")
if apu_f then
    apu_f:write("frame,addr,value\n")
end

-- Log every APU register write: reference for our synth, and for verifying a
-- ported music driver later. Callback args differ across builds, so read the
-- accumulator as a fallback (STA abs leaves the written value in A).
for addr = 0x4000, 0x4017 do
    pcall(function()
        memory.registerwrite(addr, function(...)
            local args = { ... }
            local value = args[3]
            if type(value) ~= "number" then
                value = memory.getregister("A")
            end
            if apu_f then
                apu_f:write(string.format("%d,%04X,%02X\n", emu.framecount(), addr, value or 0))
            end
            if addr == 0x4014 and value then
                last_oam = memory.readbyterange(value * 256, 256)
            end
        end)
    end)
end

-- Actor-field tracing: `WATCH="7d50,7d51,..."` logs every write to those CPU
-- addresses with the program counter that made it; `EXEC="ab48,ab5a,..."` logs
-- every time execution reaches those addresses, with the registers. This is how
-- a consumer of a per-slot array is found and how its input is read.
local WATCH = os.getenv("WATCH") or ""
local watch_f = nil
if WATCH ~= "" then
    watch_f = openw("ram-writes.csv")
    if watch_f then
        watch_f:write("frame,addr,value,pc\n")
        for token in WATCH:gmatch("[^,]+") do
            local addr = tonumber(token, 16)
            memory.registerwrite(addr, function(...)
                local args = { ... }
                local value = args[3]
                if type(value) ~= "number" then
                    value = memory.readbyte(addr)
                end
                watch_f:write(
                    string.format(
                        "%d,%04X,%02X,%04X\n",
                        emu.framecount(),
                        addr,
                        value,
                        memory.getregister("PC")
                    )
                )
            end)
        end
    end
end

local EXEC = os.getenv("EXEC") or ""
local exec_f = nil
if EXEC ~= "" then
    exec_f = openw("ram-exec.csv")
    if exec_f then
        exec_f:write("frame,addr,a,x,y,pc\n")
        for token in EXEC:gmatch("[^,]+") do
            local addr = tonumber(token, 16)
            memory.registerexec(addr, function()
                exec_f:write(
                    string.format(
                        "%d,%04X,%02X,%02X,%02X,%04X\n",
                        emu.framecount(),
                        addr,
                        memory.getregister("A"),
                        memory.getregister("X"),
                        memory.getregister("Y"),
                        memory.getregister("PC")
                    )
                )
            end)
        end
    end
end

local function write_all(path, data)
    local f = io.open(path, "wb")
    if f then
        f:write(data)
        f:close()
    end
end

local function heavy_dump(frame)
    write_all(string.format("%s/ram-%d.bin", DIR, frame), memory.readbyterange(0x0000, 0x0800))
    write_all(string.format("%s/wram-%d.bin", DIR, frame), memory.readbyterange(0x6000, 0x2000))
    if last_oam then
        write_all(string.format("%s/oam-%d.bin", DIR, frame), last_oam)
    end
    local ppu = ppu.readbyterange(0x0000, 0x4000)
    write_all(string.format("%s/ppu-%d.bin", DIR, frame), ppu)
    -- The emulator's own rendering of this frame: the ground truth our
    -- renderer is checked against.
    pcall(function()
        gui.savescreenshotas(string.format("%s/screen-%d.png", DIR, frame))
    end)
    pcall(function()
        savestate.save(string.format("%s/state-%d.fcs", DIR, frame))
    end)
end

local function log_leads(frame)
    if not leads_f then
        return
    end
    local row = { tostring(frame) }
    for _, c in ipairs(LEADS) do
        local v = memory.readbyte(c[2])
        if c[3] == 2 then
            v = memory.readword(c[2])
        end
        row[#row + 1] = tostring(v)
    end
    leads_f:write(table.concat(row, ","), "\n")
end

local function input_for(frame)
    -- Exploratory stage-1 script: confirm the title/attract length from the
    -- first run's leads.csv, then tighten these frames.
    -- INPUT=off scripts no buttons (A/B whether input registers at all);
    -- INPUT=all mashes everything, to tell "ignoring input" from "wrong buttons".
    local mode = os.getenv("INPUT") or "script"
    if mode == "off" then
        return {}
    end
    if mode == "replay" then
        -- Play the loaded `frame,pad` csv back (REPLAY=<path>); input only.
        return keys_for_pad(replay_pads[frame] or 0)
    end
    if mode == "all" and frame >= 1200 then
        return {
            A = true,
            B = true,
            start = true,
            select = true,
            up = true,
            down = true,
            left = true,
            right = true,
        }
    end
    if mode == "early" then
        local keys = {}
        if frame >= 90 and frame <= 97 then
            keys.start = true
        end
        if frame >= 260 then
            keys.right = true
        end
        if frame % 180 == 0 and frame > 300 then
            keys.A = true
        end
        return keys
    end
    if mode == "physics" then
        -- Purpose-built: reach gameplay, then stand still, walk, stop, jump
        -- from standstill, then running jump. Every frame is captured, so the
        -- acceleration, top speed, jump impulse and gravity can be read off
        -- the variables instead of guessed.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if frame >= 660 and frame <= 780 then -- walk burst from standstill
            keys.right = true
        end
        if frame == 800 then -- jump from standstill
            keys.A = true
        end
        if frame >= 900 then -- running jump
            keys.right = true
            if frame == 905 then
                keys.A = true
            end
        end
        return keys
    end
    if mode == "sweep" then
        -- Drive the actor's own level-x variable ($94, measured from the
        -- captures) so the game streams the level past the point where the
        -- scripted runs stop. SWEEP_FROM/SWEEP_TO/SWEEP_STEP come from the
        -- environment. The capture logs the nametables per frame, so whether
        -- the game actually streamed is checkable afterwards.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if frame >= 560 then
            local from = tonumber(os.getenv("SWEEP_FROM") or "185")
            local to = tonumber(os.getenv("SWEEP_TO") or "255")
            local step = tonumber(os.getenv("SWEEP_STEP") or "4")
            local x = from + math.floor((frame - 560) / step)
            if x > to then x = to end
            memory.writebyte(0x0094, x % 256)
            -- The high byte of the level x, if SWEEP_HI names it (the game's
            -- 16-bit position is not fully mapped yet, so this is a probe).
            local hi = os.getenv("SWEEP_HI")
            if hi and hi ~= "" then
                memory.writebyte(tonumber(hi, 16), math.floor(x / 256))
            end
            -- The screen/area index ($0400) steps by 2 per screen; writing it
            -- advances the level, which is how a whole sublevel gets captured.
            local scr = os.getenv("SWEEP_SCREEN")
            if scr and scr ~= "" then
                local base = tonumber(os.getenv("SWEEP_SCREEN_BASE") or "0")
                memory.writebyte(0x0400, base + 2 * math.floor(x / 256))
            end
            keys.right = true
        end
        return keys
    end
    if mode == "idle" then
        -- Control run for the move captures: identical Start timing, no input
        -- afterwards. OAM diffed against a move capture shows exactly what the
        -- move added on screen.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        return keys
    end
    if mode == "moves" then
        -- Purpose-built: exercise the player's other moves so their animation
        -- states and any projectile sprites can be read out of RAM/OAM.
        --   B tap (project), B held (charge), Down+A (slide), Up+B (aim up),
        --   A (jump), Down (crouch).
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if frame == 600 then keys.B = true end                    -- tap: shoot
        if frame >= 650 and frame <= 700 then keys.B = true end    -- hold: charge
        if frame >= 750 and frame <= 760 then keys.down = true; keys.A = true end
        if frame >= 850 and frame <= 880 then keys.up = true; keys.B = true end
        if frame == 900 then keys.A = true end                     -- jump
        if frame >= 950 and frame <= 980 then keys.down = true end -- crouch
        return keys
    end
    if mode == "hurt" then
        -- Like `walk`, but without the invincibility write, so Batman actually
        -- collides with the first enemy: pairs with WATCH to capture the damage
        -- and death sound requests.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if frame >= 560 then
            keys.right = true
            if frame % 70 < 3 then
                keys.A = true
            end
        end
        return keys
    end
    if mode == "jump" then
        -- Controlled: reach gameplay, then jump from standstill at isolated
        -- frames. Pairs with WATCH to tell a jump sound from a walking one.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        for _, f in ipairs({ 650, 750, 850, 950, 1050 }) do
            if frame == f then
                keys.A = true
            end
        end
        return keys
    end
    if mode == "walkonly" then
        -- Controlled: reach gameplay, then only walk right (no jumps, no
        -- invincibility). Pairs with WATCH to tell a walking sound from a jump.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if frame >= 560 then
            keys.right = true
        end
        return keys
    end
    if mode == "slide" then
        -- Purpose-built: press Down+A on flat ground (the actor stands at the
        -- level start; nothing else is pressed), to capture the move the manual
        -- calls a slide. SLIDE_FROM/SLIDE_TO set the press window (default a
        -- 40-frame hold); SLIDE_RIGHT=1 walks right for 40 frames first so the
        -- same press can be tried with running momentum.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        local from = tonumber(os.getenv("SLIDE_FROM") or "620")
        local to = tonumber(os.getenv("SLIDE_TO") or "660")
        if os.getenv("SLIDE_RIGHT") == "1" and frame >= from - 40 and frame < from then
            keys.right = true
        end
        if frame >= from and frame <= to then
            keys.down = true
            keys.A = true
        end
        return keys
    end
    if mode == "holdA" then
        -- Variable-height jump probe: hold A (and optionally Down) from
        -- HOLD_FROM for HOLD_LEN frames on the flat level start, so the extra
        -- thrust a held A adds can be read per hold length.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        local from = tonumber(os.getenv("HOLD_FROM") or "700")
        local len = tonumber(os.getenv("HOLD_LEN") or "1")
        if frame >= from and frame < from + len then
            keys.A = true
            if os.getenv("HOLD_DOWN") == "1" then
                keys.down = true
            end
        end
        return keys
    end
    if mode == "prog" then
        -- Progress through the sublevel: walk right and jump often, so the actor
        -- clears the ledge at level x ~160-183 (the one that stops a plain walk)
        -- and the capture sees the rest of the level, including its enemies.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if frame >= 600 then
            keys.right = true
            if frame % 40 < 4 then
                keys.A = true
            end
            if frame % 120 < 3 then
                keys.B = true
            end
        end
        return keys
    end
    if mode == "route" then
        -- Scripted stage 1-1 playthrough: walk right and hold-Jump up the step
        -- at level x 160-192, until the level's actor spawner has real enemies
        -- drawn as sprites (frames ~850+ in tools/emu/route-dense). The jump
        -- is variable-height, so the jump frames hold A across each step; the
        -- walk speed and the step's height are in docs/PHYSICS.md and
        -- docs/ACTORS.md §7f. The player is one-hit-killable, so ROUTE_INVIN=1
        -- writes the invincibility flag each frame; ROUTE_FROM is the first
        -- frame Right is held (gameplay starts ~607).
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if os.getenv("ROUTE_INVIN") == "1" then
            memory.writebyte(0x0477, 1)
        end
        if frame >= tonumber(os.getenv("ROUTE_FROM") or "610") then
            keys.right = true
        end
        if route_jumps[frame] then
            keys.A = true
        end
        return keys
    end
    if mode == "route2" then
        -- Extended stage 1-1 playthrough: like `route` but keeps advancing past
        -- the first two scripted held jumps by falling back to a periodic
        -- *held* jump (A held ROUTE_HOLD frames every ROUTE_PERIOD), so the
        -- actor clears each 32-40 px step it meets. A tap only rises 22 px, so
        -- the hold must span the thrust window (docs/PHYSICS.md). ROUTE_JUMPS
        -- still adds the route-dense explicit jumps. ROUTE_INVIN=1 pokes $0477
        -- (the game's meter/progress byte), which is only safe for RE captures.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if os.getenv("ROUTE_INVIN") == "1" then
            memory.writebyte(0x0477, 1)
        end
        -- ROUTE_END_AT=<frame>: arm the level-end transition once (write $BA=5,
        -- the value sub_D4D3 dispatches to the bank14 $D58F level-end handler).
        local end_at = tonumber(os.getenv("ROUTE_END_AT") or "0")
        if end_at > 0 and frame == end_at then
            memory.writebyte(0x00BA, 5)
        end
        if frame >= tonumber(os.getenv("ROUTE_FROM") or "610") then
            keys.right = true
            if route_jumps[frame] then
                keys.A = true
            end
            -- Bunny-hop: press A while grounded, keep holding through the whole
            -- airborne phase (the held-jump thrust), release only on the landing
            -- frame, then press again. Because he jumps again the moment he
            -- lands, he keeps his run speed and clears each step/crate; jumping
            -- from a standstill at a step face (continuous A) cannot clear it.
            local bunny_from = tonumber(os.getenv("ROUTE_BUNNY_FROM") or "0")
            -- ROUTE_BUNNY_STAGE (optional): only bunny-hop while $0402 equals this
            -- value, so the actor walks (no jump) in the other sections.
            local bunny_stage = os.getenv("ROUTE_BUNNY_STAGE")
            local bunny_ok = bunny_stage == nil
                or memory.readbyte(0x0402) == tonumber(bunny_stage)
            if bunny_from > 0 and frame >= bunny_from and bunny_ok then
                if memory.readbyte(0x047D) ~= 0 then
                    keys.A = true
                    route_air = true
                elseif not route_air then
                    keys.A = true
                else
                    route_air = false
                end
            end
            -- Camera-window trigger: hold A while the horizontal scroll $00AC is
            -- inside [from,to]. Because there is no air control, the jump must
            -- start while he is still running, before he pins on a step face.
            local cam_from = tonumber(os.getenv("ROUTE_CAM_FROM") or "0")
            local cam_to = tonumber(os.getenv("ROUTE_CAM_TO") or "0")
            if cam_to > 0 then
                local cam = memory.readbyte(0x00AC)
                if cam >= cam_from and cam <= cam_to then
                    keys.A = true
                end
            end
            -- Periodic back-off: hold Left for ROUTE_LEFT_HOLD frames every
            -- ROUTE_LEFT_PERIOD frames, to break out of a pinned corner.
            local left_period = tonumber(os.getenv("ROUTE_LEFT_PERIOD") or "0")
            if left_period > 0 then
                local left_from = tonumber(os.getenv("ROUTE_LEFT_FROM") or "0")
                local left_hold = tonumber(os.getenv("ROUTE_LEFT_HOLD") or "40")
                -- ROUTE_LEFT_STAGE (optional): only back off while $0402 equals
                -- this value, so the back-off does not wreck the earlier run.
                local left_stage = os.getenv("ROUTE_LEFT_STAGE")
                local left_ok = left_stage == nil
                    or memory.readbyte(0x0402) == tonumber(left_stage)
                if left_ok and frame >= left_from and (frame - left_from) % left_period < left_hold then
                    keys.right = false
                    keys.left = true
                end
            end
            -- Optional extra held buttons, for probing what clears a blocker.
            local up_from = tonumber(os.getenv("ROUTE_UP_FROM") or "0")
            if up_from > 0 and frame >= up_from then
                keys.up = true
            end
            local b_from = tonumber(os.getenv("ROUTE_B_FROM") or "0")
            if b_from > 0 and frame >= b_from then
                keys.B = true
            end
            local auto_from = tonumber(os.getenv("ROUTE_AUTO_FROM") or "0")
            local period = tonumber(os.getenv("ROUTE_PERIOD") or "0")
            if period > 0 and frame >= auto_from then
                local hold = tonumber(os.getenv("ROUTE_HOLD") or "25")
                if (frame - auto_from) % period < hold then
                    keys.A = true
                end
            end
        end
        return keys
    end
    if mode == "seek" then
        -- Move the *camera and screen index* instead of the player: the game
        -- streams the screen and runs its actor spawner from $0400 every frame,
        -- so this should produce a live-enemy scene without needing a gameplay
        -- route (which the level's opening blocks). $0400 steps by 2 per
        -- screen, $00AC is the pixel scroll.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if frame >= 620 then
            local screen = tonumber(os.getenv("SEEK_SCREEN") or "2")
            local level_x = tonumber(os.getenv("SEEK_X") or "700")
            -- $00AC is the nametable scroll in px; the actor's level x is the
            -- scroll plus his on-screen x, and his on-screen x is 239 - $94
            -- (measured), so writing both puts him at level_x.
            memory.writebyte(0x0400, screen * 2)
            memory.writebyte(0x00AC, level_x % 256)
            memory.writebyte(0x94, (239 - (level_x % 256)) % 256)
            memory.writebyte(0x92, 0)
            memory.writebyte(0x93, 0)
            memory.writebyte(0xB1, 192)
        end
        return keys
    end
    if mode == "actorseek" then
        -- Actor-investigation capture: same camera/screen write as `seek`, so
        -- the spawner runs from $0400 for a chosen screen without a gameplay
        -- route (the actors are not necessarily drawn; this only populates the
        -- slot arrays). Pair with the WATCH/EXEC trace files above to see which
        -- program counters read and write the per-slot actor arrays.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if frame >= 620 then
            local screen = tonumber(os.getenv("ACTOR_SCREEN") or "3")
            memory.writebyte(0x0400, screen * 2)
            memory.writebyte(0x00AC, tonumber(os.getenv("ACTOR_X") or "700") % 256)
        end
        -- ACTOR_WALK=<frame> holds Right from that frame, to move the player
        -- toward the spawned objects.
        local walk = tonumber(os.getenv("ACTOR_WALK") or "0")
        if walk > 0 and frame >= walk then
            keys.right = true
        end
        return keys
    end
    if mode == "warp" then
        -- Probe the inferred sublevel warp: leave the title, write the target
        -- stage/sublevel indices ($0402/$0401), then force a reload by zeroing
        -- the lives so the game restarts the sublevel. A savestate is taken
        -- before the write, because a wrong transition can lock the game in a
        -- handler (see docs/SUBLEVELS.md).
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if frame == 620 then
            savestate.save(DIR .. "/before-warp.fcs")
        end
        -- Kill him to force a sublevel reload, then keep writing the target
        -- indices *during* the reload so whichever frame reads them sees the
        -- target (writing them beforehand locked the game, so the reload's own
        -- writes are what matter).
        local target_stage = tonumber(os.getenv("WARP_STAGE") or "0")
        local target_sub = tonumber(os.getenv("WARP_SUBLEVEL") or "0")
        if frame >= 621 and frame <= 623 then
            memory.writebyte(0x0482, 0) -- lives: force a restart
        end
        if frame >= 624 and frame <= 700 then
            memory.writebyte(0x0402, target_stage)
            memory.writebyte(0x0401, target_sub)
        end
        return keys
    end
    if mode == "walk" then
        -- Walk through the whole sublevel so the game streams the level data.
        -- The actor would otherwise die to the first enemy (one hit kills), so
        -- this writes the invincibility flag every frame (GameHacking lead
        -- $0477 = "Super Batman"). The capture logs lives ($0482) and the
        -- sublevel index ($0401) so a death is visible rather than silent.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if frame >= 560 then
            memory.writebyte(0x0477, 1)
            keys.right = true
            if frame % 70 < 3 then
                keys.A = true
            end
        end
        return keys
    end
    if mode == "drive" then
        -- One Start press to leave the title (gameplay starts ~frame 414 in
        -- run1), then drive: never press Start again, since it toggles pause.
        local keys = {}
        if frame >= 300 and frame <= 306 then
            keys.start = true
        end
        if frame >= 500 then
            keys.right = true
            if frame % 90 < 3 then
                keys.A = true
            end
            if frame % 150 < 3 then
                keys.B = true
            end
        end
        return keys
    end
    if mode == "plan" then
        -- Hedge the title screen: mash Start through the intro, then drive
        -- right with periodic jumps, and keep going so we can find where
        -- movement actually starts.
        local keys = {}
        if frame >= 100 and frame <= 900 and frame % 20 < 3 then
            keys.start = true
        end
        if frame > 900 then
            keys.right = true
            if frame % 60 < 3 then
                keys.A = true
            end
            if frame % 137 < 3 then
                keys.B = true
            end
        end
        return keys
    end
    local keys = {}
    if frame >= 600 and frame <= 606 then
        keys.start = true
    end
    if frame >= 700 and frame <= 2990 then
        keys.right = true
    end
    if frame == 1000 or frame == 1400 or frame == 2000 then
        keys.A = true
    end
    return keys
end

local cfg = openw("config.txt")
if cfg then
    cfg:write(
        string.format("dumps=%s\nramseq=%s\nuptime=%d\nleads=%d\n", DUMPS, RAMSEQ, UPTIME, #LEADS)
    )
    cfg:write(string.format("input=%s\n", os.getenv("INPUT") or "script"))
    for _, name in ipairs({
        "ROUTE_JUMPS", "ROUTE_FROM", "ROUTE_INVIN", "SWEEP_FROM", "SWEEP_TO", "SWEEP_STEP",
        "SWEEP_HI", "SWEEP_SCREEN", "SWEEP_SCREEN_BASE", "SLIDE_FROM", "SLIDE_TO", "SLIDE_RIGHT",
        "SEEK_SCREEN", "SEEK_X", "ACTOR_SCREEN", "ACTOR_X", "ACTOR_WALK", "WARP_STAGE",
        "WARP_SUBLEVEL", "WATCH", "EXEC", "SHOTS", "REPLAY",
    }) do
        local value = os.getenv(name)
        if value then
            cfg:write(string.format("%s=%s\n", name, value))
        end
    end
    cfg:close()
end

for frame = 0, UPTIME do
    local keys = input_for(frame)
    joypad.set(1, keys)
    if inputs_f then
        local pad = 0
        for bit, key in ipairs({ "A", "B", "select", "start", "up", "down", "left", "right" }) do
            if keys[key] then
                pad = pad + 2 ^ (bit - 1)
            end
        end
        inputs_f:write(string.format("%d,%d\n", frame, pad))
    end
    if dump_at[frame] then
        heavy_dump(frame)
    end
    local shot_from, shot_to, shot_step = SHOTS:match("(%d+),(%d+),(%d+)")
    if not shot_from then
        shot_from, shot_to = SHOTS:match("(%d+),(%d+)")
    end
    shot_step = tonumber(shot_step) or 1
    if shot_from and frame >= tonumber(shot_from) and frame <= tonumber(shot_to)
        and frame % shot_step == 0 then
        pcall(function()
            gui.savescreenshotas(string.format("%s/shot-%d.png", DIR, frame))
        end)
    end
    log_leads(frame)
    if seq_f and frame >= seq_from and frame <= seq_to then
        seq_f:write(memory.readbyterange(0x0000, 0x0800))
    end
    emu.frameadvance()
end

for _, f in ipairs({ leads_f, seq_f, apu_f, watch_f, exec_f, inputs_f }) do
    if f then
        f:close()
    end
end
emu.print("dump complete: " .. DIR)
emu.exit()
