#pragma once

// Host-defined native object lifetime. No engine or shared_ptr dependency.
namespace next2::luax {
enum class ref_ownership { native, lua };

// Register on a bound root class before exposing pointers or binding children.
// Native ownership requires an invalidate_ref_object notification on destruction.
// Lua ownership retains once per live wrapper and releases when it is collected.
// Hooks must neither throw nor call Lua; destruction may notify invalidation.
template<class T> struct arky_ref_object {
    void (*addRef)(T*) noexcept = nullptr;
    void (*decRef)(T*) noexcept = nullptr;
    ref_ownership ownership = ref_ownership::native;
};
} // namespace next2::luax
