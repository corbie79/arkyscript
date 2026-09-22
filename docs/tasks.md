# Task 런타임

상태: **C++20 스레드 풀과 Lua/.tlj용 네이티브 Task API는 실행 가능**하다. `async/await`, `Task<T>` 문법, coroutine/event-loop 스케줄러, 비동기 I/O API는 아직 구현 전이다. [실행 예제](../examples/tasks/README.md), [C++ 예제](../examples/cpp_tasks.cpp), [async 문법 제안](../examples/design/async-await.md)을 구분한다.

## 실행 모델

```text
메인 Lua VM: task.spawn(fn, input) → 작업 큐 → 고정 OS worker 1 → 독립 Lua VM
                                         → 고정 OS worker 2 → 독립 Lua VM
           handle:ready()/wait() ← 복사한 결과·실패 상태 ←─────┘
```

기본 worker 수는 하드웨어 동시 실행 수를 1~4로 제한한다. `configure`로 1~64개를 지정할 수 있다. 큐에는 기본 최대 4096개 작업이 대기하며, 실행 중인 작업은 이 수에 포함하지 않는다. 큐가 가득 차면 `spawn`은 오류를 낸다. 취소된 대기 작업은 다음 제출 때 제거하여 큐 용량을 돌려준다. 한 메인 VM이 모듈을 로드하면 풀 하나를 소유하며 첫 작업 제출 시 스레드를 만든다.

Task마다 OS 스레드를 만들지 않는다. 대기 Task는 함수 바이트코드·복사한 입력·결과 상태를 보관한다. 작업이 실행되면 해당 worker에서 VM을 만들고, 완료 후 닫는다. 따라서 Task는 스레드보다 가벼운 작업 단위지만 VM 생성 비용과 데이터 복사 비용이 있다. 현재는 worker별 VM 재사용이나 fiber의 실행 중 이동을 제공하지 않는다. 풀 크기는 CPU 개수·다른 호스트 스레드 수를 고려해 정하고 작은 연산은 묶어서 제출한다.

같은 Lua VM을 여러 스레드에서 동시에 실행하지 않는다. `lua_newthread` coroutine은 VM 메모리를 공유하므로 worker용으로 쓰지 않고 `luaL_newstate`를 사용한다. 공식 LuaJIT 원본을 변경하지 않으며 JIT도 강제로 끄지 않는다. iOS 등 JIT을 사용할 수 없는 빌드에서도 이 설계 자체는 인터프리터로 동작할 수 있으나, 모바일 빌드·기기 검증은 아직 하지 않았다.

