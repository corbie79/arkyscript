# Windows x64 빌드와 LuaJIT 업데이트

macOS ARM64 또는 Linux 64비트에서 `xwin + clang-cl + lld-link + llvm-lib`로 공식 LuaJIT 소스를 크로스 빌드한다. 현재 구현 대상은 `x86_64-pc-windows-msvc`다. 이 빌더는 엔진의 EXE·DLL·정적 라이브러리를 생성한다. 독립 Rust CLI는 Cargo로 별도 빌드하며 엔진 ZIP에는 아직 포함하지 않는다. Windows CI에는 엔진과 Rust 실행기를 함께 실행하는 단계도 준비했다.

## 준비와 빌드

Python 3.12 이상, Git, Make, 호스트 C/C++20 컴파일러, LLVM·LLD 및 xwin 0.10.0이 필요하다. 고정한 Microsoft C++ STL은 Clang 19 이상을 요구하므로 Windows Task·바인더 빌드도 해당 버전을 사용한다. 공식 엔진 업데이트의 프런트엔드 회귀 검증에는 프로젝트가 고정한 Rust 도구 체인도 필요하다.

```sh
# macOS: LLVM은 Homebrew의 keg-only 경로도 자동 탐색한다.
brew install llvm lld xwin
make setup
make windows
```

Linux에서는 배포판의 `clang`, `lld`, `llvm` 패키지와 `cargo install xwin --version 0.10.0 --locked`로 준비할 수 있다. SDK 다운로드 시 xwin이 Microsoft SDK/CRT 사용권에 동의하는 옵션으로 실행된다. 도구 설치와 최초 소스·SDK 다운로드에는 네트워크가 필요하다. SDK 캐시는 `.deps/xwin-cache/`, 사용할 헤더·라이브러리는 `.deps/xwin/`에 보관한다.

`windows-toolchain.json`은 xwin 버전, SDK `10.0.26100`, CRT 패키지 `14.44.17.14`, x86_64 desktop 선택을 고정한다. 다른 버전이 필요하면 이 파일을 명시적으로 변경한다. xwin 버전이 일치하지 않거나 해당 SDK/CRT를 구할 수 없으면 실패하며 최신 버전으로 임의 대체하지 않는다. LLVM의 실제 버전은 산출물에 기록한다. LLVM 버전과 Microsoft 배포 패키지까지 모든 바이트가 고정되는 재현 빌드를 보장하지는 않는다.

필요하면 `CLANG_CL`, `LLD_LINK`, `LLVM_LIB`, `LLVM_READOBJ`, `HOST_CC`, `XWIN` 환경 변수로 실행 파일 경로를 지정할 수 있다. 변수에는 추가 인수 없이 실행 파일 하나의 경로를 넣는다. `make windows-sdk`는 SDK 준비만 수행한다.

## 빌드 방식과 결과

공식 저장소 체크아웃을 수정하지 않고 지정 SHA의 `git archive`를 별도 디렉터리에 추출한다. `minilua`, `buildvm`은 호스트에서 실행하며 Windows x64 VM 오브젝트와 등록 헤더를 생성한다. 엔진 C 소스는 Windows MSVC ABI로 컴파일한다. 생성 파일·오브젝트는 다른 커밋이나 네이티브 빌드에서 재사용하지 않는다. 실패 시 기존 성공 산출물을 유지하고 실패 로그를 남긴다.

결과는 `build/windows-x64/<전체 SHA>/`와 같은 상위 디렉터리의 `luajit-windows-x64-<전체 SHA>.zip`이다.

`make windows`는 이어서 `scripts/build_task.py --windows`를 실행해 `tynex_task.dll`을 만들고 **엔진 + Task**를 `tynex-windows-x64-<전체 SHA>.zip`으로 묶는다. `task-build-info.json`에는 모듈·소스 해시와 엔진 SHA를 별도로 기록한다. Rust CLI는 두 ZIP 모두에 아직 포함하지 않는다. Ubuntu 24.04 CI는 `clang-19 clang-tools-19 lld-19 llvm-19`를 사용하며 `clang-cl` 제공 패키지인 `clang-tools-19`도 필요하다.

