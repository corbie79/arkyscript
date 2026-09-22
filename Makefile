.PHONY: setup check run test windows-sdk windows check-luajit-update update-luajit verify-luajit
setup:
	python3 scripts/bootstrap.py

check:
	./bin/tynex check examples/main.tlj

run:
	./bin/tynex run examples/main.tlj

test:
	cargo test --workspace --locked
	python3 -m unittest discover -s tests -v

windows-sdk:
	python3 scripts/build_windows.py --prepare-sdk

windows:
	python3 scripts/build_windows.py
	python3 scripts/build_task.py --windows

check-luajit-update:
	python3 scripts/update_luajit.py --check

update-luajit:
	python3 scripts/update_luajit.py --apply

verify-luajit:
	python3 scripts/update_luajit.py --verify

.PHONY: editor-test zed-extension
editor-test:
	npm --prefix editors/vscode test
	python3 scripts/test_editor_grammar.py

zed-extension:
	cargo build --locked -p tynex-cli
	python3 scripts/prepare_zed.py

.PHONY: binder-examples binder-test binder-sanitize binder-windows
binder-examples:
	python3 scripts/test_binder.py --examples

binder-test:
	python3 scripts/test_binder.py

binder-sanitize:
	python3 scripts/test_binder.py --sanitize

binder-windows:
	python3 scripts/test_binder.py --windows

.PHONY: task task-windows task-test
task:
	python3 scripts/build_task.py

task-windows:
	python3 scripts/build_task.py --windows

task-test: task
	python3 scripts/test_binder.py --tasks
	python3 -m unittest discover -s tests -p 'test_tasks.py' -v
