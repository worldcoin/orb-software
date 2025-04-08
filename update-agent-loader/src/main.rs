use eyre::{eyre, Result};
use tracing::info;
use url::Url;

fn main() -> Result<()> {
    // Example URL - replace with actual URL in production
    let url_str = "https://example.com/path/to/file";
    let url = Url::parse(url_str)?;
    
    match update_agent_loader::download_file_in_memory(&url) {
        Ok(data) => {
            info!("Successfully downloaded file, size: {} bytes", data.len());
            // Here you would do something with the downloaded data
            Ok(())
        }
        Err(e) => Err(eyre!("Failed to download file: {}", e)),
    }
}