#include "binder/lua_binder.h"
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <utility>

class Player {
public:
    explicit Player(std::string name, int score = 0)
        : name_(std::move(name)), score_(score) {}

    std::string name() const { return name_; }
    void rename(std::string name) { name_ = std::move(name); }
    int score() const noexcept { return score_; }
    void add_score(int amount) {
        if (amount < 0) throw std::invalid_argument("score increment must be non-negative");
        if (score_ > std::numeric_limits<int>::max() - amount)
            throw std::overflow_error("score exceeds the C++ integer range");
        score_ += amount;
    }

private:
    std::string name_;
    int score_;
};

int main() {
    try {
        tynex::state vm;
        tynex::binder lua(vm);
        lua.reg_class<Player>("Player")
            .reg_con<std::string>()
            .reg_con<std::string, int>()
            .reg_func("add_score", &Player::add_score)
            .reg_prop("name", &Player::name, &Player::rename)
            .reg_prop("score", &Player::score);

        lua.dostring(R"(
            local player = Player("Mina")
            local guest = Player("Guest", 5)
            player:add_score(42)
            player.name = "Mina Kim"
            assert(player.name == "Mina Kim" and player.score == 42)
            assert(guest.name == "Guest" and guest.score == 5)

            -- Getter-only properties cannot be assigned from Lua.
            assert(not pcall(function() player.score = 100 end))
            -- A C++ exception becomes a Lua error that pcall can handle.
            assert(not pcall(function() player:add_score(-1) end))
            assert(player.score == 42)

            function summary()
                return player.name .. ": " .. player.score
            end
        )");
        const auto summary = lua.call<std::string>("summary");
        std::cout << summary << '\n';
        return summary == "Mina Kim: 42" ? 0 : 1;
    } catch (const tynex::error& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
