# 에디터와 최소 LSP

`tynex lsp`는 stdin/stdout의 LSP 3.17 메시지로 컴파일러 진단을 제공한다. VS Code와 Zed에서 같은 Rust 모듈 검사기·파일별 정책을 사용한다. 에디터 설정은 사용자 스크립트의 타입 정책을 바꾸지 않는다.

## VS Code

저장소 루트에서 CLI와 로컬 확장을 준비한다. 확장 빌드에는 Node.js 22 이상이 필요하다.

```sh
cargo build --locked -p tynex-cli
npm --prefix editors/vscode ci
npm --prefix editors/vscode run package
code --install-extension editors/vscode/tynex-0.1.0.vsix
```

저장소를 열면 [.vscode/settings.json](../.vscode/settings.json)의 실행 파일을 사용한다. 다른 프로젝트에서는 `tynex.serverPath`를 빌드한 바이너리의 절대 경로로 설정하거나 PATH에 `tynex`을 둔다. `${workspaceFolder}`를 사용할 수 있으며 Windows에서 확장자가 없는 절대 경로에는 `.exe`를 붙인다. 첫 workspace folder의 실행 파일 하나를 사용하므로 서로 다른 컴파일러 버전을 쓰는 프로젝트는 별도 창으로 연다.

`.luax`는 **Tynex** 언어로 표시된다. `.lua` 버퍼도 반영하려면 `tynex.checkLua`를 켠다. 이 저장소에서는 켜져 있다. 실행 경로나 옵션을 바꾼 뒤 **Tynex: Restart Language Server**를 실행한다. 신뢰하지 않은 workspace에서는 확장을 실행하지 않는다.

확장은 `.luax` TextMate 구문 강조·괄호/주석 편집·중단점 등록을 제공한다. LSP 진단은 Problems와 밑줄로 표시한다. 실제 디버깅은 별도 Local Lua Debugger 확장을 사용하며, `type: "tynex"` 실행 구성이 번들을 만든 뒤 생성 파일 목록과 표준 소스 맵을 전달한다. 확장에 LuaJIT이나 Rust 실행 파일을 포함하지 않는다.

## Zed

프로젝트 소유 Tynex 확장이 `.luax` 전용 Tree-sitter 문법과 Rust LSP를 제공한다. 타입·함수·인자·필드·import·declare·try/except/else·주석·문자열 강조, 괄호, 들여쓰기, 함수 outline 쿼리를 포함한다. Lua grammar 원본은 MIT 라이선스와 고정 revision으로 `editors/tree-sitter-tynex/vendor`에 보관하고 수정하지 않는다. 편집기의 오류 복구와 컴파일러의 문법·타입 검사는 별도로 테스트한다.

```sh
cargo build --locked -p tynex-cli
rustup target add wasm32-wasip2
python3 scripts/prepare_zed.py
```

Zed의 **zed: install dev extension**에서 출력된 `build/editors/zed` 폴더를 선택하고 저장소 **폴더**를 연다. 스테이징 스크립트는 Zed의 grammar revision 요구를 충족하기 위해 `build/editors/grammar`에만 로컬 Git snapshot을 만든다. 프로젝트나 공식 LuaJIT 저장소를 커밋하지 않는다. 확장/grammar 변경 뒤에는 스테이징을 다시 하고 dev extension을 재빌드한다.

