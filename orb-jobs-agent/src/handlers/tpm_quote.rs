use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use color_eyre::{eyre::bail, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use serde::{Deserialize, Serialize};

/// Input format (JSON args):
///
/// ```text
/// tpm_quote {"nonce":"<base64-encoded 32-byte nonce>"}
/// ```
///
/// Or positional:
///
/// ```text
/// tpm_quote <base64-encoded 32-byte nonce>
/// ```
#[derive(Debug, Deserialize)]
struct TpmQuoteArgs {
    nonce: String,
}

/// JSON response returned in `std_out` of the `JobExecutionUpdate`.
#[derive(Debug, Serialize)]
struct TpmQuoteResponse {
    /// Base64 echo of the input nonce for caller correlation.
    nonce: String,
    /// Base64-encoded `TPM2B_ATTEST` structure.
    quoted: String,
    /// Base64-encoded `TPMT_SIGNATURE` structure.
    signature: String,
    /// Base64-encoded DER AIK certificate (leaf first).
    aik_cert: String,
}

/// Decode and validate the nonce from either JSON or positional args.
fn decode_nonce(ctx: &Ctx) -> Result<Vec<u8>> {
    let nonce_b64 = if let Ok(args) = ctx.args_json::<TpmQuoteArgs>() {
        args.nonce
    } else {
        let args = ctx.args();
        args.into_iter().next().unwrap_or_default()
    };

    if nonce_b64.is_empty() {
        bail!("tpm_quote: nonce argument is required");
    }

    BASE64
        .decode(nonce_b64.trim())
        .map_err(|e| color_eyre::eyre::eyre!("tpm_quote: nonce is not valid base64: {e}"))
}

/// Handler for the `tpm_quote` job.
///
/// Accepts a base64-encoded 32-byte nonce, delegates to [`orb_tpm::quote`],
/// and returns a JSON object with the raw TPM structures base64-encoded.
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let nonce_bytes = decode_nonce(&ctx)?;

    tracing::info!(
        job_execution_id = %ctx.execution_id(),
        nonce_len = nonce_bytes.len(),
        "tpm_quote: invoking orb_tpm::quote",
    );

    let result = tokio::task::spawn_blocking({
        let nonce = nonce_bytes.clone();
        move || orb_tpm::quote(&nonce)
    })
    .await
    .map_err(|e| color_eyre::eyre::eyre!("tpm_quote: task panicked: {e}"))??;

    let response = TpmQuoteResponse {
        nonce: BASE64.encode(&nonce_bytes),
        quoted: BASE64.encode(&result.quoted),
        signature: BASE64.encode(&result.signature),
        aik_cert: BASE64.encode(&result.aik_cert_der),
    };

    let body = serde_json::to_string(&response)
        .map_err(|e| color_eyre::eyre::eyre!("tpm_quote: failed to serialize response: {e}"))?;

    Ok(ctx.success().stdout(body))
}
