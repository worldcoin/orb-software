use color_eyre::Result;
use orb_blob_transfer::BlobNode;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let node = BlobNode::start("data-a").await?;
    // You need to have this file in the folder you're running the example from
    let ticket = node.import(Path::new("ota_v1.txt")).await?;
    println!("Imported ota_v1.txt, Ticket: \n {ticket}");

    let node2 = BlobNode::start("data-b").await?;

    println!("Fetching imported data...");

    // Just needed any abs path, so used /tmp
    node2
        .fetch(ticket, Path::new("/tmp/my_downloaded_ota.txt"))
        .await?;
    println!("Download complete!");

    Ok(())
}
