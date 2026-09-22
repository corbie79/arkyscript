#pragma once

// C++ constructors, methods, members, and properties exposed to Lua.
// Author: eugene.choi.
#include "lua_cclosure.h"

namespace next2::luax::detail {
template<class T, class F, std::size_t... I> auto method(F function, std::index_sequence<I...>) {
    using traits = function_traits<F>;
    using self_type = std::conditional_t<traits::is_const, const T&, T&>;
    return [function](self_type self, std::tuple_element_t<I, typename traits::arguments>... args) -> decltype(auto) {
        return std::invoke(function, self, std::forward<decltype(args)>(args)...);
    };
}
template<class T, class... A> int construct(lua_State* L) {
    if (lua_gettop(L) != sizeof...(A) + 1) throw error("incorrect constructor argument count");
    auto args = arguments<std::tuple<A...>>(L, 2, std::index_sequence_for<A...>{});
    std::apply([L](auto&&... values) { emplace<T>(L, std::forward<decltype(values)>(values)...); }, std::move(args));
    return 1;
}

// These dispatchers hold no C++ resources when they enter Lua calls.
inline int constructor_dispatch(lua_State* L) {
    const int count = lua_gettop(L);
    lua_rawgeti(L, lua_upvalueindex(1), count - 1);
    if (!lua_isfunction(L, -1)) return luaL_error(L, "no constructor for %d arguments", count - 1);
    lua_insert(L, 1);
    lua_call(L, count, 1);
    return 1;
}
inline int index_dispatch(lua_State* L) {
    lua_pushvalue(L, 2);
    lua_rawget(L, lua_upvalueindex(1)); // methods
    if (!lua_isnil(L, -1)) return 1;
    lua_pop(L, 1);
    lua_pushvalue(L, 2);
    lua_rawget(L, lua_upvalueindex(2)); // getters
    if (lua_isnil(L, -1)) return 1;
    lua_pushvalue(L, 1);
    lua_call(L, 1, 1);
    return 1;
}
inline int newindex_dispatch(lua_State* L) {
    lua_pushvalue(L, 2);
    lua_rawget(L, lua_upvalueindex(1)); // setters
    if (!lua_isfunction(L, -1)) return luaL_error(L, "unknown or read-only C++ property");
    lua_pushvalue(L, 1);
    lua_pushvalue(L, 3);
    lua_call(L, 2, 0);
    return 0;
}

template<class T> void create_class(lua_State* L) {
    static_assert(object_type<T> && !std::is_const_v<T> && std::is_nothrow_destructible_v<T>);
    if (push_metatable<T>(L)) throw error("C++ class is already registered in this Lua state");
    lua_pop(L, 1);
    lua_newtable(L); // instance metatable
    const int meta = lua_gettop(L);
    for (const char* name : {"methods", "getters", "setters", "constructors"}) {
        lua_newtable(L);
        lua_setfield(L, meta, name);
    }
    lua_getfield(L, meta, "methods");
    lua_getfield(L, meta, "getters");
    lua_pushcclosure(L, index_dispatch, 2);
    lua_setfield(L, meta, "__index");
    lua_getfield(L, meta, "setters");
    lua_pushcclosure(L, newindex_dispatch, 1);
    lua_setfield(L, meta, "__newindex");
    lua_pushcfunction(L, collect<T>);
    lua_setfield(L, meta, "__gc");
    lua_pushboolean(L, false);
    lua_setfield(L, meta, "__metatable");
    lua_pushlightuserdata(L, &type_key<T>);
    lua_pushvalue(L, meta);
    lua_rawset(L, LUA_REGISTRYINDEX);
    lua_newtable(L); // public class factory
    lua_newtable(L); // factory metatable
    lua_getfield(L, meta, "constructors");
    lua_pushcclosure(L, constructor_dispatch, 1);
    lua_setfield(L, -2, "__call");
    lua_pushboolean(L, false);
    lua_setfield(L, -2, "__metatable");
    lua_setmetatable(L, -2);
    lua_remove(L, meta); // leave only the public factory
}

} // namespace next2::luax::detail

