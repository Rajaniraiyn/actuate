# Vendored pairing patch

Source: https://github.com/poapoauu/DroidMux at d21e3d729a9d233fd34ce27e5c2244c45c4c9a8e. MIT OR Apache-2.0, license files preserved. Other DroidMux packages remain pinned git dependencies.

Local changes:

- Allow bounded printable ASCII QR secrets as well as six-digit pairing codes. No cryptographic primitive changes.
- Add `connect_paired_device_with_config` and explicit connection public-key policy. A default connection refuses an unknown certificate. An explicit first-connect collector records SHA256 of SubjectPublicKeyInfo, or an expected SPKI fingerprint must match before promoting the transport to usable TLS.
- Pairing certificate and connection certificate are separate. AOSP pairing_server_new creates a fresh RSA key in pairing_connection/pairing_server.cpp; adbd_tls_handshake creates its own process key in daemon/auth.cpp. Pinning one as the other would reject legitimate devices. Our connection trust is explicit trust-on-first-use, not identity authenticated by the pairing exchange.
- Pairing mock integration test now explicitly opts into first-connection trust.

AOSP transport.cpp regenerates X509 certificates for each connection using the daemon process RSA key. We hash parsed DER SubjectPublicKeyInfo using RustCrypto x509-cert, so certificate metadata changes do not change the pin. A daemon restart can change the actual public key. This fails closed; explicit reset/retrust is not implemented yet. DroidMux is an unpublished early project; local mock tests do not replace physical-device interoperability testing or a cryptographic audit.

Connection records are tagged `public_key_sha256`. Old whole-certificate string pins are rejected, not silently reinterpreted. Tests reissue certificates with the same key and verify equal pins, then change the key and verify unequal pins.
