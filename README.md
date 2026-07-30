# ula

> **very WIP** — a prototype; interfaces and implementation may change at any time.

ula is a **light and extensive** universal agent.

## Features

- **PTC based on Luau**
- **Tools as plain directories**
- **Process isolation**

## Installing

```sh
cargo install --git https://github.com/lwb-2021/ula-monorepo --locked ula-compose ula-core ula-ptc ula-tui
```

or with Nix:

```sh
nix profile add github:lwb-2021/ula-monorepo
```

Configuration is still manual — copy `crates/ula-compose/example-config.yaml` and `crates/ula-core/example-config.yaml`, then fill them in. A friendlier way to run it is under development.

## Adding a tool

Create `~/.agents/tools/<name>/` with two files and make the script executable:

```sh
~/.agents/tools/wc/
├── main.sh
└── meta.json
```

`main.sh` — parameters arrive as plain arguments, in declaration order; defaults from `meta.json` are filled in by the host. The working directory is the tool's own directory unless `inherit_cwd` is set to `true`.

```sh
#!/bin/sh
wc "$1"
```

`meta.json` — name, description and the parameter list the model sees:

```json
{
  "name": "WordCount",
  "description": "Count lines, words and bytes of a file",
  "parameters": ["file: string"],
  "exec": "main.sh"
}
```

```sh
chmod +x ~/.agents/tools/wc/main.sh
```


## License

`crates/ula-protocol` is MIT; everything else is GPL-3.0-or-later.

