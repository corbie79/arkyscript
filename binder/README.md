# Tynex Binder

Tynex 개발 키트의 헤더 전용 C++20 바인딩 모듈입니다. 공개 네임스페이스는 `next2::luax::`, 헤더 경로는 `binder/`입니다. 함수·클래스 등록, 타입 변환, Lua 객체 참조, 스크립트 실행을 제공합니다. 현재 LuaJIT의 공개 C API를 사용하며 C++20 표준 라이브러리와 LuaJIT 헤더·라이브러리만 필요합니다. 다른 Lua 버전은 호환 계층과 별도 검증이 필요합니다.

`next2`는 “next to …”, 즉 “…의 곁에”라는 뜻이고, `luax`는 그 아래의 Lua 모듈을 구분합니다. API는 `next2::luax::binder`처럼 네임스페이스를 명시해 사용합니다.

## 구성

| 파일 | 역할 |
| --- | --- |
| `lua_binder.h` | 전역 함수·클래스 등록, 스크립트 실행, 호스트 호출 API |
| `lua_arg.h` | 핵심 인자·결과 변환, `carg`·`pusher`·`result`, userdata 소유권 |
| `lua_class.h` | `cpp_class<T>`, 생성자·메서드·멤버·프로퍼티 등록과 디스패치 |
| `lua_cclosure.h` | 가변 인자 C++ 함수·람다를 Lua 콜백으로 연결 |
| `lua_object.h` / `lua_object.inl` | registry 참조를 갖는 Lua 객체, 필드 접근·함수 호출 |
| `lua_util.h` | `state`, 스택·오류 처리, VM 수명과 경로 유틸리티 |
| `lua_value.h` | `std::variant`·`std::vector`·`std::map` 기반 재귀 동적 값 |

각 헤더는 단독으로 포함할 수 있습니다. 바인더 사용에 필요한 API는 `lua_binder.h` 하나로 포함할 수 있습니다. 테스트는 `tests/binder/`, 빌드 스크립트는 `scripts/test_binder.py`, 산출물은 `build/binder/`에 있습니다.

## 사용

```cpp
#include "binder/lua_binder.h"

struct Counter {
    int value;
    explicit Counter(int initial) : value(initial) {}
    void add(int n) { value += n; }
    int get() const noexcept { return value; }
};

next2::luax::state vm;
next2::luax::binder lua(vm);
lua.reg_func("sum", [](int a, int b) { return a + b; });
lua.reg_class<Counter>("Counter")
    .reg_con<int>()
    .reg_func("add", &Counter::add)
    .reg_var("value", &Counter::value)
    .reg_prop("total", &Counter::get);
lua.dostring("counter = Counter(20); counter:add(22); assert(counter.total == 42)");
int result = lua.call<int>("sum", 20, 22);
```

완전한 실행 예제는 [cpp_binder.cpp](../examples/cpp_binder.cpp)에 있습니다. 기존 VM에는 `next2::luax::binder lua(existing_state);`로 연결합니다. 이 생성자는 라이브러리를 열거나 VM 소유권을 가져오지 않습니다. `state`는 VM을 만들고 닫으며 기본적으로 LuaJIT 표준 라이브러리를 엽니다. 라이브러리를 직접 구성하려면 `state vm(false)`를 사용합니다.

[클래스 예제](../examples/cpp_class.cpp)는 생성자·메서드·프로퍼티·예외 처리를, [공통 동작 예제](../examples/cpp_traits.cpp)는 C++20 concept과 템플릿을 통한 바인딩 재사용을 보여줍니다. `make binder-examples`로 세 예제를 실행할 수 있습니다. 후자는 C++ 기능이며 Arky 스크립트의 `trait` 키워드와 구분합니다.

## API와 규칙

- `reg_func(name, callable)`: 일반 함수, `noexcept` 함수, 캡처·mutable·이동 전용 람다, `std::function` 지원. 오버로드·generic 람다는 명시적인 인자 타입을 가진 람다로 감쌉니다. `lua_CFunction`은 원래 C API 호출 규약으로 등록합니다.
- `reg_class<T>(name)`: 상태마다 한 번 등록합니다. `reg_con<Args...>()`은 인자 개수로 생성자를 선택하며 같은 개수의 중복 등록은 오류입니다. 생성자를 자동 등록하지 않습니다.
- `reg_func`, `reg_var`, `reg_prop`은 메서드·멤버·프로퍼티를 등록합니다. const 메서드는 그대로 유지됩니다. getter만 등록한 프로퍼티와 const 멤버는 읽기 전용입니다. 알 수 없는 멤버 쓰기와 메서드/프로퍼티 이름 충돌은 오류입니다. 같은 종류의 멤버 재등록은 교체입니다.
- `set_var/get_var<T>`, `call<R>(name, args...)`로 값을 주고받습니다. `R` 생략은 반환값을 버립니다. 현재 반환값은 하나이며, 함수 인자 개수는 정확히 맞아야 합니다.
- `set_namespace("app.math")`로 등록·조회·호출 경로를 지정합니다. `"app.math.sum"` 같은 경로도 직접 사용할 수 있습니다. 경로 조회는 일반 테이블을 사용하며 `__index` 등 메타메서드를 호출하지 않습니다.
- `dostring(source)`, `dobuffer(source, chunk_name)`, `dofile(path)`는 Lua를 실행합니다. 타입 문법을 사용하는 `.arky` 소스는 Tynex 컴파일러로 먼저 Lua로 변환합니다.

