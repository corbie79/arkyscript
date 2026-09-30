#include "binder/lua_binder.h"
#include <functional>
#include <iostream>

using namespace next2::luax;

namespace {
int checks = 0;
void check(bool condition, const char* message) {
    ++checks;
    if (!condition) throw std::runtime_error(message);
}
template<class F> void fails(F&& action, std::string_view expected) {
    try { action(); }
    catch (const error& exception) {
        check(std::string_view(exception.what()).find(expected) != std::string_view::npos, exception.what());
        return;
    }
    throw std::runtime_error("expected compatibility binding error");
}

struct Item {
    static inline int live = 0;
    int value = 4;
    const int fixed = 8;
    Item() { ++live; }
    ~Item() noexcept { --live; }
    int get() const { return value; }
    void set(int n) { value = n; }
};
struct Strict { int value = 3; };
struct Ref {
    static inline int live = 0, adds = 0, drops = 0;
    int refs = 1, value = 4;
    bool visible = true;
    int children = 0, child_order = 0;
    std::function<void(Ref*)> destroyed;
    Ref() { ++live; }
    virtual ~Ref() noexcept { --live; if (destroyed) destroyed(this); }
    void addRef() noexcept { ++refs; ++adds; }
    void decRef() noexcept { ++drops; if (--refs == 0) delete this; }
    void setVisible(bool v) { visible = v; }
    bool isVisible() const { return visible; }
};
struct Prefix { virtual ~Prefix() noexcept = default; int pad = 7; };
struct Child : Prefix, Ref { int texture = 9; };

arky_ref_object<Ref> policy(ref_ownership ownership) {
    return {
        .addRef = [](Ref* p) noexcept { p->addRef(); },
        .decRef = [](Ref* p) noexcept { p->decRef(); },
        .ownership = ownership,
    };
}

void adapters_and_instance_fields() {
    state vm;
    binder lua(vm);
    auto item = lua.reg_class<Item>("Item");
    item.enable_instance_fields().reg_con<>().reg_var("value", &Item::value)
        .reg_var("fixed", &Item::fixed).reg_prop("position", &Item::get, &Item::set)
        .reg_prop("readonly", &Item::get).reg_func("get", &Item::get)
        .reg_method("move", [](Item& self, int n) { self.value += n; })
        .reg_method("read", [](const Item& self, int n) { return self.value + n; })
        .reg_method("captured", [n = std::make_unique<int>(6)](const Item& self) { return self.value + *n; })
        .reg_raw_method("pair", +[](lua_State* L) -> int {
            if (lua_gettop(L) != 2) return luaL_error(L, "pair expects self and a number");
            luaL_checktype(L, 1, LUA_TUSERDATA);
            const auto n = luaL_checknumber(L, 2);
            lua_pushvalue(L, 1);
            lua_pushnumber(L, n * 2);
            return 2;
        });
    int (*missing)(const Item&) = nullptr;
    fails([&] { item.reg_method("missing", missing); }, "null function");
    fails([&] { item.reg_method("value", [](const Item&) { return 1; }); }, "method/property");
    fails([&] { item.reg_raw_method("missing", nullptr); }, "null Lua C function");
    fails([&] { item.reg_raw_method("value", +[](lua_State*) { return 0; }); }, "method/property");
    auto strict = lua.reg_class<Strict>("Strict");
    strict.reg_con<>().reg_var("value", &Strict::value);
    lua.dostring(R"(
        global_only = 17
        a = Item(); b = Item(); strict = Strict()
        assert(a.global_only == nil and a.missing == nil)
        a:move(3); assert(a:read(2) == 9 and a:captured() == 13)
        local same, number = a:pair(3); assert(rawequal(a,same) and number == 6)
        local co = coroutine.create(function()
            local peer, n = b:pair(5); assert(rawequal(peer,b) and n == 10)
        end)
        assert(coroutine.resume(co))
        a.position = 12; assert(a.value == 12 and a:get() == 12)
        a.tag = 'first'; b.tag = 'second'
        assert(a.tag == 'first' and b.tag == 'second' and tag == nil)
        local key = {}; a[key] = 'table key'; a[4] = 'number key'
        assert(a[key] == 'table key' and a[4] == 'number key')
        a.tag = nil; assert(a.tag == nil and b.tag == 'second')
        a.callback = function(self, n) return self:read(n) end
        assert(a:callback(5) == 17)
        -- A peer can close over its owning userdata without becoming a root.
        a.self = a
        a.cycle = (function(owner) return function() return owner end end)(a)
        weak = setmetatable({a, b}, {__mode='v'})
    )");
    for (const char* name : {"fixed", "readonly", "get", "read", "move", "captured", "pair"}) {
        const std::string code = "a." + std::string(name) + " = function() end";
        fails([&] { lua.dostring(code); }, "read-only");
    }
    fails([&] { lua.dostring("strict.peer = 1"); }, "read-only");
    fails([&] { strict.enable_instance_fields(); }, "before exposing");
    fails([&] { lua.dostring("a.read(nil, 1)"); }, "expired");
    fails([&] { lua.dostring("a.read(strict, 1)"); }, "expired");
    fails([&] { lua.dostring("a:read()"); }, "argument count");
    fails([&] { lua.dostring("a:read('bad')"); }, "number");
    fails([&] { lua.dostring("a:pair()"); }, "pair expects");
    lua.dostring("a=nil; b=nil; collectgarbage('collect'); collectgarbage('collect'); assert(weak[1]==nil and weak[2]==nil)");
    check(Item::live == 0, "peer fields and self-capturing callback cycles collect");
    check(lua_gettop(vm.get()) == 0, "adapter and field errors preserve the stack");
}

void reference_identity_inheritance_and_expiration() {
    state vm;
    binder lua(vm);
    auto base = lua.reg_class<Ref>("Ref");
    base.reg_ref_object(policy(ref_ownership::native)).enable_instance_fields().reg_var("value", &Ref::value)
        .reg_method("move", [](Ref& self, int n) -> Ref* { self.value += n; return &self; })
        .reg_method("read", [](const Ref& self) { return self.value; });
    lua.reg_class<Child>("Child").reg_base<Ref>().reg_var("texture", &Child::texture);
    fails([&] { base.enable_instance_fields(); }, "sealed");
    auto* child = new Child;
    child->destroyed = [&](Ref* p) { lua.invalidate_ref_object(p); };
    check(static_cast<void*>(child) != static_cast<void*>(static_cast<Ref*>(child)), "exercise nonzero base offset");
    lua.set_var("a", static_cast<Ref*>(child));
    lua.dostring("a.tag = {name='same peer'}");
    lua.set_var("b", child); // Upgrade the existing wrapper while retaining its environment.
    lua.set_var("constant", static_cast<const Child*>(child));
    const int adds = Ref::adds, drops = Ref::drops;
    lua.dostring(R"(
        assert(rawequal(a,b) and a.tag.name == 'same peer' and b.texture == 9)
        assert(rawequal(b:move(6), b) and b:read() == 10)
        b.tag.counter = 1; assert(a.tag.counter == 1)
        assert(constant:read() == 10 and constant.tag == nil)
        constant.note = 'Lua-only field'; assert(constant.note == 'Lua-only field' and a.note == nil)
        local install = debug.getmetatable(Ref).__arky_impl
        local trait = {describe=function(self) return self.tag.name end}
        install(trait, {}, trait)
        assert(b:describe() == 'same peer')
    )");
    fails([&] { lua.dostring("b.describe = function() end"); }, "read-only");
    fails([&] { lua.dostring("b.read = function() end"); }, "read-only");
    fails([&] { lua.dostring("constant:move(1)"); }, "const C++");
    fails([&] { lua.dostring("constant.value = 1"); }, "const C++");
    check(child->value == 10 && child->pad == 7, "adapter self conversion adjusts the native base pointer");
    check(Ref::adds == adds && Ref::drops == drops, "native peer fields do not retain or release C++ objects");
    delete child;
    fails([&] { lua.dostring("return a.tag"); }, "expired");
    fails([&] { lua.dostring("a.tag = 5"); }, "expired");
    fails([&] { lua.dostring("a:move(1)"); }, "expired");
    fails([&] { lua.dostring("return constant.note"); }, "expired");
    lua.dostring("a=nil; b=nil; constant=nil; collectgarbage('collect'); collectgarbage('collect')");
    check(Ref::live == 0 && Ref::drops == drops, "expired wrappers never touch native storage");
    check(lua_gettop(vm.get()) == 0, "identity and expiration leave a balanced stack");
}

void lua_owned_cycles() {
    const int adds = Ref::adds, drops = Ref::drops;
    {
        state vm;
        binder lua(vm);
        lua.reg_class<Ref>("Ref").reg_ref_object(policy(ref_ownership::lua))
            .enable_instance_fields(instance_field_policy::allow_method_overrides)
            .reg_method("self", [](Ref& self) -> Ref* { return &self; });
        auto* ref = new Ref;
        ref->destroyed = [&](Ref* p) { lua.invalidate_ref_object(p); };
        lua.set_var("socket", ref);
        ref->decRef();
        lua.dostring(R"(
            assert(rawequal(socket:self(),socket))
            socket.peer = socket
            socket.callback = (function(owner) return function() return owner end end)(socket)
            local original = socket.self
            socket.self = (function(owner)
                return function(self) assert(rawequal(owner,self)); return original(self) end
            end)(socket)
            assert(rawequal(socket:self(),socket))
            weak = setmetatable({socket}, {__mode='v'})
            socket = nil
            collectgarbage('collect'); collectgarbage('collect')
            assert(weak[1] == nil)
        )");
        check(Ref::live == 0 && Ref::adds == adds + 1 && Ref::drops == drops + 2,
              "Lua-owned peer cycle releases its single retained native reference");
        ref = new Ref;
        // VM shutdown also releases cycles. No VM callback is needed at shutdown.
        lua.set_var("closing", ref);
        ref->decRef();
        lua.dostring(R"(
            closing.peer = closing
            closing.self = (function(owner) return function() return owner end end)(closing)
        )");
    }
    check(Ref::live == 0 && Ref::drops == drops + 4, "VM shutdown collects a Lua-owned peer cycle");
}

void native_method_overrides() {
    state vm;
    binder lua(vm);
    auto base = lua.reg_class<Ref>("Ref");
    fails([&] { base.enable_instance_fields(static_cast<instance_field_policy>(99)); }, "invalid instance field policy");
    base.reg_ref_object(policy(ref_ownership::native))
        .enable_instance_fields(instance_field_policy::allow_method_overrides)
        .reg_var("value", &Ref::value)
        .reg_prop("visible", &Ref::isVisible, &Ref::setVisible)
        .reg_prop("readonly", &Ref::isVisible)
        .reg_method("setVisible", [](Ref& self, bool visible) -> Ref* {
            self.setVisible(visible); return &self;
        })
        .reg_func("isVisible", &Ref::isVisible)
        .reg_method("addChild", [](Ref& self, Ref* child, int order) -> Ref* {
            if (!child) throw error("expected a child");
            ++self.children; self.child_order = order; return &self;
        });
    lua.reg_class<Child>("Child").reg_base<Ref>().reg_var("texture", &Child::texture);
    fails([&] { base.enable_instance_fields(instance_field_policy::protect_native_members); }, "sealed");
    auto* child = new Child;
    child->destroyed = [&](Ref* p) { lua.invalidate_ref_object(p); };
    auto* sibling = new Child;
    sibling->destroyed = [&](Ref* p) { lua.invalidate_ref_object(p); };
    lua.set_var("a", static_cast<Ref*>(child));
    lua.set_var("sibling", sibling);
    // Actual game pattern: save a native method, forward self and arguments in a
    // Lua wrapper, and restore it when the effect/scheduler is detached.
    lua.dostring(R"(
        local original = a.setVisible
        a._originalVisible = original
        a.visible_calls = 0
        a.setVisible = function(self, visible)
            self.visible_calls = self.visible_calls + 1
            original(self, visible)
            sibling:setVisible(visible)
        end
        a._addChild = a.addChild
        a.addChild = function(self, child, ...)
            self.last_child = child
            return self:_addChild(child, ...)
        end
    )");
    lua.set_var("upgraded", child);
    lua.set_var("constant", static_cast<const Child*>(child));
    lua.dostring(R"(
        assert(rawequal(a,upgraded) and upgraded.texture == 9)
        a:setVisible(false)
        assert(a.visible_calls == 1 and not a:isVisible() and not sibling:isVisible())
        assert(sibling.visible_calls == nil and sibling.setVisible == a._originalVisible)
        assert(rawequal(a:addChild(sibling, 7), a) and rawequal(a.last_child,sibling))
        a.setVisible = a._originalVisible
        assert(rawequal(a:setVisible(true),a) and a.visible_calls == 1)
        a.setVisible = function(self, visible) return self._originalVisible(self, visible) end
        a.setVisible = nil
        assert(a.setVisible == a._originalVisible and rawequal(a:setVisible(false),a))
        a.addChild = nil
        assert(a.addChild == a._addChild)
        constant.setVisible = function(self, visible) return a._originalVisible(self, visible) end
        constant.note = 'Lua-only field'
        assert(constant.note == 'Lua-only field' and a.note == nil)
        a.visible = true
        assert(a.visible and a.readonly)
        local install = debug.getmetatable(Ref).__arky_impl
        local trait = {describe=function(self) return self.value end}
        install(trait, {}, trait)
        assert(a:describe() == 4)
    )");
    check(child->children == 1 && child->child_order == 7, "override forwards native self and variadic arguments");
    fails([&] { lua.dostring("a.setVisible = false"); }, "function or nil");
    fails([&] { lua.dostring("a.setVisible = {}"); }, "function or nil");
    fails([&] { lua.dostring("a.readonly = function() end"); }, "read-only");
    fails([&] { lua.dostring("a.visible = function() end"); }, "boolean");
    fails([&] { lua.dostring("a.describe = function() end"); }, "read-only");
    fails([&] { lua.dostring("constant:setVisible(false)"); }, "const C++");
    fails([&] { lua.dostring("constant.value = 8"); }, "const C++");
    check(child->visible && child->value == 4, "Lua overrides preserve native const and property checks");
    lua.dostring("a.setVisible = function(self, visible) return self._originalVisible(self, visible) end");
    delete child;
    fails([&] { lua.dostring("return a.setVisible"); }, "expired");
    fails([&] { lua.dostring("a.setVisible = nil"); }, "expired");
    fails([&] { lua.dostring("a:setVisible(false)"); }, "expired");
    delete sibling;
    lua.dostring("a=nil; upgraded=nil; constant=nil; sibling=nil; collectgarbage('collect'); collectgarbage('collect')");
    check(Ref::live == 0, "native override fields do not take ownership of references");
    check(lua_gettop(vm.get()) == 0, "method override validation keeps the Lua stack balanced");

    // An explicit policy on a derived class can keep native methods protected.
    state protected_vm;
    binder protected_lua(protected_vm);
    protected_lua.reg_class<Ref>("Ref").reg_ref_object(policy(ref_ownership::native))
        .enable_instance_fields(instance_field_policy::allow_method_overrides)
        .reg_func("isVisible", &Ref::isVisible);
    protected_lua.reg_class<Child>("Child").reg_base<Ref>().enable_instance_fields();
    Child guarded;
    guarded.destroyed = [&](Ref* p) { protected_lua.invalidate_ref_object(p); };
    protected_lua.set_var("guarded", &guarded);
    protected_lua.dostring("guarded.note='allowed'; assert(guarded.note=='allowed' and guarded:isVisible())");
    fails([&] { protected_lua.dostring("guarded.isVisible=function() return false end"); }, "read-only");
}

void reference_exposure_seals_configuration() {
    for (bool constant : {false, true}) {
        state vm;
        binder lua(vm);
        lua.reg_class<Ref>("Ref").reg_ref_object(policy(ref_ownership::native));
        auto derived = lua.reg_class<Child>("Child").reg_base<Ref>();
        Child child;
        child.destroyed = [&](Ref* p) { lua.invalidate_ref_object(p); };
        if (constant) {
            lua.set_var("base", static_cast<const Ref*>(&child));
            lua.set_var("derived", static_cast<const Child*>(&child));
        } else {
            lua.set_var("base", static_cast<Ref*>(&child));
            lua.set_var("derived", &child);
        }
        lua.dostring("assert(rawequal(base, derived))");
        // The derived exposure upgraded the existing holder instead of creating
        // one. It must still prevent changing the instance-field policy later.
        fails([&] { derived.enable_instance_fields(); }, "before exposing");
        if (constant) lua.set_var("again", static_cast<const Child*>(&child));
        else lua.set_var("again", &child);
        lua.dostring("assert(rawequal(derived, again))");
        fails([&] { derived.enable_instance_fields(); }, "before exposing");
        fails([&] { lua.dostring("derived.peer = 'late field'"); }, "read-only");
        check(lua_gettop(vm.get()) == 0, "cached native exposures preserve the stack");
    }
    check(Ref::live == 0, "both cached exposure views invalidate before VM teardown");
}
}

int main() {
    try {
        adapters_and_instance_fields();
        reference_identity_inheritance_and_expiration();
        lua_owned_cycles();
        native_method_overrides();
        reference_exposure_seals_configuration();
        std::cout << "compatibility: " << checks << " checks passed\n";
    } catch (const std::exception& exception) {
        std::cerr << exception.what() << '\n';
        return 1;
    }
}
