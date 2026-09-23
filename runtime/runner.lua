-- Bundled Rust CLI bridge. All typed modules are checked before this starts.
local action, manifest_path = arg[1], arg[2]
local function fail(message)
   io.stderr:write(tostring(message), "\n")
   os.exit(1)
end
local manifest = assert(loadfile(manifest_path))()
local source = manifest.entry.source
local raw_require = require
local loaded_here = {}
-- Static imports use private source-keyed state, independent of ambient
-- require/package loaders and mutable package.loaded/preload entries.
local import_cache, importing = {}, {}
local protected_call, raise = pcall, error
local load_unit
local function checked_value(unit, name, static)
   local cached = import_cache[unit.source]
   if cached and (static or cached.imported) then
      if static then cached.imported = true end
      return cached.value
   end
   if importing[unit.source] then raise("cyclic static import '" .. name .. "'", 2) end
   importing[unit.source] = true
   local public_before
   if not static then public_before = package.loaded[name] end
   local ok, value = protected_call(function() return load_unit(unit)(name) end)
   importing[unit.source] = nil
   if not ok then raise(value, 0) end
   local returned = value
   if value == nil then
      value = (not static and package.loaded[name] ~= public_before and package.loaded[name]) or true
   end
   import_cache[unit.source] = {value=value, imported=static}
   if static then return value end
   return returned
end
local function static_import(name)
   local unit = manifest.modules[name]
   if not unit then raise("unprepared static import '" .. name .. "'", 2) end
   return checked_value(unit, name, true)
end
local arguments = { [0] = source }
for i = 3, #arg do arguments[i-2] = arg[i] end
arg = arguments

