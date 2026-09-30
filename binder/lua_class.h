#pragma once

// C++ constructors, methods, members, and properties exposed to Lua.
// Author: eugene.choi.
#include "lua_cclosure.h"

namespace next2::luax {
enum class instance_field_policy { protect_native_members, allow_method_overrides };
}

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
inline char instance_fields_key;
inline char instance_fields_owner_key;
struct instance_field_options { bool enabled = false; bool method_overrides = false; };
inline instance_field_options instance_fields_options(lua_State* L, int meta) {
    meta = absolute_index(L, meta);
    lua_pushvalue(L, meta);
    while (lua_istable(L, -1)) {
        lua_pushlightuserdata(L, &instance_fields_key);
        lua_rawget(L, -2);
        const bool configured = !lua_isnil(L, -1);
        const bool overrides = lua_tointeger(L, -1)
            == static_cast<lua_Integer>(instance_field_policy::allow_method_overrides);
        lua_pop(L, 1);
        if (configured) { lua_pop(L, 1); return {true, overrides}; }
        lua_getfield(L, -1, "base");
        lua_remove(L, -2);
    }
    lua_pop(L, 1);
    return {};
}
inline void require_live_instance(lua_State* L) {
    if (lua_type(L, 1) != LUA_TUSERDATA || lua_objlen(L, 1) != sizeof(object_storage)
        || !lua_getmetatable(L, 1)) {
        luaL_error(L, "wrong or expired C++ object");
        return;
    }
    const bool matches = lua_rawequal(L, -1, lua_upvalueindex(1)) != 0;
    lua_pop(L, 1);
    if (!matches || !static_cast<object_storage*>(lua_touserdata(L, 1))->value)
        luaL_error(L, "wrong or expired C++ object");
}
// A userdata starts with its creator's environment (often the globals table).
// Create an isolated environment lazily; never use the shared environment for
// peer fields. No registry roots are needed, so Lua callback/self cycles collect.
inline bool push_instance_fields(lua_State* L, bool create) {
    lua_getfenv(L, 1);
    lua_pushlightuserdata(L, &instance_fields_owner_key);
    lua_rawget(L, -2);
    const bool own = lua_touserdata(L, -1) == lua_touserdata(L, 1);
    lua_pop(L, 1);
    if (own) return true;
    lua_pop(L, 1);
    if (!create) return false;
    lua_newtable(L);
    lua_pushlightuserdata(L, &instance_fields_owner_key);
    lua_pushlightuserdata(L, lua_touserdata(L, 1));
    lua_rawset(L, -3);
    lua_pushvalue(L, -1);
    lua_setfenv(L, 1);
    return true;
}
inline int index_dispatch(lua_State* L) {
    const auto fields = instance_fields_options(L, lua_upvalueindex(1));
    if (fields.enabled) require_live_instance(L);
    lua_pushvalue(L, lua_upvalueindex(1)); // current class metatable
    while (lua_istable(L, -1)) {
        lua_getfield(L, -1, "methods");
        lua_pushvalue(L, 2);
        lua_rawget(L, -2);
        if (!lua_isnil(L, -1)) {
            if (fields.method_overrides && push_instance_fields(L, false)) {
                lua_pushvalue(L, 2);
                lua_rawget(L, -2);
                if (lua_isfunction(L, -1)) return 1;
                lua_pop(L, 2);
            }
            return 1;
        }
        lua_pop(L, 2);
        lua_getfield(L, -1, "extensions");
        lua_pushvalue(L, 2);
        lua_rawget(L, -2);
        if (!lua_isnil(L, -1)) return 1;
        lua_pop(L, 2);
        lua_getfield(L, -1, "getters");
        lua_pushvalue(L, 2);
        lua_rawget(L, -2);
        if (!lua_isnil(L, -1)) {
            lua_pushvalue(L, 1);
            lua_call(L, 1, 1);
            return 1;
        }
        lua_pop(L, 2);
        lua_getfield(L, -1, "base");
        lua_remove(L, -2);
    }
    if (fields.enabled && push_instance_fields(L, false)) {
        lua_pushvalue(L, 2);
        lua_rawget(L, -2);
    }
    return 1;
}
inline int newindex_dispatch(lua_State* L) {
    const auto fields = instance_fields_options(L, lua_upvalueindex(1));
    if (fields.enabled) require_live_instance(L);
    lua_pushvalue(L, lua_upvalueindex(1));
    while (lua_istable(L, -1)) {
        lua_getfield(L, -1, "setters");
        lua_pushvalue(L, 2);
        lua_rawget(L, -2);
        if (lua_isfunction(L, -1)) {
            lua_pushvalue(L, 1);
            lua_pushvalue(L, 3);
            lua_call(L, 2, 0);
            return 0;
        }
        lua_pop(L, 2);
        if (fields.enabled) {
            for (const char* table : {"methods", "getters", "extensions"}) {
                lua_getfield(L, -1, table);
                lua_pushvalue(L, 2);
                lua_rawget(L, -2);
                if (!lua_isnil(L, -1)) {
                    if (fields.method_overrides && std::string_view(table) == "methods") {
                        if (!lua_isfunction(L, 3) && !lua_isnil(L, 3))
                            return luaL_error(L, "native method override must be a function or nil");
                        push_instance_fields(L, true);
                        lua_pushvalue(L, 2);
                        lua_pushvalue(L, 3);
                        lua_rawset(L, -3);
                        return 0;
                    }
                    return luaL_error(L, "unknown or read-only C++ property");
                }
                lua_pop(L, 2);
            }
        }
        lua_getfield(L, -1, "base");
        lua_remove(L, -2);
    }
    if (fields.enabled) {
        push_instance_fields(L, true);
        lua_pushvalue(L, 2);
        lua_pushvalue(L, 3);
        lua_rawset(L, -3);
        return 0;
    }
    return luaL_error(L, "unknown or read-only C++ property");
}

