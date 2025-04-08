# Update Agent Loader

A loader for the update-agent that downloads and executes a binary from a URL directly from memory without writing to disk.

## Usage

```
# Download and execute a binary from a URL
update-agent-loader --url https://example.com/path/to/executable

# Download and execute with arguments
update-agent-loader --url https://example.com/path/to/executable --args arg1 arg2 arg3

# Show help
update-agent-loader --help
```

## Features

- Downloads executables directly into memory
- Executes binaries without writing to disk using `fexecve`
- Supports passing arguments to the executed binary
- Uses secure TLS 1.3 with built-in root certificates