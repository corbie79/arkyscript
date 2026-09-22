#include "binder/lua_binder.h"
#include <cstdint>
#include <iostream>

using next::binder;
using next::error;
using next::state;

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
    throw std::runtime_error("expected next::error");
}
struct Counter {
    static inline int live = 0;
    int value = 0;
    const int fixed = 7;
    explicit Counter(int initial = 0) : value(initial) { ++live; }
    Counter(const Counter& other) : value(other.value) { ++live; }
    ~Counter() noexcept { --live; }
    int add(int n) { return value += n; }
    int get() const noexcept { return value; }
    void set(int n) { value = n; }
    int throwing() const { throw std::runtime_error("method failure"); }
};
struct alignas(64) Aligned {
    int value = 99;
    int get() const { return value; }
};
struct Other { int value = 0; };
struct Throws { Throws() { throw std::runtime_error("constructor failure"); } };
struct MoveOnly {
    std::unique_ptr<int> data;
    explicit MoveOnly(int value) : data(std::make_unique<int>(value)) {}
    int get() const { return *data; }
};
int raw(lua_State* L) {
    lua_pushinteger(L, lua_gettop(L));
    return 1;
}
int free_add(int a, int b) noexcept { return a + b; }

void functions_and_values(binder& lua) {
    lua.reg_func("add", free_add);
    lua.reg_func("raw", raw);
    lua.reg_func("capture", [value = std::make_unique<int>(40)](int n) { return *value + n; });
    lua.reg_func("std_function", std::function<int(int)>([](int n) { return n + 1; }));
    lua.reg_func("mutable", [n = 0]() mutable { return ++n; });
    lua.reg_func("length", [](const std::string& value) { return value.size(); });
    lua.reg_func("view_length", [](std::string_view value) { return value.size(); });
    lua.reg_func("bool_id", [](bool value) { return value; });
    lua.reg_func("optional", [](std::optional<int> value) { return value.value_or(8); });
    lua.reg_func("object_echo", [](next::object value) { return value; });
    lua.reg_func("many", [](int a, int b, int c, int d, int e, int f, int g, int h, int i, int j) {
        return a + b + c + d + e + f + g + h + i + j;
    });
    lua.reg_func("failure", [](const std::string&) -> int { throw std::runtime_error("100% failure"); });
    lua.reg_func("unknown_failure", [] { throw 17; });
    check(lua.call<int>("add", 20, 22) == 42, "free function");
    check(lua.call<int>("capture", 2) == 42, "move-only capture");
    check(lua.call<int>("std_function", 41) == 42, "std::function");
    check(lua.call<int>("mutable") == 1 && lua.call<int>("mutable") == 2, "mutable lambda");
    check(lua.call<int>("raw", 1, 2, 3) == 3, "raw callback");
    check(lua.call<int>("many", 1, 2, 3, 4, 5, 6, 7, 8, 9, 10) == 55, "variadic arity");
    const std::string bytes("a\0b", 3);
    lua.set_var("bytes", bytes);
    check(lua.get_var<std::string>("bytes") == bytes, "binary string round trip");
    check(lua.call<int>("length", bytes) == 3 && lua.call<int>("view_length", bytes) == 3, "string arguments");
    check(lua.call<int>("optional", nullptr) == 8, "nil optional argument");
    lua.set_var("empty", std::optional<int>{});
    check(!lua.get_var<std::optional<int>>("empty"), "nil optional result");
    lua.set_var("present", std::optional<int>{42});
    check(lua.get_var<std::optional<int>>("present") == 42, "optional value");
    lua.set_var("maximum", std::int64_t{9007199254740991});
    check(lua.get_var<std::int64_t>("maximum") == 9007199254740991, "safe integer limit");
    fails([&] { lua.set_var("large", std::numeric_limits<std::uint64_t>::max()); }, "safe number range");
    fails([&] { lua.call<int>("add", 1.5, 2); }, "exact integer");
    fails([&] { lua.call<int>("add", "1", 2); }, "number");
    fails([&] { lua.call<int>("add", 1); }, "argument count");
    fails([&] { lua.call<int>("add", 1, 2, 3); }, "argument count");
    fails([&] { lua.call<bool>("bool_id", 1); }, "boolean");
    fails([&] { lua.call<int>("failure", bytes); }, "100% failure");
    fails([&] { lua.call("unknown_failure"); }, "unknown C++ exception");
    fails([&] { (void)lua.get_var<unsigned>("negative"); }, "number");
    lua.set_var("negative", -1);
    fails([&] { (void)lua.get_var<unsigned>("negative"); }, "range");
    lua.set_var("wide", 1000);
    fails([&] { (void)lua.get_var<std::uint8_t>("wide"); }, "range");
    lua.dostring("huge = 2^63; infinity = math.huge; nan = 0/0");
    fails([&] { (void)lua.get_var<std::int64_t>("huge"); }, "exact integer");
    fails([&] { (void)lua.get_var<double>("infinity"); }, "finite");
    fails([&] { (void)lua.get_var<double>("nan"); }, "finite");
    lua.dostring(R"(
        local ok, message = pcall(failure, "value")
        assert(not ok and message:find("100%% failure"))
        function script_add(a, b) return a + b end
    )");
    check(lua.call<int>("script_add", 20, 22) == 42, "Lua function");
    lua.dostring("object_data = { value = 42, nested = { name = 'kept' } }");
    auto object = lua.get_var<next::object>("object_data");
    lua.set_var("object_copy", object);
    auto returned = lua.call<next::object>("object_echo", object);
    check(returned.get_var<int>("value") == 42, "object argument/result through binder");
    lua.dostring("assert(rawequal(object_data, object_copy)); object_data = nil; object_copy = nil; collectgarbage('collect')");
    check(object["nested"].get_var<std::string>("name") == "kept", "object retains nested table through binder");
    lua.dostring("object_array = {{value = 1}, {value = 2}}");
    auto objects = lua.get_var<std::vector<next::object>>("object_array");
    lua.dostring("object_array = nil; collectgarbage('collect')");
    check(objects[0].get_var<int>("value") == 1 && objects[1].get_var<int>("value") == 2, "array of Lua object handles");
}

