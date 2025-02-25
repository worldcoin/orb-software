// The URL must be HTTPS (or localhost) for WebTransport
// e.g., "https://192.168.0.100:443" or the appropriate path
const transportUrl = "https://localhost:1337";

const COMMAND_EVENT_NAME = "commandEvt";

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

// Function to get cursor position relative to canvas
function getCursorPosition(canvas, event) {
  // Get the bounding rectangle of the canvas
  const rect = canvas.getBoundingClientRect();
  const x = (event.clientX - rect.left) / rect.width;
  const y = (event.clientY - rect.top) / rect.height;
  return { x, y };
}

function setUpElements(canvas, positionDisplay) {
  // Style the canvas to make it visible
  canvas.style.border = "1px solid black";
  canvas.style.backgroundColor = "#f0f0f0";
  canvas.style.padding = "0";
  canvas.style.margin = "0";

  canvas.addEventListener("mousemove", function (event) {
    const position = getCursorPosition(canvas, event);
    const x = position.x.toFixed(2);
    const y = position.y.toFixed(2);
    positionDisplay.textContent = `Position: x=${x}, y=${y}`;
    const obj = {
      MouseEvent: {
        Move: {
          x: position.x,
          y: position.y,
        },
      },
    };
    const commandEvt = new CustomEvent(COMMAND_EVENT_NAME, {
      detail: obj,
    });
    canvas.dispatchEvent(commandEvt);
  });

  canvas.addEventListener("mouseleave", function () {
    positionDisplay.textContent = "Position: x=-, y=-";
    const obj = {
      MouseEvent: "Unfocus",
    };
    const commandEvt = new CustomEvent(COMMAND_EVENT_NAME, {
      detail: obj,
    });
    canvas.dispatchEvent(commandEvt);
  });
}

function encodeWithLengthPrefix(obj) {
  const jsonString = JSON.stringify(obj);

  // Convert JSON string to UTF-8 encoded bytes
  const encoder = new TextEncoder();
  const jsonBytes = encoder.encode(jsonString);

  // Create a buffer with enough space for the 32-bit length + JSON content
  const buffer = new ArrayBuffer(4 + jsonBytes.byteLength);

  // Create a view to write the 32-bit length prefix
  const view = new DataView(buffer);
  view.setUint32(0, jsonBytes.byteLength, false); // false = big endian

  // Create a view for the entire buffer
  const uint8View = new Uint8Array(buffer);

  // Copy the JSON bytes after the length prefix
  uint8View.set(jsonBytes, 4);

  return buffer;
}

async function main() {
  const canvas = document.getElementById("videoFrame");
  const ctx = canvas.getContext("2d");
  const positionDisplay = document.getElementById("position");

  setUpElements(canvas, positionDisplay);

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

  const controlStream = await transport.createUnidirectionalStream();
  const controlWriter = controlStream.getWriter();
  canvas.addEventListener(COMMAND_EVENT_NAME, async (evt) => {
    const obj = evt.detail;
    console.log("Got command event:", obj);
    const encodedBuffer = encodeWithLengthPrefix(obj);

    // Convert ArrayBuffer to Uint8Array if not already
    const dataToSend =
      encodedBuffer instanceof ArrayBuffer
        ? new Uint8Array(encodedBuffer)
        : encodedBuffer;
    await controlWriter.write(dataToSend);
    console.log("Data successfully sent");
  });

  const streamReader = transport.incomingUnidirectionalStreams.getReader();
  try {
    // Process each incoming stream
    for await (const stream of readerToAsyncIterable(streamReader)) {
      try {
        // Handle the incoming stream
        const blob = await handleIncomingStream(stream);

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
