// Binding/command collection example; no graphics API or GPU is required.
#include "binder/lua_binder.h"
#include <iostream>
#include <vector>

struct DrawCommand { double x; int texture; };
struct Node {
    double x = 0;
    bool visible = true;
    virtual ~Node() noexcept = default;
    virtual void append(std::vector<DrawCommand>& commands) const = 0;
    void move(double amount) { x += amount; }
};
struct Sprite final : Node {
    int texture = 0;
    void append(std::vector<DrawCommand>& commands) const override {
        if (visible) commands.push_back({x, texture});
    }
};
struct Renderer {
    std::vector<std::shared_ptr<Node>> nodes;
    void add(std::shared_ptr<Node> node) {
        if (!node) throw next2::luax::error("renderer requires a node");
        nodes.push_back(std::move(node));
    }
    std::vector<DrawCommand> collect() const {
        std::vector<DrawCommand> commands;
        for (const auto& node : nodes) node->append(commands);
        return commands;
    }
};

int main(int argc, char** argv) {
    try {
        next2::luax::state vm;
        next2::luax::binder lua(vm);
        lua.reg_class<Node>("Node")
            .reg_var("x", &Node::x).reg_var("visible", &Node::visible)
            .reg_func("move", &Node::move);
        lua.reg_class<Sprite>("Sprite").reg_base<Node>()
            .reg_con<>().reg_var("texture", &Sprite::texture);
        lua.reg_class<Renderer>("Renderer").reg_func("add", &Renderer::add);
        Renderer renderer; // host owns the renderer for the duration of script use
        lua.set_var("renderer", &renderer);
        if (argc == 2) {
            lua.dostring(R"(
                arky_engine = {Node=Node, Sprite=Sprite,
                    submit=function(node) renderer:add(node) end}
                __tynex_import = function(name)
                    assert(name == "engine")
                    return arky_engine
                end
            )");
            lua.dofile(argv[1]);
        } else lua.dostring(R"(
            local first = Sprite()
            first.texture = 7
            first:move(10)
            renderer:add(first)
            local second = Sprite()
            second.x = 20
            second.texture = 9
            renderer:add(second)
            collectgarbage('collect')
        )");
        // One host frame: native virtual dispatch, with no per-object Lua callback.
        const auto commands = renderer.collect();
        if (commands.size() != (argc == 2 ? 1u : 2u) || commands[0].x != 10 || commands[0].texture != 7
            || (argc != 2 && (commands[1].x != 20 || commands[1].texture != 9)))
            throw std::runtime_error("incorrect collected render commands");
        std::cout << "Native render queue: " << commands.size() << " commands\n";
    } catch (const std::exception& exception) {
        std::cerr << exception.what() << '\n';
        return 1;
    }
}
