# Tynex 예제

## 요청 기능별 확인표

| 요청 기능 | 예제/안내 | 현재 상태 |
| --- | --- | --- |
| 타입을 점진적으로 도입, legacy/gradual/strict | [type_modes/](type_modes/README.md) | 세 모드 실행 가능, strict 실패 확인 명령 포함 |
| 기본 타입·테이블·함수·다중 반환 | [main.tlj](main.tlj), [control_flow.tlj](control_flow.tlj), [multiple_values.tlj](multiple_values.tlj) | 실행 가능 |
| 정적 import·선언 전용 파일 | [imports/main.tlj](imports/main.tlj), [legacy.d.tlj](imports/legacy.d.tlj) | 실행 가능, import 재대입 진단 |
| try / except / else | [exceptions.tlj](exceptions.tlj) | 성공·실패 경로 실행 가능 |
| 실제 스레드 병렬 Task | [tasks/](tasks/README.md), [cpp_tasks.cpp](cpp_tasks.cpp) | 실행 가능: 풀·격리·취소·결과; wait는 블로킹 |
| C++20 class·객체·동적 값 연결 | 아래 C++ 예제와 [Binder API](../binder/README.md) | 실행 가능 |
| 스크립트 class | [설계 예제](design/class.md) | 미구현 |
| 스크립트 trait·기본 동작 조합 | [설계 예제](design/traits.md) | 미구현; C++ concept 예제는 실행 가능 |
| 값을 담는 enum·Result/Option·match | [설계 예제](design/enum-match.md) | 미구현 |
| async / await | [설계 예제](design/async-await.md) | 미구현; 네이티브 Task API와 구분 |
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
./bin/tynex run examples/main.tlj
./bin/tynex run examples/control_flow.tlj
./bin/tynex run examples/multiple_values.tlj
./bin/tynex run examples/imports/main.tlj
./bin/tynex run examples/exceptions.tlj
./bin/tynex run examples/modules/main.tlj
./bin/tynex run examples/tasks/main.tlj
./bin/tynex run examples/tasks/cancellation.tlj
```

`multiple_values.tlj`는 현재 지원되는 Lua 테이블 메서드와 다중 반환을, `imports/`는 정적 import·선언 파일·try/except/else를 보여줍니다. 오류 진단 확인용 `type_error.tlj`는 의도적으로 타입 검사가 실패합니다.

## 확장 문법 — 설계 예제

**스크립트의 `class`·`trait` 키워드와 명명 타입은 아직 구현되지 않았습니다.** 아래 문서는 제안 문법을 보여주며 현재 CLI로 실행할 수 없습니다. 실행 예제와 구별되도록 Markdown 파일로 제공합니다.

- [class](design/class.md): 필수 필드, 기본값, 생성, 메서드, 인스턴스별 상태
- [trait](design/traits.md): 필수 메서드, 기본 구현, 여러 trait 조합, trait 타입 인자
- [payload enum·match](design/enum-match.md): 경우별 데이터, Result/Option, 완전성 검사 제안
- [async/await](design/async-await.md): 실제 worker Task를 기다리는 비동기 문법 제안

전체 규칙은 [언어 설계](../docs/language-design.md)에 있습니다.
