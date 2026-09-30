#include "binder/lua_binder.h"
#include <functional>
#include <iostream>
#include <new>

using namespace next2::luax;

namespace {
int checks = 0;
void check(bool condition, const char* message) {
    ++checks;
    if (!condition) throw std::runtime_error(message);
}
template<class F> void fails(F action, std::string_view expected) {
    try { action(); }
    catch (const error& e) {
        check(std::string_view(e.what()).find(expected) != std::string_view::npos, e.what());
        return;
    }
    throw std::runtime_error("expected reference binding error");
}

struct Ref {
    static inline int live = 0, adds = 0, drops = 0;
    int refs = 1;
    int value = 42;
    std::function<void(Ref*)> destroyed;
    Ref() { ++live; }
    Ref(const Ref&) = delete;
    void addRef() noexcept { ++refs; ++adds; }
    void decRef() noexcept { ++drops; if (--refs == 0) delete this; }
    int get() const { return value; }
    void set(int v) { value = v; }
protected:
    virtual ~Ref() noexcept { --live; if (destroyed) destroyed(this); }
};
struct Prefix { virtual ~Prefix() noexcept = default; int pad = 17; };
struct Node : Prefix, Ref { int node_value = 8; };
struct VirtualNode : virtual Ref { int node_value = 9; };
struct Socket {
    static inline int live = 0, adds = 0, drops = 0;
    int refs = 1;
    std::function<void(Socket*)> destroyed;
    Socket() { ++live; }
    ~Socket() noexcept { --live; if (destroyed) destroyed(this); }
    void addRef() noexcept { ++refs; ++adds; }
    void decRef() noexcept { ++drops; if (--refs == 0) delete this; }
    int get() const { return 7; }
};
struct Plain { int get() const { return 1; } };
struct PlainChild : Plain {};

arky_ref_object<Ref> native_policy() {
    return {
        .addRef = [](Ref* p) noexcept { p->addRef(); },
        .decRef = [](Ref* p) noexcept { p->decRef(); },
        .ownership = ref_ownership::native,
    };
}
arky_ref_object<Socket> lua_policy() {
    return {
        .addRef = [](Socket* p) noexcept { p->addRef(); },
        .decRef = [](Socket* p) noexcept { p->decRef(); },
        .ownership = ref_ownership::lua,
    };
}
void register_native(binder& lua) {
    lua.reg_class<Ref>("Ref").reg_func("get", &Ref::get).reg_func("set", &Ref::set);
    lua.reg_ref_object(native_policy());
    lua.reg_class<Node>("Node").reg_base<Ref>().reg_var("node_value", &Node::node_value);
    lua.reg_class<VirtualNode>("VirtualNode").reg_base<Ref>();
    lua.reg_func("echo_ref", [](Ref* p) { return p; });
}
void register_socket(binder& lua) {
    lua.reg_class<Socket>("Socket").reg_ref_object(lua_policy()).reg_func("get", &Socket::get);
    lua.reg_func("echo_socket", [](Socket* p) { return p; });
}

void native_ownership_and_identity() {
    state vm;
    binder lua(vm);
    register_native(lua);
    auto* node = new Node;
    auto* root = static_cast<Ref*>(node);
    check(static_cast<void*>(root) != static_cast<void*>(node), "exercise adjusted base pointer");
    int notifications = 0;
    node->destroyed = [&](Ref* p) { notifications += static_cast<int>(lua.invalidate_ref_object(p)); };
    const int adds = Ref::adds, drops = Ref::drops;
    lua.set_var("base", root);
    lua.set_var("node", node); // upgrades the base-first userdata
    lua.set_var("again", root);
    lua.set_var("readonly", static_cast<const Node*>(node));
    lua.reg_func("native_get", [](const Ref& p) { return p.get(); });
    lua.dostring(R"(
        assert(rawequal(base, node) and rawequal(node, again))
        assert(rawequal(node, echo_ref(node)) and node.node_value == 8)
        assert(not rawequal(node, readonly) and readonly:get() == 42)
        node:set(19); assert(base:get() == 19 and native_get(node) == 19)
        assert(echo_ref(nil) == nil)
    )");
    check(Ref::adds == adds && Ref::drops == drops && root->refs == 1,
          "native mode never retains or releases");
    fails([&] { lua.dostring("readonly:set(2)"); }, "const C++");
    fails([&] { (void)lua.get_var<std::shared_ptr<Ref>>("node"); }, "no shared ownership");
    auto handle = lua.get_var<object>("node");
    root->decRef(); // native dies while Lua variables and a host Lua handle remain
    check(notifications == 2, "destruction invalidates both const capabilities");
    fails([&] { lua.dostring("node:get()"); }, "expired");
    fails([&] { lua.dostring("readonly:get()"); }, "expired");
    fails([&] { (void)handle.call_method<int>("get"); }, "expired");
    lua.dostring("base=nil; node=nil; again=nil; readonly=nil; collectgarbage('collect')");
    handle.reset();
    lua.dostring("collectgarbage('collect')");
    check(Ref::drops == drops + 1, "expired wrappers do not release a destroyed object");
    check(lua_gettop(vm.get()) == 0, "native invalidation and errors preserve the stack");

    auto* virtual_node = new VirtualNode;
    lua.set_var("v", virtual_node);
    lua.set_var("r", static_cast<Ref*>(virtual_node));
    lua.dostring("assert(rawequal(v, r) and v:get()==42)");
    virtual_node->destroyed = [&](Ref* p) { lua.invalidate_ref_object(p); };
    virtual_node->decRef();
    fails([&] { lua.dostring("r:get()"); }, "expired");
}

void native_survives_gc_and_vm() {
    auto* node = new Node;
    const int adds = Ref::adds, drops = Ref::drops;
    {
        state vm;
        binder lua(vm);
        register_native(lua);
        lua.set_var("node", node);
        lua.dostring("node=nil; collectgarbage('collect'); collectgarbage('collect')");
        check(node->refs == 1 && node->get() == 42, "native survives Lua GC");
        lua.set_var("node", node);
        lua.dostring("assert(node:get()==42)");
    }
    check(Ref::adds == adds && Ref::drops == drops && node->refs == 1, "native survives VM shutdown");
    node->decRef();
}

void address_reuse() {
    state vm;
    binder lua(vm);
    register_native(lua);
    alignas(Node) unsigned char memory[sizeof(Node)];
    auto* first = new (memory) Node;
    first->destroyed = [&](Ref* p) { lua.invalidate_ref_object(p); };
    lua.set_var("old", first);
    first->~Node();
    auto* second = new (memory) Node;
    second->value = 99;
    second->destroyed = [&](Ref* p) { lua.invalidate_ref_object(p); };
    lua.set_var("fresh", second);
    lua.dostring("assert(not rawequal(old, fresh) and fresh:get()==99)");
    fails([&] { lua.dostring("old:get()"); }, "expired");
    // A repeated old finalizer must not remove or release the new cache entry.
    lua.dostring("debug.getmetatable(old).__gc(old)");
    lua.set_var("again", second);
    lua.dostring("assert(rawequal(again, fresh))");
    second->~Node();
    check(Ref::live == 0, "reused address objects are both destroyed");
}

void lua_ownership_and_finalization() {
    state vm;
    binder lua(vm);
    register_native(lua);
    register_socket(lua);
    auto* socket = new Socket;
    socket->destroyed = [&](Socket* p) { lua.invalidate_ref_object(p); };
    const int adds = Socket::adds, drops = Socket::drops;
    lua.set_var("socket", socket);
    lua.set_var("same", socket);
    lua.dostring("assert(rawequal(socket, same) and rawequal(socket, echo_socket(socket)))");
    check(socket->refs == 2 && Socket::adds == adds + 1, "one retain per cached mutable wrapper");
    socket->decRef(); // factory/native owner's original reference
    check(Socket::live == 1, "Lua ownership keeps socket alive after native owner drops it");
    lua.dostring("socket=nil; same=nil; collectgarbage('collect'); collectgarbage('collect')");
    check(Socket::live == 0 && Socket::drops == drops + 2, "Lua GC releases its single reference");

    socket = new Socket;
    socket->destroyed = [&](Socket* p) { lua.invalidate_ref_object(p); };
    lua.set_var("socket", socket);
    lua.set_var("constant", static_cast<const Socket*>(socket));
    check(socket->refs == 3, "const view owns a separate reference");
    socket->decRef();
    lua.dostring(R"(
        local gc = debug.getmetatable(socket).__gc
        gc(socket); gc(socket)
        assert(constant:get() == 7)
    )");
    check(Socket::live == 1, "repeated finalizer is harmless and const view stays alive");
    fails([&] { lua.dostring("socket:get()"); }, "expired");
    lua.dostring("constant=nil; socket=nil; collectgarbage('collect')");
    check(Socket::live == 0, "last const view releases socket");

    socket = new Socket;
    socket->destroyed = [&](Socket* p) { lua.invalidate_ref_object(p); };
    lua.set_var("forced", socket);
    const int before_force = Socket::drops;
    delete socket; // external destruction notification must cancel the pending release
    fails([&] { lua.dostring("forced:get()"); }, "expired");
    lua.dostring("forced=nil; collectgarbage('collect')");
    check(Socket::drops == before_force, "forced destruction never decrements freed memory");

    socket = new Socket;
    int pending_invalidated = 0;
    socket->destroyed = [&](Socket* p) { pending_invalidated += static_cast<int>(lua.invalidate_ref_object(p)); };
    lua.set_var("pending", socket);
    lua_newuserdata(vm.get(), 1);
    lua_newtable(vm.get());
    lua_pushlightuserdata(vm.get(), socket);
    lua_pushcclosure(vm.get(), +[](lua_State* L) -> int {
        delete static_cast<Socket*>(lua_touserdata(L, lua_upvalueindex(1)));
        return 0;
    }, 1);
    lua_setfield(vm.get(), -2, "__gc");
    lua_setmetatable(vm.get(), -2);
    lua_setglobal(vm.get(), "native_destroyer");
    lua.dostring("pending=nil; native_destroyer=nil; collectgarbage('collect')");
    check(pending_invalidated == 1, "native deletion during GC finds a pending finalizer after weak cache clearing");
    check(Socket::live == 0, "pending finalizer never releases a destroyed socket");
    check(lua_gettop(vm.get()) == 0, "Lua-owned destruction is reentrant and stack-balanced");
}

void multiple_vms_and_coroutines() {
    auto* socket = new Socket;
    const int adds = Socket::adds;
    {
        state vm1;
        binder a(vm1);
        register_socket(a);
        a.set_var("socket", socket);
        auto* co = lua_newthread(vm1.get());
        binder coroutine(co);
        coroutine.set_var("from_coroutine", socket);
        lua_pop(vm1.get(), 1);
        a.dostring("assert(rawequal(socket, from_coroutine))");
        check(Socket::adds == adds + 1, "coroutines share the same cache");
        {
            state vm2;
            binder b(vm2);
            register_socket(b);
            b.set_var("socket", socket);
            check(socket->refs == 3, "different VMs retain independently");
            socket->decRef();
        }
        check(Socket::live == 1 && socket->refs == 1, "one VM closing keeps the other reference");
    }
    check(Socket::live == 0, "last VM closing releases Lua-owned object");

    auto* node = new Node;
    {
        state vm1, vm2;
        binder a(vm1), b(vm2);
        register_native(a); register_native(b);
        a.set_var("n", node); b.set_var("n", node);
        int notified = 0;
        node->destroyed = [&](Ref* p) {
            notified += static_cast<int>(a.invalidate_ref_object(p));
            notified += static_cast<int>(b.invalidate_ref_object(p));
        };
        node->decRef();
        check(notified == 2, "host notifies every VM observing a native object");
        fails([&] { a.dostring("n:get()"); }, "expired");
        fails([&] { b.dostring("n:get()"); }, "expired");
    }
}

void registration_rules() {
    state vm;
    binder lua(vm);
    fails([&] { lua.reg_ref_object(native_policy()); }, "not been registered");
    auto plain = lua.reg_class<Plain>("Plain");
    Plain value;
    lua.set_var("borrowed", &value);
    fails([&] { plain.reg_ref_object({}); }, "before exposing");
    auto child = lua.reg_class<PlainChild>("PlainChild");
    PlainChild child_value;
    lua.set_var("child", &child_value);
    fails([&] { child.reg_base<Plain>(); }, "before exposing");
    auto socket = lua.reg_class<Socket>("Socket");
    fails([&] { socket.reg_ref_object({.ownership=ref_ownership::lua}); }, "requires addRef and decRef");
    socket.reg_ref_object(lua_policy());
    fails([&] { socket.reg_ref_object(lua_policy()); }, "already registered");
    fails([&] { socket.reg_con<>(); }, "pointer factory");
    Socket borrowed;
    fails([&] { lua.set_var("copy", borrowed); }, "pointer factory");
    auto shared = std::make_shared<Socket>();
    fails([&] { lua.set_var("shared", shared); }, "cannot use shared_ptr");
    check(shared.use_count() == 1, "rejected shared ownership has no side effects");
    check(lua.invalidate_ref_object<Socket>(nullptr) == 0, "null invalidation is harmless");
    check(lua.invalidate_ref_object(&borrowed) == 0, "unexposed object invalidation is harmless");
    check(lua_gettop(vm.get()) == 0, "registration errors preserve the stack");
}
} // namespace

int main() {
    try {
        native_ownership_and_identity();
        native_survives_gc_and_vm();
        address_reuse();
        lua_ownership_and_finalization();
        multiple_vms_and_coroutines();
        registration_rules();
        check(Ref::live == 0 && Socket::live == 0, "all host objects cleaned up");
        std::cout << "Native reference objects: " << checks << " checks passed\n";
    } catch (const std::exception& e) {
        std::cerr << e.what() << '\n';
        return 1;
    }
}