지원 값은 bool·숫자·C++ enum·문자열·`std::optional<T>`·`std::vector<T>`·등록된 클래스와 해당 클래스의 `std::shared_ptr<T>`입니다. `std::string`과 `std::string_view`는 문자열 안의 NUL도 유지합니다. 숫자/문자열/bool 사이의 암묵 변환은 하지 않으며, 정수는 **±(2^53−1)**과 목적 C++ 타입 범위를 검사합니다. Lua number가 double이므로 64비트 정수 전체 범위를 보장하지 않습니다. NaN·무한대는 거부합니다. `value`와 `value::table`로 동적 값·키/값 테이블을 지원합니다. 다중 반환·`std::unique_ptr` 자동 변환·클래스 간 자동 다운캐스팅은 제공하지 않습니다. 명시적으로 등록한 부모 방향 변환은 `reg_base<Base>()`로 지원합니다.

## 네이티브 상속

부모 등록을 완료한 뒤 자식에 `.reg_base<Base>()`를 먼저 호출합니다. 부모 필드·메서드·프로퍼티를 노출하고, 자식 userdata를 부모 포인터·참조·`shared_ptr` 인자로 전달할 수 있습니다.

```cpp
lua.reg_class<Node>("Node").reg_func("move", &Node::move);
lua.reg_class<Sprite>("Sprite").reg_base<Node>().reg_con<>();
```

Lua에 노출하는 부모는 하나이며, public하고 모호하지 않은 C++ 관계만 허용합니다. 부모로 사용된 등록은 고정되고, 자식에서 상속된 이름을 덮어쓰면 오류입니다. C++ 가상 메서드는 기존 override를 호출합니다. 원래 소유권과 const 보호를 유지하며 역변환이나 빌린 포인터의 소유권 승격은 하지 않습니다.

[상속 API·trait 연결 설계](../docs/native-bindings.md)와 [렌더링 명령 수집 예제](../examples/cpp_rendering.cpp)를 참고하세요. 네이티브 명명 타입 선언과 분리된 `impl`은 [문법·호스트 연결 안내](../docs/impl.md)를 따릅니다. 스크립트 메서드는 별도 확장 표에 연결되며 기존 네이티브 멤버를 덮어쓸 수 없습니다.

## 핵심 인자 변환 모듈

`lua_arg.h`는 숫자·문자열·객체·참조·포인터와 배열의 Lua/C++ 변환을 담당합니다. Lua 스택을 직접 다루는 코드에서도 사용할 수 있습니다.

```cpp
#include "binder/lua_arg.h"

next2::luax::pusher<int>::push(L, 42);          // 값 하나를 스택에 추가
int value = next2::luax::carg<int>(L, -1).to(); // 읽기; 스택 유지
lua_pop(L, 1);

next2::luax::result<std::vector<int>>::push_im(L, {10, 20, 30});
next2::luax::carg<const std::vector<int>&> numbers(L, -1);
const auto& values = numbers.to();          // numbers가 변환한 배열의 수명을 소유
lua_pop(L, 1);
```

`carg`는 생성 시 변환하며, 변환한 문자열·배열의 const 참조는 `carg` 객체가 살아 있는 동안 유효합니다. 등록된 클래스 참조·포인터는 대상 객체를 빌리므로 호스트에서 수명을 유지해야 합니다. 변환 오류는 `next2::luax::error`로 전달합니다. 숫자·문자열·컨테이너의 변경 가능한 참조 인자는 지원하지 않고, 클래스의 변경 가능한 참조만 지원합니다.

`std::vector<T>`는 **키가 1부터 N까지 연속인 Lua 배열**로 변환합니다. 중첩 배열과 `vector<bool>`을 지원하며, 읽는 순서는 숫자 인덱스 순서입니다. 구멍이 있는 배열·문자열 키가 섞인 테이블은 오류입니다. nil 요소는 배열에서 사라지므로 vector의 nil 요소 push도 오류로 처리합니다. 빈 vector도 정확히 테이블 하나만 push하며, 변환이 실패하면 스택을 복구합니다.

