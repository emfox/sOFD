# Running the server

`sofd-server` is the reader as a service: it opens a document, draws its pages,
searches it and checks its signatures over HTTP. The desktop reader needs a
display; this does not, which is what a deployment wants.

Every release publishes the image to the GitHub container registry, tagged with
the version and with `latest`:

```sh
docker pull ghcr.io/emfox/sofd-server:0.7.0
docker run -d -p 8080:8080 -v sofd-data:/var/lib/sofd ghcr.io/emfox/sofd-server:0.7.0
```

A release tag names a version, so pinning one is what a deployment that must
not change under it should do. `latest` is there for a trial.

The image is built for `linux/amd64`. It runs as uid 10001, holds documents in
`/var/lib/sofd` so they survive a restart, and carries one font family
(WenQuanYi Zen Hei) because a document need not embed its own.

## Settings

All of them are read from the environment, and all have a default that is safe
to start with.

| Variable | Default | What it decides |
| --- | --- | --- |
| `SOFD_BIND` | `127.0.0.1:8080` in the binary, `0.0.0.0:8080` in the image | Where to listen |
| `SOFD_DATA_DIR` | unset, so documents are memory only; `/var/lib/sofd` in the image | Where documents are kept |
| `SOFD_API_KEYS` | unset, so every caller is one tenant | `tenant:key,tenant:key` |
| `SOFD_MAX_BODY` | 64 MiB | Largest upload |
| `SOFD_MAX_DOCUMENTS` | 256 | How many stay open |
| `SOFD_RATE_LIMIT` | generous | `rate,burst` per caller |
| `SOFD_DRAIN_TIMEOUT` | generous | Seconds in-flight work gets after a stop |

The binary's own default binds loopback, because a trial run should not be a
service. The image binds every interface, because inside a container loopback
reaches nothing.

## Checking it

`GET /healthz` answers `{"status":"ok","documents":<n>,"version":"<v>"}`, and
the image's `HEALTHCHECK` asks exactly that through `sofd-server
--health-check`, which exits non-zero unless the route answers 200.
`GET /openapi.json` describes the rest of the API.

An image that answers `/healthz` can still draw a blank page: a missing font is
not a missing process. The release job therefore opens `compliance/fixtures/minimal.ofd`,
counts the ink in the result, and fails below 200 pixels — the page draws about
17,700 here and nothing on it would count zero. The fixture is the one the tree
carries: a fetched sample is absent on CI, where the count would be of nothing.
