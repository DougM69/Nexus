-- Approvals last until removed (operator decision 2026-10-10), and the ones given under the thirty/
-- ninety-day rule become so at once (operator decision 2026-10-14), not only at the station's next
-- connect or listing or the device's next use (`keepApprovals`), so none lapses before then.
--
-- `keepApprovals` for every station that still has access, at once, and nothing more. An approval
-- that still stands (approved, not yet ended) and that a generation-binding Nexus gave (`approved_at`
-- recorded) is given the until-removed end, the last millisecond of the year 9999. It NEVER drops or
-- deletes a row, never
-- moves a generation (so every grant a station remembers stays bound), and never revives anything: a
-- device removed at the shack or forgetting itself (approved=0), an approval that has already ended,
-- one a station is still waiting on, and every device of a station whose access was revoked
-- (enabled=0) are left exactly as they are. An approval a 1.12-era Nexus gave (no approval time) keeps
-- its fixed end, because that Nexus binds its restored grants to the expiry.
--
-- Idempotent: a second run finds nothing below the until-removed end, and changes nothing.
UPDATE devices SET expires_at = 253402300799999
WHERE approved = 1
  AND approved_at IS NOT NULL
  AND expires_at > CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
  AND expires_at < 253402300799999
  AND station_id IN (SELECT id FROM stations WHERE enabled = 1);
