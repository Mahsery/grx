# grx

Search file contents and filesystem entries with inline query operators.

## Build

```sh
cargo install --path . --locked
```

## Queries

```sh
grx codex                              # Search contents
grx kind:dir codex                     # Find directory names
grx kind:file =memory.md               # Find an exact filename
grx t:rs memory                        # Search Rust contents
grx t:rs?py memory                      # Either filetype
grx font ns:# np:bak?zellij             # Exclude comment lines and either path
grx root:~/Projects t:rs                # Set the traversal root
```

Use `grx --help`, `grx --help-full`, or `grx --tutorial` for command help.

## License

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
