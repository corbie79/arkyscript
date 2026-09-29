# 현재 지원하는 typed Lua 문법

처음부터 읽는 사용 안내는 [Arky 문법 매뉴얼](syntax-manual.md)을 참고하세요. 이 문서는 검사기의 세부 규칙과 구현 범위를 정리합니다.

이 문서는 현재 구현 범위다. enum·match 등의 [설계 제안](language-design.md)과 구분한다. `.arky`는 독립 Rust 코어로 검사하고 타입 표기를 제거하고 확장 문법을 Lua로 변환해 공식 LuaJIT에서 실행한다. 원본 `.lua`의 legacy 실행에는 아래 정적 문법 제한을 적용하지 않는다.

## 문장과 함수

- 다중 지역 변수 선언과 대입, 테이블 필드·인덱스 대입.
- `local function`, 일반/테이블 함수 선언, 익명 함수, 함수 값, 재귀 함수. strict에서 재귀의 반환형 추론이 해결되지 않으면 반환형을 명시한다.
- 다중 반환값·가변 인수, 일반 함수·메서드 호출, `print "hello"`·`print {1, 2}` 형태의 인자 축약.
- `local async function`, async 함수 값·메서드, async 본문의 `await`, 결과 계약 `Task<number, string>`·`Task<>`. [async/await 규칙](async-await.md)을 따른다.
- nil이면 생략하는 `obj?.field`, `obj?[key]`, `fn?(args)`, `obj?:method(args)`. [평가·타입·체인 규칙](optional-chaining.md)을 따른다.
- `do/end`, `if/elseif/else`, `while/do`, `repeat/until`, 숫자 `for`, `pairs/ipairs` 순회, 루프 안의 `break`.
- 기본 산술·비교·문자열 연결·길이·논리 연산, Lua 문자열·긴 문자열·주석.

함수의 인자와 반환값, 지역 변수와 for 변수에 타입 표기를 붙일 수 있다. strict에서도 지역 변수·반환형은 추론한다. 인자 타입은 현재 호출 위치에서 역추론하지 않으므로 표기가 필요하다. 끝의 optional 인자는 생략할 수 있다. 함수 타입은 `function(number, string?): number`, 가변 인수는 `function(...string): ()`처럼 적는다. 함수 구현의 인자를 문맥에서 역추론하는 기능은 아직 없다.

반환형이 `number`인 함수가 일부 경로에서 아무 값도 반환하지 않으면 오류다. 숫자 for·while은 본문을 한 번도 실행하지 않을 수 있다고 분석한다. `while true`의 break 여부와 최소 한 번 실행하는 repeat도 구분한다. 모든 실행 경로를 정밀하게 증명하는 분석은 아니므로 상수 조건이나 복잡한 조건에서 보수적인 오류가 날 수 있다.

generic for는 단일 iterator 표현식을 파싱한다. `pairs/ipairs`는 테이블에서 반복 변수 타입을 추론하고, 이름을 다른 함수로 가리면 내장 함수로 취급하지 않는다. 사용자 iterator에는 gradual 또는 명시적 `any` 경계가 필요하다. `iterator, state, control`을 직접 나열하는 형식은 아직 지원하지 않는다.

## 다중 값과 가변 인수

여러 결과는 별도 튜플 객체를 만들지 않는 Lua의 값 목록이다. 여러 변수는 각각 타입을 표기할 수 있고, 함수 반환형은 다음처럼 쓴다.

```lua
local function pair(n: number): (number, string)
   return n, tostring(n)
end
local number_value: number, text: string = pair(3)
local first: number, missing: nil = (pair(3))
```

