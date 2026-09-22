#pragma once

// Registry-backed Lua value handles and protected object access.
// Author: eugene.choi.
#include "lua_arg.h"

namespace next::detail {
struct registry_reference {
    lua_State* state;
    std::shared_ptr<vm_lifetime> lifetime;
    int value = LUA_NOREF;
    int thread = LUA_NOREF;
    registry_reference(lua_State* L, std::shared_ptr<vm_lifetime> life)
        : state(L), lifetime(std::move(life)) {}
    ~registry_reference();
    registry_reference(const registry_reference&) = delete;
    registry_reference& operator=(const registry_reference&) = delete;
};
} // namespace next::detail

namespace next {
// Copies share one registry reference. A handle retains its value and originating
// coroutine, but does not keep the VM alive. Use after VM closure throws error.
class object {
public:
    object() = default;
    object(lua_State* L, int index);
    [[nodiscard]] bool validate() const noexcept;
    [[nodiscard]] lua_State* get_state() const noexcept;
    void reset() noexcept { reference_.reset(); }
    void push(lua_State* L) const;

    template<class T> [[nodiscard]] T as() const;
    template<class T> [[nodiscard]] T get_var(std::string_view name) const;
    template<class T> void set_var(std::string_view name, T&& value) const;
    [[nodiscard]] object operator[](std::string_view name) const;

    // call: table.fn(args); call_method: table:fn(args); invoke: value(args).
    template<class R = void, class... A> R call(std::string_view name, A&&... args) const;
    template<class R = void, class... A> R call_method(std::string_view name, A&&... args) const;
    template<class R = void, class... A> R invoke(A&&... args) const;
private:
    lua_State* checked_state() const;
    void push_field(lua_State* L, std::string_view name) const;
    template<class R, class... A> R call_field(bool self, std::string_view name, A&&... args) const;
    std::shared_ptr<detail::registry_reference> reference_;
};
} // namespace next

namespace next::detail {
template<> struct value_traits<next::object> {
    static constexpr bool enabled = true;
    static next::object read(lua_State* L, int index);
    static void push(lua_State* L, const next::object& value);
};
} // namespace next::detail

#include "lua_object.inl"
