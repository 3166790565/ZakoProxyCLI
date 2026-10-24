These certificate and private-key fixtures are public test data for the local
HTTPS provider mock in `tests/providers.rs`. The private key has no production
use and must never be used by a deployed service.

The certificate is valid only for `www.zayuapi.com`. Tests map that hostname to
an ephemeral loopback socket using a test-only HTTP client and explicitly trust
this certificate; they do not contact the real service or disable TLS checks.
This lets the test keep the exact canonical provider URL while exercising the
production validation, login, persistence, refresh and key-import paths.

To regenerate with OpenSSL (write the temporary PEM files outside the repository):

```sh
openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days 36500 \
  -keyout test-key.pem -out test-cert.pem -subj "/CN=www.zayuapi.com" \
  -addext "subjectAltName=DNS:www.zayuapi.com" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature,keyEncipherment" \
  -addext "extendedKeyUsage=serverAuth"
openssl x509 -in test-cert.pem -outform DER -out cert.der
openssl pkcs8 -topk8 -nocrypt -in test-key.pem -outform DER -out key.der
```
