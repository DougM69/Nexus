//! A bounded, wait-free single-producer / single-consumer ring of samples, built from atomics
//! only: what a realtime audio callback shares with the rest of Nexus, so that it never takes a
//! lock, allocates or waits. The headset's rings here are three of them; the station's monitor and
//! capture in tempo-audio use it the same way.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// A bounded, wait-free SPSC ring of `f32` samples.
///
/// One producer (the capture callback) and one consumer (the monitor output
/// callback) only. Each slot is an `AtomicU32` holding the sample's bit pattern,
/// so the whole type is safe Rust — no `UnsafeCell`, no `unsafe`. Capacity is
/// rounded up to a power of two for index masking; `head`/`tail` are monotonic
/// counters (indices are `counter & mask`).
///
/// On overflow the *incoming* sample is dropped (the producer cannot safely retire
/// the oldest — `tail` is consumer-owned), so drops are consistently newest-first.
pub struct SpscRing {
    slots: Box<[AtomicU32]>,
    mask: usize,
    /// Total samples pushed (producer-owned, published with Release).
    head: AtomicUsize,
    /// Total samples popped (consumer-owned, published with Release).
    tail: AtomicUsize,
    /// The `head` the producer asked the queue to be emptied UP TO, and nothing else moves
    /// `tail` — see [`SpscRing::request_flush`].
    flush_to: AtomicUsize,
}

impl SpscRing {
    /// A ring holding at least `min_capacity` samples (rounded up to a power of two,
    /// minimum 2).
    pub fn new(min_capacity: usize) -> Self {
        let cap = min_capacity.next_power_of_two().max(2);
        let slots = (0..cap)
            .map(|_| AtomicU32::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            mask: cap - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            flush_to: AtomicUsize::new(0),
        }
    }

