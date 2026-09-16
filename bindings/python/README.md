# Actuate for Python

Inspect and control native apps through a context-managed session.

```python
from actuate import Actuate

with Actuate.connect() as session:
    print(session.capabilities())
    print(session.discover())
```

Python 3.10 or newer is required. The release source package uses a PEP 517
backend to download a compatible prebuilt wheel from the matching GitHub release.
It checks the wheel's SHA-256 and platform tags. It never compiles Rust at install
time or downloads code during import. Direct release wheels can also be installed
with uv.

Set `GH_TOKEN` for private release access or `ACTUATE_RELEASE_BASE_URL` for an
HTTPS release mirror. Source development uses `mise run release:wheel`.

`AsyncActuate.connect()` provides `async with` ownership. Cancelling a caller's
await does not undo native input. Closing the session waits for pending work.
Use `Actuate.from_transport()` for a custom transport and ordered middleware.

See the [Python guide](https://rajaniraiyn.dev/actuate/build/python/).
