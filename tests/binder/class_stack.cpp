#include "binder/lua_binder.h"
#include <iostream>
#include <optional>
#include <string>

using namespace next2::luax;

namespace {
int checks = 0;
void check(bool condition, const char* message) {
    ++checks;
    if (!condition) throw std::runtime_error(message);
}
struct Record {
    static inline int constructed = 0;
    int first = 1, middle = 2, last = 3;
    Record() { ++constructed; }
    Record(int a, std::optional<int> b, int c) : first(a), middle(b.value_or(-1)), last(c) {
        if (a == -99) throw error("constructor failure");
        ++constructed;
    }
    virtual ~Record() = default;
    int combine(int a, std::optional<int> b, int c) const {
        if (a == -99) throw error("method failure");
        return first * 100000 + a * 1000 + b.value_or(0) * 100 + c;
    }
    int get() const {
        if (first == -99) throw error("getter failure");
        return first;
    }
    void set(int value) {
        if (value == -99) throw error("setter failure");
        first = value;
    }
};
struct Prefix { virtual ~Prefix() = default; int unrelated = 70; };
struct Derived : Prefix, Record {};

void verify_prefix(lua_State* L) {
    check(lua_gettop(L) == 4, "class operation changed the caller stack height");
    check(lua_tointeger(L, 1) == 12345, "class operation changed the number sentinel");
    check(lua_isnil(L, 2), "class operation changed the nil sentinel");
    check(lua_type(L, 3) == LUA_TSTRING && std::string_view(lua_tostring(L, 3)) == "preserved",
          "class operation changed the string sentinel");
    lua_rawgeti(L, 4, 1);
    check(lua_tointeger(L, -1) == 99, "class operation changed the table sentinel");
    lua_pop(L, 1);
}
void run(lua_State* L, const char* source, bool success = true) {
    verify_prefix(L);
    check(luaL_loadstring(L, source) == 0, "class stack test source did not compile");
    const int result = lua_pcall(L, 0, LUA_MULTRET, 0);
    if (success && result != 0) throw std::runtime_error(lua_tostring(L, -1));
    check((result == 0) == success, "class stack test did not report the expected status");
    if (!success) {
        check(lua_gettop(L) == 5, "Lua pcall failure did not leave exactly one error");
        lua_pop(L, 1);
    }
    verify_prefix(L);
}
}

int main() {
    try {
        state vm;
        auto* L = vm.get();
        binder lua(vm);
        lua_pushinteger(L, 12345); lua_pushnil(L); lua_pushliteral(L, "preserved");
        lua_newtable(L); lua_pushinteger(L, 99); lua_rawseti(L, -2, 1);
        auto record = lua.reg_class<Record>("Record");
        record.enable_instance_fields(instance_field_policy::allow_method_overrides)
            .reg_con<>().reg_con<int, std::optional<int>, int>()
            .reg_var("first", &Record::first).reg_var("middle", &Record::middle).reg_var("last", &Record::last)
            .reg_prop("property", &Record::get, &Record::set)
            .reg_func("combine", &Record::combine)
            .reg_raw_method("echo", +[](lua_State* caller) -> int {
                const int count = lua_gettop(caller);
                (void)carg<Record*>(caller, 1).to();
                if (lua_gettop(caller) != count) return luaL_error(caller, "receiver conversion changed stack");
                lua_pushinteger(caller, count);
                for (int i = 1; i <= count; ++i) lua_pushvalue(caller, i);
                return count + 1;
            })
            .reg_raw_method("raise", +[](lua_State* caller) -> int {
                if (lua_gettop(caller) != 2) return luaL_error(caller, "raise expects self and error object");
                lua_pushvalue(caller, 2);
                return lua_error(caller);
            });
        verify_prefix(L);
        lua.reg_class<Derived>("Derived").reg_base<Record>().reg_con<>();
        verify_prefix(L);
        run(L, R"(
            plain = Record()
            object = Record(11, nil, 33)
            derived = Derived()
            assert(plain.first == 1 and plain.middle == 2 and plain.last == 3)
            assert(object.first == 11 and object.middle == -1 and object.last == 33)
            assert(object:combine(7, nil, 9) == 1107009)
            assert(object.combine(object, 7, 8, 9) == 1107809)
            local detached = object.combine
            assert(detached(object, 7, nil, 9) == 1107009)
            derived.property = 12
            assert(derived.first == 12 and derived.property == 12)
            assert(derived:combine(7, nil, 9) == 1207009)
            local count, same, a, b, c = derived:echo(nil, 'middle', nil)
            assert(count == 4 and rawequal(same, derived) and a == nil and b == 'middle' and c == nil)
            assert(select('#', derived:echo(nil, 'middle', nil)) == 5)
            assert(object.missing == nil and derived.missing == nil)
            object.peer = 'peer'; assert(object.peer == 'peer' and derived.peer == nil)
            object.peer = nil; assert(object.peer == nil)
            local original = object.combine
            object.combine = function(self, ...) return original(self, ...) end
            assert(object:combine(7, nil, 9) == 1107009)
            object.combine = nil
            assert(object.combine == original and object:combine(7, nil, 9) == 1107009)
            local marker = {}
            local ok, failure = pcall(function() object:raise(marker) end)
            assert(not ok and failure == marker)
        )");
        const int constructed = Record::constructed;
        for (const char* source : {
                "Record(1)", "Record(1,2)", "Record(1,2,3,4)",
                "Record('wrong',nil,3)", "Record(1,{},3)", "Record(1,nil,'wrong')", "Record(-99,nil,3)",
                "object:combine()", "object.combine(7,nil,9)", "object:combine(1,nil,{})",
                "object:combine(-99,nil,3)", "object.property = -99", "object.property = {}",
                "object.combine = 12", "object.property = function() end", "object[nil] = 12"
            }) {
            run(L, source, false);
            run(L, "assert(object:combine(7,nil,9)==1107009 and derived.property==12)");
        }
        check(Record::constructed == constructed, "failed constructors published or constructed an instance");
        run(L, "object.first=-99");
        run(L, "local ignored=object.property", false);
        run(L, "object.first=11; assert(object.property==11)");
        // Repeated failures share one VM and a nonempty host stack. Lua calls
        // must discard their temporary dispatch tables and restore argument slots.
        run(L, R"(
            for n=1,250 do
                local ok = pcall(function() object.property = {} end)
                assert(not ok)
                ok = pcall(function() return object:combine(1,nil,{}) end)
                assert(not ok)
                object.property = n
                assert(object.property == n and object:combine(7,nil,9) == n*100000+7009)
            end
            local co=coroutine.create(function()
                object.property=17
                local count, same, a, b, c=object:echo(nil,19,nil)
                assert(count==4 and same==object and a==nil and b==19 and c==nil)
                assert(object:combine(7,nil,9)==1707009)
                assert(not pcall(function() object.property={} end))
                assert(object.property==17)
            end)
            assert(coroutine.resume(co))
        )");
        lua_settop(L, 0);
        std::cout << checks << " class stack checks plus repeated Lua assertions passed\n";
        return 0;
    } catch (const std::exception& exception) {
        std::cerr << exception.what() << '\n';
        return 1;
    }
}