    /// Total slot capacity (a power of two).
    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// Samples currently queued.
    pub fn len(&self) -> usize {
        self.head
            .load(Ordering::Acquire)
            .wrapping_sub(self.tail.load(Ordering::Acquire))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Producer: push one sample. Returns `false` (dropping the sample) when full —
    /// NEVER blocks or allocates. Safe to call from the real-time capture callback.
    pub fn push(&self, sample: f32) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head.wrapping_sub(tail) >= self.capacity() {
            return false; // full → drop (monitor glitches; the decoder never does)
        }
        self.slots[head & self.mask].store(sample.to_bits(), Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        true
    }

    /// Producer: push a block, returning how many were accepted (the remainder is
    /// dropped once the ring is full).
    pub fn push_slice(&self, samples: &[f32]) -> usize {
        let mut n = 0;
        for &s in samples {
            if !self.push(s) {
                break;
            }
            n += 1;
        }
        n
    }

    /// Consumer: pop the oldest queued sample, or `None` when empty.
    pub fn pop(&self) -> Option<f32> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail == head {
            return None;
        }
        let bits = self.slots[tail & self.mask].load(Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        Some(f32::from_bits(bits))
    }

    /// Discard every queued sample (called when the monitor stops, so a later
    /// re-enable starts on fresh audio). Only the consumer moves `tail`.
    ///
    /// ⚠️ CONSUMER-SIDE, and the type cannot check it. `tail` is consumer-owned; a PRODUCER that
    /// called this would race the consumer's `pop` — read `tail`, store `head`, and lose the race
    /// to a `pop` that stores `tail + 1` — leaving samples the clear was supposed to discard. On
    /// the TX path that is a Stop TX that stops only some of the audio, so the producer asks with
    /// [`Self::request_flush`] instead. A producer may call this directly only when it can prove
    /// no consumer exists (the TX output stream not yet built — see `device.rs`).
    pub fn clear(&self) {
        let head = self.head.load(Ordering::Acquire);
        self.tail.store(head, Ordering::Release);
    }

    /// Producer: ask the consumer to discard everything queued **as of now**, returning how many
    /// samples that was. The drop itself happens in the consumer's next [`Self::apply_flush`].
    ///
    /// Recording the HEAD (rather than a bare "please clear" flag) is what makes a flush
    /// immediately followed by fresh audio safe: Stop TX and the tune carrier's re-arm both do
    /// exactly that, and a flag would have let the consumer's clear land after the new push and
    /// silently eat the audio that was queued to replace it. Everything at or past this head
    /// survives.
    pub fn request_flush(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        self.flush_to.store(head, Ordering::Release);
        head.wrapping_sub(tail)
    }

    /// Consumer: honour a pending [`Self::request_flush`]. Call once at the top of each callback —
    /// two atomic loads when there is nothing to do, which is every callback but the one after a
    /// Stop TX.
    pub fn apply_flush(&self) {
        let target = self.flush_to.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        // Signed compare so the wrapping counters are handled: only ever move `tail` FORWARD.
        if target.wrapping_sub(tail) as isize > 0 {
            self.tail.store(target, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_fifo_order() {
        let r = SpscRing::new(8);
        assert!(r.is_empty());
        for i in 0..5 {
            assert!(r.push(i as f32));
        }
        assert_eq!(r.len(), 5);
        for i in 0..5 {
            assert_eq!(r.pop(), Some(i as f32));
        }
        assert!(r.pop().is_none(), "empty ring yields None, never blocks");
        assert!(r.is_empty());
    }

    #[test]
    fn overflow_drops_incoming_and_never_blocks() {
        // Capacity rounds up to a power of two (4). Fill it, then the next pushes
        // are dropped (return false) rather than blocking or overwriting the queue.
        let r = SpscRing::new(3);
        assert_eq!(r.capacity(), 4);
        for i in 0..4 {
            assert!(r.push(i as f32), "slot {i} accepted");
        }
        assert!(!r.push(99.0), "full ring drops the incoming sample");
        assert!(!r.push(100.0), "still full → still dropped");
        assert_eq!(r.len(), 4, "queue unchanged by the dropped pushes");
        // The retained samples are the FIRST four (newest dropped, oldest kept).
        for i in 0..4 {
            assert_eq!(r.pop(), Some(i as f32));
        }
    }

    #[test]
    fn push_slice_reports_accepted_count() {
        let r = SpscRing::new(4); // capacity 4
        let block = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        assert_eq!(r.push_slice(&block), 4, "only what fits is accepted");
        assert_eq!(r.pop(), Some(1.0));
    }

    #[test]
    fn clear_empties_the_ring() {
        let r = SpscRing::new(8);
        r.push_slice(&[1.0, 2.0, 3.0]);
        r.clear();
        assert!(r.is_empty());
        assert!(r.pop().is_none());
        // Usable again after a clear.
        assert!(r.push(7.0));
        assert_eq!(r.pop(), Some(7.0));
    }

    /// THE STOP-TX SHAPE (#172). The producer is the radio loop and the consumer is a realtime
    /// callback, so the producer may not move `tail` itself — it asks, and the consumer drops the
    /// queue at the top of its next block.
    #[test]
    fn a_producer_requested_flush_is_performed_by_the_consumer() {
        let r = SpscRing::new(16);
        r.push_slice(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(r.request_flush(), 4, "reports what it discarded");
        // Not yet: the consumer has not run. (The positive control for the assertion below —
        // without it, a `request_flush` that cleared immediately would pass the same test.)
        assert_eq!(r.len(), 4, "the producer did not touch tail itself");
        r.apply_flush();
        assert!(r.is_empty(), "the consumer dropped the queue");
        assert!(r.pop().is_none());
    }

    /// ⭐ AND AUDIO QUEUED AFTER THE ASK SURVIVES. Stop TX and the tune carrier's re-arm both
    /// flush and then immediately queue fresh audio; a bare "please clear" flag would have let the
    /// consumer's clear land after the new push and eat the replacement over.
    #[test]
    fn audio_queued_after_the_flush_request_is_not_swallowed() {
        let r = SpscRing::new(16);
        r.push_slice(&[1.0, 2.0, 3.0]);
        r.request_flush();
        r.push_slice(&[7.0, 8.0]); // the replacement, queued before the consumer ran
        r.apply_flush();
        assert_eq!(r.len(), 2, "only the pre-flush audio went");
        assert_eq!(r.pop(), Some(7.0));
        assert_eq!(r.pop(), Some(8.0));
    }

    /// Idempotent and cheap: an `apply_flush` with nothing pending must never move `tail`
    /// backwards or re-drop audio, because it runs at the top of EVERY callback.
    #[test]
    fn applying_a_flush_that_was_never_requested_does_nothing() {
        let r = SpscRing::new(16);
        r.push_slice(&[1.0, 2.0]);
        r.apply_flush();
        r.apply_flush();
        assert_eq!(r.len(), 2);
        assert_eq!(r.pop(), Some(1.0));
        r.apply_flush(); // stale request point is behind tail now — must not resurrect anything
        assert_eq!(r.pop(), Some(2.0));
        assert!(r.pop().is_none());
    }

    #[test]
    fn head_tail_survive_wraparound() {
        // Cycle far more than capacity to exercise index wrap; FIFO must hold.
        let r = SpscRing::new(4);
        for i in 0..1000 {
            assert!(r.push(i as f32));
            assert_eq!(r.pop(), Some(i as f32));
            assert!(r.is_empty());
        }
    }
}
