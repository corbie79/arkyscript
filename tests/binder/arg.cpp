// Deliberately include only lua_arg.h: conversions are usable without the binder.
#include "binder/lua_arg.h"
#include <iostream>

using tynex::carg;
using tynex::pusher;
using tynex::result;

namespace {
int checks = 0;
void check(bool condition, const char* message) {
    ++checks;
    if (!condition) throw std::runtime_error(message);
}
template<class F> void fails(F&& action, std::string_view expected) {
    try { action(); }
    catch (const tynex::error& exception) {
        check(std::string_view(exception.what()).find(expected) != std::string_view::npos, exception.what());
        return;
    }
    throw std::runtime_error("expected tynex::error");
}
}

int main() {
    try {
        std::unique_ptr<lua_State, decltype(&lua_close)> owner(luaL_newstate(), lua_close);
        if (!owner) throw std::bad_alloc();
        auto* L = owner.get();
        pusher<int>::push(L, 42);
        check(carg<int>(L, -1).to() == 42, "scalar conversion");
        lua_pop(L, 1);
        const std::string bytes("a\0b", 3);
        result<std::string>::push_im(L, bytes);
        carg<const std::string&> text(L, -1);
        lua_pop(L, 1);
        lua_gc(L, LUA_GCCOLLECT, 0);
        check(text.to() == bytes, "carg owns converted string reference");

        pusher<const char*>::push(L, "sentinel");
        const int base = lua_gettop(L);
        const std::vector<int> expected{10, 20, 30};
        pusher<const std::vector<int>&>::push(L, expected);
        check(lua_gettop(L) == base + 1, "vector pushes exactly one table");
        check(carg<std::vector<int>>(L, -1).to() == expected, "negative index ordered conversion");
        check(lua_gettop(L) == base + 1, "vector read leaves its source table");
        lua_pop(L, 1);
        pusher<std::vector<int>>::push(L, {});
        check(lua_gettop(L) == base + 1 && carg<std::vector<int>>(L, -1).to().empty(), "empty vector pushes one table");
        lua_pop(L, 1);

        const std::vector<std::vector<int>> nested{{1, 2}, {}, {3}};
        result<decltype(nested)>::push_im(L, nested);
        check(carg<decltype(nested)>(L, -1).to() == nested, "nested vectors");
        lua_pop(L, 1);
        pusher<std::vector<bool>>::push(L, {true, false, true});
        check(carg<std::vector<bool>>(L, -1).to() == std::vector<bool>({true, false, true}), "vector bool proxy conversion");
        lua_pop(L, 1);
        pusher<std::vector<std::string>>::push(L, {bytes, "second"});
        carg<const std::vector<std::string>&> strings(L, -1);
        lua_pop(L, 1);
        lua_gc(L, LUA_GCCOLLECT, 0);
        check(strings.to() == std::vector<std::string>({bytes, "second"}), "carg owns converted vector reference");

        lua_newtable(L);
        lua_pushinteger(L, 30); lua_rawseti(L, -2, 3);
        fails([&] { (void)carg<std::vector<int>>(L, -1).to(); }, "dense Lua array");
        check(lua_gettop(L) == base + 1, "sparse array failure restores stack");
        lua_pop(L, 1);
        lua_newtable(L);
        lua_pushinteger(L, 1); lua_setfield(L, -2, "named");
        fails([&] { (void)carg<std::vector<int>>(L, -1).to(); }, "dense Lua array");
        check(lua_gettop(L) == base + 1, "map rejection restores stack");
        lua_pop(L, 1);
        pusher<std::vector<std::string>>::push(L, {"not a number"});
        fails([&] { (void)carg<std::vector<int>>(L, -1).to(); }, "number");
        check(lua_gettop(L) == base + 1, "element error restores stack");
        lua_pop(L, 1);
        fails([&] { pusher<std::vector<std::optional<int>>>::push(L, {1, std::nullopt, 3}); }, "cannot be nil");
        check(lua_gettop(L) == base, "failed vector push restores stack");
        fails([&] { pusher<std::vector<double>>::push(L, {1, std::numeric_limits<double>::infinity()}); }, "finite");
        check(lua_gettop(L) == base, "failed element conversion restores stack");
        check(std::string_view(lua_tostring(L, -1)) == "sentinel", "original stack entry retained");
        lua_pop(L, 1);
        std::cout << "Tynex arguments: " << checks << " checks passed\n";
    } catch (const std::exception& exception) {
        std::cerr << "Tynex argument test failed: " << exception.what() << '\n';
        return 1;
    }
}
