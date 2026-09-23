# 혼합 모듈과 프로젝트 검사

독립 Rust 컴파일러가 정적 의존 그래프를 준비하고, 공식 LuaJIT이 생성된 Lua와 원본 legacy 파일을 실행한다. LuaJIT 배포 소스는 패치하지 않는다. 현재 범위는 정적 import·리터럴 require와 설정에 등록한 동적 모듈이며, legacy 본문 내부 분석·순환 타입 해석·실행 중 컴파일·증분 캐시는 후속 작업이다.

## 실행 예제

저장소 루트에서 다음 명령을 실행한다.

```sh
./bin/tynex run examples/modules/main.arky
./bin/tynex check --project examples/modules/tynex.toml
./bin/tynex check --project examples/modules/tynex.toml --format json
```

실행 결과는 `Mina: 60`과 `Multiplier: 2`다. [main.arky](../examples/modules/main.arky)는 [scores.arky](../examples/modules/src/scores.arky)를 가져오고, scores는 typed 숫자 모듈과 원본 legacy formatter를 사용한다. 별도로 등록한 bonus 모듈은 동적 이름으로 가져온다. legacy 구현과 [formatter.d.arky](../examples/modules/legacy/formatter.d.arky)의 공개 계약을 분리했다.

정적 `import name from "module"`와 이름별 `declare` 계약, 예외 처리의 규칙은 [import·선언·예외 처리](imports-exceptions.md)를 따른다.

## 프로젝트 설정

```toml
type_mode = "strict"
include = ["main.arky", "src/**", "legacy/**"]
exclude = ["src/experiments/**"]
module_paths = ["src", "legacy"]
dynamic_modules = ["plugins.bonus"]
external_modules = ["host.*"]

[[overrides]]
files = ["**/*.lua"]
mode = "legacy"
```

모든 상대 경로와 glob은 설정 파일 디렉터리 기준이다. `module_paths`는 존재하는 디렉터리여야 한다. 설정을 생략하면 모듈 루트는 진입 파일 디렉터리이고, 설정이 있으면 기본 모듈 루트는 설정 디렉터리다. 작업 디렉터리가 달라도 진입 프로젝트의 검색 경로는 유지한다.

| 항목 | 의미와 기본값 |
| --- | --- |
| `include` | 프로젝트 check의 시작 파일. 기본 `**/*.arky`, `**/*.lua` |
| `exclude` | 시작 파일에서 제외. 기본 `.git/**`, `.deps/**`, `build/**`, `target/**`, `archive/**` |
| `module_paths` | require 검색 디렉터리의 순서. 기본 `['.']` |
| `dynamic_modules` | 실행 전에 추가로 검사·준비할 프로젝트 모듈 이름. 기본 없음 |
| `external_modules` | 로컬 파일이 없을 때 허용할 호스트/C/preload 모듈 이름 glob. 기본 목록에 추가 |

include/exclude를 명시하면 그 항목의 기본 목록을 대체한다. exclude는 의존 모듈의 오류를 숨기지 않는다. 제외한 파일이라도 시작 파일이 정적으로 require하면 검사한다. 프로젝트 순회는 심볼릭 링크를 따라가지 않지만 명시한 진입 파일과 require로 찾은 파일은 정규 경로로 처리한다. 빈 프로젝트 선택은 성공으로 처리하지 않는다.

`check FILE`은 그 파일의 그래프, `check --project FILE.toml`은 프로젝트 전체를 검사한다. 파일 없는 `check`는 현재 디렉터리에서 가장 가까운 설정을 찾는다. `run`, `debug`, `gen`에는 진입 파일이 필요하다. `gen`은 전체 의존 검사를 거친 뒤 **진입 파일의 Lua만 표준 출력으로 내보낸다**. `bundle`/`exec-bundle`은 typed 의존을 포함한 로컬 실행·디버깅 번들을 제공한다. 절대 경로를 사용하므로 다른 컴퓨터로 옮기는 배포 패키지 형식은 아니다.

## 모듈별 타입 모드

