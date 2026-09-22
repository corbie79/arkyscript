# 개발 도구 검증

macOS ARM64와 고정 LuaJIT `c6ffc141a8762b41703f9287d63d93622a13dd8f`에서 검증했다. 아래 실행 결과는 해당 환경의 결과이며 모바일·다른 데스크톱의 실제 에디터 세션을 포함하지 않는다.

| 대상 | 검증 범위 | 결과 |
| --- | --- | --- |
| Rust 컴파일러·CLI | 타입 검사, 모듈, 설정, LSP, 소스 맵 | 156개 테스트 통과 |
| Python 통합 테스트 | 공식 LuaJIT 실행, 선언·예외·class·trait·선택적 호출, LSP·DAP, 빌드 도구 | 113개 테스트 통과 |
| C++20 바인딩 | 인자, 클래스, 객체 수명, 동적 값, 오류 처리 | 1,321개 검사 및 ASan/UBSan 통과 |
| Windows x64 C++ | xwin·clang-cl 컴파일, 정적 링크, PE 검사 | 로컬 크로스 빌드 통과 |
| TextMate | 타입·함수·주석·문자열·불완전 문법·선택적 호출 | 15개 fixture 통과 |
| Tree-sitter | 파싱, 모든 Zed query, capture, 오류 회복 | 24개 예제·2개 전용 fixture 통과 |
| VS Code | 실제 extension host, VSIX 패키징 | 통과 |
| Zed | 확장 Rust 테스트, WASM 빌드, 개발 확장 준비 | 통과 |

VS Code 1.138.0에서 `tynex-local.tynex` 확장 ID로 `.tlj` 언어 등록, 미저장 진단·수정, 외부 의존 파일 감시를 확인했다. 디버깅 검증은 원본 중단점, 모듈 Step Into/Over, Watch, 조건부 중단점, 오류 스택, 종료·재연결, import/try/else를 포함한다. 보고서는 `build/editor-test/vscode-result.json`과 `vscode-mapped-result.json`에 생성한다. 자동 테스트는 별도 profile을 사용한다.

Zed의 `tynex` 확장·언어 서버 및 `tynex-emmy` 디버그 어댑터는 설정 검사와 WASM 빌드로 검증했다. 현재 구성의 Zed GUI 세션과 Windows 실제 실행은 별도 검증 항목이다.

EmmyLua의 오류 자동 정지는 제공하지 않아 오류 발생 행에 명시적 중단점을 사용한다. try와 선택적 호출에는 내부 프레임·변수가 보일 수 있다. class·trait 변환은 이후 구현하여 문법·LSP·원본 행·self 디버그 훅을 자동 검증했다. 선택적 호출도 LF/CRLF 원본 행·번들 오류 위치·LSP 진단을 자동 검증했다. 새 class·trait·선택적 호출의 에디터 GUI 디버깅은 아직 재검증하지 않았다. match·async 변환과 async 논리 스택은 아직 구현되지 않았다.

`make test`, `make editor-test`, `make binder-test`, `make binder-sanitize`, `make binder-windows`로 해당 검사를 실행할 수 있다. VS Code 실제 세션은 `npm --prefix editors/vscode run test:editor`, Zed 준비는 `make zed-extension`을 사용한다.
