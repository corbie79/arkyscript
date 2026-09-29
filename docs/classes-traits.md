# 스크립트 class·trait

독립 Rust 컴파일러가 `.arky`의 class·trait를 검사하고 일반 Lua 테이블·메서드로 변환한다. 공식 LuaJIT 소스와 `.lua` legacy 로더는 변경하지 않는다. C++ `next2::luax` 바인더와 별도로 사용하는 스크립트 기능이다.

## 클래스

```lua
class Player
   name: string
   score: number = 0
   tags: {string} = {}

   function add(amount: number): ()
      self.score = self.score + amount
   end
end

local player: Player = Player {name = "Mina"}
player:add(42)
```

`class` 선언은 현재 블록에 지역 이름을 만든다. 그 이름은 생성자 값과 타입 표기에 사용할 수 있으며 같은 스코프에서 덮어쓸 수 없다. 서로 다른 선언은 이름이나 필드 모양이 같아도 다른 타입이다. 바깥 선언을 안쪽 블록의 별도 선언으로 가릴 수 있다. `number`, `string`, `boolean`, `any`, `Self`는 클래스·trait 이름으로 사용할 수 없다.

생성자는 정확히 하나의 초기화 테이블을 받는다. `Player {...}`와 `Player({...})`가 같은 호출이다. 기본값 없는 non-nil 필드는 필수이며 알 수 없는 필드·잘못된 필드 타입은 gradual과 strict 모두 오류다. `T?`, `nil`, `any` 필드는 생략할 수 있다. Lua 테이블의 `nil`은 키가 없는 상태이므로 기본값이 있는 필드에 `nil`을 주면 기본값을 사용한다. `false`는 그대로 유지한다.

기본값은 생략된 필드에 대해서만, 선언 순서대로, 생성할 때마다 평가한다. `{}`나 익명 함수 식은 매번 별도 값을 만들고, 바깥 변수를 반환하는 식은 그 변수의 값을 사용한다. 초기화 테이블은 얕게 복사한다. 중첩 테이블과 클래스 인스턴스는 Lua 참조를 공유하며 깊은 복사·Rust 소유권 규칙을 적용하지 않는다.

필드는 `score = 0`처럼 기본값에서 타입을 추론할 수 있다. strict에서 타입과 추론 가능한 기본값이 모두 없으면 진단하고 gradual에서는 동적 값으로 남긴다. 메서드의 `self`는 암묵적 첫 인자다. 인자·반환형·필드 접근을 검사하며, 반환형을 명시하면 같은 클래스의 재귀·전방 메서드 호출도 검사할 수 있다. 함수 인자·반환형 표기는 인자 이름을 도입하기 전의 선언 스코프에서 해석한다. 본문 안에서는 일반 지역 변수의 이름 가리기 규칙을 따른다. 메서드는 인스턴스로 접근하고 클래스 이름에 정적 메서드를 추가하는 문법은 없다.

기본값은 나중에 호출되는 식이므로 클래스 선언 시점의 지역 변수 nil 좁히기에 의존할 수 없다. 초기화 중인 인스턴스의 `self`나 바깥 함수의 `...`를 기본값에서 직접 참조하지 않는다. 바깥 함수의 가변 인수가 필요하면 먼저 지역 값에 저장한다.

## Trait 계약과 기본 구현

```lua
trait Named
   label: function(Self): string

   function describe(): string
      return "name: " .. self:label()
   end
end

class User is Named
   name: string
   function label(): string return self.name end
end

local function show(value: Named): ()
   print(value:describe())
end
show(User {name="Mina"})
```

필수 메서드는 `function(Self, 인자타입...): 반환타입`으로 선언한다. 첫 타입 `Self`가 수신자이고, 기본 구현의 `function describe()`에는 이를 직접 쓰지 않는다. `Self`는 현재 구현 클래스의 타입을 뜻한다. trait은 필드나 생성자를 가지지 않고 자신의 계약에 선언한 메서드를 통해 상태를 사용한다.

`class Player is Named, Scorable`로 여러 trait을 구현한다. 필수 메서드 누락과 계약 불일치는 오류다. 같은 이름의 기본 구현이 여러 개면 클래스가 그 메서드를 직접 구현해 충돌을 해소해야 한다. 클래스 구현은 모든 해당 계약을 만족해야 한다. 이름만 같은 메서드가 있다는 이유로 trait 구현으로 간주하지 않는다.

trait 타입으로 인자를 받을 수 있고 호출은 Lua의 동적 메서드 디스패치로 실행한다. 단, 수신자 외의 인자 타입에 `Self`가 들어가는 메서드는 서로 다른 구현을 섞지 않도록 구체 클래스 수신자를 요구한다. 예를 들어 `equal: function(Self, Self): boolean`은 구체 `Player` 값으로 호출할 수 있지만, 구현 클래스가 지워진 trait 인자로는 호출할 수 없다. 같은 이유로 추상 trait 값의 메서드를 함수 값으로 분리하거나, 변경 가능한 테이블·콜백 내부에 `Self`를 담은 반환값을 추상 trait를 통해 사용하는 것도 제한한다.

클래스 필드는 수정할 수 있지만 클래스·trait의 메서드 계약을 대입으로 바꾸지는 못한다. `any`, legacy Lua, FFI를 통해 내부 테이블이나 메타테이블을 변경하는 동작까지 정적으로 보장하지는 않는다. 동적 생성자 호출도 필수 필드·알 수 없는 키·인자 개수·테이블 여부를 확인하지만 필드 값 전체를 런타임에서 타입 검사하는 것은 아니다.

## 모듈과 도구

모듈에서 `return {Player=Player, Named=Named}`로 내보내고 `import models from "models"`로 받으면 `models.Player {...}`, `local p: models.Player`, `class Device is models.Named`를 사용할 수 있다. import 그래프는 클래스·trait 계약과 타입 식별자를 함께 전달한다. 서로 다른 모듈의 동명 클래스는 다른 타입이다.

VS Code TextMate와 Zed Tree-sitter는 선언·필드·메서드·`is`·`Self`·명명 타입을 강조한다. LSP는 컴파일러의 동일한 타입 진단을 사용한다. 생성된 메서드와 기본값 식은 원본 위치에 연결되며 소스 맵·디버깅 번들에서도 원본 행을 유지한다. 자동 검증은 [컴파일러 검사](../crates/tynex-compiler/tests/class_traits.rs), [실행·모듈·오류 위치](../tests/test_classes.py), [LSP](../tests/test_lsp.py), 에디터 문법 테스트에 있다. 이 기능에 대한 실제 에디터 GUI 디버깅은 별도 검증 대상이다.

```sh
./bin/arky run examples/class.arky
./bin/arky run examples/traits.arky
python3 -m unittest discover -s tests -p test_classes.py -v
```

상속, trait 상속, 제네릭, 접근 제어, 사용자 정의 `init` 생성자, `.d.arky`의 class·trait 선언은 아직 지원하지 않는다. 선언 파일에는 기존 함수·구조적 테이블 계약을 사용한다. enum·match도 후속 기능이다. async 메서드는 [async/await 안내](async-await.md)를 따른다.
