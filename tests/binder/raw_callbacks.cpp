#include "binder/lua_binder.h"
#include <iostream>

using next2::luax::binder;
using next2::luax::error;
using next2::luax::object;
using next2::luax::state;

namespace {
int checks = 0;
void check(bool condition, const char* message) {
    ++checks;
    if (!condition) throw std::runtime_error(message);
}
template<class F> void fails(F&& action, std::string_view expected) {
    try { action(); }
    catch (const error& exception) {
        check(std::string_view(exception.what()).find(expected) != std::string_view::npos, exception.what());
        return;
    }
    throw std::runtime_error("expected next2::luax::error");
}
int echo(lua_State* L) { return lua_gettop(L); }
int echo_noexcept(lua_State* L) noexcept { return lua_gettop(L); }
int typed_noexcept(int a, int b) noexcept { return a * 100 + b; }

void raw_registration() {
    state vm;
    binder lua(vm);
    auto* L = vm.get();
    lua_pushliteral(L, "prefix");
    lua_pushinteger(L, 753);
    lua_pushnil(L);
    lua_newtable(L);
    const void* table = lua_topointer(L, -1);
    const auto stack_intact = [&] {
        check(lua_gettop(L) == 4 && lua_type(L, 1) == LUA_TSTRING &&
              std::string_view(lua_tostring(L, 1)) == "prefix" &&
              lua_tointeger(L, 2) == 753 && lua_isnil(L, 3) && lua_topointer(L, 4) == table,
              "raw registration/call preserves every caller stack slot");
    };

    lua.set_namespace("callbacks");
    lua.reg_func("normal", echo);
    lua.reg_func("noexcept_name", echo_noexcept);
    auto normal_pointer = &echo;
    auto noexcept_pointer = &echo_noexcept;
    const auto const_pointer = noexcept_pointer;
    lua.reg_func("normal_pointer", normal_pointer);
    lua.reg_func("noexcept_pointer", noexcept_pointer);
    lua.reg_func("const_pointer", const_pointer);
    lua.reg_func("moved_pointer", std::move(noexcept_pointer));
    lua.reg_func("lambda", [](lua_State* active) -> int { return lua_gettop(active); });
    lua.reg_func("noexcept_lambda", [](lua_State* active) noexcept -> int { return lua_gettop(active); });
    const auto lambda = [](lua_State* active) -> int { return lua_gettop(active); };
    lua.reg_func("lambda_lvalue", lambda);
    lua.reg_func("lambda_pointer", +[](lua_State* active) noexcept -> int { return lua_gettop(active); });
    lua.set_namespace("");
    stack_intact();
    lua.dostring(R"(
        function verify_raw(f)
            local marker = {}
            local function check_values(...)
                assert(select('#', ...) == 6)
                local a, b, c, d, e, g = ...
                assert(a == marker and b == 317 and c == nil and d == 'tail' and e == false and g == nil)
            end
            assert(select('#', f()) == 0)
            assert(select('#', f(nil)) == 1)
            assert(f(923) == 923)
            check_values(f(marker, 317, nil, 'tail', false, nil))
            local receiver = { call = f }
            local self, a, b = receiver:call(317, 'tail')
            assert(self == receiver and a == 317 and b == 'tail')
            local co = coroutine.create(function()
                check_values(f(marker, 317, nil, 'tail', false, nil))
                return f(481)
            end)
            local ok, result = coroutine.resume(co)
            assert(ok and result == 481)
        end
    )");
    for (const char* name : {"normal", "noexcept_name", "normal_pointer", "noexcept_pointer",
                             "const_pointer", "moved_pointer", "lambda", "noexcept_lambda",
                             "lambda_lvalue", "lambda_pointer"}) {
        const std::string path = std::string("callbacks.") + name;
        lua.call("verify_raw", lua.get_var<object>(path));
        check(lua.call<int>(path, 623, nullptr, "tail") == 623, "host raw call reads first result");
        stack_intact();
    }

    // Typed functions must still convert their arguments and return a value,
    // rather than treating the returned integer as a Lua result count.
    lua.reg_func("typed_noexcept", typed_noexcept);
    lua.reg_func("typed_lambda", [](int a, int b) { return a * 100 + b; });
    lua.reg_func("typed_capture", [scale = 100](int a, int b) { return a * scale + b; });
    lua.reg_func("typed_std_function", std::function<int(int, int)>(typed_noexcept));
    for (const char* name : {"typed_noexcept", "typed_lambda", "typed_capture", "typed_std_function"}) {
        check(lua.call<int>(name, 17, 29) == 1729, "typed callable retains value conversion");
        fails([&] { lua.call<int>(name, 17); }, "argument count");
        stack_intact();
    }

    int (*null_noexcept)(lua_State*) noexcept = nullptr;
    fails([&] { lua.reg_func("null_noexcept", null_noexcept); }, "null function");
    fails([&] { lua.reg_func("null_normal", static_cast<lua_CFunction>(nullptr)); }, "null function");
    fails([&] { lua.reg_func("null_literal", nullptr); }, "null function");
    lua.reg_func("raw_error", [](lua_State* active) -> int {
        lua_pushliteral(active, "temporary");
        lua_pushliteral(active, "raw failure");
        return lua_error(active);
    });
    lua.reg_func("reentrant", [&lua](int left, int right) {
        fails([&] { lua.call("raw_error", left, nullptr, right); }, "raw failure");
        return lua.call<int>("typed_noexcept", left, right);
    });
    for (int i = 0; i < 32; ++i) {
        fails([&] { lua.call("raw_error", 11, nullptr, 73); }, "raw failure");
        check(lua.call<int>("callbacks.noexcept_name", 931, nullptr, 47) == 931, "raw call after Lua error");
        check(lua.call<int>("reentrant", 17, 29) == 1729, "nested failure preserves outer arguments");
        stack_intact();
    }
}
} // namespace

int main() {
    try {
        raw_registration();
        std::cout << "Raw callbacks: " << checks << " checks passed\n";
    } catch (const std::exception& exception) {
        std::cerr << "Raw callback test failed: " << exception.what() << '\n';
        return 1;
    }
}
