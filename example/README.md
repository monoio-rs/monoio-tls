# TLS Demo

Use curl to verify tls handshake: `curl --resolve monoio.rs:50443:127.0.0.1 --cacert rootCA.crt -vvv https://monoio.rs:50443`