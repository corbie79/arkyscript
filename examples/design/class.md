# class 예제

**구현 완료.** 실행 가능한 원본은 [class.luax](../class.luax)이며 `./bin/tynex run examples/class.luax`로 실행합니다.

```lua
class Player
   name: string
   score: number = 0
   tags: {string} = {}

   function add_score(amount: number): ()
      self.score = self.score + amount
   end

   function label(): string
      return self.name
   end
end

local mina = Player { name = "Mina" }
local joon = Player { name = "Joon", score = 5 }
mina:add_score(42)
mina.tags[1] = "leader"

print(mina:label(), mina.score) -- Mina, 42
print(joon:label(), joon.score) -- Joon, 5
assert(joon.tags[1] == nil)     -- 테이블 기본값도 인스턴스마다 별도 생성

local same = mina
same:add_score(1)
assert(mina.score == 43)       -- 대입은 인스턴스 참조를 공유
```

구현 규칙:

- 기본값이 없는 `name`은 생성 시 필수입니다. `Player {}`는 초기화 오류입니다.
- `score`에 문자열을 넣거나 선언하지 않은 필드를 전달하면 오류입니다.
- 생성 호출마다 `{}` 같은 기본값을 새로 평가합니다.
- `self`는 현재 클래스의 타입이며 메서드 호출 시 암묵적으로 전달합니다.
- 같은 필드가 있어도 클래스 이름이 다르면 서로 다른 타입입니다.
- 메모리는 Lua GC가 관리하며 대입에 Rust의 move·borrow 규칙을 적용하지 않습니다.

필드 초기화와 타입 불일치는 gradual·strict 모두 검사합니다. `.lua` legacy 코드에 이 키워드를 적용하지 않습니다. 전체 규칙은 [class·trait 규칙](../../docs/classes-traits.md)를 참고하세요.
