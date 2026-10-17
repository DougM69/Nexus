-- Paid access, and one trial per callsign and per shack PC. Every table here is new, so nothing that
-- reads the others sees it; an account with no row in any of them is exactly what it was before.
--
-- `subscriptions` is the account's plan as the payment provider last described it. The relay only
-- reads it. It is written only by the provider's webhook, served by a separate billing Worker bound to
-- this database, and only from a fresh read of the subscription itself, never from an event's own
-- copy: events arrive out of order. `stripe_updated` is when that read was made (ms), and
-- an older read never overwrites a newer one. It is NOT the event's `created`, which is in seconds and
-- shared by distinct events. `period_end` is in ms. `first_refund_at` records a refund of the account's
-- first paid invoice: one first refund per person, checked by a person, as refunds are.
CREATE TABLE subscriptions (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id),
  customer_id TEXT UNIQUE,
  subscription_id TEXT UNIQUE,
  status TEXT NOT NULL CHECK (status IN ('incomplete','incomplete_expired','trialing','active','past_due','unpaid','canceled','paused')),
  plan TEXT CHECK (plan IN ('nexus_remote_monthly','nexus_remote_annual')),
  period_end INTEGER,
  cancel_at_period_end INTEGER NOT NULL DEFAULT 0,
  first_refund_at INTEGER,
  stripe_updated INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- Each provider event once: a redelivered event finds its id here and does nothing.
CREATE TABLE billing_events (id TEXT PRIMARY KEY, type TEXT NOT NULL, received_at INTEGER NOT NULL);

-- Each checkout session this server opened, and the signed-in account it was opened for. A completed
-- checkout binds a subscription only through a row here, and only to that account: the route that
-- creates the session records it here and puts the same account id in both `client_reference_id` and
-- `metadata.account_id`, and the webhook requires all three to agree.
CREATE TABLE checkout_sessions (id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id), created_at INTEGER NOT NULL);

-- A billing event refused rather than applied, for support: a checkout this server did not open, or one
-- that would replace another customer or a live subscription, or bind what another account holds.
-- Ids only, never a name or address.
CREATE TABLE billing_refusals (event_id TEXT PRIMARY KEY, type TEXT NOT NULL, reason TEXT NOT NULL, account_id TEXT,
  customer_id TEXT, subscription_id TEXT, at INTEGER NOT NULL);

-- What a started trial spent, kept as keyed hashes only: the station's callsign (its base call) and
-- the shack PC's two identifiers, each HMAC-SHA256 under the TRIAL_KEY secret. Callsigns are public,
-- so an unkeyed hash would be reversible against the call list. The key can never be rotated: the
-- inputs are not kept, so a new key matches none of these rows.
--
-- One row per account and value, so a callsign claimed by two accounts is two rows: `proven` is 1 when
-- the account's verified mailbox matched the licence's email on file, or support granted it, and a
-- proven claim is never blocked by an unproven one. `account_id` is not a foreign key: a marker
-- outlives the account it was spent by, or deleting an account would hand out another trial.
CREATE TABLE trial_markers (
  kind TEXT NOT NULL CHECK (kind IN ('call','pc-hw','pc-os')),
  value_hmac TEXT NOT NULL,
  account_id TEXT NOT NULL,
  proven INTEGER NOT NULL DEFAULT 0,
  method TEXT CHECK (method IN ('uls-email','admin')),
  created_at INTEGER NOT NULL,
  first_refund_at INTEGER,
  PRIMARY KEY (kind, value_hmac, account_id)
);
CREATE INDEX trial_markers_value ON trial_markers(kind, value_hmac);
CREATE INDEX trial_markers_account ON trial_markers(account_id);

-- The US amateur licences, from the FCC's public ULS files, keyed the same way: the base call's HMAC,
-- whether any licence for it is active, and the HMAC of the licence email's mailbox hash (the
-- accounts.mailbox_hash form), so a verified sign-in address can prove the call without either side
-- holding an address. `uls_sync` says when the copy was last refreshed; a copy older than three days
-- refuses US trials rather than waving them through.
CREATE TABLE uls_calls (call_hmac TEXT PRIMARY KEY, email_hmac TEXT, active INTEGER NOT NULL, refreshed_at INTEGER NOT NULL);
CREATE TABLE uls_sync (id INTEGER PRIMARY KEY CHECK (id = 1), last_daily TEXT, last_full TEXT, synced_at INTEGER NOT NULL);

-- A browser ticket's scope. 'lan' is a LAN-only session: what an account whose internet use has ended
-- still has, on the shack's own network (authority.ts lanEntitlement). The room takes the scope from the
-- ticket that admitted the session and holds its stream to the LAN (stream-relay.ts). Every ticket
-- before this column is an internet one, which is what each of them was.
ALTER TABLE tickets ADD COLUMN scope TEXT NOT NULL DEFAULT 'internet' CHECK (scope IN ('internet','lan'));
