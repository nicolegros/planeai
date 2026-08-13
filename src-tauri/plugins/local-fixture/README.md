# Local plugin fixture

This fixture exercises PlaneAI’s local package contract: native JSON-RPC sidecar, prebuilt browser ESM docked page, and `fixture.status` bridge. Build and materialize the binary for the current machine before selecting this directory in Plugins:

```bash
make local-plugin-fixture
```

The generated `bin/` directory is intentionally ignored; it is a platform-specific test artifact, not source.