이는 V8의 독립 Isolate를 Node.js/Deno Worker에 연결하는 구조와 유사하다. 엔진 내부의 컴파일·GC 보조 스레드와 사용자 코드를 병렬 실행하는 worker는 역할이 다르다. 참고: [Node.js의 Isolate/Worker 구조](https://github.com/nodejs/node/blob/main/src/README.md), [Deno Worker](https://docs.deno.com/api/web/workers/), [Lua의 coroutine](https://www.lua.org/manual/5.1/manual.html#2.11).

## Lua API

| API | 동작 |
| --- | --- |
| `task.configure(workers, max_pending)` | 첫 제출 전 풀 크기·대기 작업 수 설정 |
| `task.spawn(function(input, context) ... end, input)` | 작업 제출 후 핸들 즉시 반환; input 생략 시 nil |
| `handle:ready()` | 성공·실패·취소 완료 시 true |
| `handle:status()` | queued / running / cancelling / succeeded / failed / cancelled |
| `handle:wait()` | 호출 스레드를 막고 결과를 복사해 반환; 실패·취소는 Lua 오류 |
| `handle:cancel()` | 최초 취소 요청이 받아들여지면 true; 완료·기존 요청이면 false |
| `context.checkpoint()` | 현재 작업에 취소 요청이 있으면 Lua 오류 발생 |
| `context.cancelled()` | 취소 요청 여부 조회 |
| `task.shutdown()` | 대기 작업 취소·실행 작업에 취소 요청·worker 종료까지 대기 |

`wait()`는 여러 번 호출해도 같은 결과의 **새 복사본**을 받는다. `shutdown()`은 반복 호출할 수 있으며 이후 새 작업은 거부한다. 완료된 결과 핸들은 shutdown 뒤에도 읽을 수 있다. 모듈의 소유 VM이 닫힐 때도 풀을 정리한다. 핸들만 GC되었다고 작업이 취소되지는 않는다. 메인 스레드에서 Lua 콜백을 자동 호출하는 완료 큐와 `await` resume은 후속 단계다.

취소는 협력 방식이다. 실행 중인 작업은 `cancelling`에서 실제 종료한 뒤 `cancelled`가 된다. 취소 요청 후 함수가 정상 값을 반환해도 그 결과를 버리고 취소 완료로 처리한다. 긴 루프에 적절한 간격으로 체크포인트를 둔다. 블로킹 C 함수·FFI 호출·체크포인트 없는 무한 루프를 강제 종료하지 않으므로 shutdown도 기다릴 수 있다. worker에서 다른 풀을 중첩 생성하는 사용은 지원하지 않는다.

## VM 경계와 제한

함수는 현재 프로세스의 같은 엔진 버전에서 바이트코드로 옮긴다. C 함수와 **상위 스코프를 캡처하는 모든 upvalue**는 거부한다. 입력은 함수의 첫 인자로 전달한다. worker VM에는 표준 라이브러리가 열려 있지만 메인의 전역 변수, import 캐시, 변경한 함수 환경, 등록한 C++ 객체는 전달되지 않는다. 전역을 캡처 대신 참조하면 worker의 독립 전역을 보게 된다. worker 안의 모듈 검색은 새 VM의 기본 검색 환경을 사용하며, 메인 프로젝트의 검사된 import 그래프를 재사용하지 않는다.

`input`과 반환은 nil·boolean·유한 number·string·plain table 트리 하나다. table 키는 문자열·boolean·정확히 표현 가능한 정수로 제한한다. 반환값이 여럿이면 첫 값만 전달한다. 객체·함수·userdata·coroutine·FFI cdata·메타테이블·순환 참조·공유 하위 table은 거부한다. 클래스 인스턴스를 옮길 때는 필요한 필드를 일반 table로 만들어 전달한다. 데이터 깊이 64, 노드 100000, 추정 값 크기 8 MiB, 바이트코드 8 MiB 제한을 적용한다. 이 제한은 실행 중 VM의 메모리 상한을 의미하지 않는다.

이 VM 분리는 동시 접근을 방지하는 실행 구조이며 **보안 샌드박스가 아니다**. 표준 라이브러리와 FFI가 있으므로 신뢰하는 작업 코드를 실행한다. 런타임의 전송 검사와 `.d.tlj`의 선언 계약은 서로 다른 역할이다. 아직 제네릭·사용자 정의 타입이 없어 예제 계약은 값과 핸들에 명시적 `any`를 사용한다.

메인 VM의 기존 디버깅은 유지된다. worker 내부 VM의 중단점·스텝 실행·논리 async 스택은 아직 지원하지 않는다. 오류 문자열에는 Lua가 제공한 원본 정보가 남을 수 있지만 worker에 대한 완전한 소스 맵 디버깅을 보장하지 않는다.

## C++ API와 빌드

`runtime/task/task_pool.h`는 Lua에 의존하지 않는 C++20 헤더다. `task_pool::submit`에는 `next2::luax::cancellation_token`을 받는 함수를 전달한다. `task<R>::get()`은 값 또는 void를 반환하며 실패 시 예외를 다시 던진다. 결과는 복사 가능한 값이어야 하며, 호출 객체는 move-only여도 된다. `submit`·핸들의 상태/취소/get은 여러 호출 스레드에서 사용할 수 있다. 풀 생성·shutdown·파괴는 소유자가 외부에서 직렬화한다. worker가 자기 풀의 미완료 Task를 기다리거나 shutdown하면 교착을 막기 위해 오류를 낸다. C++ 작업이 Lua 핸들을 캡처해 다른 스레드에서 사용해도 된다는 뜻은 아니다.

```sh
make setup                 # 공식 엔진 + Task 공유 모듈 + Rust 도구
make task                  # 기존 엔진 옆에 Task 공유 모듈만 빌드
make task-test             # C++ 동시성·VM 수명 + Lua/typed 예제 검증
make binder-sanitize       # 기존 바인더와 Task ASan/UBSan
make windows               # xwin 엔진 + Task DLL
make binder-windows        # Windows 테스트·예제 EXE 크로스 빌드
```

`tynex_task.so`/`tynex_task.dll`을 사용하는 LuaJIT 실행 파일 옆에 둔다. Tynex 실행기는 그 경로를 `package.cpath`에 추가한다. 순수 LuaJIT 실행은 `LUA_CPATH`나 `package.cpath`로 모듈 경로를 설정한다. 네이티브 모듈은 **바로 그 LuaJIT 빌드**에 맞춰 빌드하며, Windows DLL은 `lua51.dll`과 함께 배포한다. `make windows`가 만드는 엔진 ZIP과 별도로 Task를 포함하는 `tynex-windows-x64-<SHA>.zip`을 제공한다.

공식 엔진 업데이트 검증에는 Task 네이티브 빌드·Windows 크로스 빌드·C++ 테스트·Lua 실행 검증을 포함한다. 실제 Windows 실행은 CI의 별도 Windows job에서 수행한다. 현재 Task 대상은 macOS 개발 호스트, Linux와 Windows x64 빌드/CI이며 Android·iOS의 임베딩/패키징 검증은 후속 작업이다.

현재 typed 함수 안의 `try/except`·`import` 변환도 컴파일러 helper를 upvalue로 캡처하므로 그 함수는 Task에 직접 제출할 수 없습니다. 워커는 외부 캡처 없는 계산 함수로 작성하고, 실패 처리는 예제처럼 호출 측 `wait()` 주위의 `try/except`에서 합니다. 워커 내부에서 오류를 처리해야 한다면 현재는 표준 Lua `pcall`을 사용할 수 있습니다.
