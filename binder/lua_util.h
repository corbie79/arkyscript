#pragma once

// Lua state lifetime, stack management, and error handling.
// Author: eugene.choi.
#include <lua.hpp>
#include <memory>
#include <new>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>

namespace next2::luax {
class error : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

namespace detail {
struct stack_guard {
    lua_State* state;
    int top;
    explicit stack_guard(lua_State* L) : state(L), top(lua_gettop(L)) {}
    ~stack_guard() { lua_settop(state, top); }
    stack_guard(const stack_guard&) = delete;
    stack_guard& operator=(const stack_guard&) = delete;
};

inline void reserve(lua_State* L, int count) {
    if (!lua_checkstack(L, count)) throw error("Lua stack capacity exceeded");
}
inline void check_status(lua_State* L, int status) {
    if (status == 0) return;
    if (lua_type(L, -1) == LUA_TSTRING) {
        std::size_t size = 0;
        const char* text = lua_tolstring(L, -1, &size);
        throw error(std::string(text, size));
    }
    throw error("Lua error (non-string value)");
}
inline void expect(lua_State* L, int index, int type) {
    if (lua_type(L, index) != type)
        throw error("argument " + std::to_string(index) + ": expected " + lua_typename(L, type));
}

// One token per VM, shared by all its coroutines. Its finalizer also observes
// lua_close() on states owned by an external application.
struct vm_lifetime { bool alive = true; };
struct lifetime_token { std::shared_ptr<vm_lifetime>* value; };
inline char lifetime_key;

inline int release_lifetime(lua_State* L) noexcept {
    if (lua_type(L, 1) != LUA_TUSERDATA || lua_objlen(L, 1) != sizeof(lifetime_token)) return 0;
    auto* token = static_cast<lifetime_token*>(lua_touserdata(L, 1));
    if (token->value) {
        (*token->value)->alive = false;
        delete std::exchange(token->value, nullptr);
    }
    return 0;
}
inline std::shared_ptr<vm_lifetime> lifetime_of(lua_State* L) {
    if (!L) throw error("expected a live Lua state");
    stack_guard stack(L);
    reserve(L, 6);
    lua_pushlightuserdata(L, &lifetime_key);
    lua_rawget(L, LUA_REGISTRYINDEX);
    if (lua_isnil(L, -1)) {
        lua_pop(L, 1);
        auto* token = new (lua_newuserdata(L, sizeof(lifetime_token))) lifetime_token{nullptr};
        lua_newtable(L);
        lua_pushcfunction(L, release_lifetime);
        lua_setfield(L, -2, "__gc");
        lua_pushboolean(L, false);
        lua_setfield(L, -2, "__metatable");
        lua_setmetatable(L, -2);
        token->value = new std::shared_ptr<vm_lifetime>(std::make_shared<vm_lifetime>());
        lua_pushlightuserdata(L, &lifetime_key);
        lua_pushvalue(L, -2);
        lua_rawset(L, LUA_REGISTRYINDEX);
    }
    if (lua_type(L, -1) != LUA_TUSERDATA || lua_objlen(L, -1) != sizeof(lifetime_token))
        throw error("invalid Lua VM lifetime token");
    auto* token = static_cast<lifetime_token*>(lua_touserdata(L, -1));
    if (!token->value || !(*token->value)->alive) throw error("Lua VM has expired");
    return *token->value;
}

inline void validate_path(std::string_view path) {
    if (path.empty() || path.front() == '.' || path.back() == '.' ||
        path.find("..") != std::string_view::npos || path.find('\0') != std::string_view::npos)
        throw error("expected a non-empty dotted Lua name");
}
inline void get_path(lua_State* L, std::string_view path) {
    validate_path(path);
    lua_pushvalue(L, LUA_GLOBALSINDEX);
    while (true) {
        if (!lua_istable(L, -1)) throw error("Lua name's parent is not a table");
        const auto dot = path.find('.');
        const auto part = path.substr(0, dot);
        lua_pushlstring(L, part.data(), part.size());
        lua_rawget(L, -2);
        lua_remove(L, -2);
        if (dot == std::string_view::npos) return;
        path.remove_prefix(dot + 1);
    }
}
inline void set_path(lua_State* L, std::string_view path) {
    validate_path(path);
    const int value = lua_gettop(L);
    lua_pushvalue(L, LUA_GLOBALSINDEX);
    while (true) {
        const auto dot = path.find('.');
        const auto part = path.substr(0, dot);
        if (dot == std::string_view::npos) {
            lua_pushlstring(L, part.data(), part.size());
            lua_pushvalue(L, value);
            lua_rawset(L, -3);
            lua_pop(L, 1);
            return;
        }
        lua_pushlstring(L, part.data(), part.size());
        lua_rawget(L, -2);
        if (lua_isnil(L, -1)) {
            lua_pop(L, 1);
            lua_newtable(L);
            lua_pushlstring(L, part.data(), part.size());
            lua_pushvalue(L, -2);
            lua_rawset(L, -4);
        }
        if (!lua_istable(L, -1)) throw error("Lua name's parent is not a table");
        lua_remove(L, -2);
        path.remove_prefix(dot + 1);
    }
}
} // namespace detail

// Optional owner. A binder can also attach to an application's existing lua_State.
class state {
public:
    explicit state(bool open_libraries = true) : state_(luaL_newstate()) {
        if (!state_) throw std::bad_alloc();
        lifetime_ = detail::lifetime_of(state_.get());
        if (open_libraries) luaL_openlibs(state_.get());
    }
    ~state() { lifetime_->alive = false; }
    [[nodiscard]] lua_State* get() const noexcept { return state_.get(); }
    state(const state&) = delete;
    state& operator=(const state&) = delete;
    state(state&&) = delete;
    state& operator=(state&&) = delete;
private:
    struct closer { void operator()(lua_State* L) const noexcept { lua_close(L); } };
    std::unique_ptr<lua_State, closer> state_;
    std::shared_ptr<detail::vm_lifetime> lifetime_;
};

} // namespace next2::luax
