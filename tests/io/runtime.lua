local function finish(request)
   while not native_io._async_ready(request) do native_io._async_sleep(1) end
   return native_io._async_wait(request)
end
local file = assert(io.open('tests/io/runtime.lua', 'rb'))
local expected = file:read('*a')
file:close()
local request = native_io.read_file('tests/io/runtime.lua')
assert(finish(request) == expected)
assert(native_io._async_wait(request) == expected)
assert(not native_io._async_cancel(request))
assert(not pcall(finish, native_io.read_file('missing-io-test-file')))
assert(not pcall(finish, native_io.read_file('tests/io/runtime.lua', 1)))
assert(not pcall(finish, native_io.read_file('.')))
assert(not pcall(finish, native_io.connect('localhost', 1, 0)))
local cancelled = native_io.read_file('tests/io/runtime.lua')
assert(native_io._async_cancel(cancelled))
assert(native_io._async_ready(cancelled))
assert(not native_io._async_cancel(cancelled))
assert(not pcall(native_io._async_wait, cancelled))

-- Bad receiver/tag/size and repeated finalizers must be harmless.
local meta = debug.getmetatable(request)
for _, value in ipairs({{}, true, 42, 'x', newproxy(true)}) do
   assert(not pcall(native_io._async_ready, value))
   assert(not pcall(native_io._async_wait, value))
   assert(not pcall(native_io._async_cancel, value))
   assert(not pcall(native_io.read, value, 1))
   assert(not pcall(native_io.write, value, 'x'))
   assert(not pcall(native_io.close, value))
end
local fake = newproxy(true)
debug.setmetatable(fake, meta)
assert(not pcall(native_io._async_ready, fake))
assert(not pcall(meta.__gc, fake))
debug.setmetatable(fake, nil)
meta.__gc(request)
meta.__gc(request)
assert(not pcall(native_io._async_ready, request))
assert(not pcall(native_io._async_wait, request))
assert(not pcall(native_io._async_cancel, request))

local weak = setmetatable({}, {__mode='v'})
for i=1,128 do
   local item = native_io.read_file('tests/io/runtime.lua')
   native_io._async_cancel(item)
   weak[i] = item
end
collectgarbage('collect') collectgarbage('collect')
assert(next(weak) == nil)
print('Native I/O results, validation, cancellation and GC: passed')
