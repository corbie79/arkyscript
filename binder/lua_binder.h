#pragma once

// C++20 function and class registration, script execution, and host calls.
// Author: eugene.choi.
#include "lua_class.h"
#include "lua_value.h"

namespace next2::luax {

class binder {
public:
    explicit binder(lua_State* L) : L_(L) {
        if (!L_) throw error("binder requires a live Lua state");
    }
    explicit binder(state& owner) : binder(owner.get()) {}
    [[nodiscard]] lua_State* get_state() const noexcept { return L_; }
    void set_namespace(std::string name) {
        if (!name.empty()) detail::validate_path(name);
        namespace_ = std::move(name);
    }
    [[nodiscard]] const std::string& get_namespace() const noexcept { return namespace_; }

    template<class F> requires (!std::is_convertible_v<F, lua_CFunction>)
    binder& reg_func(std::string_view name, F&& function) {
        detail::stack_guard stack(L_);
        detail::reserve(L_, 8);
        detail::push_function(L_, std::forward<F>(function));
        detail::set_path(L_, qualify(name));
        return *this;
    }
    // Include noexcept pointers and captureless lambdas: the forwarding overload
    // must not treat their lua_State* parameter as a Lua argument to convert.
    // Raw C API callbacks retain their native Lua stack/return-count convention.
    binder& reg_func(std::string_view name, lua_CFunction function) {
        if (!function) throw error("cannot register a null function");
        detail::stack_guard stack(L_);
        detail::reserve(L_, 8);
        lua_pushcfunction(L_, function);
        detail::set_path(L_, qualify(name));
        return *this;
    }
    template<class T> cpp_class<T> reg_class(std::string_view name) {
        return cpp_class<T>(L_, qualify(name));
    }
    template<class T> binder& reg_ref_object(arky_ref_object<T> policy) {
        detail::register_ref_object(L_, policy);
        return *this;
    }
    // Invoke on the VM thread from the registered root type's destruction hook.
    // Invalidates all const/mutable views without calling decRef a second time.
    template<class T> std::size_t invalidate_ref_object(T* value) {
        return detail::invalidate_reference(L_, value);
    }
    template<class T> void set_var(std::string_view name, T&& value) {
        detail::stack_guard stack(L_);
        detail::reserve(L_, 8);
        detail::push(L_, std::forward<T>(value));
        detail::set_path(L_, qualify(name));
    }
    template<class T> [[nodiscard]] T get_var(std::string_view name) const {
        detail::check_result<T>();
        detail::stack_guard stack(L_);
        detail::reserve(L_, 8);
        detail::get_path(L_, qualify(name));
        return detail::read<T>(L_, -1);
    }
    template<class R = void, class... A> R call(std::string_view name, A&&... args) const {
        detail::check_result<R>();
        detail::stack_guard stack(L_);
        detail::reserve(L_, static_cast<int>(sizeof...(A)) + 8);
        detail::get_path(L_, qualify(name));
        detail::expect(L_, -1, LUA_TFUNCTION);
        (detail::push(L_, std::forward<A>(args)), ...);
        detail::check_status(L_, lua_pcall(L_, sizeof...(A), std::is_void_v<R> ? 0 : 1, 0));
        if constexpr (!std::is_void_v<R>) return detail::read<R>(L_, -1);
    }
    void dostring(std::string_view source) const { dobuffer(source, "=(tynex)"); }
    void dobuffer(std::string_view source, const char* chunk_name = "=(tynex)") const {
        if (!chunk_name) throw error("chunk name cannot be null");
        detail::stack_guard stack(L_);
        detail::reserve(L_, 8);
        detail::check_status(L_, luaL_loadbuffer(L_, source.data(), source.size(), chunk_name));
        detail::check_status(L_, lua_pcall(L_, 0, 0, 0));
    }
    void dofile(const char* path) const {
        if (!path) throw error("file path cannot be null");
        detail::stack_guard stack(L_);
        detail::reserve(L_, 8);
        detail::check_status(L_, luaL_loadfile(L_, path));
        detail::check_status(L_, lua_pcall(L_, 0, 0, 0));
    }
private:
    std::string qualify(std::string_view name) const {
        detail::validate_path(name);
        if (namespace_.empty()) return std::string(name);
        return namespace_ + "." + std::string(name);
    }
    lua_State* L_;
    std::string namespace_;
};
} // namespace next2::luax
