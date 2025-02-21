// The URL must be HTTPS (or localhost) for WebTransport
// e.g., "https://192.168.0.100:443" or the appropriate path
const transportUrl = "https://localhost:1337";

console.log("running script");

async function main() {
  const hash_response = await fetch("/cert_hash");
  if (!hash_response.ok) {
    throw new Error(`Response status: ${hash_response.status}`);
  }
  const hash = await hash_response.bytes();
  console.log("hash", hash);

  const https_url = new URL(window.location.origin);
  console.log(https_url);
  const wt_url = new URL(https_url);
  wt_url.port = 1337;
  console.log(wt_url);

  let transport;
  try {
    transport = new WebTransport(wt_url, {
      serverCertificateHashes: [{ algorithm: "sha-256", value: hash.buffer }],
    });
    await transport.ready;
    console.log("WebTransport connection is ready.");
  } catch (error) {
    console.error("Failed to create or initialize WebTransport:", error);
    return;
  }

  // Get a reader for incoming datagrams.
  const datagramReader = transport.datagrams.readable.getReader();

  const canvas = document.getElementById("videoFrame");
  const ctx = canvas.getContext("2d");

  try {
    // Continuously read from the datagram stream
    while (true) {
      const { value, done } = await datagramReader.read();
      if (done) {
        console.log("Datagram stream closed.");
        break;
      }

      // Convert the datagram (ArrayBuffer) into a Blob.
      // We assume each datagram is a complete JPEG image.
      const blob = new Blob([value], { type: "image/png" });
      // Create an image bitmap from the blob
      const bitmap = await createImageBitmap(blob);
      ctx.drawImage(bitmap, 0, 0);
    }
  } catch (readError) {
    console.error("Error while reading datagrams:", readError);
  }
}

await main();
