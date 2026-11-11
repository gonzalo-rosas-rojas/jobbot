# 0013. The local panel's guards

Status: accepted

## Context

The panel is a web page served by jobbot on the candidate's machine. Any other page open in the
same browser can send requests to it (cross-site requests), and a hostile domain can point its name
at `127.0.0.1` (DNS rebinding). jobbot has one user and no accounts, so logins, sessions and tokens
from an identity provider would be machinery without a purpose.

## Decision

- It listens on `127.0.0.1` only.
- Every request must carry the exact `Host` of that socket, which defeats DNS rebinding.
- Every API call needs a token made at each launch and compared in constant time, and is refused
  when it carries a foreign `Origin`.
- A POST must be `application/json`. A hostile page cannot send that type across sites without a
  CORS preflight, and the server never answers one.
- Bodies have a size limit and are read only after the headers pass.
- The page is served with a nonce-only Content-Security-Policy with trusted types,
  `frame-ancestors 'none'` and `X-Frame-Options: DENY`; data reaches it only as JSON from the API.
- Every action goes through the same functions as the command line, and none starts discovery, a
  browser or Claude. A repeated `Idempotency-Key` gets the first reply
  ([0006](0006-intent-before-every-irreversible-act.md)).

## Consequences

- The panel cannot be opened from another machine, by design.
- A tool that talks to the API must read the launch token, as the page does.
- The JSON shape of the snapshot and the `/api/*` routes are a contract with the page, pinned by a
  key list that the tests check on both sides.
