#![forbid(unsafe_code)]

use std::path::PathBuf;

use clap::Parser;
use color_eyre::Result;
use eyre::WrapErr as _;
use orb_camera_hmac_ca::{optee::OpteeBackend, Client};
use orb_camera_hmac_proto::{EmblParams, GetRowStartRequest, VerifyHmacRequest};

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    args.run()
}

#[derive(Debug, Parser)]
enum Args {
    ProvisionKey(ProvisionKeyArgs),
    GetRowStart(GetRowStartArgs),
    VerifyHmac(VerifyHmacArgs),
    Version(VersionArgs),
}

impl Args {
    fn run(self) -> Result<()> {
        match self {
            Self::ProvisionKey(args) => args.run(),
            Self::GetRowStart(args) => args.run(),
            Self::VerifyHmac(args) => args.run(),
            Self::Version(args) => args.run(),
        }
    }
}

#[derive(Debug, Parser)]
struct ProvisionKeyArgs {
    /// 32-byte Pre-Provisioned Key as 64 hex characters.
    #[clap(long)]
    ppk: String,
}

impl ProvisionKeyArgs {
    fn run(self) -> Result<()> {
        let ppk = parse_hex_bytes::<32>(&self.ppk)
            .wrap_err("failed to parse --ppk as 32 hex bytes")?;
        let mut client = make_client()?;
        client.provision_key(ppk)?;
        println!("PPK provisioned successfully");
        Ok(())
    }
}

#[derive(Debug, Parser)]
struct GetRowStartArgs {
    /// 6-byte sensor UID (registers EE44-EE49) as 12 hex characters.
    #[clap(long)]
    uid: String,
    /// 16-byte NONCE (registers EB40-EB4F) as 32 hex characters.
    #[clap(long)]
    nonce: String,
    /// 8-byte frame counter as 16 hex characters (big-endian).
    #[clap(long)]
    frame_num: String,
    /// Horizontal pixel count (e.g. 2592).
    #[clap(long)]
    hsize_raw: u32,
    /// Total row count including embedding lines (e.g. 1952).
    #[clap(long)]
    vsize: u32,
    #[clap(flatten)]
    embl: EmblArgs,
}

impl GetRowStartArgs {
    fn run(self) -> Result<()> {
        let uid = parse_hex_bytes::<6>(&self.uid)
            .wrap_err("failed to parse --uid as 6 hex bytes")?;
        let nonce = parse_hex_bytes::<16>(&self.nonce)
            .wrap_err("failed to parse --nonce as 16 hex bytes")?;
        let frame_num = parse_hex_bytes::<8>(&self.frame_num)
            .wrap_err("failed to parse --frame-num as 8 hex bytes")?;

        let mut client = make_client()?;
        let response = client.get_row_start(GetRowStartRequest {
            uid,
            nonce,
            frame_num,
            hsize_raw: self.hsize_raw,
            vsize: self.vsize,
            embl: self.embl.into(),
        })?;
        println!("sr2h={}", response.sr2h);
        Ok(())
    }
}

#[derive(Debug, Parser)]
struct VerifyHmacArgs {
    /// 6-byte sensor UID (registers EE44-EE49) as 12 hex characters.
    #[clap(long)]
    uid: String,
    /// 16-byte NONCE (registers EB40-EB4F) as 32 hex characters.
    #[clap(long)]
    nonce: String,
    /// Path to file containing the processed pixel data (binary).
    #[clap(long)]
    src_data: PathBuf,
    /// 32-byte HMAC extracted from the frame tail as 64 hex characters.
    #[clap(long)]
    embedded_hmac: String,
}

impl VerifyHmacArgs {
    fn run(self) -> Result<()> {
        let uid = parse_hex_bytes::<6>(&self.uid)
            .wrap_err("failed to parse --uid as 6 hex bytes")?;
        let nonce = parse_hex_bytes::<16>(&self.nonce)
            .wrap_err("failed to parse --nonce as 16 hex bytes")?;
        let embedded_hmac = parse_hex_bytes::<32>(&self.embedded_hmac)
            .wrap_err("failed to parse --embedded-hmac as 32 hex bytes")?;
        let src_data = std::fs::read(&self.src_data)
            .wrap_err("failed to read --src-data file")?;

        let mut client = make_client()?;
        let response = client.verify_hmac(VerifyHmacRequest {
            uid,
            nonce,
            src_data,
            embedded_hmac,
        })?;

        if response.valid {
            println!("HMAC valid");
        } else {
            println!("HMAC invalid");
        }

        Ok(())
    }
}

#[derive(Debug, Parser)]
struct VersionArgs;

impl VersionArgs {
    fn run(self) -> Result<()> {
        let mut client = make_client()?;
        let version = client.version()?;
        println!("{version}");
        Ok(())
    }
}

#[derive(Debug, Parser)]
struct EmblArgs {
    #[clap(long, default_value = "2")]
    pre_sei: u32,
    #[clap(long, default_value = "2")]
    post_sei: u32,
    #[clap(long, default_value = "2")]
    pre_ovi: u32,
    #[clap(long, default_value = "2")]
    post_ovi: u32,
    #[clap(long, default_value = "0")]
    sta: u32,
}

impl From<EmblArgs> for EmblParams {
    fn from(a: EmblArgs) -> Self {
        Self {
            pre_sei: a.pre_sei,
            post_sei: a.post_sei,
            pre_ovi: a.pre_ovi,
            post_ovi: a.post_ovi,
            sta: a.sta,
        }
    }
}

fn make_client() -> Result<Client<OpteeBackend>> {
    let mut ctx =
        optee_teec::Context::new().wrap_err("failed to create optee context")?;
    Client::new(&mut ctx).wrap_err("failed to create camera-hmac client")
}

fn parse_hex_bytes<const N: usize>(s: &str) -> Result<[u8; N]> {
    let s = s.trim();
    eyre::ensure!(
        s.len() == N * 2,
        "expected {} hex characters, got {}",
        N * 2,
        s.len()
    );
    let mut out = [0u8; N];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = hex_digit(chunk[0])?;
        let lo = hex_digit(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_digit(b: u8) -> Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(eyre::eyre!("invalid hex character: {}", b as char)),
    }
}
