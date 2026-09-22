#include "binder/lua_binder.h"
#include <concepts>
#include <iostream>
#include <string>
#include <utility>

// A reusable default implementation delegates the required operation to T.
template<class T> struct NamedBehavior {
    std::string describe() const {
        return "name: " + static_cast<const T&>(*this).label();
    }
};

struct User : NamedBehavior<User> {
    explicit User(std::string name) : name(std::move(name)) {}
    std::string label() const { return name; }
    std::string name;
};

struct Device : NamedBehavior<Device> {
    explicit Device(int id) : id(id) {}
    std::string label() const { return "device-" + std::to_string(id); }
    int id;
};

// C++20 checks the required operations when this binding helper is instantiated.
template<class T> concept Named = requires(const T& value) {
    { value.label() } -> std::same_as<std::string>;
    { value.describe() } -> std::same_as<std::string>;
};

template<Named T> void bind_named(tynex::cpp_class<T>& type) {
    type.reg_func("label", &T::label)
        .reg_func("describe", &T::describe);
}

struct MissingLabel {};
static_assert(Named<User> && Named<Device>);
static_assert(!Named<MissingLabel>);

int main() {
    try {
        tynex::state vm;
        tynex::binder lua(vm);

        auto user = lua.reg_class<User>("User");
        user.reg_con<std::string>().reg_var("name", &User::name);
        bind_named(user);

        auto device = lua.reg_class<Device>("Device");
        device.reg_con<int>();
        bind_named(device);

        lua.dostring(R"(
            local user = User("Mina")
            local device = Device(7)
            assert(user:label() == "Mina")
            assert(device:label() == "device-7")

            -- Both classes expose the same default behavior.
            function describe(value) return value:describe() end
            assert(describe(user) == "name: Mina")
            assert(describe(device) == "name: device-7")
            user.name = "Joon"
            assert(describe(user) == "name: Joon")

            function summary()
                return describe(user) .. " / " .. describe(device)
            end
        )");
        const auto summary = lua.call<std::string>("summary");
        std::cout << summary << '\n';
        return summary == "name: Joon / name: device-7" ? 0 : 1;
    } catch (const tynex::error& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
