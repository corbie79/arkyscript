# Tynex — Typed Lua Development Kit

**Tynex = Type + Nexus.** 타입 지원, C++ 연결, 개발 도구를 함께 제공하는 Lua 개발 키트입니다. 독립 Rust 컴파일러, `tynex::` C++20 바인딩, VS Code·Zed의 구문 강조·LSP·디버깅 도구를 포함합니다. 저장소는 [corbie79/tynex](https://github.com/corbie79/tynex)입니다. 현재 실행·바인딩 검증 대상은 LuaJIT이며, 다른 Lua 버전 지원은 호환 계층과 별도 검증이 필요합니다.

명령은 `tynex`, 프로젝트 설정은 `tynex.toml`, 환경 변수 접두사는 `TYNEX_`입니다. 스크립트는 `.tlj`, 선언 파일은 `.d.tlj` 확장자를 사용합니다.

**Rust 컴파일러**가 Tynex의 언어 처리를 담당합니다. Rust가 파싱·타입 검사·Lua 생성을 담당하고, 수정하지 않은 공식 LuaJIT이 실행합니다. Rust 컴파일러를 선택한 것이 사용자 스크립트에 Rust 소유권·대여 규칙을 도입한다는 뜻은 아닙니다.

```text
.tlj → Rust lexer / parser / AST / type checker → Lua → 공식 LuaJIT
.lua → legacy 원본 로딩 ────────────────────────────────┘
```

현재는 기본 타입·함수·테이블·제어 흐름·다중 값과 **정적 import·선언 파일·try/except/else·`.lua/.tlj` 혼합 모듈 검사·실행**을 지원합니다. VS Code·Zed 전용 구문 강조, 최소 진단 LSP, 소스 맵을 사용하는 디버깅 번들도 제공합니다. 전체 Lua 문법의 정적 검사와 class·trait·payload enum·async/await는 아직 구현 전입니다. 지원하지 않는 typed 문법을 검사 완료로 통과시키지 않습니다. 기존 `.lua`는 legacy에서 전체 LuaJIT 문법을 실행할 수 있습니다.

## 시작하기

Rust 1.94.0은 `rust-toolchain.toml`, Rust 의존성은 `Cargo.lock`, 공식 LuaJIT SHA는 `dependencies.json`에 고정합니다. macOS/Linux에서 엔진을 준비할 때 Python 3.12 이상·Git·Make·C/C++20 컴파일러가 필요합니다. 실행과 타입 검사에는 Python을 사용하지 않습니다.

```sh
make setup
./bin/tynex check examples/main.tlj
./bin/tynex run examples/main.tlj
./bin/tynex run examples/control_flow.tlj
./bin/tynex run examples/multiple_values.tlj
./bin/tynex run examples/modules/main.tlj
./bin/tynex run examples/imports/main.tlj
./bin/tynex run examples/tasks/main.tlj
./bin/tynex check --project examples/modules/tynex.toml
./bin/tynex run examples/legacy.lua
make test
```

첫 출력은 `LuaJIT score: 15`입니다. `bin/tynex`은 개발용 Cargo 실행 래퍼입니다. 배포용 실행 파일은 `cargo build --release --locked -p tynex-cli`로 생성하며 Windows에서는 `target/release/tynex.exe`, 다른 호스트에서는 `target/release/tynex`을 사용합니다.

컴파일러만 사용할 때는 LuaJIT 없이도 `cargo test --workspace --locked`, typed 파일만으로 구성된 그래프의 `check`·`gen`을 실행할 수 있습니다. 그래프에 `.lua` legacy 파일이 있으면 구문 검사에 엔진이 필요하며, 실제 실행에도 엔진이 필요합니다. 엔진은 `TYNEX_LUAJIT` → CLI 옆의 LuaJIT → 저장소의 고정 SHA 산출물 순서로 찾습니다. 독립 배포 시 엔진과 필요한 DLL·jit 모듈을 함께 배치해야 합니다.

## C++20 호스트 바인딩

`binder/`는 Tynex의 C++20 바인딩 모듈입니다. `binder/lua_binder.h`로 C++ 함수·람다·클래스·생성자·멤버·프로퍼티를 Lua에 등록하며, 공개 API는 `tynex::` 네임스페이스에 있습니다. 인자 변환, 클래스 등록, Lua 객체 참조, 동적 값을 각각의 헤더에서 제공합니다. `value`는 `std::variant`와 표준 컨테이너를 사용합니다. C++ 클래스 바인딩은 스크립트 언어의 `class` 문법과 별개의 호스트 기능입니다.

```cpp
#include "binder/lua_binder.h"

tynex::state vm;
tynex::binder lua(vm);
lua.reg_func("sum", [](int a, int b) { return a + b; });
lua.dostring("assert(sum(20, 22) == 42)");
```

`make setup` 후 C++20 컴파일러로 `make binder-test`, `make binder-sanitize`를 실행합니다. Windows x64는 `make windows` 후 `make binder-windows`로 xwin 크로스 빌드를 확인합니다. LuaJIT 업데이트 검증에도 바인더의 네이티브 테스트와 Windows 빌드를 포함합니다. [바인딩 API 안내](binder/README.md), [실행 예제](examples/cpp_binder.cpp)를 참고하세요.

`make binder-examples`는 [클래스·생성자·프로퍼티](examples/cpp_class.cpp)와 [C++20 concept 기반 공통 동작](examples/cpp_traits.cpp)을 포함한 C++ 예제 4개를 실행합니다. 여기에는 실제 OS 스레드 풀을 사용하는 [Task 예제](examples/cpp_tasks.cpp)도 포함됩니다. [예제 목록](examples/README.md)에는 실행 명령·출력과 스크립트 `class`·`trait`의 설계 예제를 정리했습니다. 스크립트용 새 키워드는 아직 미구현입니다.

## 실제 병렬 Task

`task.spawn`은 고정 개수의 OS worker에 작은 작업을 배분합니다. 실행 작업마다 독립 Lua VM을 사용하고 입력·결과를 복사하므로 메인 VM을 여러 스레드가 동시에 건드리지 않습니다. Task마다 스레드를 새로 만들지는 않지만 VM 생성·데이터 복사 비용은 있습니다. 공식 LuaJIT은 수정하지 않습니다.

[실행 예제](examples/tasks/README.md)는 정적 import·선언 파일과 함께 제출, 상태 조회, 결과, 취소, 실패를 보여줍니다. `make setup`으로 네이티브 모듈을 준비하고 `make task-test`로 검증합니다. **`wait()`는 호출 스레드를 막으며 async/await 문법은 아직 없습니다.** 긴 작업의 취소는 `context.checkpoint()`를 사용하는 협력 방식입니다. [Task API·제한·빌드](docs/tasks.md), [요청 기능별 예제 확인표](examples/README.md)를 참고하세요.

## 현재 문법

```lua
local function add(a: number, b: number): number
   local total = a + b
   return total
end

local total = add(10, 5)
print(total)
```

지원: 다중 지역 변수 선언·대입·반환, `local function`·일반 함수 선언·익명 함수·함수 값, 가변 인수, `obj:method()`와 `self`, 기본 연산, 주석·문자열, `number/string/boolean/nil/any/T?`, 지역 변수·반환형 추론. 테이블 생성·필드/인덱스 읽기와 대입, `do`, `if/elseif/else`, `while`, `repeat/until`, 숫자 `for`, `pairs/ipairs` 순회, `break`도 지원합니다.

```lua
local scores: {number} = {10, 20, 30}
local player: {name: string, score: number} = {name = "Mina", score = 0}
for index, score in ipairs(scores) do
   player.score = player.score + score
end

local score = scores[100] -- number?: 없는 원소는 nil
if score ~= nil then
   player.score = player.score + score
end
```

레코드는 `{name: string}`, 배열은 `{number}`, 사전은 `{[string]: number}`로 표기합니다. 배열·사전 읽기는 `nil` 가능 값을 반환합니다. 지역 변수의 `nil` 확인과 조기 반환 뒤에는 타입을 좁힙니다. 함수 호출 뒤에는 외부 변수 변경 가능성을 고려해 좁힌 타입을 되돌립니다. 상세 규칙과 제한은 [현재 지원 문법](docs/supported-language.md), 실행 예제는 [control_flow.tlj](examples/control_flow.tlj)에 있습니다.

여러 반환형은 `: (number, string)`, 반환값 없음은 `: ()`, 가변 인수는 `...: number`로 씁니다. 반환 목록의 마지막에는 `...number`를 둘 수 있습니다. `local a, b = f()`는 여러 값을 받지만 `(f())`는 한 값으로 줄어듭니다. 실행 가능한 [다중 반환·메서드 예제](examples/multiple_values.tlj)를 제공합니다.

초기 제한: 제네릭·사용자 정의 타입, 사용자 iterator의 정적 분석은 미구현입니다. 조건문·루프 안에서 새 비지역 함수나 메서드를 처음 정의하는 것도 현재는 진단하며, 미리 선언한 함수의 같은 타입 재정의는 가능합니다. 파일당 4096 토큰, 중첩 128 제한을 진단합니다. 표준 API 타입은 일부 기본 함수와 `math`·`string` 함수만 제공하며, 모든 LuaJIT API를 검사하지는 않습니다. 기존 Lua 모듈에는 선언 파일 또는 명시적 `any` 경계를 사용할 수 있습니다. **레거시 선언 파일·`any`·동적 경계에는 런타임 타입 보장이 없으며, 완전한 타입 안전성을 제공하는 단계는 아닙니다.**

`import worker from "worker"`는 검사된 프로젝트 모듈을 전용 로더로 가져오며 가져온 이름의 재대입을 막습니다. `.d.tlj`에는 `declare label: function(string): string`으로 공개 계약을 작성할 수 있습니다. `try … except err then … else … end`는 오류 처리와 성공 경로를 나누고 return·break·다중 반환을 보존합니다. [새 문법과 동작 규칙](docs/imports-exceptions.md), [실행 예제](examples/imports/main.tlj)를 참고하세요.

## 타입 모드와 설정

| 모드 | 동작 |
| --- | --- |
| `legacy` | `.lua` 원본 실행. `check`는 LuaJIT 구문 검사만 수행 |
| `gradual` | 타입 정보 부족 허용. 알려진 타입 불일치는 오류 |
| `strict` | 추론 허용. 알 수 없는 변수·인자 타입 등은 오류. 명시적 `any` 허용 |

설정이 없으면 `.lua`는 legacy, `.tlj`는 gradual입니다. strict도 `local count = 0`에 타입 표기를 강제하지 않습니다. 파일 선두의 `--!typecheck strict`로 모드를 지정할 수 있습니다.

```toml
# tynex.toml
type_mode = "gradual"

[[overrides]]
files = ["**/*.lua"]
mode = "legacy"

[[overrides]]
files = ["src/new/**/*.tlj"]
mode = "strict"
```

우선순위는 CLI `--mode` > 파일 선두 지시문 > 마지막 일치 override > 프로젝트 기본값 > 확장자 기본값입니다. **CLI `--mode`는 진입 파일에만 적용**하며, 가져온 모듈은 각자의 정책으로 검사합니다. 기본적으로 파일마다 가장 가까운 설정 하나를 찾고, `--project`를 지정하면 그래프 전체에서 그 설정을 사용합니다. 모듈 검색 경로는 진입 프로젝트의 설정으로 고정합니다.

```sh
./bin/tynex run --mode gradual examples/main.tlj
./bin/tynex check --mode strict examples/main.tlj
./bin/tynex check --format json examples/type_error.tlj
./bin/tynex gen examples/main.tlj
./bin/tynex run legacy/main.lua -- 'user argument' --script-flag
```

`run/debug/gen`은 진입 파일을 요구합니다. `check --project tynex.toml`은 include/exclude로 선택한 프로젝트 파일과 의존 모듈을 검사합니다. 파일 없는 `check`도 현재 디렉터리에서 설정을 찾습니다. JSON 출력은 `check`의 컴파일러·모듈·legacy 구문 진단과 검사한 파일 목록에 제공합니다. 설정·I/O·CLI 사용 오류는 현재 stderr 텍스트로 출력합니다. 오류가 있으면 0이 아닌 종료 코드를 반환합니다.

## 혼합 모듈

문자열 리터럴 `require('util.numbers')`로 typed 모듈을 가져오면 반환값의 공개 타입을 추론하고 호출자에게 전달합니다. 준비한 의존 그래프의 오류는 진입 코드 실행 전에 차단합니다. 기존 `formatter.lua`에는 선택적으로 `formatter.d.tlj`를 붙일 수 있습니다.

```text
declare label: function(string, number): string
```

선언 파일은 레거시 코드가 지킨다고 신뢰하는 계약이며 구현의 타입을 검증하지 않습니다. 모듈 검색은 `module_paths` 순서이며 같은 경로에서 이름에 해당하는 파일이 여럿이면 오류입니다. 정적 순환 참조도 오류로 진단합니다. 동적으로 선택할 프로젝트 모듈은 `dynamic_modules`에 등록하고, 호스트·C 모듈은 `external_modules`에 등록합니다. `gen`은 그래프를 검사한 뒤 **진입 파일의 Lua만 출력**하며 로컬 실행·디버깅 번들은 `bundle`/`exec-bundle`로 제공하며, 다른 컴퓨터로 옮기는 배포 패키지는 아직 지원하지 않습니다.

[혼합 모듈 안내](docs/modules.md)와 [실행 예제](examples/modules/main.tlj), [예제 설정](examples/modules/tynex.toml)에 검색 규칙·설정·런타임 경계를 정리했습니다.

## 에디터와 디버깅

`tynex lsp`와 VS Code 로컬 확장을 추가했습니다. 미저장 파일·의존 모듈·선언 파일의 진단을 같은 Rust 검사기로 처리합니다. VS Code 확장 설치와 Zed 연결은 [에디터 안내](docs/editors.md)를 따릅니다. 두 에디터 모두 `.tlj` 전용 타입 구문 강조를 제공합니다. 자동완성·정의 이동은 아직 없습니다.

VS Code·Zed의 **Tynex: mapped modules** 구성은 생성 번들과 원본 소스 맵을 사용합니다. [디버깅 안내](docs/debugging.md)를 따릅니다. macOS ARM64에서 실제 VS Code 확장 호스트와 EmmyLua DAP를 통해 행 이동·모듈 Step Into/Over·Watch·조건부 중단점·오류·재연결을 검증했습니다. Zed 전용 확장은 구문 강조·LSP·소스 맵 디버깅 설정과 WASM 빌드를 제공합니다. [검증 기록](docs/editor-validation.md)에 방식을 구분했습니다. class·match·async 변환과 async 논리 스택은 후속 작업입니다.

## 배포 플랫폼과 공식 소스

64비트 Android ARM64·iOS ARM64·Linux x64/ARM64·Windows x64를 대상으로 개발하며, Windows ARM64는 후속 대상입니다. macOS ARM64는 개발 환경입니다. CLI는 데스크톱 도구이며 모바일은 Rust 코어와 LuaJIT을 앱에 연결하는 임베딩 경로를 별도로 구현합니다. [Rust 대상 플랫폼](https://doc.rust-lang.org/rustc/platform-support.html) 지원만으로 SDK·링커·앱 임베딩이 완료되지는 않습니다. 일반 iOS 앱은 [공식 LuaJIT 안내](https://luajit.org/install.html#ios)에 따라 JIT을 끕니다.

LuaJIT은 `.deps/luajit`의 공식 Git 객체에서 지정 SHA를 새 빌드 디렉터리에 추출합니다. 공식 원본은 수정하지 않습니다. Windows xwin·LLVM 빌드와 버전 업데이트는 기존 경로를 사용합니다.

```sh
make windows
make check-luajit-update
make update-luajit
```

자세한 내용은 [Windows 엔진 빌드](docs/windows-build.md), [Rust 아키텍처](docs/architecture.md), [구현 계획](docs/implementation-plan.md), [언어 확장 설계](docs/language-design.md)에 있습니다. 지원하는 소스 확장자는 `.lua`와 `.tlj`이며, 다른 언어의 타입 문법을 자동으로 호환하지 않습니다.
