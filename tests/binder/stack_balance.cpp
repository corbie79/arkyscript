#include "binder/lua_binder.h"
#include <iostream>

using namespace next2::luax;

namespace sample {
struct Pair { int first, second; };
struct Fault { int mode; };
struct Native { int id = 42; };
}

namespace next2::luax {
template<> struct converter<sample::Pair> {
    static constexpr bool enabled = true;
    static sample::Pair read(lua_State* L, int index) {
        lua_getfield(L, index, "first");
        const int first = carg<int>(L, -1).to();
        lua_pop(L, 1);
        lua_getfield(L, index, "second");
        const int second = carg<int>(L, -1).to();
        lua_pop(L, 1);
        return {first, second};
    }
    static void push(lua_State* L, sample::Pair value) {
        lua_newtable(L);
        lua_pushinteger(L, value.first); lua_setfield(L, -2, "first");
        lua_pushinteger(L, value.second); lua_setfield(L, -2, "second");
    }
};
template<> struct converter<sample::Fault> {
    static constexpr bool enabled = true;
    static sample::Fault read(lua_State* L, int index) {
        const int mode = carg<int>(L, index).to();
        lua_pushliteral(L, "conversion temporary");
        if (mode == 1) throw error("read failed");
        return {mode}; // Deliberately leaves a temporary on the stack.
    }
    static void push(lua_State* L, sample::Fault value) {
        lua_pushliteral(L, "conversion temporary");
        if (value.mode == 1) throw error("push failed");
        lua_pushliteral(L, "extra result"); // Deliberately breaks the one-result contract.
    }
};
}

namespace {
int checks = 0;
constexpr int repetitions = 64;
void check(bool condition, const char* message) {
    ++checks;
    if (!condition) throw std::runtime_error(message);
}

void mixed_arguments_and_errors() {
    state vm;
    binder lua(vm);
    auto* L = vm.get();
    lua.reg_class<sample::Native>("Native").reg_var("id", &sample::Native::id);
    sample::Native native;
    lua.set_var("native", &native);
    int calls = 0;
    lua.reg_func("mixed", [&](int first, const sample::Pair& point,
                             const std::vector<std::vector<int>>& rows, sample::Native* self,
                             std::string text, std::optional<sample::Pair> optional,
                             object callback, int last) {
        if (first != 11 || point.first != 2 || point.second != 3
            || rows != std::vector<std::vector<int>>{{4, 5}, {6}}
            || self != &native || text != "seven" || !optional
            || optional->first != 8 || optional->second != 9 || last != 99)
            throw error("mixed argument positions changed");
        if (callback.invoke<int>(last) != 100) throw error("nested callback argument changed");
        ++calls;
        return sample::Pair{first, last};
    });
    lua.reg_func("bad_read", [](int, sample::Fault, int) { return 0; });
    lua.reg_func("bad_push", [](int, sample::Pair, int) { return sample::Fault{1}; });
    lua.reg_func("bad_push_count", [](int, sample::Pair, int) { return sample::Fault{2}; });

    // Keep caller slots below the protected call. Every error must leave their
    // values and table identity intact before the following successful call.
    lua_pushliteral(L, "sentinel");
    lua_pushinteger(L, 123);
    lua_newtable(L);
    const void* sentinel = lua_topointer(L, 3);
    lua.dostring(R"(
        failures = 0
        function expect_failure(expected, fn, ...)
            local ok, message = pcall(fn, ...)
            assert(not ok and tostring(message):find(expected, 1, true), tostring(message))
            failures = failures + 1
        end
    )");
    for (int i = 0; i < repetitions; ++i) {
        lua.dostring(R"(
            expect_failure('read failed', bad_read, 1, 1, 3)
            expect_failure('preserve', bad_read, 1, 2, 3)
            expect_failure('push failed', bad_push, 1, {first=2, second=3}, 4)
            expect_failure('exactly one', bad_push_count, 1, {first=2, second=3}, 4)
            local point = mixed(11, {first=2, second=3}, {{4,5},{6}}, native, 'seven',
                                {first=8, second=9}, function(n) return n+1 end, 99)
            assert(point.first == 11 and point.second == 99)
            expect_failure('number', mixed, 11, {first=2, second='bad'}, {{4,5},{6}},
                           native, 'seven', {first=8, second=9}, function(n) return n+1 end, 99)
            expect_failure('number', mixed, 11, {first=2, second=3}, {{4,5},{'bad'}},
                           native, 'seven', {first=8, second=9}, function(n) return n+1 end, 99)
            expect_failure('expired', mixed, 11, {first=2, second=3}, {{4,5},{6}},
                           false, 'seven', {first=8, second=9}, function(n) return n+1 end, 99)
            local co = coroutine.create(function()
                local result = mixed(11, {first=2, second=3}, {{4,5},{6}}, native, 'seven',
                                     {first=8, second=9}, function(n) return n+1 end, 99)
                assert(result.first == 11 and result.second == 99)
            end)
            assert(coroutine.resume(co))
        )");
        check(lua_gettop(L) == 3 && lua_type(L, 1) == LUA_TSTRING
              && std::string_view(lua_tostring(L, 1)) == "sentinel"
              && lua_tointeger(L, 2) == 123 && lua_topointer(L, 3) == sentinel,
              "success, nested callbacks, coroutines, and errors preserve caller slots");
    }
    check(calls == repetitions * 2 && lua.get_var<int>("failures") == repetitions * 7,
          "every conversion failure is followed by successful argument dispatch");

    // The source sits below unrelated tail values. Recursive conversion must
    // normalize -3 once before pushing its own table keys and element values.
    pusher<std::vector<sample::Pair>>::push(L, {{12, 13}, {14, 15}});
    const void* input = lua_topointer(L, 4);
    lua_pushinteger(L, 456);
    lua_pushliteral(L, "tail");
    for (int i = 0; i < repetitions; ++i) {
        const auto values = carg<std::optional<std::vector<sample::Pair>>>(L, -3).to();
        check(values && values->size() == 2 && values->at(0).first == 12 && values->at(1).second == 15,
              "nested conversion reads the original non-top negative index");
        check(lua_gettop(L) == 6 && lua_topointer(L, 4) == input && lua_tointeger(L, 5) == 456
              && lua_type(L, 6) == LUA_TSTRING && std::string_view(lua_tostring(L, 6)) == "tail",
              "negative-index reads preserve source and tail slots");
    }
}
}

int main() {
    try {
        mixed_arguments_and_errors();
        std::cout << "stack balance: " << checks << " checks passed\n";
    } catch (const std::exception& exception) {
        std::cerr << exception.what() << '\n';
        return 1;
    }
}
