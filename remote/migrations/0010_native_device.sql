-- The native client: Nexus on another computer (and later a phone app) driving a station over the
-- internet. It registers as a device of that station, as a browser does, and waits for the same
-- approval at the shack. Two additive, nullable columns say which kind of device a row is and on
-- which platform: `kind` is 'computer' or 'phone', `platform` one of windows, macos, linux, ios and
-- android, and the service checks both. NULL is a browser - every row written before this, and every
-- browser row after it, since the browser paths never fill them in.
--
-- A native device's credential is its key, not a cookie: each of its requests is signed by the key
-- the shack pins at approval. Its `credential_hash` is the digest of a random value nobody holds, so
-- no cookie can ever match it.
ALTER TABLE devices ADD COLUMN kind TEXT;
ALTER TABLE devices ADD COLUMN platform TEXT;

-- One-use challenges for those signatures. A signed request names a challenge the service gave the
-- same account a moment before, and the challenge is deleted as the request is read, so a signed
-- request is taken once. Stored as SHA-256 digests, as tickets are, for a minute at most, and swept
-- as they expire. A new table, so nothing that reads the others sees it.
CREATE TABLE challenges (
  digest TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id), expires_at INTEGER NOT NULL
);
CREATE INDEX challenges_expiry ON challenges(expires_at);
