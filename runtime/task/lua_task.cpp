#include "task_pool.h"
#include "binder/lua_cclosure.h"
#include "binder/lua_value.h"
#include <cmath>
#include <cstring>
#include <string>

namespace {
constexpr const char* pool_type = "tynex.task.pool";
constexpr const char* handle_type = "tynex.task.handle";
constexpr std::size_t transfer_limit = 8 * 1024 * 1024;
using handle = tynex::task<tynex::value>;
template<class T> inline char box_tag;
struct pool_box {
    const void* tag = &box_tag<pool_box>;
    std::unique_ptr<tynex::task_pool> pool;
    std::size_t workers = tynex::task_pool::default_workers();
    std::size_t capacity = 4096;
    bool closed = false;
};
struct handle_box {
    const void* tag = &box_tag<handle_box>;
    std::optional<handle> value;
};
thread_local const tynex::cancellation_token* worker_stop = nullptr;

template<class T> T& checked_box(lua_State* L, int index, const char* type) {
    // Non-raising Lua checks avoid foreign unwinds through the C++ boundary.
    if (lua_type(L, index) != LUA_TUSERDATA)
        throw tynex::error(std::string("expected ") + type + " userdata");
    if (lua_objlen(L, index) != sizeof(T))
        throw tynex::error("invalid task userdata storage");
    if (!lua_getmetatable(L, index)) throw tynex::error("task userdata has no metatable");
    luaL_getmetatable(L, type);
    const bool correct_type = lua_rawequal(L, -1, -2) != 0;
    lua_pop(L, 2);
    if (!correct_type) throw tynex::error("incorrect task userdata metatable");
    void* memory = lua_touserdata(L, index);
    const void* tag = nullptr;
    std::memcpy(&tag, memory, sizeof(tag));
    if (tag != &box_tag<T>) throw tynex::error("invalid task userdata tag");
    return *static_cast<T*>(memory);
}

void copyable(const tynex::value& value, std::size_t& bytes) {
    bytes += sizeof(tynex::value);
    std::visit([&](const auto& item) {
        using T = std::decay_t<decltype(item)>;
        if constexpr (std::is_same_v<T, tynex::object>) {
            throw tynex::error("task transfer requires plain value trees; objects, functions, cycles, shared tables and metatables cannot cross VMs");
        } else if constexpr (std::is_same_v<T, std::string>) {
            bytes += item.size();
        } else if constexpr (std::is_same_v<T, double>) {
            if (!std::isfinite(item)) throw tynex::error("task transfer requires finite numbers");
        } else if constexpr (std::is_same_v<T, tynex::value::array>) {
            for (const auto& child : item) copyable(child, bytes);
        } else if constexpr (std::is_same_v<T, tynex::value::table>) {
            for (const auto& [key, child] : item) {
                if (const auto* name = std::get_if<std::string>(&key)) bytes += name->size();
                copyable(child, bytes);
            }
        }
    }, value.data);
    if (bytes > transfer_limit) throw tynex::error("task transfer exceeds the 8 MiB value budget");
}

tynex::value read_copy(lua_State* L, int index) {
    auto result = tynex::carg<tynex::value>(L, index).to();
    std::size_t bytes = 0;
    copyable(result, bytes);
    return result;
}

// Writer callbacks must not unwind through Lua's C frames.
int dump_writer(lua_State*, const void* data, std::size_t size, void* context) noexcept {
    auto& bytes = *static_cast<std::string*>(context);
    if (size > transfer_limit - bytes.size()) return 1;
    try { bytes.append(static_cast<const char*>(data), size); return 0; }
    catch (...) { return 1; }
}

int checkpoint(lua_State* L) {
    if (worker_stop && worker_stop->stop_requested()) return luaL_error(L, "task cancelled");
    return 0;
}
int cancelled(lua_State* L) {
    lua_pushboolean(L, worker_stop && worker_stop->stop_requested());
    return 1;
}

struct stop_scope {
    explicit stop_scope(const tynex::cancellation_token& token) { worker_stop = &token; }
    ~stop_scope() { worker_stop = nullptr; }
};

tynex::value execute(const std::string& code, const tynex::value& input, tynex::cancellation_token stop) {
    if (stop.stop_requested()) throw tynex::task_cancelled();
    stop_scope context(stop);
    tynex::state vm;
    lua_State* L = vm.get();
    if (luaL_loadbuffer(L, code.data(), code.size(), "@task") != 0)
        throw tynex::error(lua_tostring(L, -1));
    tynex::pusher<tynex::value>::push(L, input);
    lua_newtable(L);
    lua_pushcfunction(L, checkpoint);
    lua_setfield(L, -2, "checkpoint");
    lua_pushcfunction(L, cancelled);
    lua_setfield(L, -2, "cancelled");
    // No Lua references leave this VM. The JIT remains enabled normally.
    if (lua_pcall(L, 2, 1, 0) != 0) {
        if (stop.stop_requested()) throw tynex::task_cancelled();
        const char* error = lua_tostring(L, -1);
        throw tynex::error(error ? error : "task raised a non-string error");
    }
    if (stop.stop_requested()) throw tynex::task_cancelled();
    return read_copy(L, -1);
}

pool_box& pool(lua_State* L) {
    return checked_box<pool_box>(L, lua_upvalueindex(1), pool_type);
}
handle& job(lua_State* L) {
    auto& box = checked_box<handle_box>(L, 1, handle_type);
    if (!box.value) throw tynex::error("task handle has been released");
    return *box.value;
}

int configure(lua_State* L) {
    auto& owner = pool(L);
    if (owner.pool || owner.closed) throw tynex::error("configure must run before the first task");
    const int workers = tynex::carg<int>(L, 1).to();
    const int capacity = tynex::carg<int>(L, 2).to();
    if (workers < 1 || workers > 64 || capacity < 1)
        throw tynex::error("configure expects 1..64 workers and a positive queue capacity");
    owner.workers = static_cast<std::size_t>(workers);
    owner.capacity = static_cast<std::size_t>(capacity);
    return 0;
}

int spawn(lua_State* L) {
    auto& owner = pool(L);
    if (owner.closed) throw tynex::error("task pool is shut down");
    if (!lua_isfunction(L, 1) || lua_iscfunction(L, 1))
        throw tynex::error("task.spawn expects a Lua function");
    if (lua_getupvalue(L, 1, 1)) {
        lua_pop(L, 1);
        throw tynex::error("task functions cannot capture upvalues; pass values in the input argument");
    }
    auto input = lua_isnone(L, 2) ? tynex::value() : read_copy(L, 2);
    std::string code;
    lua_pushvalue(L, 1);
    const int dumped = lua_dump(L, dump_writer, &code);
    lua_pop(L, 1);
    if (dumped) throw tynex::error("cannot serialize task function (8 MiB bytecode limit)");
    auto* box = new (lua_newuserdata(L, sizeof(handle_box))) handle_box;
    luaL_getmetatable(L, handle_type);
    lua_setmetatable(L, -2);
    if (!owner.pool) owner.pool = std::make_unique<tynex::task_pool>(owner.workers, owner.capacity);
    box->value.emplace(owner.pool->submit([code = std::move(code), input = std::move(input)](tynex::cancellation_token stop) {
        return execute(code, input, stop);
    }));
    return 1;
}
int ready(lua_State* L) { lua_pushboolean(L, job(L).ready()); return 1; }
int status(lua_State* L) { lua_pushstring(L, tynex::task_status_name(job(L).status())); return 1; }
int cancel(lua_State* L) { lua_pushboolean(L, job(L).cancel()); return 1; }
int wait(lua_State* L) {
    auto result = job(L).get();
    tynex::pusher<tynex::value>::push(L, result);
    return 1;
}
int shutdown(lua_State* L) {
    auto& owner = pool(L);
    owner.closed = true;
    if (owner.pool) owner.pool->shutdown();
    return 0;
}
int collect_handle(lua_State* L) {
    // Reset instead of invoking a destructor: repeated/manual __gc stays harmless.
    checked_box<handle_box>(L, 1, handle_type).value.reset();
    return 0;
}
int collect_pool(lua_State* L) {
    auto& owner = checked_box<pool_box>(L, 1, pool_type);
    owner.closed = true;
    owner.pool.reset();
    return 0;
}

template<auto F> void method(lua_State* L, const char* name) {
    lua_pushcfunction(L, (tynex::detail::boundary<F>));
    lua_setfield(L, -2, name);
}
template<auto F> void module_function(lua_State* L, const char* name, int owner) {
    lua_pushvalue(L, owner);
    lua_pushcclosure(L, tynex::detail::boundary<F>, 1);
    lua_setfield(L, -2, name);
}

int open(lua_State* L) {
    if (worker_stop) throw tynex::error("nested task pools are not supported inside a task worker");
    luaL_newmetatable(L, handle_type);
    method<collect_handle>(L, "__gc");
    lua_pushboolean(L, false); lua_setfield(L, -2, "__metatable");
    lua_newtable(L);
    method<ready>(L, "ready"); method<status>(L, "status");
    method<cancel>(L, "cancel"); method<wait>(L, "wait");
    lua_setfield(L, -2, "__index");
    lua_pop(L, 1);
    luaL_newmetatable(L, pool_type);
    method<collect_pool>(L, "__gc");
    lua_pushboolean(L, false); lua_setfield(L, -2, "__metatable");
    lua_pop(L, 1);
    new (lua_newuserdata(L, sizeof(pool_box))) pool_box;
    luaL_getmetatable(L, pool_type); lua_setmetatable(L, -2);
    const int owner = lua_gettop(L);
    lua_newtable(L);
    module_function<configure>(L, "configure", owner);
    module_function<spawn>(L, "spawn", owner);
    module_function<shutdown>(L, "shutdown", owner);
    return 1;
}
} // namespace

#if defined(_WIN32)
#define TYNEX_TASK_EXPORT __declspec(dllexport)
#else
#define TYNEX_TASK_EXPORT __attribute__((visibility("default")))
#endif
extern "C" TYNEX_TASK_EXPORT int luaopen_tynex_task(lua_State* L) {
    return tynex::detail::boundary<open>(L);
}
