#include "binder/lua_value.h"
#include "binder/lua_binder.h"
#include <iostream>

using next2::binder;
using next2::error;
using next2::object;
using next2::state;
using next2::value;

namespace {
struct Native {
    int score;
    explicit Native(int n) : score(n) {}
    int get() const { return score; }
};
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
    throw std::runtime_error("expected next2::error");
}

void standard_values(binder& lua) {
    value config = value::table{
        {"name", "LuaJIT"}, {"enabled", true}, {"version", 20},
        {"items", value::array{1, "two", false}}, {7, "seven"}, {true, "boolean key"}
    };
    lua.set_var("config", config);
    lua.dostring(R"(
        assert(config.name == 'LuaJIT' and config.enabled and config.version == 20)
        assert(config.items[1] == 1 and config.items[2] == 'two' and config.items[3] == false)
        assert(config[7] == 'seven' and config[true] == 'boolean key')
    )");
    auto snapshot = lua.get_var<value>("config");
    const auto& fields = std::get<value::table>(snapshot.data);
    check(std::get<std::string>(fields.at("name").data) == "LuaJIT", "string-keyed map");
    check(std::get<bool>(fields.at("enabled").data), "boolean value");
    check(std::get<double>(fields.at("version").data) == 20, "Lua number alternative");
    check(std::get<std::string>(fields.at(7).data) == "seven", "integer map key");
    check(std::get<std::string>(fields.at(true).data) == "boolean key", "boolean map key");
    check(lua.get_var<value::table>("config").size() == 6, "direct standard map conversion");
    const auto& items = std::get<value::array>(fields.at("items").data);
    check(items.size() == 3 && std::get<double>(items[0].data) == 1 &&
          std::get<std::string>(items[1].data) == "two" && !std::get<bool>(items[2].data), "heterogeneous ordered array");
    lua.dostring("config.name = 'changed'");
    check(std::get<std::string>(fields.at("name").data) == "LuaJIT", "plain tables are independent snapshots");
    lua.set_var("snapshot", snapshot);
    lua.dostring("assert(snapshot.name == 'LuaJIT')");
    const std::string bytes("a\0b", 3);
    lua.set_var("bytes", value(bytes));
    check(std::get<std::string>(lua.get_var<value>("bytes").data) == bytes, "binary string value");
    lua.set_var("nothing", value{});
    check(std::holds_alternative<std::monostate>(lua.get_var<value>("nothing").data), "nil value");
    lua.set_var("empty", value::array{});
    check(std::get<value::table>(lua.get_var<value>("empty").data).empty(), "empty Lua table defaults to map");
    lua.dostring("sparse = {[1] = 'first', [3] = 'third'}");
    check(std::get<value::table>(lua.get_var<value>("sparse").data).size() == 2, "sparse table retains integer keys");
    lua.reg_func("echo_value", [](value input) { return input; });
    auto echoed = lua.call<value>("echo_value", snapshot);
    check(std::get<std::string>(std::get<value::table>(echoed.data).at("name").data) == "LuaJIT", "dynamic callback argument and result");
    lua.reg_func("value_name", [](const value& input) { return std::get<std::string>(std::get<value::table>(input.data).at("name").data); });
    check(lua.call<std::string>("value_name", snapshot) == "LuaJIT", "const dynamic callback argument");
    lua.set_var("exact", value(std::uint64_t{9007199254740991}));
    check(lua.get_var<double>("exact") == 9007199254740991.0, "safe integer value");
    fails([&] { lua.set_var("too_large", value(std::numeric_limits<std::uint64_t>::max())); }, "safe number range");
    fails([&] { lua.set_var("not_finite", value(std::numeric_limits<double>::infinity())); }, "finite");
    fails([&] { lua.set_var("nil_array", value::array{1, nullptr}); }, "cannot be nil");
    fails([&] { lua.set_var("nil_map", value::table{{"missing", nullptr}}); }, "cannot be nil");
    lua.dostring("not_finite = math.huge");
    fails([&] { (void)lua.get_var<value>("not_finite"); }, "finite");
    check(lua_gettop(lua.get_state()) == 0, "value error stack restoration");
}

