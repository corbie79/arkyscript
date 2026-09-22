# Rust 프런트엔드의 원본 소스 디버깅

현재 지원하는 `.lua/.tlj` 실행기에 대해 VS Code 확장 호스트, EmmyLua DAP 프로토콜, Zed 전용 Tynex 확장의 UI 연결을 검증했다. 생성 코드의 줄이 달라도 원본 중단점·모듈 스택·Step Into/Over·조건부 중단점이 동작한다. [검증 기록](editor-validation.md)은 에디터별 방식과 남은 항목을 구분한다. 타입 진단·구문 강조의 설치 방법은 [에디터 안내](editors.md)를 따른다.

## 준비

`make setup`으로 엔진과 Rust CLI를 빌드한다. Windows에서는 준비한 엔진 경로를 `TYNEX_LUAJIT`으로 지정하거나 현재 SHA의 `build/windows-x64/<SHA>/luajit.exe`를 사용한다. `debug`도 `run`과 동일한 설정·타입 검사 과정을 거치고 디버거 연결 전에 JIT을 끈다.

## VS Code

1. 로컬 Tynex 확장과 [Local Lua Debugger](https://marketplace.visualstudio.com/items?itemName=tomblind.local-lua-debugger-vscode)를 설치하고 `cargo build --locked -p tynex-cli`를 실행한다.
2. 프로젝트 변경을 저장하고 `examples/debug/main.tlj` 2행에 중단점을 둔다.
3. **Tynex: mapped modules**를 실행한다. Tynex이 그래프 전체를 검사하고 번들을 만든 다음 upstream 어댑터에 생성 파일 목록을 전달한다.
4. Step Into로 `worker.tlj` 2행에 들어가 `score`, `amount`를 보고 Watch에서 `score + amount`를 평가한다. Step Over 후 `total`은 15다.

다른 프로젝트의 구성:

```json
{
  "name": "Tynex",
  "type": "tynex",
  "request": "launch",
  "entry": "src/main.tlj",
  "bundleDirectory": "build/debug/vscode",
  "scriptArguments": []
}
```

`entry`와 `bundleDirectory`는 workspace 기준이며 `tynex.serverPath`로 CLI를 지정한다. `mode`와 `stopOnEntry`도 사용할 수 있다. 실행은 저장한 소스를 대상으로 하므로 미저장 프로젝트 문서가 있으면 저장을 요청하는 오류를 표시한다. 번들 생성 실패 시 디버거를 시작하지 않는다. upstream 확장 파일은 수정하지 않는다.

기존 **Rust frontend on LuaJIT: example** 구성도 유지한다. 이 구성은 행을 보존하는 현재 `debug` 명령을 직접 실행하며 일반 소스 맵 검증용 구성과 구분한다.

## Zed

1. [EmmyLua 확장](https://zed.dev/extensions/emmylua)을 설치한다.
2. [EmmyLuaDebugger](https://github.com/EmmyLua/EmmyLuaDebugger/releases)의 운영체제·CPU·LuaJIT에 맞는 `emmy_core`를 준비한다.
3. 라이브러리 디렉터리를 `TYNEX_EMMY_CORE_DIR`로 지정해 실행 작업을 시작한다.

```sh
TYNEX_EMMY_CORE_DIR=/absolute/path/to/core ./bin/tynex debug examples/main.tlj
```

4. 서버 대기 메시지가 나온 뒤 **Rust frontend on LuaJIT: connect to example**을 시작한다.

[debug.json](../.zed/debug.json)은 `emmylua_new`, `127.0.0.1:9966`, `.tlj/.lua` 검색으로 구성한다. request는 upstream 예제대로 launch지만 실행 중인 디버그 서버에 연결하는 구성이다. [tasks.json](../.zed/tasks.json)의 **Tynex: start example for EmmyLua**도 같은 실행을 시작한다. 환경 변수는 Zed 작업 환경에 전달해야 한다. [EmmyLua DAP 공식 구성](https://github.com/EmmyLuaLs/emmylua_dap), [Zed 디버거 설정](https://zed.dev/docs/debugger).

포트 변경 시 `TYNEX_DEBUG_PORT`와 Zed 설정의 port를 함께 수정한다. 실행기는 `.dylib/.so/.dll`을 검색하지만 각 플랫폼 네이티브 코어는 별도 준비·검증이 필요하다.

## 생성 번들과 소스 맵

```sh
./bin/tynex bundle examples/debug/main.tlj --out build/debug/modules
./bin/tynex exec-bundle build/debug/modules
```

번들은 생성 Lua, 표준 v3 `.lua.map`, `debug-map.json`, Lua 로더/manifest를 포함한다. 생성 파일은 원래 생성 경로로 로드한다. 파일명을 원본으로 바꾸고 잘못된 행을 표시하지 않는다. 생성 헤더와 내부 helper 행에는 원본 위치를 붙이지 않고, 사용자 코드의 행은 원본으로 연결한다. Rust emitter는 한 원본 위치가 여러 생성 줄로 확장되는 경우와 매핑 없는 helper도 처리한다. class·trait의 생성 코드에도 원본 매핑을 연결한다. 메서드·필드 기본값 오류와 번들의 원본 행을 회귀 검사한다. match/async 변환은 아직 없다.

소스 맵은 0부터 시작하는 UTF-16 위치를 사용한다. DAP bridge는 디버거의 행 단위 위치와 0/1 기반 옵션을 변환한다. 원본 중단점/조건·스택/범위·breakpoint event·source 요청·출력 위치를 처리하며 변수와 식은 upstream 디버거가 평가한다. 아직 생성 변수 이름 변환, async 논리 스택, helper 자동 건너뛰기는 제공하지 않는다.

Zed의 새 **Tynex: mapped modules** 구성은 `tynex-emmy`를 사용한다. 먼저 준비된 DAP 바이너리를 복사한다.

```sh
python3 scripts/prepare_zed.py --emmy-dap /absolute/path/to/emmylua_dap
./bin/tynex bundle examples/debug/main.tlj --out build/debug/modules
TYNEX_EMMY_CORE_DIR=/absolute/path/to/core ./bin/tynex exec-bundle build/debug/modules --debug
```

대기 메시지가 보이면 Zed에서 해당 구성을 시작한다. `tynexMap`은 `debug-map.json` 경로이며 `tynexBackend`로 EmmyLua DAP 경로를 따로 지정할 수 있다. 생략하면 PATH 또는 `build/editors/bin/emmylua_dap[.exe]`를 찾는다. 중계만 직접 실행하려면 다음을 사용한다.

```sh
./bin/tynex dap --map build/debug/modules/debug-map.json --backend /absolute/path/to/emmylua_dap
```

번들은 현재 로컬 디버깅용 산출물로 절대 경로를 포함한다. 소스를 바꿨으면 재생성하고 새 디버그 세션을 시작한다. 다른 컴퓨터로 옮기는 배포 패키지 형식은 아니다. 기존 파일을 덮어쓰지 않도록 빈 디렉터리 또는 Tynex이 표시한 번들 디렉터리에만 출력한다. legacy 모듈은 원본 로딩을 유지한다.

오류 traceback도 원본 파일과 행으로 복원한다. LuaJIT이 긴 파일명을 줄여 출력하는 경우는 VM의 `short_src`를 조회해 처리한다. 실행되지 않는 타입 선언에는 중단점을 둘 수 없다. EmmyLua의 오류 자동 정지는 현재 지원되지 않아 모듈의 오류 발생 행에 중단점을 두어 검사하며, CLI는 원본 traceback과 실패 종료 코드를 제공한다.

검증 명령:

```sh
npm --prefix editors/vscode run test:editor
python3 scripts/test_emmy_debug.py --backend /absolute/path/to/emmylua_dap --core /absolute/path/to/core
```

EmmyLua 테스트는 실제 네이티브 디버거/core와 연결해 모듈 Step Into/Over, locals, Watch 요청, 조건부 중단점, 오류 발생 행과 종료, 재연결을 검사한다. Zed UI 테스트와 구분한다. Windows 경로 처리는 단위 테스트이며 Windows/Linux UI나 모바일 기기를 검증했다는 뜻은 아니다.

Local Lua Debugger의 수동 Pause와 코루틴 조회 제약은 [공식 안내](https://github.com/tomblind/local-lua-debugger-vscode#requirements--limitations)를 따른다. 디버그 실행은 JIT이 꺼져 있으므로 성능 측정은 `run`에서 한다. Rust 구현과 LuaJIT C 엔진의 네이티브 디버깅은 사용자 Lua 중단점과 별개다.
