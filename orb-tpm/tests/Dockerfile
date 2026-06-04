FROM python:3.11-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        tpm2-tools \
        libtss2-tcti-swtpm0 \
 && rm -rf /var/lib/apt/lists/*

RUN pip install --no-cache-dir cryptography

WORKDIR /app
COPY orb-attestation-test-server.py .

EXPOSE 8080
CMD ["python3", "orb-attestation-test-server.py", "--host", "0.0.0.0", "--port", "8080"]