inline void require_open_class(lua_State* L, int meta) {
    lua_getfield(L, meta, "sealed");
    const bool sealed = lua_toboolean(L, -1) != 0;
    lua_pop(L, 1);
    if (sealed) throw error("base class registration is sealed; register base members before derived classes");
}
inline void require_no_instances(lua_State* L, int meta) {
    lua_getfield(L, meta, "instances_created");
    const bool used = lua_toboolean(L, -1) != 0;
    lua_pop(L, 1);
    if (used) throw error("register ownership and bases before exposing C++ objects");
}

template<class T> void register_ref_object(lua_State* L, arky_ref_object<T> hooks) {
    static_assert(object_type<T> && !std::is_const_v<T>);
    if (hooks.ownership != ref_ownership::native && hooks.ownership != ref_ownership::lua)
        throw error("invalid native reference ownership");
    if (hooks.ownership == ref_ownership::lua && (!hooks.addRef || !hooks.decRef))
        throw error("Lua reference ownership requires addRef and decRef");
    stack_guard stack(L);
    reserve(L, 12);
    if (!push_metatable<T>(L)) throw error("C++ type has not been registered in this Lua state");
    const int meta = lua_gettop(L);
    require_open_class(L, meta);
    require_no_instances(L, meta);
    if (reference_policy<T>(L).policy) throw error("native reference policy is already registered or inherited");
    lua_getfield(L, meta, "constructors");
    lua_pushnil(L);
    if (lua_next(L, -2)) throw error("native reference objects require a pointer factory");
    lua_pop(L, 1);
    auto* policy = new (lua_newuserdata(L, sizeof(registered_ref_policy<T>))) registered_ref_policy<T>{
        {hooks.ownership, +[](object_storage& box, void* identity, const ref_policy& policy) {
            if (policy.ownership == ref_ownership::native) return;
            static_assert(std::is_standard_layout_v<registered_ref_policy<T>>);
            const auto& hooks = reinterpret_cast<const registered_ref_policy<T>&>(policy).hooks;
            auto* owner = new retained_ref<T>{static_cast<T*>(identity), hooks.decRef};
            hooks.addRef(owner->value);
            box.owner = owner;
            box.release = +[](void* value) noexcept {
                auto* held = static_cast<retained_ref<T>*>(value);
                auto pointer = held->value;
                auto release = held->decRef;
                delete held;
                release(pointer);
            };
            box.abandon = +[](void* value) noexcept { delete static_cast<retained_ref<T>*>(value); };
        }}, hooks
    };
    lua_pushlightuserdata(L, &ref_policy_key);
    lua_pushvalue(L, -2);
    lua_rawset(L, meta);
    lua_newtable(L); // const/mutable caches plus pending-finalizer tracking
    for (int view : {1, 2}) {
        lua_newtable(L);
        lua_newtable(L);
        lua_pushliteral(L, "v");
        lua_setfield(L, -2, "__mode");
        lua_setmetatable(L, -2);
        lua_rawseti(L, -2, view);
    }
    lua_newtable(L);
    lua_rawseti(L, -2, 3);
    lua_pushlightuserdata(L, &policy->policy);
    lua_pushvalue(L, -2);
    lua_rawset(L, LUA_REGISTRYINDEX);
}
inline bool inherited_member(lua_State* L, int meta, const char* name) {
    stack_guard stack(L);
    lua_getfield(L, meta, "base");
    while (lua_istable(L, -1)) {
        for (const char* table : {"methods", "getters", "extensions"}) {
            lua_getfield(L, -1, table);
            lua_getfield(L, -1, name);
            const bool exists = !lua_isnil(L, -1);
            lua_pop(L, 2);
            if (exists) return true;
        }
        lua_getfield(L, -1, "base");
        lua_remove(L, -2);
    }
    return false;
}

