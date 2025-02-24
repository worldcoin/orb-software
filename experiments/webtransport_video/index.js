// The URL must be HTTPS (or localhost) for WebTransport
// e.g., "https://192.168.0.100:443" or the appropriate path
const transportUrl = "https://localhost:1337";

console.log("running script");

// Helper function to convert a reader to an async iterable
async function* readerToAsyncIterable(reader) {
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) return;
      yield value;
    }
  } finally {
    reader.releaseLock();
  }
}

// Function to handle incoming unidirectional streams
async function handleIncomingStream(stream) {
  // Create a reader for the incoming stream
  const reader = stream.getReader();
  const chunks = [];

  try {
    // Read all chunks from the stream
    for await (const chunk of readerToAsyncIterable(reader)) {
      chunks.push(chunk);
    }
  } catch (error) {
    console.error("Error reading stream:", error);
    throw error;
  } finally {
    reader.releaseLock();
  }

  // Combine all chunks into a single Blob
  const blob = new Blob(chunks, {
    type: "image/png",
  });

  return blob;
}

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

  const canvas = document.getElementById("videoFrame");
  const ctx = canvas.getContext("2d");

  const streamReader = transport.incomingUnidirectionalStreams.getReader();
  try {
    // Process each incoming stream
    for await (const stream of readerToAsyncIterable(streamReader)) {
      try {
        // Handle the incoming stream
        const blob = await handleIncomingStream(stream);
        // Now you can use the blob
        console.log("Received blob size:", blob.size);

        // Create an image bitmap from the blob
        const bitmap = await createImageBitmap(blob);
        ctx.drawImage(bitmap, 0, 0);
      } catch (error) {
        console.error("Error handling stream:", error);
      }
    }
  } catch (error) {
    console.error("Error accepting stream:", error);
  } finally {
    streamReader.releaseLock();
  }
}

await main();