| 파일 | 용도 |
| --- | --- |
| `luajit.exe`, `lua51.dll`, `lua/jit/` | LuaJIT 실행 환경, 함께 배포 |
| `lua51.lib` | `lua51.dll`에 연결할 때 사용하는 import library |
| `lua51-static.lib` | 엔진 정적 링크용 라이브러리 |
| `include/` | 생성된 버전 헤더와 Lua/C API 헤더 |
| `*.pdb` | 엔진·실행 파일 디버그 심벌 |
| `smoke.lua`, `embed-smoke.exe` | Windows 실제 실행 검사 |
| `build-info.json`, `pe-inspection.txt`, `build.log` | 커밋·도구 버전·해시·PE 검사 및 빌드 기록 |
| `COPYRIGHT-LuaJIT` | upstream 라이선스 |

CRT는 `/MT`로 정적 링크한다. 정적 임베딩하는 호스트도 CRT 설정을 맞춰야 한다. 다른 CRT에서 할당한 메모리를 서로 해제하지 않도록 API의 소유권 규칙을 지킨다. Lua/C 확장 DLL을 로드할 앱은 공식 안내에 따라 `lua51.dll`을 사용하는 동적 링크 구성을 선택한다. xwin SDK·CRT 파일은 결과 ZIP에 포함하지 않는다.

크로스 빌드는 세 실행 이미지의 x64 PE32+ 형식과 DLL 플래그, Lua/C API export를 검사한다. 이것은 Windows에서 실행해 본 결과와 구분한다. Windows PowerShell에서 산출물 디렉터리로 이동해 다음을 실행한다.

```powershell
.\luajit.exe -v
.\luajit.exe smoke.lua
.\embed-smoke.exe
```

검사는 JIT trace 생성, 합산 결과, 64비트 Windows FFI ABI, Win32 함수 호출, FFI callback, 코루틴, 오류 처리와 정적 임베딩을 포함한다. `.github/workflows/windows.yml`은 Linux에서 크로스 빌드한 뒤 Windows runner에서 이 검사들을 실행하도록 구성되어 있다. 워크플로는 저장소에 업로드되어야 실행되며, 현재 로컬 작업만으로 GitHub 실행 결과가 생기지는 않는다.

## 공식 업데이트 적용

```sh
make check-luajit-update  # 공식 v2.1 SHA와 현재 버전 비교만 수행
make update-luajit        # 새 버전 검증 후 dependencies.json 갱신
```

업데이트는 아래 순서로 진행한다.

1. 공식 저장소 `v2.1`의 전체 커밋 SHA를 조회한다.
2. 후보의 네이티브 바이너리를 별도 SHA 디렉터리에 빌드한다.
3. 후보의 Windows x64 엔진을 크로스 빌드하고 PE/API를 검사한다.
4. Task 모듈의 네이티브·Windows 빌드와 바인더/Task C++ 검증, 후보 네이티브 바이너리로 Rust·Lua 프로젝트 테스트를 실행한다.
5. 전부 성공한 경우에만 `dependencies.json`을 원자적으로 갱신한다.

후보 빌드나 테스트 실패 시 기존 pin과 이전 커밋의 산출물이 유지된다. 검증 도중 다른 작업이 `dependencies.json`을 수정해도 덮어쓰지 않는다. 공식 소스 캐시에 추적 파일의 로컬 수정이 있으면 빌드를 중단하며 이를 리셋하거나 삭제하지 않는다. 업데이트는 수동 명령이며 주기적 자동 갱신은 설정하지 않는다.

이미 최신이면 변경 없이 끝난다. 현재 버전을 다시 검증하거나 특정 커밋을 시험할 수 있다.

```sh
make verify-luajit
python3 scripts/update_luajit.py --verify --revision <전체-SHA>
python3 scripts/build_windows.py --revision <전체-SHA>
```

검증한 과거 버전으로 되돌릴 때도 같은 흐름을 사용한다.

```sh
python3 scripts/update_luajit.py --apply --revision <이전-전체-SHA>
```

변경된 `dependencies.json`을 저장소에 반영한 뒤 Windows CI의 실제 실행 통과를 확인하고 배포한다. upstream의 빌드 절차가 크게 바뀌면 `scripts/build_windows.py` 조정이 필요할 수 있다. 라이브러리 목록은 공식 `msvcbuild.bat`에서 읽고, 해당 파일의 해시도 기록한다. 엔진 코드에 별도 패치를 누적하지 않는 방식이므로 공식 버전 교체 시 패치 충돌을 줄일 수 있다.

참고: [xwin](https://github.com/Jake-Shadle/xwin), [LuaJIT 공식 빌드·임베딩 안내](https://luajit.org/install.html), [롤링 릴리스 정책](https://luajit.org/status.html).
