# 비동기 파일·TCP I/O

저장소 루트에서 네이티브 I/O 모듈을 준비하고 파일 예제를 실행합니다.

```sh
make setup
./bin/tynex run examples/io/files.arky
```

출력은 다음과 같습니다. 값 사이는 탭입니다.

```text
first	6
second	7
```

`files.arky`는 읽기 두 개를 먼저 시작한 뒤 `await`로 결과를 받습니다. `first.txt`와 `second.txt`는 마지막 개행을 포함합니다. 경로는 CLI가 제공하는 원본 `arg[0]`을 기준으로 계산하므로 다른 작업 디렉터리에서도 실행할 수 있습니다. `args.lua`와 `args.d.arky`는 표준 Lua 인자 테이블에 대한 작은 선언 경계입니다.

TCP 예제는 로컬 echo 서버를 사용합니다. 첫 번째 터미널에서 서버를 시작합니다.

```sh
python3 examples/io/echo_server.py
```

두 번째 터미널에서 클라이언트를 실행합니다.

```sh
./bin/tynex run examples/io/tcp.arky
```

```text
sent	16
echo	hello from Tynex
```

다른 포트를 사용하려면 서버에 `--port 9876`, 클라이언트에 `-- 9876`을 전달합니다. 서버는 `127.0.0.1`에만 바인딩하며 클라이언트 하나가 연결을 종료하면 함께 종료됩니다.

`net.connect()`의 결과는 `Socket`입니다. `socket:read()`와 `socket:write()`는 Task를 반환하고, `socket:close()`는 즉시 닫습니다. 읽기는 요청한 최대 길이보다 짧을 수 있으며 EOF는 `nil`입니다. 예제는 여러 읽기를 모으고 성공·실패 경로에서 연결을 닫습니다. 기본 작업 제한 시간은 30초이며 각 호출의 마지막 인자로 밀리초 단위 값을 지정할 수 있습니다.

CLI에서는 `scheduler.run()`으로 기다립니다. 게임 호스트는 프레임마다 `scheduler.step()`을 호출해 Lua 실행을 재개할 수 있습니다. 파일·소켓 작업은 네이티브 모듈에서 처리하며, Lua 콜백과 coroutine은 소유한 VM의 스레드에서 실행됩니다.
