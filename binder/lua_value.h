#pragma once

// Recursive Lua values using std::variant and standard containers.
#include "lua_object.h"
#include <concepts>
#include <cstdint>
#include <map>
#include <unordered_set>
#include <variant>

namespace next {
struct value {
    using key = std::variant<std::int64_t, std::string, bool>;
    using array = std::vector<value>;
    using table = std::map<key, value>;
    using storage = std::variant<std::monostate, bool, std::int64_t, std::uint64_t,
                                 double, std::string, array, table, object>;
    storage data;

    value() = default;
    value(std::nullptr_t) : data(std::monostate{}) {}
    value(bool v) : data(v) {}
    template<std::integral T> requires (!std::same_as<T, bool>)
    value(T v) : data(integer(v)) {}
    template<std::floating_point T> value(T v) : data(static_cast<double>(v)) {}
    template<class T> requires std::is_enum_v<T>
    value(T v) : value(static_cast<std::underlying_type_t<T>>(v)) {}
    value(std::string v) : data(std::move(v)) {}
    value(std::string_view v) : data(std::string(v)) {}
    value(const char* v) : data(v ? storage(std::string(v)) : storage(std::monostate{})) {}
    value(array v) : data(std::move(v)) {}
    value(table v) : data(std::move(v)) {}
    value(object v) : data(std::move(v)) {}
private:
    template<std::integral T> static storage integer(T v) {
        if constexpr (std::is_signed_v<T>) return static_cast<std::int64_t>(v);
        else return static_cast<std::uint64_t>(v);
    }
};
} // namespace next

namespace next::detail {
struct retain_lua_reference {};
struct value_budget {
    static constexpr unsigned max_depth = 64;
    static constexpr std::size_t max_nodes = 100000;
    std::size_t nodes = 0;
    void visit(unsigned depth) {
        if (depth > max_depth || ++nodes > max_nodes) throw error("Lua value conversion limit exceeded");
    }
};
struct value_reader : value_budget { std::unordered_set<const void*> tables; };

inline value::key read_value_key(lua_State* L, int index) {
    switch (lua_type(L, index)) {
    case LUA_TBOOLEAN: return read<bool>(L, index);
    case LUA_TSTRING: return read<std::string>(L, index);
    case LUA_TNUMBER: {
        const double number = lua_tonumber(L, index);
        if (!std::isfinite(number) || std::abs(number) > max_integer || std::trunc(number) != number)
            throw retain_lua_reference{};
        return static_cast<std::int64_t>(number);
    }
    default: throw retain_lua_reference{};
    }
}
inline value read_value(lua_State* L, int index, value_reader& reader, unsigned depth) {
    reader.visit(depth);
    switch (lua_type(L, index)) {
    case LUA_TNONE: throw error("invalid Lua value stack index");
    case LUA_TNIL: return nullptr;
    case LUA_TBOOLEAN: return read<bool>(L, index);
    case LUA_TNUMBER: return read<double>(L, index);
    case LUA_TSTRING: return read<std::string>(L, index);
    case LUA_TTABLE: {
        stack_guard stack(L);
        reserve(L, 8);
        const int table = absolute_index(L, index);
        // A tree snapshot cannot preserve metatables, shared subtrees or cycles.
        // Fall back to a single handle for the entire root, preserving its graph.
        if (lua_getmetatable(L, table)) throw retain_lua_reference{};
        if (!reader.tables.insert(lua_topointer(L, table)).second) throw retain_lua_reference{};
        value::table entries;
        bool dense = true;
        lua_pushnil(L);
        while (lua_next(L, table)) {
            auto key = read_value_key(L, -2);
            if (auto* number = std::get_if<std::int64_t>(&key); !number || *number < 1) dense = false;
            auto element = read_value(L, -1, reader, depth + 1);
            entries.emplace(std::move(key), std::move(element));
            lua_pop(L, 1);
        }
        if (dense && !entries.empty()) {
            const auto largest = std::get<std::int64_t>(entries.rbegin()->first);
            if (static_cast<std::uint64_t>(largest) == entries.size()) {
                value::array elements;
                elements.reserve(entries.size());
                for (auto& entry : entries) elements.push_back(std::move(entry.second));
                return elements;
            }
        }
        return entries; // Empty Lua tables have no array/map type information.
    }
    default: return next::object(L, index);
    }
}
inline void push_value(lua_State* L, const value& item, value_budget& budget, unsigned depth);
template<class T> void push_value_data(lua_State* L, const T& data, value_budget& budget, unsigned depth) {
    reserve(L, 6);
    if constexpr (std::is_same_v<T, std::monostate>) {
        lua_pushnil(L);
    } else if constexpr (std::is_same_v<T, value::array>) {
        if (data.size() > value_budget::max_nodes) throw error("Lua value conversion limit exceeded");
        lua_createtable(L, static_cast<int>(data.size()), 0);
        for (std::size_t i = 0; i < data.size(); ++i) {
            push_value(L, data[i], budget, depth + 1);
            if (lua_isnil(L, -1)) throw error("Lua array elements cannot be nil");
            lua_rawseti(L, -2, static_cast<int>(i + 1));
        }
    } else if constexpr (std::is_same_v<T, value::table>) {
        if (data.size() > value_budget::max_nodes) throw error("Lua value conversion limit exceeded");
        lua_createtable(L, 0, static_cast<int>(data.size()));
        for (const auto& [key, element] : data) {
            std::visit([L](const auto& k) { push(L, k); }, key);
            push_value(L, element, budget, depth + 1);
            if (lua_isnil(L, -1)) throw error("Lua table entries cannot be nil");
            lua_rawset(L, -3);
        }
    } else {
        push(L, data);
    }
}
inline void push_value(lua_State* L, const value& item, value_budget& budget, unsigned depth) {
    budget.visit(depth);
    std::visit([&](const auto& data) { push_value_data(L, data, budget, depth); }, item.data);
}

template<> struct value_traits<value> {
    static constexpr bool enabled = true;
    static value read(lua_State* L, int index) {
        stack_guard stack(L);
        value_reader reader;
        try {
            return read_value(L, index, reader, 0);
        } catch (const retain_lua_reference&) {
            return next::object(L, index);
        }
    }
    static void push(lua_State* L, const value& item) {
        stack_guard stack(L);
        value_budget budget;
        push_value(L, item, budget, 0);
        ++stack.top;
    }
};

// value::table is a standard map and can also cross the boundary directly.
template<> struct value_traits<value::table> {
    static constexpr bool enabled = true;
    static value::table read(lua_State* L, int index) {
        expect(L, index, LUA_TTABLE);
        auto result = value_traits<value>::read(L, index);
        if (auto* table = std::get_if<value::table>(&result.data)) return std::move(*table);
        if (auto* array = std::get_if<value::array>(&result.data)) {
            value::table table;
            for (std::size_t i = 0; i < array->size(); ++i)
                table.emplace(static_cast<std::int64_t>(i + 1), std::move((*array)[i]));
            return table;
        }
        throw error("expected a plain Lua table; use value or object for tables requiring a reference");
    }
    static void push(lua_State* L, const value::table& table) {
        stack_guard stack(L);
        value_budget budget;
        budget.visit(0);
        push_value_data(L, table, budget, 0);
        ++stack.top;
    }
};
} // namespace next::detail
