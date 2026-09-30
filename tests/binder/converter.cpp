#include "binder/lua_binder.h"
#include <chrono>
#include <iostream>

namespace sample {
struct Point {
    double x, y;
    bool operator==(const Point&) const = default;
};
enum class Direction { left, right };
struct Text { std::unique_ptr<std::string> value; };
struct BrokenRead { bool throws = false; };
struct BrokenPush { int count; bool throws = false; };
struct Widget {
    Point position{0, 0};
    Point get_position() const { return position; }
    void set_position(const Point& value) { position = value; }
};
}

// These adapters use only public binding APIs and the Lua C API.
namespace next2::luax {
template<> struct converter<sample::Point> {
    static constexpr bool enabled = true;
    static sample::Point read(lua_State* L, int index) {
        if (lua_type(L, index) != LUA_TTABLE) throw error("expected a point table");
        lua_pushliteral(L, "x"); lua_rawget(L, index);
        const double x = carg<double>(L, -1).to();
        lua_pop(L, 1);
        lua_pushliteral(L, "y"); lua_rawget(L, index);
        const double y = carg<double>(L, -1).to();
        lua_pop(L, 1);
        return {x, y};
    }
    static void push(lua_State* L, const sample::Point& value) {
        lua_createtable(L, 0, 2);
        pusher<double>::push(L, value.x); lua_setfield(L, -2, "x");
        pusher<double>::push(L, value.y); lua_setfield(L, -2, "y");
    }
};
template<> struct converter<std::chrono::milliseconds> {
    static constexpr bool enabled = true;
    static std::chrono::milliseconds read(lua_State* L, int index) {
        return std::chrono::milliseconds(carg<std::chrono::milliseconds::rep>(L, index).to());
    }
    static void push(lua_State* L, std::chrono::milliseconds value) {
        pusher<std::chrono::milliseconds::rep>::push(L, value.count());
    }
};
template<> struct converter<sample::Direction> {
    static constexpr bool enabled = true;
    static sample::Direction read(lua_State* L, int index) {
        const auto name = carg<std::string>(L, index).to();
        if (name == "left") return sample::Direction::left;
        if (name == "right") return sample::Direction::right;
        throw error("unknown direction");
    }
    static void push(lua_State* L, sample::Direction value) {
        pusher<const char*>::push(L, value == sample::Direction::left ? "left" : "right");
    }
};
template<> struct converter<sample::Text> {
    static constexpr bool enabled = true;
    static sample::Text read(lua_State* L, int index) {
        return {std::make_unique<std::string>(carg<std::string>(L, index).to())};
    }
    static void push(lua_State* L, const sample::Text& value) {
        pusher<const std::string&>::push(L, *value.value);
    }
};
template<> struct converter<sample::BrokenRead> {
    static constexpr bool enabled = true;
    static sample::BrokenRead read(lua_State* L, int index) {
        const bool throws = carg<bool>(L, index).to();
        lua_pushliteral(L, "temporary");
        if (throws) throw error("read failed");
        return {false}; // Deliberately fails to remove the temporary.
    }
    static void push(lua_State* L, sample::BrokenRead value) { lua_pushboolean(L, value.throws); }
};
template<> struct converter<sample::BrokenPush> {
    static constexpr bool enabled = true;
    static sample::BrokenPush read(lua_State* L, int index) { return {carg<int>(L, index).to(), false}; }
    static void push(lua_State* L, sample::BrokenPush value) {
        for (int i = 0; i < value.count; ++i) lua_pushinteger(L, i);
        if (value.throws) throw error("push failed");
    }
};
}

using namespace next2::luax;
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
    throw std::runtime_error("expected converter error");
}

