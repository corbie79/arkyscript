# Tynex 예제

## C++20 바인딩 — 실행 가능

| 파일 | 내용 | 출력 |
| --- | --- | --- |
| [cpp_binder.cpp](cpp_binder.cpp) | 함수·클래스 등록, 동적 설정 값, C++에서 Lua 함수 호출 | `Hello, LuaJIT: 42` |
| [cpp_class.cpp](cpp_class.cpp) | 클래스 생성자 2종, 메서드, 읽기/쓰기·읽기 전용 프로퍼티, 인스턴스별 상태, 예외 처리 | `Mina Kim: 42` |
| [cpp_traits.cpp](cpp_traits.cpp) | C++20 concept으로 요구 동작 검사, 공통 기본 구현과 바인딩 재사용 | `name: Joon / name: device-7` |

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
./bin/tynex run examples/modules/main.tlj
```

`multiple_values.tlj`는 현재 지원되는 Lua 테이블 메서드와 다중 반환을, `imports/`는 정적 import·선언 파일·try/except/else를 보여줍니다. 오류 진단 확인용 `type_error.tlj`는 의도적으로 타입 검사가 실패합니다.

## class·trait 문법 — 설계 예제

**스크립트의 `class`·`trait` 키워드와 명명 타입은 아직 구현되지 않았습니다.** 아래 문서는 제안 문법을 보여주며 현재 CLI로 실행할 수 없습니다. 실행 예제와 구별되도록 Markdown 파일로 제공합니다.

- [class](design/class.md): 필수 필드, 기본값, 생성, 메서드, 인스턴스별 상태
- [trait](design/traits.md): 필수 메서드, 기본 구현, 여러 trait 조합, trait 타입 인자

구현 예정인 payload enum·match·async/await는 [언어 설계](../docs/language-design.md)에 있습니다.
