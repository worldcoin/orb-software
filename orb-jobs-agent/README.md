# Orb Jobs Agent (orb-jobs-agent)

The Orb Jobs Agent is a process to provide remote execution of prescribed commands. Commands are invoked by incoming requests and the
functionality of the command is provided completely by the Orb implementation.

### Taxonomy

- `orb-jobs-agent`: A systemd service that runs on the orb, executing a `JobExecution`(s) sent by `fleet-cmdr`
- `fleet-cmdr`: A backend service owned by the Fleet Management team that sends `JobExecution`(s) to the orb. 
- `JobNotify`: A notification sent from `fleet-cmdr` to the `orb-jobs-agent` that new jobs are pending.
- `JobRequestNext`: A request from the `orb-jobs-agent` to the `fleet-cmdr` to send the next `JobExecution`.
- `JobExecution`: A specific command to execute by the current Orb
- `JobExecutionUpdate`: A result from the `orb-jobs-agent` describing the outcome or progress of a command execution
- `JobCancel`: A cancellation request to stop a running job in `orb-jobs-agent`.

### fleet-cmdr's role from the orb-jobs-agent perspective
- `fleet-cmdr` provides an interface for users to enqueue new jobs to its internal queue
- when a new job is enqueued in `fleet-cmdr`, it sends a `JobNotify` to `orb-jobs-agent`
- when `fleet-cmdr` receives a `JobRequestNext` from `orb-jobs-agent`, it sends the first job (that is not already 
in progress) from its queue with `JobExecution`
- when `fleet-cmdr` receives a `JobExecutionUpdate` from `orb-jobs-agent`, it removes that job from its internal queue

### orb-jobs-agent role
- on startup, `orb-jobs-agent` requests `fleet-cmdr` for a new job with `JobRequestNext`
- when `orb-jobs-agent` receives a `JobNotify` from `fleet-cmdr` it requests a new job with a `JobRequestNext`
- when `orb-jobs-agent` receives a `JobExecution` from `fleet-cmdr` it executes a job
- on completion of a job, `orb-jobs-agent` reports back to `fleet-cmdr` with a `JobExecutionUpdate`
- on completion of a job, `orb-jobs-agent` requests `fleet-cmdr` for a new job with a `JobRequestNext`

### notes
- jobs can be cancelled
- some jobs can be run in parallel, other jobs not

---

## tpm_quote handler

### Purpose

Provides remote TPM attestation: the caller supplies a fresh 32-byte nonce (base64-encoded) and receives a TPM2 quote (TPM2B_ATTEST + TPMT_SIGNATURE) together with the AIK certificate so the caller can verify the quote's authenticity and the PCR values were not tampered.

### Command format

JSON style (preferred):
```
tpm_quote {"nonce":"<base64url-or-standard 32-byte nonce>"}
```

Positional style:
```
tpm_quote <base64-encoded 32-byte nonce>
```

### Response (JSON)

```json
{
  "nonce":     "<base64 echo of the input nonce>",
  "quoted":    "<base64 TPM2B_ATTEST>",
  "signature": "<base64 TPMT_SIGNATURE>",
  "aik_cert":  "<PEM AIK certificate chain>"
}
```

> **Current status:** stub — `quoted`, `signature`, and `aik_cert` are empty strings until the real TPM backing is wired in.

---

## TPM quoting implementation: `tpm2_quote` binary vs native TSS ESAPI

### Option A — subprocess: `tpm2_quote` binary

The handler calls `tpm2-tools`' `tpm2_quote` via `ctx.deps().shell.exec(...)`, parses the output files it writes to disk, and returns them.

**Pros**
- Zero additional Rust dependencies.
- Already available on systems with `tpm2-tools` installed.
- Quickest path to a working prototype.

**Cons**
- Requires `tpm2_quote` on `$PATH` at runtime — a hidden runtime dep not tracked by Cargo.
- Output is written to temp files / YAML that must be parsed and re-serialised — fragile and wasteful.
- Extra process spawn and tempfile I/O adds latency measurable in ~50–200 ms.
- No compile-time guarantees on correct TPM structure handling.
- Error messages from the subprocess are unstructured text — hard to act on programmatically.

### Option B — native: `tss-esapi` Rust crate (TSS2 ESAPI bindings)

The handler opens a TSS2 ESAPI context directly in-process via the [`tss-esapi`](https://github.com/parallaxsecond/rust-tss-esapi) crate (the same TSS2 stack that Keylime uses internally).

**Pros**
- In-process: no subprocess, no tempfiles, no PATH dependency.
- Type-safe: `TPM2B_ATTEST`, `TPMT_SIGNATURE`, PCR selection — all are Rust types verified at compile time.
- Richer error handling: TSS2 return codes map to typed errors.
- ~10× lower latency than subprocess (no fork/exec overhead, no disk I/O).
- `tss-esapi` is the de-facto standard for Rust TPM work; actively maintained; used by Keylime, Parsec, and the Confidential Containers project.

**Cons**
- Adds a native C dependency: `libtss2-esys`, `libtss2-tcti-*` (available as packages on Debian/Ubuntu).
- Cross-compilation requires the TSS2 libraries in the sysroot — manageable via Nix/Docker build environments already in use.
- More boilerplate for initial AIK provisioning (create primary, create AIK, persist handle).

### Can we reuse Keylime's implementation?

Keylime's agent (`keylime-agent` Rust crate, formerly Python) calls TSS2 ESAPI directly. Its quoting path lives in `keylime/src/tpm.rs` and does roughly:

1. `EsapiContext::new()` → open ESAPI session
2. Load AK (Attestation Key) from NVRAM or create + certify it
3. `ctx.quote(ak_handle, pcr_selection, nonce)?` → `(TPM2B_ATTEST, TPMT_SIGNATURE)`
4. Return both blobs + AK certificate

**What can be reused:**
- The high-level quoting logic and PCR selection patterns are straightforward to port (50–100 lines). It is **not** worth taking Keylime as a library dependency — it pulls in a large agent framework, an IMA event-log verifier, and a registrar client, none of which are needed here.
- The `tss-esapi` crate itself is the shared foundation; both Keylime and this handler would use the same API.

**Recommendation:** write a thin purpose-built wrapper (`tpm.rs`) using `tss-esapi` directly. Model the structure after Keylime's `tpm.rs` for the quoting call, but keep it self-contained at ~100–150 lines. This avoids Keylime's heavy transitive dependency tree while adopting the same battle-tested API path.

### Recommended Cargo deps to add when implementing

```toml
# Cargo.toml (workspace)
tss-esapi = { version = "7", features = ["integration-tests"] }  # or pin to exact version
```

The `Tcti` transport can be selected at runtime via the `TSS2_TCTI` environment variable (device `/dev/tpmrm0`, mssim for testing, etc.) — same mechanism used by `tpm2-tools` and Keylime.
