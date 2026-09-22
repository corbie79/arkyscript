#include <stdio.h>
#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"

int main(void)
{
  lua_State *L = luaL_newstate();
  if (!L) return 1;
  luaL_openlibs(L);
  if (luaL_dostring(L, "assert(jit.os == 'Windows' and jit.arch == 'x64'); "
      "assert(jit.status()); local n = 0; for i=1,10000 do n=n+i end; assert(n==50005000)")) {
    fprintf(stderr, "%s\n", lua_tostring(L, -1));
    lua_close(L);
    return 1;
  }
  lua_close(L);
  puts("static embedding OK");
  return 0;
}
