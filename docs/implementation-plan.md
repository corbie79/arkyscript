# 독립 Rust 컴파일러 구현 계획

상태: **두 에디터의 전용 구문 강조·최소 LSP, 일반 소스 맵 생성기·번들·DAP 중계를 구현했다. VS Code와 실제 EmmyLua 프로토콜에서 행 이동·모듈·조건부 중단점·오류·재연결 검증을 통과했다. Zed GUI에서도 전용 강조·LSP·mapped 디버거를 검증했다. 정적 import·named declare·try/except/else와 class·trait도 구현했다.** 전체 Lua 정적 지원에는 아직 제한이 있다. 원본 LuaJIT은 그대로 유지한다. 현재 사용 가능한 문법·명령은 [README](../README.md)를 기준으로 하고 class·trait 규칙과 enum·async 설계는 [언어 설계](language-design.md)를 따른다.

## 확정한 구조

```text
소스 → Rust lexer/parser → AST → 타입 검사 → Lua 코드 + 원본 위치 → 공식 LuaJIT
```

컴파일러 코어는 독립 Rust 라이브러리, CLI는 그 라이브러리를 사용하는 도구다. LSP도 같은 코어를 사용하고, 향후 C ABI로 모바일·게임 엔진에 연결한다. 코어에서 프로세스나 플랫폼 SDK를 요구하지 않는다. CLI·빌드 도구·VM·앱 이벤트 루프는 별도 구성 요소로 유지한다.

Rust는 공통 소스·Cargo 테스트·타입이 있는 내부 AST에 적합하다. 각 플랫폼의 링커·SDK·LuaJIT·디버거 코어를 대신하지는 않는다. 작은 기능 단위 테스트와 진단·실행 예제를 AI 보조 개발의 검증 기준으로 사용한다. 모델이 작성한 코드도 같은 완료 조건을 적용한다.

## 현재 구현한 범위

- Cargo workspace, 고정 Rust toolchain·lockfile, 외부 의존성 없는 compiler 코어.
- `.lua` legacy 원본 실행과 구문 검사. `.tlj` 기본 타입·지역 변수·local function·인자/반환형·연산 검사 및 Lua 생성.
- 다중 선언/대입/반환, 빈 반환과 nil 반환 구분, 마지막 표현식 확장과 괄호에 의한 축소, typed 가변 인수와 가변 반환 목록, select/unpack 초기 타입.
- 일반/테이블 함수 선언·메서드 선언과 호출, self 필드·인자 검사, 정적 경로의 재귀 함수와 기본 문자열 메서드.
- 익명 함수·함수 값, 레코드·배열·사전의 생성/읽기/대입, if/elseif/else·do·while·repeat·숫자 for·pairs/ipairs·break.
- 지역 변수 nil 좁히기·조기 반환·분기별 반환형 추론·반환 누락 검사, 가변 테이블 별칭의 타입 계약, 일부 math/string 함수 시그니처.
- class·trait의 명명 타입, 필드 초기화·인스턴스별 기본값, self 메서드, 필수·기본 메서드·충돌 검사, 모듈 공개 계약. [구현 규칙](classes-traits.md).
- legacy/gradual/strict, CLI·파일 지시문·프로젝트 TOML·경로별 override. 의존 모듈도 각자의 모드로 검사하며 CLI 모드는 진입 파일에만 적용.
- 정적 require 그래프·공개 타입 전달·문자열/주석/가려진 require 구분·순환/모호성 진단, module_paths·dynamic_modules·external_modules.
- `.d.tlj` 레거시 export 계약과 함수 타입 표기, include/exclude 기반 프로젝트 전체 check, 모듈별 JSON 진단·검사 목록.
- 준비된 모듈 로딩·원본 위치 보존, package.loaded 캐시와 preload 충돌 검사. legacy 구현은 원본 로딩과 구문 검사 유지.
- 원본 위치 진단, check JSON 출력, 타입 제거 시 행 보존, 공식 LuaJIT 실행·디버거 연결 bridge.
- Rust 단위/CLI 테스트와 실제 LuaJIT 실행 테스트. 공통 Lua 문법 27개 사례의 직접 실행 비교, 반복문·메서드의 디버그 훅 검사. VS Code 확장 호스트·Zed UI에서 원본 중단점·변수·식 평가·Step Over·정상 종료 확인. 검증 범위는 [에디터 기록](editor-validation.md)에 구분.