`std::shared_ptr<T>`는 원래 제어 블록을 공유하므로 객체를 중복 삭제하지 않습니다. Lua 생성자가 만든 객체도 `get_var<std::shared_ptr<T>>` 등으로 공유할 수 있으며, 이 경우 Lua GC나 VM 종료 후에도 호스트 소유권이 유지됩니다. `const T` 보호를 유지하고, 빌린 raw 포인터를 shared_ptr로 바꾸는 것은 거부합니다. 빈 shared_ptr는 nil로 변환합니다.

## 동적 값

`next2::luax::value`는 Lua 값을 C++ 표준 타입으로 표현하는 재귀 구조체입니다. 공개 `data` 멤버의 `std::variant`가 현재 값과 타입을 보관합니다.

- 값 저장소: `std::variant<std::monostate, bool, int64_t, uint64_t, double, std::string, array, table, object>`
- 배열: `value::array` = `std::vector<value>`
- 테이블: `value::table` = `std::map<value::key, value>`
- 키: `std::variant<int64_t, std::string, bool>`

```cpp
using next2::luax::value;
value config = value::table{
    {"name", "demo"},
    {"enabled", true},
    {"limits", value::array{10, 20, 30}},
    {7, "integer key"}
};
lua.set_var("config", config);
auto readback = lua.get_var<value>("config");
const auto& fields = std::get<value::table>(readback.data);
const auto& name = std::get<std::string>(fields.at("name").data);
```

`std::get`, `std::get_if`, `std::holds_alternative`, `std::visit`로 처리합니다. `value`, `const value&`, `value::table`을 바인딩 함수의 인자·반환값으로 사용할 수 있습니다. 별도 직렬화 라이브러리는 필요하지 않습니다.

일반 Lua 테이블은 독립된 C++ 값으로 복사합니다. 1..N 연속 키는 배열, 문자열·정수·bool 키가 섞인 테이블이나 희소 배열은 map입니다. 빈 Lua 테이블에는 배열/map 구분 정보가 없으므로 빈 map으로 읽습니다. Lua number는 double로 읽으며, C++에서 넣는 64비트 정수는 ±(2^53−1) 범위를 검사합니다. nil은 `std::monostate`이고, 배열·테이블 항목의 nil은 Lua에서 사라지므로 push 시 오류입니다.

함수·등록된 C++ userdata·LuaJIT cdata 등은 `object` 참조로 보관합니다. 테이블 그래프에 순환·공유 하위 테이블·메타테이블·복합 키 또는 비정수 숫자 키가 있으면 **루트 전체를 object로 유지**하여 관계나 동작을 잃지 않습니다. 따라서 순수 데이터는 다른 VM에도 복사할 수 있지만, object가 포함된 값은 원래 VM의 수명·전달 규칙을 따릅니다. 참조가 필요한 테이블을 `get_var<value::table>`로 강제 복사하면 오류이며 `value` 또는 `object`를 사용합니다. 깊이 64·값 100,000개를 넘는 변환은 오류로 제한합니다.

## Lua 객체와 독립 클래스 모듈

`lua_object.h`는 Lua 값에 registry 참조를 보유합니다. 생성 후 스택에서 값을 제거하거나 전역 이름을 지워도 객체를 사용할 수 있습니다. 복사한 handle은 참조를 공유하고, 마지막 handle이 사라지거나 `reset()`하면 참조를 해제합니다.

```cpp
lua.dostring(R"(
    config = { child = { title = "demo" }, score = 20 }
    function config.sum(a, b) return a + b end
    function config:add(n) self.score = self.score + n; return self.score end
)");
auto config = lua.get_var<next2::luax::object>("config");
auto title = config["child"].get_var<std::string>("title");
config.set_var("score", 30);
int sum = config.call<int>("sum", 20, 22);       // config.sum(20, 22)
int score = config.call_method<int>("add", 12); // config:add(12), self 전달
int direct = config["sum"].invoke<int>(20, 22); // 함수 handle 직접 호출
```

`object(lua_State*, index)`로 직접 캡처하거나 바인더 인자·반환값으로 전달할 수 있습니다. `as<T>()`는 handle이 보유한 값을 변환합니다. `get_var`·`set_var`는 Lua 메타메서드를 지원하고, 오류는 보호 호출을 통해 `next2::luax::error`로 전달합니다. `call`은 암묵적인 self를 전달하지 않으며, Lua colon 메서드와 바인딩된 C++ 메서드에는 `call_method`를 사용합니다.

