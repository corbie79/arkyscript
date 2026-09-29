#include "binder/lua_binder.h"
#include <iostream>

using namespace next2::luax;

namespace {
int checks = 0;
void check(bool value, const char* message) {
    ++checks;
    if (!value) throw std::runtime_error(message);
}
template<class F> void fails(F&& action, std::string_view expected) {
    try { action(); }
    catch (const error& exception) {
        check(std::string_view(exception.what()).find(expected) != std::string_view::npos, exception.what());
        return;
    }
    throw std::runtime_error("expected binding error");
}
struct Prefix {
    virtual ~Prefix() noexcept = default;
    int padding = 17;
};
struct Node {
    int x = 4;
    const int fixed = 8;
    Node() = default;
    explicit Node(int value) : x(value) {}
    virtual ~Node() noexcept = default;
    virtual int kind() const { return 1; }
    int get() const { return x; }
    void set(int value) { x = value; }
    int move(int amount) { return x += amount; }
};
struct Sprite : Prefix, Node {
    static inline int live = 0;
    int texture = 2;
    Sprite() { ++live; }
    ~Sprite() noexcept override { --live; }
    int kind() const override { return 2; }
};
struct Animated : Sprite {
    int frame = 0;
    int kind() const override { return 3; }
};
struct VirtualNode : virtual Node { int kind() const override { return 4; } };
struct Other { int x = 0; };
struct Late : Node {};
struct Unregistered {};
struct MissingBase : Unregistered {};
struct PlainBase { int value = 42; }; // deliberately no virtual destructor
struct PlainChild : PlainBase {
    static inline int live = 0;
    PlainChild() { ++live; }
    ~PlainChild() noexcept { --live; }
};

void inheritance() {
    state vm;
    binder lua(vm);
    auto* L = vm.get();
    auto node = lua.reg_class<Node>("Node");
    node.reg_con<>().reg_con<int>().reg_var("x", &Node::x).reg_var("fixed", &Node::fixed)
        .reg_func("kind", &Node::kind).reg_func("move", &Node::move)
        .reg_prop("position", &Node::get, &Node::set).reg_prop("readonly", &Node::get);
    auto sprite = lua.reg_class<Sprite>("Sprite");
    sprite.reg_base<Node>().reg_con<>().reg_var("texture", &Sprite::texture);
    fails([&] { sprite.reg_base<Node>(); }, "already registered");
    fails([&] { sprite.reg_var("x", &Sprite::texture); }, "inherited");
    fails([&] { sprite.reg_func("kind", &Sprite::kind); }, "inherited");
    fails([&] { sprite.reg_func("position", &Sprite::kind); }, "inherited");
    fails([&] { sprite.reg_var("kind", &Sprite::texture); }, "inherited");
    fails([&] { node.reg_func("later", &Node::get); }, "sealed");
    fails([&] { node.reg_con<float>(); }, "sealed");
    lua.reg_class<Animated>("Animated").reg_base<Sprite>().reg_con<>().reg_var("frame", &Animated::frame);
    fails([&] { sprite.reg_var("extra", &Sprite::texture); }, "sealed");
    lua.reg_class<VirtualNode>("VirtualNode").reg_base<Node>().reg_con<>();
    lua.reg_class<Other>("Other").reg_con<>();
    auto late = lua.reg_class<Late>("Late");
    late.reg_con<>();
    fails([&] { late.reg_base<Node>(); }, "before constructors");
    auto missing = lua.reg_class<MissingBase>("MissingBase");
    fails([&] { missing.reg_base<Unregistered>(); }, "has not been registered");
    lua.reg_class<Unregistered>("Unregistered");
    missing.reg_base<Unregistered>().reg_con<>(); // failed registration remains usable

    lua.reg_func("read_node", [](const Node& value) { return value.x; });
    lua.reg_func("move_node", [](Node* value) { return value ? value->move(1) : -1; });
    lua.reg_func("is_sprite", [](const Sprite& value) { return value.kind(); });
    lua.reg_func("keep_node", [](std::shared_ptr<Node> value) { return value; });
    lua.reg_func("read_shared", [](std::shared_ptr<const Node> value) { return value->x; });
    lua.dostring(R"(
        s = Sprite()
        assert(s.x == 4 and s.texture == 2 and s:kind() == 2)
        assert(s:move(2) == 6)
        s.position = 12
        assert(s.x == 12 and s.position == 12 and s.readonly == 12 and s.fixed == 8)
        assert(read_node(s) == 12 and move_node(s) == 13)
        assert(move_node(nil) == -1 and s.unknown == nil)
        a = Animated()
        a.x = 20
        assert(a:kind() == 3 and a:move(1) == 21 and read_node(a) == 21)
        assert(is_sprite(a) == 3 and a.texture == 2 and a.frame == 0)
        v = VirtualNode()
        assert(v:kind() == 4 and read_node(v) == 4 and move_node(v) == 5)
        n = Node()
        other = Other()
        kept = keep_node(s)
        assert(kept:kind() == 2 and kept.x == 13)
    )");
    fails([&] { lua.dostring("s.readonly = 9"); }, "read-only");
    fails([&] { lua.dostring("s.fixed = 9"); }, "read-only");
    fails([&] { lua.dostring("s.unknown = 9"); }, "read-only");
    fails([&] { lua.dostring("s.kind = function() return 8 end"); }, "read-only");
    fails([&] { lua.dostring("Sprite(9)"); }, "no constructor");
    fails([&] { lua.dostring("read_node(other)"); }, "wrong or expired");
    fails([&] { lua.dostring("is_sprite(n)"); }, "wrong or expired");
    fails([&] { lua.dostring("is_sprite(kept)"); }, "wrong or expired"); // no implicit downcast

    Sprite borrowed;
    check(static_cast<void*>(static_cast<Node*>(&borrowed)) != static_cast<void*>(&borrowed), "exercise nonzero base offset");
    lua.set_var("borrowed", &borrowed);
    lua.dostring("borrowed.x = 40; assert(read_node(borrowed) == 40 and move_node(borrowed) == 41)");
    check(borrowed.x == 41 && borrowed.padding == 17, "pointer adjustment writes the correct subobject");
    fails([&] { lua.dostring("keep_node(borrowed)"); }, "no shared ownership");
    lua.set_var("constant", static_cast<const Sprite*>(&borrowed));
    lua.dostring("assert(constant:kind() == 2 and read_node(constant) == 41)");
    fails([&] { lua.dostring("constant.x = 8"); }, "const C++ object");
    fails([&] { lua.dostring("constant:move(1)"); }, "const C++ object");
    fails([&] { lua.dostring("move_node(constant)"); }, "const C++ object");

    auto owner = std::make_shared<Sprite>();
    lua.set_var("shared", owner);
    auto base = lua.get_var<std::shared_ptr<Node>>("shared");
    check(base.get() == static_cast<Node*>(owner.get()), "shared upcast adjusts pointer");
    check(!base.owner_before(owner) && !owner.owner_before(base), "shared upcast preserves control block");
    lua.set_var("const_shared", std::shared_ptr<const Sprite>(owner));
    lua.dostring("assert(read_shared(const_shared) == 4)");
    fails([&] { lua.dostring("keep_node(const_shared)"); }, "const C++ object");
    lua_getglobal(L, "shared");
    auto* raw = carg<Node*>(L, -1).to();
    check(raw == base.get(), "negative stack index conversion");
    lua_pop(L, 1);
    std::weak_ptr<Sprite> weak = owner;
    owner.reset();
    lua.dostring("shared = nil; const_shared = nil; collectgarbage('collect')");
    check(!weak.expired() && base->kind() == 2, "base ownership survives Lua collection");
    base.reset();
    check(weak.expired(), "original derived allocation is released");

    // Repeated __gc, including a wrong-class collector, must remain harmless.
    lua.dostring(R"(
        local gc = debug.getmetatable(s).__gc
        debug.getmetatable(n).__gc(s)
        assert(s:kind() == 2)
        gc(s); gc(s)
    )");
    fails([&] { lua.dostring("read_node(s)"); }, "wrong or expired");
    lua.dostring("assert(kept:kind() == 2); s=nil; a=nil; kept=nil; collectgarbage('collect')");
    check(lua_gettop(L) == 0, "stack is restored after conversions and errors");
}

void script_trait_extensions() {
    state vm;
    binder lua(vm);
    lua.reg_class<Node>("Node").reg_con<>().reg_var("x", &Node::x).reg_func("kind", &Node::kind);
    auto sprite = lua.reg_class<Sprite>("Sprite");
    sprite.reg_base<Node>().reg_con<>().reg_var("texture", &Sprite::texture);
    lua.dostring(R"(
        s = Sprite()
        local extend_node = debug.getmetatable(Node).__arky_impl
        extend_sprite = debug.getmetatable(Sprite).__arky_impl
        named = {label = function(self) return "node " .. self.x end}
        extend_node(named, {}, named)
        assert(s:label() == "node 4" and s:kind() == 2)
        descriptive = {describe = function(self) return self:label() .. "!" end}
        extend_sprite(descriptive, {}, descriptive)
        assert(s:describe() == "node 4!")
        local ok = pcall(extend_sprite, {}, {valid=function() end, x=function() end}, {})
        assert(not ok and s.valid == nil)
        ok = pcall(extend_node, {}, {}, {texture=function() end})
        assert(not ok and s.texture == 2)
        ok = pcall(extend_sprite, {}, {}, {x=function() end})
        assert(not ok and s.x == 4)
    )");
    fails([&] { lua.dostring("extend_sprite(descriptive, {}, descriptive)"); }, "duplicate");
    fails([&] { sprite.reg_func("describe", &Sprite::kind); }, "script trait");
    fails([&] { lua.dostring("s.describe = function() end"); }, "read-only");
    check(lua_gettop(vm.get()) == 0, "script extensions leave a balanced stack");
}

void ownership_and_state_isolation() {
    std::shared_ptr<PlainBase> held;
    {
        state vm;
        binder lua(vm);
        lua.reg_class<PlainBase>("PlainBase").reg_var("value", &PlainBase::value);
        lua.reg_class<PlainChild>("PlainChild").reg_base<PlainBase>().reg_con<>();
        lua.dostring("child = PlainChild()");
        held = lua.get_var<std::shared_ptr<PlainBase>>("child");
    }
    check(PlainChild::live == 1 && held->value == 42, "upcast owner survives VM shutdown");
    held.reset();
    check(PlainChild::live == 0, "derived destructor runs without virtual base destructor");
    state vm;
    binder lua(vm);
    auto base = lua.reg_class<Node>("Node");
    base.reg_var("x", &Node::x); // sealed flag in the previous VM must not leak
    lua.reg_class<Sprite>("Sprite").reg_con<>(); // relation is explicitly per state
    lua.reg_func("read_node", [](const Node& value) { return value.x; });
    lua.dostring("s=Sprite()");
    fails([&] { lua.dostring("read_node(s)"); }, "wrong or expired");
}
}

int main() {
    try {
        inheritance();
        check(Sprite::live == 0, "all sprite objects released");
        ownership_and_state_isolation();
        script_trait_extensions();
        check(Sprite::live == 0, "isolated state releases objects");
        std::cout << "inheritance: " << checks << " checks passed\n";
    } catch (const std::exception& exception) {
        std::cerr << exception.what() << '\n';
        return 1;
    }
}
