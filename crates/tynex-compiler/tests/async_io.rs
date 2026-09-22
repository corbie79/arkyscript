use std::collections::BTreeMap;
use tynex_compiler::{Mode, compile, declaration, prepare};

fn accepts(source: &str) {
    for mode in [Mode::Gradual, Mode::Strict] {
        compile(source, mode, true).unwrap_or_else(|errors| panic!("{mode}: {errors:?}\n{source}"));
    }
}
fn rejects(source: &str, code: &str) {
    for mode in [Mode::Gradual, Mode::Strict] {
        let errors = compile(source, mode, true).unwrap_err();
        assert!(
            errors.iter().any(|error| error.code == code),
            "{mode}: {errors:?}\n{source}"
        );
    }
}

#[test]
fn file_read_returns_one_typed_async_result() {
    accepts(
        "local task: Task<string> = fs.read_file('data.txt') local data: string = scheduler.run(task)",
    );
    accepts(
        "local async function load(path: string): string return await fs.read_file(path, 4096, 1000) end",
    );
    accepts(
        "local async function load(path: string): string return await fs.read_file(path, nil, 1000) end",
    );
    rejects("local data: string = fs.read_file('data.txt')", "T001");
    rejects(
        "local task: Task<number> = fs.read_file('data.txt')",
        "T001",
    );
    rejects(
        "local async function load(): number return await fs.read_file('data.txt') end",
        "T001",
    );
}
#[test]
fn file_read_checks_required_optional_and_extra_arguments() {
    rejects("fs.read_file()", "T006");
    rejects("fs.read_file(nil)", "T001");
    rejects("fs.read_file(123)", "T001");
    rejects("fs.read_file('data.txt', 'limit')", "T001");
    rejects("fs.read_file('data.txt', 4096, false)", "T001");
    rejects("fs.read_file('data.txt', 4096, 1000, 1)", "T006");
    accepts("fs.read_file('data.txt', nil, nil)");
}
#[test]
fn tcp_connect_returns_an_opaque_socket_task() {
    accepts(
        "local task: Task<Socket> = net.connect('localhost', 1234, 500) local socket: Socket = scheduler.run(task) local closed: boolean = socket:close()",
    );
    accepts(
        "local async function connect(): Socket return await net.connect('localhost', 1234) end",
    );
    rejects(
        "local socket: Socket = net.connect('localhost', 1234)",
        "T001",
    );
    rejects(
        "local task: Task<string> = net.connect('localhost', 1234)",
        "T001",
    );
    rejects("net.connect()", "T006");
    rejects("net.connect('localhost')", "T006");
    rejects("net.connect(false, 1234)", "T001");
    rejects("net.connect('localhost', '1234')", "T001");
    rejects("net.connect('localhost', 1234, 'timeout')", "T001");
    rejects("net.connect('localhost', 1234, 500, 1)", "T006");
    accepts("net.connect('localhost', 1234, nil)");
}
#[test]
fn socket_read_preserves_nullable_eof_and_requires_refinement() {
    accepts(
        "local async function read(socket: Socket): string? return await socket:read(4096, 1000) end",
    );
    accepts(
        "local async function read(socket: Socket): string local data = await socket:read(4096) if data == nil then return '' end return data end",
    );
    accepts(
        "local async function read(socket: Socket): number local data = await socket:read(4096) if data ~= nil then return #data end return 0 end",
    );
    rejects(
        "local async function read(socket: Socket): string return await socket:read(4096) end",
        "T001",
    );
    rejects(
        "local async function read(socket: Socket): number local data = await socket:read(4096) return #data end",
        "T001",
    );
    rejects(
        "local async function read(socket: Socket): string? return socket:read(4096) end",
        "T001",
    );
}
#[test]
fn socket_write_and_close_have_distinct_sync_async_contracts() {
    accepts(
        "local async function send(socket: Socket, data: string): number local count: number = await socket:write(data, 500) local closed: boolean = socket:close() return count end",
    );
    accepts(
        "local function send(socket: Socket): Task<number> return socket:write('hello', nil) end",
    );
    rejects(
        "local async function send(socket: Socket): string return await socket:write('hello') end",
        "T001",
    );
    rejects(
        "local async function close(socket: Socket): boolean return await socket:close() end",
        "T016",
    );
    rejects(
        "local function close(socket: Socket): Task<boolean> return socket:close() end",
        "T001",
    );
}
#[test]
fn socket_methods_check_self_arity_and_argument_types() {
    rejects(
        "local function read(socket: Socket): () socket:read() end",
        "T006",
    );
    rejects(
        "local function read(socket: Socket): () socket:read('4096') end",
        "T001",
    );
    rejects(
        "local function read(socket: Socket): () socket:read(4096, 'timeout') end",
        "T001",
    );
    rejects(
        "local function send(socket: Socket): () socket:write() end",
        "T006",
    );
    rejects(
        "local function send(socket: Socket): () socket:write(false) end",
        "T001",
    );
    rejects(
        "local function send(socket: Socket): () socket:write('hello', {}) end",
        "T001",
    );
    rejects(
        "local function close(socket: Socket): () socket:close(1) end",
        "T006",
    );
    rejects(
        "local function read(socket: Socket): () socket.read(4096) end",
        "T001",
    );
    accepts(
        "local function use(socket: Socket): Task<string?> local read = socket.read return read(socket, 4096, nil) end",
    );
    rejects(
        "local function use(socket: Socket): () local read = socket.read read({}, 4096) end",
        "T001",
    );
}
#[test]
fn opaque_sockets_cannot_be_forged_with_structural_records_or_classes() {
    rejects("local socket: Socket = {}", "T001");
    rejects(
        "local socket: Socket = {close = function(): boolean return true end}",
        "T001",
    );
    rejects(
        "class Fake function close(): boolean return true end end local socket: Socket = Fake {}",
        "T001",
    );
    rejects(
        "local function fake(socket: Socket): {close: function(): boolean} return socket end",
        "T001",
    );
    accepts(
        "local function same(socket: Socket): Socket local alias: Socket = socket return alias end",
    );
}
#[test]
fn socket_protocol_members_are_closed_and_immutable() {
    rejects(
        "local function mutate(socket: Socket): () socket.close = function(): boolean return true end end",
        "T017",
    );
    rejects(
        "local function mutate(socket: Socket): () socket['read'] = function(): () end end",
        "T017",
    );
    rejects(
        "local function mutate(socket: Socket): () socket.extra = true end",
        "T017",
    );
    rejects(
        "local function unknown(socket: Socket): () socket:unknown() end",
        "T017",
    );
    rejects(
        "local function unknown(socket: Socket): boolean return socket.closed end",
        "T017",
    );
    rejects(
        "local function unknown(socket: Socket, key: string): any return socket[key] end",
        "T017",
    );
    accepts("local function close(socket: Socket): boolean return socket['close'](socket) end");
}
#[test]
fn nullable_sockets_need_guards_before_io_and_optional_chaining_keeps_tasks_optional() {
    accepts(
        "local async function read(socket: Socket?): string? if socket == nil then return nil end return await socket:read(4096) end",
    );
    rejects(
        "local async function read(socket: Socket?): string? return await socket:read(4096) end",
        "T005",
    );
    accepts(
        "local async function read(socket: Socket?): string? local task: Task<string?>? = socket?:read(4096) if task == nil then return nil end return await task end",
    );
    rejects(
        "local async function read(socket: Socket?): string? return await socket?:read(4096) end",
        "T016",
    );
    accepts("local function close(socket: Socket?): boolean? return socket?:close() end");
}
#[test]
fn file_and_network_libraries_follow_normal_lexical_bindings() {
    accepts(
        "local fs = {read_file = function(n: number): boolean return n > 0 end} local result: boolean = fs.read_file(1)",
    );
    accepts(
        "local net = {connect = function(name: string): number return #name end} local result: number = net.connect('custom')",
    );
    accepts(
        "local read = fs.read_file local connect = net.connect local text: Task<string> = read('data.txt') local socket: Task<Socket> = connect('localhost', 1234)",
    );
    rejects("local read = fs.read_file read(false)", "T001");
    rejects(
        "local connect = net.connect connect('localhost', false)",
        "T001",
    );
}
#[test]
fn socket_types_work_inside_named_class_trait_and_task_contracts() {
    accepts(
        "trait Connected socket: function(Self): Socket end class Client is Connected connection: Socket function socket(): Socket return self.connection end async function echo(data: string): string? await self.connection:write(data) return await self.connection:read(4096) end end local async function make(): Client return Client {connection = await net.connect('localhost', 1234)} end",
    );
    accepts(
        "local async function identity(socket: Socket): Socket return socket end local function use(socket: Socket): Task<Socket> return identity(socket) end",
    );
    rejects("class Socket end", "T014");
    rejects("trait Socket end", "T014");
    accepts("local Socket = 'ordinary local' print(Socket)");
}
#[test]
fn declaration_files_accept_socket_at_every_contract_depth() {
    for source in [
        "export: Socket",
        "export: Socket?",
        "export: Task<Socket>",
        "export: {socket: Socket, sockets: {Socket}, connect: async function(string, number): Socket}",
    ] {
        declaration(source).unwrap();
    }
    assert_eq!(declaration("export: Socket").unwrap().to_string(), "Socket");
    assert_eq!(
        declaration("export: Task<Socket>").unwrap().to_string(),
        "Task<Socket>"
    );
    assert!(declaration("export: Task<Unresolved>").is_err());
}
#[test]
fn module_interfaces_preserve_opaque_socket_and_builtin_alias_contracts() {
    let exports = compile(
        "return {connect = net.connect, read_file = fs.read_file}",
        Mode::Strict,
        true,
    )
    .unwrap()
    .interface;
    let modules = BTreeMap::from([
        ("io_alias".into(), exports),
        (
            "existing".into(),
            declaration("export: {socket: Socket, open: async function(): Socket}").unwrap(),
        ),
    ]);
    prepare("import io from 'io_alias' import existing from 'existing' local connected: Task<Socket> = io.connect('localhost', 1234) local data: Task<string> = io.read_file('data.txt') local socket: Socket = existing.socket local flag: boolean = socket:close() local fresh: Socket = scheduler.run(existing.open())", true).unwrap().compile(Mode::Strict, &modules).unwrap();
    let errors = prepare(
        "import existing from 'existing' local wrong: string = scheduler.run(existing.open())",
        true,
    )
    .unwrap()
    .compile(Mode::Strict, &modules)
    .unwrap_err();
    assert!(errors.iter().any(|error| error.code == "T001"));
}

