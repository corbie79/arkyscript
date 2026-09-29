# 분리된 trait 구현과 네이티브 타입 선언

`impl Trait for Type`은 클래스 정의와 trait 구현을 분리합니다. 스크립트 클래스와 C++ 바인더가 노출한 네이티브 클래스에 사용할 수 있습니다. 구현은 모듈 최상위에 작성하며, 타입과 trait은 먼저 선언하거나 import해야 합니다.

## 스크립트 클래스에 나중에 구현하기

```lua
class Player
   name: string
end

trait Named
   label: function(Self): string

   function describe(): string
      return "name: " .. self:label()
   end
end

impl Named for Player
   function label(): string
      return self.name
   end
end

local function show(value: Named): ()
   print(value:describe())
end
show(Player {name = "Mina"})
```

```sh
./bin/arky run examples/impl.arky
```

출력은 `name: Mina`입니다. 클래스는 먼저 정의해 두고 구현을 별도 블록에서 추가할 수 있습니다. 구현 등록 전에 만든 인스턴스도 등록 이후에는 메서드를 사용할 수 있습니다. 구현 전에 그 메서드를 호출하는 코드는 검사에서 거절합니다.

기존 `class Player is Named`도 계속 지원합니다. 같은 타입에 같은 trait을 두 방식으로 중복 구현할 수는 없습니다.

## 계약과 충돌 규칙

- impl의 메서드는 trait에 선언되어 있어야 합니다. 임의의 필드·보조 메서드를 추가하지 않습니다.
- 필요한 메서드는 impl, 기존 클래스 메서드 또는 trait 기본 구현으로 충족합니다. 매개변수·반환형·Self 계약을 검사합니다.
- impl에서 기존 클래스·네이티브 메서드·프로퍼티를 덮어쓸 수 없습니다. 이미 있는 메서드가 계약을 만족하면 impl 본문에서 생략합니다.
- 기존 클래스의 명시적 메서드는 trait의 기본 구현보다 우선합니다. 앞선 trait 구현에서 얻은 기본 메서드와 새 기본 메서드가 충돌하면 오류입니다.
- 같은 타입·trait의 중복 구현과 여러 모듈에서 추가한 동명 확장 메서드는 오류입니다. 마지막 import가 앞선 구현을 덮어쓰는 규칙은 없습니다.
- 타입 또는 trait을 정의한 모듈에서만 구현합니다. 외부 타입에 자신이 정의한 trait을 붙일 수 있지만, 둘 다 외부에서 가져와 임의로 구현할 수는 없습니다.
- 함수·조건문·반복문·do 블록 안의 impl은 허용하지 않습니다. 구현을 제공하는 모듈도 최상위에서 import 또는 정적 require해야 합니다.
- 다른 모듈의 구현을 사용할 때는 해당 모듈을 먼저 가져옵니다. 지연 import를 실행하기 전에 확장 메서드가 있다고 가정하지 않습니다.

본문에서는 `self`와 `Self`, async 메서드, 다중 반환을 사용할 수 있습니다. 재귀·전방 메서드 호출에는 반환형을 명시합니다. trait 인자를 통한 Self 사용 제한은 [기존 trait 규칙](classes-traits.md)을 따릅니다.

## 네이티브 선언 파일

`engine.lua` 같은 호스트 연결 모듈 옆에 `engine.d.arky`를 둡니다.

```text
declare class Node
   x: number
   move: function(Self, number): ()
end

declare class Sprite extends Node
   constructor: function(): Self
   texture: number
end

declare submit: function(Node): ()
```

