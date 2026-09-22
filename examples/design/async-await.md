# async / await — 구현됨

이전 제안 문법을 Rust 컴파일러와 coroutine 스케줄러에 구현했습니다.

- [같은 VM의 async 함수](../async.tlj): 실행 큐, await, Task 결과, 프레임 단위 step
- [실제 worker 연결](../tasks/async.tlj): OS worker 작업을 제출하고 await
- [문법·타입·취소·호스트 연결](../../docs/async-await.md)

```sh
./bin/tynex run examples/async.tlj
./bin/tynex run examples/tasks/async.tlj
```

파일 읽기·TCP 연결과 송수신은 [I/O 예제](../io/README.md)와 [API 안내](../../docs/io.md)에 구현되어 있습니다. HTTP/TLS, 타이머, 실제 Cocos·모바일 호스트 통합, 디버거의 전체 async 논리 스택은 후속 작업입니다.
