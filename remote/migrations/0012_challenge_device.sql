-- A challenge asked by a device reopening its station, with no account token, is that device's alone:
-- only a request naming the same device may take it, and a signed-in request never may, nor a
-- reopening one take a signed-in caller's (NULL here). Kept to a few a device, newest first, so asking
-- stores nothing without bound and never touches what the account's own requests need.
ALTER TABLE challenges ADD COLUMN device_id TEXT;
CREATE INDEX challenges_device ON challenges(device_id);
