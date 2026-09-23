# 비동기 파일·TCP I/O

파일 읽기와 TCP 연결·송수신을 managed `Task`로 반환한다. `await` 중에는 다른 coroutine과 I/O를 계속 진행할 수 있다. Lua 코드는 Lua VM을 소유한 스레드에서만 재개한다. 공식 LuaJIT 소스는 수정하지 않는다.

```lua
local async function load(): string
   local first = fs.read_file('first.txt')
   local second = fs.read_file('second.txt')
   return (await first) .. (await second)
end
print(scheduler.run(load()))
```

파일·DNS에는 별도의 고정 크기 C++20 작업 풀을 사용한다. TCP는 논블로킹 소켓이며 `scheduler.step()`에서 준비된 상태를 확인해 진행한다. 연결마다 OS 스레드를 생성하지 않는다. 현재 소켓별 준비 상태를 확인하는 방식이므로 대규모 서버용 epoll/kqueue/IOCP 이벤트 루프는 아니다.

| API | 결과 | 의미 |
| --- | --- | --- |
| `fs.read_file(path, max_bytes?, timeout_ms?)` | `Task<string>` | 일반 파일 전체를 바이너리 문자열로 읽기 |
| `net.connect(host, port, timeout_ms?)` | `Task<Socket>` | DNS·IPv4·IPv6 TCP 연결 |
| `socket:read(max_bytes, timeout_ms?)` | `Task<string?>` | 최대 지정 크기의 한 청크, EOF는 nil |
| `socket:write(data, timeout_ms?)` | `Task<number>` | 전체 문자열을 전송한 후 바이트 수 반환 |
| `socket:close()` | `boolean` | 첫 종료면 true, 이미 닫혔으면 false |

```lua
local async function exchange(): ()
   local socket: Socket = await net.connect('127.0.0.1', 8765)
   await socket:write('hello\n')
   local reply = await socket:read(4096)
   if reply ~= nil then print(reply) end
   socket:close()
end
scheduler.run(exchange())
```

TCP에는 메시지 경계가 없다. `write()` 한 번에 대응하는 데이터가 여러 `read()`로 나뉠 수 있으며 `read(n)`은 정확히 n바이트를 기다리는 함수가 아니다. 길이 헤더나 구분자 등 애플리케이션 프로토콜에 맞춰 조합해야 한다. 문자열 안의 NUL을 포함한 바이너리를 보존한다. 빈 문자열을 쓰면 0을 반환한다.

한 소켓에서 읽기 하나와 쓰기 하나는 동시에 진행할 수 있다. 동일 방향의 작업을 겹쳐 시작하면 오류가 발생한다. 여러 소켓의 I/O Task를 먼저 만든 뒤 기다리면 함께 진행된다. 완료된 연결 Task를 여러 번 기다리면 같은 Socket 객체를 받는다. Socket은 다른 worker VM으로 넘기지 않는다.

## 시간 제한·오류·수명

기본 시간 제한은 30,000ms다. `timeout_ms`는 0~2,147,483,647의 정수이며 0은 즉시 만료를 뜻한다. 연결 시간 제한은 DNS 대기를 포함하고 파일 읽기는 작업 큐 대기를 포함한다. 제한 초과 여부는 호스트가 스케줄러를 진행할 때 관찰된다.

파일은 기본 8MiB, 옵션으로 최대 64MiB까지 읽는다. 파일의 max_bytes=0은 빈 파일만 허용한다. 소켓의 한 read/write는 최대 8MiB다. 더 큰 데이터는 청크로 나눈다. 한 모듈의 미완료 요청은 최대 256개, 열린 소켓은 최대 1024개로 제한한다. 경로·호스트의 NUL, 잘못된 포트·크기·시간 제한은 제출 시 오류다. 파일 없음·용량 초과·연결 실패·시간 초과는 Task 실패이며 `await`, `result()`, `scheduler.run()`에서 오류를 전달한다. `try/except`로 처리할 수 있다.

```lua
local async function load_or_empty(path: string): string
   try
      return await fs.read_file(path)
   except reason then
      print(reason)
      return ''
   end
end
```

I/O Task 자체의 `cancel()`은 네이티브 작업에도 취소를 요청한다. 다른 coroutine이 그 Task를 기다리다가 취소된 경우에는 공유 작업을 자동 취소하지 않는다. 소켓 작업의 취소·시간 초과·치명적 오류는 해당 소켓을 닫고, 같은 소켓의 다른 진행 중인 작업도 실패시킨다. 전송 중 실패했다면 일부 바이트는 이미 상대에게 도착했을 수 있다.

Socket은 `close()`로 명시적으로 정리하고 GC 시에도 자동 정리한다. 진행 중 작업이 있는 소켓은 작업이 보관한다. 실패 경로에서도 바로 해제하고 싶다면 `try/except`에서 close 후 오류를 다시 던진다. 완료되지 않은 managed Task는 스케줄러가 보관하므로 결과를 버리는 것만으로 작업이 취소되지는 않는다.

파일 읽기는 청크 사이에 취소를 확인한다. 실행 중인 OS 파일 읽기·DNS 시스템 호출은 강제로 중단하지 않는다. Task는 먼저 취소·시간 초과를 보고할 수 있지만 VM 종료 시에는 남은 작업 스레드를 기다릴 수 있다. FIFO·장치·디렉터리는 일반 파일 로더에서 거절한다.

## 빌드·임베딩·검증

```sh
make setup                   # 엔진 + Task/I/O 모듈 + Rust 도구
make task                    # 기존 엔진용 두 네이티브 모듈 재빌드
make task-test               # C++ + coroutine + worker + 파일/TCP 통합 검사
make binder-sanitize         # ASan/UBSan 수명 검사
make windows                 # xwin Windows x64 엔진 + 두 DLL
./bin/tynex run examples/io/files.arky
```

`tynex_io.so` 또는 `tynex_io.dll`을 엔진 옆에 배치한다. `tynex run/debug/bundle`은 private native loader로 모듈을 로드하고 읽기 전용 `fs`·`net`을 typed 환경에 제공한다. 기존 `require`나 `package.loaded`를 변경해도 이미 캡처한 내부 함수를 바꾸지는 않는다. Lua 자체를 보안 샌드박스로 만드는 기능은 아니다.

임베딩에서는 모듈 검색 경로를 준비한 뒤 같은 런타임을 주입한다.

```lua
local runtime = assert(loadfile('runtime/async.lua'))()
local environment = setmetatable({
   __tynex_async_runtime = runtime,
   scheduler = runtime, fs = runtime.fs, net = runtime.net
}, {__index = _G})
-- 생성한 Lua chunk에 setfenv(chunk, environment) 적용 후 실행
-- 호스트 프레임 콜백에서 Lua 소유 스레드로:
runtime.step(64)
```

[실행 예제와 로컬 echo 서버](../examples/io/README.md)를 제공한다. 자동 테스트는 임시 파일·로컬 TCP 서버로 바이너리·EOF·동시 진행·취소·시간 제한·번들 소스 맵을 확인한다. Windows 실행 검증은 CI에 연결했다. Android·iOS 빌드/기기 검증과 Cocos 프레임 콜백 통합은 별도 작업이다.

현재 범위는 파일 읽기와 TCP 클라이언트다. 파일 쓰기, TCP 서버 listen/accept, UDP, HTTP/TLS, 타이머, 대규모 I/O 이벤트 백엔드는 아직 제공하지 않는다.