void reference_preservation(binder& lua) {
    lua.reg_class<Native>("Native").reg_con<int>().reg_func("get", &Native::get);
    lua.dostring(R"(
        cycle = {}; cycle.self = cycle
        local child = { score = 42 }
        aliases = { left = child, right = child }
        meta = setmetatable({ score = 10 }, {__index = function() return 42 end})
        table_key = {}; keyed = {[table_key] = 'kept'}
        fractional = {[1.5] = 'kept'}
        function dynamic_function(a, b) return a + b end
        with_function = { run = dynamic_function }
        native = Native(42)
        cdata = require('ffi').new('int[1]', {42})
    )");
    for (const char* name : {"cycle", "aliases", "meta", "keyed", "fractional"}) {
        auto v = lua.get_var<value>(name);
        check(std::holds_alternative<object>(v.data), "non-tree or special table retains Lua reference");
        lua.set_var("copy", v);
        lua.set_var("original", lua.get_var<object>(name));
        lua.dostring("assert(rawequal(copy, original))");
    }
    lua.set_var("cycle_copy", lua.get_var<value>("cycle"));
    lua.set_var("aliases_copy", lua.get_var<value>("aliases"));
    lua.set_var("meta_copy", lua.get_var<value>("meta"));
    lua.dostring(R"(
        assert(rawequal(cycle_copy, cycle_copy.self))
        assert(rawequal(aliases_copy.left, aliases_copy.right))
        assert(meta_copy.missing == 42)
    )");
    auto function = lua.get_var<value>("dynamic_function");
    check(std::get<object>(function.data).invoke<int>(20, 22) == 42, "function value retains callable handle");
    auto table = lua.get_var<value>("with_function");
    check(std::holds_alternative<value::table>(table.data), "plain table can contain reference-valued leaves");
    lua.set_var("table_copy", table);
    lua.dostring("assert(table_copy.run(20, 22) == 42)");
    auto native = lua.get_var<value>("native");
    check(std::get<object>(native.data).call_method<int>("get") == 42, "native userdata in standard value");
    lua.set_var("native_copy", native);
    lua.set_var("cdata_copy", lua.get_var<value>("cdata"));
    lua.dostring("assert(rawequal(native, native_copy)); assert(cdata_copy == cdata and cdata_copy[0] == 42)");
    check(std::holds_alternative<object>(lua.get_var<value>("cdata").data), "LuaJIT cdata retains reference");
    state another;
    binder other(another);
    other.set_var("plain", value::table{{"answer", 42}});
    other.dostring("assert(plain.answer == 42)");
    fails([&] { other.set_var("foreign", function); }, "different VMs");
    check(lua_gettop(another.get()) == 0, "reference-valued conversion failure restores stack");
}

void conversion_limits(binder& lua) {
    value deep = 1;
    for (int i = 0; i < 66; ++i) deep = value::array{std::move(deep)};
    fails([&] { lua.set_var("deep", deep); }, "conversion limit");
    lua.dostring("deep = {}; local p = deep; for i = 1, 66 do p.child = {}; p = p.child end");
    fails([&] { (void)lua.get_var<value>("deep"); }, "conversion limit");
    check(lua_gettop(lua.get_state()) == 0, "deep conversion failures restore stack");
}
} // namespace

int main() {
    try {
        value retained;
        {
            state vm;
            binder lua(vm);
            standard_values(lua);
            reference_preservation(lua);
            conversion_limits(lua);
            retained = lua.get_var<value>("cycle");
        }
        check(!std::get<object>(retained.data).validate(), "dynamic references detect VM closure");
        retained = nullptr;
        std::cout << "Tynex standard values: " << checks << " checks passed\n";
    } catch (const std::exception& exception) {
        std::cerr << "Tynex value test failed: " << exception.what() << '\n';
        return 1;
    }
}
