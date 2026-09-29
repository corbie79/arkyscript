# trait 예제

**구현 완료.** 실행 가능한 원본은 [traits.arky](../traits.arky)이며 `./bin/arky run examples/traits.arky`로 실행합니다.

```lua
trait Named
   label: function(Self): string

   function describe(): string
      return "name: " .. self:label()
   end
end

trait Scorable
   add_score: function(Self, number): ()
   score: function(Self): number
end

class Player is Named, Scorable
   name: string
   points: number = 0

   function label(): string
      return self.name
   end

   function add_score(amount: number): ()
      self.points = self.points + amount
   end

   function score(): number
      return self.points
   end
end

class Device is Named
   name: string

   function label(): string
      return "device: " .. self.name
   end
end

local function print_name(value: Named): ()
   print(value:describe())
end

local player = Player { name = "Mina" }
player:add_score(42)
assert(player:score() == 42)
print_name(player)                         -- name: Mina
print_name(Device { name = "sensor-7" }) -- name: device: sensor-7
```

`label`·`add_score`·`score`는 구현 클래스가 제공할 필수 메서드입니다. `describe`는 trait의 기본 구현이며 `Self`는 구현 클래스의 타입입니다. `Player`는 두 trait을 명시적으로 구현하고 `print_name`은 `Named` 계약으로 두 종류의 값을 받습니다.

오류 규칙:

- 필수 메서드 누락 또는 인자·반환 타입 불일치는 컴파일 오류입니다.
- 두 trait이 같은 이름의 기본 메서드를 제공하면 클래스에서 직접 구현해 충돌을 해소합니다.
- trait은 필드나 생성자를 선언하지 않으며 클래스 상태는 메서드를 통해 사용합니다.
- Rust의 소유권 규칙이나 정적 디스패치를 의미하지 않습니다. Lua 테이블과 메서드 호출로 실행합니다.

전체 규칙은 [class·trait 규칙](../../docs/classes-traits.md)를 참고하세요.