#[test]
fn builtin_io_library_aliases_preserve_readonly_members() {
    rejects(
        "fs.read_file = async function(path: string, maximum: number?, timeout: number?): string return '' end",
        "T017",
    );
    rejects(
        "local alias = fs alias.read_file = async function(path: string, maximum: number?, timeout: number?): string return '' end",
        "T017",
    );
    rejects(
        "local alias = net alias.connect = async function(host: string, port: number, timeout: number?): Socket return await net.connect(host, port, timeout) end",
        "T017",
    );
    rejects("local alias = fs alias.extra = true", "T017");
    rejects("local alias = net alias['missing']()", "T017");
    accepts("local alias = fs local result: Task<string> = alias.read_file('data.txt')");
    accepts(
        "local fs = {read_file = function(): number return 1 end} fs.read_file = function(): number return 2 end local n: number = fs.read_file()",
    );
}
#[test]
fn io_library_types_preserve_member_access_across_module_exports() {
    let exports = compile("return {fs = fs, net = net}", Mode::Strict, true)
        .unwrap()
        .interface;
    let modules = BTreeMap::from([("io_alias".into(), exports)]);
    prepare("import io from 'io_alias' local text: Task<string> = io.fs.read_file('data.txt') local socket: Task<Socket> = io.net.connect('localhost', 1234)", true).unwrap().compile(Mode::Strict, &modules).unwrap();
    let errors = prepare("import io from 'io_alias' io.fs.read_file = async function(path: string, maximum: number?, timeout: number?): string return '' end", true).unwrap().compile(Mode::Strict, &modules).unwrap_err();
    assert!(errors.iter().any(|error| error.code == "T017"));
}