void conversions() {
    state vm;
    binder lua(vm);
    auto* L = vm.get();
    const sample::Point point{12, 30};
    pusher<const sample::Point&>::push(L, point);
    check(lua_gettop(L) == 1, "point produces one table");
    carg<const sample::Point&> kept(L, -1);
    check(lua_gettop(L) == 1, "negative-index conversion preserves the stack");
    lua_pop(L, 1);
    lua_gc(L, LUA_GCCOLLECT, 0);
    check(kept.to() == point, "const-reference argument owns its converted point");
    fails([&] { (void)carg<sample::Point>(L, 1); }, "stack index");

    lua.reg_func("echo_point", [](const sample::Point& value) { return value; });
    lua.reg_func("echo_points", [](const std::vector<std::optional<sample::Point>>& values) { return values; });
    lua.reg_func("echo_time", [](std::chrono::milliseconds time) { return time; });
    lua.reg_func("echo_direction", [](sample::Direction direction) { return direction; });
    lua.reg_func("echo_text", [](sample::Text text) { return text; });
    lua.reg_func("optional_point", [](std::optional<sample::Point> value) { return value; });
    lua.reg_class<sample::Widget>("Widget").reg_con<>()
        .reg_var("position", &sample::Widget::position)
        .reg_prop("point", &sample::Widget::get_position, &sample::Widget::set_position);
    lua.set_var("point", point);
    lua.set_var("direction", sample::Direction::right);
    lua.dostring(R"(
        local point = echo_point(point)
        assert(point.x == 12 and point.y == 30)
        local points = echo_points({point, {x = 4, y = 5}})
        assert(#points == 2 and points[2].y == 5)
        assert(optional_point(nil) == nil and optional_point(point).x == 12)
        assert(echo_time(250) == 250 and echo_direction(direction) == "right")
        assert(echo_text("owned") == "owned")
        widget = Widget()
        widget.position = point
        assert(widget.point.x == 12)
        widget.point = {x = 20, y = 22}
        assert(widget.position.y == 22)
        function script_echo(value) return value end
        holder = {point = point}
    )");
    check(lua.get_var<sample::Point>("point") == point, "host value read");
    check(lua.call<sample::Point>("script_echo", point) == point, "host call conversion");
    check(lua.call<std::chrono::milliseconds>("echo_time", std::chrono::milliseconds(250)).count() == 250,
          "third-party value type conversion");
    check(lua.get_var<sample::Direction>("direction") == sample::Direction::right, "custom enum conversion");
    auto moved = lua.call<sample::Text>("echo_text", sample::Text{std::make_unique<std::string>("owned")});
    check(*moved.value == "owned", "move-only arguments and host results");
    const auto holder = lua.get_var<object>("holder");
    holder.set_var("point", sample::Point{1, 2});
    check(holder.get_var<sample::Point>("point") == sample::Point{1, 2}, "object handle uses converters");

    fails([&] { lua.dostring("echo_points({{x = 1, y = 'bad'}})"); }, "number");
    fails([&] { lua.dostring("echo_time(1.5)"); }, "integer");
    fails([&] { lua.dostring("echo_direction('up')"); }, "direction");
    lua.dostring(R"(
        local ok, message = pcall(echo_point, {x = 1, y = false})
        assert(not ok and message:find("number"))
    )");
    check(lua_gettop(L) == 0, "failed nested conversions and Lua errors restore stack");

    lua_pushliteral(L, "sentinel");
    const int base = lua_gettop(L);
    for (int count : {0, 2}) {
        fails([&] { pusher<sample::BrokenPush>::push(L, {count, false}); }, "exactly one");
        check(lua_gettop(L) == base, "bad push stack contract is restored");
    }
    fails([&] { pusher<sample::BrokenPush>::push(L, {1, true}); }, "push failed");
    check(lua_gettop(L) == base, "throwing push discards temporary values");
    for (bool throws : {false, true}) {
        lua_pushboolean(L, throws);
        fails([&] { (void)carg<sample::BrokenRead>(L, -1); }, throws ? "read failed" : "preserve");
        check(lua_gettop(L) == base + 1 && lua_type(L, -1) == LUA_TBOOLEAN,
              "failed read preserves its source value");
        lua_pop(L, 1);
    }
    lua.reg_func("bad_read", [](sample::BrokenRead) {});
    lua.reg_func("bad_push", [] { return sample::BrokenPush{2, false}; });
    lua.dostring(R"(
        local ok, message = pcall(bad_read, false)
        assert(not ok and message:find("preserve"))
        ok, message = pcall(bad_push)
        assert(not ok and message:find("exactly one"))
    )");
    check(lua_gettop(L) == base && std::string_view(lua_tostring(L, -1)) == "sentinel",
          "callbacks preserve the host stack on converter errors");
}
}

int main() {
    try {
        conversions();
        std::cout << "Converters: " << checks << " checks passed\n";
    } catch (const std::exception& exception) {
        std::cerr << "Converter test failed: " << exception.what() << '\n';
        return 1;
    }
}
