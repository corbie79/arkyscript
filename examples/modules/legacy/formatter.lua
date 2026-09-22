-- Existing Lua implementation: no typed syntax or compiler-specific runtime.
return {
   label = function(name, score)
      return string.format('%s: %d', name, score)
   end,
}
