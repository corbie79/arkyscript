# Lua 문법을 유지하는 타입 확장 초안

상태: **class·trait는 구현했다. enum·match·async 문법은 아직 설계 제안이다.** 현재 독립 Rust 컴파일러는 `.tlj`의 기본 타입·함수·테이블·제어 흐름·다중 값·가변 인수·Lua 메서드를 지원한다. 아래 전체 예시를 현재 CLI로 실행할 수는 없다.

목표는 스크립트의 간결함을 유지하면서 `class`, trait, 데이터를 담는 enum, `match`, `async/await`를 제공하는 것이다. `local`, `function`, `end`, `obj:method()` 같은 Lua 문법은 유지한다. 메모리는 LuaJIT GC가 관리한다. 공식 LuaJIT 소스는 수정하지 않고 독립 Rust 프런트엔드·프로젝트 런타임에서 구현한다. Teal을 중간 단계나 타입 검사기로 사용하지 않는다.

## 기존 코드에 타입을 추가하는 원칙

기존 Lua 프로젝트를 파일별로 전환할 수 있게 `legacy`·`gradual`·`strict` 모드를 제공한다. legacy는 기존 `.lua`를 그대로 실행하고, gradual은 타입 정보 부족을 허용하면서 확인 가능한 불일치를 검사한다. strict에서도 타입 추론을 허용하며 모든 변수에 표기를 강제하지 않는다. 모듈마다 자신의 정책을 적용하고 선언 파일이나 타입 어댑터로 레거시 API와 연결한다.

새 문법의 구조적 규칙은 gradual에서도 적용한다. 예를 들어 잘못 선언된 enum이나 충돌하는 trait 기본 메서드를 단순히 타입 생략으로 취급하지 않는다. 현재 Rust 검사기는 nil을 구분하고 `T?` 및 지역 변수의 nil 좁히기를 제공한다. 필드 경로와 사용자 정의 복합 타입의 좁히기는 후속 기능이다. 설정 우선순위, 디버깅, 단계별 완료 조건은 [구현 계획](implementation-plan.md)을 기준으로 한다. 정적 의존 모듈마다 정책을 적용하며 legacy 모듈에는 `.d.tlj` 공개 계약을 붙일 수 있다. [현재 모듈 규칙](modules.md)

## 기능의 역할

| 기능 | 역할 | 제안하는 첫 범위 |
| --- | --- | --- |
| `class` | 필드와 메서드를 하나의 타입으로 묶음 | 필드 타입, 생성 시 초기화 검사, 인스턴스 메서드 |
| `trait` | 타입이 제공해야 하는 동작과 공통 구현 | 명시적 구현 선언, 필수 메서드 검사, 기본 메서드 |
| `enum` | 여러 경우 중 하나와 그 경우의 데이터 | 매개변수가 있는 variant, 제네릭 `Option<T>`·`Result<T, E>` |
| `match` | enum을 안전하게 분기하고 데이터를 꺼냄 | payload 타입 추론, 빠진 경우·중복 경우 검사 |
| `async/await` | 비동기 작업을 순서대로 작성 | `Task<T>`, 코루틴 스케줄러, 오류·취소 전파 |

## class와 trait

class는 상태를 소유하고 trait은 동작을 정의한다. 첫 구현은 trait 조합을 중심으로 하고 클래스 상속은 후속 검토 항목으로 둔다.

독립된 [class 실행 예제](../examples/class.tlj)와 [여러 trait 조합 예제](../examples/traits.tlj)에서 생성·필드·공통 동작을 확인할 수 있다. 구현 규칙과 제한은 [class·trait 안내](classes-traits.md), C++ 바인딩 예제는 [예제 목록](../examples/README.md)에 있다.

```text
trait Named
   label: function(Self): string

   function describe(): string
      return "name: " .. self:label()
   end
end

class User is Named
   name: string
   score: number = 0

   function label(): string
      return self.name
   end

   function add_score(amount: number)
      self.score = self.score + amount
   end
end

local user = User { name = "Mina" }
user:add_score(10)
print(user:describe())
```

trait의 `label`은 함수 타입으로 쓴 필수 메서드 선언이고, `describe`는 본문을 가진 기본 메서드다. `Self`는 구현 클래스의 타입을 가리킨다. 메서드 본문에는 타입이 지정된 `self`가 암묵적으로 들어간다. `User { ... }`는 Lua의 테이블 인자 호출 모양을 따르는 생성자다.