`.luax`의 언어 이름은 **Tynex**다. [.zed/settings.json](../.zed/settings.json)의 `lsp.tynex.binary.path`는 `./target/debug/tynex`을 가리키며, binary override에는 `"arguments": ["lsp"]`도 반드시 지정한다. 다른 프로젝트에서는 절대 경로나 PATH의 `tynex`을 사용한다. Windows는 `.exe` 경로를 지정한다. `.lua`에는 별도 [EmmyLua 확장](https://zed.dev/extensions/emmylua)을 사용하며 이 저장소의 설정은 그 언어 서버도 Tynex으로 바꾼다. typed formatter는 아직 없으므로 format-on-save는 껐다.

경로·인자를 바꾼 뒤 **workspace: reload**를 실행한다. 단일 파일만 열면 프로젝트 설정을 찾을 수 없으므로 폴더를 열거나 사용자 설정에 절대 경로를 둔다. 소스 맵 디버거는 이 확장의 `tynex-emmy` 어댑터를 사용한다. 별도 EmmyLua DAP 실행 파일과 `emmy_core` 준비는 [디버깅 안내](debugging.md)를 따른다. [Zed 언어 확장 사양](https://zed.dev/docs/extensions/languages)

## 동작 범위

- 열린 파일의 전체 텍스트 동기화와 버전 번호를 사용한다. 저장하지 않은 파일·새 파일·의존 모듈·`.d.luax`도 검사에 반영하며 파일을 덮어쓰지 않는다.
- 편집할 때 열린 모든 진입 파일의 그래프를 다시 검사한다. 의존 파일의 export 변경도 호출자에게 반영한다. 파일을 닫으면 디스크 내용을 다시 사용하고 더 이상 필요한 진단은 지운다.
- 진단 범위는 코어의 UTF-8 span을 LSP의 0부터 시작하는 UTF-16 좌표로 바꾼다. 한글·이모지·CRLF·공백과 `#`가 있는 파일 URI를 처리한다. 모듈/설정 오류처럼 범위가 없는 진단은 위치만 표시한다.
- 클라이언트가 동적 파일 감시 등록을 지원하면 `.luax/.lua/tynex.toml` 변경을 구독한다. 저장한 설정과 디스크 의존 변경을 다시 읽는다. 미저장 TOML 설정은 반영하지 않는다. 파일 감시를 제공하지 않는 클라이언트는 문서 편집/저장 또는 서버 재시작으로 갱신한다.
- `.lua` legacy 본문은 공식 LuaJIT `loadfile`로 구문만 검사한다. `LUA_INIT`을 제거하며 사용자 본문·require를 실행하지 않는다. 엔진이 없으면 구문 검사를 할 수 없다는 경고를 표시한다. legacy의 타입 변경은 오류가 아니다.
- 로컬 `file:` URI만 지원한다. untitled·원격 URI, 자동완성·hover·정의 이동·rename·formatter·증분 변경은 미구현이다. 제공하지 않는 기능을 LSP capability로 광고하지 않는다.
- 열린 파일 최대 256개, 프로토콜 메시지 최대 16 MiB이며 기존 토큰·모듈 그래프 제한도 적용한다. 검사는 순차 실행이고 이전 작업을 중간 취소하거나 증분 캐시하지 않는다. 큰 workspace의 지연 최적화는 후속 작업이다.

## 재현 가능한 검증

```sh
cargo test --workspace --locked
python3 -m unittest discover -s tests -v
npm --prefix editors/vscode ci
npm --prefix editors/vscode test
npm --prefix editors/tree-sitter-tynex ci
python3 scripts/test_editor_grammar.py
code --user-data-dir build/editor-test/profile \
  --extensions-dir build/editor-test/extensions \
  --install-extension tomblind.local-lua-debugger-vscode
npm --prefix editors/vscode run test:editor
```

VS Code 테스트는 실제 설치된 `code`를 쓰며 `CODE_BIN`으로 CLI 경로를 바꿀 수 있다. 별도 profile에서 미저장 오류/수정, 외부 의존 파일 감시, 원본 중단점·지역 변수·Watch·Step Over·정상 종료를 검사한다. 추가로 2행 늘어난 생성 코드에서 모듈 Step Into/Over·조건부 중단점·오류 스택·새 세션을 검사한다. 결과는 `build/editor-test/vscode-result.json`, `vscode-mapped-result.json`이며 두 보고서가 모두 있어야 성공이다. 테스트를 위해 그 profile의 작업 공간 신뢰 프롬프트를 비활성화하고 테스트 파일만 `build/editor-test`에 쓴다. 개인 profile 설정은 변경하지 않는다.

검증 환경과 Zed UI에서 직접 확인한 항목은 [검증 기록](editor-validation.md)에 분리해 적었다. 표준 동작 기준은 [LSP 사양](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/), VS Code 클라이언트는 [공식 language server guide](https://code.visualstudio.com/api/language-extensions/language-server-extension-guide)를 따른다.
