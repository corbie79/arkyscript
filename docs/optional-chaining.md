# nil일 때 건너뛰는 접근과 호출

`.tlj`에서 `?.`, `?[`, `?(`, `?:`를 지원한다. 검사한 값을 한 번 평가하고, **nil이면 연속된 접근·호출을 건너뛰어 nil 하나를 반환**한다. `false`는 일반 값이며 건너뛰지 않는다. 공식 LuaJIT 원본은 수정하지 않고 Rust 컴파일러가 일반 Lua로 변환한다.

| 표기 | nil 확인 대상 | 실행되는 동작 |
| --- | --- | --- |
| `player?.name` | player | 필드 읽기 |
| `values?[key()]` | values | 키 계산 후 인덱스 읽기 |
| `callback?(argument())` | callback | 인자 계산 후 함수 호출 |
| `player?:move(10)` | player | player를 self로 전달하는 메서드 호출 |

`?`와 바로 다음 기호는 붙여 쓴다. receiver와 연산자 사이, 인자 안에는 줄바꿈과 주석을 사용할 수 있다. 선택적 일반 함수 호출에는 괄호가 필요하다. 메서드는 기존 Lua처럼 `object?:method "text"`·`object?:method {value=1}`도 지원한다.

```lua
local function notify(callback: (function(string): ())?): ()
   callback?("done")
end

notify(nil) -- 호출을 건너뜀
notify(function(message: string): () print(message) end)
```

함수 자체가 nil일 수 있는 타입은 `(function(string): ())?`처럼 묶는다. `function(string): number?`는 항상 존재하는 함수가 number 또는 nil을 반환한다는 뜻이다. 일반 타입도 `(number)?`처럼 묶을 수 있다.

nullable 함수를 반환하는 함수는 `local function factory(): (function(number): number)?`처럼 적는다. 여러 반환값 목록 전체에 `?`를 붙이는 `(number, string)?`는 허용하지 않는다. 필요한 위치마다 `(number?, string?)`처럼 표기한다.

## 연속된 체인과 괄호

```lua
local a = root?.child.name   -- root가 nil이면 전체를 건너뜀
local b = root?.child?.name  -- child도 nil인지 확인
local c = (root?.child).name -- 괄호 뒤 .name은 별도 접근
```

첫 번째 예에서 root가 존재하지만 child가 nil이면 일반 `.name` 접근에서 오류가 발생한다. 알려진 nullable 타입에 이런 접근을 하면 컴파일러가 진단한다. 괄호 뒤의 접근은 앞 체인의 생략 대상에 포함되지 않는다.

`player?:move()`는 player만 확인한다. player가 존재하는데 move가 없으면 오류다. `api?.callback?()`는 api와 callback을 각각 확인하며, 점 호출이므로 self를 추가하지 않는다. 실행된 함수 내부의 오류도 그대로 전파된다. 오류를 처리하려면 `try/except`를 사용한다.

## 평가와 반환값

- receiver는 한 번만 평가한다. 건너뛴 부분의 키·인자는 평가하지 않는다.
- 마지막 단계가 호출이면 실제 함수의 다중 반환·빈 반환·중간과 끝의 nil을 보존한다. 건너뛰면 nil 하나를 반환한다.
- 중간 호출 결과와 괄호로 묶은 호출은 Lua 규칙대로 한 값으로 줄어든다.
- 마지막이 필드나 인덱스인 체인은 값 하나를 반환한다.
- 직접적인 선택적 대입(`player?.name = "Mina"`)은 허용하지 않는다.

```lua
local function values(): (number, string, nil)
   return 42, "ok", nil
end
print(select('#', values?()))   -- 3
print(select('#', (values?()))) -- 1
```

타입 검사는 gradual·strict에서 모두 알려진 잘못된 인자와 접근을 거절한다. 실행 시 생략될 수 있는 인자도 검사하고 그 안의 정적 모듈 의존성을 준비한다. guard는 평가한 receiver에 적용하며 원래 지역 변수의 타입까지 인자 안에서 좁히지는 않는다. 호출 뒤에는 기존 규칙대로 외부 변수의 좁힌 타입을 되돌린다.

## 도구와 실행

VS Code·Zed 구문 강조와 LSP 진단에 포함된다. 생성 코드는 원본 줄·소스 맵을 유지한다. 변환에 내부 함수가 사용되어 디버거에서 추가 프레임과 내부 변수가 보일 수 있다. 새 문법의 실제 에디터 GUI 세션은 별도 검증 대상이다.

```sh
./bin/tynex run examples/optional_chaining.tlj
python3 -m unittest discover -s tests -p test_optional_chaining.py -v
```

[실행 예제](../examples/optional_chaining.tlj)는 class 메서드·필드·nullable 콜백·사전 접근을 보여준다. 기존 `.lua` legacy 파일의 문법은 바뀌지 않는다. `??` 기본값 연산자와 삼항 연산자는 포함하지 않는다.