void classes(binder& lua) {
    auto registration = lua.reg_class<Counter>("Counter");
    registration.reg_con<>().reg_con<int>()
        .reg_func("add", &Counter::add).reg_func("get", &Counter::get)
        .reg_func("throwing", &Counter::throwing)
        .reg_var("value", &Counter::value).reg_var("fixed", &Counter::fixed)
        .reg_prop("amount", &Counter::get, &Counter::set).reg_prop("readonly", &Counter::get);
    registration.reg_var("replaced", &Counter::value).reg_var("replaced", &Counter::fixed);
    fails([&] { registration.reg_con<int>(); }, "already registered");
    fails([&] { registration.reg_var("get", &Counter::value); }, "method/property");
    fails([&] { registration.reg_func("amount", &Counter::get); }, "method/property");
    lua.set_var("occupied", 1);
    fails([&] { lua.reg_class<Other>("occupied.Other"); }, "parent is not a table");
    lua.reg_class<Other>("Other").reg_con<>();
    lua.reg_class<Aligned>("Aligned").reg_con<>().reg_func("get", &Aligned::get);
    lua.reg_class<Throws>("Throws").reg_con<>();
    lua.reg_class<MoveOnly>("MoveOnly").reg_con<int>().reg_func("get", &MoveOnly::get);
    lua.reg_func("make_move", [] { return MoveOnly(42); });
    lua.reg_func("counter_value", [](const Counter& counter) { return counter.value; });
    lua.reg_func("counter_ptr", [](const Counter* counter) { return counter ? counter->value : -1; });
    lua.reg_func("counter_copy", [](Counter counter) { return counter; });
    lua.dostring(R"(
        counter = Counter(20)
        assert(counter:add(22) == 42 and counter:get() == 42)
        assert(counter.value == 42 and counter.amount == 42)
        counter.value = 10
        counter.amount = 21
        assert(counter.readonly == 21 and counter.fixed == 7)
        assert(counter.unknown == nil)
        assert(counter_value(counter) == 21 and counter_ptr(counter) == 21)
        assert(counter_ptr(nil) == -1)
        assert(counter_copy(counter):get() == 21)
        assert(Aligned():get() == 99)
        assert(MoveOnly(42):get() == 42 and make_move():get() == 42)
        assert(getmetatable(counter) == false and getmetatable(Counter) == false)
    )");
    Counter external(5);
    const Counter constant(6);
    lua.set_var("external", &external);
    lua.set_var("constant", &constant);
    lua.set_var("null_counter", static_cast<Counter*>(nullptr));
    lua.dostring("external:add(2); assert(constant:get() == 6); assert(null_counter == nil)");
    check(external.value == 7, "borrowed object mutation");
    fails([&] { lua.dostring("constant:add(1)"); }, "const C++");
    fails([&] { lua.dostring("constant.value = 1"); }, "const C++");
    fails([&] { lua.dostring("constant.amount = 1"); }, "const C++");
    fails([&] { lua.dostring("counter.readonly = 1"); }, "read-only");
    fails([&] { lua.dostring("counter.fixed = 1"); }, "read-only");
    fails([&] { lua.dostring("counter.replaced = 1"); }, "read-only");
    fails([&] { lua.dostring("counter.unknown = 1"); }, "unknown");
    fails([&] { lua.dostring("counter_value(Other())"); }, "wrong or expired");
    fails([&] { lua.dostring("counter_value({})"); }, "wrong or expired");
    fails([&] { lua.dostring("counter_value(nil)"); }, "wrong or expired");
    fails([&] { lua.dostring("counter.get(Other())"); }, "wrong or expired");
    fails([&] { lua.dostring("counter.get(newproxy(true))"); }, "wrong or expired");
    fails([&] { lua.dostring("counter:throwing()"); }, "method failure");
    fails([&] { lua.dostring("Counter(1, 2)"); }, "no constructor");
    fails([&] { lua.dostring("Counter('bad')"); }, "number");
    fails([&] { lua.dostring("Throws()"); }, "constructor failure");
    fails([&] { lua.reg_class<Counter>("Again"); }, "already registered");
    lua.dostring(R"(
        local temporary = Counter()
        local destroy = debug.getmetatable(temporary).__gc
        destroy(temporary)
        destroy(temporary)
        destroy(Other())
        assert(not pcall(function() temporary:get() end))
    )");
    {
        Counter copy = lua.get_var<Counter>("counter");
        check(copy.value == 21, "owned host result");
    }
    lua.dostring("counter = nil; external = nil; constant = nil; collectgarbage('collect')");
    check(Counter::live == 2, "GC destroys owned objects, preserves borrowed objects");
    // A second state must have independent type registration and class names.
    state second;
    binder other(second);
    other.reg_class<Counter>("Different").reg_con<int>().reg_func("get", &Counter::get);
    other.dostring("assert(Different(4):get() == 4); collectgarbage('collect')");
    check(Counter::live == 2, "per-state class registration");
}

