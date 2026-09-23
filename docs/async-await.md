# async / await

`.luax`의 `async function`, `await`, 결과 타입 `Task<T>`를 지원한다. async 함수 호출은 메인 Lua VM의 coroutine 작업을 큐에 넣고 Task를 즉시 반환한다. **async 함수마다 OS 스레드를 만들지 않는다.** 실제 병렬 계산은 기존 [worker Task](tasks.md)로 보낸다. 공식 LuaJIT은 수정하지 않는다.

```lua
local async function double(n: number): number
   return n * 2
end
local async function main(): number
   local first = double(21)
   local second = double(15)
   return (await first) + (await second)
end
print(scheduler.run(main())) -- 72
```

CLI는 `scheduler.run()`으로 결과를 기다린다. 게임·UI에서는 호스트가 매 프레임 `scheduler.step()`을 호출한다. CLI가 남은 작업을 자동으로 끝까지 실행하지는 않는다.

## 문법과 타입

- `local async function f(...)`, `async function module:method(...)`, `async function(...)` 함수 값을 지원한다. class·trait 기본 메서드에도 `async function`을 쓴다.
- `async function f(): number`의 본문은 number를 반환하고 `f()`의 타입은 `Task<number>`다. 함수 값의 계약은 `async function(number): number`로 표기한다.
- `Task<number, string>`은 두 반환값, `Task<>`는 반환값 없음, `Task<number, ...string>`은 가변 반환이다. Task는 내장 타입이며 일반 사용자 제네릭 문법은 아직 없다.
- async 반환형의 고정 위치는 실제로 반환해야 한다. `: number?`는 `return nil`로 한 값을 반환하며 빈 return·암묵적인 fallthrough는 허용하지 않는다. 반환형을 생략하면 실제 반환 개수와 타입을 추론한다.
- await는 async 본문에서만 허용한다. 중첩된 일반 함수와 클래스 필드 기본값에서는 직접 사용할 수 없다. 기본값으로 async 함수 값을 만드는 것은 가능하다.
- await는 한 단계만 풀어준다. `Task<Task<number>>`를 한 번 기다리면 `Task<number>`다.
- Lua 다중 반환·가변 인수·끝의 nil을 유지한다. 마지막 표현식의 await는 다중 값으로 확장하고 `(await task)`는 한 값으로 줄어든다. `await task`만 문장으로 써서 결과를 버릴 수도 있다.
- `await f() + 1`은 `(await f()) + 1`이다. nullable Task는 nil을 먼저 확인해야 한다. 명시적 any 경계도 실행 시 managed Task인지 검사한다.
- await 뒤에는 다른 작업이 값을 변경했을 수 있으므로 캡처 변수의 좁힌 타입을 되돌린다. imported async 함수와 Task 결과도 같은 검사기로 확인한다.

## 스케줄러와 핸들

| API | 동작 |
| --- | --- |
| `scheduler.step(max_resumes?)` | 최대 지정 횟수만큼 coroutine 재개. 기본 64. 남은 작업이 있으면 true |
| `scheduler.run(task)` | 해당 Task가 완료될 때까지 진행하고 결과 반환 |
| `scheduler.pending()` | 미완료 managed Task 수. worker·I/O 변환 핸들도 포함 |
| `scheduler.from_worker(handle)` | 네이티브 worker 핸들을 `Task<any>`로 연결 |
| `task:ready()` | 성공·실패·취소가 확정되었으면 true |
| `task:status()` | queued / running / waiting / succeeded / failed / cancelled |
| `task:result()` | 완료 결과 반환. 미완료·실패·취소는 오류 |
| `task:cancel()` | 최초 취소면 true. 이미 완료·취소되었으면 false |

step은 native 작업 완료를 기다리며 블로킹하지 않는다. 현재 구현은 native 핸들의 완료 상태를 확인하고 준비된 coroutine을 FIFO 큐에서 재개한다. run은 실행 가능한 coroutine이 없고 native 작업만 남으면 짧게 sleep하며 확인한다. OS 알림 기반 I/O 이벤트 루프는 아니다.