`validate()`는 살아 있는 VM에 속한 nil 이외의 값이면 true입니다. false 값도 유효한 객체입니다. 비어 있거나 종료된 VM의 객체 접근은 오류로 처리합니다. handle은 VM 자체를 소유하지 않으며, `state` 소멸 및 외부 코드의 `lua_close` 이후에는 자동 무효화되어 해제 시 닫힌 VM에 접근하지 않습니다. 코루틴에서 만든 handle은 그 코루틴도 참조하고, 같은 VM의 다른 코루틴으로 전달할 수 있습니다. 서로 다른 VM 사이의 전달은 거부합니다.

`lua_class.h`도 바인더 없이 사용할 수 있습니다.

```cpp
#include "binder/lua_class.h"

next2::luax::state vm;
next2::luax::cpp_class<Counter>(vm.get(), "Counter")
    .reg_con<int>()
    .reg_func("add", &Counter::add)
    .reg_var("value", &Counter::value);
```

이는 바인더의 `reg_class<Counter>("Counter")`와 같은 구현을 사용합니다. 독립 모듈 실행 테스트는 `tests/binder/object.cpp`에서 클래스·객체·코루틴·VM 종료 동작을 검증합니다.

## 소유권과 오류

Lua 생성자로 만든 객체, C++에서 값으로 전달한 객체, 클래스 값/참조 반환값은 Lua가 소유권을 가진 독립 객체가 됩니다. GC는 Lua 측 소유권을 해제하며, 공유 소유자가 모두 없어질 때 객체가 삭제됩니다. 복사할 수 없는 타입은 이동하여 반환하거나 전달할 수 있습니다. 바인딩된 람다의 캡처도 GC에서 해제됩니다. C++에는 `get_var<T>`/`call<T>`가 소유한 값을 반환하며, 포인터·참조·string_view 반환형은 허용하지 않습니다.

`set_var("object", &object)`와 C++ 함수의 포인터 반환은 **빌린 포인터**입니다. 원본 객체가 Lua에서 마지막으로 쓰일 때까지 호스트가 수명을 유지해야 합니다. `const T*`에는 비const 메서드 호출이나 멤버 쓰기를 허용하지 않습니다. null 포인터는 nil입니다. C++ 콜백의 문자열 뷰·참조 인자는 해당 호출 동안만 사용합니다.

`binder`와 `cpp_class`보다 VM이 오래 살아 있어야 하며, 같은 VM에 대한 접근은 호스트에서 직렬화해야 합니다. Lua GC로 삭제되는 C++ 객체·캡처의 소멸자는 예외를 던지면 안 됩니다.

일반 Lua 실행 오류, 타입 오류와 C++ 예외는 `next2::luax::error` 또는 Lua의 `pcall`로 처리할 수 있습니다. 호스트 API는 성공·실패 모두 기존 스택 높이를 복구합니다. 콜백의 C++ 예외는 C++ 지역 객체를 정리한 뒤 Lua 오류로 전환합니다. 메시지는 최대 1023바이트로 제한합니다. typed 콜백에서 직접 `lua_error`를 호출하지 말고 C++ 예외를 던지세요. raw C 콜백은 Lua C API의 수명·오류 처리 규칙을 직접 지켜야 합니다. 메모리 부족 등 Lua API 자체 오류의 C++ unwind 동작은 사용한 LuaJIT 빌드에 따릅니다.

## 빌드·검증

```sh
make setup                  # 공식 pin의 네이티브 LuaJIT 준비
make binder-test            # 인자·객체·클래스·표준 동적 값 + 바인더 테스트·예제
make binder-sanitize        # ASan + UBSan
make windows                # 공식 pin의 Windows x64 엔진
make binder-windows         # xwin + clang-cl C++20 크로스 빌드·PE 확인
```

네이티브 빌드는 `CXX`로 컴파일러를 지정할 수 있습니다. 소비자는 저장소 루트와 LuaJIT `src`/`include`를 include 경로에 넣고 공식 LuaJIT 라이브러리에 링크하면 됩니다. Linux 정적 링크는 `dl`, `m`도 연결합니다. Windows는 `/std:c++20 /EHsc`를 사용하며 제공한 xwin 빌드는 엔진과 동일한 `/MT` CRT를 사용합니다.

`make update-luajit`와 `make verify-luajit`은 후보 SHA로 바인더 네이티브 테스트와 Windows 빌드를 수행합니다. 실패하면 SHA 변경을 중단합니다. GitHub Actions에는 Linux/macOS 실행·sanitizer 검사와 Windows에서 크로스 빌드한 EXE 실행 검사를 연결했습니다.

이번 로컬 검증은 macOS ARM64 실행·ASan/UBSan 및 Windows x64 크로스 빌드입니다. Linux·Windows 실제 실행은 CI에서, Android/iOS 실제 실행은 해당 SDK·기기에서 추가 검증해야 합니다. 플랫폼별 엔진/JIT 설정은 공식 LuaJIT 빌드 정책을 따릅니다.