- `tynex lsp`: 미저장 문서·모듈·선언 파일, UTF-16 진단, 버전/닫기, 파일 감시, legacy 구문 검사. VS Code 로컬 VSIX·TextMate 문법과 Zed 전용 Tree-sitter grammar·WASM 확장 제공.
- UTF-16 소스 맵 emitter, v3 sidecar·원본/생성 위치 DAP 변환, 독립 bundle/exec-bundle, 긴 파일명을 포함한 원본 오류 traceback.

현재 구현은 전체 Lua 정적 검사기가 아니다. 사용자 iterator·조건부 새 멤버와 별칭 전파·순환 모듈의 타입 해석·타입 별칭·제네릭은 추가해야 한다. legacy 본문 내부 의존 분석과 실행 중 typed 컴파일도 하지 않는다. 기존 `.lua` 프로젝트는 legacy 경로를 유지하며 typed 동적 의존은 미리 등록한다. 미구현 문법을 any로 덮어 통과시키지 않는다.

## 타입 정책

| 모드 | 타입 정보 부족 | 확인 가능한 불일치 | 현재 기본 대상 |
| --- | --- | --- | --- |
| legacy | 검사하지 않음 | 타입 오류로 차단하지 않음 | `.lua` |
| gradual | 허용 | 오류 | `.tlj` |
| strict | 추론 가능하면 허용, 미해결이면 오류 | 오류 | 준비된 파일에서 선택 |

strict에서도 `local n = 1` 같은 코드에 표기를 강제하지 않는다. explicit any는 의도적인 동적 경계이며 안전성 보장에 포함하지 않는다. 현재 nil을 명시적으로 구분하고 `T?`로 부재를 허용한다. 테이블 타입과 지역 변수의 흐름에 따른 nil 좁히기를 구현했다. 필드 경로의 좁히기·사용자 정의 복합 타입·Option 규칙은 이후 구현한다. 현재의 보수적인 분석 범위는 [지원 문법](supported-language.md)에 기록한다.

설정은 `tynex.toml`, 파일 지시문은 `--!typecheck strict` 등이다. 우선순위는 진입 파일의 CLI > 파일 지시문 > 마지막 override > 프로젝트 기본값 > 확장자 기본값. 파일마다 가장 가까운 설정을 찾고 `--project`로 전체 그래프의 설정을 고정한다. 모듈 검색 루트는 진입 프로젝트 기준이다. 런처와 LSP는 같은 정책 코드를 사용한다.

`.lua`에 타입 표기를 넣으면 Lua 호환성이 깨지므로 typed 문법은 `.tlj`에 둔다. `.tl`은 지원하지 않는다. 기존 Teal record/generic 문법을 파일명만 바꿔 지원한다고 가정하지 않는다. legacy는 `.lua`에만 적용한다.

## 다음 구현 단계

| 단계 | 작업 | 완료 조건 |
| --- | --- | --- |
| 1. 기본 언어 범위 확장 — 핵심 범위 구현 | 테이블·함수 값·조건문·반복문·nil 좁히기·가변 인수·다중 값·메서드·일반 함수 선언·기본 API 시그니처 | 현재 지원 범위의 LuaJIT 실행 비교·미지원 진단·gradual/strict 회귀 통과. 전체 Lua 호환성과 라이브러리 타입은 계속 보강 |
| 2. 혼합 모듈과 프로젝트 검사 — 핵심 범위 구현 | 모듈 검색·추론한 공개 타입·레거시 `.d.tlj`·혼합 로딩·순환 진단·명시적 동적 루트·프로젝트 전체 check | 파일별 정책·진입 전 오류 차단·런타임 캐시·원본 모듈 디버그 훅 회귀 통과. 순환 해석·런타임 컴파일·레거시 내부 분석은 후속 |
| 3. 두 에디터와 원본 매핑 — 구현·프로토콜 검증 | 전용 강조·LSP·일반 emitter·v3/DAP 매핑·모듈 Step Into/Over·조건부 중단점·오류·재연결 구현 및 검사 | VS Code 확장 호스트와 네이티브 EmmyLua에서 생성 행 +2 검증. Zed 전용 확장 GUI의 강조·LSP·원본 위치·조건부 중단점·오류 종료 확인 |
| 3a. 모듈·예외 문법 — 구현 | 전용 로더의 정적 import·불변 바인딩·named declare·try/except/else | 캐시 교체 내성·레거시 호환·중첩 return/break·다중 값·LSP·강조·원본 디버깅 회귀 |
| 4. payload enum·match | 기본 제네릭, Option/Result, payload 추론, 분기 누락·중복 검사 | 서로 다른 enum 구분, 잘못된 payload 차단, 분기 내부 원본 디버깅 |
| 5. class·trait — 구현 | 생성자·필드·메서드, 필수/기본 메서드, 충돌 검사 | 초기화와 호출 검사, 인스턴스별 가변 기본값 분리, self 조회 |
| 6. async·await | Task 상태·코루틴 스케줄러·오류·협력적 취소·원본 await 위치 | 가짜 시계로 완료/실패/취소/이중 완료 검증, await 전후 정지·변수·오류 스택 |
| 7. 플랫폼 임베딩·배포 | Rust C ABI, Android/iOS 앱 연결, Linux/Windows 패키징, 호스트 이벤트 루프 | 실제 기기 실행·모듈 로딩·취소·디버깅; iOS는 JIT 없이 검증 |

