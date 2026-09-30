#pragma once

// Lua/C++ argument conversion and userdata ownership.
// Author: eugene.choi.
#include "lua_util.h"
#include "lua_ref_object.h"
#include <cmath>
#include <limits>
#include <memory>
#include <new>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

namespace next2::luax {
// Specialize for a C++ value type before its first binding use. read returns T
// by value without changing the stack; push appends exactly one Lua value.
// Converters may use carg/pusher recursively and throw C++ exceptions on error.
template<class T> struct converter { static constexpr bool enabled = false; };

namespace detail {
template<class> inline constexpr bool unsupported = false;
template<class T> using plain = std::remove_cvref_t<T>;
template<class T> struct value_traits { static constexpr bool enabled = false; };

template<class T, bool Callable = false> inline char type_key;
// Type-erased cleanup allows native reference hooks without a shared_ptr control
// block. shared is present only for the optional shared_ptr/value conversion path.
struct object_storage {
    void* value = nullptr;
    void* owner = nullptr;
    void (*release)(void*) noexcept = nullptr;
    void (*abandon)(void*) noexcept = nullptr;
    std::shared_ptr<void>* shared = nullptr;
    void* ref_policy = nullptr;
    void* ref_identity = nullptr;
    bool writable = true;
};
template<class T> using holder = object_storage;
inline char object_metatable_key;
inline char ref_policy_key;
using upcast_function = void* (*)(void*);

inline void clear_object(object_storage& box, bool destroyed = false) noexcept {
    const auto old = std::exchange(box, object_storage{});
    if (old.owner) (destroyed ? old.abandon : old.release)(old.owner);
}
inline void own_shared(object_storage& box, std::shared_ptr<void> owner) {
    auto* shared = new std::shared_ptr<void>(std::move(owner));
    box.owner = box.shared = shared;
    box.release = box.abandon = +[](void* value) noexcept {
        delete static_cast<std::shared_ptr<void>*>(value);
    };
    box.value = shared->get();
}

inline int absolute_index(lua_State* L, int index) {
    return index > 0 || index <= LUA_REGISTRYINDEX ? index : lua_gettop(L) + index + 1;
}

template<class T, bool Callable = false> bool push_metatable(lua_State* L) {
    lua_pushlightuserdata(L, &type_key<T, Callable>);
    lua_rawget(L, LUA_REGISTRYINDEX);
    return lua_istable(L, -1);
}
template<class T, bool Callable = false> holder<T>* test_holder(lua_State* L, int index) {
    if (lua_type(L, index) != LUA_TUSERDATA || lua_objlen(L, index) != sizeof(holder<T>)) return nullptr;
    auto* box = static_cast<holder<T>*>(lua_touserdata(L, index));
    if (!lua_getmetatable(L, index)) return nullptr;
    push_metatable<T, Callable>(L);
    const bool matches = lua_rawequal(L, -1, -2) != 0;
    lua_pop(L, 2);
    return matches ? box : nullptr;
}
// A weak-value cache loses pending-finalization userdata before __gc runs.
// Weak-key sets retain those keys through finalization in LuaJIT, allowing a
// native destruction notification to invalidate even a queued wrapper.
inline void forget_reference(lua_State* L, const object_storage& box, int wrapper) noexcept {
    if (!box.ref_policy) return;
    stack_guard stack(L);
    wrapper = absolute_index(L, wrapper);
    lua_pushlightuserdata(L, box.ref_policy);
    lua_rawget(L, LUA_REGISTRYINDEX);
    lua_rawgeti(L, -1, 3);
    const int identities = lua_gettop(L);
    lua_pushlightuserdata(L, box.ref_identity);
    lua_rawget(L, identities);
    if (!lua_istable(L, -1)) return;
    lua_pushvalue(L, wrapper);
    lua_pushnil(L);
    lua_rawset(L, -3);
    lua_pushnil(L);
    if (lua_next(L, -2)) return;
    lua_pushlightuserdata(L, box.ref_identity);
    lua_pushnil(L);
    lua_rawset(L, identities);
}
template<class T, bool Callable = false> int collect(lua_State* L) noexcept {
    auto* box = test_holder<T, Callable>(L, 1);
    if (box) {
        forget_reference(L, *box, 1);
        clear_object(*box);
    }
    return 0;
}
template<class T, bool Callable = false> holder<T>* new_holder(lua_State* L) {
    reserve(L, 4);
    if (!push_metatable<T, Callable>(L)) {
        lua_pop(L, 1);
        throw error("C++ type has not been registered in this Lua state");
    }
    // Store a pointer in Lua userdata; even over-aligned C++ types are allocated by new.
    if constexpr (!Callable) {
        lua_pushboolean(L, true);
        lua_setfield(L, -2, "instances_created");
    }
    auto* box = new (lua_newuserdata(L, sizeof(holder<T>))) holder<T>{};
    lua_pushvalue(L, -2);
    lua_setmetatable(L, -2);
    lua_remove(L, -2);
    return box;
}
// Policies are trivial Lua userdata. Each retained wrapper copies its release
// hook, so finalization does not depend on the policy's shutdown order.
struct ref_policy {
    ref_ownership ownership;
    void (*acquire)(object_storage&, void*, const ref_policy&);
};
template<class T> struct registered_ref_policy { ref_policy policy; arky_ref_object<T> hooks; };
template<class T> struct retained_ref {
    T* value;
    void (*decRef)(T*) noexcept;
};
struct reference_view { const ref_policy* policy = nullptr; void* identity = nullptr; };

template<class T> reference_view reference_policy(lua_State* L, T* value = nullptr) {
    stack_guard stack(L);
    reserve(L, 6);
    if (!push_metatable<std::remove_const_t<T>>(L)) return {};
    void* identity = const_cast<std::remove_const_t<T>*>(value);
    while (lua_istable(L, -1)) {
        lua_pushlightuserdata(L, &ref_policy_key);
        lua_rawget(L, -2);
        if (lua_isuserdata(L, -1))
            return {static_cast<ref_policy*>(lua_touserdata(L, -1)), identity};
        lua_pop(L, 1);
        lua_getfield(L, -1, "upcast");
        if (identity && lua_type(L, -1) == LUA_TUSERDATA)
            identity = (*static_cast<upcast_function*>(lua_touserdata(L, -1)))(identity);
        lua_pop(L, 1);
        lua_getfield(L, -1, "base");
        lua_remove(L, -2);
    }
    return {};
}

// Return the adjusted pointer only when the registered base chain reaches to.
inline void* upcast_pointer(lua_State* L, int from, int to, void* value) {
    stack_guard stack(L);
    from = absolute_index(L, from); to = absolute_index(L, to);
    lua_pushvalue(L, from);
    while (!lua_rawequal(L, -1, to)) {
        lua_getfield(L, -1, "upcast");
        if (lua_type(L, -1) != LUA_TUSERDATA || lua_objlen(L, -1) != sizeof(upcast_function)) return nullptr;
        value = (*static_cast<upcast_function*>(lua_touserdata(L, -1)))(value);
        lua_pop(L, 1);
        lua_getfield(L, -1, "base");
        if (!lua_istable(L, -1)) return nullptr;
        lua_remove(L, -2);
    }
    return value;
}

template<class T> void push_reference(lua_State* L, T* value, reference_view reference) {
    stack_guard stack(L);
    reserve(L, 10);
    lua_pushlightuserdata(L, const_cast<ref_policy*>(reference.policy));
    lua_rawget(L, LUA_REGISTRYINDEX);
    // Const and mutable views cannot share a userdata without weakening const.
    lua_rawgeti(L, -1, std::is_const_v<T> ? 2 : 1);
    const int cache = lua_gettop(L);
    lua_pushlightuserdata(L, reference.identity);
    lua_rawget(L, cache);
    auto* box = lua_type(L, -1) == LUA_TUSERDATA && lua_objlen(L, -1) == sizeof(object_storage)
        ? static_cast<object_storage*>(lua_touserdata(L, -1)) : nullptr;
    if (box && box->value) {
        const int wrapper = lua_gettop(L);
        lua_getmetatable(L, wrapper);
        const int current = lua_gettop(L);
        if (!push_metatable<std::remove_const_t<T>>(L)) throw error("C++ type has not been registered");
        const int requested = lua_gettop(L);
        if (upcast_pointer(L, current, requested, box->value) != value) {
            if (upcast_pointer(L, requested, current, const_cast<std::remove_const_t<T>*>(value)) != box->value)
                throw error("incompatible native reference view");
            // A later derived exposure upgrades a base-first wrapper in place.
            box->value = const_cast<std::remove_const_t<T>*>(value);
            lua_pushvalue(L, requested);
            lua_setmetatable(L, wrapper);
        }
        // Reusing an identity still exposes the requested class. In particular,
        // an upgrade allocates no holder, so new_holder cannot seal its setup.
        lua_pushboolean(L, true);
        lua_setfield(L, requested, "instances_created");
        lua_settop(L, wrapper);
    } else {
        lua_pop(L, 1);
        box = new_holder<std::remove_const_t<T>>(L);
        reference.policy->acquire(*box, reference.identity, *reference.policy);
        box->value = const_cast<std::remove_const_t<T>*>(value);
        box->writable = !std::is_const_v<T>;
        box->ref_policy = const_cast<ref_policy*>(reference.policy);
        box->ref_identity = reference.identity;
        const int wrapper = lua_gettop(L);
        lua_rawgeti(L, cache - 1, 3);
        const int identities = lua_gettop(L);
        lua_pushlightuserdata(L, reference.identity);
        lua_rawget(L, identities);
        if (lua_isnil(L, -1)) {
            lua_pop(L, 1);
            lua_newtable(L);
            lua_newtable(L);
            lua_pushliteral(L, "k");
            lua_setfield(L, -2, "__mode");
            lua_setmetatable(L, -2);
            lua_pushlightuserdata(L, reference.identity);
            lua_pushvalue(L, -2);
            lua_rawset(L, identities);
        }
        lua_pushvalue(L, wrapper);
        lua_pushboolean(L, true);
        lua_rawset(L, -3);
        lua_settop(L, wrapper);
        lua_pushlightuserdata(L, reference.identity);
        lua_pushvalue(L, -2);
        lua_rawset(L, cache);
    }
    lua_replace(L, stack.top + 1);
    ++stack.top;
}

template<class T> std::size_t invalidate_reference(lua_State* L, T* value) {
    stack_guard stack(L);
    reserve(L, 8);
    if (!value) return 0;
    const auto reference = reference_policy(L, value);
    if (!reference.policy) throw error("native reference policy has not been registered");
    lua_pushlightuserdata(L, const_cast<ref_policy*>(reference.policy));
    lua_rawget(L, LUA_REGISTRYINDEX);
    const int caches = lua_gettop(L);
    std::size_t invalidated = 0;
    lua_rawgeti(L, caches, 3);
    const int identities = lua_gettop(L);
    lua_pushlightuserdata(L, reference.identity);
    lua_rawget(L, identities);
    if (lua_istable(L, -1)) {
        lua_pushnil(L);
        while (lua_next(L, -2)) {
            if (lua_type(L, -2) == LUA_TUSERDATA && lua_objlen(L, -2) == sizeof(object_storage)) {
                auto* box = static_cast<object_storage*>(lua_touserdata(L, -2));
                if (box->value) { clear_object(*box, true); ++invalidated; }
            }
            lua_pop(L, 1);
        }
    }
    lua_pop(L, 1);
    lua_pushlightuserdata(L, reference.identity);
    lua_pushnil(L);
    lua_rawset(L, identities);
    lua_pop(L, 1);
    for (int view : {1, 2}) {
        lua_rawgeti(L, caches, view);
        lua_pushlightuserdata(L, reference.identity);
        lua_pushnil(L);
        lua_rawset(L, -3);
        lua_pop(L, 1);
    }
    return invalidated;
}

template<class T, bool Callable = false, class... A> void emplace(lua_State* L, A&&... args) {
    static_assert(std::is_nothrow_destructible_v<T>);
    if constexpr (!Callable) {
        if (reference_policy<T>(L).policy) throw error("native reference objects require a pointer factory");
    }
    auto* box = new_holder<T, Callable>(L);
    own_shared(*box, std::make_shared<T>(std::forward<A>(args)...));
}
// A view adjusts the pointer but keeps the original allocation/control block.
// Only class metatables carry this marker; callable userdata cannot be upcast.
template<class T> object_storage object_view(lua_State* L, int index) {
    using U = std::remove_const_t<T>;
    stack_guard stack(L);
    reserve(L, 6);
    index = absolute_index(L, index);
    if (lua_type(L, index) != LUA_TUSERDATA || lua_objlen(L, index) != sizeof(object_storage)
        || !lua_getmetatable(L, index))
        throw error("argument " + std::to_string(index) + ": wrong or expired C++ object");
    lua_pushlightuserdata(L, &object_metatable_key);
    lua_rawget(L, -2);
    const bool registered = lua_toboolean(L, -1) != 0;
    lua_pop(L, 1);
    if (!registered) throw error("wrong or expired C++ object");
    auto view = *static_cast<object_storage*>(lua_touserdata(L, index));
    if (!view.value) throw error("wrong or expired C++ object");
    if (!push_metatable<U>(L)) throw error("wrong or expired C++ object");
    const int target = lua_gettop(L);
    const int current = target - 1;
    while (!lua_rawequal(L, current, target)) {
        lua_getfield(L, current, "upcast");
        if (lua_type(L, -1) != LUA_TUSERDATA || lua_objlen(L, -1) != sizeof(upcast_function))
            throw error("wrong or expired C++ object");
        view.value = (*static_cast<upcast_function*>(lua_touserdata(L, -1)))(view.value);
        lua_pop(L, 1);
        lua_getfield(L, current, "base");
        if (!lua_istable(L, -1)) throw error("wrong or expired C++ object");
        lua_replace(L, current);
    }
    if constexpr (!std::is_const_v<T>) {
        if (!view.writable) throw error("cannot modify a const C++ object");
    }
    return view;
}
template<class T> T* object(lua_State* L, int index) {
    return static_cast<T*>(object_view<T>(L, index).value);
}

template<class T> struct optional_type : std::false_type {};
template<class T> struct optional_type<std::optional<T>> : std::true_type { using value_type = T; };
template<class T> struct vector_type : std::false_type {};
template<class T, class A> struct vector_type<std::vector<T, A>> : std::true_type { using value_type = T; };
template<class T> struct shared_type : std::false_type {};
template<class T> struct shared_type<std::shared_ptr<T>> : std::true_type { using value_type = T; };
template<class T> inline constexpr bool string_type =
    std::is_same_v<T, std::string> || std::is_same_v<T, std::string_view> ||
    std::is_same_v<T, const char*>;
template<class T> inline constexpr bool object_type =
    std::is_class_v<T> && !string_type<T> && !optional_type<T>::value &&
    !vector_type<T>::value && !shared_type<T>::value && !value_traits<T>::enabled && !converter<T>::enabled;
inline constexpr double max_integer = 9007199254740991.0;

template<class T> T read_converted(lua_State* L, int index) {
    static_assert(std::is_class_v<T> || std::is_enum_v<T>,
                  "converter requires a C++ value class or enum; wrap pointers in a value type");
    static_assert(std::is_same_v<decltype(converter<T>::read(L, index)), T>,
                  "converter<T>::read must return T by value");
    stack_guard stack(L);
    reserve(L, 8);
    if (lua_type(L, index) == LUA_TNONE) throw error("invalid converter stack index");
    T value = converter<T>::read(L, absolute_index(L, index));
    if (lua_gettop(L) != stack.top) throw error("converter read must preserve the Lua stack");
    return value;
}
template<class T> void push_converted(lua_State* L, T&& value) {
    using U = plain<T>;
    static_assert(std::is_class_v<U> || std::is_enum_v<U>,
                  "converter requires a C++ value class or enum; wrap pointers in a value type");
    stack_guard stack(L);
    reserve(L, 8);
    converter<U>::push(L, std::forward<T>(value));
    if (lua_gettop(L) != stack.top + 1) throw error("converter push must append exactly one Lua value");
    ++stack.top;
}

// Validate array shape before conversion; lua_next order must not decide vector order.
inline int array_size(lua_State* L, int index) {
    expect(L, index, LUA_TTABLE);
    const auto size = lua_objlen(L, index);
    if (size > static_cast<std::size_t>(std::numeric_limits<int>::max()))
        throw error("Lua array is too large for std::vector conversion");
    int count = 0;
    lua_pushnil(L);
    while (lua_next(L, index)) {
        if (lua_type(L, -2) != LUA_TNUMBER) throw error("expected a dense Lua array with keys 1..N");
        const double key = lua_tonumber(L, -2);
        if (key < 1 || key > static_cast<double>(size) || std::trunc(key) != key)
            throw error("expected a dense Lua array with keys 1..N");
        ++count;
        lua_pop(L, 1);
    }
    if (count != static_cast<int>(size)) throw error("expected a dense Lua array with keys 1..N");
    return count;
}

template<class T> decltype(auto) read(lua_State* L, int index) {
    using U = plain<T>;
    static_assert(!std::is_rvalue_reference_v<T>, "Lua arguments cannot bind to C++ rvalue references");
    static_assert(!std::is_lvalue_reference_v<T> || std::is_const_v<std::remove_reference_t<T>> || object_type<U>,
                  "Mutable references require a registered C++ class");
    if constexpr (converter<U>::enabled) {
        return read_converted<U>(L, index);
    } else if constexpr (value_traits<U>::enabled) {
        return value_traits<U>::read(L, index);
    } else if constexpr (std::is_same_v<U, bool>) {
        expect(L, index, LUA_TBOOLEAN);
        return lua_toboolean(L, index) != 0;
    } else if constexpr (std::is_enum_v<U>) {
        return static_cast<U>(read<std::underlying_type_t<U>>(L, index));
    } else if constexpr (std::is_arithmetic_v<U>) {
        expect(L, index, LUA_TNUMBER);
        const double number = lua_tonumber(L, index);
        if (!std::isfinite(number)) throw error("expected a finite number");
        if constexpr (std::is_integral_v<U>) {
            if (std::trunc(number) != number || std::abs(number) > max_integer)
                throw error("expected an exact integer within Lua's safe number range");
        }
        if (static_cast<long double>(number) < static_cast<long double>(std::numeric_limits<U>::lowest()) ||
            static_cast<long double>(number) > static_cast<long double>(std::numeric_limits<U>::max()))
            throw error("number is outside the C++ type's range");
        return static_cast<U>(number);
    } else if constexpr (string_type<U>) {
        expect(L, index, LUA_TSTRING);
        std::size_t size = 0;
        const char* text = lua_tolstring(L, index, &size);
        if constexpr (std::is_same_v<U, std::string>) return std::string(text, size);
        else if constexpr (std::is_same_v<U, std::string_view>) return std::string_view(text, size);
        else return text;
    } else if constexpr (optional_type<U>::value) {
        if (lua_isnil(L, index)) return U{};
        return U{read<typename optional_type<U>::value_type>(L, index)};
    } else if constexpr (vector_type<U>::value) {
        stack_guard stack(L);
        reserve(L, 8);
        const int table = absolute_index(L, index);
        const int count = array_size(L, table);
        U values;
        values.reserve(static_cast<std::size_t>(count));
        for (int i = 0; i < count; ++i) {
            lua_rawgeti(L, table, i + 1);
            values.push_back(read<typename vector_type<U>::value_type>(L, -1));
            lua_pop(L, 1);
        }
        return values;
    } else if constexpr (shared_type<U>::value) {
        using P = typename shared_type<U>::value_type;
        static_assert(object_type<std::remove_const_t<P>>, "shared_ptr requires a registered C++ class");
        if (lua_isnil(L, index)) return U{};
        const auto view = object_view<P>(L, index);
        if (!view.shared) throw error("C++ object has no shared ownership");
        return U(*view.shared, static_cast<P*>(view.value));
    } else if constexpr (std::is_pointer_v<U> && object_type<std::remove_cv_t<std::remove_pointer_t<U>>>) {
        if (lua_isnil(L, index)) return U{nullptr};
        return object<std::remove_pointer_t<U>>(L, index);
    } else if constexpr (object_type<U>) {
        if constexpr (std::is_lvalue_reference_v<T>) return *object<std::remove_reference_t<T>>(L, index);
        else return U(*object<const U>(L, index));
    } else {
        static_assert(unsupported<T>, "Unsupported Lua conversion");
    }
}

template<class T> void push(lua_State* L, T&& value) {
    using U = plain<T>;
    reserve(L, 4);
    if constexpr (converter<U>::enabled) {
        push_converted(L, std::forward<T>(value));
    } else if constexpr (value_traits<U>::enabled) {
        value_traits<U>::push(L, std::forward<T>(value));
    } else if constexpr (std::is_same_v<U, std::nullptr_t>) {
        lua_pushnil(L);
    } else if constexpr (std::is_same_v<U, bool>) {
        lua_pushboolean(L, value);
    } else if constexpr (std::is_enum_v<U>) {
        push(L, static_cast<std::underlying_type_t<U>>(value));
    } else if constexpr (std::is_arithmetic_v<U>) {
        if constexpr (std::is_integral_v<U>) {
            if (static_cast<long double>(value) < -max_integer || static_cast<long double>(value) > max_integer)
                throw error("integer is outside Lua's safe number range");
        }
        const double number = static_cast<double>(value);
        if (!std::isfinite(number)) throw error("expected a finite number");
        lua_pushnumber(L, number);
    } else if constexpr (std::is_same_v<U, std::string> || std::is_same_v<U, std::string_view>) {
        lua_pushlstring(L, value.data(), value.size());
    } else if constexpr (std::is_convertible_v<T, const char*>) {
        lua_pushstring(L, value); // nullptr maps to nil, matching the Lua C API.
    } else if constexpr (optional_type<U>::value) {
        if (value) push(L, *std::forward<T>(value));
        else lua_pushnil(L);
    } else if constexpr (vector_type<U>::value) {
        stack_guard stack(L);
        if (value.size() > static_cast<std::size_t>(std::numeric_limits<int>::max()))
            throw error("std::vector is too large for a Lua array");
        const int size = static_cast<int>(value.size());
        lua_createtable(L, size, 0);
        for (int i = 0; i < size; ++i) {
            if constexpr (std::is_same_v<typename vector_type<U>::value_type, bool>)
                push(L, static_cast<bool>(value[static_cast<std::size_t>(i)]));
            else if constexpr (std::is_rvalue_reference_v<T&&>)
                push(L, std::move(value[static_cast<std::size_t>(i)]));
            else
                push(L, value[static_cast<std::size_t>(i)]);
            if (lua_isnil(L, -1)) throw error("Lua array elements cannot be nil");
            lua_rawseti(L, -2, i + 1);
        }
        ++stack.top; // Successful push leaves exactly one table; errors restore the stack.
    } else if constexpr (shared_type<U>::value) {
        using P = typename shared_type<U>::value_type;
        using V = std::remove_const_t<P>;
        static_assert(object_type<V>, "shared_ptr requires a registered C++ class");
        if (!value) { lua_pushnil(L); return; }
        if (reference_policy<V>(L).policy) throw error("native reference policy cannot use shared_ptr ownership");
        auto* box = new_holder<V>(L);
        own_shared(*box, std::const_pointer_cast<V>(std::forward<T>(value)));
        box->writable = !std::is_const_v<P>;
    } else if constexpr (std::is_pointer_v<U> && object_type<std::remove_cv_t<std::remove_pointer_t<U>>>) {
        if (!value) { lua_pushnil(L); return; }
        using P = std::remove_pointer_t<U>;
        const auto reference = reference_policy(L, value);
        if (reference.policy) { push_reference(L, value, reference); return; }
        auto* box = new_holder<std::remove_const_t<P>>(L);
        box->value = const_cast<std::remove_const_t<P>*>(value);
        box->writable = !std::is_const_v<P>;
    } else if constexpr (object_type<U>) {
        emplace<U>(L, std::forward<T>(value));
    } else {
        static_assert(unsupported<T>, "Unsupported Lua conversion");
    }
}

template<class T> constexpr void check_result() {
    using U = plain<T>;
    static_assert(!std::is_reference_v<T> && !std::is_pointer_v<U> && !std::is_same_v<U, std::string_view>,
                  "Host results must own their data; use values, std::string or std::optional<T>");
    if constexpr (optional_type<U>::value) check_result<typename optional_type<U>::value_type>();
    if constexpr (vector_type<U>::value) check_result<typename vector_type<U>::value_type>();
}
} // namespace detail

// Low-level conversion entry points. A carg owns converted
// scalar/string/container temporaries; native class references still borrow the object.
template<class T> class carg {
    using storage_type = decltype(detail::read<T>(std::declval<lua_State*>(), 1));
    static storage_type convert(lua_State* L, int index) {
        if (!L) throw error("carg requires a live Lua state");
        detail::reserve(L, 8);
        return detail::read<T>(L, index);
    }
public:
    explicit carg(lua_State* L, int index) : value_(convert(L, index)) {}
    [[nodiscard]] T to() { return value_; }
private:
    storage_type value_;
};

template<class T> struct pusher {
    static void push(lua_State* L, T value) {
        if (!L) throw error("pusher requires a live Lua state");
        detail::stack_guard stack(L);
        detail::push(L, std::forward<T>(value));
        ++stack.top;
    }
};
template<class T> struct result {
    static void push_im(lua_State* L, T value) { pusher<T>::push(L, std::forward<T>(value)); }
};
} // namespace next2::luax
