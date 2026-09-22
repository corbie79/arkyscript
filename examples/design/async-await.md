# async / await — 설계 예제

**아직 실행할 수 없는 제안 문법**입니다. 현재 실행 가능한 병렬 기능은 [Task API](../tasks/README.md)이며, `async/await` 변환과 coroutine 스케줄러는 후속 단계입니다.

```text
-- task.spawn을 Task<number>로 검사하는 제네릭 API는 후속 구현이다.
async function calculate(): number
   local first = task.spawn(function(input: number, context): number
      return input * 2
   end, 21)
   local second = task.spawn(function(input: number, context): number
      return input * 3
   end, 10)

   -- 두 작업은 먼저 제출되어 worker에서 병렬로 진행된다.
   -- await는 현재 async 함수만 중단하고 메인 이벤트 루프는 계속 돈다.
   local a = await first
   local b = await second
   return a + b
end

async function main()
   try
      local total = await calculate()
      print(total)
   except err then
      print("task failed: " .. tostring(err))
   end
end
```

`calculate()`는 `Task<number>`를, `await calculate()`는 number를 얻는 규칙을 계획합니다. 호스트가 이벤트 루프에 `main()`을 등록하는 진입 API는 아직 없습니다. async 함수 호출만으로 새 OS 스레드를 만들지는 않으며, 실제 병렬 계산은 `task.spawn`을 통해 worker 풀에 맡깁니다.

현재 `handle:wait()`는 호출 스레드 전체를 막으므로 위 `await`와 동작이 다릅니다. 다음 단계에는 완료 큐, 중복 resume 방지, 취소/오류 전파, 원본 async 스택 매핑이 필요합니다. HTTP·파일 비동기 API 역시 이 예제에 의해 제공되는 것은 아닙니다.
