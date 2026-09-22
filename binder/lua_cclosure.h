#pragma once

// Variadic C++ callable dispatch and Lua callback boundaries.
// Author: eugene.choi.
#include "lua_arg.h"
#include <cstdio>
#include <functional>
#include <tuple>

namespace next2::detail {
// Leave the catch scope before raising a Lua error (also works with longjmp VMs).
template<auto Function> int boundary(lua_State* L) {
    char message[1024];
    try {
        return Function(L);
    } catch (const std::exception& exception) {
        std::snprintf(message, sizeof(message), "%s", exception.what());
    } catch (...) {
        std::snprintf(message, sizeof(message), "%s", "unknown C++ exception");
    }
    lua_pushstring(L, message);
    return lua_error(L);
}

template<class F> struct function_traits : function_traits<decltype(&F::operator())> {};
template<class R, class... A> struct function_traits<R (*)(A...)> {
    using result = R;
    using arguments = std::tuple<A...>;
    static constexpr bool is_const = false;
    static constexpr std::size_t arity = sizeof...(A);
};
template<class R, class... A> struct function_traits<R (*)(A...) noexcept> : function_traits<R (*)(A...)> {};
template<class C, class R, class... A> struct function_traits<R (C::*)(A...)> : function_traits<R (*)(A...)> {};
template<class C, class R, class... A> struct function_traits<R (C::*)(A...) const> : function_traits<R (*)(A...)> {
    static constexpr bool is_const = true;
};
template<class C, class R, class... A> struct function_traits<R (C::*)(A...) noexcept> : function_traits<R (C::*)(A...)> {};
template<class C, class R, class... A> struct function_traits<R (C::*)(A...) const noexcept> : function_traits<R (C::*)(A...) const> {};

template<class Tuple, std::size_t... I> auto arguments(lua_State* L, int start, std::index_sequence<I...>) {
    // List initialization sequences conversions and owns temporary strings through invocation.
    return std::tuple<decltype(read<std::tuple_element_t<I, Tuple>>(L, start + static_cast<int>(I)))...>{
        read<std::tuple_element_t<I, Tuple>>(L, start + static_cast<int>(I))...};
}
template<class F> int invoke(lua_State* L) {
    using traits = function_traits<F>;
    if (lua_gettop(L) != static_cast<int>(traits::arity)) throw error("incorrect argument count");
    reserve(L, 8);
    auto* box = test_holder<F, true>(L, lua_upvalueindex(1));
    if (!box || !box->value) throw error("expired C++ function");
    auto args = arguments<typename traits::arguments>(L, 1, std::make_index_sequence<traits::arity>{});
    if constexpr (std::is_void_v<typename traits::result>) {
        std::apply(*box->value, std::move(args));
        return 0;
    } else {
        push(L, std::apply(*box->value, std::move(args)));
        return 1;
    }
}
template<class F> void push_function(lua_State* L, F&& function) {
    using U = std::decay_t<F>;
    static_assert(std::is_nothrow_destructible_v<U>, "Bound functions need a non-throwing destructor");
    static_assert(!std::is_member_function_pointer_v<U>, "Use reg_class<T>().reg_func for member functions");
    if constexpr (std::is_pointer_v<plain<F>>) {
        if (!function) throw error("cannot register a null function");
    }
    if (!push_metatable<U, true>(L)) {
        lua_pop(L, 1);
        lua_newtable(L);
        lua_pushcfunction(L, (collect<U, true>));
        lua_setfield(L, -2, "__gc");
        lua_pushboolean(L, false);
        lua_setfield(L, -2, "__metatable");
        lua_pushlightuserdata(L, &type_key<U, true>);
        lua_pushvalue(L, -2);
        lua_rawset(L, LUA_REGISTRYINDEX);
    }
    lua_pop(L, 1);
    emplace<U, true>(L, std::forward<F>(function));
    lua_pushcclosure(L, boundary<invoke<U>>, 1);
}

} // namespace next2::detail
