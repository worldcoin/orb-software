# zorb

CLI tool for Zenoh introspection and conditional command execution.

## Usage

```bash
zorb [--port <PORT>] [--orb-id <ORB_ID>] <COMMAND>
```

- port will default to `7447`
- orb id when not given as an arg will be read from the system using the `orb-info` crate

### `pub` - Publish Messages

```bash
zorb pub <KEYEXPR> <PAYLOAD>
```

**Example**:
```bash
zorb pub test "hello world"
```

### `sub` - Subscribe to Messages

```bash
zorb sub <KEYEXPR> [--type <TYPE>]
```

The `--type` flag enables rkyv deserialization for registered types.

**Example**:
```bash
zorb sub connd/net/changed -t orb_connd_events::Connection 
```

### `when` - Conditional Command Execution

```bash
zorb when <KEYEXPR> [--type <TYPE>] <COMMAND>
```

Executes a shell command when a message is received. Use `%s%` as a placeholder for the message content.

**Example:**
```bash
zorb when events/** echo %s% >> /tmp/events.log
```