각 파일은 자신의 지시문과 설정으로 legacy/gradual/strict를 선택한다. CLI `--mode`는 명시한 진입 파일에만 적용한다. gradual 진입 파일이 strict 모듈을 가져와도 의존 모듈의 검사가 약해지지 않는다. 프로젝트 전체 검사에서는 `--mode` 대신 설정의 override를 사용한다.

기본적으로 파일마다 가장 가까운 `tynex.toml` 하나를 적용한다. `--project`를 명시하면 그래프 전체에서 그 설정 파일을 사용한다. 중첩 설정의 타입 정책은 적용할 수 있지만 module_paths·include/exclude·dynamic_modules·external_modules는 진입 프로젝트의 설정으로 고정한다. 실행 중 전역 require의 해석이 호출 파일마다 바뀌지 않게 하기 위해서다.

`.lua` 기본 모드는 legacy이고 `.arky` 기본은 gradual이다. legacy 본문은 원본을 LuaJIT으로 구문 검사하며 실행하지 않는다. run/debug도 준비된 legacy 파일의 구문 오류를 진입 전에 차단한다. 구문 검사에서는 `LUA_INIT`도 제거한다. 실제 실행은 원본 파일을 일반 Lua 환경에서 로드한다.

## 이름과 검색 순서

`require('util.numbers')`는 각 module_paths 디렉터리에서 다음 네 후보를 확인한다.

```text
util/numbers.arky
util/numbers/init.arky
util/numbers.lua
util/numbers/init.lua
```

