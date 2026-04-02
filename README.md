# depends-on-rs

Declarative process orchestration for integration tests.

- [What It Does](#what-it-does)
- [Feature Set](#feature-set)
- [Config Example](#config-example)
- [CLI](#cli)
- [Library API](#library-api)
- [Current Limits](#current-limits)

---

## What It Does

`depends-on-rs` starts external processes in dependency order, waits for readiness, runs your test or command, and kills everything on cleanup.

It is aimed at the same class of problem as `depends-on-go`:

1. databases
2. local services
3. migrations
4. server-client integration tests
5. process graphs that are too annoying to babysit by hand

## Feature Set

1. JSON config with named targets.
2. DAG validation and cycle rejection.
3. Environment expansion for command args and env values.
4. Readiness strategies:
   - TCP port
   - exit code
   - log pattern
   - immediate readiness
5. Process-group cleanup on stop.
6. Library API and small CLI.
7. Stdout and stderr sinks:
   - inherit
   - null
   - file

## Config Example

```json
{
  "server": {
    "cmd": ["python3", "-m", "http.server", "8080"],
    "wait_for": {"port": 8080, "timeout": "10s"},
    "fds": {
      "stdout": "file:logs/server.out",
      "stderr": "file:logs/server.err"
    }
  },
  "migrate": {
    "cmd": ["sh", "-c", "exit 0"],
    "wait_for": {"exit_code": 0, "timeout": "5s"}
  },
  "tests": {
    "cmd": ["sh", "-c", "echo run tests"],
    "depends": ["server", "migrate"],
    "wait_for": {"exit_code": 0, "timeout": "30s"}
  }
}
```

## CLI

Start targets and block until interrupted:

```bash
depends-on-rs start --config dependencies.json server tests
```

Start targets, run a command, then clean up:

```bash
depends-on-rs run --config dependencies.json --targets server migrate -- cargo test
```

## Library API

```rust
use depends_on_rs::Manager;

let manager = Manager::load("dependencies.json")?;
let handle = manager.start(&["server".to_string(), "migrate".to_string()])?;

// run your assertions here

drop(handle); // stops everything
```

## Current Limits

1. `fds` currently supports `stdin`, `stdout`, and `stderr`.
2. `pipe:` wiring is only implemented for `stdin` sourcing from another target's `stdout` or `stderr`.
3. Generic extra descriptors such as `fd3` are not implemented yet.

That is intentional. The useful part is reliable orchestration, not an elaborate shrine to file descriptors.
