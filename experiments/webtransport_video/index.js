// The URL must be HTTPS (or localhost) for WebTransport
// e.g., "https://192.168.0.100:443" or the appropriate path
const transportUrl = "https://localhost:1337";
const HASH = new Uint8Array([
  55, 203, 145, 244, 169, 222, 146, 152, 93, 240, 133, 47, 221, 106, 67, 189,
  151, 7, 215, 109, 57, 85, 104, 13, 222, 239, 103, 117, 226, 30, 50, 101,
]);

console.log("running script");

async function main() {
  let transport;
  try {
    transport = new WebTransport(transportUrl, {
      serverCertificateHashes: [{ algorithm: "sha-256", value: HASH.buffer }],
    });
    await transport.ready;
    console.log("WebTransport connection is ready.");
  } catch (error) {
    console.error("Failed to create or initialize WebTransport:", error);
    return;
  }

  // Get a reader for incoming datagrams.
  const datagramReader = transport.datagrams.readable.getReader();

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

      // Create a temporary Object URL to use as the image source.
      const imageUrl = URL.createObjectURL(blob);

      // Update the <img> element to display the received frame.
      const imgElement = document.getElementById("videoFrame");
      imgElement.src = imageUrl;

      // Optionally, revoke the old Object URL to free memory once the image is displayed.
      // But be careful with timing (ensure the image is loaded first if you do so).
      // URL.revokeObjectURL(previousImageUrl);
    }
  } catch (readError) {
    console.error("Error while reading datagrams:", readError);
  }
}

await main();