2단계의 검색 순서·중복 이름·preload/cache 경계는 [모듈 안내](modules.md)에 명세했다. 정적 import·리터럴 require와 등록한 dynamic_modules만 실행 전에 준비한다. 아직 준비되지 않은 typed 모듈은 실행 중 자동 컴파일하지 않으며, 이 오류 전에 생긴 부수 효과는 되돌리지 않는다. legacy 내부 require는 원래 로더에 맡기고 `.d.tlj`는 구현과 대조하지 않는 신뢰 계약이다.

캐시는 소스 내용·적용 정책·의존 공개 타입·컴파일러/생성기 버전을 키에 포함한다. 첫 버전에서는 증분 최적화보다 정확성을 우선한다.

## 디버깅 완료 조건

현재 단순 타입 제거의 줄 보존과 일반 소스 맵을 구분한다. AST 원본 범위를 모든 생성 코드에 연결하고, 임시 변수·helper를 표시할 메타데이터를 함께 보존한다. 원본 파일명만 바꿔 줄이 다른 코드를 원본처럼 표시하지 않는다.

VS Code는 Local Lua Debugger의 v3 source map과 직접 전달한 scriptFiles를 사용한다. Zed는 프로젝트 소유 DAP 중계에서 중단점·스택·source 위치를 변환한다. 단계 실행·Watch·조건부 중단점은 위치 변환만으로 해결되지 않는다. [Local Lua Debugger](https://github.com/tomblind/local-lua-debugger-vscode), [EmmyLua DAP](https://github.com/EmmyLuaLs/emmylua_dap).

에디터 UI 테스트와 Lua 디버그 API 테스트를 구분해서 기록한다. 중단점·Step Into/Over·지역 변수·Watch·Call Stack·모듈 오류·정상/오류 종료·재시작을 확인한다. Windows 경로·유니코드·네이티브 코어도 해당 호스트에서 검증한다. LSP는 컴파일러 진단을 재사용하며 별도 타입 규칙을 만들지 않는다.

async 단계에서는 Lua 코루틴 스택과 await 관계를 분리한다. Task ID·생성/대기/완료 위치를 추적하고 기존 어댑터에서 표시 가능한 범위와 추가 중계 범위를 나눈다. 예상하지 못한 오류를 원본 await 지점으로 전파한다. 현재 어댑터가 async 스택을 자동 제공한다고 가정하지 않는다.

## 플랫폼과 엔진 유지

공식 LuaJIT Git 객체를 받아 별도 빌드 스냅샷을 만드는 구조는 유지한다. 업데이트 후보는 Rust 컴파일러·LuaJIT 실행 회귀도 통과해야 채택한다. 배포 전 변환은 Lua 소스 생성이며 네이티브 AOT 컴파일이 아니다.

데스크톱 Rust CLI와 모바일용 Rust 라이브러리는 별도 산출물이다. 모바일 앱에서 외부 프로세스로 CLI를 실행하는 설계는 사용하지 않는다. Android는 NDK, iOS는 Apple SDK·서명·앱 임베딩과 JIT 비활성화를 별도로 다룬다. 빌드 대상 검사만으로 기기 지원 완료를 선언하지 않는다.

실제 OS worker 풀과 Lua Task의 제출·상태·wait·협력적 취소는 [Task 런타임](tasks.md)에 구현했다. 입력/결과는 복사하며 실행 작업마다 독립 VM을 사용한다. `wait`는 블로킹 API이고 async/await 구현을 의미하지 않는다.

후속 coroutine 스케줄러는 Lua 상태의 소유 스레드에서 동작한다. 작업 스레드의 완료를 메인 VM의 resume 큐에 연결하고, async 안의 블로킹 함수가 자동 비동기가 되지 않음을 유지한다. 첫 I/O 어댑터는 타이머와 호스트 완료 콜백으로 검증한 뒤 확장한다.
