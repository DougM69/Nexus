-- Sign in once, no pairing: a station bound to its account by the shack's own sign-in, and a device
-- approved because its sign-in proves it is the owner's or an invited person's. Additive only (new
-- nullable columns, new tables), so it is safe in the window where it runs before the Worker that
-- reads it, and every row written before it reads as it did: NULL `role` is Operate, NULL `added_by`
-- is "added at the station".
--
-- A PROOF is an ID token the identity provider signed, whose nonce is the hash of the keys it vouches
-- for (index.ts, `stationNonce` / `deviceNonce`). The service stores and forwards it; the shack checks
-- it itself, so the service can list a device but cannot mint the proof that admits one.

-- The shack's LAN key (P-256 SPKI, lowercase hex), listed to the account's devices with the station
-- proof that vouches for it, so neither the service nor a machine on the LAN can stand in for the shack.
ALTER TABLE stations ADD COLUMN lan_key TEXT;
-- The station proof. NULL for a station paired by code until its operator confirms the account at the
-- station: no device is approved by sign-in for it before then.
ALTER TABLE stations ADD COLUMN proof TEXT;
-- What a trial started at the station's first internet use will spend: the HMACs of its callsign and PC
-- identifiers as the shack sent them at sign-in, never the values (trial-identity.ts). NULL for a
-- station paired by code, whose trial started at approval.
ALTER TABLE stations ADD COLUMN trial_claim TEXT;

ALTER TABLE devices ADD COLUMN role TEXT CHECK (role IN ('operate','listen'));
ALTER TABLE devices ADD COLUMN added_by TEXT CHECK (added_by IN ('shack','account'));
-- The device proof, kept while the device is approved so a shack that restarts can check it again,
-- and cleared when the device is removed.
ALTER TABLE devices ADD COLUMN proof TEXT;
-- A computer's LAN key beside its road key (`public_key`): the device proof's nonce covers both, so
-- the shack needs both to check it.
ALTER TABLE devices ADD COLUMN lan_key TEXT;
-- When a device was removed on the web, and when the station first read its list after that: until
-- it has, the web says the station has not heard of it yet. Both NULL for a device removed at the
-- station, which heard of it by doing it.
ALTER TABLE devices ADD COLUMN revoked_at INTEGER;
ALTER TABLE devices ADD COLUMN revoke_seen_at INTEGER;

-- Where the owner's security notices go. Created here so the scheme lands in one migration; nothing
-- writes it until the notice mail exists.
ALTER TABLE accounts ADD COLUMN notice_address TEXT;

-- A person invited to a station by the mailbox their verified address delivers to (authority.ts,
-- `mailbox`), kept only as its hash. Removing one keeps the row (`removed_at`), so inviting the same
-- person again is the same row.
CREATE TABLE invites (
  id TEXT PRIMARY KEY, station_id TEXT NOT NULL REFERENCES stations(id), mailbox_hash TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('operate','listen')), created_at INTEGER NOT NULL, removed_at INTEGER,
  UNIQUE(station_id, mailbox_hash)
);
CREATE INDEX invites_mailbox ON invites(mailbox_hash);

-- The new-device notices sent, at most one a device. Nothing writes it until the notice mail exists.
CREATE TABLE notices (
  id TEXT PRIMARY KEY, account_id TEXT NOT NULL, station_id TEXT NOT NULL, device_id TEXT NOT NULL,
  sent_at INTEGER, created_at INTEGER NOT NULL
);
