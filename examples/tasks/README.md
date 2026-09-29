# 실제 병렬 Task

```sh
make setup
./bin/arky run examples/tasks/main.arky
./bin/arky run examples/tasks/cancellation.arky
make task-test
```

`main.arky` 출력:

```text
Parallel task sum: 250015000
Input isolation: 1 / 10000
Task status: succeeded
```

`cancellation.arky` 출력:

```text
Cancelled: cancelled
Failure: failed
Pool still works: 42
```

정해진 OS 스레드가 작업 큐를 소비합니다. Task마다 새 OS 스레드를 만들지 않습니다. 실행되는 각 Lua 작업에는 독립 VM을 만들므로 VM 생성·입력/결과 복사 비용은 있습니다. 짧은 연산 하나마다 Task를 만들기보다 여러 연산을 묶어 제출하세요. JIT 설정은 공식 엔진의 기본값을 유지합니다.

`spawn`은 완료를 기다리지 않고 핸들을 반환합니다. `ready()`와 `status()`로 확인하고 `wait()`로 결과를 받습니다. `wait()`는 호출 스레드를 막습니다. `async/await`와 coroutine 스케줄러도 지원합니다. [async.arky](async.arky)는 같은 worker를 `scheduler.from_worker()`로 연결해 기다립니다. [사용 규칙](../../docs/async-await.md)을 참고하세요.

클로저 캡처는 거부합니다. 입력은 두 번째 인자로 전달하고, 함수는 `(input, context)`를 받습니다. `context.checkpoint()`에서 취소 요청을 검사하며 `context.cancelled()`로 직접 조회할 수도 있습니다. `cancel()`은 대기 작업을 취소하거나 실행 작업에 중단을 요청합니다. 체크포인트 없는 무한 루프나 블로킹 네이티브 호출을 강제로 중단하지 않습니다. `shutdown()`과 VM 종료는 작업 종료까지 기다립니다.

입력과 반환은 nil·boolean·유한 number·string·일반 table 트리 하나입니다. 결과가 여러 개면 첫 값만 전달하므로 필요한 값들을 table로 묶으세요. 객체·함수·FFI cdata·메타테이블·순환/공유 table 참조는 전송할 수 없습니다. 메인 VM의 전역·import·변경된 함수 환경은 워커에 전달되지 않습니다. 자세한 규칙과 C++ API는 [Task 런타임](../../docs/tasks.md)에 있습니다.

`task.lua`는 네이티브 모듈 경계, `task.d.arky`는 정적 import 계약입니다. worker 경계의 입력·결과·native 핸들에는 명시적 `any`를 사용합니다. `scheduler.from_worker()`로 변환한 핸들은 내장 `Task<any>` 타입입니다. 입력/결과는 네이티브 런타임이 검증합니다. 선언만으로 함수 본문·핸들의 정적 타입 안전성을 보장하지는 않습니다.

현재 typed 함수 안의 `try/except`·`import` 변환도 컴파일러 helper를 upvalue로 캡처하므로 그 함수는 Task에 직접 제출할 수 없습니다. 워커는 외부 캡처 없는 계산 함수로 작성하고, 실패 처리는 예제처럼 호출 측 `wait()` 주위의 `try/except`에서 합니다. 워커 내부에서 오류를 처리해야 한다면 현재는 표준 Lua `pcall`을 사용할 수 있습니다.
