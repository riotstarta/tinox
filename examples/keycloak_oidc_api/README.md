# Keycloak-protected REST endpoint

A minimal REST API with one endpoint, `GET /api/hello`, that only answers
if the caller's OIDC access token is valid **and** contains a specific
realm role. Every part of the IdP configuration (issuer, JWKS endpoint,
expected audience, required role) comes from environment variables, so
the same binary works against any OIDC provider without a rebuild — this
example ships a `docker-compose.yml` that runs a real, pre-configured
Keycloak so you can try it end-to-end.

Token verification is fully automatic: the app fetches Keycloak's JWKS
document live (matching the token's `kid`), verifies the RS256 signature,
and checks `iss`/`aud`/`exp`/`nbf` and the required role — no key material
or role list is ever hardcoded.

## 1. Start Keycloak

```sh
cd examples/keycloak_oidc_api
docker compose up -d
```

This imports a realm `tinox-demo` (`keycloak/realm-export.json`) with:

- a public client `tinox-api` (direct-access-grants enabled, so `curl` can
  fetch a token directly — see below)
- a realm role `api-user`
- two users: `alice` / `alice-pw` (**has** `api-user`) and `bob` / `bob-pw`
  (does **not**)

Wait until it's ready (first boot takes a few seconds):

```sh
until curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/realms/tinox-demo | grep -q 200; do sleep 2; done
```

## 2. Run the app

```sh
export OIDC_ISSUER=http://localhost:8080/realms/tinox-demo
export OIDC_JWKS_URI=http://localhost:8080/realms/tinox-demo/protocol/openid-connect/certs
export OIDC_AUDIENCE=tinox-api
export OIDC_REQUIRED_ROLE=api-user
export PORT=8095   # pick a free port

tinox run src/main.tnx
```

## 3. Get a token and call the endpoint

```sh
# Alice has the required role -> 200
TOKEN=$(curl -s -X POST http://localhost:8080/realms/tinox-demo/protocol/openid-connect/token \
  -d grant_type=password -d client_id=tinox-api -d username=alice -d password=alice-pw \
  | jq -r .access_token)
curl -i http://localhost:8095/api/hello -H "Authorization: Bearer $TOKEN"
# HTTP/1.1 200 OK
# {"message":"Hello, you are authorized!","requiredRole":"api-user"}

# Bob is a valid user but lacks the role -> 403
TOKEN=$(curl -s -X POST http://localhost:8080/realms/tinox-demo/protocol/openid-connect/token \
  -d grant_type=password -d client_id=tinox-api -d username=bob -d password=bob-pw \
  | jq -r .access_token)
curl -i http://localhost:8095/api/hello -H "Authorization: Bearer $TOKEN"
# HTTP/1.1 403 Forbidden
# {"error":"missing required role: api-user"}

# No token at all -> 401
curl -i http://localhost:8095/api/hello
# HTTP/1.1 401 Unauthorized
# {"error":"missing bearer token"}
```

(No `jq`? Swap the `| jq -r .access_token` for
`| python3 -c "import json,sys;print(json.load(sys.stdin)['access_token'])"`.)

## Environment variables

| Variable             | Meaning                                              | Example (this compose file)                                              |
|-----------------------|-------------------------------------------------------|----------------------------------------------------------------------------|
| `OIDC_ISSUER`         | Expected `iss` claim                                  | `http://localhost:8080/realms/tinox-demo`                                  |
| `OIDC_JWKS_URI`       | Where to fetch the IdP's public keys                  | `http://localhost:8080/realms/tinox-demo/protocol/openid-connect/certs`    |
| `OIDC_AUDIENCE`       | Expected `aud` claim (this app's client ID)           | `tinox-api`                                                                |
| `OIDC_REQUIRED_ROLE`  | Realm role (`realm_access.roles`) required to pass    | `api-user`                                                                 |
| `PORT`                | Port the app listens on (optional, defaults to 8090)  | `8095`                                                                     |

## Cleanup

```sh
docker compose down -v
```

## Notes

- This app validates a Bearer token presented by *some other* client — a
  resource server, not the OIDC login flow itself. For "login with X"
  (the Authorization Code + PKCE flow that obtains this token in the
  first place), see `tinox.core.oidc`'s `OidcClient` in the stdlib docs.
- `src/main.tnx` inlines the RS256/JWKS verification logic instead of
  using `tinox.core.jwt`'s `Jwt`/`Jwks` classes directly, to work around
  a known compiler limitation (tracked as issue #139: two differently-
  scoped stdlib classes named `HttpResponse` collide if both
  `tinox.core.http` and `tinox.core.http_server` end up imported into the
  same program). See the comments in `src/main.tnx` and
  `src/IdpHttpResponse.tnx` for details.
