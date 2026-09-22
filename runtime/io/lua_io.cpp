// File/DNS work runs in a bounded pool; TCP progresses on the owning Lua thread.
#if defined(_WIN32)
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#else
#include <sys/socket.h>
#include <sys/stat.h>
#include <netdb.h>
#include <poll.h>
#include <fcntl.h>
#include <unistd.h>
#endif
#include "runtime/task/task_pool.h"
#include "binder/lua_cclosure.h"
#include <array>
#include <cerrno>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>
#include <string>

namespace {
using clock_type = std::chrono::steady_clock;
using deadline_type = clock_type::time_point;
using next2::luax::error;
constexpr const char* owner_type = "tynex.io.owner";
constexpr const char* request_type = "tynex.io.request";
constexpr const char* socket_type = "tynex.io.socket";
constexpr std::size_t transfer_limit = 8 * 1024 * 1024;
constexpr std::size_t file_limit = 64 * 1024 * 1024;
constexpr std::size_t request_limit = 256;
constexpr std::size_t socket_limit = 1024;
template<class T> inline char box_tag;
#if defined(_WIN32)
using socket_handle = SOCKET;
constexpr socket_handle invalid_socket = INVALID_SOCKET;
using address_length = int;
int socket_error() noexcept { return WSAGetLastError(); }
bool would_block(int code) noexcept { return code == WSAEWOULDBLOCK || code == WSAEINPROGRESS || code == WSAEALREADY; }
bool interrupted(int code) noexcept { return code == WSAEINTR; }
void release_socket(socket_handle fd) noexcept { closesocket(fd); }
#else
using socket_handle = int;
constexpr socket_handle invalid_socket = -1;
using address_length = socklen_t;
int socket_error() noexcept { return errno; }
bool would_block(int code) noexcept { return code == EAGAIN || code == EWOULDBLOCK || code == EINPROGRESS || code == EALREADY; }
bool interrupted(int code) noexcept { return code == EINTR; }
void release_socket(socket_handle fd) noexcept { ::close(fd); }
#endif
std::string socket_message(const char* action, int code) {
    return std::string(action) + " failed (socket error " + std::to_string(code) + ")";
}

struct owner_state {
    std::unique_ptr<next2::luax::task_pool> pool;
    std::size_t outstanding = 0;
    std::size_t sockets = 0;
#if defined(_WIN32)
    owner_state() {
        WSADATA data{};
        const int code = WSAStartup(MAKEWORD(2, 2), &data);
        if (code) throw error(socket_message("WSAStartup", code));
    }
    ~owner_state() { pool.reset(); WSACleanup(); }
#endif
    next2::luax::task_pool& workers() {
        if (!pool) pool = std::make_unique<next2::luax::task_pool>(next2::luax::task_pool::default_workers(), request_limit);
        return *pool;
    }
};
struct request;
struct socket_state {
    std::shared_ptr<owner_state> owner;
    socket_handle fd = invalid_socket;
    bool eof = false;
    bool closed = false;
    bool counted = true;
    std::string closed_reason;
    std::weak_ptr<request> reader;
    std::weak_ptr<request> writer;
    explicit socket_state(std::shared_ptr<owner_state> state) : owner(std::move(state)) {
        if (owner->sockets >= socket_limit) throw error("I/O socket limit reached (1024)");
        ++owner->sockets;
    }
    ~socket_state() { release(); if (counted) --owner->sockets; }
    void release() noexcept {
        if (fd != invalid_socket) { release_socket(fd); fd = invalid_socket; }
    }
    bool close(const char* reason) {
        const bool open = fd != invalid_socket;
        abandon();
        if (closed_reason.empty()) closed_reason = reason;
        return open;
    }
    void abandon() noexcept {
        closed = true;
        release();
        if (counted) { --owner->sockets; counted = false; }
    }
};
struct address {
    sockaddr_storage storage{};
    address_length length = 0;
    int family = 0;
};
using addresses = std::vector<address>;
enum class operation { file, connect, read, write };
enum class outcome { pending, succeeded, failed, cancelled };
struct request {
    std::shared_ptr<owner_state> owner;
    operation kind;
    outcome status = outcome::pending;
    deadline_type deadline;
    std::optional<next2::luax::task<std::string>> file_work;
    std::optional<next2::luax::task<addresses>> dns_work;
    std::shared_ptr<socket_state> socket;
    addresses candidates;
    std::size_t candidate = 0;
    bool connecting = false;
    bool eof = false;
    std::string data;
    std::string failure;
    std::size_t amount = 0;
    std::size_t offset = 0;
    request(std::shared_ptr<owner_state> state, operation op, deadline_type expires)
        : owner(std::move(state)), kind(op), deadline(expires) {
        if (owner->outstanding >= request_limit) throw error("I/O request limit reached (256)");
        ++owner->outstanding;
    }
    ~request() {
        if (status == outcome::pending) {
            stop_worker();
            if (socket) socket->abandon();
            --owner->outstanding;
        }
    }
    void stop_worker() {
        if (file_work) file_work->cancel();
        if (dns_work) dns_work->cancel();
    }
    void finish(outcome result) {
        if (status != outcome::pending) return;
        status = result;
        --owner->outstanding;
        file_work.reset();
        dns_work.reset();
        candidates.clear();
    }
    void fail(std::string message, outcome result = outcome::failed) {
        if (status != outcome::pending) return;
        failure = std::move(message);
        stop_worker();
        if (socket) socket->close(failure.c_str());
        finish(result);
    }
};
template<class T> struct box {
    const void* tag = &box_tag<T>;
    std::shared_ptr<T> value;
};
template<class T> box<T>& checked_box(lua_State* L, int index, const char* type) {
    if (lua_type(L, index) != LUA_TUSERDATA || lua_objlen(L, index) != sizeof(box<T>))
        throw error(std::string("expected ") + type + " userdata");
    if (!lua_getmetatable(L, index)) throw error("I/O userdata has no metatable");
    luaL_getmetatable(L, type);
    const bool correct = lua_rawequal(L, -1, -2) != 0;
    lua_pop(L, 2);
    if (!correct) throw error("incorrect I/O userdata metatable");
    void* memory = lua_touserdata(L, index);
    const void* tag = nullptr;
    std::memcpy(&tag, memory, sizeof(tag));
    if (tag != &box_tag<T>) throw error("invalid I/O userdata tag");
    return *static_cast<box<T>*>(memory);
}
template<class T> std::shared_ptr<T> checked(lua_State* L, int index, const char* type) {
    const auto& storage = checked_box<T>(L, index, type);
    if (!storage.value) throw error("I/O userdata has been released");
    return storage.value;
}
struct publication {
    void (*push)(lua_State*, void*);
    void* context;
};
int publish(lua_State* L) {
    auto& result = *static_cast<publication*>(lua_touserdata(L, 1));
    result.push(L, result.context);
    lua_pushlightuserdata(L, &result);
    lua_insert(L, -2);
    lua_rawset(L, LUA_REGISTRYINDEX);
    return 0;
}
void publish_result(lua_State* L, void (*push)(lua_State*, void*), void* context) {
    // Lua allocation errors must not jump over the caller's owning C++ values.
    // lua_cpcall also protects allocation of the Lua C closure itself.
    if (!lua_checkstack(L, 3)) throw error("cannot reserve Lua stack for I/O result");
    publication result{push, context};
    if (lua_cpcall(L, publish, &result) != 0) throw error("cannot publish I/O result: Lua allocation failed");
    lua_pushlightuserdata(L, &result);
    lua_rawget(L, LUA_REGISTRYINDEX);
    // Clear the known existing slot directly. luaL_unref can allocate its
    // freelist slot, which would put an unprotected allocation here again.
    lua_pushlightuserdata(L, &result);
    lua_pushnil(L);
    lua_rawset(L, LUA_REGISTRYINDEX);
}
template<class T> void push_box(lua_State* L, std::shared_ptr<T> value, const char* type) {
    struct context_type { const std::shared_ptr<T>* value; const char* type; } context{&value, type};
    publish_result(L, [](lua_State* state, void* pointer) {
        const auto& input = *static_cast<context_type*>(pointer);
        // An allocation error before the metatable is attached leaves only an
        // empty shared_ptr; resources are assigned after the finalizer exists.
        auto* storage = new (lua_newuserdata(state, sizeof(box<T>))) box<T>;
        luaL_getmetatable(state, input.type);
        if (!lua_istable(state, -1)) { luaL_error(state, "I/O userdata metatable is missing"); return; }
        lua_setmetatable(state, -2);
        storage->value = *input.value;
    }, &context);
}
void push_string(lua_State* L, const std::string& value) {
    publish_result(L, [](lua_State* state, void* pointer) {
        const auto& input = *static_cast<const std::string*>(pointer);
        lua_pushlstring(state, input.data(), input.size());
    }, const_cast<std::string*>(&value));
}
void arity(lua_State* L, int minimum, int maximum) {
    const int count = lua_gettop(L);
    if (count < minimum || count > maximum) throw error("incorrect I/O argument count");
}
std::string string_arg(lua_State* L, int index, const char* label, bool allow_zero, std::size_t limit) {
    if (lua_type(L, index) != LUA_TSTRING) throw error(std::string(label) + " must be a string");
    std::size_t size = 0;
    const char* data = lua_tolstring(L, index, &size);
    if (size > limit) throw error(std::string(label) + " exceeds its byte limit");
    if (!allow_zero && (size == 0 || std::memchr(data, '\0', size)))
        throw error(std::string(label) + " must be nonempty and contain no NUL bytes");
    return {data, size};
}
std::size_t integer_arg(lua_State* L, int index, std::size_t minimum, std::size_t maximum,
                        const char* label, std::optional<std::size_t> fallback = {}) {
    if (fallback && lua_isnoneornil(L, index)) return *fallback;
    if (lua_type(L, index) != LUA_TNUMBER) throw error(std::string(label) + " must be an integer");
    const double number = lua_tonumber(L, index);
    if (!std::isfinite(number) || number < static_cast<double>(minimum) ||
        number > static_cast<double>(maximum) || std::floor(number) != number)
        throw error(std::string(label) + " is outside its integer range");
    return static_cast<std::size_t>(number);
}
deadline_type deadline_arg(lua_State* L, int index) {
    return clock_type::now() + std::chrono::milliseconds(integer_arg(L, index, 0, 2147483647, "timeout_ms", 30000));
}
void checkpoint(const next2::luax::cancellation_token& stop, deadline_type deadline) {
    if (stop.stop_requested()) throw next2::luax::task_cancelled();
    if (clock_type::now() >= deadline) throw error("I/O operation timed out");
}

#if defined(_WIN32)
struct file_descriptor {
    HANDLE value = INVALID_HANDLE_VALUE;
    ~file_descriptor() { if (value != INVALID_HANDLE_VALUE) CloseHandle(value); }
};
std::wstring wide_path(const std::string& path) {
    const int count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path.data(), static_cast<int>(path.size()), nullptr, 0);
    if (!count) throw error("file path must be valid UTF-8");
    std::wstring result(static_cast<std::size_t>(count), L'\0');
    if (!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path.data(), static_cast<int>(path.size()), result.data(), count))
        throw error("cannot convert file path to UTF-16");
    return result;
}
#else
struct file_descriptor {
    int value = -1;
    ~file_descriptor() { if (value >= 0) ::close(value); }
};
#endif
std::string load_file(const std::string& path, std::size_t maximum, deadline_type deadline,
                      const next2::luax::cancellation_token& stop) {
    checkpoint(stop, deadline);
    file_descriptor file;
    std::uint64_t size = 0;
#if defined(_WIN32)
    auto name = wide_path(path);
    // Native device/pipe namespaces are outside this regular-file API.
    if (name.starts_with(L"\\\\.\\") || name.starts_with(L"\\\\?\\") ||
        name.starts_with(L"//./") || name.starts_with(L"//?/"))
        throw error("read_file requires a regular file");
    file.value = CreateFileW(name.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                            nullptr, OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
    if (file.value == INVALID_HANDLE_VALUE) throw error("cannot open file (Windows error " + std::to_string(GetLastError()) + ")");
    BY_HANDLE_FILE_INFORMATION info{};
    if (GetFileType(file.value) != FILE_TYPE_DISK || !GetFileInformationByHandle(file.value, &info) ||
        (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY)) throw error("read_file requires a regular file");
    size = (static_cast<std::uint64_t>(info.nFileSizeHigh) << 32) | info.nFileSizeLow;
#else
    file.value = ::open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    if (file.value < 0) throw error("cannot open file: " + std::string(std::strerror(errno)));
    struct stat info{};
    if (fstat(file.value, &info) != 0 || !S_ISREG(info.st_mode)) throw error("read_file requires a regular file");
    if (info.st_size > 0) size = static_cast<std::uint64_t>(info.st_size);
#endif
    if (size > maximum) throw error("file exceeds max_bytes");
    std::string result;
    result.reserve(static_cast<std::size_t>(size));
    std::array<char, 64 * 1024> buffer{};
    for (;;) {
        checkpoint(stop, deadline);
        const auto count = std::min(buffer.size(), maximum - result.size() + 1);
#if defined(_WIN32)
        DWORD received = 0;
        if (!ReadFile(file.value, buffer.data(), static_cast<DWORD>(count), &received, nullptr))
            throw error("file read failed (Windows error " + std::to_string(GetLastError()) + ")");
#else
        const auto received = ::read(file.value, buffer.data(), count);
        if (received < 0) {
            if (errno == EINTR) continue;
            throw error("file read failed: " + std::string(std::strerror(errno)));
        }
#endif
        if (!received) break;
        if (static_cast<std::size_t>(received) > maximum - result.size()) throw error("file exceeds max_bytes");
        result.append(buffer.data(), static_cast<std::size_t>(received));
    }
    checkpoint(stop, deadline);
    return result;
}

addresses resolve(const std::string& host, int port, deadline_type deadline,
                  const next2::luax::cancellation_token& stop) {
    checkpoint(stop, deadline);
    addrinfo hints{};
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_protocol = IPPROTO_TCP;
    addrinfo* raw = nullptr;
    const int code = getaddrinfo(host.c_str(), std::to_string(port).c_str(), &hints, &raw);
    std::unique_ptr<addrinfo, decltype(&freeaddrinfo)> result(raw, freeaddrinfo);
    if (code != 0) throw error("DNS lookup failed (error " + std::to_string(code) + ")");
    checkpoint(stop, deadline);
    addresses output;
    for (auto* entry = raw; entry && output.size() < 64; entry = entry->ai_next) {
        if ((entry->ai_family != AF_INET && entry->ai_family != AF_INET6) ||
            entry->ai_addrlen > sizeof(sockaddr_storage)) continue;
        address item;
        item.family = entry->ai_family;
        item.length = static_cast<address_length>(entry->ai_addrlen);
        std::memcpy(&item.storage, entry->ai_addr, static_cast<std::size_t>(item.length));
        output.push_back(item);
    }
    if (output.empty()) throw error("DNS lookup returned no TCP addresses");
    return output;
}

void nonblocking(socket_handle fd) {
#if defined(_WIN32)
    u_long enabled = 1;
    if (ioctlsocket(fd, FIONBIO, &enabled) != 0) throw error(socket_message("nonblocking socket setup", socket_error()));
#else
    const int flags = fcntl(fd, F_GETFL, 0);
    if (flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK) < 0 || fcntl(fd, F_SETFD, FD_CLOEXEC) < 0)
        throw error(socket_message("nonblocking socket setup", socket_error()));
#if defined(SO_NOSIGPIPE)
    const int enabled = 1;
    if (setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &enabled, sizeof(enabled)) < 0)
        throw error(socket_message("SIGPIPE suppression", socket_error()));