// Native members remain immutable. Script trait methods live in a separate table.
inline bool class_member(lua_State* L, int meta, const char* name) {
    stack_guard stack(L);
    for (const char* table : {"methods", "getters", "extensions"}) {
        lua_getfield(L, meta, table);
        lua_getfield(L, -1, name);
        if (!lua_isnil(L, -1)) return true;
        lua_pop(L, 2);
    }
    return inherited_member(L, meta, name);
}
inline bool descendant_member(lua_State* L, int meta, const char* name) {
    stack_guard stack(L);
    reserve(L, 8);
    meta = absolute_index(L, meta);
    lua_getfield(L, meta, "children");
    lua_pushnil(L);
    while (lua_next(L, -2)) {
        const int child = lua_gettop(L);
        if (class_member(L, child, name) || descendant_member(L, child, name)) return true;
        lua_pop(L, 1);
    }
    return false;
}
inline int implement_trait(lua_State* L) {
    reserve(L, 12);
    if (lua_gettop(L) != 3) throw error("invalid native impl registration");
    for (int i = 1; i <= 3; ++i) expect(L, i, LUA_TTABLE);
    lua_pushvalue(L, lua_upvalueindex(1));
    const int meta = lua_gettop(L);
    lua_getfield(L, meta, "traits");
    const int traits = lua_gettop(L);
    lua_pushvalue(L, 1);
    lua_rawget(L, traits);
    if (!lua_isnil(L, -1)) throw error("duplicate native trait implementation");
    lua_pop(L, 1);
    // Validate the whole registration before publishing any method.
    for (int source : {2, 3}) {
        lua_pushnil(L);
        while (lua_next(L, source)) {
            if (lua_type(L, -2) != LUA_TSTRING || !lua_isfunction(L, -1))
                throw error("native impl requires named functions");
            if (source == 2 && class_member(L, meta, lua_tostring(L, -2)))
                throw error("native impl cannot replace an existing member");
            if (!class_member(L, meta, lua_tostring(L, -2))
                && descendant_member(L, meta, lua_tostring(L, -2)))
                throw error("native impl conflicts with a derived member");
            if (source == 3) {
                // A default can reuse a native method, but never a field/property.
                lua_pushvalue(L, meta);
                while (lua_istable(L, -1)) {
                    lua_getfield(L, -1, "getters");
                    lua_pushvalue(L, -4);
                    lua_rawget(L, -2);
                    if (!lua_isnil(L, -1)) throw error("native trait default conflicts with a property");
                    lua_pop(L, 2);
                    lua_getfield(L, -1, "extensions");
                    lua_pushvalue(L, -4);
                    lua_rawget(L, -2);
                    if (!lua_isnil(L, -1)) throw error("conflicting native trait defaults");
                    lua_pop(L, 2);
                    lua_getfield(L, -1, "base");
                    lua_remove(L, -2);
                }
                lua_pop(L, 1);
            }
            lua_pop(L, 1);
        }
    }
    lua_getfield(L, meta, "extensions");
    const int methods = lua_gettop(L);
    for (int source : {3, 2}) {
        lua_pushnil(L);
        while (lua_next(L, source)) {
            if (source == 2 || !class_member(L, meta, lua_tostring(L, -2))) {
                lua_pushvalue(L, -2);
                lua_pushvalue(L, -2);
                lua_rawset(L, methods);
            }
            lua_pop(L, 1);
        }
    }
    lua_pushvalue(L, 1);
    lua_pushboolean(L, true);
    lua_rawset(L, traits);
    return 0;
}