void errors_and_stack(binder& lua) {
    auto* L = lua.get_state();
    lua_pushliteral(L, "stack sentinel");
    const int top = lua_gettop(L);
    for (int iteration = 0; iteration < 100; ++iteration) {
        fails([&] { lua.dostring("error('Lua failure')"); }, "Lua failure");
        fails([&] { lua.dostring("error({})"); }, "non-string");
        fails([&] { lua.dostring("function("); }, "tynex");
        fails([&] { lua.call<int>("missing"); }, "function");
        fails([&] { (void)lua.get_var<int>("missing"); }, "number");
        fails([&] { lua.call<int>("failure", "x"); }, "100% failure");
        fails([&] { lua.call<std::string>("add", 1, 2); }, "string");
        fails([&] { lua.dofile("__tynex_nonexistent__.lua"); }, "cannot open");
        check(lua.call<int>("add", 1, 2) == 3, "success after error");
        check(lua_gettop(L) == top && std::string_view(lua_tostring(L, -1)) == "stack sentinel", "stack restoration");
    }
    lua_pop(L, 1);
    lua.set_namespace("app.math");
    lua.reg_func("sum", free_add);
    lua.set_var("value", 42);
    check(lua.call<int>("sum", 20, 22) == 42 && lua.get_var<int>("value") == 42, "namespace");
    lua.set_namespace("");
    check(lua.call<int>("app.math.sum", 20, 22) == 42, "dotted name");
    fails([&] { lua.set_var("app.math.value.child", 1); }, "parent is not a table");
    fails([&] { lua.set_var("bad..name", 1); }, "dotted");
    fails([&] { lua.set_namespace("bad."); }, "dotted");
    fails([&] { binder invalid(nullptr); }, "live Lua state");
    lua.reg_func("reentrant", [&lua](int value) { return lua.call<int>("add", value, 1); });
    check(lua.call<int>("reentrant", 41) == 42, "nested protected calls");
}