처음 후보가 발견되는 루트를 사용한다. 같은 루트에서 후보가 둘 이상이면 오류다. 예를 들어 `numbers.lua`와 `numbers.arky`가 공존하거나 파일과 init 모듈이 겹치면 하나를 선택하지 않는다. 나중 검색 루트의 후보는 앞선 루트를 덮어쓰지 않는다. 이름은 점으로 구분된 문자·숫자·밑줄·하이픈을 허용하며 `/`, `\`, 빈 구간, 상위 디렉터리 경로는 허용하지 않는다.

내장 require를 가리키는 호출의 문자열 리터럴을 정적으로 분석한다. 지역 변수로 가린 require, 문자열·주석 속의 require는 import로 취급하지 않는다. `local load = require` 같은 추적 가능한 별칭과 모듈이 반환한 require 별칭도 처리한다. `require(name)`처럼 변수·계산식으로 이름을 전달하면 동적 경계다. 실행되지 않을 수 있는 함수 본문과 분기 안의 정적 require도 검사한다.

정적 순환 의존은 `M103`과 전체 경로로 진단한다. 지연 호출하면 실행 가능한 순환이라도 현재는 거절한다. 모듈 타입을 순환해서 추론하거나 선언 파일로 순환을 해소하는 기능은 아직 없다.

## 공개 타입과 legacy 선언 파일

typed 모듈의 첫 반환값을 공개 타입으로 추론한다. 예를 들어 `return {add = add}`의 함수 인자·반환형은 호출 파일에서 검사한다. require는 한 결과를 사용하므로 추가 반환값은 공개하지 않는다. 빈 반환/nil에는 require의 기본 boolean 결과를 반영한다. package.loaded를 직접 바꾸는 방식은 이 추론의 보장 범위에 포함하지 않는다.

원본 `formatter.lua` 옆에 `formatter.d.arky`를 두면 다음처럼 계약을 선언할 수 있다.

```text
export: {
   label: function(string, number): string,
   read: function(string): (string?, string?),
   log: function(...string): ()
}
```

기본 타입·optional·테이블·함수 타입을 조합한다. 선언 파일에는 `export: TYPE` 하나 또는 여러 `declare 이름: TYPE`을 허용한다. declare는 반환 테이블의 필드 계약이며 두 형식을 섞거나 실행 코드를 넣을 수 없다. 명명 타입·제네릭·class/trait 선언은 아직 없다. `.d.arky` 자체는 strict 선언 문법으로 검사한다. typed 구현에 sidecar를 붙이면 진단하며 공개 타입은 구현에서 추론한다.

선언 파일은 **legacy 구현과 비교 검증하지 않는 신뢰 계약**이다. 잘못된 계약은 런타임 오류를 일으킬 수 있다. 선언 없는 legacy와 외부 모듈은 unknown이며 strict에서는 `local api: any = require('legacy')`처럼 명시적 경계가 필요하다. gradual 모듈의 미해결 인자도 다른 파일에서 자동으로 안전한 타입이 되지 않는다.

## 동적 로딩과 Lua 런타임

동적으로 선택할 프로젝트 모듈은 dynamic_modules에 이름을 등록한다. 등록한 모듈과 정적 의존도 실행 전에 검사한다. 이름을 계산해 가져온 결과는 현재 등록 여부와 무관하게 unknown이므로 strict에서 명시적 any를 사용한다. 런타임에는 준비한 모듈 목록만 사용하고 Rust 컴파일러를 다시 실행하지 않는다.

typed 코드가 등록되지 않은 로컬 모듈을 동적으로 요청하면 본문 실행 전에 오류를 낸다. 다만 이 요청보다 앞서 발생한 진입 코드의 부수 효과는 되돌리지 않는다. legacy 내부의 일반 `.lua` require는 기존 로더에 맡기며 정적 그래프를 추론하지 않는다. legacy가 typed 모듈을 가져오려면 그 모듈을 dynamic_modules로 준비하거나 정적 typed 그래프에서 준비해야 한다. 프로젝트 전체 check를 사용하면 선택한 legacy 파일의 구문도 검사할 수 있다.

기본 외부 허용 이름은 `bit`, `ffi`, `jit`, `jit.*`, `table`, `string`, `math`, `coroutine`, `os`, `io`, `package`, `debug`, `_G`다. 그 밖의 호스트·C·preload 모듈을 정적 이름으로 가져오려면 external_modules에 등록한다. 계산한 이름으로 가져오는 외부 모듈은 정적 등록 여부를 검사할 수 없는 동적 경계다. 외부 모듈의 존재·본문·반환형은 정적으로 검증하지 않으며 원래 Lua 검색기와 `LUA_PATH`/`LUA_CPATH`를 사용한다. 같은 이름의 로컬 파일이 있으면 로컬 파일이 우선이므로 external_modules가 로컬 모듈을 강제로 우회하지는 않는다.

준비된 로더는 preload 다음, 일반 Lua 파일 검색기 앞에 들어간다. Lua의 package.loaded 캐시를 사용하므로 반복 require는 값을 재사용하고 false 반환은 다시 로드한다. typed 호출에서는 로컬 모듈과 preload가 충돌하거나 해당 이름이 검사한 로더 밖에서 미리 캐시되어 있으면 오류로 처리한다. legacy 호출의 preload 동작은 유지한다. 사용자 코드가 package.loaders/loaded나 `_G.require`를 임의로 바꾸는 경우는 동적 경계다.

typed 청크의 환경은 require만 검사한 래퍼로 제공하고 다른 전역 읽기·쓰기는 원래 전역 환경으로 전달한다. legacy 환경은 바꾸지 않는다. 따라서 getfenv/setfenv를 조작하는 typed 코드를 원래 Lua 실행과 완전히 같다고 보장하지 않는다.

## 진단과 검증 범위

check의 JSON에는 파일별 오류 코드·원본 경로·행·열·실제 모드와 검사한 파일 목록, 공개 타입 요약, 동적 import 개수가 포함된다. 설정·파일 I/O·CLI 사용 오류는 현재 텍스트 stderr로 반환한다. legacy 구문 오류는 공식 VM 메시지도 보존한다.

파일당 4096 토큰·구문 중첩 128, 프로젝트 파일 4096·의존 깊이 128·import 발견 반복 128 제한이 있다. 영속 캐시 없이 매 호출마다 다시 읽고 검사한다. 문자열과 typed 소스·실행 목록의 경로는 UTF-8을 사용하며 legacy 소스 바이트는 그대로 둔다.

자동 테스트는 모드 독립성, 공개 API 오류, 선언 파일, 검색 순서, 누락/중복/순환, 실행 전 차단, 동적 등록, Lua 캐시·preload, 모듈 이름 인자, 한글 경로, 모듈 원본 디버그 훅과 오류 위치를 확인한다. 실제 Windows/모바일 기기 실행과 VS Code/Zed UI 검증은 별도이며 Rust 대상 컴파일 검사만으로 완료를 선언하지 않는다.