template<class T> void create_class(lua_State* L) {
    // Externally managed types may have a protected destructor. emplace checks
    // destructibility only when the binder actually constructs a value.
    static_assert(object_type<T> && !std::is_const_v<T>);
    if (push_metatable<T>(L)) throw error("C++ class is already registered in this Lua state");
    lua_pop(L, 1);
    lua_newtable(L); // instance metatable
    const int meta = lua_gettop(L);
    lua_pushlightuserdata(L, &object_metatable_key);
    lua_pushboolean(L, true);
    lua_rawset(L, meta);
    for (const char* name : {"methods", "getters", "setters", "constructors", "extensions", "traits", "children"}) {
        lua_newtable(L);
        lua_setfield(L, meta, name);
    }
    lua_pushvalue(L, meta);
    lua_pushcclosure(L, index_dispatch, 1);
    lua_setfield(L, meta, "__index");
    lua_pushvalue(L, meta);
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
    lua_pushvalue(L, meta);
    lua_pushcclosure(L, boundary<implement_trait>, 1);
    lua_setfield(L, -2, "__arky_impl");
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
    // One explicit public/unambiguous base chain per Lua class. Register the
    // complete base first; sealing prevents later member collision changes.
    template<class Base> cpp_class& reg_base() {
        static_assert(!std::is_same_v<T, Base> && !std::is_const_v<Base>
                      && std::is_convertible_v<T*, Base*>,
                      "reg_base requires a distinct public, unambiguous C++ base");
        detail::stack_guard stack(L_);
        detail::reserve(L_, 8);
        detail::push_metatable<T>(L_);
        const int derived = lua_gettop(L_);
        detail::require_open_class(L_, derived);
        detail::require_no_instances(L_, derived);
        if (detail::reference_policy<T>(L_).policy)
            throw error("register the C++ base before reference ownership");
        lua_getfield(L_, derived, "base");
        if (!lua_isnil(L_, -1)) throw error("a C++ base is already registered");
        lua_pop(L_, 1);
        for (const char* table : {"methods", "getters", "setters", "constructors", "extensions", "traits"}) {
            lua_getfield(L_, derived, table);
            lua_pushnil(L_);
            if (lua_next(L_, -2)) throw error("register the C++ base before constructors and members");
            lua_pop(L_, 1);
        }
        if (!detail::push_metatable<Base>(L_)) throw error("C++ base has not been registered in this Lua state");
        const int base = lua_gettop(L_);
        new (lua_newuserdata(L_, sizeof(detail::upcast_function))) detail::upcast_function{
            +[](void* value) -> void* { return static_cast<Base*>(static_cast<T*>(value)); }
        };
        lua_setfield(L_, derived, "upcast");
        lua_pushvalue(L_, base);
        lua_setfield(L_, derived, "base");
        lua_pushboolean(L_, true);
        lua_setfield(L_, base, "sealed");
        lua_getfield(L_, base, "children");
        lua_pushvalue(L_, derived);
        lua_rawseti(L_, -2, static_cast<int>(lua_objlen(L_, -2)) + 1);
        return *this;
    }
    template<class... A> cpp_class& reg_con() {
        detail::stack_guard stack(L_);
        detail::reserve(L_, 8);
        detail::push_metatable<T>(L_);
        detail::require_open_class(L_, -1);
        if (detail::reference_policy<T>(L_).policy)
            throw error("native reference objects require a pointer factory");
        lua_getfield(L_, -1, "constructors");
        lua_rawgeti(L_, -1, sizeof...(A));
        const bool exists = !lua_isnil(L_, -1);
        lua_pop(L_, 1);
        if (exists) throw error("a constructor with this argument count is already registered");
        lua_pushcfunction(L_, (detail::boundary<detail::construct<T, A...>>));
        lua_rawseti(L_, -2, sizeof...(A));
        return *this;
    }
    cpp_class& reg_ref_object(arky_ref_object<T> policy) {
        detail::register_ref_object(L_, policy);
        return *this;
    }
    // The closest explicitly configured class supplies the inherited policy.
    // Overrides live in each userdata's fields, never in the native method table.
    cpp_class& enable_instance_fields(instance_field_policy policy = instance_field_policy::protect_native_members) {
        if (policy != instance_field_policy::protect_native_members
            && policy != instance_field_policy::allow_method_overrides)
            throw error("invalid instance field policy");
        detail::stack_guard stack(L_);
        detail::reserve(L_, 4);
        detail::push_metatable<T>(L_);
        detail::require_open_class(L_, -1);
        detail::require_no_instances(L_, -1);
        lua_pushlightuserdata(L_, &detail::instance_fields_key);
        lua_pushinteger(L_, static_cast<lua_Integer>(policy));
        lua_rawset(L_, -3);
        return *this;
    }
    // Adapter functions spell out self as T& or const T&. This keeps receiver
    // conversion, const checks, and lifetime checks identical to native methods.
    template<class F> requires (!std::is_member_function_pointer_v<std::decay_t<F>>)
    cpp_class& reg_method(const char* name, F&& function) {
        using traits = detail::function_traits<std::decay_t<F>>;
        static_assert(traits::arity > 0, "reg_method requires an explicit T& or const T& self parameter");
        if constexpr (traits::arity > 0) {
            using self = std::tuple_element_t<0, typename traits::arguments>;
            static_assert(std::is_same_v<self, T&> || std::is_same_v<self, const T&>,
                          "reg_method requires an explicit T& or const T& self parameter");
        }
        put("methods", name, std::forward<F>(function));
        return *this;
    }
    // Raw Lua C callbacks own argument validation and exception boundaries.
    // They receive the calling state (including coroutines) and self at slot 1.
    cpp_class& reg_raw_method(const char* name, lua_CFunction function) {
        if (!function) throw error("cannot register a null Lua C function");
        put("methods", name, function);
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
        detail::require_open_class(L_, -1);
        if (detail::inherited_member(L_, -1, name))
            throw error("member name is inherited from a C++ base");
        lua_getfield(L_, -1, "extensions");
        lua_getfield(L_, -1, name);
        if (!lua_isnil(L_, -1)) throw error("member name is used by a script trait");
        lua_pop(L_, 2);
        lua_getfield(L_, -1, std::string_view(table) == "methods" ? "getters" : "methods");
        lua_getfield(L_, -1, name);
        const bool conflict = !lua_isnil(L_, -1);
        lua_pop(L_, 2);
        if (conflict) throw error("member name is already used by a method/property");
        lua_getfield(L_, -1, table);
        if constexpr (std::is_same_v<std::decay_t<F>, lua_CFunction>)
            lua_pushcfunction(L_, function);
        else
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
