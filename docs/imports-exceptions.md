# import, 선언 파일, 예외 처리

공식 LuaJIT 소스를 수정하지 않고 Rust 컴파일러와 프로젝트 로더에서 제공한다. `.arky`의 gradual/strict에서 사용할 수 있다. 기존 `.lua`의 legacy 실행과 `require`는 유지한다. 실행 예제:

```sh
./bin/tynex run examples/imports/main.arky
```

출력은 `score: 42`다.

## 정적 import

```lua
import worker from "worker"
import formatter from "legacy.formatter"
local score = worker.double(21)
```

`from` 뒤에는 문자열 리터럴만 허용한다. 검색 경로·중복 이름·순환 의존 검사는 기존 프로젝트 그래프와 공유하며, 실행 전에 의존 모듈을 검사한다. `import worker: any from "legacy"`처럼 선언 없는 레거시와의 경계를 명시할 수도 있다.

가져온 이름에는 재대입할 수 없다. 같은 스코프에서 같은 이름을 다시 `local`로 선언하거나 함수로 재정의해도 `T012` 오류다. 안쪽 블록의 새 지역 변수로 가리는 것은 가능하다. **모듈 테이블의 필드까지 불변으로 만드는 기능은 아니다.**

실행기는 확정한 파일 목록에서 직접 모듈을 로드하며, 정규 소스 경로별 전용 캐시에 값을 보관한다. 전역/지역 `require`, `package.loaded`, `package.preload`, `package.loaders`를 바꿔도 정적 import의 대상을 바꾸지 못한다. 컴파일러 전용 `__tynex_` 식별자는 typed 코드에서 예약한다. Lua의 `debug`·환경 조작·FFI까지 제한하는 보안 샌드박스는 아니다.

- import는 한 반환값을 사용하고, 빈 반환/nil은 `true`다. `false`도 캐시하며 성공한 초기화는 반복하지 않는다. 오류가 난 초기화는 캐시하지 않아 다시 시도할 수 있다.
- 검사된 로더를 통해 먼저 require한 값은 import에서 재사용한다. 먼저 import한 값도 이후 검사된 require 로더가 재사용한다. 외부에서 조작한 공개 캐시는 import가 신뢰하지 않는다.
- `require`만 쓰는 기존 코드의 `false` 재로딩·`module()` 등록·외부 모듈 로딩은 유지한다. import로 옮길 레거시 모듈은 공개 값을 명시적으로 반환해야 한다.
- 현재 import 대상은 구현 파일이 있는 프로젝트 `.arky/.lua` 모듈이다. 호스트/C/preload 전용 모듈은 `require`와 `external_modules`를 사용한다. 정적 import에 구현이 없으면 `M110` 오류다.
- 조건문이나 함수 안의 import도 의존성은 미리 검사한다. 실제 초기화는 해당 문장이 실행될 때 일어난다.

`gen`은 진입 파일만 변환한다. import가 있는 생성 Lua에는 프로젝트 전용 로더가 필요하며 임의의 전역 require로 대체하지 않는다. `run`, `debug` 또는 `bundle`/`exec-bundle`을 사용한다. 현재 번들은 원본/생성 파일의 절대 경로를 사용하는 로컬 산출물이다.

## 선언 전용 .d.arky

원본 `legacy.lua` 옆에 `legacy.d.arky`를 둔다.

```lua
-- legacy.d.arky
declare label: function(string, number): string
declare version: string
declare read: function(string): (string?, string?)
```

각 선언은 모듈이 반환하는 테이블의 공개 필드다. 생성 Lua나 전역 변수는 만들지 않는다. 초기값·실행문·중복 이름은 오류다. 기존 `export: {label: function(string, number): string}` 형식도 유지하며, 두 형식은 한 파일에서 섞지 않는다. 테이블 이외의 모듈 반환값은 `export: TYPE`으로 기술한다.

선언 파일은 strict 문법으로 검사하지만 레거시 구현이 계약을 지키는지 검사하지는 않는다. typed 구현의 공개 타입은 계속 추론하며 sidecar를 붙이면 오류다. 명명 타입·제네릭·전역 ambient 선언은 아직 지원하지 않는다.

## try / except / else

```lua
local score = 0
try
   score = worker.double(21)
except err then
   print("failed: " .. tostring(err))
else
   print(score)
end
```

`except 이름 then` 하나가 필요하고 `else`는 선택 사항이다. Lua의 `error(...)`와 실행 오류를 받으며, `err`의 타입은 문자열에 한정되지 않는 `any`다. 테이블·nil 오류도 원래 값을 전달한다.

- try 본문이 예외와 조기 종료 없이 끝났을 때만 else를 실행한다.
- except/else에서 발생한 오류는 같은 except로 되돌아가지 않고 바깥 try로 전달된다.
- try 안의 return은 바깥 함수를 반환한다. 빈 반환, 여러 값, 중간/끝의 nil, 가변 인수를 보존한다.
- break는 원래 반복문을 빠져나온다. 중첩된 try와 반복문도 해당 대상을 유지한다.
- 본문·except·else는 각각 지역 스코프다. 함께 사용할 변수는 예제의 score처럼 try 밖에서 선언한다. except 진입 시 부분 대입을 고려해 변수의 좁힌 타입을 보수적으로 초기화한다.
- LuaJIT의 보호 호출 위에서 코루틴 yield/resume이 동작한다. 별도의 async 스케줄러를 제공하는 것은 아니다.

컴파일러는 보호 호출과 종료 종류를 나타내는 내부 값으로 변환한다. 원본 줄 수와 소스 맵을 유지한다. VS Code·Zed grammar/LSP에 새 문법을 반영했으며, 디버거에서 생성한 내부 지역 변수나 보호 호출 프레임이 보일 수 있다. `finally`, 예외 타입별 다중 except는 현재 지원하지 않는다.