**재개 횟수는 프레임 실행 시간의 상한이 아니다.** async 본문의 긴 계산이나 블로킹 함수는 메인 스레드를 멈춘다. 완료된 Task의 await도 즉시 반환할 수 있다. CPU 작업은 worker 풀에 맡긴다. run·step을 async 안에서 다시 호출하는 것은 정적 검사와 런타임에서 거절한다.

같은 VM의 async 함수끼리는 테이블·클래스·캡처 변수를 공유한다. 같은 Task를 여러 번 기다리면 같은 결과 객체를 받는다. native worker 경계에서는 기존처럼 입력·결과를 복사하며 userdata나 C++ 객체를 전달하지 않는다.

## 오류와 취소

실패한 Task의 await·result·run은 원래 Lua 오류 값을 전달한다. 문자열뿐 아니라 table·false·nil도 유지하며 `try/except/else`로 처리할 수 있다. 실패를 관찰하려면 Task를 기다리거나 result를 읽는다. step 자체가 모든 Task의 실패를 던지지는 않는다.

취소한 coroutine은 대기 목록에서 빠지고 다시 재개하지 않는다. 기다리던 작업을 취소해도 공유 의존 작업은 취소하지 않는다. worker 변환 핸들의 명시적 취소는 네이티브 핸들에도 취소를 요청한다. 변환 핸들은 즉시 cancelled가 되지만 실제 worker 종료는 기존 checkpoint 기반 협력 규칙을 따른다. 실행 중인 계산을 강제 선점하지 않는다. 순환 await와 실행 가능한 작업이 없는 교착은 오류로 처리한다.

## 호스트 루프 연결

호스트는 Lua 상태를 소유한 스레드에서 scheduler를 초기화하고 typed 코드와 같은 인스턴스를 사용해야 한다. 순수 Lua 임베딩의 최소 예다.

```lua
local runtime = assert(loadfile("runtime/async.lua"))()
local environment = setmetatable({
   __tynex_async_runtime = runtime, scheduler = runtime,
   fs = runtime.fs, net = runtime.net
}, {__index = _G})
local chunk = assert(loadfile("generated.lua"))
setfenv(chunk, environment)
chunk() -- async 진입 함수를 호출해 큐에 등록하는 코드
-- 호스트의 매 프레임 콜백에서 같은 Lua 스레드로 호출:
runtime.step(64)
```

`tynex run/debug/bundle`은 런타임 연결을 자동으로 준비한다. gen만 사용하는 호스트는 직접 주입해야 하며 static import가 있으면 전용 로더도 필요하다. worker를 쓸 때는 초기화 전에 해당 엔진의 tynex_task 모듈 경로를 설정한다. 실제 Cocos Director/Scheduler 연결·앱 종료 처리는 호스트 통합 작업으로 남아 있다.

## 예제와 도구

```sh
./bin/tynex run examples/async.luax
./bin/tynex run examples/tasks/async.luax
python3 -m unittest discover -s tests -p 'test_async*.py' -v
```

VS Code·Zed 구문 강조, Task/await 진단, 원본 행·소스 맵을 지원한다. 런타임은 실패한 coroutine traceback을 보존하고 CLI 번들에서 원본 경로·행으로 매핑한다. 디버거의 전체 async 논리 스택·스케줄러 내부 프레임 숨김과 새 문법의 실제 GUI 세션은 후속 검증 대상이다.

파일 읽기·DNS·TCP 연결/송수신도 Task로 기다릴 수 있다. [I/O API](io.md)와 [실행 예제](../examples/io/README.md)를 따른다. HTTP/TLS, 타이머·호스트 완료 콜백용 Task 생성 API, 모바일 임베딩·기기 검증은 아직 없다.