제안 규칙:

- class는 이름으로 구분되는 타입이다. 필드 모양이 같은 다른 class와 자동으로 같은 타입이 되지 않는다.
- 기본값이 없는 필드는 생성 시 반드시 제공하며, 알 수 없는 필드와 타입 불일치는 컴파일 오류다. 테이블 형태의 기본값은 인스턴스마다 새로 평가한다.
- 인스턴스는 참조로 전달하고 필드는 변경 가능하다. 대입 시 자동 복제나 Rust의 move 규칙을 적용하지 않는다.
- `class User is Named, Savable`처럼 구현할 trait을 명시한다. 필수 메서드의 인자·반환 타입이 맞아야 한다.
- trait 자체는 인스턴스 필드와 생성자를 갖지 않는다. 기본 메서드는 선언된 메서드와 `Self`를 통해 동작한다.
- 여러 trait의 기본 메서드 이름이 충돌하면 class가 명시적으로 구현해야 한다. 선언 순서로 어느 하나를 자동 선택하지 않는다.
- trait 타입으로 값을 받을 수 있다. 첫 구현은 Lua 테이블의 메서드 호출을 사용하며 Rust식 정적 디스패치 성능을 약속하지 않는다.

## 데이터를 담는 enum과 match

[독립 enum·match 설계 예제](../examples/design/enum-match.md)에 여러 payload와 Result/Option을 정리했다.

```text
enum Result<T, E>
   Ok(T)
   Err(E)
end

enum LoadState
   Idle
   Loading
   Ready(User)
   Failed(string)
end

local state: LoadState = LoadState.Ready(user)

match state
   case LoadState.Idle then
      print("idle")
   case LoadState.Loading then
      print("loading")
   case LoadState.Ready(value) then
      print(value.name)
   case LoadState.Failed(message) then
      print(message)
end
```

`Ready(User)`에는 사용자 값이, `Failed(string)`에는 오류 설명이 들어간다. `match`의 각 분기에서 payload 타입을 추론한다. 분기는 variant뿐 아니라 해당 enum 타입의 식별자까지 확인한다. 다른 enum의 같은 이름을 가진 variant를 혼동하지 않는다.

첫 구현의 `match`는 문장으로 제한하고, enum의 모든 variant를 처리하거나 마지막에 `case _ then`을 둬야 한다. 중복 분기와 wildcard 뒤의 도달 불가능한 분기는 오류다. 조건 guard와 중첩 패턴은 이후 확장한다.

enum 값은 논리적으로 타입 식별자·variant·payload로 구성된다. 생성 후 variant를 바꾸거나 검사 없이 payload에 접근하는 문법을 제공하지 않는다. payload가 class이면 그 인스턴스 자체는 변경될 수 있다. 기존 Lua/FFI 코드가 내부 표현을 직접 변경하는 경우까지 정적 안전성을 보장하지 않으며 외부 경계에서 검증한다.

`Option<T>`는 `Some(T)`와 `None`으로 값의 부재를 표현한다. 현재 기본 타입용 `T?`와 Option 사이의 변환, 복합 타입의 nil 규칙과 Lua/FFI에서 들어오는 값의 검증은 별도로 설계한다. any나 외부 선언만으로 런타임 값이 검증되지는 않는다.

## async와 await

별도로 **실행 가능한 OS worker Task API**를 추가했다. [Task 런타임](tasks.md)의 `spawn`·`wait`·취소는 현재 사용할 수 있지만 아래 async/await 문법과 메인 coroutine 스케줄러는 아직 없다. [독립 async 설계 예제](../examples/design/async-await.md)는 두 계층의 연결 방향을 보여준다.

```text
-- User, Result, HttpClient, HttpResponse는 별도로 선언된 타입이다.
-- http:get은 플랫폼 어댑터가 제공하는 비동기 API라는 가정이다.
async function load_user(http: HttpClient, url: string): Result<User, string>
   local response = await http:get(url)
   if response.status ~= 200 then
      return Result.Err("request failed")
   end
   return Result.Ok(User { name = response.body })
end

async function main()
   local result = await load_user(http, "/user/1")
   match result
      case Result.Ok(user) then
         print(user.name)
      case Result.Err(message) then
         print(message)
   end
end
```

이 예제의 `http`는 임베딩 호스트가 주입한다. HTTP 클라이언트를 이미 제공한다는 뜻은 아니다.

제안 규칙:

