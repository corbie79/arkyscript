# Tynex 예제

## 요청 기능별 확인표

| 요청 기능 | 예제/안내 | 현재 상태 |
| --- | --- | --- |
| 타입을 점진적으로 도입, legacy/gradual/strict | [type_modes/](type_modes/README.md) | 세 모드 실행 가능, strict 실패 확인 명령 포함 |
| 기본 타입·테이블·함수·다중 반환 | [main.luax](main.luax), [control_flow.luax](control_flow.luax), [multiple_values.luax](multiple_values.luax) | 실행 가능 |
| 정적 import·선언 전용 파일 | [imports/main.luax](imports/main.luax), [legacy.d.luax](imports/legacy.d.luax) | 실행 가능, import 재대입 진단 |
| try / except / else | [exceptions.luax](exceptions.luax) | 성공·실패 경로 실행 가능 |
| nil일 때만 접근·호출 (`?.`, `?[`, `?(`, `?:`) | [optional_chaining.luax](optional_chaining.luax) | 실행 가능: 인자 생략·self·다중 반환 유지 |
| 실제 스레드 병렬 Task | [tasks/](tasks/README.md), [cpp_tasks.cpp](cpp_tasks.cpp) | 실행 가능: 풀·격리·취소·결과; wait는 블로킹 |
| C++20 class·객체·동적 값 연결 | 아래 C++ 예제와 [Binder API](../binder/README.md) | 실행 가능 |
| 스크립트 class | [class.luax](class.luax), [안내](design/class.md) | 실행 가능: 필수 필드·기본값·메서드·명명 타입 |
| 스크립트 trait·기본 동작 조합 | [traits.luax](traits.luax), [안내](design/traits.md) | 실행 가능: 필수·기본 메서드·복수 trait·충돌 검사 |
| 값을 담는 enum·Result/Option·match | [설계 예제](design/enum-match.md) | 미구현 |
| async / await | [async.luax](async.luax), [tasks/async.luax](tasks/async.luax) | 실행 가능: coroutine·Task 타입·worker 연결·오류·취소·프레임 step |
| 비동기 파일·TCP I/O | [io/](io/README.md), [files.luax](io/files.luax), [tcp.luax](io/tcp.luax) | 실행 가능: 파일 동시 읽기·Socket·로컬 echo·EOF·연결 닫기 |
| VS Code·Zed 하이라이트·디버깅 | [debug/](debug/), [에디터](../docs/editors.md), [디버깅](../docs/debugging.md) | 현재 문법 지원; Task worker 디버깅은 미구현 |
| 공식 엔진 다운로드·업데이트, xwin | [업데이트/플랫폼 안내](../README.md#배포-플랫폼과-공식-소스), [Windows](../docs/windows-build.md) | 원본 수정 없이 빌드·검증 |
| 64비트 Android/iOS/Linux/Windows | [구현 계획](../docs/implementation-plan.md) | 모바일 앱 임베딩·기기 검증은 미완료 |

Rust는 독립 컴파일러 구현 언어입니다. 아래 설계 예제에 Rust 소유권·대여 규칙을 추가한다는 뜻은 아닙니다. **모든 요청 항목을 문서에 명시했지만 모든 언어 기능이 구현된 상태는 아닙니다.**

## C++20 바인딩 — 실행 가능

| 파일 | 내용 | 출력 |
| --- | --- | --- |
| [cpp_binder.cpp](cpp_binder.cpp) | 함수·클래스 등록, 동적 설정 값, C++에서 Lua 함수 호출 | `Hello, LuaJIT: 42` |
| [cpp_class.cpp](cpp_class.cpp) | 클래스 생성자 2종, 메서드, 읽기/쓰기·읽기 전용 프로퍼티, 인스턴스별 상태, 예외 처리 | `Mina Kim: 42` |
| [cpp_traits.cpp](cpp_traits.cpp) | C++20 concept으로 요구 동작 검사, 공통 기본 구현과 바인딩 재사용 | `name: Joon / name: device-7` |
| [cpp_tasks.cpp](cpp_tasks.cpp) | 고정 OS worker 풀에 계산 2개 제출, 결과 받기 | `Parallel task sum: 250015000` |

저장소 루트에서 실행합니다.

```sh
make setup
make binder-examples
```

예제는 Lua의 `assert`와 프로그램 종료 코드로 결과도 검사합니다. `make binder-test`·`make binder-sanitize`·`make binder-windows`에도 모두 포함됩니다. Windows 크로스 빌드 결과는 Windows에서 별도로 실행해야 합니다.

`cpp_traits.cpp`의 `Named`는 **C++20 concept**이고 `NamedBehavior<T>`는 공통 구현을 제공하는 C++ 템플릿입니다. 등록 도우미 `bind_named`가 두 클래스에 같은 메서드를 노출합니다. 이것이 Tynex 스크립트의 `trait` 키워드 구현을 의미하지는 않습니다. Lua의 `describe(value)` 함수는 동적으로 메서드를 호출하며 스크립트 trait 계약을 검사하지 않습니다.

바인딩 API와 소유권 규칙은 [Binder 안내](../binder/README.md)에 있습니다.

## typed Lua — 실행 가능

```sh
./bin/tynex run examples/main.luax
./bin/tynex run examples/control_flow.luax
./bin/tynex run examples/multiple_values.luax
./bin/tynex run examples/class.luax
./bin/tynex run examples/traits.luax
./bin/tynex run examples/optional_chaining.luax
./bin/tynex run examples/async.luax
./bin/tynex run examples/io/files.luax
./bin/tynex run examples/imports/main.luax
./bin/tynex run examples/exceptions.luax
./bin/tynex run examples/modules/main.luax
./bin/tynex run examples/tasks/main.luax
./bin/tynex run examples/tasks/cancellation.luax
./bin/tynex run examples/tasks/async.luax
```

`multiple_values.luax`는 현재 지원되는 Lua 테이블 메서드와 다중 반환을, `imports/`는 정적 import·선언 파일·try/except/else를 보여줍니다. 오류 진단 확인용 `type_error.luax`는 의도적으로 타입 검사가 실패합니다.

`class.luax`는 필수 필드·기본값·참조 공유를, `traits.luax`는 필수 메서드·기본 구현·여러 trait 조합·trait 타입 인자를 보여줍니다. 자세한 규칙은 [class·trait](../docs/classes-traits.md)에 있습니다.

`optional_chaining.luax`는 nil일 때 메서드·콜백 호출과 필드·사전 접근을 생략합니다. 출력은 `Mina 10`, `nil nil`, `done`, `42 nil`이며 값 사이는 탭입니다. [선택적 접근·호출](../docs/optional-chaining.md)에 평가·타입 규칙을 정리했습니다.

`io/files.luax`는 읽기 두 개를 시작한 뒤 결과를 받습니다. `io/tcp.luax`는 별도 로컬 echo 서버와 연결하므로 [I/O 예제 안내](io/README.md)의 서버 실행 명령을 먼저 사용하세요.

## 확장 문법 — 설계 예제

아래 문서는 아직 구현되지 않은 제안 문법이며 현재 CLI로 실행할 수 없습니다.

- [payload enum·match](design/enum-match.md): 경우별 데이터, Result/Option, 완전성 검사 제안

전체 규칙은 [언어 설계](../docs/language-design.md)에 있습니다.
