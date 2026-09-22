#include "binder/lua_binder.h"
#include <iostream>

struct Counter {
    int value;
    explicit Counter(int initial) : value(initial) {}
    void add(int amount) { value += amount; }
    int get() const noexcept { return value; }
};

int main() {
    try {
        tynex::state vm;
        tynex::binder lua(vm);
        lua.reg_func("greet", [](const std::string& name) { return "Hello, " + name; });
        lua.reg_class<Counter>("Counter")
            .reg_con<int>()
            .reg_func("add", &Counter::add)
            .reg_var("value", &Counter::value)
            .reg_prop("total", &Counter::get);
        using tynex::value;
        lua.set_var("settings", value::table{
            {"name", "demo"}, {"enabled", true}, {"scores", value::array{20, 22}}
        });
        lua.dostring(R"(
            assert(settings.name == "demo" and settings.enabled)
            assert(settings.scores[1] + settings.scores[2] == 42)
            local counter = Counter(20)
            counter:add(22)
            assert(counter.value == 42 and counter.total == 42)
            function answer() return counter.total end
        )");
        const int answer = lua.call<int>("answer");
        std::cout << lua.call<std::string>("greet", "LuaJIT") << ": " << answer << '\n';
        return answer == 42 ? 0 : 1;
    } catch (const tynex::error& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
