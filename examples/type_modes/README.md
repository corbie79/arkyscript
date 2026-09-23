# 타입을 조금씩 도입하기

```sh
./bin/tynex run examples/type_modes/legacy.lua
./bin/tynex run examples/type_modes/gradual.arky
./bin/tynex run examples/type_modes/strict.arky
./bin/tynex check --project examples/type_modes/tynex.toml
```

각 파일은 `legacy 42`, `gradual 42`, `strict 42`를 탭으로 구분해 출력합니다. 같은 기능을 원본 Lua → 타입 없는 typed 파일 → 타입을 붙인 strict 파일 순서로 옮겼습니다. strict도 추론 가능한 모든 변수에 타입을 강제로 붙이지 않습니다.

다음 명령은 **의도적으로 실패**해야 합니다. gradual 파일의 매개변수 타입은 strict에서 결정할 수 없고, `type_error.arky`에는 명확한 타입 불일치가 있습니다.

```sh
./bin/tynex check --mode strict examples/type_modes/gradual.arky
./bin/tynex check examples/type_error.arky
```

설정과 파일 지시문이 겹치면 CLI `--mode` > 파일 지시문 > override > 프로젝트 기본값 순으로 적용됩니다. 기존 Lua에 계약만 붙이는 예제는 [imports/legacy.d.arky](../imports/legacy.d.arky)에 있습니다.
