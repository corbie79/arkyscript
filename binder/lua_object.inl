#pragma once

#include "lua_object.h"

namespace next2::luax::detail {
inline registry_reference::~registry_reference() {
    if (lifetime->alive && lua_checkstack(state, 2)) {
        luaL_unref(state, LUA_REGISTRYINDEX, value);
        luaL_unref(state, LUA_REGISTRYINDEX, thread);
    }
}

// Keep metamethod errors inside protected Lua calls; these C frames own no C++ resources.
inline int object_get_field(lua_State* L) {
    lua_pushvalue(L, 2);
    lua_gettable(L, 1);
    return 1;
}
inline int object_set_field(lua_State* L) {
    lua_settable(L, 1);
    return 0;
}
inline next2::luax::object value_traits<next2::luax::object>::read(lua_State* L, int index) {
    return next2::luax::object(L, index);
}
inline void value_traits<next2::luax::object>::push(lua_State* L, const next2::luax::object& value) {
    value.push(L);
}
} // namespace next2::luax::detail

namespace next2::luax {
inline object::object(lua_State* L, int index) {
    if (!L) throw error("object requires a live Lua state");
    detail::stack_guard stack(L);
    detail::reserve(L, 8);
    if (lua_type(L, index) == LUA_TNONE) throw error("invalid Lua object stack index");
    auto reference = std::make_shared<detail::registry_reference>(L, detail::lifetime_of(L));
    lua_pushvalue(L, index);
    reference->value = luaL_ref(L, LUA_REGISTRYINDEX);
    // The registry is shared by coroutines, but their stacks have separate lifetimes.
    lua_pushthread(L);
    reference->thread = luaL_ref(L, LUA_REGISTRYINDEX);
    reference_ = std::move(reference);
}
inline bool object::validate() const noexcept {
    return get_state() && reference_->value != LUA_NOREF && reference_->value != LUA_REFNIL;
}
inline lua_State* object::get_state() const noexcept {
    return reference_ && reference_->lifetime->alive ? reference_->state : nullptr;
}
inline lua_State* object::checked_state() const {
    auto* L = get_state();
    if (!L) throw error("empty or expired Lua object");
    return L;
}
inline void object::push(lua_State* L) const {
    if (!L) throw error("object push requires a live Lua state");
    detail::reserve(L, 2);
    if (!reference_) { lua_pushnil(L); return; }
    (void)checked_state();
    if (detail::lifetime_of(L) != reference_->lifetime) throw error("cannot move a Lua object between different VMs");
    if (reference_->value == LUA_REFNIL) lua_pushnil(L);
    else lua_rawgeti(L, LUA_REGISTRYINDEX, reference_->value);
}
inline void object::push_field(lua_State* L, std::string_view name) const {
    lua_pushcfunction(L, detail::object_get_field);
    push(L);
    lua_pushlstring(L, name.data(), name.size());
    detail::check_status(L, lua_pcall(L, 2, 1, 0));
}

template<class T> T object::as() const {
    detail::check_result<T>();
    auto* L = checked_state();
    detail::stack_guard stack(L);
    detail::reserve(L, 8);
    push(L);
    return detail::read<T>(L, -1);
}
template<class T> T object::get_var(std::string_view name) const {
    detail::check_result<T>();
    auto* L = checked_state();
    detail::stack_guard stack(L);
    detail::reserve(L, 8);
    push_field(L, name);
    return detail::read<T>(L, -1);
}
template<class T> void object::set_var(std::string_view name, T&& value) const {
    auto* L = checked_state();
    detail::stack_guard stack(L);
    detail::reserve(L, 8);
    lua_pushcfunction(L, detail::object_set_field);
    push(L);
    lua_pushlstring(L, name.data(), name.size());
    detail::push(L, std::forward<T>(value));
    detail::check_status(L, lua_pcall(L, 3, 0, 0));
}
inline object object::operator[](std::string_view name) const { return get_var<object>(name); }

template<class R, class... A> R object::call_field(bool self, std::string_view name, A&&... args) const {
    detail::check_result<R>();
    auto* L = checked_state();
    detail::stack_guard stack(L);
    detail::reserve(L, static_cast<int>(sizeof...(A)) + 8);
    push_field(L, name);
    if (self) push(L);
    (detail::push(L, std::forward<A>(args)), ...);
    const int count = static_cast<int>(sizeof...(A)) + (self ? 1 : 0);
    detail::check_status(L, lua_pcall(L, count, std::is_void_v<R> ? 0 : 1, 0));
    if constexpr (!std::is_void_v<R>) return detail::read<R>(L, -1);
}
template<class R, class... A> R object::call(std::string_view name, A&&... args) const {
    return call_field<R>(false, name, std::forward<A>(args)...);
}
template<class R, class... A> R object::call_method(std::string_view name, A&&... args) const {
    return call_field<R>(true, name, std::forward<A>(args)...);
}
template<class R, class... A> R object::invoke(A&&... args) const {
    detail::check_result<R>();
    auto* L = checked_state();
    detail::stack_guard stack(L);
    detail::reserve(L, static_cast<int>(sizeof...(A)) + 8);
    push(L);
    (detail::push(L, std::forward<A>(args)), ...);
    detail::check_status(L, lua_pcall(L, sizeof...(A), std::is_void_v<R> ? 0 : 1, 0));
    if constexpr (!std::is_void_v<R>) return detail::read<R>(L, -1);
}
} // namespace next2::luax