대입·함수 인자·return의 마지막 표현식이 호출 또는 `...`이면 여러 값으로 확장한다. 앞선 표현식과 괄호로 감싼 호출은 한 값으로 조정한다. 대입 대상보다 값이 적으면 nil로 채우고 많으면 버리지만, 버려지는 우변도 검사하고 실행한다. 테이블에서는 마지막 필드가 목록 형태일 때만 확장한다. `{f()}`와 `{(f())}`, `{f(), 1}`, `{x = f()}`를 구분한다. 이 규칙은 [Lua 공식 문서](https://www.lua.org/manual/5.1/manual.html#2.5)에 따른다.

`: ()`는 반환값이 없다는 계약이고 `: nil`은 nil을 포함하는 한 위치의 계약이다. 빈 반환과 명시적인 nil 반환의 실제 개수는 추론에 보존한다. nullable 위치는 빠질 수 있지만 선언보다 많은 반환값은 오류다. 타입을 붙이지 않은 함수도 각 결과 위치와 분기별 부재를 추론한다. 알려진 함수 시그니처의 인자 개수를 벗어나는 호출도 진단한다.

```lua
local function values(first: number, ...: string): (number, ...string)
   return first, ...
end
local function count(...: any): number
   return select('#', ...)
end
print(values(1, 'a', 'b'), count(1, nil, 3))
```

가변 인수는 매개변수 목록의 마지막 `...: T`, 가변 반환 목록은 `: (...T)` 또는 `: (number, ...T)`다. strict에서 가변 인수의 T가 필요하고 gradual에서는 표기를 생략할 수 있다. 가변 인수를 한 값으로 읽으면 값이 없을 수 있으므로 `T?`다. 내부의 비가변 함수는 바깥 함수의 `...`를 직접 참조할 수 없다. 최상위 청크의 인수는 호스트 경계이므로 any로 취급한다.

현재는 호출별로 가변 인수의 실제 개수를 특수화하지 않는다. `local function pass(...: number) return ... end`의 결과는 `pass(1)`에서도 `number?`다. `select('#', ...)`는 number이며, 숫자 인덱스의 select와 unpack은 개수·위치를 보수적으로 추론한다. unpack은 배열 구멍도 고려해 nil 가능 원소를 반환한다. `'#'`를 변수에 담은 select는 아직 문자열 상수로 좁히지 못한다.

명시적인 반환형이 any API의 가변 결과를 받으면 그 반환형을 호출자에게 노출한다. 이 경우에는 선언한 타입·개수를 런타임에 보장하지 않는 동적 경계가 된다.

## 일반 함수와 메서드

`function name(...)`, `function module.fn(...)`, `function module:method(...)`를 지원한다. 단순 이름은 같은 이름의 지역 변수가 있으면 그 위치에 대입하고, 없으면 전역 함수로 선언한다. `obj:method(args)`는 수신자를 한 번 평가하며 첫 인자로 전달한다. 생성된 Lua에서도 `:`를 그대로 유지한다.

```lua
local counter = {value = 0}
function counter:add(amount: number): number
   self.value = self.value + amount
   return self.value
end
local result = counter:add(3)
print(result)
```

`self`는 선언 시점의 테이블에서 추론한다. 기존 필드 타입과 인자를 검사하고, `counter.add(counter, 3)` 형식의 점 호출에서도 self 계약을 검사한다. 타입이 지정된 기존 함수 필드도 콜론으로 호출할 수 있다. 문자열의 `:len()`, `:upper()`, `:lower()`, `:sub(...)`도 지원한다.

현재 새 함수 멤버는 정적인 이름/점 경로에 추가하고, 기존 필드의 계약을 바꾸는 재정의는 거절한다. 조건문·루프 안에서 새 비지역 함수/멤버를 처음 선언하는 것은 `T011`로 진단한다. 먼저 선언한 함수의 동일 타입 재정의는 가능하다. 새 멤버를 선언 이전에 만든 모든 별칭에 전파하지 않으므로 멤버 정의를 마친 뒤 별칭을 만든다. 함수 안에서 생성한 전역 멤버가 호출 뒤 존재한다고 추론하지도 않는다.

`self`의 스냅샷에는 현재 정의 중인 메서드와 나중에 추가하는 메서드가 없다. 따라서 strict의 `self:동일메서드()` 재귀·전방 메서드 참조는 후속 작업이며, 정적 경로의 `module.fn()` 재귀는 반환형을 명시해 사용할 수 있다. 이 제한은 일반 Lua 테이블 함수에 적용한다. 별도로 구현한 class·trait 메서드는 전체 계약을 먼저 등록하므로 반환형을 명시한 self 재귀·전방 호출을 지원한다. [class·trait 안내](classes-traits.md)를 참고한다.

## 타입과 테이블

| 표기 | 의미 |
| --- | --- |
| `number`, `string`, `boolean`, `nil` | 기본 값 |
| `any` | 명시적인 동적 경계 |
| `Player`, `Named`, `models.Player`, `Self` | 선언한 class·trait, import한 명명 타입, 현재 구현 타입 |
| `T?` | T 또는 nil |
| `(T)`·`(function(number): number)?` | 타입 묶기·nil일 수 있는 함수 |
| `{name: string, score: number?}` | 고정 필드가 있는 레코드 |
| `{number}` | 숫자 키로 접근하는 배열 |
| `{[string]: number}` | 문자열 키를 사용하는 사전 |
| `{}` | 알려진 필드가 없는 닫힌 레코드 |

중첩 테이블 타입을 사용할 수 있다. 하나의 타입 표기 안에 고정 필드와 인덱스 계약을 함께 선언하거나, 타입 별칭·제네릭·`A | B`를 적는 문법은 아직 없다. 서로 다른 반환형 등은 검사기 내부에서 union으로 합치며, 알려진 잘못된 타입이 unknown 경로에 가려지지 않도록 검사한다.

배열·사전은 실제 Lua 테이블이다. 연속된 원소나 길이를 정적으로 보장하지 않는다. 인덱스로 읽은 값은 없는 키일 수 있으므로 optional이다. `ipairs`가 실제로 제공하는 원소는 nil이 아니다. `pairs`의 키·값은 필드와 인덱스 타입을 합쳐 추론한다.

```lua
local counts: {[string]: number} = {}
counts.apples = 3
local count = counts["apples"]
if count ~= nil then
   print(count + 1)
end
```

strict에서 닫힌 레코드에 일반 대입으로 새 필드를 추가할 수 없다. 위의 명시적 함수 멤버 선언은 별도로 처리한다. 원소를 추가할 컨테이너는 `local items: {number} = {}`처럼 선언한다. gradual은 알 수 없는 필드 접근을 허용하지만 그 필드에 대한 추적·안전성은 보장하지 않는다. 알려진 필드에 잘못된 값을 대입하는 오류는 두 모드 모두 차단한다.

가변 테이블의 필드 계약은 별칭 사이에서 바꾸지 않는다. 아래의 새 리터럴은 optional 계약으로 검사하지만, 이미 추론한 테이블의 계약을 넓히는 두 번째 예는 오류다. 테이블 복제나 Rust의 move가 발생하는 것은 아니다.

```lua
local player: {score: number?} = {score = 1}
player.score = nil -- 허용
```

```lua
local original = {score = 1}
local wider: {score: number?} = original -- 오류: 별칭을 통해 원래 계약을 깨뜨릴 수 있음
```

문자열 상수 키는 Lua escape를 해석해서 비교한다. `name`과 `["na\x6de"]`는 같은 필드다. 같은 상수 필드를 중복 선언하면 진단한다. nil 키를 거절하며, 서로 다른 필드 타입을 가진 레코드의 동적 키 쓰기도 제한한다. 메타테이블 동작·사용자 API·명시적 any를 통한 변경은 정적으로 보장하지 않는다.

## nil 확인과 흐름 분석

지역 변수의 `== nil`, `~= nil`, truthiness, `not`, 조기 반환을 분석한다. `and/or`의 오른쪽 표현식에서도 왼쪽의 조건을 반영한다. 단순한 지역 변수만 좁히므로 `if player.score ~= nil` 뒤에 `player.score` 자체가 좁혀지지는 않는다. 필드를 지역 변수로 읽어 확인한다.

```lua
local function add_one(score: number?): number
   if score == nil then
      return 0
   end
   return score + 1
end
```

함수 호출은 외부 지역 변수를 변경할 수 있으므로 현재는 `print`를 포함한 모든 호출 뒤에 좁힌 타입을 선언 계약으로 되돌린다. 호출 전 값을 `local saved = score`에 담으면 그 값의 추론 타입을 유지할 수 있다. 지연 실행되는 함수 본문과 반복문 경계에도 보수적인 처리를 적용한다. 복합 조건 전체의 좁히기를 분기 본문까지 재구성하지는 않는다.

## 기본 API와 남은 범위

내장 타입은 `print`, `tostring`, `tonumber`, `require`, `error`, `type`, `pairs`, `ipairs`, `select`, `unpack`, `math.abs/floor/ceil/sqrt/min/max/pi/huge`, `string.len/upper/lower/sub`에 제공한다. 인자·반환형을 검사하는 초기 시그니처이며 모든 라이브러리나 값 범위·런타임 예외를 모델링하지 않는다. CLI에서 정적 require의 결과는 typed 모듈의 공개 타입 또는 legacy `.d.arky`에서 얻는다. 선언 없는 legacy·외부·동적 require는 unknown이며 strict에서는 명시적 `any` 경계가 필요하다. [혼합 모듈 안내](modules.md)

아직 지원하지 않는 typed 문법은 일반 iterator 목록, goto/label, 타입 별칭·사용자 제네릭·enum·match다. Task는 내장 결과 타입으로만 제공한다. 정적 혼합 모듈 로더·레거시 선언 파일·프로젝트 전체 검사와 진단 LSP를 구현했다.

파일당 4096 토큰과 중첩 128의 초기 제한이 있다. 한계를 넘거나 미지원 문법을 사용하면 검사 성공으로 처리하지 않고 진단한다. 큰 기존 프로젝트는 `.lua` legacy 경로를 유지하며 지원 범위에 맞는 파일부터 전환할 수 있다.

실행 예제는 [control_flow.arky](../examples/control_flow.arky)와 [multiple_values.arky](../examples/multiple_values.arky), 자동 검증은 [테이블 검사](../crates/tynex-compiler/tests/tables_control.rs)·[다중 값과 메서드 검사](../crates/tynex-compiler/tests/multiple_values.rs)·[LuaJIT 통합 테스트](../tests/test_cli.py)에 있다. 통합 테스트는 공통 Lua 문법 27개 사례의 공식 엔진 직접 실행 결과와 변환 후 결과, 원본 위치의 디버그 훅을 확인한다. 이는 VS Code/Zed UI나 다른 플랫폼 기기에서 실행했다는 의미는 아니다.

## 프로젝트 문법 확장

`.arky`는 `import name from "module"`, `.d.arky`는 `declare name: TYPE`을 지원한다. `try … except err then … else … end`는 중첩된 return/break와 다중 반환을 보존한다. 스코프·로더·캐시·선언 계약·지원 범위는 [import·선언·예외 처리](imports-exceptions.md)를 따른다. 기존 `require`와 `export: TYPE`도 유지한다.
