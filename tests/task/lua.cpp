// Embed the same module to run its lifetime paths under ASan/UBSan, including
// lua_close while work is queued or running. The upstream VM is unchanged.
#include "runtime/task/lua_task.cpp"
#include "binder/lua_binder.h"
#include <iostream>

int main() {
    try {
        {
            next::state vm;
            lua_pushcfunction(vm.get(), luaopen_tynex_task);
            if (lua_pcall(vm.get(), 0, 1, 0)) throw next::error(lua_tostring(vm.get(), -1));
            lua_setglobal(vm.get(), "task");
            next::binder(vm).dofile("tests/task/runtime.lua");
        }
        for (int i = 0; i < 20; ++i) {
            next::state vm;
            lua_pushcfunction(vm.get(), luaopen_tynex_task);
            if (lua_pcall(vm.get(), 0, 1, 0)) throw next::error(lua_tostring(vm.get(), -1));
            lua_setglobal(vm.get(), "task");
            next::binder(vm).dostring(R"(
                task.configure(2, 32)
                for i = 1, 20 do
                    task.spawn(function(_, context)
                        while true do context.checkpoint() end
                    end)
                end
            )");
        }
        std::cout << "Lua task VM teardown: passed\n";
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
