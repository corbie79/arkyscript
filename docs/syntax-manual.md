# Arky 문법 매뉴얼

Arky는 Lua 문법에 타입 표기, class, trait, import, 예외 처리, async/await를 더한 스크립트 언어입니다. 소스 확장자는 `.arky`이며, Tynex 컴파일러가 타입을 검사하고 Lua로 변환합니다. 현재 실행 대상은 공식 LuaJIT입니다.

이 매뉴얼은 **현재 저장소에서 구현한 문법**을 설명합니다. 설계 중인 enum·match 등은 마지막 절에 따로 정리했습니다. 타입 표기는 정적 검사에 사용하며, 모든 값에 런타임 타입 검사를 추가하는 것은 아닙니다.

## 목차

1. [첫 프로그램과 실행](#1-첫-프로그램과-실행)
2. [소스 표기와 값](#2-소스-표기와-값)
3. [변수와 스코프](#3-변수와-스코프)
4. [타입](#4-타입)
5. [연산자](#5-연산자)
6. [조건문과 반복문](#6-조건문과-반복문)
7. [함수와 메서드](#7-함수와-메서드)
8. [다중 반환과 가변 인수](#8-다중-반환과-가변-인수)
9. [테이블](#9-테이블)
10. [nil 검사와 선택적 접근](#10-nil-검사와-선택적-접근)
11. [클래스](#11-클래스)
12. [Trait](#12-trait)
13. [모듈과 선언 파일](#13-모듈과-선언-파일)
14. [예외 처리](#14-예외-처리)
15. [async와 await](#15-async와-await)
16. [검사 모드](#16-검사-모드)
17. [현재 제약과 상세 문서](#17-현재-제약과-상세-문서)

## 1. 첫 프로그램과 실행

다음 내용을 `hello.arky`로 저장합니다.

```lua
--!typecheck strict
local function greet(name: string): string
   return "Hello, " .. name
end

print(greet("Arky"))
```

저장소 루트에서 실행합니다. `make setup`은 처음 한 번 엔진과 실행에 필요한 구성 요소를 준비합니다.

```sh
make setup
./bin/tynex check hello.arky
./bin/tynex run hello.arky
./bin/tynex gen hello.arky
```

- `check`: 타입과 문법을 검사합니다.
- `run`: 검사한 다음 실행합니다. 위 예제는 `Hello, Arky`를 출력합니다.
- `gen`: 검사한 다음 진입 파일의 Lua 코드를 출력합니다.

`bin/tynex`은 저장소의 개발용 실행 래퍼입니다. 배포된 CLI를 사용한다면 `tynex`로 바꿉니다. import나 async를 사용하는 생성 코드는 전용 로더·런타임 연결이 필요하므로 실행은 `run`을 사용하세요.

## 2. 소스 표기와 값

들여쓰기는 가독성을 위한 것입니다. 블록은 `end` 등의 키워드로 닫습니다. 문장 끝의 세미콜론은 생략할 수 있습니다. 이름은 영문자 또는 `_`로 시작하고 영문자·숫자·`_`를 사용하며 대소문자를 구분합니다. 문자열 안에는 한글을 사용할 수 있습니다.

```lua
local count = 42
local ratio = 1.5
local small = 1e-3
local mask = 0xff
local active = true
local absent = nil
local title = "Arky"
local message = '안녕하세요\nArky'
local multiline = [=[첫째 줄
둘째 줄]=]
-- 한 줄 주석
--[=[
여러 줄 주석
]=]
print(count, ratio, small, mask, active, absent, title)
print(message, multiline)
```

숫자는 `number` 타입입니다. 정수와 실수를 구분하는 사용자 타입은 없습니다. 10진수·16진수·지수 표기를 지원합니다. `1ULL` 같은 LuaJIT 전용 숫자 접미사는 typed 문법에서 지원하지 않습니다.

문자열은 작은따옴표 또는 큰따옴표로 감싸며 `\n`, `\t`, `\\`, `\"`, `\'`, `\xHH`, 10진 바이트 escape 등을 사용할 수 있습니다. 긴 문자열은 `[[...]]` 또는 `[=[...]=]`처럼 작성하며 escape를 해석하지 않습니다. 긴 주석도 같은 괄호 규칙을 사용합니다.

조건에서 **`false`와 `nil`만 거짓**입니다. `0`과 빈 문자열 `""`은 참입니다. `nil`은 값이 없음을 나타내며, 테이블 필드에 대입하면 그 키가 제거됩니다.

Lua 키워드와 `class`, `trait`, `async`, `await`, `import`, `from`, `declare`, `try`, `except` 등을 변수 이름으로 사용하지 마세요. 미구현인 `enum`, `match`, `goto`도 예약되어 있습니다. `__tynex_`로 시작하는 이름은 컴파일러용입니다.

## 3. 변수와 스코프

`local 이름 = 값`으로 지역 변수를 선언합니다. 타입은 이름 뒤의 `:`로 붙입니다. 선언한 변수에는 다시 대입할 수 있지만 알려진 타입 계약을 바꿀 수는 없습니다.

```lua
local score = 10                 -- number로 추론
local name: string = "Mina"
local bonus: number? = nil
local left: number, right: number = 1, 2
left, right = right, left        -- 두 우변을 평가한 뒤 대입
score = score + 5

do
   local name = "Joon"           -- 안쪽 블록에서만 바깥 이름을 가림
   print(name)
end
print(name, score, bonus, left, right)
```

함수·조건 분기·반복문·`do ... end`는 지역 스코프를 만듭니다. 함수는 바깥 지역 변수를 캡처할 수 있습니다. 테이블과 클래스 값의 대입은 참조를 공유하며 자동으로 복제하지 않습니다.

여러 변수에 대입할 때 값이 부족하면 `nil`로 채우고, 많으면 초과 값을 버립니다. `number`처럼 nil을 허용하지 않는 계약에 값이 부족하면 타입 오류입니다.

strict에서는 선언하지 않은 변수 사용을 진단합니다. 일반 함수 선언의 이름 처리에는 별도 규칙이 있으며 [함수 절](#7-함수와-메서드)을 참고하세요.

## 4. 타입

| 표기 | 의미 |
| --- | --- |
| `number`, `string`, `boolean`, `nil` | 기본 타입 |
| `any` | 정적 검사를 일부 넘어가는 동적 값 |
| `T?` | `T` 또는 `nil` |
| `{name: string, score: number}` | 필드 계약이 있는 레코드 |
| `{number}` | 숫자 원소 배열 |
| `{[string]: number}` | 문자열 키 사전 |
| `{}` | 알려진 필드가 없는 닫힌 레코드 |
| `function(number): string` | 숫자 하나를 받아 문자열 하나를 반환하는 함수 |
| `(function(number): string)?` | 함수 자체가 nil일 수도 있는 값 |
| `Player`, `Named`, `models.Player` | class·trait 또는 모듈에서 가져온 명명 타입 |
| `Self` | class·trait의 현재 구현 타입 |
| `Task<number, string>` | 두 결과를 가진 비동기 작업 |
| `Task<>` | 반환값 없는 비동기 작업 |

함수 반환 위치에서는 `: (number, string)`으로 여러 반환형을, `: ()`로 반환값 없음을 표기합니다. 이것은 튜플 객체 타입이 아니라 **Lua의 반환값 목록**입니다.

`function(number): string?`는 항상 존재하는 함수가 nullable 문자열을 반환한다는 뜻입니다. 함수 자체가 optional이면 위 표처럼 함수 타입 전체를 괄호로 감쌉니다.

타입 추론이 가능하면 생략할 수 있습니다. strict에서도 `local n = 1`은 올바릅니다. 함수 매개변수는 호출 위치에서 역추론하지 않으므로 타입을 써야 합니다. 재귀 함수와 클래스 메서드는 반환형을 명시하면 계약을 확인하기 쉽습니다.

`any`는 런타임 타입 변환이나 검증이 아닙니다. legacy 모듈이나 호스트 API가 잘못된 값을 돌려주면 실행 중 오류가 날 수 있습니다. 타입 별칭, 사용자 제네릭, `A | B` 표기는 아직 지원하지 않습니다. `Task<...>`는 내장 타입입니다.

## 5. 연산자

아래는 우선순위가 낮은 것부터 높은 것 순서입니다. 괄호로 평가 순서를 명시할 수 있습니다.

| 순서 | 연산자 | 의미 |
| --- | --- | --- |
| 1 | `or` | 왼쪽이 거짓이면 오른쪽 값 |
| 2 | `and` | 왼쪽이 참이면 오른쪽 값 |
| 3 | `==`, `~=`, `<`, `<=`, `>`, `>=` | 비교 |
| 4 | `..` | 문자열 연결 |
| 5 | `+`, `-` | 덧셈·뺄셈 |
| 6 | `*`, `/`, `%` | 곱셈·나눗셈·나머지 |
| 7 | `not`, 단항 `-`, `#`, `await` | 부정·음수·길이·비동기 대기 |
| 8 | `^` | 거듭제곱 |

필드 접근·인덱스·함수 호출은 위 연산자보다 먼저 묶입니다. `..`와 `^`는 오른쪽부터 묶이고 나머지 이항 연산자는 왼쪽부터 묶입니다. 예를 들어 `-2 ^ 2`는 `-(2 ^ 2)`입니다.

```lua
local n = 2 + 3 * 4
local label = "score: " .. n
local fallback: string? = nil
local chosen = fallback or "default"
print(n, label, chosen, -2 ^ 2, #"Arky")
```

`and`와 `or`는 단락 평가하며 boolean으로 강제 변환하지 않고 선택된 값을 반환합니다. 따라서 `value or default`는 `false`도 기본값으로 바꿉니다. nil만 대체하려면 `if value == nil then ... end`를 사용합니다.

문자열 연결에는 `+` 대신 `..`, 서로 다름 비교에는 `!=` 대신 `~=`를 씁니다. 숫자와 연결 연산자는 `1 .. "개"`처럼 공백으로 구분합니다. 문자열의 `#`는 바이트 길이이며, 구멍이 있는 테이블의 `#`를 원소 개수로 신뢰하면 안 됩니다.

`+=`, `++`, `//`, `??`, 삼항 연산자와 비트 연산자 문법은 지원하지 않습니다.

## 6. 조건문과 반복문

### if / elseif / else

```lua
local score = 75
if score >= 90 then
   print("A")
elseif score >= 60 then
   print("B")
else
   print("C")
end
```

### while / repeat

`while`은 본문 실행 전에, `repeat`는 실행 후에 조건을 확인합니다. `repeat`는 최소 한 번 실행하고 `until` 조건이 참이면 끝납니다.

```lua
local count = 0
while count < 3 do
   count = count + 1
end
repeat
   count = count - 1
until count == 0
print(count)
```

### 숫자 for / 테이블 순회

숫자 `for`는 끝값을 포함하며 증가값을 생략하면 1입니다. 감소할 때는 음수 증가값을 씁니다. `break`는 가장 가까운 반복문을 끝냅니다.

```lua
for i = 3, 1, -1 do
   print(i)
end

local names: {string} = {"Mina", "Joon"}
for index, name in ipairs(names) do
   print(index, name)
end

local counts: {[string]: number} = {apple = 2, pear = 3}
for key, value in pairs(counts) do
   print(key, value)
end

for i = 1, 10 do
   if i == 3 then break end
   print(i)
end
```

`ipairs`는 1부터 연속된 원소를 순회하고 첫 nil에서 멈춥니다. `pairs`의 순서는 보장되지 않습니다. 검사기는 두 내장 함수의 반복 변수 타입을 추론합니다. 사용자 iterator 정적 분석과 `iterator, state, control` 목록 형식은 지원하지 않습니다. `continue`도 없으므로 조건문으로 본문을 나눕니다.

## 7. 함수와 메서드

### 선언, 함수 값, optional 인자

```lua
local function add(a: number, b: number): number
   return a + b
end

local twice: function(number): number = function(n: number): number
   return n * 2
end

local function greet(name: string?): string
   return "Hello, " .. (name or "guest")
end

print(add(2, 3), twice(4), greet(), greet("Mina"))
```

끝의 optional 인자는 생략할 수 있습니다. 일반 매개변수에 `name = "guest"` 형태의 기본값 문법은 없습니다. 함수 본문에서 처리합니다. 함수는 일급 값이므로 변수에 저장하거나 인자로 전달할 수 있습니다.

일반 선언 `function name(...)`은 같은 이름의 지역 변수가 있으면 그 변수에 대입하고, 없으면 전역 함수로 선언합니다. 지역 함수에는 `local function`을 사용하세요. 반환형에 non-nil 값을 선언했다면 모든 정상 종료 경로에서 그 값을 반환해야 합니다.

### 테이블 메서드와 self

```lua
local counter = {value = 0}
function counter:add(amount: number): number
   self.value = self.value + amount
   return self.value
end
print(counter:add(3))
print(counter.add(counter, 2))
```

`counter:add(3)`은 counter를 첫 인자 `self`로 전달합니다. `counter.add(3)`은 self를 자동으로 넣지 않습니다. 메서드 호출의 receiver는 한 번 평가됩니다.

테이블 함수는 `function module.fn(...)`으로도 선언합니다. 조건문·루프 안에서 새 비지역 함수나 새 메서드를 처음 정의할 수는 없습니다. 먼저 선언한 함수의 같은 타입 재정의는 가능합니다.

일반 테이블 메서드의 self 타입에는 나중에 추가한 메서드가 반영되지 않습니다. self 재귀·전방 메서드 호출이 필요하다면 전체 계약을 먼저 등록하는 class를 사용하세요. class 메서드의 반환형은 명시합니다.

## 8. 다중 반환과 가변 인수

```lua
local function pair(n: number): (number, string)
   return n, tostring(n)
end
local first: number, text: string = pair(7)
local one: number, missing: nil = (pair(7))
print(first, text, one, missing)

local function forward(first: number, ...: string): (number, ...string)
   return first, ...
end
local function count(...: any): number
   return select("#", ...)
end
print(forward(1, "a", "b"))
print(count(1, nil, 3))
```

- 대입·인자·return 목록의 **마지막 호출 또는 `...`**만 여러 값으로 확장합니다.
- `(pair(7))`처럼 괄호로 감싸거나 목록의 앞쪽에 두면 한 값만 사용합니다.
- 테이블도 마지막 목록 필드만 확장합니다. `{pair(7)}`과 `{(pair(7))}`는 다릅니다.
- `: ()`는 반환값 없음이고 `: nil`은 nil을 담는 한 위치입니다.
- `...: T`는 마지막 매개변수에만 쓸 수 있습니다. 가변 반환은 `: (...T)` 또는 `: (number, ...T)`입니다.
- `...`를 한 값으로 읽으면 인수가 없을 수 있으므로 `T?`입니다.

`select("#", ...)`는 nil을 포함한 전달 값의 개수를 반환합니다. 내부에 정의한 비가변 함수에서 바깥 함수의 `...`를 직접 사용할 수 없습니다. 현재 가변 결과 개수는 호출별로 정밀하게 특수화하지 않습니다.

## 9. 테이블

레코드·배열·사전은 모두 Lua 테이블이며, 타입 표기는 접근 계약을 구분합니다.

```lua
local player: {name: string, score: number} = {name = "Mina", score = 0}
local scores: {number} = {10, 20, 30}
local counts: {[string]: number} = {}

player.score = 42
scores[4] = 40
counts["apple"] = 3
print(player.name, player["score"])

local score = scores[100]
if score ~= nil then
   print(score + 1)
end
```

배열은 보통 1부터 인덱싱합니다. 배열·사전 읽기는 없는 키를 고려해 `T?`를 반환합니다. 알려진 레코드 필드는 선언한 타입으로 읽습니다. 고정 문자열 키는 `player.name`과 `player["name"]`로 접근할 수 있습니다.

strict에서 `local t = {}`는 닫힌 빈 레코드입니다. 나중에 원소를 넣을 배열은 `local t: {number} = {}`처럼 의도를 명시합니다. 레코드에 일반 대입으로 새 필드를 추가할 수는 없습니다.

테이블은 참조를 공유하므로 이미 추론한 필드 계약을 별칭을 통해 넓힐 수 없습니다. nil을 저장할 필드는 처음부터 optional로 선언하세요.

```lua
local player: {score: number?} = {score = 1}
local alias = player
alias.score = nil
print(player.score) -- nil: 같은 테이블
```

한 타입 표기 안에 고정 필드와 인덱스 계약을 섞는 문법은 아직 없습니다. 배열의 연속성·길이, 메타테이블과 any를 통한 변경도 정적으로 보장하지 않습니다.

## 10. nil 검사와 선택적 접근

### 지역 변수의 타입 좁히기

```lua
local function add_one(value: number?): number
   if value == nil then
      return 0
   end
   return value + 1
end
print(add_one(nil), add_one(4))
```

검사기는 지역 변수의 nil 비교·참/거짓 확인·`not`·조기 반환을 분석합니다. 테이블 필드 자체를 계속 추적하지는 않으므로 필드를 지역 변수로 읽어 검사하세요.

현재 `print`를 포함한 함수 호출 뒤에는 좁힌 타입을 원래 계약으로 되돌립니다. 호출 이후에도 사용할 값은 nil 확인 뒤 새 지역 변수에 저장할 수 있습니다.

```lua
local function show(value: number?): ()
   if value ~= nil then
      local saved = value
      print("value found")
      print(saved + 1)
   end
end
show(3)
```

### 선택적 접근과 호출

| 문법 | nil일 때 생략하는 대상 |
| --- | --- |
| `object?.field` | object의 필드 접근 |
| `items?[key]` | items의 인덱스 접근과 key 평가 |
| `callback?(args)` | callback 호출과 인자 평가 |
| `object?:method(args)` | object의 메서드 호출과 인자 평가 |

```lua
local function notify(callback: (function(string): ())?): ()
   callback?("done")
end
local function title(player: {name: string}?): string?
   return player?.name
end
notify(nil)
notify(function(message: string): () print(message) end)
print(title(nil), title({name = "Mina"}))
```

`?`와 다음 기호는 붙여 씁니다. nil만 건너뛰고 false는 건너뛰지 않습니다. 생략하면 nil 하나를 반환합니다. 실제 호출했다면 마지막 호출의 다중 반환을 보존합니다.

`root?.child.name`은 root가 nil일 때 연속 체인 전체를 생략합니다. root는 있지만 child가 nil일 수 있다면 `root?.child?.name`으로 각각 확인해야 합니다. `(root?.child).name`의 괄호 뒤 접근은 생략 대상에서 벗어납니다.

`object?:method()`는 object만 확인합니다. method의 존재나 함수 내부 오류까지 처리하지 않습니다. `object?.field = value`와 같은 선택적 대입은 지원하지 않습니다.

## 11. 클래스

```lua
class Player
   name: string
   score: number = 0
   tags: {string} = {}

   function add(amount: number): ()
      self.score = self.score + amount
   end

   function label(): string
      return self.name .. ": " .. self.score
   end
end

local player: Player = Player {name = "Mina"}
player:add(10)
print(player:label())
```

`class`는 현재 블록에 생성자 값과 타입 이름을 만듭니다. 생성자는 정확히 하나의 초기화 테이블을 받습니다. `Player {...}`와 `Player({...})`는 같습니다.

- 기본값 없는 non-nil 필드는 필수입니다. 알 수 없는 필드나 잘못된 타입은 오류입니다.
- optional·nil·any 필드는 생략할 수 있습니다.
- 기본값은 생략된 필드에 대해서만, 선언 순서대로, 인스턴스를 만들 때마다 평가합니다.
- 기본값이 있는 필드에 nil을 주면 기본값을 사용합니다. false는 유지합니다.
- `tags = {}` 같은 기본값 식은 생성할 때마다 새 테이블을 만듭니다. 바깥 테이블 변수를 기본값으로 쓰면 그 참조를 공유합니다.
- 초기화 테이블은 얕게 복사합니다. 중첩 값은 자동으로 깊게 복사하지 않습니다.

클래스는 선언의 정체성으로 구분합니다. 필드 모양이 같아도 서로 다른 클래스는 같은 타입이 아닙니다. 메서드는 인스턴스에서 호출하고 self는 암묵적으로 전달됩니다. `score = 0`처럼 필드 타입을 기본값에서 추론할 수도 있습니다.

상속·정적 메서드·접근 제어·사용자 정의 생성자 문법은 아직 없습니다. `function init()`를 작성해도 생성 시 자동 실행되는 특별한 생성자가 되지 않습니다. 기본값에서 초기화 중인 self를 참조할 수도 없습니다.

## 12. Trait

trait은 클래스가 구현할 메서드 계약과 기본 구현을 정의합니다.

```lua
trait Named
   label: function(Self): string

   function describe(): string
      return "name: " .. self:label()
   end
end

class User is Named
   name: string
   function label(): string
      return self.name
   end
end

local function show(value: Named): ()
   print(value:describe())
end
show(User {name = "Mina"})
```

`label: function(Self): string`은 필수 메서드 계약입니다. 첫 `Self`는 receiver이며, 실제 메서드 선언의 매개변수 목록에는 self를 직접 적지 않습니다. 본문이 있는 `function describe()`는 기본 구현입니다.

`class Player is Named, Scorable`처럼 여러 trait을 구현할 수 있습니다. 필수 메서드 누락·계약 불일치는 오류입니다. 여러 trait이 같은 이름의 기본 구현을 제공하면 클래스에서 직접 구현해 충돌을 해결합니다.

메서드 모양이 같다는 이유만으로 trait을 구현한 것으로 보지 않습니다. `is`를 명시해야 합니다. trait에는 필드·생성자·상속이 없습니다.

`equal: function(Self, Self): boolean`처럼 receiver 외의 인자에도 Self가 있으면 구체 클래스 값으로 호출해야 합니다. 구현 클래스가 지워진 trait 값에서는 서로 다른 구현을 섞을 수 있어 호출이 제한됩니다. 자세한 Self 제약은 [class·trait 상세 문서](classes-traits.md)를 참고하세요.

## 13. 모듈과 선언 파일

### 모듈 반환과 import

모듈은 공개할 값을 `return`합니다. 아래 두 파일을 같은 폴더에 저장합니다.

**`numbers.arky`**

```lua
local function double(n: number): number
   return n * 2
end
return {double = double}
```

**`main.arky`**

```lua
import numbers from "numbers"
print(numbers.double(21))
```

`./bin/tynex run 경로/main.arky`로 실행하면 42를 출력합니다. `from` 뒤에는 문자열 리터럴을 사용합니다. 모듈 검색·중복·순환 의존은 실행 전에 검사합니다.

import 이름은 재대입할 수 없지만 가져온 테이블의 필드까지 불변이 되는 것은 아닙니다. 조건문이나 함수 안의 import도 의존성 검사는 미리 수행하고 초기화는 문장을 실행할 때 수행합니다.

기존 `local numbers = require("numbers")`도 지원합니다. 문자열 리터럴을 사용하는 정적 require는 모듈의 공개 타입을 가져옵니다. 호스트·C 모듈에는 `require`와 프로젝트의 `external_modules` 설정을 사용합니다. 구현 파일이 없는 모듈은 정적 import 대상이 될 수 없습니다.

클래스·trait은 `return {Player = Player, Named = Named}`로 내보낼 수 있습니다. 가져온 뒤 `models.Player {...}`, `local p: models.Player`, `class Device is models.Named`처럼 사용합니다.

### legacy Lua용 선언 파일

`formatter.lua`가 반환하는 테이블의 계약은 같은 폴더의 `formatter.d.arky`에 작성합니다.

```text
declare label: function(string, number): string
declare version: string
```

각 `declare`는 모듈이 반환하는 테이블의 필드이며 전역 변수를 만들지 않습니다. 테이블 이외의 값을 반환하는 모듈은 `export: function(number): number`처럼 전체 계약을 작성합니다. `declare`와 `export:` 형식은 한 파일에서 섞지 않습니다.

선언 파일에는 초기값이나 실행문을 쓰지 않습니다. 현재 class·trait 명명 타입 선언도 지원하지 않습니다. 선언 파일은 legacy 구현을 검증하는 것이 아니라 **구현이 지킨다고 신뢰하는 계약**입니다. `.arky` 구현의 공개 타입은 반환값에서 추론하며 별도 sidecar 선언을 붙이지 않습니다.

## 14. 예외 처리

```lua
local result = 0
try
   result = 21 * 2
except err then
   print("failed: " .. tostring(err))
else
   print("success: " .. result)
end

try
   error("example failure")
except err then
   print(tostring(err))
end
```

`except 이름 then` 하나가 필수이고 `else`는 선택입니다. err는 문자열 이외의 Lua 오류 값도 받을 수 있는 `any`입니다.

- try 본문이 오류·return·break 없이 끝났을 때만 else를 실행합니다.
- except나 else에서 발생한 오류는 같은 except에서 다시 잡지 않습니다.
- try 안의 return은 바깥 함수를 반환하며 다중 반환·nil을 보존합니다.
- break는 원래 대상 반복문을 빠져나옵니다.
- try·except·else는 각각 별도 지역 스코프입니다. 함께 사용할 변수는 try 밖에 선언합니다.

`finally`, 여러 except, 예외 타입에 따른 분기 문법은 아직 없습니다.

## 15. async와 await

```lua
local async function double(n: number): number
   return n * 2
end

local async function main(): number
   local first: Task<number> = double(21)
   local second = double(15)
   return (await first) + (await second)
end

print(scheduler.run(main())) -- 72
```

`async function f(): number`는 본문에서 number를 반환하지만, `f()` 호출은 즉시 `Task<number>`를 반환합니다. await는 async 본문에서 Task를 기다리고 결과를 꺼냅니다. `local async function`, 일반 async 함수·메서드·함수 값, class·trait의 async 메서드를 지원합니다.

함수 값의 계약은 `async function(number): number`입니다. 결과가 여럿이면 `Task<number, string>`, 없으면 `Task<>`, 가변 결과면 `Task<number, ...string>`으로 표기합니다.

- 최상위나 일반 함수 본문에서 await를 사용할 수 없습니다.
- `return await task`는 여러 결과를 보존하고 `(await task)`는 한 값으로 줄입니다.
- await는 한 단계만 풀며, nullable Task는 nil 검사가 먼저 필요합니다.
- 명시적인 async 반환 계약의 고정 위치는 실제로 반환해야 합니다. `: number?`에서도 `return nil`과 빈 return은 다릅니다.
- 실패한 Task를 기다리면 오류가 전달되므로 async 본문에서 try/except로 처리할 수 있습니다.

CLI는 `scheduler.run(task)`으로 완료까지 진행합니다. 게임·UI 호스트는 프레임마다 `scheduler.step()`을 호출합니다. CLI가 남은 작업을 자동으로 끝까지 실행하지는 않습니다.

async는 메인 VM의 coroutine을 사용하며 새 OS 스레드를 만들지 않습니다. 긴 계산을 async 함수에 넣는 것만으로 병렬 실행되지 않습니다. CPU 작업에는 worker Task, 파일·TCP 대기에는 `fs`·`net` API를 사용합니다. API와 취소 동작은 [async/await](async-await.md), [Task](tasks.md), [I/O](io.md) 문서에 있습니다.

## 16. 검사 모드

| 모드 | 동작 |
| --- | --- |
| `legacy` | 원본 Lua 실행, check는 LuaJIT 구문 검사 |
| `gradual` | 부족한 타입 정보를 허용하지만 알려진 타입 오류는 진단 |
| `strict` | 알 수 없는 변수·인자 타입 등을 진단하며 추론·명시적 any는 허용 |

설정이 없으면 `.arky`는 gradual, `.lua`는 legacy입니다. 새 코드는 파일 선두에 `--!typecheck strict`를 붙여 시작하는 것을 권합니다.

```toml
# tynex.toml
type_mode = "gradual"

[[overrides]]
files = ["src/**/*.arky"]
mode = "strict"
```

우선순위는 CLI `--mode` → 파일 선두 지시문 → 마지막 일치 override → 프로젝트 기본값 → 확장자 기본값입니다. CLI의 `--mode`는 진입 파일에만 적용합니다.

```sh
./bin/tynex check --mode strict hello.arky
./bin/tynex check --project tynex.toml
./bin/tynex check --format json hello.arky
```

strict가 타입 표기를 모든 변수에 강제하지는 않습니다. any·선언 파일·외부 모듈 경계에는 런타임 타입 보장이 없습니다.

## 17. 현재 제약과 상세 문서

| 항목 | 현재 상태 |
| --- | --- |
| 타입 별칭·사용자 제네릭·명시적 union | 미지원 |
| enum·match | 설계 단계 |
| 클래스·trait 상속, 접근 제어, 특별한 생성자 문법 | 미지원 |
| goto·label·continue | typed 문법에서 미지원 |
| finally·예외 타입별 다중 except | 미지원 |
| 사용자 iterator의 정적 분석 | 제한됨; 일반 iterator 목록은 미지원 |
| 파일 크기·중첩 | 파일당 4096 토큰, 중첩 128 제한 |
| 표준 라이브러리 타입 | 일부 기본 함수와 math·string, scheduler·I/O 등만 제공 |

LuaJIT이 실행할 수 있는 모든 문법을 `.arky` 검사기가 지원하는 것은 아닙니다. 기존 Lua 코드는 `.lua` legacy 경로로 유지하고 필요한 경계를 선언 파일로 설명할 수 있습니다.

### 더 읽기

- [현재 지원 범위와 타입 검사 세부 규칙](supported-language.md)
- [class·trait와 Self 제약](classes-traits.md)
- [선택적 접근과 다중 반환](optional-chaining.md)
- [import·선언 파일·예외 처리](imports-exceptions.md)
- [모듈 검색·혼합 프로젝트 설정](modules.md)
- [async·스케줄러·취소](async-await.md)
- [실행 가능한 예제 목록](../examples/README.md)
- [설계 제안](language-design.md): 구현 여부는 이 매뉴얼과 지원 범위 문서를 기준으로 확인하세요.
