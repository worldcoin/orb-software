use tokio::sync::OnceCell;
use reqwest::{
    Client,
    Certificate,
};
use futures::TryFutureExt;

const AMAZON_ROOT_CA_1_PEM: &[u8] = include_bytes!("../ca-certificates/Amazon_Root_CA_1.pem");
const GTS_ROOT_R1_PEM: &[u8] = include_bytes!("../ca-certificates/GTS_Root_R1.pem");

static AMAZON_ROOT_CA_1_CERT: OnceCell<Certificate> = OnceCell::const_new();
static GTS_ROOT_R1_CERT: OnceCell<Certificate> = OnceCell::const_new();

static INSTANCE: OnceCell<Client> = OnceCell::const_new();

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("failed initializing HTTP client")]
    BuildClient(#[source] reqwest::Error),
    #[error("failed creating x509 certificate for AMAZON_ROOT_CA_1 from PEM bytes")]
    CreateAmazonRootCa1Cert(#[source] reqwest::Error),
    #[error("failed creating x509 certificate for GTS_ROOT_R1 from PEM bytes")]
    CreateGtsRootR1Cert(#[source] reqwest::Error),
}

pub async fn get() -> Result<&'static Client, Error> {
    INSTANCE.get_or_try_init(initialize).await
}

async fn initialize() -> Result<Client, Error> {
    let amazon_cert = AMAZON_ROOT_CA_1_CERT
        .get_or_try_init(|| async {Certificate::from_pem(AMAZON_ROOT_CA_1_PEM)})
        .map_err(Error::CreateAmazonRootCa1Cert).await?
        .clone();
    let google_cert = GTS_ROOT_R1_CERT
        .get_or_try_init(|| async {Certificate::from_pem(GTS_ROOT_R1_PEM)})
        .map_err(Error::CreateGtsRootR1Cert).await?
        .clone();
    Client::builder()
        .add_root_certificate(amazon_cert)
        .add_root_certificate(google_cert)
        .tls_built_in_root_certs(false)
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(Error::BuildClient)
}