- `async function f(): T`를 호출한 결과의 타입은 `Task<T>`다. 본문의 반환값과 `await f()`의 결과는 `T`다.
- 호출하면 Task를 만들고 실행 큐에 등록한다. 본문은 스케줄러에서 실행한다. 여러 비동기 작업을 동시에 진행하려면 먼저 여러 Task를 만들고 각각 기다린다. CPU 코드를 여러 스레드에서 병렬 실행한다는 뜻은 아니다.
- `await`는 async 함수 안에서 Task에만 사용한다. 대기 중에는 현재 코루틴을 중단하고, 완료 시 한 번만 다시 실행한다. 일반 동기 함수나 외부 C 콜백 안에서 무조건 yield하지 않는다.
- 기대 가능한 업무상 실패는 `Result<T, E>`로 반환한다. 예상하지 못한 Lua 오류는 Task 실패로 보존하고 `await`한 지점으로 전파하며 원래 스택 정보도 유지한다.
- 취소는 협력적으로 처리한다. I/O 어댑터는 취소·완료 경합 시 이중 resume을 막고, 취소 후 도착한 콜백을 무시한다. 취소 요청은 CPU 연산을 강제 선점하지 않는다.
- 스케줄러와 Lua 상태는 같은 소유 스레드에서 실행한다. 외부 작업 스레드의 결과는 완료 큐로 전달한다.
- 블로킹 파일·네트워크 함수는 `async` 안에서 호출해도 그대로 블로킹한다. 실제 비동기 I/O 어댑터나 별도 작업 스레드 연결이 필요하다.

코루틴 스케줄러의 공통 부분과 플랫폼 이벤트 루프 어댑터를 분리한다. Android·iOS는 앱의 이벤트 루프, Linux·Windows는 선택한 I/O 이벤트 루프에 연결한다. 특정 네트워크 라이브러리나 libuv 채택은 아직 확정하지 않는다. 이 실행 모델은 JIT 활성화 여부에 의존하지 않도록 구현하고, iOS 인터프리터 모드에서도 검증한다.

## 구현 경계와 순서

```text
확장 소스 → lexer/parser → AST·타입 검사 → Lua 5.1 코드 → 공식 LuaJIT
                                      ↘ 원본 위치 매핑
```

Rust lexer/parser와 AST에서 문법과 규칙을 직접 구현한다. payload enum·match 완전성·trait 기본 구현·Task 효과도 이 검사기의 책임이다. Teal 소스나 AST를 재사용하지 않으며 키워드를 정규식으로 치환하지 않는다. 현재 생성기의 타입 제거는 파서가 확인한 annotation span에만 적용한다.

구현 순서는 [점진적 도입 계획](implementation-plan.md#다음-구현-단계)에 맞춘다.

1. 현재 Rust 코어의 기본 문법 범위를 넓히고 legacy/gradual/strict의 혼합 모듈 검사를 완성한다.
2. VS Code·Zed의 Rust 실행기 연결을 실제 검증하고, 줄이 달라지는 변환의 소스 매핑을 시험한다.
3. 사용자 정의 타입·기본 제네릭·일반 소스 맵을 추가하고 같은 검사기를 쓰는 최소 LSP를 연결한다.
4. `enum + match`, `Option/Result`를 구현한다. 잘못된 payload와 빠진 분기를 실행 전에 검출한다.
5. class의 생성·필드·메서드, trait의 필수·기본 메서드 및 충돌 검사를 구현한다.
6. 가짜 시계와 완료 큐로 검증할 수 있는 Task 스케줄러를 구현하고 `async/await` 문법과 원본 async 디버깅에 연결한다.
7. 목표 플랫폼의 I/O 어댑터·임베딩·배포와 기기 디버깅을 검증한다.

소스 변환으로 코드 줄이 늘어나므로 현재의 단순 줄 번호 보존만으로는 충분하지 않다. 타입 오류와 실행 오류, 중단점, async 스택을 원본 위치에 연결하는 매핑도 각 단계에 포함한다. 네트워크 없이 성공·실패·취소·이중 완료를 재현하는 테스트를 먼저 둔다.

참고: [Rust의 payload enum](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html), [trait과 기본 구현](https://doc.rust-lang.org/book/ch10-02-traits.html), [Lua 코루틴](https://www.lua.org/manual/5.1/manual.html#2.11), [LuaJIT 확장과 VM의 resume 제약](https://luajit.org/extensions.html).
