// Run native I/O ownership and validation under the same sanitizer as the binder.
#include "runtime/io/lua_io.cpp"
#include "binder/lua_binder.h"
#include <cassert>
#include <iostream>

struct failing_allocator {
    lua_Alloc original;
    void* context;
    int remaining = 0;
    bool fired = false;
};
void* allocate_with_failure(void* opaque, void* pointer, std::size_t old_size, std::size_t new_size) {
    auto& allocator = *static_cast<failing_allocator*>(opaque);
    if (new_size && allocator.remaining > 0 && --allocator.remaining == 0) {
        allocator.fired = true;
        return nullptr;
    }
    return allocator.original(allocator.context, pointer, old_size, new_size);
}
void allocation_failures() {
    int injected = 0;
    for (int fail_at = 1; fail_at <= 50; ++fail_at) {
        next2::luax::state vm;
        lua_State* L = vm.get();
        lua_pushcfunction(L, luaopen_tynex_io);
        if (lua_pcall(L, 0, 1, 0)) throw next2::luax::error(lua_tostring(L, -1));
        lua_setglobal(L, "native_io");
        lua_getglobal(L, "native_io");
        lua_getfield(L, -1, "read_file");
        assert(lua_getupvalue(L, -1, 1));
        auto state = checked<owner_state>(L, -1, owner_type);
        lua_settop(L, 0);
        assert(luaL_loadstring(L, "return native_io.read_file('tests/io/runtime.lua')") == 0);
        failing_allocator allocator{};
        allocator.original = lua_getallocf(L, &allocator.context);
        allocator.remaining = fail_at;
        lua_setallocf(L, allocate_with_failure, &allocator);
        (void)lua_pcall(L, 0, 1, 0);
        lua_setallocf(L, allocator.original, allocator.context);
        injected += allocator.fired ? 1 : 0;
        lua_settop(L, 0);
        lua_gc(L, LUA_GCCOLLECT, 0);
        lua_gc(L, LUA_GCCOLLECT, 0);
        assert(state->outstanding == 0);
    }
    assert(injected > 0);
    std::cout << "I/O allocation failure cleanup: " << injected << " injection points passed\n";
}

int main() {
    try {
        allocation_failures();
        {
            next2::luax::state vm;
            lua_pushcfunction(vm.get(), luaopen_tynex_io);
            if (lua_pcall(vm.get(), 0, 1, 0)) throw next2::luax::error(lua_tostring(vm.get(), -1));
            lua_setglobal(vm.get(), "native_io");
            next2::luax::binder(vm).dofile("tests/io/runtime.lua");
        }
        // Teardown with live file and DNS jobs must cancel/join before native state
        // disappears; their jobs never retain a Lua state or a Lua reference.
        for (int i = 0; i < 10; ++i) {
            next2::luax::state vm;
            lua_pushcfunction(vm.get(), luaopen_tynex_io);
            if (lua_pcall(vm.get(), 0, 1, 0)) throw next2::luax::error(lua_tostring(vm.get(), -1));
            lua_setglobal(vm.get(), "native_io");
            next2::luax::binder(vm).dostring(R"(
                local pending = {}
                for i = 1, 32 do
                    pending[#pending+1] = native_io.read_file('tests/io/runtime.lua')
                end
                pending[#pending+1] = native_io.connect('localhost', 1, 0)
                _G.pending_io = pending
            )");
        }
        std::cout << "Native I/O validation and VM teardown: passed\n";
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