namespace next2::luax {
template<class T> class cpp_class {
public:
    cpp_class(lua_State* L, std::string_view name) : L_(L) {
        if (!L_) throw error("cpp_class requires a live Lua state");
        detail::stack_guard stack(L_);
        detail::reserve(L_, 12);
        detail::validate_path(name);
        detail::create_class<T>(L_);
        try {
            detail::set_path(L_, name);
        } catch (...) {
            lua_pushlightuserdata(L_, &detail::type_key<T>);
            lua_pushnil(L_);
            lua_rawset(L_, LUA_REGISTRYINDEX);
            throw;
        }
    }
    template<class... A> cpp_class& reg_con() {
        detail::stack_guard stack(L_);
        detail::reserve(L_, 8);
        detail::push_metatable<T>(L_);
        lua_getfield(L_, -1, "constructors");
        lua_rawgeti(L_, -1, sizeof...(A));
        const bool exists = !lua_isnil(L_, -1);
        lua_pop(L_, 1);
        if (exists) throw error("a constructor with this argument count is already registered");
        lua_pushcfunction(L_, (detail::boundary<detail::construct<T, A...>>));
        lua_rawseti(L_, -2, sizeof...(A));
        return *this;
    }
    template<class F> requires std::is_member_function_pointer_v<F>
    cpp_class& reg_func(const char* name, F function) {
        if (!function) throw error("cannot register a null member function");
        using traits = detail::function_traits<F>;
        put("methods", name, detail::method<T>(function, std::make_index_sequence<traits::arity>{}));
        return *this;
    }
    template<class C, class V> cpp_class& reg_var(const char* name, V C::* member) {
        static_assert(std::is_base_of_v<C, T>);
        if (!member) throw error("cannot register a null member variable");
        put("getters", name, [member](const T& self) -> decltype(auto) { return (self.*member); });
        if constexpr (!std::is_const_v<V>)
            put("setters", name, [member](T& self, V value) { self.*member = std::move(value); });
        else
            erase("setters", name);
        return *this;
    }
    template<class G> cpp_class& reg_prop(const char* name, G getter) {
        static_assert(std::is_member_function_pointer_v<G> && detail::function_traits<G>::arity == 0);
        if (!getter) throw error("cannot register a null property getter");
        put("getters", name, detail::method<T>(getter, std::index_sequence<>{}));
        erase("setters", name);
        return *this;
    }
    template<class G, class S> cpp_class& reg_prop(const char* name, G getter, S setter) {
        static_assert(std::is_member_function_pointer_v<S> && detail::function_traits<S>::arity == 1);
        if (!setter) throw error("cannot register a null property setter");
        reg_prop(name, getter);
        put("setters", name, detail::method<T>(setter, std::index_sequence<0>{}));
        return *this;
    }
private:
    template<class F> void put(const char* table, const char* name, F&& function) {
        if (!name || !*name) throw error("expected a non-empty member name");
        detail::stack_guard stack(L_);
        detail::reserve(L_, 8);
        detail::push_metatable<T>(L_);
        lua_getfield(L_, -1, std::string_view(table) == "methods" ? "getters" : "methods");
        lua_getfield(L_, -1, name);
        const bool conflict = !lua_isnil(L_, -1);
        lua_pop(L_, 2);
        if (conflict) throw error("member name is already used by a method/property");
        lua_getfield(L_, -1, table);
        detail::push_function(L_, std::forward<F>(function));
        lua_setfield(L_, -2, name);
    }
    void erase(const char* table, const char* name) {
        detail::stack_guard stack(L_);
        detail::push_metatable<T>(L_);
        lua_getfield(L_, -1, table);
        lua_pushnil(L_);
        lua_setfield(L_, -2, name);
    }
    lua_State* L_;
};

} // namespace next2::luax