#endif
#endif
}
bool writable(socket_handle fd) {
#if defined(_WIN32)
    fd_set writes;
    fd_set errors;
    FD_ZERO(&writes); FD_SET(fd, &writes);
    FD_ZERO(&errors); FD_SET(fd, &errors);
    timeval timeout{};
    const int count = select(0, nullptr, &writes, &errors, &timeout);
#else
    pollfd descriptor{fd, POLLOUT, 0};
    const int count = poll(&descriptor, 1, 0);
#endif
    if (count < 0) {
        const int code = socket_error();
        if (interrupted(code)) return false;
        throw error(socket_message("socket poll", code));
    }
    return count > 0;
}
void progress_connect(request& work) {
    if (work.dns_work) {
        if (!work.dns_work->ready()) return;
        work.candidates = work.dns_work->get();
        work.dns_work.reset();
    }
    auto& stream = *work.socket;
    if (work.connecting) {
        if (!writable(stream.fd)) return;
        int code = 0;
        address_length length = sizeof(code);
#if defined(_WIN32)
        const int result = getsockopt(stream.fd, SOL_SOCKET, SO_ERROR, reinterpret_cast<char*>(&code), &length);
#else
        const int result = getsockopt(stream.fd, SOL_SOCKET, SO_ERROR, &code, &length);
#endif
        if (result != 0) code = socket_error();
        if (code == 0) { work.finish(outcome::succeeded); return; }
        work.failure = socket_message("TCP connect", code);
        stream.release();
        work.connecting = false;
    }
    // Bound per-pump work even if DNS returns many immediately failing addresses.
    for (int budget = 0; budget < 4 && work.candidate < work.candidates.size(); ++budget) {
        const auto& item = work.candidates[work.candidate++];
        stream.fd = ::socket(item.family, SOCK_STREAM, IPPROTO_TCP);
        if (stream.fd == invalid_socket) { work.failure = socket_message("socket", socket_error()); continue; }
        nonblocking(stream.fd);
        const int connected = ::connect(stream.fd, reinterpret_cast<const sockaddr*>(&item.storage), item.length);
        if (connected == 0) { work.finish(outcome::succeeded); return; }
        const int code = socket_error();
        if (would_block(code) || interrupted(code)) { work.connecting = true; return; }
        work.failure = socket_message("TCP connect", code);
        stream.release();
    }
    if (work.candidate == work.candidates.size())
        work.fail(work.failure.empty() ? "TCP connect failed" : work.failure);
}
void progress_read(request& work) {
    auto& stream = *work.socket;
    if (stream.eof) { work.eof = true; work.finish(outcome::succeeded); return; }
    // One nonblocking recv per poll bounds main-loop work and returns available data.
    work.data.resize(std::min<std::size_t>(work.amount, 64 * 1024));
#if defined(_WIN32)
    const int count = recv(stream.fd, work.data.data(), static_cast<int>(work.data.size()), 0);
#else
    const auto count = recv(stream.fd, work.data.data(), work.data.size(), 0);
#endif
    if (count < 0) {
        const int code = socket_error();
        if (would_block(code) || interrupted(code)) return;
        throw error(socket_message("TCP read", code));
    }
    work.data.resize(static_cast<std::size_t>(count));
    if (count == 0) { work.eof = true; stream.eof = true; }
    work.finish(outcome::succeeded);
}
void progress_write(request& work) {
    if (work.offset == work.data.size()) { work.finish(outcome::succeeded); return; }
    // At most 64 KiB is sent per pump, keeping large writes fair to other tasks.
    const auto amount = std::min<std::size_t>(64 * 1024, work.data.size() - work.offset);
#if defined(_WIN32)
    const int count = send(work.socket->fd, work.data.data() + work.offset, static_cast<int>(amount), 0);
#else
#if defined(MSG_NOSIGNAL)
    constexpr int flags = MSG_NOSIGNAL;
#else
    constexpr int flags = 0;
#endif
    const auto count = send(work.socket->fd, work.data.data() + work.offset, amount, flags);
#endif
    if (count < 0) {
        const int code = socket_error();
        if (would_block(code) || interrupted(code)) return;
        throw error(socket_message("TCP write", code));
    }
    if (count == 0) throw error("TCP write made no progress");
    work.offset += static_cast<std::size_t>(count);
    if (work.offset == work.data.size()) work.finish(outcome::succeeded);
}
void progress(request& work) {
    if (work.status != outcome::pending) return;
    try {
        if (clock_type::now() >= work.deadline) { work.fail("I/O operation timed out"); return; }
        if (work.socket && work.socket->closed) {
            work.fail(work.socket->closed_reason.empty() ? "socket is closed" : work.socket->closed_reason);
            return;
        }
        switch (work.kind) {
        case operation::file:
            if (work.file_work->ready()) { work.data = work.file_work->get(); work.finish(outcome::succeeded); }
            break;
        case operation::connect: progress_connect(work); break;
        case operation::read: progress_read(work); break;
        case operation::write: progress_write(work); break;
        }
    } catch (const std::exception& exception) {
        work.fail(exception.what());
    }
}