local prefixes = {}
for _, root in ipairs(manifest.roots) do
   prefixes[#prefixes+1] = root .. "/?.lua;" .. root .. "/?/init.lua;"
end
package.path = table.concat(prefixes) .. package.path
local runtime_dir = os.getenv("TYNEX_LUAJIT_DIR")
if runtime_dir then
   package.path = runtime_dir .. "/?.lua;" .. runtime_dir .. "/lua/?.lua;" .. package.path
   package.cpath = runtime_dir .. "/?.so;" .. runtime_dir .. "/?.dll;" .. package.cpath
end

-- Load the scheduler from the checked bundle, and capture its native bridge
-- before user modules run. Neither lookup uses mutable require/preload state.
local native_loadlib, native_paths = package.loadlib, package.cpath
local function load_native_adapter(name)
   local errors = {}
   for template in native_paths:gmatch("[^;]+") do
      local path = template:gsub("%?", name)
      local file = io.open(path, "rb")
      if file then
         file:close()
         local open, message = native_loadlib(path, "luaopen_" .. name)
         if open then return open() end
         errors[#errors+1] = message
      end
   end
   error(name .. " adapter unavailable: " .. table.concat(errors, "; "), 0)
end
local async_runtime = assert(loadfile(manifest.async_runtime))(
   function() return load_native_adapter("tynex_task") end,
   function() return load_native_adapter("tynex_io") end)
local async_traceback = async_runtime.__traceback
local public_scheduler = setmetatable({}, {__index=async_runtime, __metatable="Tynex scheduler",
   __newindex=function() error("scheduler methods are read-only", 2) end})

local function local_candidate(name)
   if type(name) ~= "string" then return end
   -- Static project names are dotted, never paths. Other names are external.
   if name:find("[/\\%z]") or name:find("..", 1, true) or name:sub(1,1)=="." or name:sub(-1)=="." then return end
   local stem = name:gsub("%.", "/")
   for _, root in ipairs(manifest.roots) do
      local selected
      for _, suffix in ipairs({".luax", "/init.luax", ".lua", "/init.lua"}) do
         local path = root .. "/" .. stem .. suffix
         local file = io.open(path, "rb")
         if file then
            file:close()
            if selected then error("ambiguous project module '" .. name .. "'", 3) end
            selected = path
         end
      end
      if selected then return selected end
   end
end

local function typed_require(name)
   if manifest.modules[name] then
      if package.preload[name] then
         error("project module '" .. name .. "' conflicts with package.preload; use distinct local and host module names", 2)
      end
      if package.loaded[name] and not loaded_here[name] then
         error("project module '" .. name .. "' was loaded outside the checked module graph", 2)
      end
   elseif local_candidate(name) then
      error("project module '" .. name .. "' was not prepared; use a literal require or register dynamic_modules", 2)
   end
   return raw_require(name)
end
local typed_environment = setmetatable({require=typed_require, __tynex_import=static_import,
   __tynex_async_runtime=async_runtime, scheduler=public_scheduler,
   fs=async_runtime.fs, net=async_runtime.net}, {__index=_G, __newindex=_G})
load_unit = function(unit)
   local chunk, err
   if unit.generated then
      local file
      file, err = io.open(unit.generated, "rb")
      if not file then error(err, 2) end
      local code = file:read("*a")
      file:close()
      chunk, err = loadstring(code, "@" .. (unit.mapped and unit.generated or unit.source))
      if chunk then setfenv(chunk, typed_environment) end
   else
      chunk, err = loadfile(unit.source)
   end
   if not chunk then error(err, 2) end
   return chunk
end
-- Preserve Lua's package.loaded and preload handling. Typed callers reject
-- conflicting preloads/cached modules rather than trusting the wrong interface.
table.insert(package.loaders, 2, function(name)
   local unit = manifest.modules[name]
   if unit then
      return function(module_name)
         local value = checked_value(unit, module_name, false)
         loaded_here[name] = true
         return value
      end
   end
   local candidate = local_candidate(name)
   if candidate and candidate:sub(-5)==".luax" then
      error("typed module '" .. name .. "' was not prepared; add it to dynamic_modules", 2)
   end
   return "\n\tno prepared Tynex module '" .. tostring(name) .. "'"
end)

if action == "debug" then
   jit.off()
   jit.flush()
   if os.getenv("LOCAL_LUA_DEBUGGER_VSCODE") == "1" then
      local ok, debugger = pcall(require, "lldebugger")
      if not ok and os.getenv("LOCAL_LUA_DEBUGGER_FILEPATH") then
         ok, debugger = pcall(dofile, os.getenv("LOCAL_LUA_DEBUGGER_FILEPATH"))
      end
      if not ok then fail("Cannot load VS Code lldebugger: " .. tostring(debugger)) end
      debugger.start()
   elseif os.getenv("TYNEX_EMMY_CORE_DIR") then
      local core = os.getenv("TYNEX_EMMY_CORE_DIR")
      package.cpath = core .. "/?.dylib;" .. core .. "/?.so;" .. core .. "/?.dll;" .. package.cpath
      local ok, debugger = pcall(require, "emmy_core")
      if not ok then fail("Cannot load EmmyLua core: " .. tostring(debugger)) end
      local port = tonumber(os.getenv("TYNEX_DEBUG_PORT") or "9966")
      if not port or port < 1 or port > 65535 or port % 1 ~= 0 then fail("Invalid TYNEX_DEBUG_PORT") end
      debugger.tcpListen("127.0.0.1", port)
      io.stderr:write("Waiting for EmmyLua on 127.0.0.1:", port, "...\n")
      debugger.waitIDE()
   else
      fail("Use the VS Code launch configuration or set TYNEX_EMMY_CORE_DIR. See docs/debugging.md.")
   end
end
local ok, chunk = pcall(load_unit, manifest.entry)
if not ok then fail(chunk) end
local function report_traceback(message)
   local trace = async_traceback(message) or debug.traceback(tostring(message), 2)
   if manifest.entry.mapped then
      -- Error text from the VM uses real generated paths. Convert only locations
      -- belonging to this bundle; leave helper and external frames unchanged.
      for generated, mapping in pairs(manifest.locations) do
         -- Lua truncates long filenames in short_src. Ask the VM for the
         -- exact display name instead of assuming a fixed LUA_IDSIZE.
         local probe = assert(loadstring("", "@" .. generated))
         local short = debug.getinfo(probe, "S").short_src
         local function replace(name)
            local pattern = name:gsub("(%W)", "%%%1") .. ":(%d+)"
            trace = trace:gsub(pattern, function(line)
               local original = mapping.lines[tonumber(line)]
               return original and (mapping.source .. ":" .. original) or (name .. ":" .. line)
            end)
         end
         replace(generated)
         if short ~= generated then replace(short) end
      end
   end
   return trace
end
local success, message = xpcall(function() chunk(unpack(arguments)) end, report_traceback)
if not success then fail(message) end
