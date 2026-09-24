# Orb Jobs Agent

`orb-jobs-agent` provides infrastructure for remote job execution on an Orb. It receives jobs, manages execution and cancellation, and reports progress and results.

## Ownership boundary

This crate must remain infrastructure-only. Do not add new job handlers directly to `orb-jobs-agent`.

Implement new remote operations as ZOCI handlers in the service that owns the underlying functionality. Jobs without a local handler are routed to the owning service through `**/job/<command>`.

See the [ZOCI documentation](../zenorb/README.md#4-zoci---zenoh-orb-command-interface) for the handler convention and examples.

## Legacy handlers

All handlers currently defined in `src/handlers` and registered in `src/program.rs` are technical debt. They remain for compatibility and must not serve as precedent for new jobs.

Maintain these handlers only while they are still needed. When practical, migrate each handler to the service that owns its functionality and remove its local implementation from `orb-jobs-agent`.
