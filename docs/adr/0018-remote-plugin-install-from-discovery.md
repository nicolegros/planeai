# ADR-0018: Install plugins from Discovery

## Status

Accepted - amends ADR-0011, whose v1 excluded remote URL installation

## Context

ADR-0011 made plugin installation a trusted local-code decision: the user selects a package directory, PlaneAI validates it and copies it into content-addressed storage.
Discovery (Preferences > Plugins) already lists public GitHub repositories tagged `planeai`, but a card only linked to the repository.
Installing one meant cloning or downloading by hand, building or locating the platform archive, extracting it and selecting the directory.
Updates were invisible: nothing compared an installed version to what the repository published.

A download from a repository is still third-party native code running with the user's permissions.
What changes is who performs the mechanical steps, not who decides to trust the package.

## Decision

- **Source.** For a repository `owner/name` and the host platform key, the archive comes first from the `downloads[<platform>]` entry of `planeai-plugin.json` on the default branch, read through `gh api` with the raw accept header and parsed leniently (`id` and `downloads` only), because HEAD may already target a newer host.
  Its HEAD `version` is never trusted.
  When that manifest is missing, unparseable or has no entry for the platform, the latest non-draft, non-prerelease release is used, with the asset named `<name>-<platform>.tar.gz` or `<name>-<platform>.zip`.
  Otherwise the card is unavailable and says why.
  `downloads` is an optional manifest field validated by the contract crate: known platform keys and `https` URLs only, so `planeai-cli plugin test` and installation agree.
- **Trust gate.** Install is two steps.
  Staging downloads, verifies, extracts and validates the package under `<app data>/plugins/staging/<uuid>/` and returns a preview; nothing is recorded.
  The user reads the review (manifest facts, source URL, release tag, SHA-256 and whether it was verified, capabilities, UI placements, MCP tools, what it replaces, warnings) and chooses "Install and enable" or cancels.
  Commit runs the existing local import on the staged directory, so one code path validates, copies and records every local package, then enables the plugin; an enable failure leaves it installed in the error state with the error shown.
  Discard and any other way of closing the review remove the staging directory, an unknown staging id discards as a no-op, and startup wipes the staging root, so a crash mid-stage leaves nothing behind.
- **Transport.** `reqwest` with rustls and the platform trust store, no default features, streaming to disk with a 100 MB cap and a 60 second timeout.
  Only `https` URLs are fetched and redirects to anything else fail.
  `gh` stays the metadata client, so Discovery and installs share its authentication and rate limits.
  Extraction (`tar` + `flate2`, `zip` with deflate only) rejects absolute paths, `..` components, symbolic and hard links, stops once the archive decompresses past 500 MB in total (tar headers included, since the tar reader buffers long-name headers before yielding an entry), and requires exactly one top-level directory holding `planeai-plugin.json`.
- **Integrity.** When the archive is an asset of the latest release (the fallback path, or a `downloads` URL of the form `https://github.com/{owner}/{name}/releases/latest/download/{asset}` or `.../releases/download/{latest tag}/{asset}`), its SHA-256 must equal the `digest` GitHub reports for that asset, and a mismatch fails closed before the review.
  Any other URL installs as unverified and the review shows the computed hash.
  Signing remains out of scope.
- **Provenance.** The inventory records the repository, the release tag when the archive was a release asset, the source URL and the archive SHA-256 as `remote_origin`.
  A local package has exactly one provenance, a folder or a remote origin, and installing the same id again replaces it whole, so a later folder install clears the remote origin.
  Bundled plugins have none.
- **Card state.** Right after discovery returns, the frontend asks the backend for every card's state at once.
  Each resolution runs two `gh` processes side by side (the HEAD manifest and the latest release) and holds two permits of an 8-permit semaphore, so at most 8 `gh` processes run at once.
  A card matches an installed plugin by provenance repository first, then by the HEAD manifest `id`.
  Latest is the release tag without a leading `v`, parsed as semver; an update is offered when it is greater than the installed manifest version, also semver.
  A repository-installed plugin whose versions do not both parse, or whose repository has no release, is offered "Reinstall latest".
  A plugin installed from a folder is shown as installed locally with "Replace", never as a plain install.
  A plugin with the same id installed from a different repository is shown with that repository and "Replace", and its review says what it replaces, so another author's package never reads as an update.
  A bundled id is unavailable.
  The review also warns when the staged manifest version differs from the release tag the archive came from.
- **Scope.** Update awareness lives in Discovery only: no polling, no background checks, no automatic updates.
  The flow is app UI only; there is no MCP tool and no CLI command for it.
  Commands take a repository name or a staging id, never a URL from the frontend, and the backend re-resolves the source itself.

## Consequences

- A user installs or updates a community plugin from its card after reading one review, and the inventory shows where it came from.
- The host gains an HTTP client and archive readers as pinned dependencies; the download path is covered by tests against a local server, and `cargo test -- --ignored remote_install_real_catalog` stages a real repository end to end.
- Authors publish `<name>-<platform>.tar.gz` or `.zip` release assets with a single top-level directory, or point `downloads` at their own `https` host and accept that those installs show as unverified.
- Resolution runs two `gh` processes per card and four cards at a time, so a large catalog takes several seconds to settle while cards show "Checking".

## Rejected alternatives

- **Install on click without a review.** The download replaces manual steps, not the trust decision ADR-0011 placed with the user.
- **Trusting the HEAD manifest version.** Default branches commonly carry `0.0.0` or the next version; the release tag and the archive's own manifest are the facts.
- **Talking to the GitHub API with reqwest.** It would duplicate `gh`'s authentication and lose its rate limits for the metadata calls Discovery already makes.
- **Polling for updates or updating automatically.** Each is a background-execution policy for third-party native code and deserves its own decision.
- **Allowing arbitrary `http` downloads or redirect targets.** A plain-text hop lets a network attacker swap the native code the user is about to run.