void shared_ownership() {
    std::shared_ptr<Counter> retained;
    std::weak_ptr<Counter> weak;
    {
        Counter borrowed(5);
        state owner;
        binder lua(owner);
        lua.reg_class<Counter>("Counter").reg_con<int>()
            .reg_func("add", &Counter::add).reg_func("get", &Counter::get);
        lua.reg_func("shared_echo", [](std::shared_ptr<Counter> counter) { return counter; });
        lua.reg_func("array_sum", [](const std::vector<int>& numbers) {
            int sum = 0;
            for (int n : numbers) sum += n;
            return sum;
        });
        lua.reg_func("array_return", [] { return std::vector<int>{10, 20, 12}; });
        check(lua.call<int>("array_sum", std::vector<int>{10, 20, 12}) == 42, "vector callback argument");
        check(lua.call<std::vector<int>>("array_return") == std::vector<int>({10, 20, 12}), "vector callback result");
        lua.dostring("assert(array_sum(array_return()) == 42)");

        auto shared = std::make_shared<Counter>(40);
        weak = shared;
        lua.set_var("shared", shared);
        shared.reset();
        check(!weak.expired(), "Lua keeps shared ownership");
        auto host = lua.get_var<std::shared_ptr<Counter>>("shared");
        check(host.get() == weak.lock().get(), "shared identity is preserved");
        lua.dostring("shared_echo(shared):add(2)");
        check(host->value == 42, "shared reference mutates original object");
        lua.set_var("readonly", std::shared_ptr<const Counter>(host));
        check(lua.get_var<std::shared_ptr<const Counter>>("readonly")->value == 42, "const shared result");
        fails([&] { (void)lua.get_var<std::shared_ptr<Counter>>("readonly"); }, "const C++");
        fails([&] { lua.dostring("readonly:add(1)"); }, "const C++");
        fails([&] { lua.dostring("shared_echo(readonly)"); }, "const C++");
        fails([&] { (void)lua.get_var<std::shared_ptr<Other>>("shared"); }, "wrong or expired");
        lua.set_var("borrowed", &borrowed);
        fails([&] { (void)lua.get_var<std::shared_ptr<Counter>>("borrowed"); }, "no shared ownership");
        lua.set_var("empty_shared", std::shared_ptr<Counter>{});
        check(!lua.get_var<std::shared_ptr<Counter>>("empty_shared"), "empty shared pointer round trip");
        check(!lua.call<std::shared_ptr<Counter>>("shared_echo", nullptr), "nil shared argument and result");
        lua.set_var("shared_array", std::vector<std::shared_ptr<Counter>>{host});
        check(lua.get_var<std::vector<std::shared_ptr<Counter>>>("shared_array")[0] == host, "shared pointer array");

        lua.dostring("created = Counter(7)");
        retained = lua.get_var<std::shared_ptr<Counter>>("created");
        lua.dostring("created = nil; collectgarbage('collect')");
        check(retained->value == 7, "host ownership survives Lua GC");
        check(lua_gettop(owner.get()) == 0, "shared conversion stack balance");
    }
    check(weak.expired(), "Lua close releases its shared ownership");
    check(Counter::live == 1 && retained->value == 7, "host ownership survives Lua state close");
    retained.reset();
    check(Counter::live == 0, "shared object destroyed after last owner");
}
} // namespace

int main() {
    try {
        std::weak_ptr<int> captured;
        {
            state owner;
            binder lua(owner);
            functions_and_values(lua);
            classes(lua);
            errors_and_stack(lua);
            auto value = std::make_shared<int>(42);
            captured = value;
            lua.reg_func("owned_capture", [value = std::move(value)] { return *value; });
            check(!captured.expired(), "Lua retains callback captures");
            lua.dostring("retained = Counter(42)");
            check(Counter::live == 1, "Lua retains owned objects until close");
            check(lua_gettop(owner.get()) == 0, "final stack balance");
        }
        check(Counter::live == 0, "state closes all owned objects");
        check(captured.expired(), "state closes callback captures");
        shared_ownership();
        std::cout << "Tynex binder: " << checks << " checks passed\n";
    } catch (const std::exception& exception) {
        std::cerr << "Tynex binder test failed: " << exception.what() << '\n';
        return 1;
    }
}
