# payload enum과 match — 설계 예제

**아직 실행할 수 없는 제안 문법**입니다. enum·match·제네릭은 현재 컴파일러에 없습니다. class·trait 명명 타입은 별도로 구현되어 있습니다. enum에 값이 들어간다는 것은 각 경우(variant)가 서로 다른 형태의 데이터를 담는다는 뜻입니다.

```text
enum Download
   Waiting
   Progress(number, number)
   Done(string)
   Failed(string)
end

local state: Download = Download.Progress(30, 100)
match state
   case Download.Waiting then
      print("waiting")
   case Download.Progress(received, total) then
      print(received .. "/" .. total)
   case Download.Done(path) then
      print(path)
   case Download.Failed(message) then
      print(message)
end
```

`Progress`는 수신량과 전체 크기, `Done`은 저장 경로를 담습니다. 같은 enum 타입 안에 여러 데이터 모양이 있고 `match`로 경우를 확인한 뒤 데이터를 꺼냅니다.

```text
enum Result<T, E>
   Ok(T)
   Err(E)
end

enum Option<T>
   Some(T)
   None
end

local answer: Result<number, string> = Result.Ok(42)
match answer
   case Result.Ok(value) then
      print(value + 1)
   case Result.Err(message) then
      print(message)
end
```

제안하는 오류 예: `Download.Progress("30", 100)`의 payload 타입 불일치, `Done` 등을 빠뜨린 불완전한 match, 중복 분기. 모든 경우를 열거하거나 마지막에 `case _ then`을 두도록 검사할 계획입니다. 다른 enum의 같은 이름 variant는 구분합니다. 이 규칙이 현재 검사된다는 뜻은 아닙니다.
