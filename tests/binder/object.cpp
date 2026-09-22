// The object and class modules must work without lua_binder.h.
#include "binder/lua_object.h"
#include "binder/lua_class.h"
#include <iostream>

using tynex::object;
using tynex::state;
using tynex::error;

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
    throw std::runtime_error("expected tynex::error");
}
void execute(lua_State* L, const char* source) {
    if (luaL_dostring(L, source) != 0) {
        const std::string message = lua_tostring(L, -1);
        lua_pop(L, 1);
        throw std::runtime_error(message);
    }
}
object global(lua_State* L, const char* name) {
    lua_getglobal(L, name);
    object value(L, -1);
    lua_pop(L, 1);
    return value;
}
struct Counter {
    static inline int live = 0;
    int value;
    explicit Counter(int n) : value(n) { ++live; }
    ~Counter() noexcept { --live; }
    int get() const { return value; }
    int add(int n) { return value += n; }
};

void table_access_and_calls() {
    state vm;
    auto* L = vm.get();
    execute(L, R"(
        data = { value = 10, child = { label = "nested" } }
        function data.sum(a, b) return a + b end
        function data:bump(n) self.value = self.value + n; return self.value end
        function data.fail() error("object call failure") end
        function data.mark() marked = true end
        function data:mark_self() self.marked = true end
        proxy = setmetatable({}, {
            __index = function(_, key)
                if key == "virtual" then return 42 end
                error("getter failure")
            end,
            __newindex = function() error("setter failure") end
        })
        scalar = 42; falsy = false
    )");
    auto data = global(L, "data");
    auto child = data["child"];
    check(data.validate() && child.validate(), "object and nested handle validation");
    execute(L, "data = nil; collectgarbage('collect')");
    check(data.get_var<int>("value") == 10, "handle survives stack pop and global deletion");
    check(child.get_var<std::string>("label") == "nested", "nested handle retains table");
    auto copy = data;
    auto moved = std::move(copy);
    check(!copy.validate() && moved.validate(), "move transfers handle");
    data.reset();
    check(!data.validate() && moved.get_var<int>("value") == 10, "copy retains registry reference");
    moved.set_var("value", 20);
    check(moved.get_var<int>("value") == 20, "field assignment");
    check(moved.call<int>("sum", 20, 22) == 42, "dot function call");
    check(moved.call_method<int>("bump", 22) == 42, "method self argument");
    check(moved["sum"].invoke<int>(20, 22) == 42, "function value invocation");
    moved.call("mark");
    check(global(L, "marked").as<bool>(), "void function call");
    moved.call_method("mark_self");
    check(moved.get_var<bool>("marked"), "void method call");
    moved["mark"].invoke();
    moved.set_var("child_copy", child);
    check(moved["child_copy"].get_var<std::string>("label") == "nested", "handle as field value");
    check(moved["absent"].as<std::optional<int>>() == std::nullopt, "nil handle conversion");
    check(!moved["absent"].validate(), "nil handle validation");
    check(global(L, "scalar").as<int>() == 42, "scalar handle conversion");
    check(global(L, "falsy").validate() && !global(L, "falsy").as<bool>(), "false is a valid Lua value");
    auto proxy = global(L, "proxy");
    check(proxy.get_var<int>("virtual") == 42, "protected index metamethod");
    lua_pushliteral(L, "sentinel");
    const int base = lua_gettop(L);
    for (int n = 0; n < 20; ++n) {
        fails([&] { moved.call("fail"); }, "object call failure");
        fails([&] { moved.call("absent"); }, "nil value");
        fails([&] { (void)moved.get_var<std::string>("value"); }, "string");
        fails([&] { (void)proxy.get_var<int>("broken"); }, "getter failure");
        fails([&] { proxy.set_var("broken", 1); }, "setter failure");
        fails([&] { moved.invoke(); }, "table value");
        check(lua_gettop(L) == base && std::string_view(lua_tostring(L, -1)) == "sentinel", "error stack restoration");
    }
    lua_pop(L, 1);
    state other;
    fails([&] { moved.push(other.get()); }, "different VMs");
    check(lua_gettop(other.get()) == 0 && lua_gettop(L) == 0, "cross-VM rejection restores stacks");
    object empty;
    fails([&] { (void)empty.get_var<int>("value"); }, "empty or expired");
    empty.push(L);
    check(lua_isnil(L, -1), "empty handle pushes nil");
    lua_pop(L, 1);
    fails([&] { object invalid(L, 1); }, "stack index");
    fails([&] { object invalid(nullptr, 1); }, "live Lua state");
}

void native_class_handles() {
    state vm;
    auto* L = vm.get();
    tynex::cpp_class<Counter>(L, "Counter").reg_con<int>()
        .reg_func("get", &Counter::get).reg_func("add", &Counter::add)
        .reg_var("value", &Counter::value).reg_prop("readonly", &Counter::get);
    check(lua_gettop(L) == 0, "standalone class registration stack balance");
    auto counter = global(L, "Counter").invoke<object>(20);
    check(Counter::live == 1, "object handle roots native userdata");
    check(counter.get_var<int>("value") == 20, "native member lookup");
    counter.set_var("value", 30);
    check(counter.call_method<int>("add", 12) == 42, "native method call");
    check(counter.get_var<int>("readonly") == 42, "native property getter");
    fails([&] { counter.set_var("readonly", 1); }, "read-only");
    auto copy = counter;
    counter.reset();
    execute(L, "collectgarbage('collect')");
    check(Counter::live == 1 && copy.call_method<int>("get") == 42, "copied handle retains native object");
    copy.reset();
    execute(L, "collectgarbage('collect')");
    check(Counter::live == 0, "last handle releases native userdata to GC");
    check(lua_gettop(L) == 0, "native object stack balance");
}

void coroutine_and_vm_lifetime() {
    object retained;
    {
        state vm;
        auto* L = vm.get();
        auto* thread = lua_newthread(L);
        execute(thread, "return { value = 17, sum = function(a, b) return a + b end }");
        retained = object(thread, -1);
        lua_pop(thread, 1);
        lua_pop(L, 1);
        execute(L, "collectgarbage('collect')");
        check(retained.get_var<int>("value") == 17, "handle retains originating coroutine");
        check(retained.call<int>("sum", 20, 22) == 42, "call on retained coroutine");
        retained.push(L);
        object on_main(L, -1);
        lua_pop(L, 1);
        check(on_main.get_var<int>("value") == 17, "same VM transfer between coroutines");
    }
    check(!retained.validate() && retained.get_state() == nullptr, "handle detects owned VM closure");
    fails([&] { (void)retained.get_var<int>("value"); }, "expired");
    retained.reset(); // Must not access the freed VM.
    auto* raw = luaL_newstate();
    if (!raw) throw std::bad_alloc();
    lua_newtable(raw);
    lua_pushinteger(raw, 9); lua_setfield(raw, -2, "value");
    retained = object(raw, -1);
    lua_pop(raw, 1);
    lua_close(raw);
    check(!retained.validate(), "handle detects external lua_close");
    fails([&] { (void)retained.get_var<int>("value"); }, "expired");
    retained.reset();
}
} // namespace

int main() {
    try {
        table_access_and_calls();
        native_class_handles();
        coroutine_and_vm_lifetime();
        std::cout << "Tynex objects/classes: " << checks << " checks passed\n";
    } catch (const std::exception& exception) {
        std::cerr << "Tynex object test failed: " << exception.what() << '\n';
        return 1;
    }
}
