use eyre::{eyre, Result};
use tracing::info;
use url::Url;

fn main() -> Result<()> {
    // Example URL - replace with actual URL in production
    let url_str = "https://example.com/path/to/file";
    let url = Url::parse(url_str)?;
    
    // Download file directly into a memory file
    match update_agent_loader::download_to_memfile(&url) {
        Ok(mem_file) => {
            info!("Successfully downloaded file to memory file with fd: {}", mem_file.as_raw_fd());
            // Here you would use the memory file for further operations
            Ok(())
        }
        Err(e) => Err(eyre!("Failed to download file: {}", e)),
    }
}