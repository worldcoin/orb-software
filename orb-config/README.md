# orb-config

Android configuration daemon. Supply the full backend endpoint, Orb ID,
static token file, and optionally the local Zenoh router socket:

```sh
orb-config \
  --endpoint https://fleet.example.com/config \
  --orb-id P12345678 \
  --token-file /data/vendor/orb-config/token
```

The endpoint above is an example; use the dedicated Android endpoint once available.
Authentication uses HTTP Basic with the Orb ID as username and the file contents
as password. The file is read once at startup, without trimming or refreshing.
A missing or unreadable token file prevents startup.

The Zenoh session and configuration queryable are initialized before polling
starts. Session creation uses `zenorb`'s default startup retries, as in `connd`;
failure to initialize either prevents startup. The same session is reused while
running, with transport reconnection handled by Zenoh.

The response must be a JSON object. Each section is forwarded unchanged to the
exact `config/{section}` topic through `zenorb`, without an Orb ID prefix. The
default router socket is `/dev/socket/zenohd.sock`. Consumers validate their own
sections and supply their own defaults.

Only the daemon's `config` section is interpreted locally:

```json
{
  "config": {"poll_interval": {"secs": 60, "nanos": 0}},
  "home": {"example": true},
  "engine": {"example": true},
  "ota": {"example": true}
}
```

This produces independent `config/config`, `config/home`, `config/engine`, and
`config/ota` publications. Other section names work the same way; the daemon has
no service-specific types. Section names must be single, non-wildcard Zenoh key
segments. Invalid names are logged and skipped without blocking other sections.

The poll interval defaults to 60 seconds and accepts 1 second through 24 hours.
An invalid interval keeps the previous value. HTTP requests time out after
30 seconds.

ETags and their parsed JSON are cached together in memory. Fetch failures retain
the previous snapshot. Runtime failures are logged and retried at the next poll;
`Retry-After` can extend the delay, capped at 24 hours.

The ETag describes the whole response. After a changed response, each section is
compared as a JSON value with its last successful publication. Only new or changed
sections are published; changing `engine` does not republish `home` or `ota`.
HTTP 304 and identical JSON values cause no new publications. Failed publications
are retried at the next poll, including after a 304 or fetch failure. Omitted
sections are not published or deleted; consumers decide how to handle absence.

Consumers subscribe to their topic and query it on startup or reconnection to
retrieve the latest cached section (for example, a Zenoh `get("config/engine")`).
The daemon answers queries independently of backend availability. Publications
do not acknowledge consumer receipt and may be dropped during congestion;
consumers that need recovery should periodically query and avoid reapplying
identical values. Query replies and live updates can overlap, so consumers must
also handle that ordering when applying their initial configuration.

The cache is lost on daemon restart; consumers use their defaults until the first
successful fetch.