std::shared_ptr<owner_state> owner(lua_State* L) { return checked<owner_state>(L, lua_upvalueindex(1), owner_type); }
int read_file(lua_State* L) {
    arity(L, 1, 3);
    auto path = string_arg(L, 1, "path", false, 32768);
    const auto maximum = integer_arg(L, 2, 0, file_limit, "max_bytes", transfer_limit);
    const auto deadline = deadline_arg(L, 3);
    auto state = owner(L);
    auto work = std::make_shared<request>(state, operation::file, deadline);
    if (clock_type::now() >= deadline) work->fail("I/O operation timed out");
    else work->file_work.emplace(state->workers().submit([path = std::move(path), maximum, deadline](next2::luax::cancellation_token stop) {
        return load_file(path, maximum, deadline, stop);
    }));
    push_box(L, std::move(work), request_type);
    return 1;
}
int connect(lua_State* L) {
    arity(L, 2, 3);
    auto host = string_arg(L, 1, "host", false, 1024);
    const auto port = integer_arg(L, 2, 1, 65535, "port");
    const auto deadline = deadline_arg(L, 3);
    auto state = owner(L);
    auto work = std::make_shared<request>(state, operation::connect, deadline);
    work->socket = std::make_shared<socket_state>(state);
    if (clock_type::now() >= deadline) work->fail("I/O operation timed out");
    else work->dns_work.emplace(state->workers().submit([host = std::move(host), port, deadline](next2::luax::cancellation_token stop) {
        return resolve(host, static_cast<int>(port), deadline, stop);
    }));
    push_box(L, std::move(work), request_type);
    return 1;
}
std::shared_ptr<socket_state> active_socket(lua_State* L) {
    auto socket = checked<socket_state>(L, 1, socket_type);
    if (socket->fd == invalid_socket) throw error("socket is closed");
    return socket;
}
void vacant(const std::weak_ptr<request>& slot, const char* direction) {
    const auto current = slot.lock();
    if (current && current->status == outcome::pending)
        throw error(std::string("socket already has a pending ") + direction);
}
int read(lua_State* L) {
    arity(L, 2, 3);
    auto socket = active_socket(L);
    const auto maximum = integer_arg(L, 2, 1, transfer_limit, "max_bytes");
    const auto deadline = deadline_arg(L, 3);
    vacant(socket->reader, "read");
    auto work = std::make_shared<request>(socket->owner, operation::read, deadline);
    work->socket = socket;
    work->amount = maximum;
    socket->reader = work;
    push_box(L, std::move(work), request_type);
    return 1;
}
int write(lua_State* L) {
    arity(L, 2, 3);
    auto socket = active_socket(L);
    auto data = string_arg(L, 2, "data", true, transfer_limit);
    const auto deadline = deadline_arg(L, 3);
    vacant(socket->writer, "write");
    auto work = std::make_shared<request>(socket->owner, operation::write, deadline);
    work->socket = socket;
    work->amount = data.size();
    work->data = std::move(data);
    socket->writer = work;
    push_box(L, std::move(work), request_type);
    return 1;
}
int close(lua_State* L) {
    arity(L, 1, 1);
    auto stream = checked<socket_state>(L, 1, socket_type);
    lua_pushboolean(L, stream->close("socket is closed"));
    return 1;
}
int ready(lua_State* L) {
    arity(L, 1, 1);
    auto work = checked<request>(L, 1, request_type);
    progress(*work);
    lua_pushboolean(L, work->status != outcome::pending);
    return 1;
}
int wait(lua_State* L) {
    arity(L, 1, 1);
    auto work = checked<request>(L, 1, request_type);
    if (work->status == outcome::pending) throw error("I/O request is pending");
    if (work->status != outcome::succeeded) throw error(work->failure);
    switch (work->kind) {
    case operation::connect: push_box(L, work->socket, socket_type); break;
    case operation::write: lua_pushnumber(L, static_cast<lua_Number>(work->amount)); break;
    case operation::read:
        if (work->eof) { lua_pushnil(L); break; }
        [[fallthrough]];
    case operation::file: push_string(L, work->data); break;
    }
    return 1;
}
int cancel(lua_State* L) {
    arity(L, 1, 1);
    auto work = checked<request>(L, 1, request_type);
    const bool pending = work->status == outcome::pending;
    if (pending) work->fail("I/O operation cancelled", outcome::cancelled);
    lua_pushboolean(L, pending);
    return 1;
}
int sleep(lua_State* L) {
    arity(L, 1, 1);
    std::this_thread::sleep_for(std::chrono::milliseconds(integer_arg(L, 1, 0, 1000, "sleep milliseconds")));
    return 0;
}
template<class T> int collect(lua_State* L) {
    const char* type = nullptr;
    if constexpr (std::is_same_v<T, owner_state>) type = owner_type;
    else if constexpr (std::is_same_v<T, request>) type = request_type;
    else type = socket_type;
    checked_box<T>(L, 1, type).value.reset();
    return 0;
}
template<auto F> void method(lua_State* L, const char* name) {
    lua_pushcfunction(L, (next2::luax::detail::boundary<F>));
    lua_setfield(L, -2, name);
}
template<auto F> void module_function(lua_State* L, const char* name, int state) {
    lua_pushvalue(L, state);
    lua_pushcclosure(L, next2::luax::detail::boundary<F>, 1);
    lua_setfield(L, -2, name);
}
template<class T> void metatable(lua_State* L, const char* name) {
    luaL_newmetatable(L, name);
    method<collect<T>>(L, "__gc");
    lua_pushboolean(L, false); lua_setfield(L, -2, "__metatable");
    lua_pop(L, 1);
}
int open_module(lua_State* L) {
    metatable<owner_state>(L, owner_type);
    metatable<request>(L, request_type);
    metatable<socket_state>(L, socket_type);
    push_box(L, std::make_shared<owner_state>(), owner_type);
    const int state = lua_gettop(L);
    lua_newtable(L);
    module_function<read_file>(L, "read_file", state);
    module_function<connect>(L, "connect", state);
    method<read>(L, "read");
    method<write>(L, "write");
    method<close>(L, "close");
    method<ready>(L, "_async_ready");
    method<wait>(L, "_async_wait");
    method<cancel>(L, "_async_cancel");
    method<sleep>(L, "_async_sleep");
    return 1;
}
} // namespace

#if defined(_WIN32)
#define TYNEX_IO_EXPORT __declspec(dllexport)
#else
#define TYNEX_IO_EXPORT __attribute__((visibility("default")))
#endif
extern "C" TYNEX_IO_EXPORT int luaopen_tynex_io(lua_State* L) {
    return next2::luax::detail::boundary<open_module>(L);
}