- `declare class`는 모듈이 반환하는 테이블의 클래스 팩토리 필드를 선언합니다. C++ 등록 이름과 대응시켜야 합니다.
- `extends`는 먼저 선언한 네이티브 부모 하나를 지정합니다. 부모 필드와 메서드를 사용할 수 있고 자식 값을 부모 인자로 전달할 수 있습니다. 역변환은 허용하지 않습니다.
- `constructor: function(...): Self`는 클래스 팩토리의 호출 계약입니다. 선언하지 않으면 해당 클래스를 직접 생성할 수 없습니다. 부모 생성자 계약은 상속하지 않습니다.
- 메서드는 첫 인자 타입에 `Self`를 씁니다. `move: function(Self, number): ()`는 인스턴스의 `:move(number)`에 대응합니다.
- 필드는 `x: number`, 읽기 전용 프로퍼티는 `readonly id: number`로 선언합니다. 상속받은 멤버를 다시 선언하거나 덮어쓰면 오류입니다.
- 현재 클래스당 하나의 생성자 시그니처를 선언할 수 있습니다. 오버로드, trait 선언, 전역 ambient 선언, 정적 메서드는 제공하지 않습니다.
- 일반 `declare 함수: function(...)`에서 앞서 선언한 네이티브 이름을 사용할 수 있습니다. `export:` 형식과 섞지 않습니다.

선언 파일은 **호스트 구현이 지킨다고 신뢰하는 계약**입니다. C++ 함수의 실제 매개변수 타입이나 필드 값을 자동 추출·대조하지 않습니다. 정적 검사와 별도로 바인더가 실행 시 객체 타입·등록 관계·const·멤버 충돌을 확인합니다. 잘못된 선언을 자동으로 안전하게 만드는 기능은 아닙니다.

## 네이티브 객체에 trait 구현하기

```lua
import engine from "engine"

trait Named
   label: function(Self): string
end

impl Named for engine.Sprite
   function label(): string
      return "sprite " .. self.texture
   end
end

local sprite = engine.Sprite()
sprite.texture = 7
sprite:move(10)
print(sprite:label())
engine.submit(sprite)
```

C++ 객체는 그대로 유지됩니다. 스크립트 메서드는 별도 확장 표에 들어가며 네이티브 등록 메서드나 가상 함수 포인터를 교체하지 않습니다. 메서드의 self는 원래 userdata이므로 네이티브 함수에 전달할 때 추가 래퍼를 벗길 필요가 없습니다. 수명과 shared_ptr 소유권도 기존 바인딩 규칙을 따릅니다.

부모 네이티브 타입에 추가한 구현은 등록된 자식에서도 사용할 수 있습니다. 새 메서드가 이미 존재하는 자식 멤버와 충돌하면 정적 검사 또는 호스트 등록 검사에서 거절합니다. 호스트에만 존재하고 선언 파일에 누락된 멤버 충돌도 실행 시 거절합니다.

## C++ 호스트에서 실행하기

[네이티브 예제](../examples/native_traits/main.arky)는 실제 C++ Node·Sprite에 연결하는 예제입니다. `engine.lua`는 호스트가 제공한 `arky_engine` 테이블을 반환합니다. 일반 CLI에서 단독 실행하려면 이 호스트 객체가 필요합니다.

```sh
./bin/arky check examples/native_traits/main.arky
make binder-examples
./bin/arky gen examples/native_traits/main.arky > build/native-traits.lua
revision=$(python3 -c 'import json; print(json.load(open("dependencies.json"))["luajit"]["revision"])')
build/binder/native/"$revision"/cpp_rendering build/native-traits.lua
```

마지막 실행은 다음을 출력합니다.

```text
name: sprite 7
Native render queue: 1 commands
```

호스트 예제는 생성 코드에 이 모듈 전용 import 로더를 주입합니다. 제품 엔진에서는 프로젝트 로더와 동일한 모듈 대응 관계를 유지해야 합니다. async 메서드도 사용하는 호스트는 기존 [async 런타임](async-await.md)을 함께 연결합니다.

## 구현 경계

스크립트 class 상속, 스크립트에서 C++ 가상 메서드 재정의, 네이티브 메모리 레이아웃에 스크립트 필드 추가는 포함하지 않습니다. 상태는 합성으로 구성하고, 렌더링의 컬링·배칭·GPU 명령 제출은 네이티브에 둡니다.

검증은 [컴파일러 테스트](../crates/tynex-compiler/tests/impl_native.rs), [실행·모듈·실제 호스트 통합 테스트](../tests/test_impl.py), [C++ 상속·확장 충돌 테스트](../tests/binder/inheritance.cpp)에 있습니다. 에디터 문법에도 impl·네이티브 선언을 반영했습니다.
